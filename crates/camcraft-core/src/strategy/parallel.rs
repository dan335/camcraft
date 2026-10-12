//! Parallel (raster) finishing.
//!
//! The simplest 3D finishing strategy, and the one that proves the pipeline: sweep
//! the tool along evenly spaced straight lines and let it ride the surface.
//!
//! # Sampled from the exact query, not the grid
//!
//! Every emitted point comes from [`DropCutter`], never from an interpolated height
//! map. Between grid nodes an interpolant passes *below* a convex surface, and below
//! is a gouge. The grid is for areal questions; this is a path.
//!
//! # Refinement is adaptive and bounded
//!
//! A fixed sample spacing is either too coarse on curvature or ruinously fine on
//! flats. Instead each span is split while its midpoint deviates from the chord by
//! more than `tolerance`, up to a fixed depth. Bounded, so a pathological surface
//! produces a coarse path and a diagnostic rather than a hang.
//!
//! # Gaps become links, not guesses
//!
//! Where nothing lies under the tool the pass simply stops, and the connection to
//! the next one is marked [`MoveRole::Link`] for the linker to resolve. The strategy
//! does not invent a traverse it has no business planning.
//!
//! # Kept to a boundary by clipping lines, not probes
//!
//! Kept to picked faces ([`parallel_finish_within`]), each line is cut exactly to
//! where it crosses the boundary, and each piece swept on its own. Asking the
//! drop-cutter for nothing outside instead would end every pass up to a sample
//! short of the boundary, and count each as a coverage break.

use camcraft_geom::contour::Region;
use camcraft_geom::steps::bounded_count;
use glam::{DVec2, DVec3, dvec2, dvec3};
use rayon::prelude::*;

use crate::cutter::dropcutter::{DropCutter, DropTest};
use crate::path::toolpath::{PLUNGE_RATIO, Toolpath};
use crate::strategy::projected::Emit;
use crate::strategy::{StrategyError, check};

/// How to lay out the passes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParallelParams {
    /// Distance between neighbouring passes, in millimetres.
    pub stepover: f64,
    /// Pass direction, in radians. Zero runs along +X.
    pub angle: f64,
    /// Chordal tolerance for adaptive refinement, in millimetres.
    pub tolerance: f64,
    /// Cutting feed rate, in mm/min.
    pub feed: f64,
    /// Maximum refinement depth per span.
    pub max_depth: u32,
    /// Largest Z drop, in millimetres, that may be traversed as a cutting move.
    ///
    /// Where the CL surface is discontinuous — any tool crossing a vertical wall —
    /// two adjacent samples can sit far apart in Z while barely separated in XY.
    /// Both are correct. Joining them with a cut is not: it is a full-depth plunge
    /// at cutting feed. Past this drop the pass is broken instead, and the linker
    /// turns the break into a retract, a traverse and a controlled plunge.
    pub max_cut_step: f64,
}

impl Default for ParallelParams {
    fn default() -> Self {
        Self {
            stepover: 0.5,
            angle: 0.0,
            tolerance: 0.01,
            feed: 1000.0,
            max_depth: 12,
            max_cut_step: 0.5,
        }
    }
}

/// What happened while building a path, beyond the path itself.
///
/// Returned rather than logged, because silently emitting a path that hit its
/// refinement limit is exactly the failure this project refuses to have.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParallelReport {
    /// Spans that exhausted [`ParallelParams::max_depth`] while still out of
    /// tolerance. Non-zero means the path is coarser than requested.
    pub refinement_limited: u64,
    /// Spans that shrank below the geometric limit while still out of tolerance.
    ///
    /// This is the signature of a genuine discontinuity in the CL surface — a flat
    /// mill crossing a step, or any tool passing a vertical wall — where no chord
    /// can follow the surface however finely it is split. Expected on most real
    /// parts, and distinct from giving up early, which is why it is counted
    /// separately. Every emitted point is still exactly on the surface.
    pub discontinuities: u64,
    /// Contiguous runs of cutting produced.
    pub passes: u64,
    /// Passes broken because coverage ended part-way through a span.
    ///
    /// The tool reaches no material somewhere between two samples, so the two sides
    /// must not be joined by a cut. Common on the outermost scanlines, which graze
    /// the edge of the part.
    pub coverage_breaks: u64,
    /// Passes broken because the surface dropped further than `max_cut_step`,
    /// at a wall or down a face steeper than [`PLUNGE_RATIO`].
    ///
    /// Not a fault. Each one is a plunge that would otherwise have been cut at
    /// feed, and is now a retract and a controlled re-entry.
    pub wall_breaks: u64,
}

