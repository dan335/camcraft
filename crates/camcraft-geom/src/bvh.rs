//! Spatial index over a triangle mesh.
//!
//! The drop-cutter's only spatial question is *"which triangles could the tool
//! footprint touch at this XY position?"* — a 2D disc query. This wraps a bounding
//! volume hierarchy to answer it, querying with a column AABB that spans the mesh's
//! full Z range.
//!
//! Picking in the app and finding a picked face again ask two more: what a ray from
//! the eye hits first ([`MeshBvh::cast_ray`]), and which triangle lies nearest a
//! point ([`MeshBvh::nearest`]). Both are exact per triangle; the tree only prunes.
//!
//! `parry3d-f64` is glam-native in this version — its `Vector` *is* `glam::DVec3` —
//! so mesh vertices cross the boundary with no conversion at all.

use crate::mesh::{TriMesh, closest_point_on_triangle, ray_triangle};
use glam::DVec3;
use parry3d_f64::bounding_volume::Aabb;
use parry3d_f64::partitioning::{Bvh, BvhBuildStrategy, BvhNode};
use parry3d_f64::query::{PointQuery, Ray};

/// Where a ray first meets a mesh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    /// The triangle it meets.
    pub triangle: u32,
    /// How far along the ray, in units of its direction: the point is
    /// `origin + t * dir`.
    pub t: f64,
}

/// A bounding volume hierarchy over the triangles of a [`TriMesh`].
///
/// Built once per mesh and reused for every sample, which is what makes a dense CL
/// map affordable.
#[derive(Debug)]
pub struct MeshBvh {
    bvh: Bvh,
    z_min: f64,
    z_max: f64,
}

impl MeshBvh {
    /// Build an index over `mesh`.
    ///
    /// An empty mesh yields an empty index that returns no candidates, rather than
    /// an error — "nothing under the tool" is a normal answer, not a failure.
    pub fn new(mesh: &TriMesh) -> Self {
        let aabbs: Vec<Aabb> = (0..mesh.len())
            .map(|i| match mesh.triangle(i) {
                Some(t) => Aabb::new(t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2])),
                // Unreachable for i < len, but a geometry crate does not panic.
                None => Aabb::new(DVec3::ZERO, DVec3::ZERO),
            })
            .collect();

        let (z_min, z_max) = match mesh.bounds() {
            Some(b) => (b.min.z, b.max.z),
            None => (0.0, 0.0),
        };

        Self {
            bvh: Bvh::from_leaves(BvhBuildStrategy::Binned, &aabbs),
            z_min,
            z_max,
        }
    }

    /// Collect the triangles whose bounding boxes overlap the vertical column of
    /// radius `r` about `(x, y)`, appending their indices to `out`.
    ///
    /// `out` is cleared first. Taking a caller-owned buffer keeps a dense CL map from
    /// allocating once per sample.
    ///
    /// This is a conservative over-approximation — callers still perform the exact
    /// per-triangle test. Erring wide is correct here; erring narrow would silently
    /// miss contacts, and a missed contact is a gouge.
    pub fn candidates_in_column(&self, x: f64, y: f64, r: f64, out: &mut Vec<u32>) {
        out.clear();
        if !x.is_finite() || !y.is_finite() || !r.is_finite() {
            return;
        }
        // Pad the Z range so the column cannot miss a leaf through a boundary
        // comparison, and so a flat mesh still has non-degenerate extent.
        let pad = 1.0 + (self.z_max - self.z_min).abs();
        let aabb = Aabb::new(
            DVec3::new(x - r, y - r, self.z_min - pad),
            DVec3::new(x + r, y + r, self.z_max + pad),
        );
        out.extend(self.bvh.intersect_aabb(&aabb));
    }

    /// The first triangle of `mesh` that the ray `origin + t * dir` meets, from
    /// either side, with `0 <= t <= max_t`.
    ///
    /// `mesh` must be the mesh this index was built over. `dir` need not be a unit
    /// vector; `t` is measured in its units. Non-finite input misses.
    pub fn cast_ray(
        &self,
        mesh: &TriMesh,
        origin: DVec3,
        dir: DVec3,
        max_t: f64,
    ) -> Option<RayHit> {
        if !origin.is_finite() || !dir.is_finite() || dir == DVec3::ZERO || max_t.is_nan() {
            return None;
        }
        let ray = Ray::new(origin, dir);
        let (triangle, t) = self.bvh.cast_ray(&ray, max_t, |leaf, best| {
            let t = ray_triangle(origin, dir, mesh.triangle(leaf as usize)?)?;
            (t <= best).then_some(t)
        })?;
        Some(RayHit { triangle, t })
    }

    /// The triangle of `mesh` nearest `p`, and how far away it is, if one lies
    /// within `max_distance`.
    ///
    /// `mesh` must be the mesh this index was built over.
    pub fn nearest(&self, mesh: &TriMesh, p: DVec3, max_distance: f64) -> Option<(u32, f64)> {
        if !p.is_finite() || max_distance.is_nan() {
            return None;
        }
        // Just past the limit, so a triangle exactly `max_distance` away is found.
        let limit = max_distance + max_distance.abs() * 1e-12 + f64::MIN_POSITIVE;
        self.bvh.find_best(
            limit,
            |node: &BvhNode, _| node.aabb().distance_to_local_point(p, true),
            |leaf, _| {
                let t = mesh.triangle(leaf as usize)?;
                Some(closest_point_on_triangle(p, t).distance(p))
            },
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
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
}
