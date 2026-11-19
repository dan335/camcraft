//! Morph finishing: passes laid between two picked curves, each a step of the
//! way from the one to the other, dropped onto the part. Fusion's Morph.
//!
//! # Between the two
//!
//! Each curve is seen from above, and measured along from its start. A point
//! a fraction of the way along the one is matched with the point as far along
//! the other, and each pass is the same step of the way from every point of
//! the first curve to its match: the first pass is the first curve, the last
//! the second, and those between change shape from the one into the other.
//! So Fusion's programs for `ops_3d` lay them (decision 0030): a corner of
//! one curve is a corner of every pass, moved a step at a time towards its
//! match on the other, and lines across the floor between two of its edges of
//! different lengths end a step of the way between their ends.
//!
//! There are as many steps as keep every point of a pass within a stepover of
//! its match on the next: the furthest apart two matched points are, over the
//! stepover. Fusion's programs for two open curves have that many.
//!
//! Two open curves are matched end to end, the second turned round where it
//! runs the other way: Fusion's Morph gives no passes for two that run
//! opposite ways, and a curve picked in camcraft runs the way its edges do.
//! Two closed ones both run clockwise seen from above, as Fusion's new chains
//! do, and the second is started where it makes the matched points closest
//! on the whole. One open and one closed are refused.
//!
//! # Order and direction
//!
//! The passes are cut from one curve to the other, starting at the curve that
//! comes nearer the work zero, as Fusion's programs do whichever was picked
//! first. *One Way* runs each pass as the first curve runs, *Other Way* the
//! reverse, and *Both Ways* each pass the other way from the one before: the
//! first, open, from its end nearer the work zero, and closed, anticlockwise,
//! as Fusion's programs start. They are then dropped onto the part as a
//! pattern's lines are ([`crate::strategy::projected::project`]), in that
//! order, each closed pass after the first started where the last left off.

use camcraft_geom::contour::Region;
use glam::{DVec2, DVec3};

use crate::cutter::surfaces::Seat;
use crate::path::toolpath::Toolpath;
use crate::strategy::curve::Curve as Picked;
use crate::strategy::morph::Lap;
use crate::strategy::project::Way;
use crate::strategy::projected::{Curve, ProjectParams, ProjectReport, project};
use crate::strategy::{StrategyError, check};

/// The most passes a morph lays: far past any real part.
pub const MAX_PASSES: u32 = 100_000;

/// Fewest and most places along two closed curves their matching is judged
/// at: enough to place the second's start to a fraction of the stepover.
const MATCHED_AT: (u32, u32) = (64, 2048);

/// How the passes are laid and cut.
#[derive(Debug, Clone, PartialEq)]
pub struct MorphCurvesParams {
    /// How each is dropped onto the part; its stepover the most any point of
    /// a pass is from its match on the next.
    pub drop: ProjectParams,
    /// Which way each runs.
    pub way: Way,
}

/// What happened while morphing between the curves.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MorphCurvesReport {
    /// Passes laid between the curves, both included.
    pub laid: u64,
    /// The furthest any point of a pass is from its match on the next, in
    /// millimetres: no more than the stepover, but for curves so far apart
    /// they need more passes than [`MAX_PASSES`].
    pub widest: f64,
    /// Cutting them on the part.
    pub cut: ProjectReport,
}

/// Why a morph's picks cannot be morphed between.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum MorphError {
    /// The picked edges, joined end to end, are not two curves.
    #[error("pick two curves to morph between: the edges picked make {0}")]
    NotTwo(usize),
    /// The curves are picked on a model that stands more than once: a pick is
    /// on every copy, so they are two on each, and a morph is between two.
    #[error(
        "Morph cannot yet morph across copies, and its curves are picked on '{model}', which \
         stands {copies} times: give the part one copy, or make each copy a model of its own \
         with a Morph of its own"
    )]
    Copies {
        /// The model.
        model: String,
        /// How many copies of it stand.
        copies: usize,
    },
    /// One curve is open and the other closed.
    #[error("pick two open curves or two closed ones: one picked is open and the other closed")]
    OpenAndClosed,
    /// Laying or cutting the passes failed, as any strategy can.
    #[error(transparent)]
    Strategy(#[from] StrategyError),
}

/// The passes between `curves`, the two picked, each the way asked, dropped
/// onto the part within `area` and cut along it.
///
/// The path holds [`MoveRole::Cut`](crate::path::toolpath::MoveRole::Cut)
/// passes separated by links, for the linker.
pub fn morph_curves(
    seat: &Seat<'_>,
    area: &Region,
    curves: &[Picked],
    params: &MorphCurvesParams,
) -> Result<(Toolpath, MorphCurvesReport), MorphError> {
    let [first, second] = curves else {
        return Err(MorphError::NotTwo(curves.len()));
    };
    let stepover = params.drop.stepover;
    check("stepover", stepover)?;
    check("tolerance", params.drop.tolerance)?;
    let (laid, widest) = between((first, second), params.drop.tolerance, stepover, params.way)?;
    let (path, cut) = project(seat, area, &laid, &params.drop)?;
    let report = MorphCurvesReport {
        laid: laid.len() as u64,
        widest,
        cut,
    };
    Ok((path, report))
}