impl std::ops::AddAssign for ParallelReport {
    fn add_assign(&mut self, other: Self) {
        let Self {
            refinement_limited,
            discontinuities,
            passes,
            coverage_breaks,
            wall_breaks,
        } = other;
        self.refinement_limited += refinement_limited;
        self.discontinuities += discontinuities;
        self.passes += passes;
        self.coverage_breaks += coverage_breaks;
        self.wall_breaks += wall_breaks;
    }
}

impl ParallelReport {
    /// Whether anything needs the caller's attention.
    ///
    /// Discontinuities do not count: they are a property of the part, not a
    /// shortcoming of the path.
    pub fn is_clean(&self) -> bool {
        self.refinement_limited == 0
    }
}

/// Sweep `tool` along parallel lines across `[min, max]`.
///
/// Returns the path together with a report. The path contains [`MoveRole::Cut`] runs
/// separated by [`MoveRole::Link`] moves; it is not yet machinable, and the linker
/// turns those links into real traverses.
pub fn parallel_finish<T: DropTest + Sync>(
    cutter: &DropCutter<'_>,
    tool: &T,
    min: DVec2,
    max: DVec2,
    params: &ParallelParams,
) -> Result<(Toolpath, ParallelReport), StrategyError> {
    parallel_finish_within(cutter, tool, min, max, None, params)
}

/// As [`parallel_finish`], with the tool's centre kept inside `boundary` as well
/// as `[min, max]`.
///
/// The lines are laid across the boundary rather than the rectangle, centred on it
/// so neither side is a whole stepover short. Without a boundary this is
/// `parallel_finish`, move for move.
///
/// The lines are sampled in parallel, each with scratch space of its own, and
/// written into the path in order.
pub fn parallel_finish_within<T: DropTest + Sync>(
    cutter: &DropCutter<'_>,
    tool: &T,
    min: DVec2,
    max: DVec2,
    boundary: Option<&Region>,
    params: &ParallelParams,
) -> Result<(Toolpath, ParallelReport), StrategyError> {
    check("stepover", params.stepover)?;
    check("tolerance", params.tolerance)?;
    check("feed", params.feed)?;
    check("max_cut_step", params.max_cut_step)?;
    if !params.angle.is_finite() {
        return Err(StrategyError::BadParameter {
            name: "angle",
            value: params.angle,
        });
    }
    if !(min.x < max.x && min.y < max.y) {
        return Err(StrategyError::EmptyRegion { min, max });
    }

    let (sin, cos) = params.angle.sin_cos();
    let along = dvec2(cos, sin);
    let across = dvec2(-sin, cos);

    // Project the region corners onto both axes to find the sweep extents.
    let corners = [min, dvec2(max.x, min.y), max, dvec2(min.x, max.y)];
    let span = |axis: DVec2| {
        corners.iter().fold((f64::MAX, f64::MIN), |(lo, hi), c| {
            let v = c.dot(axis);
            (lo.min(v), hi.max(v))
        })
    };
    let (u0, u1) = span(along);
    let (mut v0, mut v1) = span(across);
    let mut start = v0;
    if let Some(b) = boundary {
        // Across the boundary, where it lies within the rectangle.
        let (lo, hi) =
            b.rings()
                .iter()
                .flat_map(|r| r.points())
                .fold((f64::MAX, f64::MIN), |(lo, hi), p| {
                    let v = p.dot(across);
                    (lo.min(v), hi.max(v))
                });
        (v0, v1) = (v0.max(lo), v1.min(hi));
        if v0.is_nan() || v1.is_nan() || v0 >= v1 {
            return Ok((Toolpath::new(), ParallelReport::default()));
        }
        let n = ((v1 - v0) / params.stepover).floor();
        start = v0 + (v1 - v0 - n * params.stepover) / 2.0;
    }

    let lines = bounded_count(((v1 - start) / params.stepover).floor() + 1.0);

    // Each line's runs, in order.
    let swept: Vec<(Vec<Run>, ParallelReport)> = (0..lines)
        .into_par_iter()
        .map_init(Vec::new, |scratch, i| {
            let mut found = ParallelReport::default();
            let v = start + f64::from(i) * params.stepover;
            // Zigzag: reverse alternate passes so the tool does not traverse back.
            let reversed = i % 2 == 1;
            let mut pieces = match boundary {
                None => vec![(u0, u1)],
                Some(b) => b
                    .spans(along, v)
                    .into_iter()
                    .map(|(a, b)| (a.max(u0), b.min(u1)))
                    .filter(|(a, b)| a < b)
                    .collect(),
            };
            if reversed {
                pieces.reverse();
            }

            let mut runs = Vec::new();
            for (p0, p1) in pieces {
                let (a, b) = if reversed { (p1, p0) } else { (p0, p1) };
                let mut samples = Vec::new();
                Line {
                    probe: |u: f64| {
                        let p = along * u + across * v;
                        cutter
                            .drop_at_in(tool, p.x, p.y, scratch)
                            .map(|z| dvec3(p.x, p.y, z))
                    },
                    params,
                    out: &mut samples,
                    report: &mut found,
                }
                .sweep(a, b);
                runs.extend(runs_of(samples, false, params, &mut found));
            }
            (runs, found)
        })
        .collect();

    let mut emit = Emit::new(params.feed);
    let mut report = ParallelReport::default();
    for (runs, found) in swept {
        report += found;
        for run in &runs {
            emit.emit(run)?;
        }
    }
    report.passes = emit.passes;
    Ok((emit.path, report))
}

