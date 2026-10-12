use super::*;
use crate::synthetic;
use glam::dvec3;

#[test]
fn empty_mesh_yields_no_candidates() {
    let bvh = MeshBvh::new(&TriMesh::default());
    let mut out = vec![7, 7, 7];
    bvh.candidates_in_column(0.0, 0.0, 1.0, &mut out);
    assert!(out.is_empty(), "buffer should be cleared and left empty");
}

#[test]
fn finds_the_triangle_under_the_tool() {
    let mesh = synthetic::plane(2.0, 10.0);
    let bvh = MeshBvh::new(&mesh);
    let mut out = Vec::new();
    bvh.candidates_in_column(0.0, 0.0, 1.0, &mut out);
    assert!(!out.is_empty(), "a plane under the tool must be found");
    assert!(out.iter().all(|i| (*i as usize) < mesh.len()));
}

#[test]
fn misses_geometry_far_away() {
    let mesh = synthetic::plane(0.0, 1.0);
    let bvh = MeshBvh::new(&mesh);
    let mut out = Vec::new();
    bvh.candidates_in_column(500.0, 500.0, 0.5, &mut out);
    assert!(out.is_empty());
}

/// The index must be conservative: anything the exact test would accept has to
/// appear as a candidate first. A narrow index silently drops contacts.
#[test]
fn is_conservative_against_brute_force() {
    let mesh = synthetic::uv_sphere(5.0, 12, 16);
    let bvh = MeshBvh::new(&mesh);
    let mut out = Vec::new();
    let r = 1.5;

    for gx in -6..=6 {
        for gy in -6..=6 {
            let (x, y) = (f64::from(gx) * 0.9, f64::from(gy) * 0.9);
            bvh.candidates_in_column(x, y, r, &mut out);

            for i in 0..mesh.len() {
                let t = mesh.triangle(i).unwrap();
                let lo = t[0].min(t[1]).min(t[2]);
                let hi = t[0].max(t[1]).max(t[2]);
                let overlaps = lo.x <= x + r && hi.x >= x - r && lo.y <= y + r && hi.y >= y - r;
                if overlaps {
                    assert!(
                        out.iter().any(|c| *c as usize == i),
                        "triangle {i} overlaps the column at ({x}, {y}) but was not a candidate"
                    );
                }
            }
        }
    }
}

#[test]
fn rejects_non_finite_queries_without_panicking() {
    let bvh = MeshBvh::new(&synthetic::plane(0.0, 1.0));
    let mut out = Vec::new();
    for bad in [f64::NAN, f64::INFINITY] {
        bvh.candidates_in_column(bad, 0.0, 1.0, &mut out);
        assert!(out.is_empty());
        bvh.candidates_in_column(0.0, 0.0, bad, &mut out);
        assert!(out.is_empty());
    }
}

fn brute_ray(mesh: &TriMesh, o: DVec3, d: DVec3) -> Option<f64> {
    (0..mesh.len())
        .filter_map(|i| ray_triangle(o, d, mesh.triangle(i).unwrap()))
        .min_by(f64::total_cmp)
}

/// Rays in every direction through a sphere: the index finds exactly the hit a
/// search of every triangle finds.
#[test]
fn ray_casts_agree_with_brute_force() {
    let mesh = synthetic::uv_sphere(5.0, 12, 18);
    let bvh = MeshBvh::new(&mesh);
    for gx in -4..=4 {
        for gy in -4..=4 {
            for (o, d) in [
                (
                    dvec3(f64::from(gx) * 1.4, f64::from(gy) * 1.4, 20.0),
                    dvec3(0.0, 0.0, -1.0),
                ),
                (
                    dvec3(-20.0, f64::from(gx), f64::from(gy)),
                    dvec3(2.0, 0.1, 0.05),
                ),
                (
                    dvec3(0.0, 0.0, 0.0),
                    dvec3(f64::from(gx), f64::from(gy), 1.0),
                ),
            ] {
                let fast = bvh.cast_ray(&mesh, o, d, f64::INFINITY).map(|h| h.t);
                let slow = brute_ray(&mesh, o, d);
                assert_eq!(fast.is_some(), slow.is_some(), "at {o} {d}");
                if let (Some(a), Some(b)) = (fast, slow) {
                    assert!((a - b).abs() < 1e-9, "{a} vs {b} at {o} {d}");
                }
            }
        }
    }
    // Too short to reach, and aimed away.
    assert!(
        bvh.cast_ray(&mesh, dvec3(0.0, 0.0, 20.0), dvec3(0.0, 0.0, -1.0), 10.0)
            .is_none()
    );
    assert!(
        bvh.cast_ray(
            &mesh,
            dvec3(0.0, 0.0, 20.0),
            dvec3(0.0, 0.0, 1.0),
            f64::INFINITY
        )
        .is_none()
    );
    assert!(
        bvh.cast_ray(
            &mesh,
            dvec3(f64::NAN, 0.0, 20.0),
            dvec3(0.0, 0.0, -1.0),
            1.0
        )
        .is_none()
    );
    assert!(
        bvh.cast_ray(&mesh, dvec3(0.0, 0.0, 20.0), DVec3::ZERO, 1.0)
            .is_none()
    );
}

#[test]
fn nearest_triangles_agree_with_brute_force() {
    let mesh = synthetic::uv_sphere(5.0, 10, 14);
    let bvh = MeshBvh::new(&mesh);
    for p in [
        dvec3(0.0, 0.0, 9.0),
        dvec3(3.0, -2.0, 1.0),
        dvec3(0.1, 0.2, 0.3),
        dvec3(-7.0, 7.0, -7.0),
    ] {
        let (_, fast) = bvh.nearest(&mesh, p, f64::INFINITY).unwrap();
        let slow = (0..mesh.len())
            .map(|i| closest_point_on_triangle(p, mesh.triangle(i).unwrap()).distance(p))
            .min_by(f64::total_cmp)
            .unwrap();
        assert!((fast - slow).abs() < 1e-12, "{fast} vs {slow} at {p}");
    }
    assert!(bvh.nearest(&mesh, dvec3(0.0, 0.0, 9.0), 1.0).is_none());
    assert!(bvh.nearest(&mesh, dvec3(0.0, 0.0, 9.0), 4.0).is_some());
    assert!(bvh.nearest(&mesh, dvec3(f64::NAN, 0.0, 0.0), 1.0).is_none());
}

#[test]
fn handles_a_degenerate_single_point_mesh() {
    let mesh = TriMesh::new(vec![dvec3(0.0, 0.0, 0.0)], vec![[0, 0, 0]]).unwrap();
    let bvh = MeshBvh::new(&mesh);
    let mut out = Vec::new();
    bvh.candidates_in_column(0.0, 0.0, 1.0, &mut out);
    assert_eq!(out, vec![0]);
}
