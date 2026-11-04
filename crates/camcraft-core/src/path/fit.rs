//! Smoothing: a path's runs of short straight moves fitted with fewer, longer
//! ones, arcs where they lie on a circle and straight lines where they lie on a
//! line, none further than a tolerance from the moves it stands for. Fusion's
//! Smoothing.
//!
//! A strategy's passes are its steps: adaptive's a fifth of the tool's radius
//! at most, a finish's as close as the part needs. Each is a line of the
//! program, and a controller reads only so many a second; at a high feed, short
//! enough moves come faster than it reads them, and it waits between them. The
//! same path in fewer moves is the same cut, in a smaller program, run smoothly.
//!
//! # What is fitted
//!
//! Straight feeding moves one after another with the same role and feed, that
//! may be arcs: cuts, transitions, leads and ramps. A rapid, plunge, retract or
//! link, an arc, or a change of role or feed, ends a run, and is kept as it is,
//! so every pass starts and ends where it did, and every point where a move
//! changes what it is for is still a point of the path.
//!
//! An arc is only fitted where a three-axis machine cuts one: the points level,
//! or square to X or Y ([`principal`](super::toolpath::principal)), or round a
//! circle seen from above while climbing or falling steadily as they turn, a
//! helix about Z, as a ramp round a ring is; never turning more than a half
//! turn, so an arc written by its radius is not ambiguous. A line or arc stands for the moves it replaces only if every one
//! of their points is within the tolerance of it, and every one of its points
//! within the tolerance of them: going the same way along it, never back.
//!
//! # How far it strays
//!
//! It strays up to the tolerance from the path the strategy made, either way,
//! so the tolerance the result is verified to is the job's and this together,
//! as Fusion's total tolerance is.

use std::f64::consts::{FRAC_PI_2, PI};

use camcraft_geom::arc::turn;
use glam::{DVec2, DVec3};

use super::toolpath::{Arc, Move, MoveKind, MoveRole, Toolpath, ToolpathError};

/// Points closer than this to one plane are in it, in millimetres.
const IN_PLANE: f64 = 1.0e-6;

/// The largest radius an arc is fitted with, in millimetres: flatter than this
/// is a line's to stand for.
const LARGEST_RADIUS: f64 = 2000.0;

/// What smoothing did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FitReport {
    /// Moves before.
    pub before: u64,
    /// Moves after.
    pub after: u64,
    /// Of them, arcs it fitted.
    pub arcs: u64,
}

/// `path` with its runs of short straight moves fitted with arcs and lines,
/// each within `tolerance` of the moves it stands for.
pub fn fit_arcs(path: &Toolpath, tolerance: f64) -> Result<(Toolpath, FitReport), ToolpathError> {
    let mut out = Toolpath::new();
    let mut report = FitReport {
        before: path.len() as u64,
        ..FitReport::default()
    };
    // The run being gathered: its role and feed, and its points, from where it
    // starts.
    let mut run: Option<(MoveRole, f64, Vec<DVec3>)> = None;
    for (at, m) in path.steps() {
        let fits = fittable(m) && at.is_some();
        let joins = match (&run, m.feed) {
            (Some((role, feed, _)), Some(f)) => {
                fits && *role == m.role && feed.to_bits() == f.to_bits()
            }
            _ => false,
        };
        if joins {
            if let Some((_, _, points)) = run.as_mut() {
                points.push(m.to);
            }
        } else {
            if let Some((role, feed, points)) = run.take() {
                report.arcs += emit(&mut out, role, feed, &points, tolerance)?;
            }
            match (fits, at, m.feed) {
                (true, Some(from), Some(feed)) => run = Some((m.role, feed, vec![from, m.to])),
                _ => out.push(*m)?,
            }
        }
    }
    if let Some((role, feed, points)) = run.take() {
        report.arcs += emit(&mut out, role, feed, &points, tolerance)?;
    }
    report.after = out.len() as u64;
    Ok((out, report))
}

