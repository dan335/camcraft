//! Picking into the job: which property a click in the view fills, what it takes,
//! what a click does to the job, and what the view and the panel show of the picks
//! already made.
//!
//! A property that takes picks is a *slot*: a setup's work zero, an operation's
//! faces or its edges. Its Select button arms the view for it, so the view offers
//! only what the slot takes; a click then writes straight into the job, where it is
//! one step to undo like any other edit. A work zero takes one point and is done; a
//! list of faces or edges takes any number, a second click on one taking it out, and
//! is done when the person says so.
//!
//! What is shown of a pick is found the same way the engine finds it
//! ([`camcraft_core::features`]), so a face lit in the view is the face that will be
//! cut, and a pick that can no longer be found says so here before it stops a run.

use camcraft_core::features::{self, FeatureError};
use camcraft_core::job::{BoxOf, FeatureRef, Geometry, Origin, Snap};
use camcraft_core::run::cutting_tool;
use camcraft_core::setup::{Bodies, Prepared};
use camcraft_core::tool::ToolProfile;
use camcraft_geom::topology::{Body, EdgeKind, FaceKind};
use glam::{DVec3, Vec3};

use crate::document::{Document, Selection};
use crate::measure::Measure;
use crate::pick::{Accept, PickScene, Target, describe_box_point, describe_circle};
use crate::viewport::{Highlight, Outline, OutlineKind};

/// A property a click in the view can fill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slot {
    /// Setup `setup`'s work zero: a point on a model or on the stock's box.
    WorkZero {
        /// The setup, by name.
        setup: String,
    },
    /// The faces operation `op` of setup `setup` is kept to.
    Faces {
        /// The setup, by name.
        setup: String,
        /// The operation's place in it.
        op: usize,
    },
    /// The edges operation `op` of setup `setup` is kept to.
    Edges {
        /// The setup, by name.
        setup: String,
        /// The operation's place in it.
        op: usize,
    },
}

impl Slot {
    /// What a click may land on while this is being filled.
    pub fn accept(&self) -> Accept {
        match self {
            Slot::WorkZero { .. } => Accept::POINTS,
            Slot::Faces { .. } => Accept::FACES,
            Slot::Edges { .. } => Accept::EDGES,
        }
    }

    /// Whether one pick fills it, so it is done after the first click.
    pub fn single(&self) -> bool {
        matches!(self, Slot::WorkZero { .. })
    }

    /// The setup it belongs to.
    pub fn setup(&self) -> &str {
        match self {
            Slot::WorkZero { setup } | Slot::Faces { setup, .. } | Slot::Edges { setup, .. } => {
                setup
            }
        }
    }

    /// What the view says while it is armed.
    pub fn hint(&self) -> &'static str {
        match self {
            Slot::WorkZero { .. } => {
                "Click a corner, a round edge for its centre, or a point on the stock · Esc to stop"
            }
            Slot::Faces { .. } => "Click faces to add or remove them · Esc or Done to finish",
            Slot::Edges { .. } => {
                "Click edges to add or remove them; they must close into loops · Esc or Done to finish"
            }
        }
    }

    /// Whether it still makes sense to be filling it: its setup and operation are
    /// still there, and still what the panel shows. Undoing past an operation, or
    /// selecting something else, stops picking into it.
    pub fn still_valid(&self, doc: &Document) -> bool {
        let Some(i) = doc.setup_index(self.setup()) else {
            return false;
        };
        match self {
            Slot::WorkZero { setup } => doc.selection == Selection::Setup(setup.clone()),
            Slot::Faces { setup, op } | Slot::Edges { setup, op } => {
                doc.job
                    .setups
                    .get(i)
                    .is_some_and(|s| *op < s.operations.len())
                    && doc.selection == Selection::Operation(setup.clone(), *op)
            }
        }
    }
}

/// The picks a slot holds, as they stand in the job.
fn geometry_mut<'a>(doc: &'a mut Document, setup: &str, op: usize) -> Option<&'a mut Geometry> {
    let i = doc.setup_index(setup)?;
    Some(&mut doc.job.setups.get_mut(i)?.operations.get_mut(op)?.geometry)
}

