//! Slots: picked by the edges round their floors, and milled along their
//! middles. Fusion's Slot.
//!
//! # From the picked loop
//!
//! A slot is picked as Fusion's is, by the edges round its floor: a loop, level,
//! with two sides a constant width apart and a round end at each end, half that
//! width across ([`slot_of`]). Its sides may be straight, arcs about one centre,
//! or several of either; its middle is what is cut along. Each end is an arc of a
//! half turn; of the pairs of them the same size, the first whose sides keep
//! every point of the loop half the width from the middle between them is the
//! slot's. A loop that has none is not a slot, and is refused.
//!
//! # Passes
//!
//! Each slot is cut level by level from its top down to its floor, a whole
//! stepdown each and the last what is left, or straight to the floor without
//! Multiple Depths, as Fusion's Slot ramps to the depth. At each level the tool
//! runs along the middle from one end to the other, each level the other way
//! from the one above, so it starts where the last ended; the linker takes it
//! down to each by ramping back and forth along the middle. A tool narrower than
//! the slot then goes once round its walls at the level, stepping straight across
//! to them from the end of the middle, so the slot is cut to its width; one more
//! than three times narrower leaves a band either side of the middle, and the
//! report says how wide. A tool wider than the slot is refused.
//!
//! The last pass at the floor is cut twice where asked (Fusion's Repeat
//! Finishing Pass), and at the end of each slot the tool backs off the wall
//! before it lifts (Fusion's Backoff Distance): back along the middle, or in
//! from the wall towards it.

use std::f64::consts::PI;

use glam::{DVec2, DVec3};

use crate::path::toolpath::{MoveRole, Toolpath, ToolpathError};
use crate::strategy::curve::{Curve, Piece, offset};
use crate::strategy::{StrategyError, at_least_zero, check, slabs};

/// One slot, in the setup's frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    /// Its middle at its floor, from one end's centre to the other's.
    pub middle: Curve,
    /// Half its width: its ends' radius.
    pub radius: f64,
    /// The height it is cut down from.
    pub top: f64,
    /// The height the tool's tip goes down to.
    pub bottom: f64,
}

/// Why picked edges are not slots, or a slot cannot be cut.
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum SlotError {
    /// Nothing was picked.
    #[error("pick the edges round the floor of each slot to cut, or its floor")]
    NothingPicked,
    /// Picked edges that do not close round a floor.
    #[error(
        "the picked edges do not close round a slot's floor: one ends at \
         ({:.3}, {:.3}, {:.3}) with nothing picked to carry on from it",
        at[0], at[1], at[2]
    )]
    Open {
        /// The loose end, in the setup's frame.
        at: [f64; 3],
    },
    /// A picked loop that does not lie level.
    #[error(
        "the loop picked at ({x:.3}, {y:.3}) does not lie level in this setup: a slot is \
         picked by the edges round its floor"
    )]
    NotLevel {
        /// Where.
        x: f64,
        /// Where.
        y: f64,
    },
    /// A picked loop that is not a slot's.
    #[error(
        "the loop picked at ({x:.3}, {y:.3}) is not a slot: two sides a constant width \
         apart, straight or round, with a round end half that width across at each end"
    )]
    NotASlot {
        /// Where it starts.
        x: f64,
        /// Where it starts.
        y: f64,
    },
    /// A slot narrower than the tool.
    #[error(
        "the slot at ({x:.3}, {y:.3}) is {width:.3} mm wide: narrower than the {tool:.3} mm \
         tool; use one no wider than the slot"
    )]
    TooWide {
        /// Where its middle starts.
        x: f64,
        /// Where its middle starts.
        y: f64,
        /// Its width.
        width: f64,
        /// The tool's diameter.
        tool: f64,
    },
    /// A slot whose floor is at or above its top.
    #[error("the slot at ({x:.3}, {y:.3}) has its floor at or above its top")]
    NoDepth {
        /// Where its middle starts.
        x: f64,
        /// Where its middle starts.
        y: f64,
    },
    /// The path could not be made.
    #[error(transparent)]
    Strategy(#[from] StrategyError),
}

