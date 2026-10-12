//! Finishing by a pattern laid out seen from above and dropped onto the part:
//! what the 3D finishing strategies share once their passes are placed.
//!
//! A strategy lays out its pattern as lines in the plane, in the order they are
//! cut and each the way it runs: a spiral, spokes, a spiral morphed to the
//! boundary. [`project`] keeps them to the area, and to the slopes asked for, and
//! samples each on the part exactly, as a parallel pass is
//! ([`crate::strategy::parallel::sweep_polyline`]): refined until it is within tolerance,
//! and broken where nothing is under the tool, at a wall, and down a plunge. It
//! keeps the pattern's order: the linker joins each pass to the next.
//!
//! [`Emit`] writes passes into the path as every finishing strategy does: a
//! [`MoveRole::Link`] to its start, for the linker, then its cuts.

use std::collections::HashMap;

use camcraft_geom::contour::{Region, polyline_length};
use camcraft_geom::mesh::distance_to_segment;
use glam::{DVec2, DVec3};
use rayon::prelude::*;

use crate::cutter::dropcutter::{DropCutter, DropTest};
use crate::path::toolpath::{MoveRole, Toolpath};
use crate::strategy::field;
use crate::strategy::nesting::nearest;
use crate::strategy::parallel::{
    ParallelParams, ParallelReport, Run, sweep_open_polyline, sweep_polyline,
};
use crate::strategy::slope::{self, SlopeCounts};
use crate::strategy::{StrategyError, check};

/// A line of a pattern, seen from above.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    /// Its points; a closed curve does not repeat its first.
    pub points: Vec<DVec2>,
    /// Whether it closes on itself.
    pub closed: bool,
}

impl Curve {
    /// An open curve through `points`.
    pub fn open(points: Vec<DVec2>) -> Self {
        Self {
            points,
            closed: false,
        }
    }

    /// A closed curve through `points`.
    pub fn closed(points: Vec<DVec2>) -> Self {
        Self {
            points,
            closed: true,
        }
    }
}

/// How a pattern is dropped onto the part.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectParams {
    /// Distance between neighbouring passes, in millimetres: slopes are judged
    /// every third of it, or every half of the tool's radius where that is less,
    /// and a stretch shorter than two of those is not cut.
    pub stepover: f64,
    /// Chordal tolerance along each pass, in millimetres.
    pub tolerance: f64,
    /// Cutting feed, in mm/min.
    pub feed: f64,
    /// Largest Z drop, in millimetres, that may be cut rather than linked.
    pub max_cut_step: f64,
    /// Where the surface is at least this steep, in radians from level.
    pub slope_from: f64,
    /// And at most this steep.
    pub slope_to: f64,
}

impl Default for ProjectParams {
    fn default() -> Self {
        Self {
            stepover: 0.5,
            tolerance: 0.01,
            feed: 1000.0,
            max_cut_step: 0.5,
            slope_from: 0.0,
            slope_to: std::f64::consts::FRAC_PI_2,
        }
    }
}

/// What happened while dropping a pattern onto the part.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ProjectReport {
    /// Passes cut.
    pub passes: u64,
    /// Stretches left out for being outside the slope range asked for.
    pub outside_slope: u64,
    /// Stretches too short to be worth cutting.
    pub fragments: u64,
    /// What sampling the passes on the part found, as for parallel finishing.
    pub sweep: ParallelReport,
}

impl ProjectReport {
    /// Whether anything needs the caller's attention.
    pub fn is_clean(&self) -> bool {
        self.sweep.is_clean()
    }
}

