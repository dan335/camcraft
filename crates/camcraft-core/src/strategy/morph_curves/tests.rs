use camcraft_geom::synthetic;
use glam::{DVec3, dvec2, dvec3};

use super::*;
use crate::cutter::allowance::{Allowance, EffectiveTool};
use crate::cutter::dropcutter::DropCutter;
use crate::cutter::surfaces::PartSurfaces;
use crate::cutter::tool::Ball;
use crate::path::toolpath::Move;
use crate::strategy::curve::Piece;

/// A ball of radius `r`, leaving nothing.
fn ball(r: f64) -> EffectiveTool {
    EffectiveTool::new(&Ball::new(r).unwrap(), Allowance::none()).unwrap()
}

/// A curve through `points`, each joined to the next by a line; closed back
/// to the first where `closed`.
fn through(points: &[DVec3], closed: bool) -> Picked {
    let mut pieces: Vec<Piece> = points
        .iter()
        .skip(1)
        .map(|&to| Piece::Line { to })
        .collect();
    if closed {
        pieces.push(Piece::Line { to: points[0] });
    }
    Picked {
        start: points[0],
        pieces,
    }
}

/// A square `size` across about (`x`, `y`), anticlockwise from its lower
/// left corner.
fn square(x: f64, y: f64, size: f64) -> Picked {
    let h = size / 2.0;
    through(
        &[
            dvec3(x - h, y - h, 0.0),
            dvec3(x + h, y - h, 0.0),
            dvec3(x + h, y + h, 0.0),
            dvec3(x - h, y + h, 0.0),
        ],
        true,
    )
}

/// A line along X at `y`, from `x0` to `x1`.
fn line(y: f64, x0: f64, x1: f64) -> Picked {
    through(&[dvec3(x0, y, 0.0), dvec3(x1, y, 0.0)], false)
}

fn params(way: Way, stepover: f64) -> MorphCurvesParams {
    MorphCurvesParams {
        drop: ProjectParams {
            stepover,
            tolerance: 0.01,
            ..Default::default()
        },
        way,
    }
}

/// The passes between `curves` over `mesh` with a ball 2 across, each its
/// points, and what was found.
fn cut(
    mesh: &camcraft_geom::mesh::TriMesh,
    curves: &[Picked],
    p: &MorphCurvesParams,
) -> (Vec<Vec<DVec3>>, MorphCurvesReport) {
    let part = PartSurfaces::whole(DropCutter::new(mesh));
    let seat = part.under(&ball(1.0)).unwrap();
    let area = Region::rectangle(dvec2(-18.0, -18.0), dvec2(18.0, 18.0));
    let (path, report) = morph_curves(&seat, &area, curves, p).unwrap();
    let passes: Vec<Vec<DVec3>> = path
        .passes()
        .map(|pass| pass.iter().map(|m: &Move| m.to).collect())
        .collect();
    assert_eq!(report.cut.passes, passes.len() as u64, "{report:?}");
    (passes, report)
}

/// The area a pass's points close, seen from above: positive anticlockwise.
fn turn(points: &[DVec3]) -> f64 {
    let mut twice = 0.0;
    for (a, b) in points.iter().zip(points.iter().cycle().skip(1)) {
        twice += a.truncate().perp_dot(b.truncate());
    }
    twice / 2.0
}

/// Between two open lines of different lengths, each pass is the same step
/// of the way from every point of the one to its match on the other: its
/// ends a step of the way between theirs. There are as many steps as keep
/// matched points a stepover apart, as Fusion's lines across the floor of
/// `ops_3d` are, and they are cut where the tool rests.
#[test]
fn passes_between_two_lines_end_a_step_of_the_way_between_their_ends() {
    let mesh = synthetic::plane(1.0, 20.0);
    let curves = [line(-1.0, -5.0, 5.0), line(-7.0, -5.0, 3.0)];
    let (passes, report) = cut(&mesh, &curves, &params(Way::OneWay, 0.5));
    // 6 mm apart at their starts, √40 at their ends: 13 steps.
    assert_eq!(report.laid, 14, "{report:?}");
    assert!(
        (report.widest - 40.0_f64.sqrt() / 13.0).abs() < 1e-9,
        "{report:?}"
    );
    assert_eq!(passes.len(), 14);
    for (k, pass) in passes.iter().enumerate() {
        // From the line at Y-1, the nearer the work zero.
        let t = k as f64 / 13.0;
        let start = dvec2(-5.0, -1.0).lerp(dvec2(-5.0, -7.0), t);
        let end = dvec2(5.0, -1.0).lerp(dvec2(3.0, -7.0), t);
        let (a, b) = (pass[0], *pass.last().unwrap());
        assert!(a.truncate().distance(start) < 1e-9, "{k}: {a}");
        assert!(b.truncate().distance(end) < 1e-9, "{k}: {b}");
        // The ball's tip on the plane.
        assert!(pass.iter().all(|p| (p.z - 1.0).abs() < 1e-9), "{pass:?}");
    }
}