/// The passes from `a` to `b`, seen from above, in the order they are cut and
/// each the way it runs, and the furthest any point of one is from its match
/// on the next.
fn between(
    (a, b): (&Picked, &Picked),
    tol: f64,
    stepover: f64,
    way: Way,
) -> Result<(Vec<Curve>, f64), MorphError> {
    let closed = a.is_closed(tol);
    if closed != b.is_closed(tol) {
        return Err(MorphError::OpenAndClosed);
    }
    let seen = |c: &Picked| Lap::through(c.sample(tol).iter().map(|p| p.truncate()).collect());
    let (from, to) = if closed {
        let (from, to) = (seen(&a.wound(false)), seen(&b.wound(false)));
        let start = best_start(&from, &to, stepover);
        (from, started_at(&to, start))
    } else {
        // The second run the way the first does: the pairing of their ends,
        // seen from above, that does not cross.
        let gap = |p: DVec3, q: DVec3| p.truncate().distance(q.truncate());
        let crossed = gap(a.start, b.end()) + gap(a.end(), b.start)
            < gap(a.start, b.start) + gap(a.end(), b.end());
        (
            seen(a),
            seen(&if crossed { b.reversed() } else { b.clone() }),
        )
    };

    // Every corner of either is a corner of every pass: each pass is the
    // same step from one to the other at every point of both.
    let fraction = |lap: &Lap| -> Vec<f64> {
        lap.along
            .iter()
            .map(|d| {
                if lap.length > 0.0 {
                    d / lap.length
                } else {
                    0.0
                }
            })
            .collect()
    };
    let mut places = fraction(&from);
    places.extend(fraction(&to));
    places.sort_by(f64::total_cmp);
    places.dedup_by(|x, y| (*x - *y).abs() < 1e-12);
    let matched: Vec<(DVec2, DVec2)> = places.iter().map(|&t| (from.at(t), to.at(t))).collect();
    // Matched points move straight towards each other, so they are furthest
    // apart at a corner of one or the other.
    let furthest = matched
        .iter()
        .map(|(p, q)| p.distance(*q))
        .fold(0.0, f64::max);
    let steps = camcraft_geom::steps::bounded_count((furthest / stepover).ceil()).min(MAX_PASSES);

    let mut passes: Vec<Vec<DVec2>> = (0..=steps)
        .map(|k| {
            let t = f64::from(k) / f64::from(steps);
            let mut points: Vec<DVec2> = matched.iter().map(|(p, q)| p.lerp(*q, t)).collect();
            if closed {
                // A closed pass does not repeat its first point.
                points.pop();
            }
            points
        })
        .collect();

    // From the curve nearer the work zero.
    let from_zero = |p: Option<&DVec2>| p.map_or(f64::MAX, |p| p.length());
    let near = |points: &Vec<DVec2>| -> f64 {
        if closed {
            points.iter().map(|p| p.length()).fold(f64::MAX, f64::min)
        } else {
            from_zero(points.first()).min(from_zero(points.last()))
        }
    };
    if let (Some(first), Some(last)) = (passes.first(), passes.last())
        && near(last) < near(first)
    {
        passes.reverse();
    }
    // Each laid the way the first curve runs. Both ways, the first turned
    // round where it is closed, or open and nearer the work zero at its end.
    let first_turned = closed
        || passes
            .first()
            .is_some_and(|p| from_zero(p.last()) < from_zero(p.first()));
    for (i, pass) in passes.iter_mut().enumerate() {
        let turned = match way {
            Way::OneWay => false,
            Way::OtherWay => true,
            Way::BothWays => first_turned != (i % 2 == 1),
        };
        if turned {
            pass.reverse();
        }
    }
    let laid = passes
        .into_iter()
        .map(|points| {
            if closed {
                Curve::closed(points)
            } else {
                Curve::open(points)
            }
        })
        .collect();
    Ok((laid, furthest / f64::from(steps)))
}

/// How far round `to`, as a fraction of it, it is best started for its points
/// to be matched with `from`'s: where the matched points are closest on the
/// whole, their distances squared, judged at evenly spaced places along both.
fn best_start(from: &Lap, to: &Lap, stepover: f64) -> f64 {
    let longer = from.length.max(to.length);
    let n = camcraft_geom::steps::bounded_count((longer / stepover).ceil())
        .clamp(MATCHED_AT.0, MATCHED_AT.1);
    let places = |lap: &Lap| -> Vec<DVec2> {
        (0..n)
            .map(|i| lap.at(f64::from(i) / f64::from(n)))
            .collect()
    };
    let (a, b) = (places(from), places(to));
    let cost = |shift: usize| -> f64 {
        a.iter()
            .zip(b.iter().cycle().skip(shift))
            .map(|(p, q)| p.distance_squared(*q))
            .sum()
    };
    // The least, the first of any as low.
    let best = (0..b.len())
        .map(|shift| (cost(shift), shift))
        .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)))
        .map_or(0, |(_, shift)| shift);
    // A count of places, so exact as a float.
    #[allow(clippy::cast_precision_loss)]
    let fraction = best as f64 / f64::from(n);
    fraction
}

/// `lap`, a closed line, started `t` of the way round it instead.
fn started_at(lap: &Lap, t: f64) -> Lap {
    let start = lap.at(t);
    let cut = t * lap.length;
    // Its corners, the last of which is its first again.
    let corners = lap.points.len().saturating_sub(1);
    let at = || lap.points.iter().zip(&lap.along).take(corners);
    let mut points = vec![start];
    points.extend(at().filter(|(_, d)| **d > cut).map(|(p, _)| *p));
    points.extend(at().filter(|(_, d)| **d < cut).map(|(p, _)| *p));
    points.push(start);
    Lap::through(points)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::cast_precision_loss
)]
mod tests;