/// `curves`, in order, kept to `area` and the slopes asked for, and cut on the
/// part.
///
/// The path holds [`MoveRole::Cut`] passes separated by [`MoveRole::Link`] moves,
/// in the pattern's order. A closed curve kept whole is started nearest where
/// the tool is.
pub fn project<T: DropTest + Sync>(
    cutter: &DropCutter<'_>,
    tool: &T,
    area: &Region,
    curves: &[Curve],
    params: &ProjectParams,
) -> Result<(Toolpath, ProjectReport), StrategyError> {
    check("stepover", params.stepover)?;
    check("tolerance", params.tolerance)?;
    check("feed", params.feed)?;
    check("max_cut_step", params.max_cut_step)?;
    slope::check_range(params.slope_from, params.slope_to)?;
    let mut report = ProjectReport::default();
    // Slopes judged as scallop's grid does, and kept to the range asked for and
    // nothing else, as Fusion's: a steep stretch is cut down the part as a
    // parallel pass is, broken only where it falls further than a cut step.
    let pitch = field::pitch(params.stepover, tool.max_radius());
    let range = (params.slope_from, params.slope_to, params.slope_to);
    let thin = 0.25 * params.tolerance;
    let sweep = ParallelParams {
        stepover: params.stepover,
        tolerance: params.tolerance,
        feed: params.feed,
        max_cut_step: params.max_cut_step,
        ..Default::default()
    };

    // Each stage in parallel over what the last gave, and in its order: a
    // spiral is one line, but many stretches once kept to the area. The
    // stretches within the slopes asked for...
    let kept: Vec<(Vec<Stretch>, SlopeCounts)> = curves
        .par_iter()
        .map(|curve| {
            let mut counts = SlopeCounts::default();
            let points = simplify(&curve.points, curve.closed, thin);
            let stretches = slope::keep(
                cutter,
                tool,
                (points, curve.closed),
                pitch,
                range,
                &mut counts,
            );
            (stretches, counts)
        })
        .collect();
    let mut stretches = Vec::new();
    for (some, counts) in kept {
        report.outside_slope += counts.outside_slope;
        stretches.extend(some);
    }
    // ...then within the area, long enough to be worth cutting...
    let clipped = stretches
        .par_iter()
        .map(|(points, closed)| clip_in_order(area, points, *closed))
        .collect::<Result<Vec<_>, StrategyError>>()?;
    let mut pieces = Vec::new();
    for (points, closed) in clipped.into_iter().flatten() {
        if !closed && polyline_length(&points) < 2.0 * pitch {
            report.fragments += 1;
        } else {
            pieces.push((points, closed));
        }
    }
    // ...and cut on the part.
    let swept: Vec<(Vec<Run>, ParallelReport)> = pieces
        .par_iter()
        .map(|(points, closed)| {
            let mut found = ParallelReport::default();
            let runs = if *closed {
                sweep_polyline(cutter, tool, points, true, &sweep, &mut found)
            } else {
                sweep_open_polyline(cutter, tool, points, &sweep, &mut found)
            };
            (runs, found)
        })
        .collect();

    let mut emit = Emit::new(params.feed);
    for (runs, found) in swept {
        report.sweep += found;
        for mut run in runs {
            if run.closed
                && let Some(at) = emit.at
            {
                start_near(&mut run, at);
            }
            emit.emit(&run)?;
        }
    }
    report.passes = emit.passes;
    Ok((emit.path, report))
}

/// A stretch of a line: its points, and whether it closes on itself.
type Stretch = (Vec<DVec2>, bool);

