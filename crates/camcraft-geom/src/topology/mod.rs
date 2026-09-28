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

mod derive;
mod fit;

pub use derive::DeriveParams;

use crate::mesh::{Aabb, TriMesh, closest_point_on_segment, closest_point_on_triangle};
use glam::{DAffine3, DVec3};
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
        Self::derived_with(mesh, &DeriveParams::default())
    }

    /// As [`derived`](Body::derived), with its settings given.
    pub fn derived_with(mesh: TriMesh, params: &DeriveParams) -> Self {
        let size = diagonal(&mesh);
        let sides = sides(&mesh);
        let (labels, count) = derive::faces(&mesh, &sides, params, fit_tolerance(size), size);
        let topology = Topology::build(&mesh, labels, count, FaceSource::Derived);
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

/// Where mesh vertex `v` is, or NaN for a vertex the mesh does not have.
fn point(mesh: &TriMesh, v: u32) -> DVec3 {
    mesh.vertices()
        .get(v as usize)
        .copied()
        .unwrap_or(DVec3::NAN)
}

/// The mesh edges that bound faces: every side not shared by exactly two
/// triangles of one face.
fn features(mesh: &TriMesh, tri_face: &[u32]) -> Vec<Feature> {
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
    features
}

/// The features, and which of them meet at each mesh vertex.
struct Features {
    list: Vec<Feature>,
    /// `(mesh vertex, feature)` for both ends of every feature, sorted by vertex.
    incidence: Vec<(u32, u32)>,
}

/// The mesh vertices where chains end.
struct Corners {
    /// Whether each mesh vertex is one.
    flags: Vec<bool>,
    /// The corners, in order of mesh vertex.
    list: Vec<u32>,
}

impl Corners {
    /// Whether mesh vertex `v` is a corner.
    fn contains(&self, v: u32) -> bool {
        self.flags.get(v as usize).copied().unwrap_or(false)
    }
}

/// A chain of features, before it is an edge.
struct Chain {
    points: Vec<u32>,
    closed: bool,
    /// One of its features; they all run along the same faces.
    feature: u32,
}

impl Features {
    /// `list`, with which features meet at each mesh vertex, sorted by vertex.
    fn new(list: Vec<Feature>) -> Self {
        let mut incidence: Vec<(u32, u32)> = Vec::with_capacity(list.len() * 2);
        for (i, f) in list.iter().enumerate() {
            let Ok(i) = u32::try_from(i) else { break };
            incidence.push((f.a, i));
            incidence.push((f.b, i));
        }
        incidence.sort_unstable();
        Self { list, incidence }
    }

    /// Feature `e`.
    fn get(&self, e: u32) -> Option<&Feature> {
        self.list.get(e as usize)
    }

    /// The features that meet at mesh vertex `v`, as `(v, feature)`.
    fn incident(&self, v: u32) -> &[(u32, u32)] {
        let lo = self.incidence.partition_point(|p| p.0 < v);
        let hi = self.incidence.partition_point(|p| p.0 <= v);
        self.incidence.get(lo..hi).unwrap_or(&[])
    }

    /// The vertices, where chains end: every mesh vertex where other than two
    /// features meet, or two that run along different faces or turn sharper than
    /// [`KINK_DEGREES`].
    fn corners(&self, mesh: &TriMesh) -> Corners {
        let at = |v: u32| point(mesh, v);
        let kink = KINK_DEGREES.to_radians().cos();
        let mut corner = vec![false; mesh.vertices().len()];
        let mut corners = Vec::new();
        for group in self.incidence.chunk_by(|x, y| x.0 == y.0) {
            let Some(&(v, _)) = group.first() else {
                continue;
            };
            let is_corner = match group {
                [(_, e1), (_, e2)] => match (self.get(*e1), self.get(*e2)) {
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
        Corners {
            flags: corner,
            list: corners,
        }
    }

    /// Walk the chains: from each vertex first, then the loops that are left.
    fn chains(&self, mesh: &TriMesh, corners: &Corners) -> Vec<Chain> {
        let at = |v: u32| point(mesh, v);
        let mut used = vec![false; self.list.len()];
        let mut chains: Vec<Chain> = Vec::new();
        for &v in &corners.list {
            for &(_, e) in self.incident(v) {
                if used.get(e as usize).copied().unwrap_or(true) {
                    continue;
                }
                let mut points = self.walk(corners, v, e, &mut used);
                if let (Some(&a), Some(&b)) = (points.first(), points.last())
                    && cmp_points(at(b), at(a)).is_lt()
                {
                    points.reverse();
                }
                chains.push(Chain {
                    points,
                    closed: false,
                    feature: e,
                });
            }
        }
        for e in 0..self.list.len() {
            let Ok(e) = u32::try_from(e) else { break };
            if used.get(e as usize).copied().unwrap_or(true) {
                continue;
            }
            let Some(f) = self.get(e) else { continue };
            let points = self.walk(corners, f.a, e, &mut used);
            chains.push(Chain {
                points: start_at_smallest(points, &at),
                closed: true,
                feature: e,
            });
        }
        chains
    }

    /// A chain from `start` along feature `first`, to the next vertex, or back
    /// round to `start` when it is not one. The features it takes are marked in
    /// `used`.
    fn walk(&self, corners: &Corners, start: u32, first: u32, used: &mut [bool]) -> Vec<u32> {
        let mut points = vec![start];
        let (mut v, mut e) = (start, first);
        loop {
            if let Some(u) = used.get_mut(e as usize) {
                *u = true;
            }
            let Some(f) = self.get(e) else { break };
            let next = f.other(v);
            if corners.contains(next) {
                points.push(next);
                break;
            }
            if next == start {
                break;
            }
            points.push(next);
            let Some(&(_, ne)) = self
                .incident(next)
                .iter()
                .find(|(_, x)| *x != e && !used.get(*x as usize).copied().unwrap_or(true))
            else {
                break;
            };
            (v, e) = (next, ne);
        }
        points
    }
}

/// The edges, one per chain, in order: by the faces they separate, then their
/// first points. Their ends are not tied to vertices yet.
fn edges(mesh: &TriMesh, features: &Features, chains: Vec<Chain>, tol: f64) -> Vec<Edge> {
    let at = |v: u32| point(mesh, v);
    let mut edges: Vec<Edge> = chains
        .into_iter()
        .map(|chain| {
            let Chain {
                points,
                closed,
                feature: e,
            } = chain;
            let faces = features.get(e).map(|f| f.faces.clone()).unwrap_or_default();
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
    edges
}

/// The vertices, one per corner, in order of position; and `edges` tied to the
/// vertices at their ends, and each vertex to the edges that end there.
fn vertices(mesh: &TriMesh, mut corners: Vec<u32>, edges: &mut [Edge]) -> Vec<Vertex> {
    let at = |v: u32| point(mesh, v);
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
    vertices
}

/// The faces, `count` of them: their triangles as `tri_face` names them, their
/// area, bounds and shape, and the edges around them.
fn faces(mesh: &TriMesh, tri_face: &[u32], count: u32, edges: &[Edge], tol: f64) -> Vec<Face> {
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
    faces
}

impl Topology {
    /// The topology of `mesh`, whose triangles lie on the faces `tri_face` names,
    /// out of `count`.
    fn build(mesh: &TriMesh, tri_face: Vec<u32>, count: u32, source: FaceSource) -> Self {
        let size = diagonal(mesh);
        let tol = fit_tolerance(size);
        let features = Features::new(features(mesh, &tri_face));
        let corners = features.corners(mesh);
        let chains = features.chains(mesh, &corners);
        let mut edges = edges(mesh, &features, chains, tol);
        let vertices = vertices(mesh, corners.list, &mut edges);
        let faces = faces(mesh, &tri_face, count, &edges, tol);
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
