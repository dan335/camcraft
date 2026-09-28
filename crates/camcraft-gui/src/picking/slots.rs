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

use camcraft_core::allowance::EffectiveTool;
use camcraft_core::features::{self, FeatureError};
use camcraft_core::job::{
    self, Boundary, BoxOf, FeatureRef, MachiningBoundary, Operation, Origin, Snap, StrategySpec,
};
use camcraft_core::planar::{centre_offset, picked_pieces};
use camcraft_core::run::cutting_tool;
use camcraft_core::setup::{Bodies, Prepared};
use camcraft_core::tool::ToolProfile;
use camcraft_geom::contour::Region;
use camcraft_geom::mesh::Aabb;
use camcraft_geom::topology::Body;
use glam::{DVec2, DVec3, Vec3};

use crate::document::{Document, Selection};
use crate::picking::describe::{describe_edge, describe_face, describe_point};
use crate::picking::ray::{Accept, PickModel, PickScene, Target};
use crate::ui::measure::Measure;
use crate::view::scene::box_edges;
use crate::view::viewport::{Highlight, Outline, OutlineKind};

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
                "Click an edge to add the loop it goes round, or take it out · Esc or Done to finish"
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
            let at = own(doc, model, *at);
            doc.operation_mut(setup, *op).is_some_and(|o| {
                toggle(&mut o.selection.faces, model, *face, at, |r| {
                    current(r, Kind::Face)
                })
            })
        }
        (Slot::Edges { setup, op }, Target::Edge { model, edge, at }) => {
            let at = own(doc, model, *at);
            doc.operation_mut(setup, *op).is_some_and(|o| {
                toggle(&mut o.selection.edges, model, *edge, at, |r| {
                    current(r, Kind::Edge)
                })
            })
        }
        (Slot::Edges { setup, op }, Target::Loop { model, edges }) => {
            let edges: Vec<(u32, DVec3)> = edges
                .iter()
                .map(|&(e, at)| (e, own(doc, model, at)))
                .collect();
            doc.operation_mut(setup, *op).is_some_and(|o| {
                toggle_loop(&mut o.selection.edges, model, &edges, |r| {
                    current(r, Kind::Edge)
                })
            })
        }
        _ => false,
    }
}

/// Where `at`, on `model` where the job places it, is in the model's own
/// coordinates: where a pick is saved, so it moves with its model.
fn own(doc: &Document, model: &str, at: DVec3) -> DVec3 {
    doc.models
        .get(model)
        .and_then(|s| s.model())
        .map_or(at, |m| DVec3::from_array(features::unplaced(&m.body, at)))
}

/// What a click on `target` picks into `slot`: for edges, the whole loop the edge
/// goes round ([`edge_loop`]), as they are only ever cut round in loops.
pub fn widen(scene: &PickScene, slot: &Slot, target: Target) -> Target {
    match (slot, target) {
        (Slot::Edges { .. }, t) => whole_loop(scene, t),
        (_, t) => t,
    }
}

/// An edge as the loop it goes round; anything else as it is.
pub fn whole_loop(scene: &PickScene, target: Target) -> Target {
    let Target::Edge { model, edge, at } = target else {
        return target;
    };
    let Some(body) = scene.model(&model).map(PickModel::body) else {
        return Target::Edge { model, edge, at };
    };
    let t = body.topology();
    let edges = edge_loop(scene, &model, edge)
        .into_iter()
        .map(|e| {
            let on = if e == edge {
                at
            } else {
                t.edge_middle(body.mesh(), e).unwrap_or(at)
            };
            (e, on)
        })
        .collect();
    Target::Loop { model, edges }
}

/// The loop edge `edge` of `model` goes round, in order from it: the edges joined
/// to it end to end at its height in the setup, turning least at each corner
/// ([`Topology::level_loop`](camcraft_geom::topology::Topology::level_loop)). So
/// an edge of a pocket's floor or of the part's outline takes the whole of it,
/// however its faces are split, and a hole's rim is its own loop. An edge that is
/// not level, or whose loop does not close, is taken alone.
pub fn edge_loop(scene: &PickScene, model: &str, edge: u32) -> Vec<u32> {
    let Some(pm) = scene.model(model) else {
        return vec![edge];
    };
    let t = pm.body().topology();
    let points = |e: u32| pm.edges.get(e as usize).cloned().unwrap_or_default();
    t.level_loop(edge, points, t.tolerance())
        .unwrap_or_else(|| vec![edge])
}