/// A second curve picked running the other way is turned round to run as the
/// first does: Fusion's Morph gives no passes at all for two that run
/// opposite ways, and an edge picked in camcraft runs the way it does.
#[test]
fn a_second_curve_running_the_other_way_is_turned_round() {
    let mesh = synthetic::plane(0.0, 20.0);
    let p = params(Way::OneWay, 0.5);
    let same = cut(&mesh, &[line(-1.0, -5.0, 5.0), line(-7.0, -5.0, 3.0)], &p);
    let opposite = cut(&mesh, &[line(-1.0, -5.0, 5.0), line(-7.0, 3.0, -5.0)], &p);
    assert_eq!(same, opposite);
}

/// The passes start at the curve nearer the work zero, whichever was picked
/// first; one way runs each as the first picked runs, the other way the
/// reverse, and both ways each the other way from the one before, the first
/// from its end nearer the work zero.
#[test]
fn passes_start_at_the_curve_nearer_the_work_zero_and_run_the_way_asked() {
    let mesh = synthetic::plane(0.0, 20.0);
    let far = line(9.0, -8.0, 2.0);
    // Nearer the work zero at its end, (2, 2).
    let near = line(2.0, -8.0, 2.0);
    let curves = [far, near];
    let way = |w: Way| cut(&mesh, &curves, &params(w, 1.0)).0;
    for passes in [way(Way::OneWay), way(Way::OtherWay), way(Way::BothWays)] {
        assert_eq!(passes.len(), 8);
        assert!((passes[0][0].y - 2.0).abs() < 1e-9, "{:?}", passes[0]);
        assert!((passes[7][0].y - 9.0).abs() < 1e-9, "{:?}", passes[7]);
    }
    let rising = |pass: &Vec<DVec3>| pass.last().unwrap().x > pass[0].x;
    assert!(way(Way::OneWay).iter().all(rising));
    assert!(!way(Way::OtherWay).iter().any(rising));
    let both = way(Way::BothWays);
    for (i, pass) in both.iter().enumerate() {
        // The first from (2, 2), its end nearer the work zero.
        assert_eq!(rising(pass), i % 2 == 1, "{i}: {pass:?}");
    }
}

/// Between two closed curves both run clockwise, the second started where
/// the matched points are closest on the whole: between two squares about
/// the work zero, corner with corner, every pass a square between them,
/// however each was picked and wherever it starts. Both ways, the first is
/// cut anticlockwise and each after it the other way from the one before.
#[test]
fn passes_between_two_squares_are_squares() {
    let mesh = synthetic::plane(0.0, 20.0);
    let inner = square(0.0, 0.0, 4.0);
    // Picked clockwise, from its upper right corner.
    let outer = through(
        &[
            dvec3(6.0, 6.0, 0.0),
            dvec3(6.0, -6.0, 0.0),
            dvec3(-6.0, -6.0, 0.0),
            dvec3(-6.0, 6.0, 0.0),
        ],
        true,
    );
    let (passes, report) = cut(&mesh, &[outer, inner], &params(Way::OneWay, 1.0));
    // Corners 4 √2 apart: 6 steps.
    assert_eq!(report.laid, 7, "{report:?}");
    assert_eq!(passes.len(), 7);
    for (k, pass) in passes.iter().enumerate() {
        // From the inner square, nearer the work zero.
        let half = 2.0 + 4.0 * k as f64 / 6.0;
        assert!(turn(pass) < 0.0, "{k}: {pass:?}");
        for p in pass {
            let edge = p.x.abs().max(p.y.abs());
            assert!((edge - half).abs() < 1e-9, "{k}: {p}");
        }
        assert!((turn(pass) + 4.0 * half * half).abs() < 1e-6, "{k}");
    }
    let (both, _) = cut(
        &mesh,
        &[square(0.0, 0.0, 12.0), square(0.0, 0.0, 4.0)],
        &params(Way::BothWays, 1.0),
    );
    for (i, pass) in both.iter().enumerate() {
        assert_eq!(turn(pass) > 0.0, i % 2 == 0, "{i}");
    }
}

/// Two curves are morphed between, open or closed alike: anything else is
/// refused.
#[test]
fn it_needs_two_open_curves_or_two_closed() {
    let mesh = synthetic::plane(0.0, 20.0);
    let part = PartSurfaces::whole(DropCutter::new(&mesh));
    let seat = part.under(&ball(1.0)).unwrap();
    let area = Region::rectangle(dvec2(-18.0, -18.0), dvec2(18.0, 18.0));
    let p = params(Way::BothWays, 1.0);
    let try_with = |curves: &[Picked]| morph_curves(&seat, &area, curves, &p).err();
    assert_eq!(
        try_with(&[line(0.0, -1.0, 1.0)]),
        Some(MorphError::NotTwo(1))
    );
    let three = [
        line(0.0, -1.0, 1.0),
        line(2.0, -1.0, 1.0),
        line(4.0, -1.0, 1.0),
    ];
    assert_eq!(try_with(&three), Some(MorphError::NotTwo(3)));
    assert_eq!(
        try_with(&[line(0.0, -1.0, 1.0), square(0.0, 0.0, 4.0)]),
        Some(MorphError::OpenAndClosed)
    );
}

#[test]
fn is_deterministic() {
    let mesh = synthetic::uv_sphere(6.0, 40, 60);
    let curves = [square(-1.0, 0.5, 3.0), square(0.0, 0.0, 11.0)];
    let p = params(Way::BothWays, 0.4);
    assert_eq!(cut(&mesh, &curves, &p), cut(&mesh, &curves, &p));
}
