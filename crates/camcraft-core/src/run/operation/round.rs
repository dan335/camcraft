//! The paths of the strategies that cut round walls, Fusion's Bore, Circular
//! and Thread, and the walls they cut, found from the picked faces.

use camcraft_geom::topology::EdgeKind;

use super::clearing::level_leads;
use super::curves::body_of;
use super::*;
use crate::job::ThreadHand;
use crate::setup::features::{self, find_face};
use crate::strategy::round::{
    Down, Levels, RoundParams, Wall, bore, circular, nearest_first, rounds,
};
use crate::strategy::thread::{ThreadParams, thread};

/// What a bore or a circular asks of each wall: how many passes out to it,
/// whether the last is cut twice, and whether its leads go to the centre.
#[derive(Debug, Clone, Copy)]
pub(super) struct RoundAsked {
    pub(super) stepovers: u32,
    pub(super) repeat: bool,
    pub(super) lead_to_center: bool,
}

/// How `work`'s passes round each wall are laid out.
fn round_params(work: &Work<'_>, asked: RoundAsked) -> RoundParams {
    let op = work.op;
    let diameter = work.spec.diameter;
    RoundParams {
        radius: work.tool.cutter_radius(),
        leave: op.stock_to_leave().radial,
        stepovers: asked.stepovers,
        stepover: op.passes.stepover,
        // A bore's finishing pass is one, at the cutting feed, as Fusion's
        // has no more: its stepover is all it is given.
        finishing: op.finishing(diameter).map(|f| WallFinish {
            passes: 1,
            stepover: f.stepover,
            feed: op.passes.feed,
            repeat: false,
        }),
        repeat: asked.repeat,
        feed: op.passes.feed,
        kept_on_right: work.kept_on_right(),
        leads: op.leads(diameter).map(level_leads),
        to_centre: asked.lead_to_center,
    }
}

/// What a bore or a circular did, said where it is worth a person's
/// attention.
fn tell(report: &RoundReport, warnings: &mut Vec<String>) {
    if report.cores > 0 {
        warnings.push(format!(
            "{} hole(s) left with a core standing in the middle, up to {:.3} mm across: \
             more stepovers, or wider ones, would cut it",
            report.cores, report.widest_core
        ));
    }
    tell_dropped(report.dropped, warnings);
    if report.no_room_to_lead > 0 {
        warnings.push(format!(
            "{} pass(es) have no room for a lead inside the hole, and are entered and \
             left without",
            report.no_room_to_lead
        ));
    }
}

/// The `dropped` passes past a hole's middle a bore, a circular or a thread
/// left out, said where there are any.
fn tell_dropped(dropped: u64, warnings: &mut Vec<String>) {
    if dropped > 0 {
        warnings.push(format!(
            "{dropped} pass(es) past the middle of a hole left out: fewer stepovers, or \
             narrower ones, would keep them"
        ));
    }
}

/// Each of `walls` bored down a helix, at `ramp_angle` degrees where given,
/// or else `pitch`.
pub(super) fn bore_paths(
    work: &Work<'_>,
    (ramp_angle, pitch): (Option<f64>, f64),
    asked: RoundAsked,
    walls: &[Round],
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let op = work.op;
    let down = match ramp_angle {
        Some(a) => Down::Angle(a.to_radians()),
        None => Down::Pitch(pitch),
    };
    let (path, report) = bore(walls, down, &round_params(work, asked)).map_err(in_op(op))?;
    tell(&report, warnings);
    Ok((path, report.passes, StrategyDetail::Bore(report)))
}

/// Each of `walls` cut level by level, a stepdown apart, evenly when `even`.
pub(super) fn circular_paths(
    work: &Work<'_>,
    asked: RoundAsked,
    even: bool,
    walls: &[Round],
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let op = work.op;
    let levels = Levels {
        stepdown: op.stepdown(),
        even,
    };
    let (path, report) = circular(walls, levels, &round_params(work, asked)).map_err(in_op(op))?;
    tell(&report, warnings);
    Ok((path, report.passes, StrategyDetail::Circular(report)))
}

/// What a thread asks of each hole, beyond its operation's passes.
#[derive(Debug, Clone, Copy)]
pub(super) struct ThreadAsked {
    pub(super) pitch: f64,
    pub(super) offset: f64,
    pub(super) hand: ThreadHand,
    pub(super) stepovers: u32,
    pub(super) helical: bool,
    pub(super) lead_to_center: bool,
}

