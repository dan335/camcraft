//! Threads milled round holes picked by their walls, with a single-profile
//! thread mill: Fusion's Thread.
//!
//! # From the picked walls
//!
//! A hole is picked by its round wall, as a bore's is, and read the same way
//! ([`crate::strategy::round::rounds`]): its axis, its radius, and the top and
//! bottom of its wall. A boss's wall, an outside thread, is refused.
//!
//! # A pass
//!
//! The tooth's point goes round a circle the tool's radius in from the wall,
//! and half the pitch diameter offset out: with none it only touches the wall,
//! with a millimetre it cuts half a millimetre into it. Each pass, as Fusion's
//! programs for its Thread go:
//!
//! - down the hole's centre to where the lead starts, a link the linker makes;
//! - along a straight to the lead's arc (Lead To Center), round the arc onto
//!   the thread's circle, tangent to it where the helix starts, on +X;
//! - round the helix at the pitch, from the bottom of the wall to its top, or
//!   from its top to its bottom, in arcs of half a turn and what is left;
//! - off it round an arc the same size, and back along a straight to the
//!   centre, where the linker lifts.
//!
//! The arcs' radius is a tenth of the tool's diameter, no more than half the
//! circle's, each a quarter turn. With helical leads they rise or fall as the
//! helix does, at its pitch round the circle it goes round: the lead in starts
//! below the helix's start, by the arc's length at the helix's slope, and the
//! lead off ends as far past its end: 0.117 mm in a hole 16 across with a
//! 9.14 mm thread mill at 1.75 mm. Without Lead To Center a pass starts the
//! leads' straight before its arc, along it, and ends as far past the arc off.
//! Without helical leads the arcs are level, at the helix's ends.
//!
//! # Which way
//!
//! The hand and the direction between them decide which way round the helix
//! goes and whether it climbs or falls. A right-hand thread rises
//! anticlockwise, seen from above, and a left-hand one clockwise. With a
//! clockwise spindle a climb cut keeps the wall on the tool's right: round a
//! hole that is anticlockwise. So a right-hand thread climbing rises from the
//! bottom, as Fusion's does, and conventional falls clockwise from the top; a
//! left-hand thread climbing falls anticlockwise from the top, and
//! conventional rises clockwise from the bottom. Only the first is held to a
//! program of Fusion's.
//!
//! # With multiple passes
//!
//! Each pass a stepover further in than the next, the last at the thread: each
//! a whole pass of its own, from the centre and back.
//!
//! # What it cuts on purpose
//!
//! The model has a plain hole: a thread cut into its wall, as deep as the
//! offset asks, is meant. Each hole's [`ThreadCut`] says how far out the
//! tooth reaches, down to the thread's bottom, so the part the material is
//! checked against is lowered there, and nowhere else ([`ThreadCut::height_at`]).

use std::f64::consts::{PI, TAU};

use glam::{DVec2, DVec3, dvec3};

use crate::path::toolpath::{Arc, MoveRole, PLUNGE_RATIO, Toolpath, ToolpathError};
use crate::strategy::planar::LeadArc;
use crate::strategy::round::Round;
use crate::strategy::{StrategyError, at_least_zero, check};

/// A circle no smaller than this, as the tool's centre goes, in millimetres.
const LEAST_RADIUS: f64 = 1.0e-3;

/// A pass before the last on a circle no bigger than this, as the tool's
/// centre goes, is past the hole's middle and left out, in millimetres.
const SLIVER: f64 = 0.1;

/// The most turns one helix makes.
const MOST_TURNS: f64 = 10_000.0;

/// How the threads are milled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreadParams {
    /// The tool's radius, to its tooth's point.
    pub radius: f64,
    /// How far the thread rises a turn, in millimetres.
    pub pitch: f64,
    /// How much wider across than the hole the thread's pitch diameter is,
    /// in millimetres: the tooth goes half of it into the wall.
    pub offset: f64,
    /// A right-hand thread, rising anticlockwise; otherwise left.
    pub right_hand: bool,
    /// Whether the wall is kept on the tool's right: climbing with a
    /// clockwise spindle.
    pub kept_on_right: bool,
    /// How many passes out to the thread, the last at it.
    pub stepovers: u32,
    /// The distance between them, in millimetres.
    pub stepover: f64,
    /// The arcs each pass is entered and left along, if any.
    pub leads: Option<LeadArc>,
    /// Whether the leads rise or fall with the helix, at its pitch.
    pub helical: bool,
    /// Each pass from the hole's centre, and back to it.
    pub to_centre: bool,
    /// Cutting feed, in mm/min, along the leads too.
    pub feed: f64,
}

