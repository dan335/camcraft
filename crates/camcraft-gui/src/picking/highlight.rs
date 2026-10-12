//! What is drawn over the part for what is under the pointer, and for what could
//! be picked into the setup: faces, edges, points and boxes, in the setup's frame.

use camcraft_core::job::Snap;
use camcraft_geom::mesh::Aabb;
use camcraft_geom::topology::Body;
use glam::{DVec3, Vec3};

use crate::picking::ray::{PickModel, PickScene, Target};
use crate::picking::slots::Slot;
use crate::view::scene::box_edges;
use crate::view::viewport::{Highlight, Outline, OutlineKind};

/// What is under the pointer, drawn over the part.
pub fn hovered(scene: &PickScene, target: &Target) -> Highlight {
    let mut h = Highlight::default();
    let to_setup = |p: DVec3| scene.frame.to_setup(p);
    match target {
        Target::Face { model, face, .. } => {
            if let Some(pm) = scene.model(model) {
                h.faces = face_triangles(pm.body(), *face, &to_setup).collect();
            }
        }
        Target::Edge { model, edge, .. } => {
            if let Some(pm) = scene.model(model) {
                h.lines = model_edge_lines(pm, *edge, OutlineKind::Hover);
            }
        }
        Target::Loop { model, edges } => {
            if let Some(pm) = scene.model(model) {
                for &(e, _) in edges {
                    h.lines.extend(model_edge_lines(pm, e, OutlineKind::Hover));
                }
            }
        }
        Target::Point { setup, point } => {
            h.markers.push(*setup);
            // A circle's centre is shown with its circle, so it is clear which.
            if point.snap == Snap::CircleCenter
                && let Some(pm) = scene.model(&point.model)
            {
                h.lines = edge_lines(pm.edges.get(point.index as usize), true, OutlineKind::Hover);
            }
        }
        Target::StockPoint { setup, .. } => h.markers.push(*setup),
        Target::Model { model } => {
            if let Some(pm) = scene.model(model) {
                h.faces = model_triangles(pm.body(), &to_setup).collect();
            }
        }
        Target::Stock { stock } => {
            if let Some(s) = scene.stocks.iter().find(|s| &s.name == stock) {
                h.lines = box_lines(&s.bounds, OutlineKind::Hover);
            }
        }
    }
    h
}

/// What could be picked into the setup for `slot` but is not in it: the job's
/// other models, or its other stocks, drawn faintly where the setup would put
/// them.
pub fn ghosts(scene: &PickScene, slot: &Slot) -> Highlight {
    let mut h = Highlight::default();
    let to_setup = |p: DVec3| scene.frame.to_setup(p);
    match slot {
        Slot::Models { .. } => {
            for m in scene.models.iter().filter(|m| !m.in_setup) {
                h.faces.extend(model_triangles(m.body(), &to_setup));
            }
        }
        Slot::Stocks { .. } => {
            for s in scene.stocks.iter().filter(|s| !s.in_setup) {
                h.lines.extend(box_lines(&s.bounds, OutlineKind::Ghost));
            }
        }
        Slot::WorkZero { .. } | Slot::Faces { .. } | Slot::Edges { .. } => {}
    }
    h
}

/// Every triangle of `body`, in the setup's frame.
fn model_triangles<'a>(
    body: &'a Body,
    to_setup: &'a impl Fn(DVec3) -> DVec3,
) -> impl Iterator<Item = [Vec3; 3]> + 'a {
    let mesh = body.mesh();
    (0..mesh.len())
        .filter_map(|t| mesh.triangle(t))
        .map(move |t| t.map(|p| to_setup(p).as_vec3()))
}

/// A box's twelve edges.
fn box_lines(b: &Aabb, kind: OutlineKind) -> Vec<Outline> {
    box_edges(b)
        .into_iter()
        .map(|(a, b)| Outline { a, b, kind })
        .collect()
}

/// The triangles of face `face` of `body`, in the setup's frame.
pub(super) fn face_triangles<'a>(
    body: &'a Body,
    face: u32,
    to_setup: &'a impl Fn(DVec3) -> DVec3,
) -> impl Iterator<Item = [Vec3; 3]> + 'a {
    body.topology()
        .face(face)
        .map_or(&[][..], |f| f.triangles.as_slice())
        .iter()
        .filter_map(|&t| body.mesh().triangle(t as usize))
        .map(move |t| t.map(|p| to_setup(p).as_vec3()))
}

/// Edge `edge` of `pm` as lines, closed where it closes on itself.
pub(super) fn model_edge_lines(pm: &PickModel, edge: u32, kind: OutlineKind) -> Vec<Outline> {
    let closed = pm.body().topology().edge(edge).is_some_and(|e| e.closed);
    edge_lines(pm.edges.get(edge as usize), closed, kind)
}

/// An edge's `points` as lines, joined back to the first when `closed`.
fn edge_lines(points: Option<&Vec<DVec3>>, closed: bool, kind: OutlineKind) -> Vec<Outline> {
    let Some(points) = points else {
        return Vec::new();
    };
    let mut out: Vec<Outline> = points
        .windows(2)
        .filter_map(|w| match w {
            [a, b] => Some(Outline { a: *a, b: *b, kind }),
            _ => None,
        })
        .collect();
    if closed
        && points.len() > 2
        && let (Some(&a), Some(&b)) = (points.last(), points.first())
    {
        out.push(Outline { a, b, kind });
    }
    out
}
