//! The paths of the strategies that run along what is picked, Fusion's Slot
//! and Trace, and the slots and curves they run along, found from the picked
//! edges and faces.

use camcraft_geom::topology::{Body as Model, EdgeKind};

use super::*;
use crate::setup::features::{self, find_edge, find_face};
use crate::setup::frame::Frame;
use crate::strategy::curve::{Curve, Edge, Piece, join};
use crate::strategy::nesting::take_nearest;
use crate::strategy::slot::{Slot, SlotError, SlotParams, slot, slot_of};
use crate::strategy::trace::{Beside, TraceError, TraceParams, trace};

/// Edge `index` of `body`, placed as the setup has it, as a piece of curve:
/// a round edge lying level an arc about its centre, anything else the line
/// through its points.
fn edge_curve(model: &str, body: &Model, index: u32, frame: &Frame, tol: f64) -> Option<Edge> {
    let t = body.topology();
    let edge = t.edge(index)?;
    let points: Vec<DVec3> = t
        .edge_points(body.mesh(), index)
        .map(|p| frame.to_setup(p))
        .collect();
    let first = *points.first()?;
    let level = points.iter().all(|p| (p.z - first.z).abs() <= tol);
    let mut pieces = Vec::new();
    match edge.kind {
        EdgeKind::Circle(c)
            if level && frame.orientation.apply(c.normal).z.abs() >= 1.0 - 1.0e-6 =>
        {
            let centre = frame.to_setup(c.center).truncate();
            // How far round it turns, point by point.
            let mut ring = points.clone();
            if edge.closed {
                ring.push(first);
            }
            let sweep: f64 = ring
                .windows(2)
                .filter_map(|w| match w {
                    [a, b] => Some(camcraft_geom::arc::turn(
                        a.truncate() - centre,
                        b.truncate() - centre,
                    )),
                    _ => None,
                })
                .sum();
            let to = ring.last().copied().unwrap_or(first);
            pieces.push(Piece::Arc { to, centre, sweep });
        }
        _ => {
            pieces.extend(points.iter().skip(1).map(|&to| Piece::Line { to }));
            if edge.closed {
                pieces.push(Piece::Line { to: first });
            }
        }
    }
    Some(Edge {
        model: model.to_string(),
        ends: edge.ends,
        curve: Curve {
            start: first,
            pieces,
        },
    })
}

/// The body `model` names.
fn body_of<'b>(op: &Operation, bodies: &Bodies<'b>, model: &str) -> Result<&'b Model, RunError> {
    bodies
        .get(model)
        .copied()
        .ok_or_else(|| in_op(op)(FeatureError::NotLoaded(model.to_string())))
}

/// `op`'s picked edges, each once, as pieces of curve in the setup's frame,
/// and, where `faces`, the edges round each picked face, in the order picked.
fn picked_edges(
    job: &Job,
    op: &Operation,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    faces: bool,
) -> Result<Vec<Edge>, RunError> {
    let frame = &prepared.frame;
    let tol = job.tolerance.max(0.001);
    let mut found: Vec<(String, u32)> = Vec::new();
    if faces {
        for pick in &op.selection.faces {
            let body = body_of(op, bodies, &pick.model)?;
            let face = find_face(body, pick, features::tolerance(job.tolerance, body))
                .map_err(in_op(op))?;
            for &e in body
                .topology()
                .face(face.index)
                .map_or(&[][..], |f| f.edges.as_slice())
            {
                found.push((pick.model.clone(), e));
            }
        }
    }
    for pick in &op.selection.edges {
        let body = body_of(op, bodies, &pick.model)?;
        let edge =
            find_edge(body, pick, features::tolerance(job.tolerance, body)).map_err(in_op(op))?;
        found.push((pick.model.clone(), edge.index));
    }
    let mut out: Vec<Edge> = Vec::with_capacity(found.len());
    let mut seen: Vec<(String, u32)> = Vec::with_capacity(found.len());
    for (model, e) in found {
        if seen.contains(&(model.clone(), e)) {
            continue;
        }
        let body = body_of(op, bodies, &model)?;
        if let Some(edge) = edge_curve(&model, body, e, frame, tol) {
            out.push(edge);
        }
        seen.push((model, e));
    }
    Ok(out)
}