/// One hole's thread, as cut: what the part is meant to be once threaded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreadCut {
    /// Where its axis is.
    pub at: DVec2,
    /// The hole's radius, as the model has it.
    pub hole: f64,
    /// How far from the axis the tooth reaches: the hole's radius and half
    /// the pitch diameter offset.
    pub reach: f64,
    /// The thread's bottom, the hole's.
    pub bottom: f64,
    /// The lowest the tool's tip goes in the hole, its leads' too.
    pub lowest: f64,
}

impl ThreadCut {
    /// The height the part is meant to be at `p` once threaded, where the
    /// thread cuts into the hole's wall: down to the thread's bottom, between
    /// the hole's wall and the tooth's reach; `None` anywhere else. A thread
    /// cut further out, or a cut anywhere else, is still a gouge.
    pub fn height_at(&self, p: DVec2) -> Option<f64> {
        let d = p.distance(self.at);
        (d > self.hole && d <= self.reach).then_some(self.bottom)
    }
}

/// What a thread did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThreadReport {
    /// Holes threaded.
    pub holes: u64,
    /// Passes round them.
    pub passes: u64,
    /// How many turns the helices make, all of them together.
    pub turns: f64,
    /// Whether the helices rise from the bottom; otherwise they fall from
    /// the top.
    pub rising: bool,
    /// Passes asked for past a hole's middle, and left out.
    pub dropped: u64,
    /// Each hole's thread, in the order cut: what it cuts into the part on
    /// purpose.
    pub cuts: Vec<ThreadCut>,
    /// Where a lead goes deepest below the floor of a hole, and how far,
    /// where one does: set by the run, which knows the part.
    pub below_floor: Option<([f64; 2], f64)>,
}

/// Why a hole cannot be threaded.
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum ThreadError {
    /// A boss's wall was picked: an outside thread.
    #[error(
        "the round wall at ({x:.3}, {y:.3}) is a boss's: only threads inside holes are \
         milled yet"
    )]
    Boss {
        /// Where.
        x: f64,
        /// Where.
        y: f64,
    },
    /// A hole no wider than the thread mill.
    #[error(
        "the hole at ({x:.3}, {y:.3}) is {across:.3} mm across: no wider than the {tool:.3} mm \
         thread mill; use a smaller one"
    )]
    TooSmall {
        /// Where.
        x: f64,
        /// Where.
        y: f64,
        /// Its diameter.
        across: f64,
        /// The tool's.
        tool: f64,
    },
    /// A pitch diameter offset that takes the thread inside the tool.
    #[error(
        "a pitch diameter offset of {offset:.3} mm leaves the thread mill no room in the hole \
         at ({x:.3}, {y:.3})"
    )]
    NoRoom {
        /// Where.
        x: f64,
        /// Where.
        y: f64,
        /// The offset asked.
        offset: f64,
    },
    /// A helix that would fall more steeply than a plunge.
    #[error(
        "the thread round the circle {across:.3} mm across at ({x:.3}, {y:.3}) rises \
         {pitch:.3} mm a turn, more steeply than a plunge: use a smaller pitch"
    )]
    TooSteep {
        /// Where.
        x: f64,
        /// Where.
        y: f64,
        /// The circle's diameter, as the tool's centre goes.
        across: f64,
        /// How far it rises a turn.
        pitch: f64,
    },
    /// A hole whose bottom is at or above its top.
    #[error("the round wall at ({x:.3}, {y:.3}) has its bottom at or above its top")]
    NoDepth {
        /// Where.
        x: f64,
        /// Where.
        y: f64,
    },
    /// The path could not be made.
    #[error(transparent)]
    Strategy(#[from] StrategyError),
}

impl From<ToolpathError> for ThreadError {
    fn from(e: ToolpathError) -> Self {
        ThreadError::Strategy(e.into())
    }
}

fn check_params(p: &ThreadParams) -> Result<(), StrategyError> {
    check("tool radius", p.radius)?;
    check("pitch", p.pitch)?;
    check("feed", p.feed)?;
    if !p.offset.is_finite() {
        return Err(StrategyError::BadParameter {
            name: "pitch diameter offset",
            value: p.offset,
        });
    }
    if p.stepovers > 1 {
        check("stepover", p.stepover)?;
    }
    if let Some(l) = &p.leads {
        check("lead radius", l.radius)?;
        check("lead sweep", l.sweep)?;
        at_least_zero("lead straight", l.straight)?;
    }
    Ok(())
}

