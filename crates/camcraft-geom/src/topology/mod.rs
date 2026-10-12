//! The faces, edges and corners of a model, over its triangles.
//!
//! A person machining a part points at *features*: the floor of a pocket, the rim
//! of a hole, a corner to put the work zero on. The engine cuts triangles. This
//! module joins the two: a [`Topology`] says which face every triangle belongs to,
//! and from that, where the edges between faces run and where they meet.
//!
//! # Where faces come from
//!
//! - **STEP** files carry their faces. The importer labels every triangle with the
//!   B-rep face it was tessellated from, in the file's own face order, so a face
//!   here is exactly a face in the CAD program ([`FaceSource::File`]).
//! - **STL** files do not. The faces are found again from the triangles ([`derive`],
//!   [`FaceSource::Derived`]): planes where the triangles are flat, curved faces
//!   between creases.
//!
//! Everything past the labels is the same for both.
//!
//! # Edges and vertices
//!
//! An edge is a chain of mesh edges along which the faces on either side stay the
//! same: between two faces, or along an open boundary of one. A chain ends at a
//! *vertex*: where three or more faces meet, where the faces on either side change,
//! or where it turns sharper than 30°, so an open plate's outline still has its
//! corners. A chain that meets none of those closes on itself: the rim of a
//! hole.
//!
//! A B-rep edge is a curve between two faces. Two of them between the same pair of
//! faces, meeting smoothly, come out here as one chain; nothing needs them apart.
//!
//! # Placement
//!
//! A body is where its model is placed in the job ([`Body::placed`]): moved,
//! turned and scaled from where its file put it. The faces keep their numbers, so
//! a pick made before a move still names the same face, and its point, saved in
//! the file's coordinates, is found by [`Body::placement`].
//!
//! # Determinism
//!
//! Every order is fixed: edges by the faces they separate and then their first
//! point, vertices by position, chains start at their smallest point. No hashing
//! decides anything, so the same triangles give the same topology everywhere.

mod build;
mod derive;
mod fit;

use crate::mesh::{Aabb, TriMesh, closest_point_on_segment, closest_point_on_triangle};
use derive::DeriveParams;
use glam::{DAffine3, DVec3};

/// Where a model's faces came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceSource {
    /// The file named them: every face is a B-rep face, numbered as the file
    /// numbers them, and keeps its number when tessellated again.
    File,
    /// Found from the triangles, for a file that carries no faces.
    Derived,
}

/// What shape a face is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FaceKind {
    /// Flat: every point `p` on it has `normal · p == offset`, within the
    /// topology's [`tolerance`](Topology::tolerance). The normal is a unit vector
    /// pointing the way the face's largest triangle is wound, which for a closed
    /// solid read from a file is outward.
    Plane {
        /// Unit normal.
        normal: DVec3,
        /// Distance of the plane from the origin along `normal`.
        offset: f64,
    },
    /// Anything else.
    Curved,
}

/// One face of a model.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    /// Its triangles, as indices into the mesh, ascending.
    pub triangles: Vec<u32>,
    /// Its shape.
    pub kind: FaceKind,
    /// Its area, in square millimetres.
    pub area: f64,
    /// Its bounds, or `None` for a face with no triangles.
    pub bounds: Option<Aabb>,
    /// The edges around it, as indices into [`Topology::edges`], ascending.
    pub edges: Vec<u32>,
}

/// A circle, or an arc of one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle {
    /// Its centre.
    pub center: DVec3,
    /// A unit normal to its plane. A circle has no side, so this points up, or if
    /// the circle stands on edge, towards +Y, or failing that +X.
    pub normal: DVec3,
    /// Its radius.
    pub radius: f64,
    /// Whether it goes all the way round.
    pub full: bool,
}

/// What shape an edge is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeKind {
    /// A straight line between its ends.
    Line,
    /// A circle or an arc of one.
    Circle(Circle),
    /// Anything else.
    Curve,
}