/// Each of `walls` threaded with `work`'s thread mill. Where a lead goes below
/// a hole's floor, as Fusion's does in a blind hole whose thread runs to its
/// bottom, the run says where and how far; the part check then fails there,
/// unchanged.
pub(super) fn thread_paths(
    work: &Work<'_>,
    asked: ThreadAsked,
    walls: &[Round],
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let op = work.op;
    let diameter = work.spec.diameter;
    let p = ThreadParams {
        radius: work.tool.cutter_radius(),
        pitch: asked.pitch,
        offset: asked.offset,
        right_hand: asked.hand == ThreadHand::Right,
        kept_on_right: work.kept_on_right(),
        stepovers: asked.stepovers,
        stepover: op.passes.stepover,
        leads: op.leads(diameter).map(level_leads),
        helical: asked.helical,
        to_centre: asked.lead_to_center,
        feed: op.passes.feed,
    };
    let (path, mut report) = thread(walls, &p).map_err(in_op(op))?;
    // The floor under each hole's middle, where it has one, against the
    // lowest its leads go.
    let tolerance = work.job.tolerance;
    for cut in &report.cuts {
        let Some(floor) = work.cutter.drop_at(work.tool, cut.at.x, cut.at.y) else {
            continue;
        };
        let deep = floor - cut.lowest;
        if deep > tolerance && report.below_floor.is_none_or(|(_, d)| deep > d) {
            report.below_floor = Some(([cut.at.x, cut.at.y], deep));
        }
    }
    if let Some(([x, y], deep)) = report.below_floor {
        warnings.push(format!(
            "the thread's lead goes {deep:.3} mm below the floor of the hole at X{x:.3} \
             Y{y:.3}, as Fusion's does where a thread runs to a blind hole's bottom: set the \
             Bottom that much higher, or level leads"
        ));
    }
    tell_dropped(report.dropped, warnings);
    if let Some(ground) = work.spec.pitch
        && (ground - asked.pitch).abs() > 1.0e-6
    {
        warnings.push(format!(
            "the thread's pitch, {:.3} mm, is not the {ground:.3} mm the thread mill's tooth \
             is ground for: its profile will not be the thread's",
            asked.pitch
        ));
    }
    Ok((path, report.passes, StrategyDetail::Thread(report)))
}

/// The round walls a bore's, a circular's or a thread's picked faces make, in
/// the setup's frame, in the order they are cut: each from its own top to its
/// own bottom, or the Top and Bottom where they are set, its bottom raised by
/// the stock to leave down.
pub(in crate::run) fn round_walls(
    job: &Job,
    op: &Operation,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    ctx: &HeightContext,
) -> Result<Vec<Round>, RunError> {
    let frame = &prepared.frame;
    let mut walls: Vec<Wall> = Vec::new();
    let mut tol = job.tolerance.max(0.01);
    for pick in &op.selection.faces {
        let body = body_of(op, bodies, &pick.model)?;
        let near = features::tolerance(job.tolerance, body);
        tol = tol.max(near);
        let found = find_face(body, pick, near).map_err(in_op(op))?;
        if walls
            .iter()
            .any(|w| w.model == pick.model && w.face == found.index)
        {
            continue;
        }
        let t = body.topology();
        let Some(face) = t.face(found.index) else {
            continue;
        };
        let circles = face
            .edges
            .iter()
            .filter_map(|&e| match t.edge(e)?.kind {
                EdgeKind::Circle(c) => Some((
                    frame.to_setup(c.center),
                    c.radius,
                    frame.orientation.apply(c.normal).z.abs() >= 1.0 - 1.0e-6,
                )),
                EdgeKind::Line | EdgeKind::Curve => None,
            })
            .collect();
        let triangles = face
            .triangles
            .iter()
            .filter_map(|&i| body.mesh().triangle(i as usize))
            .map(|tri| tri.map(|p| frame.to_setup(p)))
            .collect();
        walls.push(Wall {
            model: pick.model.clone(),
            face: found.index,
            circles,
            triangles,
        });
    }
    let mut found = rounds(&walls, tol).map_err(in_op(op))?;
    // A Top or Bottom set goes for every wall; left out, each its own, as
    // Fusion's are derived from the face picked.
    let (top, bottom) = op.levels_3d(ctx);
    let axial = op.stock_to_leave().axial;
    for r in &mut found {
        if op.heights.top.is_some() {
            r.top = top;
        }
        if op.heights.bottom.is_some() {
            r.bottom = bottom;
        }
        r.bottom += axial;
    }
    Ok(nearest_first(&found))
}
