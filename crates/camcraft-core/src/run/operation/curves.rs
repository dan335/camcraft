//! The paths of the strategies that run along what is picked, Fusion's Slot,
//! Trace and 2D Chamfer, and the slots, curves and edges they run along,
//! found from the picked edges and faces.

use camcraft_geom::mesh::TriMesh;
use camcraft_geom::topology::{Body as Model, EdgeKind};

use super::*;
use crate::cutter::tool::Cutter;
use crate::job::JobError;
use crate::setup::features::{self, find_edge, find_face};
use crate::setup::frame::Frame;
use crate::strategy::chamfer::{ChamferEdge, ChamferError, ChamferParams, chamfer};
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
    Ok(picked_edge_ids(job, op, prepared, bodies, faces)?
        .into_iter()
        .map(|(_, edge)| edge)
        .collect())
}

/// A picked edge, with its model and its place in its model's edges.
type PickedEdge = ((String, u32), Edge);

/// [`picked_edges`], each with its model and its place in its model's
/// edges.
fn picked_edge_ids(
    job: &Job,
    op: &Operation,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    faces: bool,
) -> Result<Vec<PickedEdge>, RunError> {
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
    let mut out: Vec<PickedEdge> = Vec::with_capacity(found.len());
    let mut seen: Vec<(String, u32)> = Vec::with_capacity(found.len());
    for (model, e) in found {
        if seen.contains(&(model.clone(), e)) {
            continue;
        }
        let body = body_of(op, bodies, &model)?;
        if let Some(edge) = edge_curve(&model, body, e, frame, tol) {
            out.push(((model.clone(), e), edge));
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

/// What a 2D chamfer cuts: its edges, each with the side the part is on, and
/// the part's faces other than those the edges join, which its clearance
/// keeps off, in the setup's frame; `None` where there are none.
pub(in crate::run) struct Chamfers {
    pub(in crate::run) edges: Vec<ChamferEdge>,
    pub(in crate::run) others: Option<TriMesh>,
}

/// The edges a 2D chamfer's picks chamfer, in the setup's frame, in the order
/// picked: each picked edge, and every edge round each picked face, joined
/// end to end; and the faces they do not join.
pub(in crate::run) fn chamfers_picked(
    job: &Job,
    op: &Operation,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
) -> Result<Chamfers, RunError> {
    let picked = picked_edge_ids(job, op, prepared, bodies, true)?;
    if picked.is_empty() {
        return Err(in_op(op)(ChamferError::NothingPicked));
    }
    let tol = job.tolerance.max(0.001);
    let cutter = prepared.drop_cutter();
    let edges: Vec<Edge> = picked.iter().map(|(_, e)| e.clone()).collect();
    let mut found = Vec::new();
    for curve in join(&edges) {
        let part_on_left = part_side(&curve, &cutter, tol).map_err(in_op(op))?;
        found.push(ChamferEdge {
            curve,
            part_on_left,
        });
    }
    // The faces the chamfered edges join, by model.
    let mut joined: Vec<(String, u32)> = Vec::new();
    for ((model, e), _) in &picked {
        let body = body_of(op, bodies, model)?;
        for &f in body
            .topology()
            .edge(*e)
            .map_or(&[][..], |e| e.faces.as_slice())
        {
            joined.push((model.clone(), f));
        }
    }
    let mut vertices: Vec<DVec3> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    for piece in &prepared.pieces {
        let Some(body) = bodies.get(piece.model.as_str()) else {
            continue;
        };
        let mesh = body.mesh();
        let first = u32::try_from(vertices.len()).unwrap_or(u32::MAX);
        vertices.extend(mesh.vertices().iter().map(|&p| prepared.frame.to_setup(p)));
        for (t, tri) in mesh.triangles().iter().enumerate() {
            let face = u32::try_from(t)
                .ok()
                .and_then(|t| body.topology().face_of(t));
            if face.is_some_and(|f| joined.contains(&(piece.model.clone(), f))) {
                continue;
            }
            triangles.push(tri.map(|v| v.saturating_add(first)));
        }
    }
    let others = if triangles.is_empty() {
        None
    } else {
        Some(
            TriMesh::new(vertices, triangles)
                .map_err(|e| in_op(op)(crate::strategy::StrategyError::Region(e.to_string())))?,
        )
    };
    Ok(Chamfers {
        edges: found,
        others,
    })
}

/// Which side of `curve` the part is, seen from above: its left, or its
/// right. Told where it runs straightest, a little either side of the middle
/// of its longest piece: the part is level with the edge or above it on its
/// side, and lower on the other.
fn part_side(curve: &Curve, cutter: &DropCutter<'_>, tol: f64) -> Result<bool, ChamferError> {
    let longest = curve
        .steps()
        .max_by(|a, b| a.1.flat_length(a.0).total_cmp(&b.1.flat_length(b.0)));
    let Some((from, piece)) = longest else {
        return Err(ChamferError::NothingPicked);
    };
    let at = piece.point_at(from, 0.5);
    let ahead = piece.point_at(from, 0.5 + 1e-3);
    let behind = piece.point_at(from, 0.5 - 1e-3);
    let no_side = ChamferError::NoSide { x: at.x, y: at.y };
    let Some(heading) = (ahead - behind).truncate().try_normalize() else {
        return Err(no_side);
    };
    // A dot of a tool: what stands there.
    let Ok(probe) = crate::cutter::tool::Flat::new(1e-3).map(Cutter::from) else {
        return Err(no_side);
    };
    let across = (tol * 20.0).max(0.05);
    let height = |side: f64| {
        let q = at.truncate() + heading.perp() * across * side;
        cutter.drop_at(&probe, q.x, q.y)
    };
    let on = |z: Option<f64>| z.is_some_and(|z| z >= at.z - 2.0 * tol);
    match (on(height(1.0)), on(height(-1.0))) {
        (true, false) => Ok(true),
        (false, true) => Ok(false),
        _ => Err(no_side),
    }
}

/// What a 2D chamfer asks of each edge, beyond its operation's passes.
#[derive(Debug, Clone, Copy)]
pub(super) struct ChamferAsked {
    pub(super) width: f64,
    pub(super) tip_offset: f64,
    pub(super) clearance: f64,
    pub(super) finishing_overlap: f64,
    pub(super) preserve_order: bool,
}

/// `chamfers` chamfered, with `work`'s tool, which must be a cone.
pub(super) fn chamfer_paths(
    work: &Work<'_>,
    asked: ChamferAsked,
    chamfers: &Chamfers,
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let op = work.op;
    let Cutter::Cone(cone) = *work.tool.base() else {
        return Err(JobError::WrongTool {
            op: op.name.clone(),
            tool: op.tool.clone().unwrap_or_default(),
            why: "a 2D chamfer cuts with a cone: use a chamfer mill or V-bit".into(),
        }
        .into());
    };
    let radius = cone.max_radius();
    let tolerance = work.job.tolerance.max(0.001);
    // The tool grown by the clearance, dropped onto the faces the edges do
    // not join: as high as its tip must be to keep off them.
    let others = chamfers.others.as_ref().map(DropCutter::new);
    let grown = (asked.clearance > 0.0)
        .then(|| {
            crate::cutter::tool::Cone::new(
                radius + asked.clearance,
                cone.tip() + asked.clearance,
                (1.0 / cone.rise()).atan(),
            )
            .ok()
        })
        .flatten()
        .map(Cutter::from);
    let lift = |p: DVec2| -> Option<f64> {
        let (Some(others), Some(grown)) = (&others, &grown) else {
            return None;
        };
        others.drop_at(grown, p.x, p.y)
    };
    let params = ChamferParams {
        tip: cone.tip(),
        radius,
        rise: cone.rise(),
        width: asked.width,
        tip_offset: asked.tip_offset,
        overlap: asked.finishing_overlap,
        lead: 0.1 * work.spec.diameter,
        preserve_order: asked.preserve_order,
        kept_on_right: work.kept_on_right(),
        feed: op.passes.feed,
        tolerance,
        lift: grown
            .is_some()
            .then_some(&lift as &(dyn Fn(DVec2) -> Option<f64> + Sync)),
    };
    let (path, report) = chamfer(&chamfers.edges, &params).map_err(in_op(op))?;
    if report.lifted > 0 {
        warnings.push(format!(
            "the tool was lifted on {} stretch(es) to keep the chamfer clearance off the \
             part: the chamfer is narrower there",
            report.lifted
        ));
    }
    if asked.width <= 0.0 {
        warnings.push(
            "the chamfer width is zero: a sharp edge is not cut, only a chamfer the model \
             has is followed"
                .into(),
        );
    }
    Ok((path, report.passes, StrategyDetail::Chamfer2d(report)))
}