impl From<ToolpathError> for SlotError {
    fn from(e: ToolpathError) -> Self {
        SlotError::Strategy(e.into())
    }
}

/// The most pairs of round ends a loop is tried for: a slot has two.
const MOST_PAIRS: usize = 64;

/// A run of a loop's pieces: an arc about one centre, made of one piece or
/// several, or a piece of anything else.
#[derive(Debug, Clone)]
struct Run {
    /// Where it starts.
    start: DVec3,
    /// Its pieces.
    pieces: Vec<Piece>,
    /// Its centre and radius, and how far round it turns, for an arc.
    arc: Option<(DVec2, f64, f64)>,
}

/// The middle and half-width of the slot `boundary`, a closed level loop, goes
/// round, to `tol`.
pub fn slot_of(boundary: &Curve, tol: f64) -> Result<(Curve, f64), SlotError> {
    let tol = tol.max(1.0e-6);
    let start = boundary.start;
    let not_a_slot = || SlotError::NotASlot {
        x: start.x,
        y: start.y,
    };
    let points = boundary.sample(tol);
    if points.iter().any(|p| (p.z - start.z).abs() > tol) {
        return Err(SlotError::NotLevel {
            x: start.x,
            y: start.y,
        });
    }
    // Anticlockwise, so what it closes is on its left.
    let ring = if boundary.signed_area() < 0.0 {
        boundary.reversed()
    } else {
        boundary.clone()
    };
    let runs = runs(&ring, tol).ok_or_else(not_a_slot)?;
    // Its round ends: arcs turning a half turn, convex.
    let ends: Vec<usize> = runs
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            r.arc
                .is_some_and(|(_, radius, sweep)| sweep > 0.0 && (sweep - PI).abs() * radius <= 4.0 * tol)
        })
        .map(|(i, _)| i)
        .collect();
    let n = runs.len();
    let mut tried = 0;
    for (k, &a) in ends.iter().enumerate() {
        for &b in ends.iter().skip(k + 1) {
            tried += 1;
            if tried > MOST_PAIRS {
                return Err(not_a_slot());
            }
            let (Some(ra), Some(rb)) = (runs.get(a), runs.get(b)) else {
                continue;
            };
            let (Some((ca, ra_r, _)), Some((cb, rb_r, _))) = (ra.arc, rb.arc) else {
                continue;
            };
            if (ra_r - rb_r).abs() > tol || b == a + 1 || (a == 0 && b == n - 1) {
                continue;
            }
            let radius = (ra_r + rb_r) / 2.0;
            // One side, from the end of the first round end to the start of
            // the other; its middle is half the width in from it.
            let side_runs = runs.get(a + 1..b).unwrap_or(&[]);
            let Some(first) = side_runs.first() else {
                continue;
            };
            let side = Curve {
                start: first.start,
                pieces: side_runs.iter().flat_map(|r| r.pieces.clone()).collect(),
            };
            let Ok(middle) = offset(&side, radius, tol) else {
                continue;
            };
            if middle.start.truncate().distance(ca) > 2.0 * tol
                || middle.end().truncate().distance(cb) > 2.0 * tol
            {
                continue;
            }
            // Every point of the loop half the width from the middle.
            let even = points
                .iter()
                .all(|p| (middle.distance_to(p.truncate()) - radius).abs() <= 2.0 * tol);
            if even {
                return Ok((middle, radius));
            }
        }
    }
    Err(not_a_slot())
}

