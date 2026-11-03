//! Slopes: where a pass is kept to the range of steepness asked for, as Fusion's
//! contact angle limits. What scallop and the other 3D finishing strategies share.
//!
//! A slope is the cutter location's: how steep the surface the tool's tip rides
//! is, in radians from level, as scallop's grid measures it (decision 0009).
//! Under a ball that is the part's own slope where the tool touches it.

use camcraft_geom::contour::{Contour, Indexed, Region};
use camcraft_geom::steps::bounded_count;
use camcraft_geom::trigrid::TriGrid;
use glam::DVec2;
use rayon::prelude::*;

use crate::cutter::dropcutter::{DropCutter, DropTest};
use crate::path::toolpath::PLUNGE_RATIO;
use crate::strategy::StrategyError;

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
fn is_everything(from: f64, to: f64) -> bool {
    from <= 0.0 && to >= std::f64::consts::FRAC_PI_2 - 1e-9
}

/// Where, as the tool's centre goes, the surface it rides on `grid` is steeper
/// than a plunge: the band a pass laid on that surface leaves out, for rings
/// round the walls there to finish.
///
/// One region, decided once, so every pass is judged by the same thing and
/// none flickers in and out of it where the surface stands near the plunge's
/// slope, as judging each segment alone does.
///
/// A face that stands near the plunge's slope all along, as the bottom of a
/// roundover turning in under the part does, has its slope measured on the
/// grid a little either side of it: a row of specks above it, with gaps
/// between. So the band is found as edges are in a picture, by two
/// thresholds: it starts on ground steeper than a plunge, and spreads from
/// there across the triangles next to it that are at least half that steep
/// ([`SPREAD_RATIO`]), so a face that steep all along is one wall. Ground that
/// is nowhere as steep as a plunge is never a wall, however steep it is
/// measured nearby. Then its gaps narrower than the tool, `radius` across each
/// way, are closed: a strip that narrow between steep ground is no pass's to
/// cut, but the rings'.
///
/// Where the tool leaves the part, at `leaving`'s cells
/// ([`AreaGrid::leaving_part`]), it would fall: ground as steep as any. So a
/// wall is found from there too, and closed out to it: a strip narrower than
/// the tool between a wall and where the tool leaves the part, as round the
/// foot of a part's outside, is no pass's either. Past there the band goes
/// on, the tool's width beyond `area`: nothing is cut out there, but a pass
/// along the area's edge, as its first is, is then well inside the band, not
/// on the edge of it.
///
/// Leaving ground out of it never lets a pass ride down a wall: every pass is
/// broken where it falls steeper than a plunge further than the operation
/// allows, wherever it runs ([`crate::strategy::parallel`]).
///
/// [`AreaGrid::leaving_part`]: crate::strategy::field::AreaGrid::leaving_part
pub(crate) fn steep_band(
    grid: &TriGrid,
    radius: f64,
    area: &Region,
    leaving: &[[u32; 4]],
) -> Result<Region, StrategyError> {
    let steep = PLUNGE_RATIO.atan();
    let spread = SPREAD_RATIO.atan();
    let slope = |t: u32| grid.slope(t).unwrap_or(0.0);
    // Started on ground steeper than a plunge, and beside where the tool leaves
    // the part, on ground steep enough to spread across.
    let mut queue: Vec<(u32, [u32; 3])> = grid
        .triangles()
        .filter(|(t, _)| slope(*t) > steep)
        .collect();
    queue.extend(
        leaving
            .iter()
            .flatten()
            .flat_map(|&n| grid.triangles_at(n))
            .filter(|(t, _)| slope(*t) > spread),
    );
    let mut wall: std::collections::HashSet<u32> = queue.iter().map(|(t, _)| *t).collect();
    while let Some((_, nodes)) = queue.pop() {
        for n in nodes {
            for (t, next) in grid.triangles_at(n) {
                if slope(t) > spread && wall.insert(t) {
                    queue.push((t, next));
                }
            }
        }
    }
    let outline = |nodes: &[u32]| {
        let corners: Option<Vec<DVec2>> = nodes.iter().map(|&n| grid.xy(n)).collect();
        Contour::new(corners?)
    };
    let falls: Vec<Contour> = leaving.iter().filter_map(|cell| outline(cell)).collect();
    let mut pieces: Vec<Contour> = grid
        .triangles()
        .filter(|(t, _)| wall.contains(t))
        .filter_map(|(_, nodes)| outline(&nodes))
        .collect();
    pieces.extend(falls.iter().cloned());
    if pieces.is_empty() {
        return Ok(Region::default());
    }
    let band = Region::union_of(&pieces)?;
    if !(radius.is_finite() && radius > 0.0) {
        return Ok(band);
    }
    // Grown and shrunk back: what the closing adds is gaps under the tool's
    // width, and corners within it; the steep ground is all still in it.
    let arc = (0.05 * grid.pitch()).max(1e-4);
    let closed = band.offset(radius, arc).offset(-radius, arc).union(&band)?;
    if falls.is_empty() {
        return Ok(closed);
    }
    let beyond = Region::union_of(&falls)?
        .offset(radius, arc)
        .difference(area)?;
    Ok(closed.union(&beyond)?)
}

