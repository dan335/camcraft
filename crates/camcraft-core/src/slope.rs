//! Slopes: where a pass is kept to the range of steepness asked for, as Fusion's
//! contact angle limits. What scallop and the other 3D finishing strategies share.
//!
//! A slope is the cutter location's: how steep the surface the tool's tip rides
//! is, in radians from level, as scallop's grid measures it (decision 0009).
//! Under a ball that is the part's own slope where the tool touches it.

use camcraft_geom::steps::bounded_count;
use glam::DVec2;
use rayon::prelude::*;

use crate::dropcutter::{DropCutter, DropTest};
use crate::strategy::StrategyError;
use crate::toolpath::PLUNGE_RATIO;

/// Stretches left out, and why.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SlopeCounts {
    /// Steeper than a plunge, where a waterline pass is to finish.
    pub(crate) too_steep: u64,
    /// Outside the range asked for.
    pub(crate) outside_slope: u64,
}

/// A slope range in radians, `[from, to]`, checked: `0 <= from < to <= 90°`.
pub(crate) fn check_range(from: f64, to: f64) -> Result<(), StrategyError> {
    let right = std::f64::consts::FRAC_PI_2 + 1e-9;
    if from >= 0.0 && from < to && to <= right {
        Ok(())
    } else {
        Err(StrategyError::BadParameter {
            name: "slope range",
            value: to,
        })
    }
}

/// Whether `[from, to]` is every slope there is, so nothing need be measured.
pub(crate) fn is_everything(from: f64, to: f64) -> bool {
    from <= 0.0 && to >= std::f64::consts::FRAC_PI_2 - 1e-9
}

/// The steepest a stretch may be and still be cut along, up to `to`: a plunge's.
pub(crate) fn ceiling(to: f64) -> f64 {
    PLUNGE_RATIO.atan().min(to)
}

/// How steep the surface the tool's tip rides is at `p`, in radians from level:
/// from the drop-cutter's heights `h` either side, across and along. `None` where
/// the tool is off the part.
pub(crate) fn cl_slope<T: DropTest>(
    cutter: &DropCutter<'_>,
    tool: &T,
    p: DVec2,
    h: f64,
    scratch: &mut Vec<u32>,
) -> Option<f64> {
    let mut z = |q: DVec2| cutter.drop_at_in(tool, q.x, q.y, scratch);
    let dx = z(p + DVec2::X * h)? - z(p - DVec2::X * h)?;
    let dy = z(p + DVec2::Y * h)? - z(p - DVec2::Y * h)?;
    Some((dx.hypot(dy) / (2.0 * h)).atan())
}

/// A line to cut, with the slope under each of its segments.
pub(crate) struct Sloped {
    pub(crate) points: Vec<DVec2>,
    pub(crate) closed: bool,
    /// One a segment: `points[i]` to the next. `None` where nothing is known.
    pub(crate) slopes: Vec<Option<f64>>,
}

impl Sloped {
    /// `line`, closed back to its first point if `closed`, split so no segment is
    /// longer than `pitch`, with `slope_at` the middle of each: asked in
    /// parallel, each thread with scratch space of its own.
    pub(crate) fn split(
        line: &[DVec2],
        closed: bool,
        pitch: f64,
        slope_at: impl Fn(DVec2, &mut Vec<u32>) -> Option<f64> + Sync,
    ) -> Self {
        let mut points = Vec::new();
        let segments = if closed {
            line.len()
        } else {
            line.len().saturating_sub(1)
        };
        for (&a, &b) in line.iter().zip(line.iter().cycle().skip(1)).take(segments) {
            let pieces = bounded_count((a.distance(b) / pitch).ceil()).max(1);
            for k in 0..pieces {
                points.push(a.lerp(b, f64::from(k) / f64::from(pieces)));
            }
        }
        if !closed && let Some(&last) = line.last() {
            points.push(last);
        }
        let segments = if closed {
            points.len()
        } else {
            points.len().saturating_sub(1)
        };
        let slopes = (0..segments)
            .into_par_iter()
            .map_init(Vec::new, |scratch, i| {
                let a = *points.get(i)?;
                let b = *points.get((i + 1) % points.len())?;
                slope_at(a.midpoint(b), scratch)
            })
            .collect();
        Self {
            points,
            closed,
            slopes,
        }
    }

    /// The stretches of it whose slope is in `[from, to]` and not past `ceiling`,
    /// counting each stretch left out, as too steep or out of the range asked for.
    pub(crate) fn within(
        &self,
        from: f64,
        ceiling: f64,
        to: f64,
        w: &mut SlopeCounts,
    ) -> Vec<(Vec<DVec2>, bool)> {
        let keep = |s: &Option<f64>| s.is_none_or(|s| s >= from - 1e-12 && s <= ceiling + 1e-12);
        let segments = self.slopes.len();
        let kept = |i: usize| self.slopes.get(i).is_none_or(keep);
        if (0..segments).all(kept) {
            return vec![(self.points.clone(), self.closed)];
        }
        // A closed line is started where a stretch left out ends, so no stretch
        // runs round its join.
        let start = if self.closed {
            (0..segments)
                .find(|&i| !kept(i) && kept((i + 1) % segments))
                .map_or(0, |i| (i + 1) % segments)
        } else {
            0
        };
        let mut out = Vec::new();
        let mut run: Vec<DVec2> = Vec::new();
        let mut leaving = false;
        for step in 0..segments {
            let i = (start + step) % segments;
            let (Some(&a), Some(&b), Some(slope)) = (
                self.points.get(i),
                self.points.get((i + 1) % self.points.len()),
                self.slopes.get(i),
            ) else {
                continue;
            };
            if keep(slope) {
                if run.is_empty() {
                    run.push(a);
                }
                run.push(b);
                leaving = false;
                continue;
            }
            if !leaving {
                match slope {
                    Some(s) if *s > ceiling && *s <= to + 1e-12 => w.too_steep += 1,
                    _ => w.outside_slope += 1,
                }
            }
            leaving = true;
            if run.len() >= 2 {
                out.push((std::mem::take(&mut run), false));
            }
            run.clear();
        }
        if run.len() >= 2 {
            out.push((run, false));
        }
        out
    }
}