/// One edge of a model: a chain of mesh edges.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// Its points, as mesh vertex indices, in order. A closed edge does not repeat
    /// its first point at the end.
    pub points: Vec<u32>,
    /// Whether it closes on itself.
    pub closed: bool,
    /// The faces it runs along, ascending: one for an open boundary, two between
    /// faces, more where the mesh is not a proper solid.
    pub faces: Vec<u32>,
    /// The vertices at its ends, as indices into [`Topology::vertices`]; `None`
    /// for a closed edge.
    pub ends: Option<[u32; 2]>,
    /// Its length, in millimetres.
    pub length: f64,
    /// Its shape.
    pub kind: EdgeKind,
}

/// A corner: where edges end.
#[derive(Debug, Clone, PartialEq)]
pub struct Vertex {
    /// The mesh vertex it is.
    pub mesh_vertex: u32,
    /// Where it is.
    pub position: DVec3,
    /// The edges that end here, ascending.
    pub edges: Vec<u32>,
}

/// A model's faces, edges and vertices. See the [module docs](self).
#[derive(Debug, Clone, PartialEq)]
pub struct Topology {
    source: FaceSource,
    tri_face: Vec<u32>,
    faces: Vec<Face>,
    edges: Vec<Edge>,
    vertices: Vec<Vertex>,
    size: f64,
}

/// Face labels that do not fit their mesh.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TopologyError {
    /// There was not one label per triangle.
    #[error("{labels} face labels for {triangles} triangles")]
    LabelCount {
        /// How many labels there were.
        labels: usize,
        /// How many triangles.
        triangles: usize,
    },
    /// A label named a face past the last.
    #[error("triangle {triangle} is on face {face}, but there are only {count} faces")]
    LabelRange {
        /// The triangle.
        triangle: usize,
        /// Its label.
        face: u32,
        /// How many faces there are.
        count: u32,
    },
}

/// A mesh with its topology: what a model is, from import on.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    mesh: TriMesh,
    topology: Topology,
    placement: DAffine3,
}

impl Body {
    /// A mesh whose faces the file named: `labels` gives each triangle's face, out
    /// of `count`.
    pub fn with_faces(mesh: TriMesh, labels: Vec<u32>, count: u32) -> Result<Self, TopologyError> {
        if labels.len() != mesh.len() {
            return Err(TopologyError::LabelCount {
                labels: labels.len(),
                triangles: mesh.len(),
            });
        }
        if let Some((triangle, &face)) = labels.iter().enumerate().find(|(_, f)| **f >= count) {
            return Err(TopologyError::LabelRange {
                triangle,
                face,
                count,
            });
        }
        let topology = Topology::build(&mesh, labels, count, FaceSource::File);
        Ok(Self {
            mesh,
            topology,
            placement: DAffine3::IDENTITY,
        })
    }

    /// A mesh with no faces of its own, whose faces are found from its triangles.
    pub fn derived(mesh: TriMesh) -> Self {
        let size = build::diagonal(&mesh);
        let sides = build::sides(&mesh);
        let params = DeriveParams::default();
        let (labels, count) = derive::faces(&mesh, &sides, &params, fit_tolerance(size), size);
        let topology =
            Topology::build_with(&mesh, &sides, size, labels, count, FaceSource::Derived);
        Self {
            mesh,
            topology,
            placement: DAffine3::IDENTITY,
        }
    }

    /// The body as its file put it, placed by `placement` instead: every point
    /// moved by it, and the faces keeping their numbers. A placement that mirrors
    /// turns the triangles round, so they still face out.
    pub fn placed(&self, placement: DAffine3) -> Result<Self, crate::mesh::MeshError> {
        let to = placement * self.placement.inverse();
        let mut mesh = self.mesh.mapped(|p| to.transform_point3(p))?;
        if to.matrix3.determinant() < 0.0 {
            mesh = TriMesh::new(
                mesh.vertices().to_vec(),
                mesh.triangles()
                    .iter()
                    .map(|&[a, b, c]| [a, c, b])
                    .collect(),
            )?;
        }
        let t = &self.topology;
        let count = u32::try_from(t.faces.len()).unwrap_or(u32::MAX);
        let topology = Topology::build(&mesh, t.tri_face.clone(), count, t.source);
        Ok(Self {
            mesh,
            topology,
            placement,
        })
    }

    /// Where it is placed: from the coordinates of its file to where it is.
    pub fn placement(&self) -> DAffine3 {
        self.placement
    }

