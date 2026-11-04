//! Feed optimisation: a path's cuts slowed into and out of its sharp corners
//! and round its tight arcs. Fusion's Feed Optimization.
//!
//! Where a pass turns sharply into the material, as into a pocket's corner,
//! the tool is wrapped in more of it for a moment than along the walls either
//! side, and at the same feed each tooth takes more: the load jumps, the tool
//! deflects and chatters, and a small one breaks. Slowing for that moment cuts
//! the corner at the load of the walls.
//!
//! # What is slowed
//!
//! Cuts, one after another: a corner is where one cut ends and the next starts
//! turning by more than the most it may, and a tight arc one whose radius is
//! less than the least it may have. The cut is slowed from a distance before
//! each to the same distance after, as the load builds going in and falls
//! coming out; spans slowed that meet are one. Nothing else changes: every
//! move goes where it went, cut in two where the feed changes along it.
//!
//! # Inner corners
//!
//! Only the corners that turn into the material load the tool, the corners of
//! a pocket and not a boss's. Seen from above, a pass with the material on its
//! right, climbing with a clockwise spindle, turns into it at a corner that
//! turns left, away from what it is cutting, towards the wall ahead; and the
//! other way round with the material on its left. Where an operation's passes
//! cut both ways, which side the material is on is not known, and every corner
//! seen from above is taken as an inner one. A corner that turns up or down,
//! as over a ridge or along a valley, has the material below it: it is inner
//! where it turns up, into the slope ahead.

use glam::DVec3;

use super::toolpath::{Arc, Move, MoveKind, MoveRole, Toolpath, ToolpathError, starting_at};

/// Pieces shorter than this, in millimetres, are not cut off a move.
const SHORTEST: f64 = 1.0e-6;

/// How a path's corners are slowed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CornerParams {
    /// The most a cut may turn into the next without slowing, in radians:
    /// Fusion's Maximum Directional Change.
    pub most_turn: f64,
    /// An arc tighter than this is slowed along, in millimetres: Fusion's
    /// Reduced Feed Radius.
    pub radius: f64,
    /// How far before a corner or tight arc the cut slows, and after it it
    /// speeds up again, in millimetres: Fusion's Reduced Feed Distance.
    pub distance: f64,
    /// The feed slowed to, in mm/min: Fusion's Reduced Feedrate. A cut
    /// already slower keeps its own.
    pub feed: f64,
    /// Only corners that turn into the material: Fusion's Only Inner Corners.
    pub only_inner: bool,
    /// Which side of the passes the material is on, seen from above: on
    /// their right when `Some(true)`, their left when `Some(false)`, and
    /// either, as where they cut both ways, when `None`.
    pub material_on_right: Option<bool>,
}

/// What feed optimisation did.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CornerReport {
    /// Corners slowed for.
    pub corners: u64,
    /// Tight arcs slowed along.
    pub arcs: u64,
    /// How far the cuts run slowed, in millimetres.
    pub slowed: f64,
}

/// `path` with its cuts slowed into and out of their corners and along their
/// tight arcs, as `params` says.
pub fn slow_corners(
    path: &Toolpath,
    params: &CornerParams,
) -> Result<(Toolpath, CornerReport), ToolpathError> {
    let moves = path.moves();
    let mut report = CornerReport::default();
    let mut out = Toolpath::new();
    let mut i = 0;
    while let Some(m) = moves.get(i) {
        let from = i.checked_sub(1).and_then(|k| moves.get(k)).map(|p| p.to);
        let (Some(from), true) = (from, cuts(m)) else {
            out.push(*m)?;
            i += 1;
            continue;
        };
        // The run of cuts from here.
        let end = (i..moves.len())
            .find(|&k| !moves.get(k).is_some_and(cuts))
            .unwrap_or(moves.len());
        let run = moves.get(i..end).unwrap_or_default();
        slow_run(&mut out, from, run, params, &mut report)?;
        i = end;
    }
    Ok((out, report))
}

/// Whether `m` is a cut, with a feed.
fn cuts(m: &Move) -> bool {
    m.role == MoveRole::Cut && m.feed.is_some()
}

