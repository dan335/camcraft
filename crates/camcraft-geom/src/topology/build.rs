//! Building a topology from a mesh and its face labels: the edges between faces,
//! the vertices where they end, and each face's shape.

use super::{Edge, Face, FaceKind, FaceSource, Topology, Vertex, fit, fit_tolerance, pair};
use crate::mesh::TriMesh;
use glam::DVec3;
use std::cmp::Ordering;

/// A chain of edges turning sharper than this, in degrees, is split in two there.
const KINK_DEGREES: f64 = 30.0;

/// The diagonal of the mesh's bounds.
pub(super) fn diagonal(mesh: &TriMesh) -> f64 {
    mesh.bounds().map_or(0.0, |b| b.extent().length())
}

/// One side of one triangle, keyed by its corners in ascending order.
#[derive(Debug, Clone, Copy)]
pub(super) struct Side {
    pub(super) lo: u32,
    pub(super) hi: u32,
    pub(super) tri: u32,
    /// Whether the triangle runs this side from `lo` to `hi`.
    pub(super) forward: bool,
}

/// Every side of every triangle, sorted so sides shared by several triangles are
/// together. Triangles that repeat a corner have no sides.
pub(super) fn sides(mesh: &TriMesh) -> Vec<Side> {
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

/// The mesh edges that bound faces: every side, of `sides`, not shared by exactly
/// two triangles of one face.
fn features(sides: &[Side], tri_face: &[u32]) -> Vec<Feature> {
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
        }
        if let Some(tb) = mesh.triangle_bounds(t as usize) {
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
    pub(super) fn build(
        mesh: &TriMesh,
        tri_face: Vec<u32>,
        count: u32,
        source: FaceSource,
    ) -> Self {
        Self::build_with(mesh, &sides(mesh), diagonal(mesh), tri_face, count, source)
    }

    /// As [`build`](Self::build), given the mesh's [`sides`] and the [`diagonal`]
    /// of its bounds, `size`.
    pub(super) fn build_with(
        mesh: &TriMesh,
        sides: &[Side],
        size: f64,
        tri_face: Vec<u32>,
        count: u32,
        source: FaceSource,
    ) -> Self {
        let tol = fit_tolerance(size);
        let features = Features::new(features(sides, &tri_face));
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