/// Take a loop out of `list` if all of it is there, or else add what is missing.
fn toggle_loop(
    list: &mut Vec<FeatureRef>,
    model: &str,
    edges: &[(u32, DVec3)],
    current: impl Fn(&FeatureRef) -> Option<u32>,
) -> bool {
    let has = |list: &[FeatureRef], e: u32| {
        list.iter()
            .any(|r| r.model == model && current(r) == Some(e))
    };
    if edges.iter().all(|&(e, _)| has(list, e)) {
        list.retain(|r| !(r.model == model && edges.iter().any(|&(e, _)| current(r) == Some(e))));
    } else {
        for &(e, at) in edges {
            if !has(list, e) {
                list.push(FeatureRef {
                    model: model.to_string(),
                    index: e,
                    at: at.to_array(),
                });
            }
        }
    }
    true
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
    /// The picked edges as the panel lists them: each loop once, as its picks'
    /// places in the job's order and what it is, and each pick not found alone.
    pub edge_loops: Vec<(Vec<usize>, Result<String, String>)>,
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
    let sel = &operation.selection;
    // Each pick found, with the model it is on, or why not.
    let find = |r: &FeatureRef, kind: Kind| {
        let body = bodies
            .get(r.model.as_str())
            .ok_or_else(|| FeatureError::NotLoaded(r.model.clone()).to_string())?;
        let index = kind.find(body, r, tolerance).map_err(|e| e.to_string())?;
        Ok::<_, String>((*body, index))
    };
    for r in &sel.faces {
        out.faces.push(find(r, Kind::Face).map(|(body, face)| {
            out.highlight
                .faces
                .extend(face_triangles(body, face, &to_setup));
            describe_face(body, &r.model, face, m)
        }));
    }
    // Each loop the picked edges go round, by model and its edges, with the picks
    // on it.
    type Round<'a> = (&'a str, Vec<u32>);
    let mut loops: Vec<(Round<'_>, Vec<usize>)> = Vec::new();
    for (i, r) in sel.edges.iter().enumerate() {
        let found = find(r, Kind::Edge);
        if let Ok((body, edge)) = &found {
            if let Some(pm) = scene.model(&r.model) {
                out.highlight.lines.extend(edge_lines(
                    pm.edges.get(*edge as usize),
                    body.topology().edge(*edge).is_some_and(|e| e.closed),
                    OutlineKind::Selected,
                ));
            }
            let mut round = edge_loop(scene, &r.model, *edge);
            round.sort_unstable();
            let key = (r.model.as_str(), round);
            match loops.iter_mut().find(|(k, _)| *k == key) {
                Some((_, picks)) => picks.push(i),
                None => loops.push((key, vec![i])),
            }
        } else {
            loops.push((("", Vec::new()), vec![i]));
        }
        out.edges
            .push(found.map(|(body, edge)| describe_edge(body, &r.model, edge, m)));
    }
    for ((model, round), picks) in loops {
        let what = match (round.len(), picks.first()) {
            (0 | 1, Some(&i)) => out.edges.get(i).cloned().unwrap_or(Ok(String::new())),
            (n, _) if picks.len() < n => Ok(format!(
                "{} of the {n} edges of a loop · on '{model}'",
                picks.len()
            )),
            (n, _) => Ok(format!(
                "Loop of {n} edges{} · on '{model}'",
                scene
                    .model(model)
                    .and_then(|pm| loop_size(pm, &round, m))
                    .unwrap_or_default()
            )),
        };
        out.edge_loops.push((picks, what));
    }

    // The boundary, drawn at the stock's top so it reads over everything. Worked
    // out as the run will work it out, with the operation's own tool.
    let found = out.faces.iter().chain(&out.edges).all(Result::is_ok);
    let z = prepared.stock_bounds().max.z;
    // Facing works from the stock alone: what it faces is its footprint and the
    // stock offset.
    if let StrategySpec::Face { stock_offset, .. } = operation.strategy {
        let b = prepared.stock_bounds();
        let grow = DVec2::splat(stock_offset);
        let lo = b.min.truncate() - grow;
        let hi = b.max.truncate() + grow;
        out.boundary = Some(if hi.x > lo.x && hi.y > lo.y {
            let area = Region::rectangle(lo, hi);
            out.highlight
                .lines
                .extend(rings_at(&area, z, OutlineKind::Boundary));
            Ok(())
        } else {
            Err("the stock offset leaves nothing to face".into())
        });
        return out;
    }
    let uses = operation.strategy.uses();
    if uses.outline {
        if found {
            let tool = cutting_tool(&doc.job, operation).ok();
            out.boundary = Some(
                outline_lines(operation, tool.as_ref(), prepared, &bodies, tolerance, z)
                    .map(|lines| out.highlight.lines.extend(lines))
                    .map_err(|e| e.to_string()),
            );
        }
        return out;
    }
    let b = &operation.boundary;
    if uses.boundary && b.kind != Boundary::Stock && found {
        let tool = cutting_tool(&doc.job, operation).ok();
        out.boundary = Some(
            boundary_lines(b, sel, tool.as_ref(), prepared, &bodies, tolerance, z)
                .map(|lines| out.highlight.lines.extend(lines))
                .map_err(|e| e.to_string()),
        );
    }
    out
}

