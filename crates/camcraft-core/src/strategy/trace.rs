//! Picked edges followed: Fusion's Trace.
//!
//! # The curves
//!
//! The picked edges, joined end to end into curves ([`super::curve::join`]), are
//! followed as they lie: each at its own height, a round edge lying level as an
//! arc. A closed curve runs anticlockwise seen from above; an open one from its
//! end lowest in X, then Y, then Z, so the way it runs depends on nothing but the
//! curve. They are followed in the order picked, or, unless that is asked for
//! (Fusion's Preserve Order), from the first picked always to the nearest next.
//!
//! # Where the tool goes
//!
//! On the curve, the tool's tip on it (Fusion's Sideways Compensation, center),
//! or its centre the tool's radius and the radial stock to leave to the left or
//! right of it as it runs ([`super::curve::offset`]). Every pass is moved up by
//! the axial offset and the axial stock to leave. An open curve's passes run on
//! past its ends along it by the pass extension. What of a pass is steeper than
//! the maximum angle is left out, the pass broken there.
//!
//! # Levels
//!
//! With Multiple Depths, each curve is cut that many stepdowns above itself and
//! then on itself: Fusion's Number of Stepdowns, each a stepdown apart, the last
//! on the curve. Level by level across every curve, or each curve all the way
//! down before the next (Fusion's Order by Depth). Both ways, an open curve's
//! levels go back and forth.

use glam::DVec3;

use crate::path::toolpath::{MoveRole, Toolpath, ToolpathError};
use crate::strategy::curve::{Curve, Piece, offset};
use crate::strategy::nesting::take_nearest;
use crate::strategy::{StrategyError, at_least_zero, check};

/// Where the tool goes against a curve: Fusion's Sideways Compensation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Beside {
    /// Its centre on it.
    On,
    /// Its centre to the left of it, as it runs, seen from above.
    Left,
    /// To the right.
    Right,
}

/// How the curves are followed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TraceParams {
    /// Where the tool goes against each curve.
    pub beside: Beside,
    /// How far its centre is from the curve beside it, in millimetres: the
    /// tool's radius and the radial stock to leave.
    pub offset: f64,
    /// How far every pass is moved up from the curve, in millimetres: the
    /// axial offset and the axial stock to leave.
    pub raise: f64,
    /// How far an open curve's passes run on past each end, in millimetres.
    pub extension: f64,
    /// The steepest a pass may go, in radians from level.
    pub max_angle: f64,
    /// Levels above the curve: the stepdown between them and how many there
    /// are above it. On the curve alone when `None`.
    pub levels: Option<(f64, u32)>,
    /// Every curve at a level before the next one down, rather than each all
    /// the way down before the next.
    pub by_depth: bool,
    /// An open curve's levels cut back and forth.
    pub both_ways: bool,
    /// Each curve's last pass cut twice.
    pub repeat: bool,
    /// The curves in the order given, rather than always the nearest next.
    pub preserve_order: bool,
    /// Cutting feed, in mm/min.
    pub feed: f64,
    /// How close two ends must be to be one, in millimetres.
    pub tolerance: f64,
}

/// What a trace did.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TraceReport {
    /// Curves followed.
    pub curves: u64,
    /// Of them, closed.
    pub closed: u64,
    /// Passes, every level's.
    pub passes: u64,
    /// The most levels any curve was cut at.
    pub levels: u64,
    /// Stretches steeper than the maximum angle, left out.
    pub steep: u64,
}

/// Why picked edges cannot be followed.
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum TraceError {
    /// Nothing was picked.
    #[error("pick the edges to follow")]
    NothingPicked,
    /// Faces were picked: a trace follows edges.
    #[error("a trace follows edges: pick the edges to follow, not faces")]
    FacesPicked,
    /// A curve that turns too tightly for the tool to keep beside it.
    #[error(
        "the curve turns too tightly at ({x:.3}, {y:.3}) for the tool to keep its radius \
         beside it: follow it on the curve, or use a smaller tool"
    )]
    TooTight {
        /// Where.
        x: f64,
        /// Where.
        y: f64,
    },
    /// The path could not be made.
    #[error(transparent)]
    Strategy(#[from] StrategyError),
}

