//! Fewer triangles on faces ruled by straight lines: cylinders and cones.
//!
//! How far a triangle on a cylinder or cone strays from it depends only on how far
//! round the axis it reaches, not on how long it is along the rulings. ogeom 0.10
//! still lays rows of points along the rulings, holding each cell of its grid to
//! six times as tall as it is wide, and those rows hold no part of the tolerance:
//! on the shop fixtures, cylinders and cones carry 83 to 93 percent of the
//! triangles (gilbertorconde/ogeom-rs#213). So such a face is triangulated again
//! from its boundary points alone, constrained Delaunay in its own parameters.
//!
//! Nothing here is trusted: the points are ogeom's own, unmoved, so the faces either
//! side of an edge still meet, and the importer measures the new triangles against
//! the surface as it does ogeom's, keeping ogeom's mesh for a face whose new
//! triangles stray further than the tolerance. Delete this once ogeom meshes
//! ruled faces without the rows.

use std::collections::BTreeMap;

use ogeom_topo::Triangulation;
use spade::handles::FixedVertexHandle;
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation as _};

/// `mesh`'s triangles laid again on its boundary points alone, or `None` where the
/// face's boundary cannot be followed (not a closed run of edges in its parameters,
/// or crossing itself there). The positions are `mesh`'s, so its indices still
/// hold; interior points are left unused.
pub(crate) fn from_boundary(mesh: &Triangulation) -> Option<Vec<[u32; 3]>> {
    if mesh.parameters.len() != mesh.positions.len() || mesh.triangles.is_empty() {
        return None;
    }
    let boundary = boundary_edges(&mesh.triangles)?;
    let (su, sv) = chart_scale(mesh)?;
    let chart = |i: u32| {
        let (u, v) = *mesh.parameters.get(i as usize)?;
        Some(Point2::new(u * su, v * sv))
    };
    let area = |t: &[u32; 3]| -> Option<f64> {
        let [a, b, c] = [chart(t[0])?, chart(t[1])?, chart(t[2])?];
        Some((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x))
    };
    // Which way round ogeom's triangles go in the chart, so the new ones go the
    // same way and face out of the part as the old ones did.
    let mut turning = 0.0;
    for t in &mesh.triangles {
        turning += area(t)?;
    }
    if turning == 0.0 || !turning.is_finite() {
        return None;
    }

    let mut cdt = ConstrainedDelaunayTriangulation::<Point2<f64>>::new();
    let mut handle = BTreeMap::new();
    let mut index_of: BTreeMap<FixedVertexHandle, u32> = BTreeMap::new();
    for &(a, b) in &boundary {
        for i in [a, b] {
            if let std::collections::btree_map::Entry::Vacant(slot) = handle.entry(i) {
                let h = cdt.insert(chart(i)?).ok()?;
                // Two boundary points at one place in the chart are one point.
                index_of.entry(h).or_insert(i);
                slot.insert(h);
            }
        }
    }
    for (a, b) in &boundary {
        let (ha, hb) = (*handle.get(a)?, *handle.get(b)?);
        if ha != hb && cdt.try_add_constraint(ha, hb).is_empty() {
            return None;
        }
    }

    // Inside is where an odd number of the boundary's edges lie between a
    // triangle and the outside, found by walking in from the hull. A closed
    // boundary gives every triangle one answer whichever way it is reached; two
    // answers mean it is not closed, and the face is left as ogeom drew it.
    let mut inside: Vec<Option<bool>> = vec![None; cdt.num_all_faces()];
    let mut todo = Vec::new();
    for face in cdt.inner_faces() {
        for edge in face.adjacent_edges() {
            if edge.rev().face().is_outer() {
                let odd = cdt.is_constraint_edge(edge.as_undirected().fix());
                if !settle(&mut inside, face.fix().index(), odd)? {
                    todo.push(face.fix());
                }
            }
        }
    }
    while let Some(fixed) = todo.pop() {
        let here = (*inside.get(fixed.index())?)?;
        for edge in cdt.face(fixed).adjacent_edges() {
            let Some(next) = edge.rev().face().as_inner() else {
                continue;
            };
            let crossed = cdt.is_constraint_edge(edge.as_undirected().fix());
            if !settle(&mut inside, next.fix().index(), here != crossed)? {
                todo.push(next.fix());
            }
        }
    }

    let mut out = Vec::new();
    for face in cdt.inner_faces() {
        if inside.get(face.fix().index()) != Some(&Some(true)) {
            continue;
        }
        let [a, b, c] = face.vertices().map(|v| index_of.get(&v.fix()).copied());
        let mut t = [a?, b?, c?];
        if area(&t)?.signum() != turning.signum() {
            t.swap(1, 2);
        }
        out.push(t);
    }
    (!out.is_empty()).then_some(out)
}