/// Write a click on `target` into the job, for `slot`. Returns whether anything
/// changed: a click on something the slot does not take changes nothing.
///
/// A face or edge already in the list is taken out instead, as picking works in
/// CAD; `current` says which feature a saved pick finds now, so one saved before
/// the model was renumbered is still recognised as the same.
pub fn apply(
    doc: &mut Document,
    slot: &Slot,
    target: &Target,
    current: impl Fn(&FeatureRef, Kind) -> Option<u32>,
) -> bool {
    match (slot, target) {
        (Slot::WorkZero { setup }, Target::Point { point, .. }) => {
            set_origin(doc, setup, |offset| Origin::Point {
                point: point.clone(),
                offset,
            })
        }
        (Slot::WorkZero { setup }, &Target::StockPoint { x, y, z, .. }) => {
            set_origin(doc, setup, |offset| Origin::BoxPoint {
                of: BoxOf::Stock,
                x,
                y,
                z,
                offset,
            })
        }
        (Slot::Faces { setup, op }, Target::Face { model, face, at }) => {
            let Some(g) = geometry_mut(doc, setup, *op) else {
                return false;
            };
            toggle(&mut g.faces, model, *face, *at, |r| current(r, Kind::Face))
        }
        (Slot::Edges { setup, op }, Target::Edge { model, edge, at }) => {
            let Some(g) = geometry_mut(doc, setup, *op) else {
                return false;
            };
            toggle(&mut g.edges, model, *edge, *at, |r| current(r, Kind::Edge))
        }
        _ => false,
    }
}

/// Whether a saved pick is a face or an edge, for finding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A face.
    Face,
    /// An edge.
    Edge,
}

fn set_origin(doc: &mut Document, setup: &str, make: impl FnOnce([f64; 3]) -> Origin) -> bool {
    let Some(i) = doc.setup_index(setup) else {
        return false;
    };
    let Some(s) = doc.job.setups.get_mut(i) else {
        return false;
    };
    // The offset stays: it is the person's, measured from wherever zero is.
    let origin = make(s.origin.offset());
    let changed = s.origin != origin;
    s.origin = origin;
    changed
}

fn toggle(
    list: &mut Vec<FeatureRef>,
    model: &str,
    index: u32,
    at: DVec3,
    current: impl Fn(&FeatureRef) -> Option<u32>,
) -> bool {
    let same = |r: &FeatureRef| r.model == model && current(r) == Some(index);
    if list.iter().any(same) {
        list.retain(|r| !same(r));
    } else {
        list.push(FeatureRef {
            model: model.to_string(),
            index,
            at: at.to_array(),
        });
    }
    true
}

/// What the view and the panel show of the picks for what is selected: a setup's
/// picked work zero, or an operation's faces, edges and the boundary they make.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shown {
    /// Drawn over the part.
    pub highlight: Highlight,
    /// The work zero's point: what it is, or why it cannot be found.
    pub origin: Option<Result<String, String>>,
    /// Each picked face, in the job's order: what it is, or why it cannot be
    /// found.
    pub faces: Vec<Result<String, String>>,
    /// Each picked edge, likewise.
    pub edges: Vec<Result<String, String>>,
    /// The boundary the picks make, or why they make none.
    pub boundary: Option<Result<(), String>>,
}

