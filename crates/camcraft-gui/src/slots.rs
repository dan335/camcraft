//! Picking into the job: which property a click in the view fills, what it takes,
//! what a click does to the job, and what the view and the panel show of the picks
//! already made.
//!
//! A property that takes picks is a *slot*: a setup's work zero, its models or its
//! stocks, an operation's faces or its edges. Its Select button arms the view for it, so the view offers
//! only what the slot takes; a click then writes straight into the job, where it is
//! one step to undo like any other edit. A work zero takes one point and is done; a
//! list of faces or edges takes any number, a second click on one taking it out, and
//! is done when the person says so.
//!
//! What is shown of a pick is found the same way the engine finds it
//! ([`camcraft_core::features`]), so a face lit in the view is the face that will be
//! cut, and a pick that can no longer be found says so here before it stops a run.

use camcraft_core::features::{self, FeatureError};
use camcraft_core::job::{BoxOf, FeatureRef, Origin, Snap};
use camcraft_core::run::cutting_tool;
use camcraft_core::setup::{Bodies, Prepared};
use camcraft_core::tool::ToolProfile;
use camcraft_geom::mesh::Aabb;
use camcraft_geom::topology::{Body, EdgeKind, FaceKind};
use glam::{DVec3, Vec3};

use crate::document::{Document, Selection};
use crate::measure::Measure;
use crate::pick::{Accept, PickScene, Target, describe_circle};
use crate::scene::box_edges;
use crate::viewport::{Highlight, Outline, OutlineKind};

/// A property a click in the view can fill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slot {
    /// Setup `setup`'s work zero: a point on a model or on the stock's box.
    WorkZero {
        /// The setup, by name.
        setup: String,
    },
    /// The models setup `setup` machines. The job's others are shown faintly, to
    /// be clicked in.
    Models {
        /// The setup, by name.
        setup: String,
    },
    /// The stocks setup `setup` starts from, likewise.
    Stocks {
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
            Slot::Models { .. } => Accept::MODELS,
            Slot::Stocks { .. } => Accept::STOCKS,
            Slot::Faces { .. } => Accept::FACES,
            Slot::Edges { .. } => Accept::EDGES,
        }
    }

    /// Whether one pick fills it, so it is done after the first click.
    pub fn single(&self) -> bool {
        matches!(self, Slot::WorkZero { .. })
    }

    /// What the view says while it is armed.
    pub fn hint(&self) -> &'static str {
        match self {
            Slot::WorkZero { .. } => {
                "Click a corner, a round edge for its centre, or a point on the stock · Esc to stop"
            }
            Slot::Models { .. } => {
                "Click models to add them to the setup or take them out · Esc or Done to finish"
            }
            Slot::Stocks { .. } => {
                "Click stocks to add them to the setup or take them out · Esc or Done to finish"
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
        match self {
            Slot::WorkZero { setup } | Slot::Models { setup } | Slot::Stocks { setup } => {
                doc.setup(setup).is_some() && doc.selection == Selection::Setup(setup.clone())
            }
            Slot::Faces { setup, op } | Slot::Edges { setup, op } => {
                doc.operation(setup, *op).is_some()
                    && doc.selection == Selection::Operation(setup.clone(), *op)
            }
        }
    }
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
        (Slot::Models { setup }, Target::Model { model }) => doc
            .setup_mut(setup)
            .is_some_and(|s| toggle_name(&mut s.models, model)),
        (Slot::Stocks { setup }, Target::Stock { stock }) => doc
            .setup_mut(setup)
            .is_some_and(|s| toggle_name(&mut s.stocks, stock)),
        (Slot::Faces { setup, op }, Target::Face { model, face, at }) => {
            doc.operation_mut(setup, *op).is_some_and(|o| {
                toggle(&mut o.geometry.faces, model, *face, *at, |r| {
                    current(r, Kind::Face)
                })
            })
        }
        (Slot::Edges { setup, op }, Target::Edge { model, edge, at }) => {
            doc.operation_mut(setup, *op).is_some_and(|o| {
                toggle(&mut o.geometry.edges, model, *edge, *at, |r| {
                    current(r, Kind::Edge)
                })
            })
        }
        _ => false,
    }
}

/// Take `name` out of `names` if it is there, or else add it.
fn toggle_name(names: &mut Vec<String>, name: &str) -> bool {
    if names.iter().any(|n| n == name) {
        names.retain(|n| n != name);
    } else {
        names.push(name.to_string());
    }
    true
}

