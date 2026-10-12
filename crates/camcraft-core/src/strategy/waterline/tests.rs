use super::*;
use crate::cutter::allowance::{Allowance, EffectiveTool};
use crate::cutter::tool::{Ball, Flat};
use crate::path::toolpath::MoveRole;
use camcraft_geom::mesh::TriMesh;
use camcraft_geom::synthetic;
use glam::DVec3;
use glam::dvec2;

const TOLERANCE: f64 = 0.02;

fn passes(path: &Toolpath) -> Vec<Vec<DVec3>> {
    let mut out: Vec<Vec<DVec3>> = Vec::new();
    for m in path.moves() {
        match m.role {
            MoveRole::Link => out.push(vec![m.to]),
            _ => {
                if let Some(p) = out.last_mut() {
                    p.push(m.to);
                }
            }
        }
    }
    out
}

/// A pass's level, if every point of it is at it.
fn at_level(run: &[DVec3]) -> Option<f64> {
    let z = run.first()?.z;
    run.iter().all(|p| (p.z - z).abs() < 1e-12).then_some(z)
}

fn run(
    mesh: &TriMesh,
    tool: &EffectiveTool,
    params: &WaterlineParams,
) -> (Toolpath, WaterlineReport) {
    let cutter = DropCutter::new(mesh);
    let mut reach = LevelReach::new(mesh, tool, params.tolerance).unwrap();
    let area = Region::rectangle(dvec2(-30.0, -30.0), dvec2(30.0, 30.0));
    waterline(&mut reach, &cutter, tool, &area, params).unwrap()
}

/// A 10 mm square boss, 5 mm tall.
fn boss() -> TriMesh {
    synthetic::cuboid(DVec3::new(-5.0, -5.0, 0.0), DVec3::new(5.0, 5.0, 5.0))
}

/// A sphere of radius 10 about the origin: a dome above the XY plane.
fn dome() -> TriMesh {
    synthetic::uv_sphere(10.0, 60, 90)
}

fn ball(r: f64) -> EffectiveTool {
    EffectiveTool::new(&Ball::new(r).unwrap(), Allowance::none()).unwrap()
}

fn flat(r: f64) -> EffectiveTool {
    EffectiveTool::new(&Flat::new(r).unwrap(), Allowance::none()).unwrap()
}

fn params(top: f64, bottom: f64, stepdown: f64) -> WaterlineParams {
    WaterlineParams {
        stepdown,
        top,
        bottom,
        tolerance: TOLERANCE,
        ..Default::default()
    }
}

#[test]
fn a_boss_has_a_ring_at_every_level_on_its_wall() {
    // A 10 mm square boss 5 mm tall on a plate.
    let mesh = boss();
    let tool = flat(1.5);
    let (path, report) = run(&mesh, &tool, &params(5.0, 0.0, 1.0));
    let passes = passes(&path);
    assert!(report.levels >= 5, "{report:?}");
    assert!(!passes.is_empty());
    for pass in &passes {
        let z = at_level(pass).expect("every pass at its level");
        assert!((0.0..=5.0 + 1e-9).contains(&z));
        // Each point is a tool radius off the boss's side, within tolerance.
        for p in pass {
            let d = (p.x.abs().max(p.y.abs())) - 5.0;
            let corner = (dvec2(p.x.abs(), p.y.abs()) - dvec2(5.0, 5.0)).max(DVec2::ZERO);
            let off = if corner.x > 0.0 && corner.y > 0.0 {
                corner.length()
            } else {
                d
            };
            assert!(
                (off - 1.5).abs() <= TOLERANCE,
                "{p} is {off} off the boss, not 1.5"
            );
        }
    }
}

#[test]
fn every_point_is_where_the_tool_first_touches() {
    let mesh = dome();
    let tool = ball(2.0);
    let cutter = DropCutter::new(&mesh);
    let (path, report) = run(&mesh, &tool, &params(9.0, 0.5, 1.0));
    assert!(report.pulled > 0, "{report:?}");
    for pass in passes(&path) {
        for &p in &pass {
            let h = cutter.drop_at(&tool, p.x, p.y).unwrap();
            // Clear, and touching within the tolerance.
            assert!(h <= p.z + 1e-9, "{p} is in the part: {h}");
            assert!(p.z - h <= TOLERANCE, "{p} is {} off the part", p.z - h);
        }
    }
}

#[test]
fn a_coarse_region_still_puts_every_point_on_the_part() {
    let mesh = dome();
    let tool = ball(2.0);
    let cutter = DropCutter::new(&mesh);
    // Bands and arcs ten times coarser than the tolerance asked for.
    let mut reach = LevelReach::new(&mesh, &tool, 10.0 * TOLERANCE).unwrap();
    let area = Region::rectangle(dvec2(-30.0, -30.0), dvec2(30.0, 30.0));
    let (path, report) =
        waterline(&mut reach, &cutter, &tool, &area, &params(9.0, 0.5, 1.0)).unwrap();
    assert!(report.passes > 5, "{report:?}");
    for pass in passes(&path) {
        for w in pass.windows(2) {
            let (a, b) = (w[0], w[1]);
            // Each point clear of the part and touching it within the
            // tolerance; a chord between, as any pass's, within the
            // tolerance of it either way.
            for p in [a, b] {
                let h = cutter.drop_at(&tool, p.x, p.y).unwrap();
                assert!(h <= p.z + 1e-9, "{p} is in the part: {h}");
                assert!(p.z - h <= TOLERANCE, "{p} is {} off the part", p.z - h);
            }
            // Measured across the chord, as a wall's is: on a steep one a
            // hair across is a long way up or down.
            let m = a.midpoint(b);
            let across = (b - a).truncate().normalize().perp() * TOLERANCE;
            let clear = |q: glam::DVec2| {
                cutter
                    .drop_at(&tool, q.x, q.y)
                    .is_none_or(|h| h <= m.z + 1e-9)
            };
            let (here, left, right) = (m.truncate(), m.truncate() + across, m.truncate() - across);
            assert!(
                clear(here) || clear(left) || clear(right),
                "{m} is more than the tolerance into the part"
            );
            assert!(
                !clear(left) || !clear(right),
                "{m} is more than the tolerance off the part"
            );
        }
    }
}