/// The picks of `setup`, and of its operation `op` when one is selected, as they
/// stand on the models laid out in `prepared`.
pub fn shown(
    doc: &Document,
    setup: &str,
    op: Option<usize>,
    prepared: &Prepared,
    scene: &PickScene,
    m: Measure,
) -> Shown {
    let mut out = Shown::default();
    let Some(s) = doc.setup_index(setup).and_then(|i| doc.job.setups.get(i)) else {
        return out;
    };
    let bodies: Bodies<'_> = scene.models.iter().map(|m| (m.name(), m.body())).collect();
    let tol = |body: &Body| features::tolerance(doc.job.tolerance, body);
    let to_setup = |p: DVec3| scene.frame.to_setup(p);

    if let Origin::Point { point, .. } = &s.origin {
        out.origin = Some(match bodies.get(point.model.as_str()) {
            Some(body) => match features::find_point(body, point, tol(body)) {
                Ok((p, found)) => {
                    out.highlight.markers.push(to_setup(p));
                    Ok(describe_point(
                        body,
                        &point.model,
                        point.snap,
                        found.index,
                        m,
                    ))
                }
                Err(e) => Err(e.to_string()),
            },
            None => Err(FeatureError::NotLoaded(point.model.clone()).to_string()),
        });
    }

    let Some(operation) = op.and_then(|i| s.operations.get(i)) else {
        return out;
    };
    let g = &operation.geometry;
    for r in &g.faces {
        out.faces.push(match bodies.get(r.model.as_str()) {
            Some(body) => match features::find_face(body, r, tol(body)) {
                Ok(found) => {
                    out.highlight
                        .faces
                        .extend(face_triangles(body, found.index, &to_setup));
                    Ok(describe_face(body, &r.model, found.index, m))
                }
                Err(e) => Err(e.to_string()),
            },
            None => Err(FeatureError::NotLoaded(r.model.clone()).to_string()),
        });
    }
    for r in &g.edges {
        out.edges.push(match bodies.get(r.model.as_str()) {
            Some(body) => match features::find_edge(body, r, tol(body)) {
                Ok(found) => {
                    if let Some(pm) = scene.model(&r.model) {
                        out.highlight.lines.extend(edge_lines(
                            pm.edges.get(found.index as usize),
                            body.topology().edge(found.index).is_some_and(|e| e.closed),
                            OutlineKind::Selected,
                        ));
                    }
                    Ok(describe_edge(body, &r.model, found.index, m))
                }
                Err(e) => Err(e.to_string()),
            },
            None => Err(FeatureError::NotLoaded(r.model.clone()).to_string()),
        });
    }

    // The boundary, drawn at the stock's top so it reads over everything. Worked
    // out as the run will work it out, with the operation's own tool.
    if g.picked() && out.faces.iter().chain(&out.edges).all(Result::is_ok) {
        let radius = cutting_tool(&doc.job, operation).map_or(0.0, |t| t.max_radius());
        let z = prepared.stock_bounds().max.z;
        out.boundary = Some(
            match features::machining_boundary(g, prepared, &bodies, radius, doc.job.tolerance) {
                Ok(Some(region)) => {
                    for ring in region.rings() {
                        let pts = ring.points();
                        let next = pts.iter().skip(1).chain(pts.first());
                        out.highlight
                            .lines
                            .extend(pts.iter().zip(next).map(|(a, b)| Outline {
                                a: DVec3::new(a.x, a.y, z),
                                b: DVec3::new(b.x, b.y, z),
                                kind: OutlineKind::Boundary,
                            }));
                    }
                    Ok(())
                }
                Ok(None) => Ok(()),
                Err(e) => Err(e.to_string()),
            },
        );
    }
    out
}

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
                let closed = pm.body().topology().edge(*edge).is_some_and(|e| e.closed);
                h.lines = edge_lines(pm.edges.get(*edge as usize), closed, OutlineKind::Hover);
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
    }
    h
}

