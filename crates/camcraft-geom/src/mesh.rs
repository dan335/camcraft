//! Triangle meshes.
//!
//! This is the interface between import and everything downstream: nothing past
//! this point knows or cares whether the triangles came from STL or STEP.

use glam::DVec3;

/// A triangle mesh in setup space, millimetres, Z-up.
///
/// Construction validates what would otherwise surface later as a panic deep in a
/// toolpath: indices are checked against the vertex count and coordinates are
/// checked for finiteness. Zero-area triangles are *permitted* — real CAD
/// tessellations contain them, they contribute nothing to a drop-cutter, and
/// rejecting a whole model over one is the wrong trade.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TriMesh {
    vertices: Vec<DVec3>,
    triangles: Vec<[u32; 3]>,
}

/// A mesh that could not be accepted.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum MeshError {
    /// A triangle referenced a vertex that does not exist.
    #[error("triangle {triangle} references vertex {index}, but the mesh has {count}")]
    IndexOutOfRange {
        /// Index of the offending triangle.
        triangle: usize,
        /// The out-of-range vertex index.
        index: u32,
        /// How many vertices the mesh actually has.
        count: usize,
    },
    /// A vertex coordinate was NaN or infinite.
    #[error("vertex {index} is not finite: {value:?}")]
    NonFiniteVertex {
        /// Index of the offending vertex.
        index: usize,
        /// The offending value.
        value: DVec3,
    },
}

/// An axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    /// Lower corner.
    pub min: DVec3,
    /// Upper corner.
    pub max: DVec3,
}

impl Aabb {
    /// Size along each axis.
    pub fn extent(&self) -> DVec3 {
        self.max - self.min
    }
}

impl TriMesh {
    /// Build a mesh, validating indices and coordinates.
    pub fn new(vertices: Vec<DVec3>, triangles: Vec<[u32; 3]>) -> Result<Self, MeshError> {
        for (index, value) in vertices.iter().enumerate() {
            if !value.is_finite() {
                return Err(MeshError::NonFiniteVertex {
                    index,
                    value: *value,
                });
            }
        }
        let count = vertices.len();
        for (triangle, abc) in triangles.iter().enumerate() {
            for index in abc {
                if *index as usize >= count {
                    return Err(MeshError::IndexOutOfRange {
                        triangle,
                        index: *index,
                        count,
                    });
                }
            }
        }
        Ok(Self {
            vertices,
            triangles,
        })
    }

    /// The mesh vertices.
    pub fn vertices(&self) -> &[DVec3] {
        &self.vertices
    }

    /// The mesh triangles, as indices into [`vertices`](TriMesh::vertices).
    pub fn triangles(&self) -> &[[u32; 3]] {
        &self.triangles
    }

    /// How many triangles the mesh has.
    pub fn len(&self) -> usize {
        self.triangles.len()
    }

    /// Whether the mesh has no triangles.
    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }

    /// The three corners of triangle `i`, or `None` if `i` is out of range.
    ///
    /// Indices were validated at construction, so the vertex lookups cannot fail.
    pub fn triangle(&self, i: usize) -> Option<[DVec3; 3]> {
        let [a, b, c] = *self.triangles.get(i)?;
        Some([
            *self.vertices.get(a as usize)?,
            *self.vertices.get(b as usize)?,
            *self.vertices.get(c as usize)?,
        ])
    }

    /// Bounding box of the mesh, or `None` when it has no vertices.
    pub fn bounds(&self) -> Option<Aabb> {
        let mut iter = self.vertices.iter();
        let first = *iter.next()?;
        let (min, max) = iter.fold((first, first), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        Some(Aabb { min, max })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use glam::dvec3;

    fn tri() -> TriMesh {
        TriMesh::new(
            vec![
                dvec3(0.0, 0.0, 0.0),
                dvec3(1.0, 0.0, 0.0),
                dvec3(0.0, 1.0, 2.0),
            ],
            vec![[0, 1, 2]],
        )
        .unwrap()
    }

    #[test]
    fn rejects_out_of_range_indices() {
        let err = TriMesh::new(vec![dvec3(0.0, 0.0, 0.0)], vec![[0, 1, 2]]).unwrap_err();
        assert_eq!(
            err,
            MeshError::IndexOutOfRange {
                triangle: 0,
                index: 1,
                count: 1
            }
        );
    }

    #[test]
    fn rejects_non_finite_vertices() {
        let err = TriMesh::new(vec![dvec3(0.0, f64::NAN, 0.0)], vec![]).unwrap_err();
        assert!(matches!(err, MeshError::NonFiniteVertex { index: 0, .. }));
        let err = TriMesh::new(vec![dvec3(f64::INFINITY, 0.0, 0.0)], vec![]).unwrap_err();
        assert!(matches!(err, MeshError::NonFiniteVertex { index: 0, .. }));
    }

    /// Real CAD tessellations contain these; rejecting a model over one would be
    /// the wrong trade, and they contribute nothing to a drop-cutter anyway.
    #[test]
    fn accepts_degenerate_triangles() {
        let m = TriMesh::new(
            vec![dvec3(0.0, 0.0, 0.0), dvec3(1.0, 0.0, 0.0)],
            vec![[0, 0, 0], [0, 1, 1]],
        );
        assert!(m.is_ok());
    }

    #[test]
    fn reports_geometry() {
        let m = tri();
        assert_eq!(m.len(), 1);
        assert!(!m.is_empty());
        assert_eq!(m.triangle(0).unwrap()[2], dvec3(0.0, 1.0, 2.0));
        assert_eq!(m.triangle(1), None);
        let b = m.bounds().unwrap();
        assert_eq!(b.min, dvec3(0.0, 0.0, 0.0));
        assert_eq!(b.max, dvec3(1.0, 1.0, 2.0));
        assert_eq!(b.extent(), dvec3(1.0, 1.0, 2.0));
    }

    #[test]
    fn empty_mesh_has_no_bounds() {
        assert_eq!(TriMesh::default().bounds(), None);
        assert!(TriMesh::default().is_empty());
    }
}