/// Which way round the helices go, anticlockwise seen from above or not, and
/// whether they rise from the bottom: round a hole the wall is on the right
/// of a pass going anticlockwise, and a right-hand thread rises that way.
pub fn turning(p: &ThreadParams) -> (bool, bool) {
    let ccw = p.kept_on_right;
    (ccw, ccw == p.right_hand)
}

/// Each of `rounds` threaded, in order: Fusion's Thread.
pub fn thread(rounds: &[Round], p: &ThreadParams) -> Result<(Toolpath, ThreadReport), ThreadError> {
    check_params(p)?;
    let (ccw, rising) = turning(p);
    let mut path = Toolpath::new();
    let mut report = ThreadReport {
        rising,
        ..ThreadReport::default()
    };
    for round in rounds {
        let (x, y) = (round.at.x, round.at.y);
        if round.boss {
            return Err(ThreadError::Boss { x, y });
        }
        let depth = round.top - round.bottom;
        if depth.is_nan() || depth <= 1.0e-9 {
            return Err(ThreadError::NoDepth { x, y });
        }
        if round.radius - p.radius <= LEAST_RADIUS || round.radius.is_nan() {
            return Err(ThreadError::TooSmall {
                x,
                y,
                across: 2.0 * round.radius,
                tool: 2.0 * p.radius,
            });
        }
        let last = round.radius - p.radius + 0.5 * p.offset;
        if last.is_nan() || last <= LEAST_RADIUS {
            return Err(ThreadError::NoRoom {
                x,
                y,
                offset: p.offset,
            });
        }
        let turns = depth / p.pitch;
        if !(turns.is_finite() && turns <= MOST_TURNS) {
            return Err(StrategyError::BadParameter {
                name: "pitch",
                value: p.pitch,
            }
            .into());
        }
        let (from, to) = if rising {
            (round.bottom, round.top)
        } else {
            (round.top, round.bottom)
        };
        let mut lowest = f64::INFINITY;
        let n = p.stepovers.max(1);
        for k in (0..n).rev() {
            let rho = last - f64::from(k) * p.stepover;
            if k > 0 && rho <= SLIVER {
                report.dropped += 1;
                continue;
            }
            if p.pitch > PLUNGE_RATIO * TAU * rho {
                return Err(ThreadError::TooSteep {
                    x,
                    y,
                    across: 2.0 * rho,
                    pitch: p.pitch,
                });
            }
            let low = pass(&mut path, round.at, (rho, ccw), (from, to), p)?;
            lowest = lowest.min(low);
            report.passes += 1;
            report.turns += turns;
        }
        report.holes += 1;
        report.cuts.push(ThreadCut {
            at: round.at,
            hole: round.radius,
            reach: last + p.radius,
            bottom: round.bottom,
            lowest,
        });
    }
    Ok((path, report))
}

/// The point at `angle` round the circle of `rho` about `centre`.
fn on_circle(centre: DVec2, rho: f64, angle: f64) -> DVec2 {
    centre + DVec2::from_angle(angle) * rho
}

/// An arc about an upright axis through `centre`, turning `sweep`.
fn upright(centre: DVec2, sweep: f64) -> Arc {
    Arc {
        centre: dvec3(centre.x, centre.y, 0.0),
        axis: DVec3::Z,
        sweep,
    }
}

/// The lead arc a pass on the circle of `rho` has, if it has one: its radius,
/// no more than half the circle's, its sweep and its straight.
fn lead_arc(rho: f64, p: &ThreadParams) -> Option<(f64, f64, f64)> {
    let l = p.leads?;
    let a = l.radius.min(0.5 * rho);
    (a > LEAST_RADIUS).then_some((a, l.sweep, l.straight))
}

