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

use camcraft_geom::mesh::Aabb;
use glam::{DVec2, DVec3, dvec2, dvec3};

use crate::dropcutter::{DropCutter, DropTest};
use crate::toolpath::{MoveRole, Toolpath, ToolpathError};

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
}

impl Default for ParallelParams {
    fn default() -> Self {
        Self {
            stepover: 0.5,
            angle: 0.0,
            tolerance: 0.01,
            feed: 1000.0,
            max_depth: 12,
        }
    }
}

/// A strategy that could not produce a path.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum StrategyError {
    /// A parameter was outside its usable range.
    #[error("{name} must be finite and greater than zero, got {value}")]
    BadParameter {
        /// Which parameter.
        name: &'static str,
        /// What it was.
        value: f64,
    },
    /// The region was empty or inverted.
    #[error("region is empty or inverted: {min:?} to {max:?}")]
    EmptyRegion {
        /// Lower corner.
        min: DVec2,
        /// Upper corner.
        max: DVec2,
    },
    /// A move could not be added.
    #[error(transparent)]
    Toolpath(#[from] ToolpathError),
}

/// What happened while building a path, beyond the path itself.
///
/// Returned rather than logged, because silently emitting a path that hit its
/// refinement limit is exactly the failure this project refuses to have.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StrategyReport {
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
}

impl StrategyReport {
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
) -> Result<(Toolpath, StrategyReport), StrategyError> {
    check("stepover", params.stepover)?;
    check("tolerance", params.tolerance)?;
    check("feed", params.feed)?;
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
    let (v0, v1) = span(across);

    let lines = clamp_count(((v1 - v0) / params.stepover).floor() + 1.0);

    let mut path = Toolpath::new();
    let mut report = StrategyReport::default();
    let mut samples = Vec::new();

    for i in 0..lines {
        let v = v0 + f64::from(i) * params.stepover;
        // Zigzag: reverse alternate passes so the tool does not traverse back.
        let reversed = i % 2 == 1;
        let (a, b) = if reversed { (u1, u0) } else { (u0, u1) };

        samples.clear();
        sweep_line(
            cutter,
            tool,
            along,
            across,
            v,
            a,
            b,
            params,
            &mut samples,
            &mut report,
        );

        for run in samples.split(|p| p.is_none()) {
            let run: Vec<DVec3> = run.iter().filter_map(|p| *p).collect();
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

    Ok((path, report))
}

/// Convenience: sweep the whole XY extent of a mesh's bounds.
pub fn region_from_bounds(bounds: &Aabb, margin: f64) -> (DVec2, DVec2) {
    (
        dvec2(bounds.min.x - margin, bounds.min.y - margin),
        dvec2(bounds.max.x + margin, bounds.max.y + margin),
    )
}

/// Upper bound on passes or samples along one line.
///
/// A bound rather than a hope: a tiny stepover against a large region would
/// otherwise try to allocate an unbounded path. Staying inside `u32` also keeps
/// every later conversion to `f64` exact.
const MAX_STEPS: u32 = 1_000_000;

/// Refinement stops once a span is this many tolerances wide.
///
/// Below it, the surface is discontinuous rather than merely curved, and further
/// splitting cannot help.
const GEOMETRIC_LIMIT: f64 = 0.5;

/// Clamp a computed count into `[1, MAX_STEPS]`.
fn clamp_count(n: f64) -> u32 {
    if !n.is_finite() || n < 1.0 {
        return 1;
    }
    // Clamped and finite, so the narrowing is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = n.min(f64::from(MAX_STEPS)) as u32;
    n
}

fn check(name: &'static str, value: f64) -> Result<(), StrategyError> {
    (value.is_finite() && value > 0.0)
        .then_some(())
        .ok_or(StrategyError::BadParameter { name, value })
}

#[allow(clippy::too_many_arguments)]
fn sweep_line<T: DropTest>(
    cutter: &DropCutter<'_>,
    tool: &T,
    along: DVec2,
    across: DVec2,
    v: f64,
    a: f64,
    b: f64,
    params: &ParallelParams,
    out: &mut Vec<Option<DVec3>>,
    report: &mut StrategyReport,
) {
    let at = |u: f64| along * u + across * v;
    let probe = |u: f64| -> Option<DVec3> {
        let p = at(u);
        cutter.drop_at(tool, p.x, p.y).map(|z| dvec3(p.x, p.y, z))
    };

    // Coarse seeding at the stepover, then adaptive refinement between seeds. The
    // seed spacing bounds how small a feature can be missed entirely.
    let steps = clamp_count(((b - a).abs() / params.stepover).ceil());
    let mut prev = probe(a);
    out.push(prev);

    for k in 1..=steps {
        let t = f64::from(k) / f64::from(steps);
        let u = a + (b - a) * t;
        let next = probe(u);
        if let (Some(p0), Some(p1)) = (prev, next) {
            let u0 = a + (b - a) * (f64::from(k - 1) / f64::from(steps));
            refine(&probe, u0, p0, u, p1, params, 0, out, report);
        }
        out.push(next);
        prev = next;
    }
}

/// Split a span while its midpoint deviates from the chord by more than tolerance.
///
/// Emits interior points only; the caller supplies both endpoints.
#[allow(clippy::too_many_arguments)]
fn refine<F: Fn(f64) -> Option<DVec3>>(
    probe: &F,
    u0: f64,
    p0: DVec3,
    u1: f64,
    p1: DVec3,
    params: &ParallelParams,
    depth: u32,
    out: &mut Vec<Option<DVec3>>,
    report: &mut StrategyReport,
) {
    let um = (u0 + u1) / 2.0;
    let Some(pm) = probe(um) else {
        // Coverage ends inside this span; the seed loop records the gap.
        return;
    };
    let chord = (p0 + p1) * 0.5;
    if (pm.z - chord.z).abs() <= params.tolerance {
        return;
    }
    // A span narrower than this cannot be meaningfully split further; continuing
    // would just burn depth against a step the chord can never follow.
    if (u1 - u0).abs() <= params.tolerance * GEOMETRIC_LIMIT {
        report.discontinuities += 1;
        return;
    }
    if depth >= params.max_depth {
        report.refinement_limited += 1;
        return;
    }
    refine(probe, u0, p0, um, pm, params, depth + 1, out, report);
    out.push(Some(pm));
    refine(probe, um, pm, u1, p1, params, depth + 1, out, report);
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