    /// The triangles.
    pub fn mesh(&self) -> &TriMesh {
        &self.mesh
    }

    /// Their faces, edges and vertices.
    pub fn topology(&self) -> &Topology {
        &self.topology
    }
}

/// How far a point may lie from a fitted plane, line or circle and still be on it,
/// for a model whose bounds have diagonal `size`.
///
/// A tenth of a micron, and a millionth of the part's size on top: what float
/// coordinates in a file carry, and far below anything a cutter could show.
pub(crate) fn fit_tolerance(size: f64) -> f64 {
    1e-4 + 1e-6 * size
}

impl Topology {
    /// Where the faces came from.
    pub fn source(&self) -> FaceSource {
        self.source
    }

    /// The diagonal of the model's bounds.
    pub fn size(&self) -> f64 {
        self.size
    }

    /// How close to a plane, line or circle a point must be to be on it.
    pub fn tolerance(&self) -> f64 {
        fit_tolerance(self.size)
    }

    /// The face of each triangle.
    pub fn triangle_faces(&self) -> &[u32] {
        &self.tri_face
    }

    /// The face triangle `t` is part of.
    pub fn face_of(&self, t: u32) -> Option<u32> {
        self.tri_face.get(t as usize).copied()
    }

    /// Every face. A model read from a file may have faces with no triangles: the
    /// file's numbering is kept whole even where a face tessellated to nothing.
    pub fn faces(&self) -> &[Face] {
        &self.faces
    }