/// Record that face `index` is inside or not. `Some(false)` if it was not known
/// yet, `Some(true)` if it was and agrees, `None` if it disagrees.
fn settle(inside: &mut [Option<bool>], index: usize, odd: bool) -> Option<bool> {
    let slot = inside.get_mut(index)?;
    match *slot {
        Some(was) => (was == odd).then_some(true),
        None => {
            *slot = Some(odd);
            Some(false)
        }
    }
}

/// The face's boundary: each edge of its triangles that no other of them shares,
/// as `(from, to)` by vertex. `None` if an edge is shared by more than two.
fn boundary_edges(triangles: &[[u32; 3]]) -> Option<Vec<(u32, u32)>> {
    let mut uses: BTreeMap<(u32, u32), (u32, (u32, u32))> = BTreeMap::new();
    for t in triangles {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let key = (a.min(b), a.max(b));
            let entry = uses.entry(key).or_insert((0, (a, b)));
            entry.0 += 1;
        }
    }
    let mut out = Vec::new();
    for (n, edge) in uses.into_values() {
        match n {
            1 => out.push(edge),
            2 => {}
            _ => return None,
        }
    }
    Some(out)
}

/// How long a unit of each parameter is on the surface, on average over the face:
/// fitted to its triangles' sides, `|dp|^2 ~ su^2 du^2 + sv^2 dv^2`, which holds on
/// a cylinder or cone, whose parameters run square to each other. Scaling the
/// chart by it keeps the Delaunay triangles from reaching far round the axis
/// to join points that are near only in the parameters.
fn chart_scale(mesh: &Triangulation) -> Option<(f64, f64)> {
    let (mut uu, mut uv, mut vv, mut pu, mut pv) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for t in &mesh.triangles {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (pa, pb) = (
                mesh.positions.get(a as usize)?,
                mesh.positions.get(b as usize)?,
            );
            let (qa, qb) = (
                mesh.parameters.get(a as usize)?,
                mesh.parameters.get(b as usize)?,
            );
            let d = *pb - *pa;
            let length = d.dot(d);
            let (du, dv) = ((qb.0 - qa.0).powi(2), (qb.1 - qa.1).powi(2));
            uu += du * du;
            uv += du * dv;
            vv += dv * dv;
            pu += du * length;
            pv += dv * length;
        }
    }
    let det = uu * vv - uv * uv;
    if det.abs() <= f64::EPSILON * uu * vv {
        return None;
    }
    let su2 = (pu * vv - pv * uv) / det;
    let sv2 = (pv * uu - pu * uv) / det;
    (su2 > 0.0 && sv2 > 0.0).then(|| (su2.sqrt(), sv2.sqrt()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use ogeom_math::Point;

    /// A quarter of a cylinder of radius 10, 40 long, drawn as ogeom draws one:
    /// its rims in `columns` chords, its straight sides from end to end, and rows
    /// of points between that hold no part of the tolerance. Triangulated here by
    /// Delaunay in the chart, as any mesh of the face would do.
    fn quarter_cylinder(columns: u32, rows: u32) -> Triangulation {
        let mut mesh = Triangulation::new();
        let mut add = |i: u32, j: u32| {
            let u = std::f64::consts::FRAC_PI_2 * f64::from(i) / f64::from(columns);
            let v = 40.0 * f64::from(j) / f64::from(rows);
            mesh.positions
                .push(Point::new(10.0 * u.cos(), 10.0 * u.sin(), v));
            mesh.parameters.push((u, v));
        };
        for j in 0..=rows {
            for i in 0..=columns {
                let side = i == 0 || i == columns;
                if j == 0 || j == rows || !side {
                    add(i, j);
                }
            }
        }
        let mut cdt = ConstrainedDelaunayTriangulation::<Point2<f64>>::new();
        let mut index = BTreeMap::new();
        for (i, &(u, v)) in mesh.parameters.iter().enumerate() {
            let h = cdt.insert(Point2::new(u * 10.0, v)).unwrap();
            index.insert(h, u32::try_from(i).unwrap());
        }
        for face in cdt.inner_faces() {
            let t = face.vertices().map(|v| index[&v.fix()]);
            mesh.triangles.push(t);
        }
        mesh
    }

    fn interior(mesh: &Triangulation, i: u32) -> bool {
        let (u, v) = mesh.parameters[i as usize];
        u > 1e-9 && u < std::f64::consts::FRAC_PI_2 - 1e-9 && v > 1e-9 && v < 40.0 - 1e-9
    }

    fn turning(mesh: &Triangulation, t: [u32; 3]) -> f64 {
        let p = |i: u32| mesh.parameters[i as usize];
        let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
        (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
    }

    /// The rows go: what is left is the boundary's own points, as few triangles as
    /// a polygon of them takes, each turning the way the face's did, and none
    /// reaching further round the axis than one chord of the rims.
    #[test]
    fn a_cylinder_keeps_only_its_boundary() {
        let (columns, rows) = (12, 30);
        let mesh = quarter_cylinder(columns, rows);
        assert!(mesh.triangles.len() > 600, "{}", mesh.triangles.len());
        let fewer = from_boundary(&mesh).unwrap();
        let boundary = 2 * (columns as usize + 1);
        assert_eq!(fewer.len(), boundary - 2);
        let column = std::f64::consts::FRAC_PI_2 / f64::from(columns);
        for &t in &fewer {
            assert!(!t.iter().any(|&i| interior(&mesh, i)), "{t:?}");
            assert!(turning(&mesh, t) > 0.0, "{t:?} turns the other way");
            let us = t.map(|i| mesh.parameters[i as usize].0);
            let reach = us.iter().copied().fold(f64::MIN, f64::max)
                - us.iter().copied().fold(f64::MAX, f64::min);
            assert!(reach <= column * (1.0 + 1e-9), "{t:?} reaches {reach}");
        }
    }

    /// Wound the other way in the chart, as a reversed face is, the new triangles
    /// follow.
    #[test]
    fn the_winding_follows_the_face() {
        let mut mesh = quarter_cylinder(6, 10);
        let ogeoms = mesh.triangles.first().map(|&t| turning(&mesh, t)).unwrap();
        if ogeoms > 0.0 {
            mesh.triangles.iter_mut().for_each(|t| t.swap(1, 2));
        }
        let fewer = from_boundary(&mesh).unwrap();
        assert!(fewer.iter().all(|&t| turning(&mesh, t) < 0.0));
    }

    /// A boundary that does not close is not guessed at.
    #[test]
    fn an_edge_shared_three_ways_is_refused() {
        let mut mesh = quarter_cylinder(2, 2);
        let t = mesh.triangles[0];
        mesh.positions.push(Point::new(0.0, 0.0, 99.0));
        mesh.parameters.push((9.0, 9.0));
        let extra = u32::try_from(mesh.positions.len() - 1).unwrap();
        mesh.triangles.push([t[0], t[1], extra]);
        mesh.triangles.push([t[1], t[0], extra]);
        assert_eq!(from_boundary(&mesh), None);
    }
}