/// The stretches of the line `points` inside `area`, in the order the line runs
/// through them: the clipper hands them back in an order of its own.
fn clip_in_order(
    area: &Region,
    points: &[DVec2],
    closed: bool,
) -> Result<Vec<Stretch>, StrategyError> {
    let pieces = area.clip_lines(&[(points.to_vec(), closed)])?;
    if pieces.len() < 2 {
        return Ok(pieces);
    }
    // Where along the line each piece starts: by the first point of the line it
    // keeps, which the clipper hands back to within its tick; failing that, by
    // the segment nearest its start.
    let key = |p: DVec2| {
        // A hash key a micron across, not a coordinate.
        #[allow(clippy::cast_possible_truncation)]
        let k = ((p.x * 1.0e3).round() as i64, (p.y * 1.0e3).round() as i64);
        k
    };
    let mut at: HashMap<(i64, i64), usize> = HashMap::new();
    for (i, &p) in points.iter().enumerate() {
        at.entry(key(p)).or_insert(i);
    }
    let segments = if closed {
        points.len()
    } else {
        points.len().saturating_sub(1)
    };
    // Places counted in half segments: `2i` at vertex `i`, `2i + 1` along the
    // segment after it.
    let nearest_segment = |p: DVec2| -> usize {
        (0..segments)
            .filter_map(|i| {
                let a = *points.get(i)?;
                let b = *points.get((i + 1) % points.len())?;
                Some((i, distance_to_segment(p, a, b)))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(usize::MAX, |(i, _)| 2 * i + 1)
    };
    let place = |piece: &[DVec2]| -> usize {
        let kept = piece.iter().skip(1).find_map(|&p| at.get(&key(p)).copied());
        match (kept, piece.first()) {
            // Along the segment before the vertex it keeps.
            (Some(i), _) => (2 * i).saturating_sub(1),
            (None, Some(&first)) => nearest_segment(first),
            (None, None) => usize::MAX,
        }
    };
    let mut placed: Vec<(usize, Stretch)> = pieces.into_iter().map(|p| (place(&p.0), p)).collect();
    placed.sort_by_key(|p| p.0);
    Ok(placed.into_iter().map(|(_, p)| p).collect())
}

/// Passes written into a path, in the order they are cut, from where the tool is.
pub(crate) struct Emit {
    /// The path so far.
    pub(crate) path: Toolpath,
    /// Where the last pass left the tool, seen from above.
    pub(crate) at: Option<DVec2>,
    feed: f64,
    /// Passes written.
    pub(crate) passes: u64,
    /// Ramping down along a closed ring from the closed ring above it, as
    /// contour finishing goes from level to level: the ramp's angle from level,
    /// in radians, and how near where the last ended the ring must start.
    pub(crate) ramp: Option<(f64, f64)>,
    /// Where the last pass left the tool, and whether it was closed.
    end: Option<(DVec3, bool)>,
}

impl Emit {
    pub(crate) fn new(feed: f64) -> Self {
        Self {
            path: Toolpath::new(),
            at: None,
            feed,
            passes: 0,
            ramp: None,
            end: None,
        }
    }

    /// `run`: a link to its start, for the linker to resolve, then its cuts; a
    /// closed one back round to where it started.
    pub(crate) fn emit(&mut self, run: &Run) -> Result<(), StrategyError> {
        let Some(&first) = run.points.first() else {
            return Ok(());
        };
        if self.ramp_onto(run)? {
            return Ok(());
        }
        // The strategy does not plan how to arrive; the linker does.
        self.path.feed_to(first, MoveRole::Link, self.feed)?;
        for &p in run.points.iter().skip(1) {
            self.path.feed_to(p, MoveRole::Cut, self.feed)?;
        }
        if run.closed {
            self.path.feed_to(first, MoveRole::Cut, self.feed)?;
        }
        self.passes += 1;
        let end = if run.closed {
            first
        } else {
            run.points.last().map_or(first, |p| *p)
        };
        self.at = Some(end.truncate());
        self.end = Some((end, run.closed));
        Ok(())
    }

    /// `run`, a closed ring below the closed ring just cut that starts beside
    /// where that ended, gone down to along itself at the ramp angle, from the
    /// height the tool left, and then cut once round from where the ramp
    /// reaches it. `false`, writing nothing, where it is not such a ring.
    fn ramp_onto(&mut self, run: &Run) -> Result<bool, StrategyError> {
        let (Some((angle, near)), Some((left, true)), true) = (self.ramp, self.end, run.closed)
        else {
            return Ok(false);
        };
        // The ring started at its point nearest where the tool left off, on a
        // side or at a corner.
        let ring = &run.points;
        let n = ring.len();
        let from = left.truncate();
        let nearest = (0..n)
            .filter_map(|i| {
                let (a, b) = (*ring.get(i)?, *ring.get((i + 1) % n)?);
                let (a2, b2) = (a.truncate(), b.truncate());
                let along = (b2 - a2).length_squared();
                let t = if along > 0.0 {
                    ((from - a2).dot(b2 - a2) / along).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let p = a.lerp(b, t);
                Some((p.truncate().distance(from), i, p))
            })
            .min_by(|x, y| x.0.total_cmp(&y.0));
        let Some((gap, i, start)) = nearest else {
            return Ok(false);
        };
        let points: Vec<DVec3> = std::iter::once(start)
            .chain((1..=n).filter_map(|k| ring.get((i + k) % n).copied()))
            .collect();
        let n = points.len();
        let first = start;
        let drop = left.z - first.z;
        let lap: f64 = (0..n)
            .filter_map(|i| Some(points.get(i)?.distance(*points.get((i + 1) % n)?)))
            .sum();
        if n < 3 || drop <= 1e-9 || lap <= 1e-9 || gap > near {
            return Ok(false);
        }
        let length = drop / angle.tan();
        let ramp = MoveRole::Transition;
        // Over to the ring, at the height the tool left.
        let mut at = left;
        let mut go = |path: &mut Toolpath, p: DVec3, role: MoveRole| {
            if p.distance(at) > 1e-9 {
                at = p;
                path.feed_to(p, role, self.feed)
            } else {
                Ok(())
            }
        };
        go(&mut self.path, first.truncate().extend(left.z), ramp)?;
        // Down along it, round again if it is short, to where the ramp reaches
        // the ring's level.
        let mut along = 0.0;
        let mut i = 0;
        let end = loop {
            let (Some(&a), Some(&b)) = (points.get(i % n), points.get((i + 1) % n)) else {
                return Ok(false);
            };
            let piece = a.distance(b);
            if along + piece >= length {
                let t = if piece > 0.0 {
                    (length - along) / piece
                } else {
                    1.0
                };
                break (i, a.lerp(b, t));
            }
            along += piece;
            go(
                &mut self.path,
                b.truncate().extend(left.z - drop * along / length),
                ramp,
            )?;
            i += 1;
        };
        let (i, reached) = end;
        go(&mut self.path, reached, ramp)?;
        // Once round from there, back to it.
        for k in 1..=n {
            if let Some(&p) = points.get((i + k) % n) {
                go(&mut self.path, p, MoveRole::Cut)?;
            }
        }
        go(&mut self.path, reached, MoveRole::Cut)?;
        self.passes += 1;
        self.at = Some(reached.truncate());
        self.end = Some((reached, true));
        Ok(true)
    }

    /// `runs`, the nearest next from where the tool is: a closed one started at
    /// its point nearest, and, `both_ways`, an open one from its nearer end.
    pub(crate) fn nearest_first(
        &mut self,
        mut runs: Vec<Run>,
        both_ways: bool,
    ) -> Result<(), StrategyError> {
        while !runs.is_empty() {
            let at = self.at;
            let k = nearest(&runs, |r| {
                at.map_or(0.0, |a| start_distance(r, a, both_ways))
            });
            let mut run = runs.remove(k);
            if let Some(at) = at {
                if run.closed {
                    start_near(&mut run, at);
                } else if both_ways
                    && let (Some(first), Some(last)) = (run.points.first(), run.points.last())
                    && last.truncate().distance(at) < first.truncate().distance(at)
                {
                    run.points.reverse();
                }
            }
            self.emit(&run)?;
        }
        Ok(())
    }
}

/// How far the tool at `at` is from where `run` would start.
pub(crate) fn start_distance(run: &Run, at: DVec2, both_ways: bool) -> f64 {
    let d = |p: &DVec3| p.truncate().distance(at);
    if run.closed {
        run.points.iter().map(d).fold(f64::MAX, f64::min)
    } else {
        let first = run.points.first().map_or(f64::MAX, d);
        let last = run.points.last().map_or(f64::MAX, d);
        if both_ways { first.min(last) } else { first }
    }
}

/// A closed run started at its point nearest `at`, going the same way.
fn start_near(run: &mut Run, at: DVec2) {
    let nearest = run
        .points
        .iter()
        .enumerate()
        .min_by(|a, b| {
            a.1.truncate()
                .distance(at)
                .total_cmp(&b.1.truncate().distance(at))
        })
        .map(|(i, _)| i);
    if let Some(i) = nearest {
        run.points.rotate_left(i);
    }
}

/// `line`, thinned: each point dropped that is within `tolerance` of the line
/// through the ones kept either side of it, seen from above (Douglas and Peucker).
pub(crate) fn simplify(line: &[DVec2], closed: bool, tolerance: f64) -> Vec<DVec2> {
    let mut points = line.to_vec();
    if closed && let Some(&first) = line.first() {
        points.push(first);
    }
    if points.len() < 3 {
        return line.to_vec();
    }
    let mut keep = vec![false; points.len()];
    if let Some(k) = keep.first_mut() {
        *k = true;
    }
    if let Some(k) = keep.last_mut() {
        *k = true;
    }
    let mut spans = vec![(0usize, points.len() - 1)];
    // A closed line starts and ends at one point: split it at the point furthest
    // from there first, so the rest is judged against a real chord.
    if closed && let Some(&start) = points.first() {
        let far = points
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.distance(start).total_cmp(&b.1.distance(start)))
            .map_or(0, |(i, _)| i);
        if far > 0 && far < points.len() - 1 {
            if let Some(k) = keep.get_mut(far) {
                *k = true;
            }
            spans = vec![(0, far), (far, points.len() - 1)];
        }
    }
    while let Some((a, b)) = spans.pop() {
        let (Some(&pa), Some(&pb)) = (points.get(a), points.get(b)) else {
            continue;
        };
        let mut worst = (0usize, 0.0f64);
        for i in a + 1..b {
            let Some(&p) = points.get(i) else { continue };
            let d = distance_to_segment(p, pa, pb);
            if d > worst.1 {
                worst = (i, d);
            }
        }
        if worst.1 > tolerance {
            if let Some(k) = keep.get_mut(worst.0) {
                *k = true;
            }
            spans.push((a, worst.0));
            spans.push((worst.0, b));
        }
    }
    let mut out: Vec<DVec2> = points
        .iter()
        .zip(&keep)
        .filter_map(|(&p, &k)| k.then_some(p))
        .collect();
    if closed {
        out.pop();
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use camcraft_geom::synthetic;
    use glam::dvec2;

    use super::*;
    use crate::cutter::allowance::{Allowance, EffectiveTool};
    use crate::cutter::tool::Flat;

    /// The slope range is the one asked for and nothing else, as Fusion's: a
    /// step lower than a cut step, steeper than a plunge where the tool drops
    /// off it, is cut down to 89.9° as it is to 90°.
    #[test]
    fn a_range_short_of_a_wall_cuts_what_every_slope_does() {
        let mesh = synthetic::step(0.4, 6.0);
        let cutter = DropCutter::new(&mesh);
        let tool = EffectiveTool::new(&Flat::new(1.0).unwrap(), Allowance::none()).unwrap();
        let area = Region::rectangle(dvec2(-5.0, -5.0), dvec2(5.0, 5.0));
        let across = [Curve::open(vec![dvec2(-4.0, 0.0), dvec2(4.0, 0.0)])];
        let cut = |slope_to: f64| {
            let params = ProjectParams {
                stepover: 0.1,
                tolerance: 0.01,
                slope_to,
                ..Default::default()
            };
            project(&cutter, &tool, &area, &across, &params).unwrap()
        };
        let (all, every) = cut(std::f64::consts::FRAC_PI_2);
        let (short, report) = cut(89.9_f64.to_radians());
        assert_eq!(every.passes, 1, "{every:?}");
        assert_eq!(report.passes, 1, "{report:?}");
        assert_eq!(short, all);
    }
}