impl From<ToolpathError> for TraceError {
    fn from(e: ToolpathError) -> Self {
        TraceError::Strategy(e.into())
    }
}

/// The most levels above a curve.
const MOST_LEVELS: u32 = 1000;

fn check_params(p: &TraceParams) -> Result<(), StrategyError> {
    check("feed", p.feed)?;
    check("tolerance", p.tolerance)?;
    at_least_zero("pass extension", p.extension)?;
    if !p.raise.is_finite() {
        return Err(StrategyError::BadParameter {
            name: "axial offset",
            value: p.raise,
        });
    }
    if p.beside != Beside::On {
        check("tool radius", p.offset)?;
    }
    if !(p.max_angle > 0.0 && p.max_angle <= std::f64::consts::FRAC_PI_2 + 1.0e-12) {
        return Err(StrategyError::BadParameter {
            name: "maximum angle",
            value: p.max_angle,
        });
    }
    if let Some((d, n)) = p.levels {
        check("stepdown", d)?;
        if n > MOST_LEVELS {
            return Err(StrategyError::BadParameter {
                name: "number of stepdowns",
                value: f64::from(n),
            });
        }
    }
    Ok(())
}

/// `curve` run the way it is followed: anticlockwise when closed, from its
/// lowest end when open.
fn oriented(curve: &Curve, tol: f64) -> Curve {
    if curve.is_closed(tol) {
        if curve.signed_area() < 0.0 {
            return curve.reversed();
        }
        return curve.clone();
    }
    let (a, b) = (curve.start, curve.end());
    let key = |p: DVec3| [p.x, p.y, p.z];
    let lower = key(b)
        .iter()
        .zip(key(a))
        .map(|(x, y)| x.total_cmp(&y))
        .find(|o| o.is_ne())
        .is_some_and(std::cmp::Ordering::is_lt);
    if lower {
        curve.reversed()
    } else {
        curve.clone()
    }
}

/// The way `curve` runs where it starts (`at_end` false) or ends, in space: a
/// unit vector, or `None` for a curve that goes nowhere.
fn tangent(curve: &Curve, at_end: bool) -> Option<DVec3> {
    let steps: Vec<(DVec3, Piece)> = curve.steps().map(|(f, p)| (f, *p)).collect();
    let (from, p) = if at_end {
        *steps.last()?
    } else {
        *steps.first()?
    };
    match p {
        Piece::Line { to } => (to - from).try_normalize(),
        Piece::Arc { to, centre, sweep } => {
            let q = if at_end { to } else { from };
            (q.truncate() - centre)
                .perp()
                .try_normalize()
                .map(|t| (t * sweep.signum()).extend(0.0))
        }
    }
}

/// `curve` run on `by` past each end, straight along it.
fn extended(curve: &Curve, by: f64) -> Curve {
    let mut out = curve.clone();
    if by <= 0.0 {
        return out;
    }
    if let Some(t) = tangent(curve, false) {
        let start = curve.start - t * by;
        out.pieces.insert(0, Piece::Line { to: curve.start });
        out.start = start;
    }
    if let Some(t) = tangent(curve, true) {
        out.pieces.push(Piece::Line {
            to: curve.end() + t * by,
        });
    }
    out
}

/// `curve` broken where it is steeper than `most` from level, those stretches
/// left out: the pieces kept, and how many stretches were left out.
fn not_steeper(curve: &Curve, most: f64) -> (Vec<Curve>, u64) {
    if most >= std::f64::consts::FRAC_PI_2 - 1.0e-12 {
        return (vec![curve.clone()], 0);
    }
    let mut out: Vec<Curve> = Vec::new();
    let mut steep = 0;
    let mut current: Option<Curve> = None;
    let mut was_steep = false;
    for (from, p) in curve.steps() {
        let too_steep = match *p {
            Piece::Line { to } => {
                let rise = (to.z - from.z).abs();
                let run = from.truncate().distance(to.truncate());
                rise.atan2(run) > most + 1.0e-9
            }
            Piece::Arc { .. } => false,
        };
        if too_steep {
            if !was_steep {
                steep += 1;
            }
            was_steep = true;
            out.extend(current.take());
            continue;
        }
        was_steep = false;
        current
            .get_or_insert_with(|| Curve {
                start: from,
                pieces: Vec::new(),
            })
            .pieces
            .push(*p);
    }
    out.extend(current);
    (out, steep)
}