/// Whether `m` may be fitted: straight, feeding, and of a role that may be an
/// arc.
fn fittable(m: &Move) -> bool {
    m.kind == MoveKind::Linear && m.feed.is_some() && m.role.along_the_work()
}

/// Append the run through `points`, from its first, to `out` as arcs and lines
/// within `tolerance` of it: from each point, the arc or line that reaches
/// furthest along. How many arcs.
fn emit(
    out: &mut Toolpath,
    role: MoveRole,
    feed: f64,
    points: &[DVec3],
    tolerance: f64,
) -> Result<u64, ToolpathError> {
    let last = points.len().saturating_sub(1);
    let mut arcs = 0;
    let mut i = 0;
    while i < last {
        let line = furthest(i, last, |j| {
            fits_line(points.get(i..=j)?, tolerance).then_some(())
        });
        let arc = furthest(i + 1, last, |j| fit_arc(points.get(i..=j)?, tolerance));
        match arc {
            Some((j, a)) if j > line.map_or(i + 1, |(l, ())| l) => {
                let Some(&to) = points.get(j) else { break };
                out.arc_to(to, a, role, feed)?;
                arcs += 1;
                i = j;
            }
            _ => {
                let j = line.map_or(i + 1, |(l, ())| l);
                let Some(&to) = points.get(j) else { break };
                out.feed_to(to, role, feed)?;
                i = j;
            }
        }
    }
    Ok(arcs)
}

/// The furthest `j` up to `last`, from `from` on, that `fits` says fits, and
/// what it said: doubling the reach while it fits, then halving back to where
/// it stops. A run that fits up to some point fits nearly always up to every
/// point before it, so this finds where it stops in a few tries, not one per
/// point. `None` where none past `from` fits.
fn furthest<T>(from: usize, last: usize, fits: impl Fn(usize) -> Option<T>) -> Option<(usize, T)> {
    let mut good: Option<(usize, T)> = None;
    let mut step = 1;
    let mut lo = from;
    let mut bad = None;
    loop {
        let j = (lo + step).min(last);
        if j <= lo {
            break;
        }
        match fits(j) {
            Some(t) => {
                good = Some((j, t));
                lo = j;
                step *= 2;
            }
            None => {
                bad = Some(j);
                break;
            }
        }
    }
    // Between the last that fits and the first that does not.
    if let Some(mut hi) = bad {
        let mut lo = good.as_ref().map_or(from, |(j, _)| *j);
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            match fits(mid) {
                Some(t) => {
                    good = Some((mid, t));
                    lo = mid;
                }
                None => hi = mid,
            }
        }
    }
    good
}

/// Whether the line from the first of `points` to the last stands for the
/// moves through them: every point within `tolerance` of it, each further
/// along it than the one before.
fn fits_line(points: &[DVec3], tolerance: f64) -> bool {
    let (Some(&a), Some(&b)) = (points.first(), points.last()) else {
        return false;
    };
    if points.len() <= 2 {
        return true;
    }
    let d = b - a;
    let dd = d.length_squared();
    if dd <= f64::EPSILON {
        return false;
    }
    let mut was = 0.0;
    for &p in points.iter().skip(1) {
        let t = (p - a).dot(d) / dd;
        if t < was || t > 1.0 {
            return false;
        }
        if (a + d * t).distance(p) > tolerance {
            return false;
        }
        was = t;
    }
    true
}

/// The plane `points` all lie in that a three-axis machine cuts arcs in, as
/// its axis and the two directions across it, the second a quarter turn
/// counter-clockwise from the first about the axis; and how far along the axis
/// it is.
fn plane_of(points: &[DVec3]) -> Option<(DVec3, DVec3, DVec3, f64)> {
    let first = *points.first()?;
    let flat = |along: fn(DVec3) -> f64| {
        points
            .iter()
            .all(|p| (along(*p) - along(first)).abs() <= IN_PLANE)
    };
    if flat(|p| p.z) {
        Some((DVec3::Z, DVec3::X, DVec3::Y, first.z))
    } else if flat(|p| p.y) {
        Some((DVec3::Y, DVec3::Z, DVec3::X, first.y))
    } else if flat(|p| p.x) {
        Some((DVec3::X, DVec3::Y, DVec3::Z, first.x))
    } else {
        None
    }
}