/// " · 31.80 × 15.80 mm", the size of loop `round` of `pm`'s edges seen from above.
fn loop_size(pm: &PickModel, round: &[u32], m: Measure) -> Option<String> {
    let mut points = round
        .iter()
        .filter_map(|&e| pm.edges.get(e as usize))
        .flatten();
    let first = points.next()?.truncate();
    let (lo, hi) = points.fold((first, first), |(lo, hi), p| {
        (lo.min(p.truncate()), hi.max(p.truncate()))
    });
    let size = hi - lo;
    Some(format!(
        " · {} × {}",
        m.number(size.x, 2),
        m.length(size.y, 2)
    ))
}

/// A 2D operation's outline as lines, and hatched, what the tool cuts away beside
/// it: a band along it for a profile, all inside it for a pocket. Each piece of it
/// is drawn at the lowest of the picks in it, so it lies on them; with nothing
/// picked in it, at `top`, the stock's top. The stock's footprint when that is the
/// boundary. Nothing is hatched without a tool.
fn outline_lines(
    op: &Operation,
    tool: Option<&EffectiveTool>,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    tolerance: f64,
    top: f64,
) -> Result<Vec<Outline>, FeatureError> {
    let region = |e: camcraft_geom::contour::RegionError| FeatureError::Region(e.to_string());
    let (shape, picked) = features::outline(op, prepared, bodies, tolerance)?;
    let shape = shape.unwrap_or_else(|| {
        let b = prepared.stock_bounds();
        Region::rectangle(b.min.truncate(), b.max.truncate())
    });
    let arc = tolerance / 2.0;
    let radius = tool.map_or(0.0, EffectiveTool::cutter_radius);
    let offset = centre_offset(op, radius);
    let pocket = matches!(op.strategy, StrategySpec::Pocket { .. });
    let mut lines = Vec::new();
    let mut room = false;
    for (piece, picks) in picked_pieces(&shape, arc, &picked) {
        let z = picks.map_or(top, |p| p.lowest);
        lines.extend(rings_at(&piece, z, OutlineKind::Boundary));
        if radius <= 0.0 {
            room = true;
            continue;
        }
        let centres = piece.offset(offset, arc);
        if centres.area() <= 1e-9 {
            continue;
        }
        room = true;
        let reach = centres.offset(radius, arc);
        let cut = if pocket {
            reach
        } else {
            reach
                .difference(&centres.offset(-radius, arc))
                .map_err(region)?
        };
        lines.extend(hatched(&cut, z).map_err(region)?);
    }
    if !room {
        return Err(FeatureError::TooWide {
            diameter: radius * 2.0,
        });
    }
    Ok(lines)
}

/// An operation's machining boundary as lines at height `z`: the boundary as
/// chosen, where the tool's centre may go against it, and hatched, what the tool
/// cuts outside the part. That last is worked out against the part's whole
/// outline, so for a wall straight down it is exact, and for one that leans out
/// it is where the tool reaches at the bottom.
fn boundary_lines(
    b: &MachiningBoundary,
    selection: &job::Selection,
    tool: Option<&EffectiveTool>,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    tolerance: f64,
    z: f64,
) -> Result<Vec<Outline>, FeatureError> {
    let Some(shape) = features::boundary_shape(b.kind, selection, prepared, bodies, tolerance)?
    else {
        return Ok(Vec::new());
    };
    let radius = tool.map_or(0.0, EffectiveTool::cutter_radius);
    let arc = tolerance / 2.0;
    let limit = shape.offset(features::containment_offset(b, radius), arc);
    if limit.area() <= 1e-9 {
        return Err(FeatureError::NoRoom {
            diameter: radius * 2.0,
        });
    }
    let mut lines = rings_at(&shape, z, OutlineKind::Boundary);
    lines.extend(rings_at(&limit, z, OutlineKind::Limit));
    if let Some(tool) = tool {
        let region = |e: camcraft_geom::contour::RegionError| FeatureError::Region(e.to_string());
        let part = features::silhouette(&prepared.part).map_err(region)?;
        let centres = limit
            .difference(&part.offset(tool.max_radius(), arc))
            .map_err(region)?;
        let cut = centres
            .offset(radius, arc)
            .difference(&part)
            .map_err(region)?;
        lines.extend(hatched(&cut, z).map_err(region)?);
    }
    Ok(lines)
}