/// `curves` followed: Fusion's Trace. Each pass a link to where it starts and
/// the curve, as it is moved for the tool and the level.
pub fn trace(curves: &[Curve], p: &TraceParams) -> Result<(Toolpath, TraceReport), TraceError> {
    check_params(p)?;
    if curves.is_empty() {
        return Err(TraceError::NothingPicked);
    }
    let tol = p.tolerance;
    let mut report = TraceReport::default();
    let oriented: Vec<Curve> = curves.iter().map(|c| oriented(c, tol)).collect();
    // In order: as given, or the first, then always the nearest next.
    let ordered = if p.preserve_order {
        oriented
    } else {
        let mut left = oriented;
        let mut out = Vec::with_capacity(left.len());
        let mut at = left.first().map(|c| c.start);
        while let Some(c) = take_nearest(&mut left, |c| {
            at.map_or(0.0, |a| a.truncate().distance(c.start.truncate()))
        }) {
            at = Some(c.end());
            out.push(c);
        }
        out
    };
    // Each curve as the tool's tip follows it: beside it, run on past its
    // ends, and broken where it is too steep.
    let mut followed: Vec<(bool, Vec<Curve>)> = Vec::with_capacity(ordered.len());
    for c in &ordered {
        let closed = c.is_closed(tol);
        report.curves += 1;
        report.closed += u64::from(closed);
        let d = match p.beside {
            Beside::On => 0.0,
            Beside::Left => p.offset,
            Beside::Right => -p.offset,
        };
        let beside = offset(c, d, tol).map_err(|t| TraceError::TooTight {
            x: t.at.x,
            y: t.at.y,
        })?;
        let run_on = if closed {
            beside
        } else {
            extended(&beside, p.extension)
        };
        let (pieces, steep) = not_steeper(&run_on, p.max_angle);
        report.steep += steep;
        followed.push((closed, pieces));
    }
    // How far above the curve each level is, the highest first.
    let lifts: Vec<f64> = match p.levels {
        Some((d, n)) => (0..=n.min(MOST_LEVELS))
            .rev()
            .map(|k| d * f64::from(k))
            .collect(),
        None => vec![0.0],
    };
    report.levels = lifts.len() as u64;
    let last = lifts.len().saturating_sub(1);
    let mut path = Toolpath::new();
    let mut cut = |path: &mut Toolpath, (closed, pieces): &(bool, Vec<Curve>), level: usize| {
        let lift = lifts.get(level).copied().unwrap_or(0.0);
        let times = if p.repeat && level == last { 2 } else { 1 };
        for c in pieces {
            let mut c = c.raised(p.raise + lift);
            if p.both_ways && !*closed && level % 2 == 1 {
                c = c.reversed();
            }
            for time in 0..times {
                // A closed curve cut again goes round again from where it
                // ended; anything else is linked back to its start.
                let there = path.end().is_some_and(|e| e.distance(c.start) <= 1.0e-9);
                if time == 0 || !there {
                    path.feed_to(c.start, MoveRole::Link, p.feed)?;
                }
                c.cut_into(path, MoveRole::Cut, p.feed)?;
                report.passes += 1;
            }
        }
        Ok::<(), ToolpathError>(())
    };
    if p.by_depth {
        for level in 0..lifts.len() {
            for f in &followed {
                cut(&mut path, f, level)?;
            }
        }
    } else {
        for f in &followed {
            for level in 0..lifts.len() {
                cut(&mut path, f, level)?;
            }
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
