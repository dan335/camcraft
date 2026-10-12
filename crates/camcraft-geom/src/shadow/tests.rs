use super::*;
use crate::synthetic;
use glam::dvec3;

/// A closed box as a mesh.
fn cuboid(min: DVec3, max: DVec3) -> TriMesh {
    let v = |x: bool, y: bool, z: bool| {
        dvec3(
            if x { max.x } else { min.x },
            if y { max.y } else { min.y },
            if z { max.z } else { min.z },
        )
    };
    let verts: Vec<DVec3> = (0..8)
        .map(|i| v(i & 1 != 0, i & 2 != 0, i & 4 != 0))
        .collect();
    let quads = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    let tris = quads
        .iter()
        .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
        .collect();
    TriMesh::new(verts, tris).unwrap()
}

#[test]
fn a_box_casts_its_own_footprint() {
    let m = cuboid(dvec3(-2.0, -1.0, 0.0), dvec3(3.0, 4.0, 5.0));
    let s = shadow_above(&m, 2.0).unwrap();
    // Exactly 5 x 5: the walls lie along its edge, and add nothing.
    assert!(
        (s.area().area() - 25.0).abs() < 1e-6,
        "got {}",
        s.area().area()
    );
    assert!(
        s.sheets().is_empty(),
        "walls of a closed box are not sheets"
    );
    assert!(
        shadow_above(&m, 5.0).unwrap().is_empty(),
        "nothing above the top"
    );
}

/// A floor is somewhere to stand. A face exactly on the level must not count.
#[test]
fn a_face_on_the_level_casts_nothing() {
    let floor = synthetic::plane(0.0, 10.0);
    assert!(shadow_above(&floor, 0.0).unwrap().is_empty());
    assert!(
        shadow_above(&floor, -2.0 * LEVEL_EPSILON)
            .unwrap()
            .area()
            .area()
            > 399.0
    );
}

/// Down-facing triangles wind the other way once projected. Unioned naively
/// they cancel the top, and a solid block casts no shadow at all.
#[test]
fn faces_seen_from_below_do_not_cancel() {
    let m = cuboid(dvec3(0.0, 0.0, 0.0), dvec3(4.0, 4.0, 4.0));
    let below = shadow_above(&m, -1.0).unwrap();
    assert!(
        (below.area().area() - 16.0).abs() < 1e-6,
        "got {}",
        below.area().area()
    );
}

/// A lone vertical sheet has no area from above, and is still an obstacle.
#[test]
fn a_vertical_sheet_still_casts_a_shadow() {
    let fin = TriMesh::new(
        vec![
            dvec3(0.0, 0.0, 0.0),
            dvec3(10.0, 0.0, 0.0),
            dvec3(10.0, 0.0, 5.0),
            dvec3(0.0, 0.0, 5.0),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
    )
    .unwrap();
    let s = shadow_above(&fin, 1.0).unwrap();
    assert!(!s.is_empty(), "the sheet vanished");
    assert!(s.area().is_empty(), "a sheet has no area");
    let length: f64 = s
        .sheets()
        .iter()
        .map(|l| l.windows(2).map(|w| w[0].distance(w[1])).sum::<f64>())
        .sum();
    assert!(length >= 10.0 - 1e-6, "sheet length {length}");
    // Grown by 1 with round ends: a 10 x 2 stadium.
    let grown = s.grown(1.0, 0.001).unwrap().area();
    let stadium = 20.0 + std::f64::consts::PI;
    assert!(grown >= stadium && grown < stadium + 0.05, "{grown}");
}

/// The shadow of a sphere above `t` is a disc of radius sqrt(r^2 - t^2),
/// within the tessellation's own error.
#[test]
fn a_sphere_casts_the_disc_its_slice_says() {
    let r = 10.0;
    let m = synthetic::uv_sphere(r, 120, 180);
    let sag = synthetic::uv_sphere_max_sag(r, 120, 180);
    for t in [-5.0, 0.0, 3.0, 7.5] {
        let s = shadow_above(&m, t).unwrap().area().clone();
        // Below the equator the shadow is the whole equator disc.
        let rho = if t < 0.0 { r } else { (r * r - t * t).sqrt() };
        let ideal = std::f64::consts::PI * rho * rho;
        let slack = 2.0 * std::f64::consts::PI * rho * (sag * 4.0 + 0.01);
        assert!(
            (s.area() - ideal).abs() < slack,
            "t {t}: area {} vs {ideal}",
            s.area()
        );
    }
}

/// Descending queries reuse work. They must also give the same answer as
/// starting afresh, including after a query that goes back up.
#[test]
fn incremental_matches_from_scratch() {
    let m = synthetic::uv_sphere(8.0, 40, 60);
    let mut shadows = Shadows::new(&m);
    for t in [7.0, 5.0, 2.5, 2.5, 0.0, -3.0, 4.0, 1.0] {
        let a = shadows.above(t).unwrap().area().clone();
        let b = shadow_above(&m, t).unwrap().area().clone();
        assert!(
            (a.area() - b.area()).abs() < 1e-6,
            "t {t}: {} vs {}",
            a.area(),
            b.area()
        );
    }
}
