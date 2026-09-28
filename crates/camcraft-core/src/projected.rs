//! Finishing by a pattern laid out seen from above and dropped onto the part:
//! what the 3D finishing strategies share once their passes are placed.
//!
//! A strategy lays out its pattern as lines in the plane, in the order they are
//! cut and each the way it runs: a spiral, spokes, a spiral morphed to the
//! boundary. [`project`] keeps them to the area, and to the slopes asked for, and
//! samples each on the part exactly, as a parallel pass is
//! ([`crate::parallel::sweep_polyline`]): refined until it is within tolerance,
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

use crate::dropcutter::{DropCutter, DropTest};
use crate::parallel::{ParallelParams, ParallelReport, Run, sweep_open_polyline, sweep_polyline};
use crate::slope::{self, SlopeCounts, Sloped};
use crate::strategy::{StrategyError, check};
use crate::toolpath::{MoveRole, Toolpath};

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
    /// every third of it, and a stretch shorter than two thirds is not cut.
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
    /// Stretches left out for being steeper than a plunge.
    pub too_steep: u64,
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
    // As scallop's grid: a third of a stepover, fine enough for the tool.
    let pitch = (params.stepover / 3.0)
        .min(tool.max_radius() / 2.0)
        .max(1e-3);
    let everything = slope::is_everything(params.slope_from, params.slope_to);
    let ceiling = slope::ceiling(params.slope_to);
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
            let stretches = if everything {
                vec![(points, curve.closed)]
            } else {
                Sloped::split(&points, curve.closed, pitch, |m, scratch| {
                    slope::cl_slope(cutter, tool, m, pitch / 2.0, scratch)
                })
                .within(params.slope_from, ceiling, params.slope_to, &mut counts)
            };
            (stretches, counts)
        })
        .collect();
    let mut stretches = Vec::new();
    for (some, counts) in kept {
        report.too_steep += counts.too_steep;
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
pub(crate) fn clip_in_order(
    area: &Region,
    points: &[DVec2],
    closed: bool,
) -> Result<Vec<(Vec<DVec2>, bool)>, StrategyError> {
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
    let mut placed: Vec<(usize, (Vec<DVec2>, bool))> =
        pieces.into_iter().map(|p| (place(&p.0), p)).collect();
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
}

impl Emit {
    pub(crate) fn new(feed: f64) -> Self {
        Self {
            path: Toolpath::new(),
            at: None,
            feed,
            passes: 0,
        }
    }

    /// `run`: a link to its start, for the linker to resolve, then its cuts; a
    /// closed one back round to where it started.
    pub(crate) fn emit(&mut self, run: &Run) -> Result<(), StrategyError> {
        let Some(&first) = run.points.first() else {
            return Ok(());
        };
        // The strategy does not plan how to arrive; the linker does.
        self.path.feed_to(first, MoveRole::Link, self.feed)?;
        for &p in run.points.iter().skip(1) {
            self.path.feed_to(p, MoveRole::Cut, self.feed)?;
        }
        if run.closed {
            self.path.feed_to(first, MoveRole::Cut, self.feed)?;
        }
        self.passes += 1;
        self.at = Some(if run.closed {
            first.truncate()
        } else {
            run.points.last().map_or(first, |p| *p).truncate()
        });
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
pub(crate) fn start_near(run: &mut Run, at: DVec2) {
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
