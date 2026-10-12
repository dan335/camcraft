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
/// genuinely distinct, and a tolerance-based weld can do exactly that. Negative
/// zero is zero, though: exporters write `-0` for some corners on an axis and
/// `0` for others, and two bit patterns there would split the part along it.
#[derive(Default)]
pub(crate) struct Welder {
    index: HashMap<[u64; 3], u32>,
    vertices: Vec<DVec3>,
    triangles: Vec<[u32; 3]>,
    /// Triangles dropped: a corner was not finite, or two corners coincided.
    pub degenerate: u64,
}

/// A corner as it is welded, and its key: `None` when it is not finite.
fn keyed(p: DVec3) -> Option<(DVec3, [u64; 3])> {
    if !p.is_finite() {
        return None;
    }
    // Adding zero makes negative zero positive, and leaves the rest alone.
    let p = p + DVec3::ZERO;
    Some((p, [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]))
}

impl Welder {
    fn intern(&mut self, (p, key): (DVec3, [u64; 3])) -> Option<u32> {
        if let Some(&i) = self.index.get(&key) {
            return Some(i);
        }
        let i = u32::try_from(self.vertices.len()).ok()?;
        self.vertices.push(p);
        self.index.insert(key, i);
        Some(i)
    }

    /// Add one triangle. Returns whether it was kept.
    ///
    /// A triangle is judged before any of its corners is added, so one that is
    /// dropped leaves no vertex behind.
    pub fn push(&mut self, corners: [DVec3; 3]) -> bool {
        let [Some(a), Some(b), Some(c)] = corners.map(keyed) else {
            self.degenerate += 1;
            return false;
        };
        if a.1 == b.1 || b.1 == c.1 || a.1 == c.1 {
            self.degenerate += 1;
            return false;
        }
        let mut abc = [0u32; 3];
        for (slot, corner) in abc.iter_mut().zip([a, b, c]) {
            match self.intern(corner) {
                Some(i) => *slot = i,
                None => {
                    self.degenerate += 1;
                    return false;
                }
            }
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// A corner written `-0` on one triangle and `0` on the next is one corner,
    /// or a part centred on the origin splits along its axes.
    #[test]
    fn negative_zero_welds_to_zero() {
        let mut w = Welder::default();
        let (a, b) = (DVec3::new(0.0, 0.0, 0.0), DVec3::new(-0.0, 1.0, -0.0));
        assert!(w.push([a, DVec3::new(1.0, 0.0, 0.0), DVec3::new(0.0, 1.0, 0.0)]));
        assert!(w.push([DVec3::new(-0.0, -0.0, 0.0), b, DVec3::new(-1.0, 0.0, 0.0)]));
        assert_eq!(w.vertex_count(), 4);
    }

    /// A triangle that is dropped, for a corner that is not finite or two that
    /// coincide, leaves none of its corners behind as vertices.
    #[test]
    fn a_dropped_triangle_adds_no_vertices() {
        let mut w = Welder::default();
        let (a, b, c) = (
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(0.0, 1.0, 0.0),
        );
        assert!(!w.push([a, b, DVec3::new(f64::NAN, 0.0, 0.0)]));
        assert!(!w.push([a, b, b]));
        assert!(!w.push([c, DVec3::new(-0.0, 1.0, 0.0), a]));
        assert_eq!((w.vertex_count(), w.degenerate), (0, 3));
        assert!(w.push([a, b, c]));
        let mesh = w.finish().unwrap();
        assert_eq!(
            (mesh.vertices(), mesh.triangles()),
            (&[a, b, c][..], &[[0, 1, 2]][..])
        );
    }
}