/// `ring`'s pieces in runs, each arc about one centre one run, starting where
/// one run ends and another begins; `None` where it is all one arc, a circle.
fn runs(ring: &Curve, tol: f64) -> Option<Vec<Run>> {
    let steps: Vec<(DVec3, Piece)> = ring.steps().map(|(f, p)| (f, *p)).collect();
    let n = steps.len();
    let circle = |(from, p): &(DVec3, Piece)| match *p {
        Piece::Arc { centre, sweep, .. } => Some((centre, from.truncate().distance(centre), sweep)),
        Piece::Line { .. } => None,
    };
    let same = |a: Option<(DVec2, f64, f64)>, b: Option<(DVec2, f64, f64)>| match (a, b) {
        (Some((c1, r1, s1)), Some((c2, r2, s2))) => {
            c1.distance(c2) <= tol && (r1 - r2).abs() <= tol && s1 * s2 > 0.0
        }
        _ => false,
    };
    // A piece that does not carry on the run before it.
    let first = (0..n).find(|&i| {
        let before = steps.get((i + n - 1) % n).and_then(circle);
        !same(before, steps.get(i).and_then(circle))
    })?;
    let mut out: Vec<Run> = Vec::new();
    for k in 0..n {
        let Some(step) = steps.get((first + k) % n) else {
            continue;
        };
        let arc = circle(step);
        match out.last_mut() {
            Some(run) if same(run.arc, arc) => {
                run.pieces.push(step.1);
                if let (Some(r), Some((_, _, s))) = (run.arc.as_mut(), arc) {
                    r.2 += s;
                }
            }
            _ => out.push(Run {
                start: step.0,
                pieces: vec![step.1],
                arc,
            }),
        }
    }
    Some(out)
}

/// How each slot is cut.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlotParams {
    /// The tool's radius.
    pub radius: f64,
    /// The deepest a level may be below the one above, in millimetres; the
    /// floor alone when `None`.
    pub stepdown: Option<f64>,
    /// Cutting feed, in mm/min.
    pub feed: f64,
    /// How far the tool backs off the wall at the end of each slot, in
    /// millimetres.
    pub backoff: f64,
    /// Whether the last pass at the floor is cut twice.
    pub repeat: bool,
    /// Whether the material kept is on the tool's right going round the
    /// walls: climbing with a clockwise spindle.
    pub kept_on_right: bool,
    /// How close two ends must be to be one, in millimetres.
    pub tolerance: f64,
}

/// What a slot operation did.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SlotReport {
    /// Slots cut.
    pub slots: u64,
    /// Passes along them and round them, every level's.
    pub passes: u64,
    /// The most levels any was cut in.
    pub levels: u64,
    /// Slots wider than the tool, cut round their walls as well.
    pub walls: u64,
    /// Slots more than three tools wide, which leave a band either side of the
    /// middle uncut.
    pub bands: u64,
    /// The widest band left, in millimetres.
    pub widest_band: f64,
}

fn check_params(p: &SlotParams) -> Result<(), StrategyError> {
    check("tool radius", p.radius)?;
    check("feed", p.feed)?;
    check("tolerance", p.tolerance)?;
    at_least_zero("backoff distance", p.backoff)?;
    if let Some(d) = p.stepdown {
        check("stepdown", d)?;
    }
    Ok(())
}

/// Each of `slots` cut along its middle, level by level: Fusion's Slot. Each
/// pass a link to where it starts, the middle, and the walls where the tool is
/// narrower than the slot.
pub fn slot(slots: &[Slot], p: &SlotParams) -> Result<(Toolpath, SlotReport), SlotError> {
    check_params(p)?;
    let tol = p.tolerance;
    let mut path = Toolpath::new();
    let mut report = SlotReport::default();
    for s in slots {
        let at = s.middle.start;
        if p.radius > s.radius + tol {
            return Err(SlotError::TooWide {
                x: at.x,
                y: at.y,
                width: 2.0 * s.radius,
                tool: 2.0 * p.radius,
            });
        }
        if !(s.top - s.bottom > 1.0e-9) {
            return Err(SlotError::NoDepth { x: at.x, y: at.y });
        }
        let zs: Vec<f64> = match p.stepdown {
            Some(d) => slabs(s.top, s.bottom, d, &[])
                .into_iter()
                .map(|(z, _)| z)
                .collect(),
            None => vec![s.bottom],
        };
        // How far the tool's centre goes from the middle round the walls.
        let walls = (s.radius - p.radius > tol).then_some(s.radius - p.radius);
        report.slots += 1;
        report.levels = report.levels.max(zs.len() as u64);
        if let Some(h) = walls {
            report.walls += 1;
            let band = h - 2.0 * p.radius;
            if band > tol {
                report.bands += 1;
                report.widest_band = report.widest_band.max(band);
            }
        }
        let flat = s.middle.raised(-s.middle.start.z);
        let last = zs.len().saturating_sub(1);
        for (i, &z) in zs.iter().enumerate() {
            let along = if i % 2 == 0 {
                flat.raised(z)
            } else {
                flat.reversed().raised(z)
            };
            let ring = match walls {
                Some(h) => Some(round_walls(&along, h, p.kept_on_right, tol)?),
                None => None,
            };
            let times = if p.repeat && i == last { 2 } else { 1 };
            pass(&mut path, (&along, ring.as_ref()), p.feed, times, &mut report)?;
            if i == last && p.backoff > 0.0 {
                back_off(&mut path, (&along, ring.as_ref()), walls, p)?;
            }
        }
    }
    Ok((path, report))
}

