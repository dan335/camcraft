use super::*;
use crate::cutter::allowance::{Allowance, EffectiveTool};
use crate::cutter::tool::{Ball, Flat};
use camcraft_geom::mesh::TriMesh;
use camcraft_geom::synthetic;
use glam::dvec2;

const TOLERANCE: f64 = 0.02;

/// Each pass: its points, the first the link to it.
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

fn run(mesh: &TriMesh, tool: &EffectiveTool, params: &RampParams) -> (Toolpath, RampReport) {
    let cutter = DropCutter::new(mesh);
    let mut reach = LevelReach::new(mesh, tool, params.tolerance).unwrap();
    let area = Region::rectangle(dvec2(-30.0, -30.0), dvec2(30.0, 30.0));
    ramp(&mut reach, &cutter, tool, &area, params).unwrap()
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

fn params(top: f64, bottom: f64, stepdown: f64) -> RampParams {
    RampParams {
        stepdown,
        top,
        bottom,
        tolerance: TOLERANCE,
        ..Default::default()
    }
}

fn xy_length(points: &[DVec3]) -> f64 {
    points
        .windows(2)
        .map(|w| w[0].truncate().distance(w[1].truncate()))
        .sum()
}

/// Down a boss's wall in one pass, never going up, a stepdown each time round,
/// and once round at its foot: as Fusion's Ramp goes down `ops_3d`'s outside.
#[test]
fn a_wall_is_gone_down_in_one_pass_a_stepdown_each_time_round() {
    let mesh = boss();
    let tool = flat(1.5);
    let (path, report) = run(&mesh, &tool, &params(5.0, 0.0, 1.0));
    assert_eq!(report.walls, 1, "{report:?}");
    let passes = passes(&path);
    assert_eq!(passes.len(), 1, "{report:?}");
    let pass = &passes[0];
    // From the Top, down to the foot, never up.
    assert!((pass[0].z - 5.0).abs() < 1e-9, "{:?}", pass[0]);
    assert!(pass.last().unwrap().z.abs() < 1e-9);
    assert!(pass.windows(2).all(|w| w[1].z <= w[0].z + 1e-9));
    // The ring is the boss and the tool's radius, round: 40 + 3 pi.
    let lap = 40.0 + 2.0 * std::f64::consts::PI * 1.5;
    // Five times round going down, and once at the foot.
    let length = xy_length(pass);
    assert!(
        (length - 6.0 * lap).abs() < 0.02 * lap,
        "{length} for {lap}"
    );
    let level: f64 = pass
        .windows(2)
        .filter(|w| w[0].z.abs() < 1e-9 && w[1].z.abs() < 1e-9)
        .map(|w| w[0].truncate().distance(w[1].truncate()))
        .sum();
    assert!(
        (level - lap).abs() < 0.02 * lap,
        "{level} at the foot for {lap}"
    );
    // Every point a tool radius off the boss's side, within the tolerance.
    for p in pass {
        let c = dvec2(p.x.abs(), p.y.abs()) - dvec2(5.0, 5.0);
        let off = if c.x > 0.0 && c.y > 0.0 {
            c.length()
        } else {
            c.x.max(c.y)
        };
        assert!((off - 1.5).abs() <= TOLERANCE, "{p} is {off} off the boss");
    }
}

/// Going down a dome, the tool is on the part at every height it passes, never
/// in it: the ring below, clear of the part above its level, is pulled in to
/// where the tool first touches at the height it is at.
#[test]
fn going_down_a_dome_the_tool_is_on_the_part_all_the_way() {
    let mesh = dome();
    let tool = ball(2.0);
    let cutter = DropCutter::new(&mesh);
    let (path, report) = run(&mesh, &tool, &params(9.0, 0.5, 1.0));
    assert_eq!(report.walls, 1, "{report:?}");
    assert!(report.pulled > 0, "{report:?}");
    let pass = &passes(&path)[0];
    // Below the first ring's level, every point is on the part, measured
    // across the surface, out from the dome's axis, as a wall is: clear of it
    // half a tolerance further out, which allows for a wall starting or a step
    // ending partway along a ring's chord (contour's rings keep their chords to
    // a quarter of the tolerance, and are thinned by another quarter after),
    // and touching it a tolerance further in. Measured up or down, a point a
    // micron off near the dome's upright foot would read a hundredth.
    let first = 8.0;
    let mut on = 0;
    for &p in pass.iter().filter(|p| p.z <= first) {
        let out = p.truncate().normalize();
        let at = |by: f64| {
            let q = p.truncate() + out * by;
            cutter.drop_at(&tool, q.x, q.y).unwrap()
        };
        assert!(at(0.5 * TOLERANCE) <= p.z + 1e-9, "{p} is in the part");
        assert!(at(-TOLERANCE) > p.z, "{p} is off the part");
        on += 1;
    }
    assert!(on > 100, "{on} points checked");
    // And it falls no steeper than the ramp angle, and on the dome's flank
    // no steeper than a stepdown round the ring below.
    let tan = 2f64.to_radians().tan();
    for w in pass.windows(2) {
        let run = w[0].truncate().distance(w[1].truncate());
        if run > 1e-6 {
            assert!(w[0].z - w[1].z <= tan * run + 1e-6, "{w:?}");
        }
    }
}

/// Two bosses are two walls, each its own pass, the second gone once round at
/// its top level before going on down, as every wall but Fusion's first is.
#[test]
fn every_wall_but_the_first_is_gone_round_at_its_top() {
    let left = synthetic::cuboid(DVec3::new(-15.0, -5.0, 0.0), DVec3::new(-5.0, 5.0, 5.0));
    let right = synthetic::cuboid(DVec3::new(5.0, -5.0, 0.0), DVec3::new(15.0, 5.0, 5.0));
    let mesh = TriMesh::merged(&[&left, &right]).unwrap();
    let tool = flat(1.5);
    let (path, report) = run(&mesh, &tool, &params(5.0, 0.0, 1.0));
    assert_eq!(report.walls, 2, "{report:?}");
    let passes = passes(&path);
    assert_eq!(passes.len(), 2);
    let lap = 40.0 + 2.0 * std::f64::consts::PI * 1.5;
    let at_top = |pass: &[DVec3]| -> f64 {
        pass.windows(2)
            .filter(|w| (w[0].z - 4.0).abs() < 1e-9 && (w[1].z - 4.0).abs() < 1e-9)
            .map(|w| w[0].truncate().distance(w[1].truncate()))
            .sum()
    };
    assert!(at_top(&passes[0]) < 0.05 * lap, "{}", at_top(&passes[0]));
    assert!(
        (at_top(&passes[1]) - lap).abs() < 0.02 * lap,
        "{}",
        at_top(&passes[1])
    );
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
