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

use crate::dropcutter::{DropCutter, DropTest};
use crate::strategy::{StrategyError, check};
use crate::toolpath::{MoveRole, PLUNGE_RATIO, Toolpath};

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
pub fn parallel_finish<T: DropTest>(
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
pub fn parallel_finish_within<T: DropTest>(
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

    let mut path = Toolpath::new();
    let mut report = ParallelReport::default();
    let mut samples = Vec::new();

    for i in 0..lines {
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

        for (p0, p1) in pieces {
            let (a, b) = if reversed { (p1, p0) } else { (p0, p1) };
            samples.clear();
            let probe = |u: f64| -> Option<DVec3> {
                let p = along * u + across * v;
                cutter.drop_at(tool, p.x, p.y).map(|z| dvec3(p.x, p.y, z))
            };
            Line {
                probe,
                params,
                out: &mut samples,
                report: &mut report,
            }
            .sweep(a, b);

            let runs: Vec<Vec<DVec3>> = samples
                .split(|p| p.is_none())
                .flat_map(|run| {
                    let run: Vec<DVec3> = run.iter().filter_map(|p| *p).collect();
                    break_steep(run, params.max_cut_step, &mut report)
                })
                .collect();
            for run in runs {
                if run.len() < 2 {
                    continue;
                }
                report.passes += 1;
                let mut first = true;
                for p in run {
                    if first {
                        // The strategy does not plan traverses; the linker does.
                        path.feed_to(p, MoveRole::Link, params.feed)?;
                        first = false;
                    } else {
                        path.feed_to(p, MoveRole::Cut, params.feed)?;
                    }
                }
            }
        }
    }

    Ok((path, report))
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

impl<F: Fn(f64) -> Option<DVec3>> Line<'_, F> {
    /// Sample the line from `a` to `b`.
    fn sweep(&mut self, a: f64, b: f64) {
        // Coarse seeding at the stepover, then adaptive refinement between seeds. The
        // seed spacing bounds how small a feature can be missed entirely.
        let steps = bounded_count(((b - a).abs() / self.params.stepover).ceil());
        let mut prev = (self.probe)(a);
        self.out.push(prev);

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
mod tests {
    use super::*;
    use crate::tool::{Ball, Flat};
    use camcraft_geom::synthetic;

    fn params() -> ParallelParams {
        ParallelParams {
            stepover: 1.0,
            tolerance: 0.01,
            ..Default::default()
        }
    }

    #[test]
    fn rejects_unusable_parameters() {
        let mesh = synthetic::plane(0.0, 10.0);
        let dc = DropCutter::new(&mesh);
        let tool = Flat::new(1.0).unwrap();
        let (lo, hi) = (dvec2(-5.0, -5.0), dvec2(5.0, 5.0));

        for bad in [
            ParallelParams {
                stepover: 0.0,
                ..params()
            },
            ParallelParams {
                tolerance: -1.0,
                ..params()
            },
            ParallelParams {
                feed: f64::NAN,
                ..params()
            },
            ParallelParams {
                angle: f64::INFINITY,
                ..params()
            },
        ] {
            assert!(parallel_finish(&dc, &tool, lo, hi, &bad).is_err());
        }
        assert!(matches!(
            parallel_finish(&dc, &tool, hi, lo, &params()),
            Err(StrategyError::EmptyRegion { .. })
        ));
    }

    /// A flat surface needs no refinement at all: the chord is the surface.
    #[test]
    fn a_plane_needs_no_refinement() {
        let mesh = synthetic::plane(2.0, 20.0);
        let dc = DropCutter::new(&mesh);
        let tool = Flat::new(2.0).unwrap();
        let (path, report) =
            parallel_finish(&dc, &tool, dvec2(-8.0, -8.0), dvec2(8.0, 8.0), &params()).unwrap();

        assert!(report.is_clean());
        assert!(
            report.passes > 5,
            "expected several passes, got {}",
            report.passes
        );
        for m in path.moves() {
            assert!((m.to.z - 2.0).abs() < 1e-9, "off the plane: {:?}", m.to);
        }
    }

    /// Curvature must actually trigger refinement, or the adaptive logic is dead
    /// code that happens not to fail.
    ///
    /// The expected factor is not arbitrary. Over a span `h` on a CL surface of
    /// radius `R` the sag is about `h^2 / 8R`; here that is ~0.011 mm per 1 mm span
    /// against a 0.001 mm tolerance, which needs two levels of splitting and so
    /// roughly four times the points.
    #[test]
    fn curvature_produces_more_points_than_a_plane() {
        let p = ParallelParams {
            tolerance: 0.001,
            ..params()
        };
        let flat_mesh = synthetic::plane(0.0, 20.0);
        let dome_mesh = synthetic::uv_sphere(10.0, 240, 320);
        let tool = Ball::new(1.5).unwrap();
        let (lo, hi) = (dvec2(-6.0, -6.0), dvec2(6.0, 6.0));

        let flat = parallel_finish(&DropCutter::new(&flat_mesh), &tool, lo, hi, &p)
            .unwrap()
            .0;
        let dome = parallel_finish(&DropCutter::new(&dome_mesh), &tool, lo, hi, &p)
            .unwrap()
            .0;

        assert!(
            dome.len() > flat.len() * 3,
            "a curved surface must refine repeatedly: dome {} vs plane {}",
            dome.len(),
            flat.len()
        );
    }

    /// The reason the path is sampled exactly rather than interpolated.
    #[test]
    fn every_point_sits_on_the_exact_surface() {
        let mesh = synthetic::uv_sphere(10.0, 120, 160);
        let dc = DropCutter::new(&mesh);
        let tool = Ball::new(2.0).unwrap();
        let (path, _) =
            parallel_finish(&dc, &tool, dvec2(-5.0, -5.0), dvec2(5.0, 5.0), &params()).unwrap();

        assert!(!path.is_empty());
        for m in path.moves() {
            let exact = dc.drop_at(&tool, m.to.x, m.to.y).unwrap();
            assert!(
                (m.to.z - exact).abs() < 1e-9,
                "point {:?} is not on the exact surface (want {exact})",
                m.to
            );
        }
    }

    /// Chord deviation must respect the tolerance. Deliberately measured *away* from
    /// the midpoint: the refinement criterion tests the midpoint, so a test that also
    /// only looked there would just be restating the implementation to itself.
    #[test]
    fn chords_stay_within_tolerance_away_from_the_midpoint() {
        let (sr, lat, lon) = (10.0, 240, 320);
        let mesh = synthetic::uv_sphere(sr, lat, lon);
        let dc = DropCutter::new(&mesh);
        let tool = Ball::new(2.0).unwrap();
        let p = ParallelParams {
            stepover: 0.8,
            tolerance: 0.005,
            ..Default::default()
        };
        let (path, report) =
            parallel_finish(&dc, &tool, dvec2(-6.0, -6.0), dvec2(6.0, 6.0), &p).unwrap();
        assert!(report.is_clean(), "refinement hit its limit: {report:?}");

        let budget = p.tolerance + synthetic::uv_sphere_max_sag(sr, lat, lon) + 1e-9;
        let mut checked = 0;
        let mut worst: f64 = 0.0;
        for (prev, cur) in path.moves().iter().zip(path.moves().iter().skip(1)) {
            if cur.role != MoveRole::Cut {
                continue;
            }
            let (a, b) = (prev.to, cur.to);
            for frac in [0.2, 0.35, 0.65, 0.8] {
                let q = a + (b - a) * frac;
                if let Some(exact) = dc.drop_at(&tool, q.x, q.y) {
                    let sag = (q.z - exact).abs();
                    worst = worst.max(sag);
                    assert!(
                        sag <= budget,
                        "chord sags {sag} at t={frac}, budget {budget}"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 500, "expected real coverage, checked {checked}");
        assert!(worst > 0.0, "a curved surface must deviate somewhere");
    }

    /// The strategy plans cuts, not traverses. Every pass must begin with a link for
    /// the linker to resolve, and the path must admit it is unfinished.
    #[test]
    fn passes_begin_with_unresolved_links() {
        let mesh = synthetic::plane(0.0, 20.0);
        let dc = DropCutter::new(&mesh);
        let tool = Flat::new(2.0).unwrap();
        let (path, report) =
            parallel_finish(&dc, &tool, dvec2(-6.0, -6.0), dvec2(6.0, 6.0), &params()).unwrap();

        assert!(
            path.has_unplanned_links(),
            "strategy output is not machinable yet"
        );
        let links = path
            .moves()
            .iter()
            .filter(|m| m.role == MoveRole::Link)
            .count();
        assert_eq!(
            links as u64, report.passes,
            "exactly one link per pass start"
        );
        assert!(
            path.moves().iter().all(|m| m.role != MoveRole::Rapid),
            "a strategy must not invent rapids"
        );
    }

    /// Where the part does not reach, the pass stops rather than inventing geometry.
    #[test]
    fn coverage_gaps_split_passes() {
        // A small plane inside a much larger swept region.
        let mesh = synthetic::plane(1.0, 3.0);
        let dc = DropCutter::new(&mesh);
        let tool = Flat::new(0.5).unwrap();
        let (path, report) = parallel_finish(
            &dc,
            &tool,
            dvec2(-10.0, -10.0),
            dvec2(10.0, 10.0),
            &params(),
        )
        .unwrap();

        assert!(report.passes > 0);
        for m in path.moves() {
            assert!(
                m.to.x.abs() <= 3.6 && m.to.y.abs() <= 3.6,
                "point {:?} is outside the part",
                m.to
            );
        }
    }

    /// The defect found by inspecting real output: crossing a wall produced cutting
    /// moves that descend far faster than they advance, which is a full-depth plunge
    /// at cutting feed. The pass must break instead.
    ///
    /// Stated comparatively rather than as "no steep descents at all", because some
    /// steep descent near a wall is legitimate: a ball nose rolling over the top
    /// edge traces a genuinely steep but perfectly smooth arc, and breaking there
    /// would be wrong. The guarantee is that the *step itself* is no longer cut
    /// through, which is what the before-and-after shows.
    #[test]
    fn a_wall_breaks_the_pass_instead_of_plunging_through_it() {
        let step_height = 20.0;
        let mesh = synthetic::step(step_height, 30.0);
        let dc = DropCutter::new(&mesh);
        let tool = Ball::new(3.0).unwrap();
        let base = ParallelParams {
            stepover: 1.0,
            tolerance: 0.02,
            ..Default::default()
        };
        let (lo, hi) = (dvec2(-10.0, -6.0), dvec2(10.0, 6.0));

        // Allowance larger than the step: nothing breaks, and the wall gets cut.
        let through = ParallelParams {
            max_cut_step: 50.0,
            ..base
        };
        let (before, before_report) = parallel_finish(&dc, &tool, lo, hi, &through).unwrap();
        let b_start = before.moves().first().unwrap().to;
        let (_, b_deep) = before.steep_cut_descents(b_start, PLUNGE_RATIO);
        assert_eq!(before_report.wall_breaks, 0);
        assert!(
            b_deep > step_height * 0.5,
            "without breaking, the pass should dive most of the step; got {b_deep}"
        );

        // Default allowance: the step is no longer traversed as a cut.
        let (after, after_report) = parallel_finish(&dc, &tool, lo, hi, &base).unwrap();
        let a_start = after.moves().first().unwrap().to;
        let (_, a_deep) = after.steep_cut_descents(a_start, PLUNGE_RATIO);
        assert!(
            after_report.wall_breaks > 0,
            "the step must break the passes"
        );
        assert!(
            a_deep <= base.max_cut_step,
            "deepest cutting descent {a_deep} exceeds the {} mm allowance",
            base.max_cut_step
        );
        assert!(
            a_deep < b_deep / 10.0,
            "breaking should cut the worst descent by an order of magnitude: {b_deep} -> {a_deep}"
        );
    }

    /// The gap this closes: a steep face that is *straight* passes the chord test
    /// at once, so refinement never looks at its depth, and a pass rode down it at
    /// cutting feed. Measured as the gate measures, as one continuous descent, the
    /// pass must now break, and nothing left may plunge further than allowed.
    #[test]
    fn a_straight_steep_face_breaks_the_pass() {
        // A flat top at 10 for x < 0, a straight 20:1 ramp (about 87 degrees) down
        // to a floor at 0 by x = 0.5, and the floor beyond.
        let y = 10.0;
        let pts = [(-10.0, 10.0), (0.0, 10.0), (0.5, 0.0), (10.0, 0.0)];
        let mut vertices = Vec::new();
        for &(x, z) in &pts {
            vertices.push(dvec3(x, -y, z));
            vertices.push(dvec3(x, y, z));
        }
        let triangles = (0..3u32)
            .flat_map(|k| {
                let (a, b, c, d) = (2 * k, 2 * k + 1, 2 * k + 2, 2 * k + 3);
                [[a, c, d], [a, d, b]]
            })
            .collect();
        let mesh = camcraft_geom::mesh::TriMesh::new(vertices, triangles).unwrap();
        let dc = DropCutter::new(&mesh);
        let tool = Ball::new(2.0).unwrap();
        let (path, report) =
            parallel_finish(&dc, &tool, dvec2(-6.0, -2.0), dvec2(6.0, 2.0), &params()).unwrap();
        assert!(report.wall_breaks > 0, "the ramp must break the passes");
        let cut = |f: fn(f64) -> bool| {
            path.moves()
                .iter()
                .any(|m| m.role == MoveRole::Cut && f(m.to.x))
        };
        assert!(
            cut(|x| x < -3.0) && cut(|x| x > 4.0),
            "both flats are still cut"
        );
        let start = path.moves().first().unwrap().to;
        let (_, deepest) = path.steep_cut_descents(start, PLUNGE_RATIO);
        assert!(
            deepest <= params().max_cut_step,
            "a {deepest} mm descent is left, past the {} mm allowance",
            params().max_cut_step
        );
    }

    /// A steep face that is not a plunge — 3:1, as steep as the router jig gets —
    /// is ridden, not broken.
    #[test]
    fn a_steep_but_cuttable_face_is_ridden() {
        let mesh = synthetic::inclined_plane(-3.0, 0.0, 0.0, 10.0);
        let dc = DropCutter::new(&mesh);
        let tool = Ball::new(2.0).unwrap();
        let (_, report) =
            parallel_finish(&dc, &tool, dvec2(-2.0, -2.0), dvec2(2.0, 2.0), &params()).unwrap();
        assert_eq!(report.wall_breaks, 0);
    }

    /// Breaking must not be indiscriminate. A smooth dome has no discontinuity, so
    /// a pass across it should stay in one piece.
    #[test]
    fn a_smooth_surface_is_not_broken_up() {
        let mesh = synthetic::uv_sphere(20.0, 200, 300);
        let dc = DropCutter::new(&mesh);
        let tool = Ball::new(2.0).unwrap();
        let p = ParallelParams {
            stepover: 1.0,
            tolerance: 0.02,
            ..Default::default()
        };
        let (_, report) =
            parallel_finish(&dc, &tool, dvec2(-8.0, -8.0), dvec2(8.0, 8.0), &p).unwrap();
        assert_eq!(report.wall_breaks, 0, "a dome has no walls to break at");
    }

    /// The threshold is the control: a generous one tolerates the step, a tight one
    /// does not. If both behaved alike the parameter would be decoration.
    #[test]
    fn the_threshold_actually_governs_breaking() {
        let mesh = synthetic::step(20.0, 30.0);
        let dc = DropCutter::new(&mesh);
        let tool = Ball::new(3.0).unwrap();
        let base = ParallelParams {
            stepover: 1.0,
            tolerance: 0.02,
            ..Default::default()
        };

        let tight = parallel_finish(
            &dc,
            &tool,
            dvec2(-10.0, -6.0),
            dvec2(10.0, 6.0),
            &ParallelParams {
                max_cut_step: 0.1,
                ..base
            },
        )
        .unwrap()
        .1;
        let loose = parallel_finish(
            &dc,
            &tool,
            dvec2(-10.0, -6.0),
            dvec2(10.0, 6.0),
            &ParallelParams {
                max_cut_step: 50.0,
                ..base
            },
        )
        .unwrap()
        .1;

        assert!(tight.wall_breaks > 0);
        assert_eq!(
            loose.wall_breaks, 0,
            "a 50 mm allowance should tolerate a 20 mm step"
        );
    }

    /// A bounded algorithm that reports being clamped beats one that spins.
    #[test]
    fn refinement_limit_is_reported_not_hidden() {
        let mesh = synthetic::uv_sphere(10.0, 200, 300);
        let dc = DropCutter::new(&mesh);
        let tool = Ball::new(2.0).unwrap();
        let p = ParallelParams {
            stepover: 4.0,
            tolerance: 1e-9,
            max_depth: 1,
            ..Default::default()
        };
        let (_, report) =
            parallel_finish(&dc, &tool, dvec2(-4.0, -4.0), dvec2(4.0, 4.0), &p).unwrap();
        assert!(
            !report.is_clean(),
            "an impossible tolerance must be reported, not silently ignored"
        );
    }
}
