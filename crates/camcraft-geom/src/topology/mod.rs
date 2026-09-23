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
//! or where it turns sharper than [`KINK_DEGREES`], so an open plate's outline still
//! has its corners. A chain that meets none of those closes on itself: the rim of a
//! hole.
//!
//! A B-rep edge is a curve between two faces. Two of them between the same pair of
//! faces, meeting smoothly, come out here as one chain; nothing needs them apart.
//!
//! # Determinism
//!
//! Every order is fixed: edges by the faces they separate and then their first
//! point, vertices by position, chains start at their smallest point. No hashing
//! decides anything, so the same triangles give the same topology everywhere.

mod derive;
mod fit;

pub use derive::DeriveParams;

use crate::mesh::{Aabb, TriMesh, closest_point_on_segment, closest_point_on_triangle};
use glam::DVec3;
use std::cmp::Ordering;

/// A chain of edges turning sharper than this, in degrees, is split in two there.
pub const KINK_DEGREES: f64 = 30.0;

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
        Ok(Self { mesh, topology })
    }

    /// A mesh with no faces of its own, whose faces are found from its triangles.
    pub fn derived(mesh: TriMesh) -> Self {
        Self::derived_with(mesh, &DeriveParams::default())
    }

    /// As [`derived`](Body::derived), with its settings given.
    pub fn derived_with(mesh: TriMesh, params: &DeriveParams) -> Self {
        let size = diagonal(&mesh);
        let sides = sides(&mesh);
        let (labels, count) = derive::faces(&mesh, &sides, params, fit_tolerance(size), size);
        let topology = Topology::build(&mesh, labels, count, FaceSource::Derived);
        Self { mesh, topology }
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
pub fn fit_tolerance(size: f64) -> f64 {
    1e-4 + 1e-6 * size
}

fn diagonal(mesh: &TriMesh) -> f64 {
    mesh.bounds().map_or(0.0, |b| b.extent().length())
}

/// One side of one triangle, keyed by its corners in ascending order.
#[derive(Debug, Clone, Copy)]
struct Side {
    lo: u32,
    hi: u32,
    tri: u32,
    /// Whether the triangle runs this side from `lo` to `hi`.
    forward: bool,
}

/// Every side of every triangle, sorted so sides shared by several triangles are
/// together. Triangles that repeat a corner have no sides.
fn sides(mesh: &TriMesh) -> Vec<Side> {
    let mut out = Vec::with_capacity(mesh.len() * 3);
    for (t, &[a, b, c]) in mesh.triangles().iter().enumerate() {
        let Ok(tri) = u32::try_from(t) else { break };
        if a == b || b == c || a == c {
            continue;
        }
        for (p, q) in [(a, b), (b, c), (c, a)] {
            out.push(Side {
                lo: p.min(q),
                hi: p.max(q),
                tri,
                forward: p < q,
            });
        }
    }
    out.sort_unstable_by_key(|s| (s.lo, s.hi, s.tri, s.forward));
    out
}

fn cmp_points(a: DVec3, b: DVec3) -> Ordering {
    a.x.total_cmp(&b.x)
        .then(a.y.total_cmp(&b.y))
        .then(a.z.total_cmp(&b.z))
}

/// A mesh edge between different faces, or on a boundary.
struct Feature {
    a: u32,
    b: u32,
    faces: Vec<u32>,
}

impl Feature {
    fn other(&self, v: u32) -> u32 {
        if self.a == v { self.b } else { self.a }
    }
}

impl Topology {
    fn build(mesh: &TriMesh, tri_face: Vec<u32>, count: u32, source: FaceSource) -> Self {
        let size = diagonal(mesh);
        let tol = fit_tolerance(size);
        let at = |v: u32| {
            mesh.vertices()
                .get(v as usize)
                .copied()
                .unwrap_or(DVec3::NAN)
        };

        // The mesh edges that bound faces.
        let sides = sides(mesh);
        let mut features = Vec::new();
        for group in sides.chunk_by(|x, y| (x.lo, x.hi) == (y.lo, y.hi)) {
            let Some(first) = group.first() else { continue };
            let mut faces: Vec<u32> = group
                .iter()
                .filter_map(|s| tri_face.get(s.tri as usize).copied())
                .collect();
            faces.sort_unstable();
            faces.dedup();
            if group.len() != 2 || faces.len() > 1 {
                features.push(Feature {
                    a: first.lo,
                    b: first.hi,
                    faces,
                });
            }
        }

        // Which features meet at each mesh vertex, sorted by vertex.
        let mut incidence: Vec<(u32, u32)> = Vec::with_capacity(features.len() * 2);
        for (i, f) in features.iter().enumerate() {
            let Ok(i) = u32::try_from(i) else { break };
            incidence.push((f.a, i));
            incidence.push((f.b, i));
        }
        incidence.sort_unstable();
        let incident = |v: u32| {
            let lo = incidence.partition_point(|p| p.0 < v);
            let hi = incidence.partition_point(|p| p.0 <= v);
            incidence.get(lo..hi).unwrap_or(&[])
        };
        let feature = |e: u32| features.get(e as usize);

        // The vertices, where chains end.
        let kink = KINK_DEGREES.to_radians().cos();
        let mut corner = vec![false; mesh.vertices().len()];
        let mut corners = Vec::new();
        for group in incidence.chunk_by(|x, y| x.0 == y.0) {
            let Some(&(v, _)) = group.first() else {
                continue;
            };
            let is_corner = match group {
                [(_, e1), (_, e2)] => match (feature(*e1), feature(*e2)) {
                    (Some(f1), Some(f2)) => {
                        let p = at(v);
                        let d1 = (p - at(f1.other(v))).normalize_or_zero();
                        let d2 = (at(f2.other(v)) - p).normalize_or_zero();
                        f1.faces != f2.faces || d1.dot(d2) < kink
                    }
                    _ => true,
                },
                _ => true,
            };
            if is_corner {
                if let Some(slot) = corner.get_mut(v as usize) {
                    *slot = true;
                }
                corners.push(v);
            }
        }
        let is_corner = |v: u32| corner.get(v as usize).copied().unwrap_or(false);

        // Walk the chains: from each vertex first, then the loops that are left.
        let mut used = vec![false; features.len()];
        let mut chains: Vec<(Vec<u32>, bool, u32)> = Vec::new(); // (points, closed, a feature)
        // A chain from `start` along feature `first`, to the next vertex, or back
        // round to `start` when it is not one.
        let walk = |start: u32, first: u32, used: &mut Vec<bool>| -> Vec<u32> {
            let mut points = vec![start];
            let (mut v, mut e) = (start, first);
            loop {
                if let Some(u) = used.get_mut(e as usize) {
                    *u = true;
                }
                let Some(f) = feature(e) else { break };
                let next = f.other(v);
                if is_corner(next) {
                    points.push(next);
                    break;
                }
                if next == start {
                    break;
                }
                points.push(next);
                let Some(&(_, ne)) = incident(next)
                    .iter()
                    .find(|(_, x)| *x != e && !used.get(*x as usize).copied().unwrap_or(true))
                else {
                    break;
                };
                (v, e) = (next, ne);
            }
            points
        };
        for &v in &corners {
            for &(_, e) in incident(v) {
                if used.get(e as usize).copied().unwrap_or(true) {
                    continue;
                }
                let mut points = walk(v, e, &mut used);
                if let (Some(&a), Some(&b)) = (points.first(), points.last())
                    && cmp_points(at(b), at(a)).is_lt()
                {
                    points.reverse();
                }
                chains.push((points, false, e));
            }
        }
        for e in 0..features.len() {
            let Ok(e) = u32::try_from(e) else { break };
            if used.get(e as usize).copied().unwrap_or(true) {
                continue;
            }
            let Some(f) = feature(e) else { continue };
            let points = walk(f.a, e, &mut used);
            chains.push((start_at_smallest(points, &at), true, e));
        }

        // Order the edges, then the vertices, then tie them together.
        let mut edges: Vec<Edge> = chains
            .into_iter()
            .map(|(points, closed, e)| {
                let faces = feature(e).map(|f| f.faces.clone()).unwrap_or_default();
                let positions: Vec<DVec3> = points.iter().map(|&v| at(v)).collect();
                let mut length: f64 = positions.windows(2).map(segment_length).sum();
                if closed && let (Some(a), Some(b)) = (positions.first(), positions.last()) {
                    length += a.distance(*b);
                }
                Edge {
                    kind: fit::edge(&positions, closed, tol),
                    points,
                    closed,
                    faces,
                    ends: None,
                    length,
                }
            })
            .collect();
        edges.sort_by(|x, y| {
            x.faces.cmp(&y.faces).then_with(|| {
                let p = |e: &Edge, i: usize| e.points.get(i).map_or(DVec3::NAN, |&v| at(v));
                cmp_points(p(x, 0), p(y, 0))
                    .then(cmp_points(p(x, 1), p(y, 1)))
                    .then(x.points.len().cmp(&y.points.len()))
                    .then(x.points.cmp(&y.points))
            })
        });

        corners.sort_by(|a, b| cmp_points(at(*a), at(*b)).then(a.cmp(b)));
        let mut vertices: Vec<Vertex> = corners
            .iter()
            .map(|&v| Vertex {
                mesh_vertex: v,
                position: at(v),
                edges: Vec::new(),
            })
            .collect();
        let mut vertex_of: Vec<(u32, u32)> = corners
            .iter()
            .enumerate()
            .filter_map(|(i, &v)| Some((v, u32::try_from(i).ok()?)))
            .collect();
        vertex_of.sort_unstable();
        let vertex_id = |v: u32| {
            vertex_of
                .binary_search_by_key(&v, |p| p.0)
                .ok()
                .and_then(|i| vertex_of.get(i))
                .map(|p| p.1)
        };
        for (i, edge) in edges.iter_mut().enumerate() {
            let Ok(i) = u32::try_from(i) else { break };
            if edge.closed {
                continue;
            }
            let ends = edge
                .points
                .first()
                .and_then(|&a| vertex_id(a))
                .zip(edge.points.last().and_then(|&b| vertex_id(b)));
            if let Some((a, b)) = ends {
                edge.ends = Some([a, b]);
                for id in [a, b] {
                    if let Some(vx) = vertices.get_mut(id as usize)
                        && vx.edges.last() != Some(&i)
                    {
                        vx.edges.push(i);
                    }
                }
            }
        }

        // The faces.
        let mut faces: Vec<Face> = (0..count)
            .map(|_| Face {
                triangles: Vec::new(),
                kind: FaceKind::Curved,
                area: 0.0,
                bounds: None,
                edges: Vec::new(),
            })
            .collect();
        for (t, &f) in tri_face.iter().enumerate() {
            let (Ok(t), Some(face)) = (u32::try_from(t), faces.get_mut(f as usize)) else {
                continue;
            };
            face.triangles.push(t);
            if let Some([a, b, c]) = mesh.triangle(t as usize) {
                face.area += (b - a).cross(c - a).length() / 2.0;
                let tb = Aabb {
                    min: a.min(b).min(c),
                    max: a.max(b).max(c),
                };
                face.bounds = Some(face.bounds.map_or(tb, |fb| fb.union(&tb)));
            }
        }
        for face in &mut faces {
            face.kind = fit::plane(mesh, &face.triangles, tol);
        }
        for (i, edge) in edges.iter().enumerate() {
            let Ok(i) = u32::try_from(i) else { break };
            for &f in &edge.faces {
                if let Some(face) = faces.get_mut(f as usize) {
                    face.edges.push(i);
                }
            }
        }

        Self {
            source,
            tri_face,
            faces,
            edges,
            vertices,
            size,
        }
    }

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

fn pair(w: &[DVec3]) -> Option<(DVec3, DVec3)> {
    match w {
        [a, b] => Some((*a, *b)),
        _ => None,
    }
}

fn segment_length(w: &[DVec3]) -> f64 {
    pair(w).map_or(0.0, |(a, b)| a.distance(b))
}

/// A closed chain turned to start at its smallest point and run towards the
/// smaller of that point's neighbours, so it reads the same whichever mesh edge it
/// was found from.
fn start_at_smallest(mut points: Vec<u32>, at: &impl Fn(u32) -> DVec3) -> Vec<u32> {
    let Some(first) = points
        .iter()
        .enumerate()
        .min_by(|(i, a), (j, b)| cmp_points(at(**a), at(**b)).then(i.cmp(j)))
        .map(|(i, _)| i)
    else {
        return points;
    };
    points.rotate_left(first);
    if let (Some(&next), Some(&prev)) = (points.get(1), points.last())
        && points.len() > 2
        && cmp_points(at(prev), at(next)).is_lt()
        && let Some(tail) = points.get_mut(1..)
    {
        tail.reverse();
    }
    points
}

#[cfg(test)]
mod tests;