/// The tool's tip on the part along the line from `from` to `to`, as a pass
/// would sample it: seeded every `seed` millimetres and refined until the path
/// between samples is within `tolerance` of the part. Only points with part under
/// the tool; walls are kept, not broken at.
///
/// For the linker, to take a link over the part without reaching into it.
pub(crate) fn follow<T: DropTest>(
    cutter: &DropCutter<'_>,
    tool: &T,
    from: DVec2,
    to: DVec2,
    tolerance: f64,
    seed: f64,
) -> Vec<DVec3> {
    if from.distance(to) < 1e-9 {
        return cutter
            .drop_at(tool, from.x, from.y)
            .map(|z| vec![dvec3(from.x, from.y, z)])
            .unwrap_or_default();
    }
    let params = ParallelParams {
        stepover: seed,
        tolerance,
        max_cut_step: f64::INFINITY,
        ..Default::default()
    };
    let mut report = ParallelReport::default();
    sample_polyline(cutter, tool, &[from, to], false, &params, &mut report)
        .into_iter()
        .flatten()
        .collect()
}

/// A stretch of cutting on the part: its points, and whether it closes on itself.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Run {
    /// Each point on the exact surface; a closed run does not repeat its first.
    pub(crate) points: Vec<DVec3>,
    /// Whether it closes on itself.
    pub(crate) closed: bool,
}

/// The tool's tip on the part along the polyline `points`, closed back to its
/// first point if `closed`: sampled and refined as a parallel pass is, segment by
/// segment, and broken where it is as a parallel pass is, where nothing is under
/// the tool, at a wall, and down a steep stretch.
///
/// A closed line unbroken comes back closed. One broken is started where it
/// breaks, so no run crosses the join.
pub(crate) fn sweep_polyline<T: DropTest>(
    cutter: &DropCutter<'_>,
    tool: &T,
    points: &[DVec2],
    closed: bool,
    params: &ParallelParams,
    report: &mut ParallelReport,
) -> Vec<Run> {
    let samples = sample_polyline(cutter, tool, points, closed, params, report);
    runs_of(samples, closed, params, report)
}

/// Segments of a long open line sampled together, each such stretch on a
/// thread of its own.
const CHUNK: usize = 512;

/// As [`sweep_polyline`], for an open line: a long one sampled a stretch of
/// [`CHUNK`] segments at a time in parallel, each stretch from its first point
/// afresh, then joined and broken as one. The stretches are the line's own,
/// whatever the number of cores, so the result is too.
pub(crate) fn sweep_open_polyline<T: DropTest + Sync>(
    cutter: &DropCutter<'_>,
    tool: &T,
    points: &[DVec2],
    params: &ParallelParams,
    report: &mut ParallelReport,
) -> Vec<Run> {
    if points.len() <= 2 * CHUNK {
        return sweep_polyline(cutter, tool, points, false, params, report);
    }
    let starts: Vec<usize> = (0..points.len() - 1).step_by(CHUNK).collect();
    let stretches: Vec<(Vec<Option<DVec3>>, ParallelReport)> = starts
        .par_iter()
        .map(|&a| {
            let b = (a + CHUNK).min(points.len() - 1);
            let mut found = ParallelReport::default();
            let stretch = points.get(a..=b).unwrap_or_default();
            let samples = sample_polyline(cutter, tool, stretch, false, params, &mut found);
            (samples, found)
        })
        .collect();
    let mut samples = Vec::new();
    for (k, (some, found)) in stretches.into_iter().enumerate() {
        *report += found;
        // Each stretch after the first starts where the last ended, sampled.
        samples.extend(some.into_iter().skip(usize::from(k > 0)));
    }
    runs_of(samples, false, params, report)
}