/// Whether a saved pick is a face or an edge, for finding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A face.
    Face,
    /// An edge.
    Edge,
}

impl Kind {
    /// Which of `body`'s faces or edges saved pick `r` is now, found as the engine
    /// finds it, in a job machined to `tolerance`.
    pub fn find(self, body: &Body, r: &FeatureRef, tolerance: f64) -> Result<u32, FeatureError> {
        let tol = features::tolerance(tolerance, body);
        match self {
            Kind::Face => features::find_face(body, r, tol),
            Kind::Edge => features::find_edge(body, r, tol),
        }
        .map(|found| found.index)
    }
}

fn set_origin(doc: &mut Document, setup: &str, make: impl FnOnce([f64; 3]) -> Origin) -> bool {
    let Some(s) = doc.setup_mut(setup) else {
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
    let Some(s) = doc.setup(setup) else {
        return out;
    };
    let bodies: Bodies<'_> = scene.setup_models().map(|m| (m.name(), m.body())).collect();
    let tolerance = doc.job.tolerance;
    let to_setup = |p: DVec3| scene.frame.to_setup(p);

    if let Origin::Point { point, .. } = &s.origin {
        out.origin = Some(match bodies.get(point.model.as_str()) {
            Some(body) => {
                match features::find_point(body, point, features::tolerance(tolerance, body)) {
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
                }
            }
            None => Err(FeatureError::NotLoaded(point.model.clone()).to_string()),
        });
    }

    let Some(operation) = op.and_then(|i| s.operations.get(i)) else {
        return out;
    };
    let g = &operation.geometry;
    // Each pick found, with the model it is on, or why not.
    let find = |r: &FeatureRef, kind: Kind| {
        let body = bodies
            .get(r.model.as_str())
            .ok_or_else(|| FeatureError::NotLoaded(r.model.clone()).to_string())?;
        let index = kind.find(body, r, tolerance).map_err(|e| e.to_string())?;
        Ok::<_, String>((*body, index))
    };
    for r in &g.faces {
        out.faces.push(find(r, Kind::Face).map(|(body, face)| {
            out.highlight
                .faces
                .extend(face_triangles(body, face, &to_setup));
            describe_face(body, &r.model, face, m)
        }));
    }
    for r in &g.edges {
        out.edges.push(find(r, Kind::Edge).map(|(body, edge)| {
            if let Some(pm) = scene.model(&r.model) {
                out.highlight.lines.extend(edge_lines(
                    pm.edges.get(edge as usize),
                    body.topology().edge(edge).is_some_and(|e| e.closed),
                    OutlineKind::Selected,
                ));
            }
            describe_edge(body, &r.model, edge, m)
        }));
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

    /// A model or stock clicked is added to the setup; clicked again, taken out.
    /// A face means nothing to them.
    #[test]
    fn models_and_stocks_toggle() {
        let mut doc = doc_with_op();
        let setup = doc.job.setups[0].name.clone();
        let never = |_: &FeatureRef, _| None;
        let models = Slot::Models {
            setup: setup.clone(),
        };
        assert!(!models.single());
        let q = Target::Model { model: "q".into() };
        assert!(apply(&mut doc, &models, &q, never));
        assert_eq!(doc.job.setups[0].models, ["p", "q"]);
        assert!(apply(&mut doc, &models, &q, never));
        assert_eq!(doc.job.setups[0].models, ["p"]);
        assert!(!apply(&mut doc, &models, &face(1), never));

        let stocks = Slot::Stocks {
            setup: setup.clone(),
        };
        let before = doc.job.setups[0].stocks.clone();
        let other = Target::Stock {
            stock: "Other".into(),
        };
        assert!(apply(&mut doc, &stocks, &other, never));
        assert_eq!(
            doc.job.setups[0].stocks.last().map(String::as_str),
            Some("Other")
        );
        assert!(apply(&mut doc, &stocks, &other, never));
        assert_eq!(doc.job.setups[0].stocks, before);
        assert!(!apply(&mut doc, &stocks, &q, never));

        doc.selection = Selection::Setup(setup);
        assert!(models.still_valid(&doc));
        doc.selection = Selection::Job;
        assert!(!models.still_valid(&doc));
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