/// One pass round the hole about `c`: its helix round the circle of `rho`,
/// anticlockwise when `ccw`, from height `from` to `to`, with its leads. The
/// lowest the tool's tip goes on it.
fn pass(
    path: &mut Toolpath,
    c: DVec2,
    (rho, ccw): (f64, bool),
    (from, to): (f64, f64),
    p: &ThreadParams,
) -> Result<f64, ToolpathError> {
    let way = if ccw { 1.0 } else { -1.0 };
    // Which way the helix goes in height: up from the bottom, or down.
    let up = if to >= from { 1.0 } else { -1.0 };
    let total = way * TAU * (to - from).abs() / p.pitch;
    // How far it rises along each millimetre it goes round.
    let slope = p.pitch / (TAU * rho);
    let feed = p.feed;
    let start = on_circle(c, rho, 0.0);
    let lead = lead_arc(rho, p);
    let rise = |a: f64, sweep: f64| if p.helical { a * sweep * slope } else { 0.0 };

    // In: down the centre (the linker's), along the straight, round the arc.
    let z_in = match lead {
        Some((a, sweep, straight)) => {
            let z_in = from - up * rise(a, sweep);
            let q = c + DVec2::from_angle(0.0) * (rho - a);
            let at = -way * sweep;
            let begin = q + DVec2::from_angle(at) * a;
            if p.to_centre {
                path.feed_to(dvec3(c.x, c.y, z_in), MoveRole::Link, feed)?;
                path.feed_to(dvec3(begin.x, begin.y, z_in), MoveRole::Lead, feed)?;
            } else {
                // Along the arc's own tangent where it starts.
                let tangent = DVec2::from_angle(at).perp() * way;
                let before = begin - tangent * straight;
                path.feed_to(dvec3(before.x, before.y, z_in), MoveRole::Link, feed)?;
                if straight > 0.0 {
                    path.feed_to(dvec3(begin.x, begin.y, z_in), MoveRole::Lead, feed)?;
                }
            }
            path.arc_to(
                dvec3(start.x, start.y, from),
                upright(q, way * sweep),
                MoveRole::Lead,
                feed,
            )?;
            z_in
        }
        None => {
            if p.to_centre {
                path.feed_to(dvec3(c.x, c.y, from), MoveRole::Link, feed)?;
                path.feed_to(dvec3(start.x, start.y, from), MoveRole::Lead, feed)?;
            } else {
                path.feed_to(dvec3(start.x, start.y, from), MoveRole::Link, feed)?;
            }
            from
        }
    };

    helix(path, (c, rho), total, (from, to), feed)?;

    // Out: round the arc, along the straight back to the centre, where the
    // linker lifts.
    let z_out = match lead {
        Some((a, sweep, straight)) => {
            let z_out = to + up * rise(a, sweep);
            let q = c + DVec2::from_angle(total) * (rho - a);
            let at = total + way * sweep;
            let off = q + DVec2::from_angle(at) * a;
            path.arc_to(
                dvec3(off.x, off.y, z_out),
                upright(q, way * sweep),
                MoveRole::Lead,
                feed,
            )?;
            if p.to_centre {
                path.feed_to(dvec3(c.x, c.y, z_out), MoveRole::Lead, feed)?;
            } else if straight > 0.0 {
                let tangent = DVec2::from_angle(at).perp() * way;
                let after = off + tangent * straight;
                path.feed_to(dvec3(after.x, after.y, z_out), MoveRole::Lead, feed)?;
            }
            z_out
        }
        None => {
            if p.to_centre {
                path.feed_to(dvec3(c.x, c.y, to), MoveRole::Lead, feed)?;
            }
            to
        }
    };
    Ok(z_in.min(z_out).min(from).min(to))
}

/// Round the helix about `c` of radius `rho`, from angle zero turning
/// `total` radians (anticlockwise where positive), from height `from` to
/// `to` evenly as it turns: as Fusion's programs write it, in half turns from
/// where it starts and what is left, so no move ends where it starts, seen
/// from above.
fn helix(
    path: &mut Toolpath,
    (c, rho): (DVec2, f64),
    total: f64,
    (from, to): (f64, f64),
    feed: f64,
) -> Result<(), ToolpathError> {
    let turned = total.abs();
    let way = total.signum();
    // Whole half turns short of the end, and what is left: the last half
    // turn of a helix of whole ones among it.
    let whole = (turned / PI - 1.0e-9).floor();
    let halves = if whole >= 1.0 {
        camcraft_geom::steps::bounded_count(whole)
    } else {
        0
    };
    let left = turned - f64::from(halves) * PI;
    let mut ends: Vec<f64> = (1..=halves).map(|k| f64::from(k) * PI).collect();
    if left > 1.0e-9 {
        ends.push(turned);
    }
    let mut was = 0.0;
    for (i, &e) in ends.iter().enumerate() {
        // The last exactly where the helix ends.
        let e = if i + 1 == ends.len() { turned } else { e };
        let at = on_circle(c, rho, way * e);
        let z = from + (to - from) * (e / turned);
        path.arc_to(
            dvec3(at.x, at.y, z),
            upright(c, way * (e - was)),
            MoveRole::Cut,
            feed,
        )?;
        was = e;
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::panic
)]
mod tests;