/// How steep, as a rise over a run, ground next to a wall may be and still be
/// part of it: half a plunge's, about 79°.
const SPREAD_RATIO: f64 = PLUNGE_RATIO / 2.0;

/// `line` with each segment in `band` judged steeper than a plunge, and every
/// other judged by its own slope as before: so it leaves the band out in one
/// stretch where it crosses it.
///
/// The band's edge is only known to `pitch`, the grid it was found on, so a
/// line running along it, as the first lines in from a boundary do, is not
/// taken to cross it each time its segments fall either side: it is in the
/// band once more than `pitch` inside it, out once more than `pitch` outside,
/// and between, as it was. Where it starts between, it is as the first
/// segment that is clearly one or the other has it.
pub(crate) fn banded(mut line: Sloped, band: &Indexed, pitch: f64) -> Sloped {
    let n = line.points.len();
    let middles: Vec<Option<f64>> = (0..line.slopes.len())
        .map(|i| {
            let (a, b) = (line.points.get(i)?, line.points.get((i + 1) % n.max(1))?);
            Some(band.depth_at((*a + *b) / 2.0))
        })
        .collect();
    let clearly = |depth: f64| {
        if depth > pitch {
            Some(true)
        } else if depth < -pitch {
            Some(false)
        } else {
            None
        }
    };
    let mut inside = middles
        .iter()
        .flatten()
        .find_map(|&d| clearly(d))
        .unwrap_or(false);
    for (slope, depth) in line.slopes.iter_mut().zip(middles) {
        if let Some(now) = depth.and_then(clearly) {
            inside = now;
        }
        if inside {
            *slope = Some(std::f64::consts::FRAC_PI_2);
        }
    }
    line
}

/// The steepest a stretch may be and still be cut along, up to `to`: a plunge's.
pub(crate) fn ceiling(to: f64) -> f64 {
    PLUNGE_RATIO.atan().min(to)
}