/// One level of a slot: a link to the start of `along`, its middle, then where
/// there are walls to go round, straight across to them and round, `ring`. Its
/// last pass, the walls or the middle, cut `times` over.
fn pass(
    path: &mut Toolpath,
    (along, ring): (&Curve, Option<&Curve>),
    feed: f64,
    times: u32,
    report: &mut SlotReport,
) -> Result<(), ToolpathError> {
    path.feed_to(along.start, MoveRole::Link, feed)?;
    along.cut_into(path, MoveRole::Cut, feed)?;
    report.passes += 1;
    if let Some(ring) = ring {
        path.feed_to(ring.start, MoveRole::Cut, feed)?;
        ring.cut_into(path, MoveRole::Cut, feed)?;
        report.passes += 1;
    }
    for _ in 1..times {
        let again = ring.unwrap_or(along);
        path.feed_to(again.start, MoveRole::Link, feed)?;
        again.cut_into(path, MoveRole::Cut, feed)?;
        report.passes += 1;
    }
    Ok(())
}

/// Back off the wall from where the last pass ended: in from the walls
/// towards the middle, or back along the middle.
fn back_off(
    path: &mut Toolpath,
    (along, ring): (&Curve, Option<&Curve>),
    walls: Option<f64>,
    p: &SlotParams,
) -> Result<(), ToolpathError> {
    match (ring, walls) {
        (Some(ring), Some(h)) => {
            let end = ring.end();
            let towards = (along.end() - end).truncate().normalize_or_zero();
            let to = end + (towards * p.backoff.min(h)).extend(0.0);
            path.feed_to(to, MoveRole::Lead, p.feed)?;
        }
        _ => {
            along
                .back_from_end(p.backoff)
                .cut_into(path, MoveRole::Lead, p.feed)?;
        }
    }
    Ok(())
}

/// The walls of the slot whose middle is `along`, gone round `h` from it:
/// from beside the end `along` ends at, round that end, back along the
/// other side, round the other end and along to where it started.
/// Anticlockwise, so the walls are on the tool's right, when `kept_on_right`.
fn round_walls(along: &Curve, h: f64, kept_on_right: bool, tol: f64) -> Result<Curve, SlotError> {
    let at = along.start;
    let not_a_slot = |_| SlotError::NotASlot { x: at.x, y: at.y };
    let right = offset(along, -h, tol).map_err(not_a_slot)?;
    let back = offset(&along.reversed(), -h, tol).map_err(not_a_slot)?;
    let (a, b) = (along.start.truncate(), along.end().truncate());
    let mut pieces = vec![Piece::Arc {
        to: back.start,
        centre: b,
        sweep: PI,
    }];
    pieces.extend(back.pieces.iter().copied());
    pieces.push(Piece::Arc {
        to: right.start,
        centre: a,
        sweep: PI,
    });
    pieces.extend(right.pieces.iter().copied());
    let ring = Curve {
        start: right.end(),
        pieces,
    };
    Ok(if kept_on_right { ring } else { ring.reversed() })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::panic
)]
mod tests;
