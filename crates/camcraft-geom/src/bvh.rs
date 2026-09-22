//! Spatial index over a triangle mesh.
//!
//! The drop-cutter's only spatial question is *"which triangles could the tool
//! footprint touch at this XY position?"* — a 2D disc query, not a ray cast. This
//! wraps a bounding volume hierarchy to answer it, querying with a column AABB that
//! spans the mesh's full Z range.
//!
//! `parry3d-f64` is glam-native in this version — its `Vector` *is* `glam::DVec3` —
//! so mesh vertices cross the boundary with no conversion at all.

use crate::mesh::TriMesh;
use glam::DVec3;
use parry3d_f64::bounding_volume::Aabb;
use parry3d_f64::partitioning::{Bvh, BvhBuildStrategy};

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

    #[test]
    fn handles_a_degenerate_single_point_mesh() {
        let mesh = TriMesh::new(vec![dvec3(0.0, 0.0, 0.0)], vec![[0, 0, 0]]).unwrap();
        let bvh = MeshBvh::new(&mesh);
        let mut out = Vec::new();
        bvh.candidates_in_column(0.0, 0.0, 1.0, &mut out);
        assert_eq!(out, vec![0]);
    }
}