/// The tool's tip on the part along `points`, closed back to the first if
/// `closed`: each segment sampled and refined, `None` where nothing is under
/// the tool.
fn sample_polyline<T: DropTest>(
    cutter: &DropCutter<'_>,
    tool: &T,
    points: &[DVec2],
    closed: bool,
    params: &ParallelParams,
    report: &mut ParallelReport,
) -> Vec<Option<DVec3>> {
    // The drop-cutter's list of triangles, made once for the whole line.
    let mut scratch = Vec::new();
    let mut samples: Vec<Option<DVec3>> = Vec::new();
    let ends = points.iter().zip(
        points
            .iter()
            .skip(1)
            .chain(points.first().filter(|_| closed)),
    );
    for (&from, &to) in ends {
        let length = from.distance(to);
        if length < 1e-12 {
            continue;
        }
        let along = (to - from) / length;
        // Each segment starts where the last ended, already sampled.
        let start = samples.last().copied();
        let mut line = Line {
            probe: |u: f64| {
                let p = from + along * u;
                cutter
                    .drop_at_in(tool, p.x, p.y, &mut scratch)
                    .map(|z| dvec3(p.x, p.y, z))
            },
            params,
            out: &mut samples,
            report,
        };
        match start {
            Some(first) => line.sweep_on(first, 0.0, length),
            None => line.sweep(0.0, length),
        }
    }

    samples
}

/// A line's samples as runs, broken where it is as a parallel pass is.
fn runs_of(
    mut samples: Vec<Option<DVec3>>,
    closed: bool,
    params: &ParallelParams,
    report: &mut ParallelReport,
) -> Vec<Run> {
    if closed && samples.len() > 2 {
        // The join is the first point, sampled again at the end.
        samples.pop();
        if samples.iter().all(Option::is_some) {
            return ring(samples.into_iter().flatten().collect(), params, report);
        }
        // Broken: started at a break, so no run crosses the join.
        if let Some(at) = samples.iter().position(Option::is_none) {
            samples.rotate_left(at);
        }
    }
    open_runs(
        samples
            .split(|p| p.is_none())
            .flat_map(|run| {
                let run: Vec<DVec3> = run.iter().filter_map(|p| *p).collect();
                break_steep(run, params.max_cut_step, report)
            })
            .collect(),
    )
}

/// A closed line sampled all the way round without a break: one closed run, or,
/// broken down steep stretches, the runs between them, those either side of the
/// join made one.
fn ring(mut points: Vec<DVec3>, params: &ParallelParams, report: &mut ParallelReport) -> Vec<Run> {
    // Judged steep all the way round, back to where it started.
    if let Some(&first) = points.first() {
        points.push(first);
    }
    let mut runs: Vec<Vec<DVec3>> = break_steep(points, params.max_cut_step, report)
        .into_iter()
        .filter(|run| run.len() >= 2)
        .collect();
    if let [whole] = runs.as_mut_slice()
        && whole.first() == whole.last()
    {
        whole.pop();
        return vec![Run {
            points: std::mem::take(whole),
            closed: true,
        }];
    }
    if runs.len() > 1
        && let (Some(last), Some(first)) = (runs.last(), runs.first())
        && last.last() == first.first()
    {
        let head = runs.remove(0);
        if let Some(last) = runs.last_mut() {
            last.extend(head.into_iter().skip(1));
        }
    }
    open_runs(runs)
}

/// `runs` as open runs, those of fewer than two points left out.
fn open_runs(runs: Vec<Vec<DVec3>>) -> Vec<Run> {
    runs.into_iter()
        .filter(|run| run.len() >= 2)
        .map(|points| Run {
            points,
            closed: false,
        })
        .collect()
}

