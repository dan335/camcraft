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
//! As Fusion's Slot cuts one, from its programs for `ops_2d`'s slots: the
//! tool comes down to the ramp clearance height over the slot's top, above
//! where its middle starts, and ramps back and forth along the whole middle,
//! end to end, each leg falling as far, no more than half the maximum ramp
//! stepdown (the most there and back) and no more steeply than the ramp angle,
//! to the floor; then once along the floor at the cutting feed, from the end
//! the ramp ended at to the other. With Multiple Depths each level, a whole
//! stepdown and the last what is left, is ramped down to so from the one above
//! and cut along. A slot is one pass, its ramp its own. A tool narrower than
//! the slot cuts its middle only, as Fusion's does, and leaves its walls; the
//! report says how much. A tool wider than the slot is refused.
//!
//! Where asked, the floor is cut a second time, back the way it came
//! (Fusion's Repeat Finishing Pass), and at the end of each slot the tool backs
//! off the end wall along the middle before it lifts (Fusion's Backoff
//! Distance).

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
            r.arc.is_some_and(|(_, radius, sweep)| {
                sweep > 0.0 && (sweep - PI).abs() * radius <= 4.0 * tol
            })
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
    /// Cutting feed, along the floor, in mm/min.
    pub feed: f64,
    /// Feed down the ramp, in mm/min.
    pub ramp_feed: f64,
    /// The steepest the ramp may fall, in radians from level.
    pub ramp_angle: f64,
    /// The most the ramp may fall there and back along the slot, in
    /// millimetres: Fusion's Maximum Ramp Stepdown. Each way, half of it.
    pub ramp_stepdown: f64,
    /// How far above the slot's top the ramp starts, in millimetres: Fusion's
    /// Ramp Clearance Height.
    pub ramp_clearance: f64,
    /// How far the tool backs off the wall at the end of each slot, in
    /// millimetres.
    pub backoff: f64,
    /// Whether the last pass along the floor is cut twice.
    pub repeat: bool,
    /// How close two ends must be to be one, in millimetres.
    pub tolerance: f64,
}

/// What a slot operation did.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SlotReport {
    /// Slots cut.
    pub slots: u64,
    /// Passes along their floors, every level's.
    pub passes: u64,
    /// The most levels any was cut in.
    pub levels: u64,
    /// Legs of ramp, one way along a slot each.
    pub legs: u64,
    /// Slots wider than the tool, whose walls are left uncut, as Fusion's are.
    pub narrower: u64,
    /// The most left on a wall of one of them, in millimetres.
    pub most_left: f64,
}

/// The most legs a ramp down one level of a slot may have.
const MOST_LEGS: f64 = 10_000.0;

fn check_params(p: &SlotParams) -> Result<(), StrategyError> {
    check("tool radius", p.radius)?;
    check("feed", p.feed)?;
    check("ramp feed", p.ramp_feed)?;
    check("ramp stepdown", p.ramp_stepdown)?;
    check("tolerance", p.tolerance)?;
    at_least_zero("ramp clearance height", p.ramp_clearance)?;
    at_least_zero("backoff distance", p.backoff)?;
    if !(p.ramp_angle > 0.0 && p.ramp_angle < PI / 2.0) {
        return Err(StrategyError::BadParameter {
            name: "ramp angle",
            value: p.ramp_angle,
        });
    }
    if let Some(d) = p.stepdown {
        check("stepdown", d)?;
    }
    Ok(())
}

/// Each of `slots` cut along its middle, as Fusion's Slot cuts one: down by
/// ramping back and forth along the whole middle, end to end, each leg
/// falling as far, then once along it at the level, from the end the ramp
/// ended at to the other. A slot is one pass: a link to above where its
/// middle starts, its levels one after another, and at the end, where asked,
/// the floor again and the tool backed off the wall. A tool narrower than a
/// slot leaves its walls, as Fusion's does.
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
        if s.top - s.bottom <= 1.0e-9 || s.top.is_nan() || s.bottom.is_nan() {
            return Err(SlotError::NoDepth { x: at.x, y: at.y });
        }
        let flat = s.middle.raised(-s.middle.start.z);
        let length = flat.flat_length();
        if length.is_nan() || length <= tol {
            return Err(SlotError::NotASlot { x: at.x, y: at.y });
        }
        let zs: Vec<f64> = match p.stepdown {
            Some(d) => slabs(s.top, s.bottom, d, &[])
                .into_iter()
                .map(|(z, _)| z)
                .collect(),
            None => vec![s.bottom],
        };
        report.slots += 1;
        report.levels = report.levels.max(zs.len() as u64);
        if s.radius - p.radius > tol {
            report.narrower += 1;
            report.most_left = report.most_left.max(s.radius - p.radius);
        }
        // The most a leg falls: half the most there and back, and no steeper
        // than the ramp's angle along the slot.
        let per_leg = (p.ramp_stepdown / 2.0).min(length * p.ramp_angle.tan());
        let back = flat.reversed();
        // Which way along it the tool goes next: from its start, or back.
        let mut forward = true;
        let mut high = s.top + p.ramp_clearance;
        path.feed_to(at.truncate().extend(high), MoveRole::Link, p.ramp_feed)?;
        let mut last = flat.raised(s.bottom);
        for &z in &zs {
            let fall = high - z;
            let legs = (fall / per_leg - 1.0e-9).ceil().max(1.0);
            if legs.is_nan() || legs > MOST_LEGS {
                return Err(StrategyError::BadParameter {
                    name: "ramp stepdown",
                    value: p.ramp_stepdown,
                }
                .into());
            }
            let n = camcraft_geom::steps::bounded_count(legs);
            for k in 0..n {
                let from = high - fall * f64::from(k) / f64::from(n);
                let to = high - fall * f64::from(k + 1) / f64::from(n);
                let way = if forward { &flat } else { &back };
                way.cut_falling(&mut path, (from, to), MoveRole::Ramp, p.ramp_feed)?;
                forward = !forward;
                report.legs += 1;
            }
            last = (if forward { &flat } else { &back }).raised(z);
            last.cut_into(&mut path, MoveRole::Cut, p.feed)?;
            forward = !forward;
            report.passes += 1;
            high = z;
        }
        if p.repeat {
            last = last.reversed();
            last.cut_into(&mut path, MoveRole::Cut, p.feed)?;
            report.passes += 1;
        }
        if p.backoff > 0.0 {
            last.back_from_end(p.backoff)
                .cut_into(&mut path, MoveRole::Lead, p.feed)?;
        }
    }
    Ok((path, report))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::panic
)]
mod tests;
