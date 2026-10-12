use super::*;
use crate::cutter::tool::{Ball, Flat};
use crate::path::toolpath::MoveRole;
use camcraft_geom::synthetic;

#[test]
fn a_long_line_sampled_in_stretches_is_the_line_sampled_whole() {
    let mesh = synthetic::uv_sphere(10.0, 40, 60);
    let cutter = DropCutter::new(&mesh);
    let tool = Ball::new(2.0).unwrap();
    // A spiral of many short segments, out past the sphere's edge.
    let points: Vec<DVec2> = (0..3000)
        .map(|i| {
            let a = f64::from(i) * 0.02;
            dvec2(a.cos(), a.sin()) * (0.5 + a * 0.3)
        })
        .collect();
    let (mut whole, mut parts) = (ParallelReport::default(), ParallelReport::default());
    let one = sweep_polyline(&cutter, &tool, &points, false, &params(), &mut whole);
    let many = sweep_open_polyline(&cutter, &tool, &points, &params(), &mut parts);
    assert!(!one.is_empty());
    assert_eq!(one.len(), many.len());
    for (a, b) in one.iter().zip(&many) {
        assert_eq!(a.points.len(), b.points.len());
        for (p, q) in a.points.iter().zip(&b.points) {
            assert!(p.distance(*q) < 1e-9, "{p} {q}");
        }
    }
}

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
    let (_, report) = parallel_finish(&dc, &tool, dvec2(-8.0, -8.0), dvec2(8.0, 8.0), &p).unwrap();
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
    let (_, report) = parallel_finish(&dc, &tool, dvec2(-4.0, -4.0), dvec2(4.0, 4.0), &p).unwrap();
    assert!(
        !report.is_clean(),
        "an impossible tolerance must be reported, not silently ignored"
    );
}
