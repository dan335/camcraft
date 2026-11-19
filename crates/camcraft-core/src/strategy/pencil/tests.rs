use super::*;
use crate::cutter::allowance::{Allowance, EffectiveTool};
use crate::cutter::dropcutter::DropCutter;
use crate::cutter::surfaces::PartSurfaces;
use crate::cutter::tool::Ball;
use crate::path::toolpath::MoveRole;
use camcraft_geom::mesh::TriMesh;
use camcraft_geom::synthetic;
use glam::{dvec2, dvec3};

const TOLERANCE: f64 = 0.01;

fn ball(r: f64) -> EffectiveTool {
    EffectiveTool::new(&Ball::new(r).unwrap(), Allowance::none()).unwrap()
}

fn params() -> PencilParams {
    PencilParams {
        tolerance: TOLERANCE,
        ..Default::default()
    }
}

fn run(
    mesh: &TriMesh,
    tool: &EffectiveTool,
    half: f64,
    params: &PencilParams,
) -> (Toolpath, PencilReport) {
    let part = PartSurfaces::whole(DropCutter::new(mesh));
    let seat = part.under(tool).unwrap();
    let area = Region::rectangle(dvec2(-half, -half), dvec2(half, half));
    pencil(&seat, &seat, &area, params).unwrap()
}

/// Each pass's points, the link to it first.
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

/// No point of a cut, at its ends and between them, has the tool in the part
/// by more than the tolerance: where the tool rests dropped there is no higher
/// than the cut, or, at a wall's foot, is not a tolerance aside, its side in
/// the wall by less than that.
fn on_the_part(mesh: &TriMesh, tool: &EffectiveTool, path: &Toolpath) {
    let cutter = DropCutter::new(mesh);
    let clear = |p: DVec3, aside: DVec2| {
        let q = p.truncate() + aside;
        cutter
            .drop_at(tool, q.x, q.y)
            .is_none_or(|rest| rest <= p.z + TOLERANCE)
    };
    for pass in passes(path) {
        for pair in pass.windows(2) {
            let [a, b] = [pair[0], pair[1]];
            for k in 0..=10 {
                let p = a.lerp(b, f64::from(k) / 10.0);
                let aside = (0..16).any(|i| {
                    let turn = f64::from(i) * std::f64::consts::TAU / 16.0;
                    clear(p, DVec2::from_angle(turn) * TOLERANCE)
                });
                assert!(clear(p, DVec2::ZERO) || aside, "{p} is in the part");
            }
        }
    }
}

/// A V groove along Y, its sides `slope` up from its bottom at X0.
fn groove(slope: f64, half: f64) -> TriMesh {
    let v = vec![
        dvec3(-half, -half, slope * half),
        dvec3(0.0, -half, 0.0),
        dvec3(0.0, half, 0.0),
        dvec3(-half, half, slope * half),
        dvec3(half, -half, slope * half),
        dvec3(half, half, slope * half),
    ];
    TriMesh::new(v, vec![[0, 1, 2], [0, 2, 3], [1, 4, 5], [1, 5, 2]]).unwrap()
}

/// A floor at Z0 with a sphere of `radius` about (0, 0, `centre`) standing
/// on it.
fn dome_on_floor(radius: f64, centre: f64) -> TriMesh {
    let floor = synthetic::plane(0.0, 20.0);
    let sphere = synthetic::uv_sphere(radius, 60, 120);
    let mut v: Vec<DVec3> = floor.vertices().to_vec();
    let base = u32::try_from(v.len()).unwrap();
    v.extend(
        sphere
            .vertices()
            .iter()
            .map(|p| *p + dvec3(0.0, 0.0, centre)),
    );
    let mut t: Vec<[u32; 3]> = floor.triangles().to_vec();
    t.extend(sphere.triangles().iter().map(|t| t.map(|i| i + base)));
    TriMesh::new(v, t).unwrap()
}

/// At a wall's foot a ball's side touches the wall and its end the floor: the
/// line is a radius out from the wall, on the floor, all along it.
#[test]
fn a_step_is_finished_along_its_foot() {
    let mesh = synthetic::step(4.0, 10.0);
    let tool = ball(1.0);
    let (path, report) = run(&mesh, &tool, 8.0, &params());
    assert_eq!(report.lines, 1, "{report:?}");
    let pass = &passes(&path)[0];
    for p in pass {
        assert!((p.x - 1.0).abs() <= TOLERANCE, "{p}");
        assert!(p.z.abs() <= 1e-9, "{p}");
    }
    let ys: Vec<f64> = pass.iter().map(|p| p.y).collect();
    let span =
        ys.iter().copied().fold(f64::MIN, f64::max) - ys.iter().copied().fold(f64::MAX, f64::min);
    assert!(span > 15.0, "{span}");
    on_the_part(&mesh, &tool, &path);
}