/// Append `run`, cuts one after another from `from`, to `out`, slowed where
/// `params` says.
fn slow_run(
    out: &mut Toolpath,
    from: DVec3,
    run: &[Move],
    params: &CornerParams,
    report: &mut CornerReport,
) -> Result<(), ToolpathError> {
    // Where each move starts along the run, and its length.
    let mut starts = Vec::with_capacity(run.len());
    let mut total = 0.0;
    for (at, m) in starting_at(from, run) {
        let length = m.length(at);
        starts.push((at, total, length));
        total += length;
    }

    // The spans to slow, along the run.
    let d = params.distance.max(0.0);
    let mut spans: Vec<(f64, f64)> = Vec::new();
    for (k, (m, &(at, s, length))) in run.iter().zip(&starts).enumerate() {
        if let MoveKind::Arc(a) = m.kind
            && a.radius(at) < params.radius
            && let (Some(t0), Some(t1)) = (m.tangent_at(at, 0.0), m.tangent_at(at, 0.5))
            && (!params.only_inner || inner(t0, t1, params.material_on_right))
        {
            spans.push((s - d, s + length + d));
            report.arcs += 1;
        }
        let Some(next) = run.get(k + 1) else { continue };
        let (Some(t_in), Some(t_out)) = (m.tangent_at(at, 1.0), next.tangent_at(m.to, 0.0)) else {
            continue;
        };
        let turn = t_in.dot(t_out).clamp(-1.0, 1.0).acos();
        if turn > params.most_turn
            && (!params.only_inner || inner(t_in, t_out, params.material_on_right))
        {
            let corner = s + length;
            spans.push((corner - d, corner + d));
            report.corners += 1;
        }
    }
    let spans = merged(spans, total);

    // Each move, cut where a span starts or ends along it.
    for (m, &(at, s, length)) in run.iter().zip(&starts) {
        // A run is of cuts, which feed; were one not to, it is kept as it is.
        let Some(feed) = m.feed else {
            out.push(*m)?;
            continue;
        };
        let slow = params.feed.min(feed);
        let mut breaks: Vec<f64> = vec![0.0];
        for &(a, b) in &spans {
            for edge in [a, b] {
                let t = edge - s;
                if t > SHORTEST && t < length - SHORTEST {
                    breaks.push(t / length);
                }
            }
        }
        breaks.push(1.0);
        breaks.sort_by(f64::total_cmp);
        for w in breaks.windows(2) {
            let (&[t0, t1], Some(&last)) = (w, breaks.last()) else {
                continue;
            };
            let to = if t1 >= last { m.to } else { m.point_at(at, t1) };
            let middle = s + length * (t0 + t1) * 0.5;
            let slowed = spans.iter().any(|&(a, b)| middle > a && middle < b);
            // A piece of an arc is the same arc, turning its share.
            let kind = match m.kind {
                MoveKind::Linear => MoveKind::Linear,
                MoveKind::Arc(a) => MoveKind::Arc(Arc {
                    sweep: a.sweep * (t1 - t0),
                    ..a
                }),
            };
            if slowed {
                report.slowed += length * (t1 - t0);
            }
            out.push(Move {
                to,
                kind,
                feed: Some(if slowed { slow } else { feed }),
                ..*m
            })?;
        }
    }
    Ok(())
}

/// Whether a turn from `t_in` to `t_out` is into the material, the material
/// on the right of the passes seen from above when `right` says so.
fn inner(t_in: DVec3, t_out: DVec3, right: Option<bool>) -> bool {
    let left = t_in.x * t_out.y - t_in.y * t_out.x;
    let up = t_out.z - t_in.z;
    if up.abs() > left.abs() {
        // Over a ridge or along a valley, the material below.
        return up > 0.0;
    }
    // Into the material is away from it, towards the wall ahead.
    right.is_none_or(|right| (left > 0.0) == right)
}

/// `spans` clipped to `0..=total`, those that meet or overlap made one, in
/// order.
fn merged(mut spans: Vec<(f64, f64)>, total: f64) -> Vec<(f64, f64)> {
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(spans.len());
    for (a, b) in spans {
        let (a, b) = (a.max(0.0), b.min(total));
        if b <= a {
            continue;
        }
        match out.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::indexing_slicing)]
mod tests;
