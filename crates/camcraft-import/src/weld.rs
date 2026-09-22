//! Turning a soup of triangles into an indexed mesh.
//!
//! Shared by every importer, so that STL and STEP weld by the same rule and count
//! what they drop the same way.

use camcraft_geom::mesh::{MeshError, TriMesh};
use glam::DVec3;
use std::collections::HashMap;

/// Accumulates triangles, welding corners that are bit-for-bit identical.
///
/// The key is the exact bit pattern: welding must never merge two points that are
/// genuinely distinct, and a tolerance-based weld can do exactly that.
#[derive(Default)]
pub(crate) struct Welder {
    index: HashMap<[u64; 3], u32>,
    vertices: Vec<DVec3>,
    triangles: Vec<[u32; 3]>,
    /// Triangles dropped: a corner was not finite, or two corners coincided.
    pub degenerate: u64,
}

impl Welder {
    fn intern(&mut self, p: DVec3) -> Option<u32> {
        if !p.is_finite() {
            return None;
        }
        let key = [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
        if let Some(&i) = self.index.get(&key) {
            return Some(i);
        }
        let i = u32::try_from(self.vertices.len()).ok()?;
        self.vertices.push(p);
        self.index.insert(key, i);
        Some(i)
    }

    /// Add one triangle. Returns whether it was kept.
    pub fn push(&mut self, corners: [DVec3; 3]) -> bool {
        let mut abc = [0u32; 3];
        for (slot, p) in abc.iter_mut().zip(corners) {
            match self.intern(p) {
                Some(i) => *slot = i,
                None => {
                    self.degenerate += 1;
                    return false;
                }
            }
        }
        if abc[0] == abc[1] || abc[1] == abc[2] || abc[0] == abc[2] {
            self.degenerate += 1;
            return false;
        }
        self.triangles.push(abc);
        true
    }

    /// Distinct vertices so far.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Triangles kept so far.
    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    /// The finished mesh.
    pub fn finish(self) -> Result<TriMesh, MeshError> {
        TriMesh::new(self.vertices, self.triangles)
    }
}