/// What `target` is, as rows of a card: name and value.
pub fn describe(target: &Target, scene: &PickScene, m: Measure) -> Vec<(String, String)> {
    let at = |p: DVec3| {
        format!(
            "{}, {}, {}",
            m.number(p.x, 3),
            m.number(p.y, 3),
            m.length(p.z, 3)
        )
    };
    match target {
        Target::Face { model, face, .. } => {
            let Some(pm) = scene.model(model) else {
                return Vec::new();
            };
            let mut rows = vec![("Face".into(), describe_face(pm.body(), model, *face, m))];
            if let Some(f) = pm.body().topology().face(*face) {
                if let FaceKind::Plane { normal, .. } = f.kind {
                    let n = scene.frame.orientation.apply(normal);
                    rows.push((
                        "Facing".into(),
                        format!("{:.3}, {:.3}, {:.3}", n.x, n.y, n.z),
                    ));
                }
                if let Some(b) = f.bounds {
                    // A square turn keeps a box a box: its ends' heights are its range.
                    let (lo, hi) = (scene.frame.to_setup(b.min).z, scene.frame.to_setup(b.max).z);
                    rows.push((
                        "Height".into(),
                        format!("{} to {}", m.length(lo.min(hi), 3), m.length(lo.max(hi), 3)),
                    ));
                }
            }
            rows
        }
        Target::Edge { model, edge, .. } => match scene.model(model) {
            Some(pm) => vec![("Edge".into(), describe_edge(pm.body(), model, *edge, m))],
            None => Vec::new(),
        },
        Target::Point { point, setup } => {
            let what = scene.model(&point.model).map_or_else(String::new, |pm| {
                describe_point(pm.body(), &point.model, point.snap, point.index, m)
            });
            vec![
                ("Point".into(), what),
                ("In the setup".into(), at(*setup)),
                ("In the model".into(), at(DVec3::from_array(point.at))),
            ]
        }
        &Target::StockPoint { x, y, z, setup } => vec![
            ("Stock".into(), describe_box_point(x, y, z)),
            ("In the setup".into(), at(setup)),
        ],
    }
}

fn face_triangles<'a>(
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

/// "Face 12 · plane · 240.0 mm² · on 'part'".
pub fn describe_face(body: &Body, model: &str, face: u32, m: Measure) -> String {
    match body.topology().face(face) {
        Some(f) => {
            let kind = match f.kind {
                FaceKind::Plane { .. } => "plane",
                FaceKind::Curved => "curved",
            };
            format!("Face {face} · {kind} · {} · on '{model}'", m.area(f.area))
        }
        None => format!("Face {face} on '{model}'"),
    }
}

/// "Edge 3 · Ø6.35 circle · on 'part'", or a line or curve with its length.
pub fn describe_edge(body: &Body, model: &str, edge: u32, m: Measure) -> String {
    let Some(e) = body.topology().edge(edge) else {
        return format!("Edge {edge} on '{model}'");
    };
    let what = match e.kind {
        EdgeKind::Circle(c) if c.full => format!("Ø{} circle", m.number(c.radius * 2.0, 2)),
        EdgeKind::Circle(c) => format!("R{} arc", m.number(c.radius, 2)),
        EdgeKind::Line => format!("line {} long", m.length(e.length, 2)),
        EdgeKind::Curve => format!("curve {} long", m.length(e.length, 2)),
    };
    let closed = if e.closed { ", closed" } else { "" };
    format!("Edge {edge} · {what}{closed} · on '{model}'")
}