#[test]
fn climbing_goes_round_the_part_clockwise() {
    let mesh = boss();
    let tool = flat(1.5);
    let (path, _) = run(&mesh, &tool, &params(5.0, 0.0, 2.5));
    for pass in passes(&path) {
        let flat: Vec<DVec2> = pass.iter().map(|p| p.truncate()).collect();
        let area = camcraft_geom::contour::Contour::new(flat)
            .unwrap()
            .signed_area();
        assert!(area < 0.0, "a climbing ring round a boss goes clockwise");
    }
}

#[test]
fn a_slope_range_leaves_the_shallow_top() {
    let mesh = dome();
    let tool = ball(1.0);
    let all = run(&mesh, &tool, &params(9.9, 0.5, 0.5)).1;
    let steep = run(
        &mesh,
        &tool,
        &WaterlineParams {
            slope_from: 45f64.to_radians(),
            ..params(9.9, 0.5, 0.5)
        },
    );
    assert!(steep.1.outside_slope > 0, "{:?}", steep.1);
    for pass in passes(&steep.0) {
        // Steeper than 45 degrees on a hemisphere is below its radius / √2.
        assert!(pass[0].z < 10.0 / 2f64.sqrt() + 0.5, "{:?}", pass[0]);
    }
    assert!(steep.1.passes < all.passes);
}

#[test]
fn by_area_cuts_each_boss_all_the_way_down_first() {
    let left = synthetic::cuboid(DVec3::new(-11.0, -3.0, 0.0), DVec3::new(-5.0, 3.0, 4.0));
    let right = synthetic::cuboid(DVec3::new(5.0, -3.0, 0.0), DVec3::new(11.0, 3.0, 4.0));
    let mesh = TriMesh::merged(&[&left, &right]).unwrap();
    let tool = flat(1.0);
    let by_area = WaterlineParams {
        order: Order::ByArea,
        ..params(4.0, 0.0, 1.0)
    };
    let (path, _) = run(&mesh, &tool, &by_area);
    let sides: Vec<bool> = passes(&path).iter().map(|p| p[0].x > 0.0).collect();
    let changes = sides.windows(2).filter(|w| w[0] != w[1]).count();
    assert_eq!(changes, 1, "{sides:?}");
}

/// By area, the next piece is the one whose highest pass starts nearest: an
/// open pass cut one way only starts at its first point, however near its
/// last.
#[test]
fn by_area_goes_to_the_piece_whose_pass_starts_nearest() {
    let open = |a: DVec2, b: DVec2| Run {
        points: vec![a.extend(0.0), b.extend(0.0)],
        closed: false,
    };
    let by_level = vec![
        Vec::new(),
        vec![
            (0, open(dvec2(10.0, 0.0), dvec2(0.0, 0.0))),
            (1, open(dvec2(4.0, 0.0), dvec2(5.0, 0.0))),
        ],
    ];
    let at = Some(DVec2::ZERO);
    assert_eq!(nearest_island(&by_level, at, false), Some(1));
    assert_eq!(nearest_island(&by_level, at, true), Some(0));
    assert_eq!(nearest_island(&[Vec::new()], at, true), None);
}

/// A boundary across a straight wall leaves a stretch of two points, one
/// where it crosses the ring either side, each pulled onto the part as any
/// other.
#[test]
fn a_straight_stretch_of_two_points_is_pulled_onto_the_part() {
    let mesh = boss();
    let tool = flat(1.5);
    let cutter = DropCutter::new(&mesh);
    let mut reach = LevelReach::new(&mesh, &tool, TOLERANCE).unwrap();
    // Across the boss's wall at x = 5 alone.
    let area = Region::rectangle(dvec2(0.0, -3.0), dvec2(30.0, 3.0));
    let (path, report) =
        waterline(&mut reach, &cutter, &tool, &area, &params(5.0, 0.0, 1.0)).unwrap();
    assert!(report.passes >= 5, "{report:?}");
    assert_eq!(report.unpulled, 0, "{report:?}");
    for pass in passes(&path) {
        assert_eq!(pass.len(), 2, "{pass:?}");
        for p in pass {
            assert!((p.x - 6.5).abs() <= TOLERANCE, "{p} is not on the wall");
        }
    }
}

#[test]
fn is_deterministic() {
    let mesh = dome();
    let tool = ball(2.0);
    let p = params(9.0, 0.5, 1.0);
    let a = run(&mesh, &tool, &p).0;
    let b = run(&mesh, &tool, &p).0;
    assert_eq!(a.moves(), b.moves());
}
