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
            .map(|i| match mesh.triangle_bounds(i) {
                Some(b) => Aabb::new(b.min, b.max),
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
    /// `out` is cleared first. Taking a caller-owned buffer lets a caller making many
    /// queries reuse one allocation.
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
mod tests;
