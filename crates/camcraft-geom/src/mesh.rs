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
    /// Merged meshes would have more vertices than a `u32` index can name.
    #[error("the merged mesh would have {vertices} vertices, more than a mesh can index")]
    TooLarge {
        /// How many vertices it would have.
        vertices: usize,
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

    /// The smallest box holding both.
    pub fn union(&self, other: &Aabb) -> Aabb {
        Aabb {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    /// The middle of the box.
    pub fn center(&self) -> DVec3 {
        (self.min + self.max) * 0.5
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

    /// The mesh with every vertex moved by `f`; the triangles are unchanged.
    ///
    /// `f` should be a rigid motion. A reflection would turn every triangle inside
    /// out, which nothing downstream checks for.
    pub fn mapped(&self, f: impl Fn(DVec3) -> DVec3) -> Result<TriMesh, MeshError> {
        TriMesh::new(
            self.vertices.iter().map(|v| f(*v)).collect(),
            self.triangles.clone(),
        )
    }

    /// Several meshes as one: vertices appended in order, indices offset to match.
    pub fn merged(parts: &[&TriMesh]) -> Result<TriMesh, MeshError> {
        let vertices: usize = parts.iter().map(|m| m.vertices.len()).sum();
        if u32::try_from(vertices).is_err() {
            return Err(MeshError::TooLarge { vertices });
        }
        let mut out = TriMesh {
            vertices: Vec::with_capacity(vertices),
            triangles: Vec::with_capacity(parts.iter().map(|m| m.triangles.len()).sum()),
        };
        for m in parts {
            // Fits: the total was checked above.
            #[allow(clippy::cast_possible_truncation)]
            let base = out.vertices.len() as u32;
            out.vertices.extend_from_slice(&m.vertices);
            out.triangles
                .extend(m.triangles.iter().map(|t| t.map(|i| i + base)));
        }
        Ok(out)
    }
}

/// Where the ray `origin + t * dir` meets the triangle, as `t >= 0`, from either
/// side.
///
/// Möller-Trumbore. Two-sided because a picked face is picked whichever way its
/// triangles happen to be wound, and STL files get that wrong often enough. A ray
/// in the triangle's plane, or a triangle with no area, misses.
pub fn ray_triangle(origin: DVec3, dir: DVec3, [a, b, c]: [DVec3; 3]) -> Option<f64> {
    let (e1, e2) = (b - a, c - a);
    let p = dir.cross(e2);
    let det = e1.dot(p);
    // Relative to the sizes involved, so it holds at any scale.
    let scale = e1.length() * e2.length() * dir.length();
    if det.is_nan() || det.abs() <= 1e-12 * scale {
        return None;
    }
    let inv = 1.0 / det;
    let s = origin - a;
    let u = s.dot(p) * inv;
    if !(-1e-12..=1.0 + 1e-12).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) * inv;
    if v < -1e-12 || u + v > 1.0 + 1e-12 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t >= 0.0 && t.is_finite()).then_some(t)
}

/// The point of the triangle nearest `p`.
///
/// By the triangle's Voronoi regions (Ericson, *Real-Time Collision Detection*,
/// 5.1.5), so a degenerate triangle still gives the nearest point of what it is: a
/// segment or a point.
pub fn closest_point_on_triangle(p: DVec3, [a, b, c]: [DVec3; 3]) -> DVec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return a + ab * v;
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return a + ac * w;
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return b + (c - b) * w;
    }
    let sum = va + vb + vc;
    if sum.is_nan() || sum == 0.0 {
        // Collinear corners with p off every edge's end: the nearest end will do.
        return [a, b, c]
            .into_iter()
            .min_by(|x, y| x.distance_squared(p).total_cmp(&y.distance_squared(p)))
            .unwrap_or(a);
    }
    let denom = 1.0 / sum;
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// The point of the segment `a`-`b` nearest `p`.
pub fn closest_point_on_segment(p: DVec3, a: DVec3, b: DVec3) -> DVec3 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2.is_nan() || len2 <= 0.0 {
        return a;
    }
    a + ab * ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use glam::dvec3;

    #[test]
    fn rays_hit_triangles_from_either_side_and_miss_beside_them() {
        let t = [
            dvec3(0.0, 0.0, 0.0),
            dvec3(2.0, 0.0, 0.0),
            dvec3(0.0, 2.0, 0.0),
        ];
        let down = ray_triangle(dvec3(0.5, 0.5, 3.0), dvec3(0.0, 0.0, -1.0), t);
        assert_eq!(down, Some(3.0));
        let up = ray_triangle(dvec3(0.5, 0.5, -2.0), dvec3(0.0, 0.0, 2.0), t);
        assert_eq!(up, Some(1.0));
        assert_eq!(
            ray_triangle(dvec3(1.5, 1.5, 3.0), dvec3(0.0, 0.0, -1.0), t),
            None
        );
        // Pointing away, and lying in the plane.
        assert_eq!(
            ray_triangle(dvec3(0.5, 0.5, 3.0), dvec3(0.0, 0.0, 1.0), t),
            None
        );
        assert_eq!(
            ray_triangle(dvec3(-1.0, 0.5, 0.0), dvec3(1.0, 0.0, 0.0), t),
            None
        );
        assert_eq!(
            ray_triangle(dvec3(0.5, 0.5, f64::NAN), dvec3(0.0, 0.0, -1.0), t),
            None
        );
    }

    #[test]
    fn nearest_points_on_a_triangle_by_region() {
        let t = [
            dvec3(0.0, 0.0, 0.0),
            dvec3(2.0, 0.0, 0.0),
            dvec3(0.0, 2.0, 0.0),
        ];
        let near = |p| closest_point_on_triangle(p, t);
        assert_eq!(near(dvec3(0.5, 0.5, 4.0)), dvec3(0.5, 0.5, 0.0));
        assert_eq!(near(dvec3(-1.0, -1.0, 0.0)), t[0]);
        assert_eq!(near(dvec3(3.0, -1.0, 1.0)), t[1]);
        assert_eq!(near(dvec3(1.0, -3.0, 0.0)), dvec3(1.0, 0.0, 0.0));
        assert_eq!(near(dvec3(2.0, 2.0, 0.0)), dvec3(1.0, 1.0, 0.0));
        // A triangle with no area is the segment it collapses to.
        let flat = [
            dvec3(0.0, 0.0, 0.0),
            dvec3(1.0, 0.0, 0.0),
            dvec3(2.0, 0.0, 0.0),
        ];
        let q = closest_point_on_triangle(dvec3(1.5, 1.0, 0.0), flat);
        assert!((q - dvec3(1.5, 0.0, 0.0)).length() < 1e-12, "{q}");
        assert_eq!(
            closest_point_on_segment(dvec3(5.0, 1.0, 0.0), t[0], t[1]),
            t[1]
        );
    }

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
    fn merged_meshes_keep_every_triangle_and_both_bounds() {
        let a = tri();
        let b = a.mapped(|v| v + dvec3(10.0, 0.0, -1.0)).unwrap();
        let m = TriMesh::merged(&[&a, &b]).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m.triangle(1).unwrap(), b.triangle(0).unwrap());
        let bounds = m.bounds().unwrap();
        assert_eq!(bounds, a.bounds().unwrap().union(&b.bounds().unwrap()));
        assert_eq!(bounds.min, dvec3(0.0, 0.0, -1.0));
        assert_eq!(bounds.max, dvec3(11.0, 1.0, 2.0));
        assert_eq!(TriMesh::merged(&[]).unwrap(), TriMesh::default());
    }

    #[test]
    fn empty_mesh_has_no_bounds() {
        assert_eq!(TriMesh::default().bounds(), None);
        assert!(TriMesh::default().is_empty());
    }
}