/// In a V groove a ball touches both sides at once all along its bottom,
/// raised off it by `r (sqrt(1 + slope^2) - 1)`; the sides' normals are 90°
/// apart, so a bitangency angle past that finds nothing.
#[test]
fn a_groove_is_finished_along_its_bottom_where_sharp_enough() {
    let mesh = groove(1.0, 10.0);
    let tool = ball(1.0);
    let (path, report) = run(&mesh, &tool, 8.0, &params());
    assert_eq!(report.lines, 1, "{report:?}");
    let want = 2f64.sqrt() - 1.0;
    for p in &passes(&path)[0] {
        assert!(p.x.abs() <= TOLERANCE, "{p}");
        assert!((p.z - want).abs() <= TOLERANCE, "{p}");
    }
    on_the_part(&mesh, &tool, &path);
    let blunt = PencilParams {
        bitangency: 100f64.to_radians(),
        ..params()
    };
    let (path, report) = run(&mesh, &tool, 8.0, &blunt);
    assert_eq!(report.passes, 0, "{report:?}");
    assert!(path.is_empty());
}

/// Round a dome's foot a ball rests on the floor and against the dome: once
/// round, its centre the ball's radius and the dome's from the dome's.
#[test]
fn a_dome_on_a_floor_is_gone_round_at_its_foot() {
    let mesh = dome_on_floor(5.0, 2.0);
    let tool = ball(2.0);
    let (path, report) = run(&mesh, &tool, 12.0, &params());
    assert_eq!(report.lines, 1, "{report:?}");
    // The ball's centre at Z2, as high as the sphere's: 5 + 2 out from it,
    // less what the facets cut off the sphere.
    let sag = synthetic::uv_sphere_max_sag(5.0, 60, 120);
    let pass = &passes(&path)[0];
    for p in pass {
        let r = p.truncate().length();
        assert!(
            r <= 7.0 + TOLERANCE && r >= 7.0 - sag - TOLERANCE,
            "{p}: {r}"
        );
        // On the floor, or where a chord is kept off the dome, as high on its
        // foot as the tolerance at most.
        assert!(p.z >= -1e-9 && p.z <= TOLERANCE, "{p}");
    }
    // Round, back to where it started.
    let turned: f64 = pass
        .windows(2)
        .map(|w| {
            let (a, b) = (w[0].truncate(), w[1].truncate());
            a.perp_dot(b).atan2(a.dot(b))
        })
        .sum();
    assert!(
        (turned.abs() - std::f64::consts::TAU).abs() < 0.1,
        "{turned}"
    );
    on_the_part(&mesh, &tool, &path);
}

/// One way has the higher side on the tool's right, climbing with a clockwise
/// spindle: along the step's foot, the wall at -X, it goes -Y; the other way
/// +Y.
#[test]
fn one_way_keeps_the_wall_on_the_right() {
    let mesh = synthetic::step(4.0, 10.0);
    let tool = ball(1.0);
    for (direction, sign) in [(Direction::Climb, -1.0), (Direction::Conventional, 1.0)] {
        let (path, _) = run(
            &mesh,
            &tool,
            8.0,
            &PencilParams {
                direction,
                ..params()
            },
        );
        let pass = &passes(&path)[0];
        let (first, last) = (pass[0], pass[pass.len() - 1]);
        assert!(
            (last.y - first.y) * sign > 10.0,
            "{direction:?}: {first} to {last}"
        );
    }
}

/// With stepovers, passes beside the line a stepover apart: round the dome,
/// rings up it and out over the floor, cut before the line.
#[test]
fn stepovers_add_passes_beside_the_line() {
    let mesh = dome_on_floor(5.0, 2.0);
    let tool = ball(2.0);
    let (path, report) = run(
        &mesh,
        &tool,
        12.0,
        &PencilParams {
            stepovers: 2,
            stepover: 1.0,
            ..params()
        },
    );
    assert_eq!(report.lines, 1, "{report:?}");
    assert!(report.beside >= 4, "{report:?}");
    assert_eq!(report.passes, report.lines + report.beside, "{report:?}");
    // The line is last.
    let last = passes(&path).pop().unwrap();
    let r = last[1].truncate().length();
    assert!((r - 7.0).abs() < 0.1, "{r}");
}

/// Identical inputs give identical paths, however the work is shared out.
#[test]
fn is_deterministic() {
    let mesh = dome_on_floor(5.0, 2.0);
    let tool = ball(2.0);
    let p = PencilParams {
        stepovers: 1,
        stepover: 1.0,
        ..params()
    };
    let (a, ra) = run(&mesh, &tool, 12.0, &p);
    let (b, rb) = run(&mesh, &tool, 12.0, &p);
    assert_eq!(ra, rb);
    assert_eq!(a.moves(), b.moves());
}