/// "Centre of the Ø6.35 circle on 'part'", "Corner 4 on 'part'", "Middle of edge 7
/// on 'part'".
pub fn describe_point(body: &Body, model: &str, snap: Snap, index: u32, m: Measure) -> String {
    match snap {
        Snap::Vertex => format!("Corner {index} on '{model}'"),
        Snap::CircleCenter => match describe_circle(body, index) {
            Some(c) => format!("Centre of the {c} on '{model}'"),
            None => format!("Centre of edge {index} on '{model}'"),
        },
        Snap::EdgeMiddle => {
            let long = body
                .topology()
                .edge(index)
                .map(|e| format!(", {} long", m.length(e.length, 2)))
                .unwrap_or_default();
            format!("Middle of edge {index}{long} on '{model}'")
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)]
mod tests {
    use super::*;
    use camcraft_core::job::{Along, PointRef};

    fn doc_with_op() -> Document {
        let mut doc = Document::untitled(1);
        doc.job.models.push(camcraft_core::job::ModelDef {
            name: "p".into(),
            path: "p.stl".into(),
            units: None,
        });
        let setup = doc.job.setups[0].name.clone();
        doc.job.setups[0].models = vec!["p".into()];
        doc.add_operation(
            &setup,
            camcraft_core::job::StrategySpec::Parallel { angle: 0.0 },
        );
        doc
    }

    fn face(face: u32) -> Target {
        Target::Face {
            model: "p".into(),
            face,
            at: DVec3::new(1.0, 2.0, 3.0),
        }
    }

    /// A face clicked once is added; clicked again, taken out; a face the slot
    /// does not take changes nothing.
    #[test]
    fn faces_toggle() {
        let mut doc = doc_with_op();
        let setup = doc.job.setups[0].name.clone();
        let slot = Slot::Faces { setup, op: 0 };
        let same = |r: &FeatureRef, _| Some(r.index);
        assert!(apply(&mut doc, &slot, &face(3), same));
        assert!(apply(&mut doc, &slot, &face(5), same));
        let faces = &doc.job.setups[0].operations[0].geometry.faces;
        assert_eq!(faces.iter().map(|r| r.index).collect::<Vec<_>>(), [3, 5]);
        assert_eq!(faces[0].at, [1.0, 2.0, 3.0]);
        assert!(apply(&mut doc, &slot, &face(3), same));
        let faces = &doc.job.setups[0].operations[0].geometry.faces;
        assert_eq!(faces.iter().map(|r| r.index).collect::<Vec<_>>(), [5]);
        let edge = Target::Edge {
            model: "p".into(),
            edge: 1,
            at: DVec3::ZERO,
        };
        assert!(!apply(&mut doc, &slot, &edge, same));
    }

    /// A saved pick found now under another number is still the same face: the
    /// click takes it out rather than adding it twice.
    #[test]
    fn a_renumbered_pick_is_still_recognised() {
        let mut doc = doc_with_op();
        let setup = doc.job.setups[0].name.clone();
        let slot = Slot::Faces { setup, op: 0 };
        doc.job.setups[0].operations[0]
            .geometry
            .faces
            .push(FeatureRef {
                model: "p".into(),
                index: 9,
                at: [0.0; 3],
            });
        let now_seven = |_: &FeatureRef, _| Some(7);
        assert!(apply(&mut doc, &slot, &face(7), now_seven));
        assert!(doc.job.setups[0].operations[0].geometry.faces.is_empty());
    }

    /// A work zero takes one point, and keeps its offset whichever kind it becomes.
    #[test]
    fn a_work_zero_keeps_its_offset() {
        let mut doc = doc_with_op();
        let setup = doc.job.setups[0].name.clone();
        doc.job.setups[0].origin = Origin::ModelOrigin {
            offset: [0.0, 0.0, 2.0],
        };
        let slot = Slot::WorkZero {
            setup: setup.clone(),
        };
        assert!(slot.single());
        let point = PointRef {
            model: "p".into(),
            snap: Snap::CircleCenter,
            index: 4,
            at: [1.0, 1.0, 5.0],
        };
        let target = Target::Point {
            point: point.clone(),
            setup: DVec3::ZERO,
        };
        let never = |_: &FeatureRef, _| None;
        assert!(apply(&mut doc, &slot, &target, never));
        assert_eq!(
            doc.job.setups[0].origin,
            Origin::Point {
                point,
                offset: [0.0, 0.0, 2.0]
            }
        );
        let corner = Target::StockPoint {
            x: Along::Min,
            y: Along::Min,
            z: Along::Max,
            setup: DVec3::ZERO,
        };
        assert!(apply(&mut doc, &slot, &corner, never));
        assert!(matches!(
            doc.job.setups[0].origin,
            Origin::BoxPoint {
                of: BoxOf::Stock,
                z: Along::Max,
                offset: [0.0, 0.0, 2.0],
                ..
            }
        ));
        // The same again changes nothing.
        assert!(!apply(&mut doc, &slot, &corner, never));
        assert!(!apply(&mut doc, &slot, &face(1), never));
    }

    /// Picking stops making sense when what it fills is no longer shown.
    #[test]
    fn a_slot_lasts_while_its_owner_is_selected() {
        let mut doc = doc_with_op();
        let setup = doc.job.setups[0].name.clone();
        let slot = Slot::Faces {
            setup: setup.clone(),
            op: 0,
        };
        doc.selection = Selection::Operation(setup.clone(), 0);
        assert!(slot.still_valid(&doc));
        doc.selection = Selection::Setup(setup.clone());
        assert!(!slot.still_valid(&doc));
        doc.selection = Selection::Operation(setup.clone(), 3);
        assert!(!Slot::Faces { setup, op: 3 }.still_valid(&doc));
    }
}