/// `region` hatched at height `z`.
fn hatched(region: &Region, z: f64) -> Result<Vec<Outline>, camcraft_geom::contour::RegionError> {
    let mut lines = Vec::new();
    for line in region.clip_polylines(&hatch(region))? {
        lines.extend(line.windows(2).filter_map(|w| match w {
            [a, b] => Some(Outline {
                a: DVec3::new(a.x, a.y, z),
                b: DVec3::new(b.x, b.y, z),
                kind: OutlineKind::Channel,
            }),
            _ => None,
        }));
    }
    Ok(lines)
}

/// Each ring of `region` as closed lines at height `z`.
fn rings_at(region: &Region, z: f64, kind: OutlineKind) -> Vec<Outline> {
    let mut out = Vec::new();
    for ring in region.rings() {
        let pts = ring.points();
        let next = pts.iter().skip(1).chain(pts.first());
        out.extend(pts.iter().zip(next).map(|(a, b)| Outline {
            a: DVec3::new(a.x, a.y, z),
            b: DVec3::new(b.x, b.y, z),
            kind,
        }));
    }
    out
}

/// Diagonal lines across `region`'s bounds, to be clipped to it: some sixty
/// across its longer side, and no closer than a millimetre and a half.
fn hatch(region: &Region) -> Vec<Vec<DVec2>> {
    let Some((lo, hi)) = region.bounds() else {
        return Vec::new();
    };
    let size = hi - lo;
    let gap = (size.x.max(size.y) / 60.0).max(1.5);
    // Lines x - y = c, from the lowest c to the highest.
    let (from, to) = (lo.x - hi.y, hi.x - lo.y);
    let mut out = Vec::new();
    let mut c = from + gap / 2.0;
    while c < to {
        out.push(vec![DVec2::new(lo.y + c, lo.y), DVec2::new(hi.y + c, hi.y)]);
        c += gap;
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
        Target::Loop { model, edges } => {
            if let Some(pm) = scene.model(model) {
                for &(e, _) in edges {
                    let closed = pm.body().topology().edge(e).is_some_and(|e| e.closed);
                    h.lines.extend(edge_lines(
                        pm.edges.get(e as usize),
                        closed,
                        OutlineKind::Hover,
                    ));
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
            transform: Default::default(),
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
        let faces = &doc.job.setups[0].operations[0].selection.faces;
        assert_eq!(faces.iter().map(|r| r.index).collect::<Vec<_>>(), [3, 5]);
        assert_eq!(faces[0].at, [1.0, 2.0, 3.0]);
        assert!(apply(&mut doc, &slot, &face(3), same));
        let faces = &doc.job.setups[0].operations[0].selection.faces;
        assert_eq!(faces.iter().map(|r| r.index).collect::<Vec<_>>(), [5]);
        let edge = Target::Edge {
            model: "p".into(),
            edge: 1,
            at: DVec3::ZERO,
        };
        assert!(!apply(&mut doc, &slot, &edge, same));
    }

    /// A loop clicked adds the edges of it not yet picked; clicked again with all
    /// of it picked, takes them all out.
    #[test]
    fn edges_toggle_a_loop_at_a_time() {
        let mut doc = doc_with_op();
        let setup = doc.job.setups[0].name.clone();
        let slot = Slot::Edges { setup, op: 0 };
        let same = |r: &FeatureRef, _| Some(r.index);
        let round = Target::Loop {
            model: "p".into(),
            edges: vec![(4, DVec3::X), (5, DVec3::Y), (6, DVec3::Z)],
        };
        let one = Target::Edge {
            model: "p".into(),
            edge: 5,
            at: DVec3::Y,
        };
        let picked = |doc: &Document| {
            doc.job.setups[0].operations[0]
                .selection
                .edges
                .iter()
                .map(|r| r.index)
                .collect::<Vec<_>>()
        };
        assert!(apply(&mut doc, &slot, &one, same));
        assert!(apply(&mut doc, &slot, &round, same));
        assert_eq!(picked(&doc), [5, 4, 6]);
        assert_eq!(
            doc.job.setups[0].operations[0].selection.edges[2].at,
            [0.0, 0.0, 1.0]
        );
        assert!(apply(&mut doc, &slot, &round, same));
        assert!(picked(&doc).is_empty());
    }

    /// A saved pick found now under another number is still the same face: the
    /// click takes it out rather than adding it twice.
    #[test]
    fn a_renumbered_pick_is_still_recognised() {
        let mut doc = doc_with_op();
        let setup = doc.job.setups[0].name.clone();
        let slot = Slot::Faces { setup, op: 0 };
        doc.job.setups[0].operations[0]
            .selection
            .faces
            .push(FeatureRef {
                model: "p".into(),
                index: 9,
                at: [0.0; 3],
            });
        let now_seven = |_: &FeatureRef, _| Some(7);
        assert!(apply(&mut doc, &slot, &face(7), now_seven));
        assert!(doc.job.setups[0].operations[0].selection.faces.is_empty());
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