    /// Every edge.
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    /// Every vertex.
    pub fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }

    /// Face `f`.
    pub fn face(&self, f: u32) -> Option<&Face> {
        self.faces.get(f as usize)
    }

    /// Edge `e`.
    pub fn edge(&self, e: u32) -> Option<&Edge> {
        self.edges.get(e as usize)
    }

    /// Vertex `v`.
    pub fn vertex(&self, v: u32) -> Option<&Vertex> {
        self.vertices.get(v as usize)
    }

    /// The loop edge `e` goes round at its own height: the edges joined to it end
    /// to end that stay level with it, taking at each corner the one that turns
    /// least, back to where `e` starts. How faces are split does not matter, so
    /// a chamfer cut into strips still gives the outline it runs along. A closed
    /// edge is a loop by itself.
    ///
    /// `points` gives an edge's points, in order, in the frame whose Z is up;
    /// level is within `tolerance`. `None` for an edge that is not level, or whose
    /// loop does not close.
    pub fn level_loop(
        &self,
        e: u32,
        points: impl Fn(u32) -> Vec<DVec3>,
        tolerance: f64,
    ) -> Option<Vec<u32>> {
        let first = self.edge(e)?;
        if first.closed {
            return Some(vec![e]);
        }
        let height = |n: u32| {
            let p = points(n);
            let lo = p.iter().map(|p| p.z).reduce(f64::min)?;
            let hi = p.iter().map(|p| p.z).reduce(f64::max)?;
            (hi - lo <= tolerance).then_some((lo + hi) / 2.0)
        };
        let z = height(e)?;
        // Edge `n`'s points in order away from vertex `from`, and the vertex at
        // its far end.
        let away = |n: u32, from: u32| {
            let edge = self.edge(n)?;
            let [a, b] = edge.ends?;
            let far = if a == from {
                b
            } else if b == from {
                a
            } else {
                return None;
            };
            let mut p = points(n);
            let starts_here = edge.points.first() == Some(&self.vertex(from)?.mesh_vertex);
            if !starts_here {
                p.reverse();
            }
            Some((p, far))
        };
        let [origin, start] = first.ends?;
        let (p, at) = away(e, origin)?;
        debug_assert_eq!(at, start);
        let mut heading = arriving(&p)?;
        let mut at = start;
        let mut out = vec![e];
        while at != origin {
            let (next, far, arrive) = self
                .vertex(at)?
                .edges
                .iter()
                .filter(|n| !out.contains(n))
                .filter(|&&n| height(n).is_some_and(|h| (h - z).abs() <= tolerance))
                .filter_map(|&n| {
                    let (p, far) = away(n, at)?;
                    let turn = turn(heading, leaving(&p)?);
                    Some((n, far, arriving(&p)?, turn))
                })
                .min_by(|a, b| a.3.total_cmp(&b.3))
                .map(|(n, far, arrive, _)| (n, far, arrive))?;
            out.push(next);
            at = far;
            heading = arrive;
            if out.len() > self.edges.len() {
                return None;
            }
        }
        Some(out)
    }

    /// Edge `e`'s points, in order, from `mesh`, the mesh this topology is of. A
    /// closed edge's first point is not repeated.
    pub fn edge_points<'a>(
        &'a self,
        mesh: &'a TriMesh,
        e: u32,
    ) -> impl Iterator<Item = DVec3> + 'a {
        self.edges
            .get(e as usize)
            .map_or(&[][..], |e| e.points.as_slice())
            .iter()
            .filter_map(|&v| mesh.vertices().get(v as usize).copied())
    }

    /// Edge `e`'s segments, the closing one included.
    fn segments<'a>(
        &'a self,
        mesh: &'a TriMesh,
        e: u32,
    ) -> impl Iterator<Item = (DVec3, DVec3)> + 'a {
        let closed = self.edge(e).is_some_and(|e| e.closed);
        let points: Vec<DVec3> = self.edge_points(mesh, e).collect();
        let closing = match (closed, points.first(), points.last()) {
            (true, Some(&a), Some(&b)) if points.len() > 2 => Some((b, a)),
            _ => None,
        };
        let open: Vec<(DVec3, DVec3)> = points.windows(2).filter_map(pair).collect();
        open.into_iter().chain(closing)
    }

    /// The point of edge `e` nearest `p`, and how far it is.
    pub fn closest_on_edge(&self, mesh: &TriMesh, e: u32, p: DVec3) -> Option<(DVec3, f64)> {
        let single = self
            .edge(e)
            .filter(|e| e.points.len() == 1)
            .and_then(|_| self.edge_points(mesh, e).next())
            .map(|q| (q, q.distance(p)));
        self.segments(mesh, e)
            .map(|(a, b)| {
                let q = closest_point_on_segment(p, a, b);
                (q, q.distance(p))
            })
            .chain(single)
            .min_by(|x, y| x.1.total_cmp(&y.1))
    }

    /// How far `p` lies from face `f`, or `None` for a face with no triangles.
    pub fn distance_to_face(&self, mesh: &TriMesh, f: u32, p: DVec3) -> Option<f64> {
        self.face(f)?
            .triangles
            .iter()
            .filter_map(|&t| mesh.triangle(t as usize))
            .map(|t| closest_point_on_triangle(p, t).distance(p))
            .min_by(f64::total_cmp)
    }

    /// The point halfway along edge `e`, measured along it.
    pub fn edge_middle(&self, mesh: &TriMesh, e: u32) -> Option<DVec3> {
        let half = self.edge(e)?.length / 2.0;
        let mut run = 0.0;
        let mut last = None;
        for (a, b) in self.segments(mesh, e) {
            let l = a.distance(b);
            if run + l >= half && l > 0.0 {
                return Some(a + (b - a) * ((half - run) / l));
            }
            run += l;
            last = Some(b);
        }
        last.or_else(|| self.edge_points(mesh, e).next())
    }
}

/// The way a chain of points leaves its first point, seen from above.
fn leaving(p: &[DVec3]) -> Option<glam::DVec2> {
    let from = p.first()?.truncate();
    p.iter()
        .map(|q| q.truncate() - from)
        .find(|d| d.length() > 1e-9)
        .map(glam::DVec2::normalize)
}

/// The way a chain of points arrives at its last point, seen from above.
fn arriving(p: &[DVec3]) -> Option<glam::DVec2> {
    let to = p.last()?.truncate();
    p.iter()
        .rev()
        .map(|q| to - q.truncate())
        .find(|d| d.length() > 1e-9)
        .map(glam::DVec2::normalize)
}

/// How far, in radians either way, heading `from` turns to go `to`.
fn turn(from: glam::DVec2, to: glam::DVec2) -> f64 {
    from.perp_dot(to).atan2(from.dot(to)).abs()
}

fn pair(w: &[DVec3]) -> Option<(DVec3, DVec3)> {
    match w {
        [a, b] => Some((*a, *b)),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