/// How steep the surface the tool's tip rides is at `p`, in radians from level:
/// from the drop-cutter's heights `h` either side, across and along. `None` where
/// the tool is off the part.
fn cl_slope<T: DropTest>(
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

/// The stretches of `line`, closed back to its start if `closed`, whose slope
/// under the tool is in `[from, to]` and not past `ceiling`, judged every `pitch`
/// ([`Sloped::within`]): all of it when the range is every slope.
pub(crate) fn keep<T: DropTest + Sync>(
    cutter: &DropCutter<'_>,
    tool: &T,
    (line, closed): (Vec<DVec2>, bool),
    pitch: f64,
    (from, ceiling, to): (f64, f64, f64),
    counts: &mut SlopeCounts,
) -> Vec<(Vec<DVec2>, bool)> {
    if is_everything(from, to) {
        return vec![(line, closed)];
    }
    Sloped::split(&line, closed, pitch, |m, scratch| {
        cl_slope(cutter, tool, m, pitch / 2.0, scratch)
    })
    .within(from, ceiling, to, counts)
}

/// A line to cut, with the slope under each of its segments.
pub(crate) struct Sloped {
    pub(crate) points: Vec<DVec2>,
    pub(crate) closed: bool,
    /// One a segment: `points[i]` to the next. `None` where nothing is known.
    pub(crate) slopes: Vec<Option<f64>>,
    /// One a point: whether it is the line's own, not one it was only split at.
    pub(crate) own: Vec<bool>,
}

impl Sloped {
    /// `line`, closed back to its first point if `closed`, split so no segment is
    /// longer than `pitch`, with `slope_at` the middle of each: asked in
    /// parallel, each thread with scratch space of its own. The points it is
    /// split at are not its own.
    pub(crate) fn split(
        line: &[DVec2],
        closed: bool,
        pitch: f64,
        slope_at: impl Fn(DVec2, &mut Vec<u32>) -> Option<f64> + Sync,
    ) -> Self {
        let (mut points, mut own) = (Vec::new(), Vec::new());
        let segments = if closed {
            line.len()
        } else {
            line.len().saturating_sub(1)
        };
        for (&a, &b) in line.iter().zip(line.iter().cycle().skip(1)).take(segments) {
            let pieces = bounded_count((a.distance(b) / pitch).ceil());
            for k in 0..pieces {
                points.push(a.lerp(b, f64::from(k) / f64::from(pieces)));
                own.push(k == 0);
            }
        }
        if !closed && let Some(&last) = line.last() {
            points.push(last);
            own.push(true);
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
            own,
        }
    }

    /// The stretches of it whose slope is in `[from, to]` and not past `ceiling`,
    /// each its two ends and the line's own points between; counting each
    /// stretch left out, as too steep or out of the range asked for.
    pub(crate) fn within(
        self,
        from: f64,
        ceiling: f64,
        to: f64,
        w: &mut SlopeCounts,
    ) -> Vec<(Vec<DVec2>, bool)> {
        let keep = |s: &Option<f64>| s.is_none_or(|s| s >= from - 1e-12 && s <= ceiling + 1e-12);
        let segments = self.slopes.len();
        let kept = |i: usize| self.slopes.get(i).is_none_or(keep);
        if (0..segments).all(kept) {
            let points = self
                .points
                .into_iter()
                .zip(self.own)
                .filter_map(|(p, own)| own.then_some(p))
                .collect();
            return vec![(points, self.closed)];
        }
        let own = |i: usize| self.own.get(i).copied().unwrap_or(true);
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
                } else if !own(i) {
                    // Only split at, and the stretch runs on past it.
                    run.pop();
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use glam::dvec2;

    /// `line`, judged every millimetre, the slope `at` the middle of each piece.
    fn judged(line: &[DVec2], closed: bool, at: impl Fn(DVec2) -> f64 + Sync) -> Sloped {
        Sloped::split(line, closed, 1.0, |m, _| Some(at(m)))
    }

    /// A stretch is the line's own points and the two it is cut at, not every
    /// point the line was split at to judge its slope.
    #[test]
    fn a_stretch_keeps_the_lines_own_points_and_its_ends() {
        let l = [dvec2(0.0, 0.0), dvec2(8.0, 0.0), dvec2(8.0, 8.0)];
        let mut counts = SlopeCounts::default();
        // All of it within the range: the line as it was.
        let all = judged(&l, false, |_| 0.5).within(0.2, 1.0, 1.0, &mut counts);
        assert_eq!(all, vec![(l.to_vec(), false)]);
        // Level short of x = 5: from there, round the corner.
        let rest = judged(&l, false, |m| if m.x < 5.0 { 0.0 } else { 0.5 }).within(
            0.2,
            1.0,
            1.0,
            &mut counts,
        );
        assert_eq!(
            rest,
            vec![(
                vec![dvec2(5.0, 0.0), dvec2(8.0, 0.0), dvec2(8.0, 8.0)],
                false
            )]
        );
        // Within the range between x = 2 and 5 only: its ends alone.
        let middle = judged(
            &l,
            false,
            |m| if m.x > 2.0 && m.x < 5.0 { 0.5 } else { 0.0 },
        )
        .within(0.2, 1.0, 1.0, &mut counts);
        assert_eq!(
            middle,
            vec![(vec![dvec2(2.0, 0.0), dvec2(5.0, 0.0)], false)]
        );
        // A closed square, level above y = 6: open, from where that ends.
        let square = [
            dvec2(0.0, 0.0),
            dvec2(8.0, 0.0),
            dvec2(8.0, 8.0),
            dvec2(0.0, 8.0),
        ];
        let below = judged(&square, true, |m| if m.y > 6.0 { 0.0 } else { 0.5 }).within(
            0.2,
            1.0,
            1.0,
            &mut counts,
        );
        assert_eq!(
            below,
            vec![(
                vec![
                    dvec2(0.0, 6.0),
                    dvec2(0.0, 0.0),
                    dvec2(8.0, 0.0),
                    dvec2(8.0, 6.0)
                ],
                false
            )]
        );
        assert_eq!(counts.outside_slope, 4);
    }
}