/// The slots a slot operation's picked edges and floors go round, in the
/// setup's frame, in the order they are cut: each from the Top, the stock's
/// unless set, down to its own floor, or the Bottom where one is set, raised
/// by the stock to leave down.
pub(in crate::run) fn slots_picked(
    job: &Job,
    op: &Operation,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    ctx: &HeightContext,
) -> Result<Vec<Slot>, RunError> {
    let edges = picked_edges(job, op, prepared, bodies, true)?;
    if edges.is_empty() {
        return Err(in_op(op)(SlotError::NothingPicked));
    }
    let tol = job.tolerance.max(0.001);
    let (top, bottom) = op.levels_3d(ctx);
    let axial = op.stock_to_leave().axial;
    let mut found = Vec::new();
    for boundary in join(&edges) {
        if !boundary.is_closed(tol) {
            return Err(in_op(op)(SlotError::Open {
                at: boundary.end().to_array(),
            }));
        }
        let (middle, radius) = slot_of(&boundary, tol).map_err(in_op(op))?;
        let floor = if op.heights.bottom.is_some() {
            bottom
        } else {
            middle.start.z
        };
        found.push(Slot {
            middle,
            radius,
            top,
            bottom: floor + axial,
        });
    }
    let preserve = matches!(
        op.strategy,
        StrategySpec::Slot {
            preserve_order: true,
            ..
        }
    );
    if preserve {
        return Ok(found);
    }
    // The first picked, then always the nearest.
    let mut out = Vec::with_capacity(found.len());
    let mut at = found.first().map(|s| s.middle.start.truncate());
    while let Some(s) = take_nearest(&mut found, |s| {
        at.map_or(0.0, |a| a.distance(s.middle.start.truncate()))
    }) {
        at = Some(s.middle.end().truncate());
        out.push(s);
    }
    Ok(out)
}

/// The curves a trace's picked edges make, joined end to end, in the setup's
/// frame, in the order picked.
pub(in crate::run) fn curves_picked(
    job: &Job,
    op: &Operation,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
) -> Result<Vec<Curve>, RunError> {
    if !op.selection.faces.is_empty() {
        return Err(in_op(op)(TraceError::FacesPicked));
    }
    let edges = picked_edges(job, op, prepared, bodies, false)?;
    if edges.is_empty() {
        return Err(in_op(op)(TraceError::NothingPicked));
    }
    Ok(join(&edges))
}

/// What a slot or a trace did, said where it is worth a person's attention.
fn tell_slots(report: &crate::strategy::slot::SlotReport, warnings: &mut Vec<String>) {
    if report.bands > 0 {
        warnings.push(format!(
            "{} slot(s) more than three tools wide: a band up to {:.3} mm wide is left \
             either side of the middle; use a wider tool",
            report.bands, report.widest_band
        ));
    }
}

/// Each of `slots` cut along its middle, level by level.
pub(super) fn slot_paths(
    work: &Work<'_>,
    (backoff, repeat): (f64, bool),
    slots: &[Slot],
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let op = work.op;
    let params = SlotParams {
        radius: work.tool.cutter_radius(),
        stepdown: op.stepdown(),
        feed: op.passes.feed,
        backoff,
        repeat,
        kept_on_right: work.kept_on_right(),
        tolerance: work.job.tolerance.max(0.001),
    };
    let (path, report) = slot(slots, &params).map_err(in_op(op))?;
    tell_slots(&report, warnings);
    Ok((path, report.passes, StrategyDetail::Slot(report)))
}

/// What a trace asks of each curve, beyond its operation's passes.
#[derive(Debug, Clone, Copy)]
pub(super) struct TraceAsked {
    pub(super) beside: Beside,
    pub(super) axial_offset: f64,
    pub(super) pass_extension: f64,
    pub(super) max_angle: f64,
    pub(super) stepdowns: u32,
    pub(super) by_depth: bool,
    pub(super) both_ways: bool,
    pub(super) repeat: bool,
    pub(super) preserve_order: bool,
}

/// Each of `curves` followed.
pub(super) fn trace_paths(
    work: &Work<'_>,
    asked: TraceAsked,
    curves: &[Curve],
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let op = work.op;
    let leave = op.stock_to_leave();
    let params = TraceParams {
        beside: asked.beside,
        offset: work.tool.cutter_radius() + leave.radial,
        raise: asked.axial_offset + leave.axial,
        extension: asked.pass_extension,
        max_angle: asked.max_angle.to_radians(),
        levels: op.stepdown().map(|d| (d, asked.stepdowns)),
        by_depth: asked.by_depth,
        both_ways: asked.both_ways,
        repeat: asked.repeat,
        preserve_order: asked.preserve_order,
        feed: op.passes.feed,
        tolerance: work.job.tolerance.max(0.001),
    };
    let (path, report) = trace(curves, &params).map_err(in_op(op))?;
    if report.steep > 0 {
        warnings.push(format!(
            "{} stretch(es) steeper than the maximum angle left out",
            report.steep
        ));
    }
    Ok((path, report.passes, StrategyDetail::Trace(report)))
}
