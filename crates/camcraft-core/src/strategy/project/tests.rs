use camcraft_geom::synthetic;
use glam::{DVec3, dvec2, dvec3};

use super::*;
use crate::cutter::tool::Ball;
use crate::path::toolpath::Move;
use crate::strategy::curve::Piece;

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

/// A square 8 across about the origin at `z`, anticlockwise from its lower
/// left corner.
fn square(z: f64) -> Picked {
    through(
        &[
            dvec3(-4.0, -4.0, z),
            dvec3(4.0, -4.0, z),
            dvec3(4.0, 4.0, z),
            dvec3(-4.0, 4.0, z),
        ],
        true,
    )
}

fn params(way: Way, raise: f64) -> ProjectCurvesParams {
    ProjectCurvesParams {
        drop: ProjectParams {
            stepover: 0.5,
            tolerance: 0.01,
            raise,
            ..Default::default()
        },
        way,
    }
}

/// The passes `curves` make over `mesh` with a ball 2 across, each its
/// points.
fn cut(
    mesh: &camcraft_geom::mesh::TriMesh,
    curves: &[Picked],
    p: &ProjectCurvesParams,
) -> Vec<Vec<DVec3>> {
    let cutter = DropCutter::new(mesh);
    let tool = Ball::new(1.0).unwrap();
    let area = Region::rectangle(dvec2(-9.0, -9.0), dvec2(9.0, 9.0));
    let (path, report) = project_curves(&cutter, &tool, &area, curves, p).unwrap();
    let passes: Vec<Vec<DVec3>> = path
        .passes()
        .map(|pass| pass.iter().map(|m: &Move| m.to).collect())
        .collect();
    assert_eq!(report.passes, passes.len() as u64, "{report:?}");
    passes
}

/// The area a pass's points close, seen from above: positive anticlockwise.
fn turn(points: &[DVec3]) -> f64 {
    let mut twice = 0.0;
    for (a, b) in points.iter().zip(points.iter().cycle().skip(1)) {
        twice += a.truncate().perp_dot(b.truncate());
    }
    twice / 2.0
}

/// A curve lies wherever it lies: its points are dropped onto the part, each
/// where the tool comes to rest over it, and raised by the axial offset.
#[test]
fn a_curve_is_cut_where_the_tool_rests_on_the_part() {
    let mesh = synthetic::inclined_plane(0.2, 0.1, 3.0, 10.0);
    let cutter = DropCutter::new(&mesh);
    let tool = Ball::new(1.0).unwrap();
    // Picked well above the part: only where it is seen from above counts.
    let line = through(&[dvec3(-5.0, -3.0, 40.0), dvec3(5.0, 4.0, 40.0)], false);
    for raise in [0.0, 1.5] {
        let passes = cut(
            &mesh,
            std::slice::from_ref(&line),
            &params(Way::OneWay, raise),
        );
        assert_eq!(passes.len(), 1);
        for p in &passes[0] {
            let rest = cutter.drop_at(&tool, p.x, p.y).unwrap();
            assert!((p.z - raise - rest).abs() < 1e-9, "{p} rests at {rest}");
        }
        let (first, last) = (passes[0][0], *passes[0].last().unwrap());
        assert!(
            first.truncate().distance(dvec2(-5.0, -3.0)) < 1e-9,
            "{first}"
        );
        assert!(last.truncate().distance(dvec2(5.0, 4.0)) < 1e-9, "{last}");
    }
}

/// One way runs a closed curve clockwise, whichever way it was picked, and
/// an open one as picked; the other way, the reverse of both.
#[test]
fn one_way_is_clockwise_or_as_picked_and_the_other_way_the_reverse() {
    let mesh = synthetic::plane(0.0, 10.0);
    let line = through(&[dvec3(-6.0, 6.0, 0.0), dvec3(6.0, 6.0, 0.0)], false);
    for picked in [square(0.0), square(0.0).reversed()] {
        let curves = [picked, line.clone()];
        let one = cut(&mesh, &curves, &params(Way::OneWay, 0.0));
        assert!(turn(&one[0]) < -60.0, "{}", turn(&one[0]));
        assert!(
            one[1][0].x < 0.0 && one[1].last().unwrap().x > 0.0,
            "{:?}",
            one[1]
        );
        let other = cut(&mesh, &curves, &params(Way::OtherWay, 0.0));
        assert!(turn(&other[0]) > 60.0, "{}", turn(&other[0]));
        assert!(
            other[1][0].x > 0.0 && other[1].last().unwrap().x < 0.0,
            "{:?}",
            other[1]
        );
    }
}

/// Both ways runs a closed curve clockwise, as one way does, but an open one
/// from whichever end is nearer where the last curve left the tool; and the
/// curves are cut in the order picked, each closed one started nearest where
/// the tool is, the first at its own start.
#[test]
fn both_ways_starts_an_open_curve_at_its_nearer_end() {
    let mesh = synthetic::plane(0.0, 10.0);
    // Picked from its far end: the square leaves the tool at its lower left.
    let line = through(&[dvec3(6.0, -6.0, 0.0), dvec3(-6.0, -6.0, 0.0)], false);
    let ring = through(
        &[
            dvec3(-5.0, -8.0, 0.0),
            dvec3(5.0, -8.0, 0.0),
            dvec3(0.0, -7.0, 0.0),
        ],
        true,
    );
    let passes = cut(
        &mesh,
        &[square(0.0), line, ring],
        &params(Way::BothWays, 0.0),
    );
    assert_eq!(passes.len(), 3);
    assert!(turn(&passes[0]) < 0.0);
    assert!(
        passes[0][0].truncate().distance(dvec2(-4.0, -4.0)) < 1e-9,
        "{:?}",
        passes[0][0]
    );
    assert!(
        passes[1][0].truncate().distance(dvec2(-6.0, -6.0)) < 1e-9,
        "{:?}",
        passes[1]
    );
    // The ring's corner nearest where the line ended, (6, -6).
    assert!(
        passes[2][0].truncate().distance(dvec2(5.0, -8.0)) < 1e-9,
        "{:?}",
        passes[2]
    );
}

#[test]
fn is_deterministic() {
    let mesh = synthetic::uv_sphere(6.0, 40, 60);
    let curves = [
        square(0.0),
        through(&[dvec3(-7.0, -1.0, 0.0), dvec3(7.0, 2.0, 0.0)], false),
    ];
    let p = params(Way::BothWays, 0.0);
    assert_eq!(cut(&mesh, &curves, &p), cut(&mesh, &curves, &p));
}