/// The arc from the first of `points` to the last, through the middle one,
/// that stands for the moves through them: in a plane a machine cuts arcs in,
/// or else a helix about Z, round a circle seen from above and climbing
/// steadily as it turns; going round one way, by less than a quarter turn a
/// move and at most a half turn in all, every move within `tolerance` of it.
/// `None` where there is none, as where they are in a line.
fn fit_arc(points: &[DVec3], tolerance: f64) -> Option<Arc> {
    if points.len() < 3 {
        return None;
    }
    let first = *points.first()?;
    let (axis, u, v, along, helix) = match plane_of(points) {
        Some((axis, u, v, along)) => (axis, u, v, along, false),
        None => (DVec3::Z, DVec3::X, DVec3::Y, first.z, true),
    };
    let flat = |p: DVec3| DVec2::new(p.dot(u), p.dot(v));
    let a = flat(first);
    let m = flat(*points.get(points.len() / 2)?);
    let b = flat(*points.last()?);
    let c = circumcentre(a, m, b)?;
    let r = a.distance(c);
    if !(r > 4.0 * tolerance && r <= LARGEST_RADIUS) {
        return None;
    }
    // Each move's turn about the centre, and how far its chord strays from
    // the circle across the axis: by its ends' distances from it and by how
    // far the circle bows out past it.
    let mut sweep = 0.0;
    let mut ccw: Option<bool> = None;
    let mut prev = a;
    let mut strays = Vec::with_capacity(points.len());
    for &p in points.iter().skip(1) {
        let q = flat(p);
        let (from, to) = (prev - c, q - c);
        let turn = turn(from, to);
        if turn.abs() <= f64::EPSILON
            || turn.abs() >= FRAC_PI_2
            || ccw.is_some_and(|c| c != (turn > 0.0))
        {
            return None;
        }
        ccw = Some(turn > 0.0);
        sweep += turn;
        let half = prev.distance(q) * 0.5;
        let bow = r - (r * r - half * half).max(0.0).sqrt();
        let off = (prev.distance(c) - r).abs().max((q.distance(c) - r).abs());
        if off + bow > tolerance {
            return None;
        }
        strays.push((off + bow, sweep));
        prev = q;
    }
    if sweep.abs() > PI + 1e-9 {
        return None;
    }
    if helix {
        // A helix climbs as it turns, so each point is where it is along Z
        // by how far round it has turned; a chord between two points climbs
        // as steadily, so strays along Z no further than its ends.
        let rise = points.last()?.z - first.z;
        let mut was = 0.0;
        for (&(across, turned), p) in strays.iter().zip(points.iter().skip(1)) {
            let off = (p.z - (first.z + rise * turned / sweep)).abs();
            if across + off.max(was) > tolerance {
                return None;
            }
            was = off;
        }
    }
    let centre = u * c.x + v * c.y + axis * along;
    Some(Arc {
        centre,
        axis,
        sweep,
    })
}

/// The centre of the circle through `a`, `b` and `c`, or `None` where they are
/// in a line.
fn circumcentre(a: DVec2, b: DVec2, c: DVec2) -> Option<DVec2> {
    let (ab, ac) = (b - a, c - a);
    let d = 2.0 * ab.perp_dot(ac);
    if d.abs() <= 1e-12 * ab.length_squared().max(ac.length_squared()).max(1e-12) {
        return None;
    }
    let (b2, c2) = (ab.length_squared(), ac.length_squared());
    let x = (ac.y * b2 - ab.y * c2) / d;
    let y = (ab.x * c2 - ac.x * b2) / d;
    Some(a + DVec2::new(x, y))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
mod tests;