/// Break a run wherever it falls, or climbs, further than `max_cut_step` while
/// steeper than [`PLUNGE_RATIO`].
///
/// Refinement only breaks at a genuine discontinuity. A steep face that is
/// straight passes the chord test at once, so a pass would ride down it at
/// cutting feed; and split finely, each move alone falls little while the whole
/// still plunges. So the run is judged here, as the gate judges it: by each
/// continuous steep stretch in full. Both directions, because ordering may reverse
/// a pass and turn a climb into a descent.
///
/// The stretch is left out: the linker retracts from its top and plunges, at the
/// plunge feed, to its foot.
fn break_steep(run: Vec<DVec3>, max_cut_step: f64, report: &mut ParallelReport) -> Vec<Vec<DVec3>> {
    // How far a segment falls (negative: climbs), when it is steeper than the ratio.
    let steep = |a: DVec3, b: DVec3| -> Option<f64> {
        let fall = a.z - b.z;
        let flat = (b.truncate() - a.truncate()).length();
        (fall.abs() > flat * PLUNGE_RATIO).then_some(fall)
    };
    let mut out = Vec::new();
    let mut piece: Vec<DVec3> = Vec::new();
    let mut i = 0;
    while let Some(&p) = run.get(i) {
        piece.push(p);
        // The steep stretch starting here, all one way: where it ends and how far.
        let mut j = i;
        let mut total = 0.0;
        while let (Some(&a), Some(&b)) = (run.get(j), run.get(j + 1)) {
            let Some(f) = steep(a, b) else { break };
            if j > i && (f > 0.0) != (total > 0.0) {
                break;
            }
            total += f;
            j += 1;
        }
        if total.abs() > max_cut_step {
            report.wall_breaks += 1;
            out.push(std::mem::take(&mut piece));
            i = j;
        } else {
            i += 1;
        }
    }
    out.push(piece);
    out
}

/// Refinement stops once a span is this many tolerances wide.
///
/// Below it, the surface is discontinuous rather than merely curved, and further
/// splitting cannot help.
const GEOMETRIC_LIMIT: f64 = 0.5;

/// One line of a sweep being sampled: how a point along it is probed, the limits
/// it is refined to, and where its samples and counts go.
struct Line<'s, F> {
    /// The tool's tip on the surface at a distance along the line, if anything is
    /// under it there.
    probe: F,
    params: &'s ParallelParams,
    out: &'s mut Vec<Option<DVec3>>,
    report: &'s mut ParallelReport,
}

impl<F: FnMut(f64) -> Option<DVec3>> Line<'_, F> {
    /// Sample the line from `a` to `b`.
    fn sweep(&mut self, a: f64, b: f64) {
        let first = (self.probe)(a);
        self.out.push(first);
        self.sweep_on(first, a, b);
    }

    /// Sample the line from `a` to `b`, whose sample at `a`, `first`, is already
    /// out.
    fn sweep_on(&mut self, first: Option<DVec3>, a: f64, b: f64) {
        // Coarse seeding at the stepover, then adaptive refinement between seeds. The
        // seed spacing bounds how small a feature can be missed entirely.
        let steps = bounded_count(((b - a).abs() / self.params.stepover).ceil());
        let mut prev = first;

        for k in 1..=steps {
            let t = f64::from(k) / f64::from(steps);
            let u = a + (b - a) * t;
            let next = (self.probe)(u);
            if let (Some(p0), Some(p1)) = (prev, next) {
                let u0 = a + (b - a) * (f64::from(k - 1) / f64::from(steps));
                self.refine(u0, p0, u, p1, 0);
            }
            self.out.push(next);
            prev = next;
        }
    }

    /// Split a span while its midpoint deviates from the chord by more than tolerance.
    ///
    /// Emits interior points only; the caller supplies both endpoints.
    fn refine(&mut self, u0: f64, p0: DVec3, u1: f64, p1: DVec3, depth: u32) {
        let params = self.params;
        let um = (u0 + u1) / 2.0;
        let Some(pm) = (self.probe)(um) else {
            // Coverage ends *inside* this span. Returning quietly would leave the two
            // endpoints adjacent in the output and joined by a cut, across a region
            // where the surface was never established -- which is how a 17 mm step got
            // traversed at cutting feed on a scanline grazing the part edge. Break the
            // run instead; the linker turns it into a retract and a re-entry.
            self.report.coverage_breaks += 1;
            self.out.push(None);
            return;
        };
        let chord = (p0 + p1) * 0.5;
        if (pm.z - chord.z).abs() <= params.tolerance {
            return;
        }
        // A span narrower than this cannot be meaningfully split further; continuing
        // would just burn depth against a step the chord can never follow.
        if (u1 - u0).abs() <= params.tolerance * GEOMETRIC_LIMIT {
            self.report.discontinuities += 1;
            // If the surface drops far enough here that joining the two sides would
            // mean plunging at cutting feed, break the pass. `None` is the same
            // marker a coverage gap uses, so the split and the linker's retract come
            // out of machinery that already exists.
            if (p0.z - p1.z).abs() > params.max_cut_step {
                self.report.wall_breaks += 1;
                self.out.push(None);
            }
            return;
        }
        if depth >= params.max_depth {
            self.report.refinement_limited += 1;
            return;
        }
        self.refine(u0, p0, um, pm, depth + 1);
        self.out.push(Some(pm));
        self.refine(um, pm, u1, p1, depth + 1);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests;
