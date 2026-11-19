use super::*;
use crate::cutter::allowance::{Allowance, EffectiveTool};
use crate::cutter::surfaces::{Group, PartSurfaces};
use crate::cutter::tool::Ball;
use crate::path::toolpath::MoveRole;
use camcraft_geom::mesh::TriMesh;
use camcraft_geom::synthetic;
use glam::{DVec3, dvec3};

const TOLERANCE: f64 = 0.01;

/// The step, 5 high over `x <= 0` and 0 beyond, its plateau and wall
/// avoided with a clearance of `clear` sideways, its shelf machined.
fn step(clear: f64) -> PartSurfaces<'static> {
    let mesh = synthetic::step(5.0, 10.0);
    let of = |triangles: Vec<[u32; 3]>, radial: f64, machined: bool| {
        Group::new(
            TriMesh::new(mesh.vertices().to_vec(), triangles).unwrap(),
            Allowance::new(radial, 0.0).unwrap(),
            machined,
        )
    };
    let t = mesh.triangles();
    PartSurfaces::grouped(vec![
        of(vec![t[0], t[1], t[4], t[5]], clear, false),
        of(vec![t[2], t[3]], 0.0, true),
    ])
}

/// A pass across the step at `y`, as a strategy lays one: on the part, every
/// tenth of a millimetre from `x = -8` to `8`.
fn across(seat: &crate::cutter::surfaces::Seat<'_>, y: f64) -> Toolpath {
    let mut p = Toolpath::new();
    for i in 0..=160 {
        let x = -8.0 + f64::from(i) * 0.1;
        let at = dvec3(x, y, seat.drop_at(x, y).unwrap());
        let role = if i == 0 {
            MoveRole::Link
        } else {
            MoveRole::Cut
        };
        p.feed_to(at, role, 1000.0).unwrap();
    }
    p
}

/// What rests only on the faces avoided is cut out: what is left of the pass
/// comes down off the wall's top with the tool in the air, onto the shelf
/// where the tool is its radius and the clearance from the wall, and runs on
/// to its end. Short of there, nothing is kept but in the air.
#[test]
fn a_pass_is_cut_where_it_rests_on_faces_avoided_alone() {
    let tool = EffectiveTool::new(&Ball::new(2.0).unwrap(), Allowance::none()).unwrap();
    for clear in [0.0, 1.0] {
        let part = step(clear);
        let seat = part.under(&tool).unwrap();
        let (kept, report) = machined_only(&across(&seat, 0.0), &seat, TOLERANCE).unwrap();
        assert_eq!((report.before, report.after), (1, 1), "{clear}");
        let points: Vec<DVec3> = kept.moves().iter().map(|m| m.to).collect();
        let reach = 2.0 + clear;
        for p in &points {
            let rest = seat.drop_at(p.x, p.y).unwrap();
            assert!(
                p.x >= reach - TOLERANCE || p.z > rest + TOLERANCE,
                "{clear}: {p}"
            );
        }
        let landed = points.iter().find(|p| p.z.abs() < 1e-9).unwrap();
        assert!(landed.x <= reach + 0.1 + 1e-9, "{clear}: lands at {landed}");
        assert!((points.last().unwrap().x - 8.0).abs() < 1e-9);
    }
}

/// A pass at a level, towards the wall avoided, as a way down along a ring
/// goes: up off the shelf, it touches nothing until it would rub the wall,
/// and is left out whole; on the shelf, which it machines all along, it is
/// kept, up to the wall's foot.
#[test]
fn a_piece_touching_nothing_machined_is_left_out() {
    let tool = EffectiveTool::new(&Ball::new(2.0).unwrap(), Allowance::none()).unwrap();
    let part = step(0.0);
    let seat = part.under(&tool).unwrap();
    let level = |z: f64| {
        let mut p = Toolpath::new();
        p.feed_to(dvec3(8.0, 0.0, z), MoveRole::Link, 1000.0)
            .unwrap();
        p.feed_to(dvec3(2.0, 0.0, z), MoveRole::Cut, 1000.0)
            .unwrap();
        p
    };
    let (kept, report) = machined_only(&level(1.0), &seat, TOLERANCE).unwrap();
    assert!(kept.is_empty(), "{:?}", kept.moves());
    assert_eq!((report.before, report.after), (1, 0));
    let (kept, report) = machined_only(&level(0.0), &seat, TOLERANCE).unwrap();
    assert_eq!((report.before, report.after), (1, 1));
    assert_eq!(kept, level(0.0));
}

/// With nothing avoided, a pass is as it was.
#[test]
fn nothing_avoided_keeps_every_pass() {
    let mesh = synthetic::step(5.0, 10.0);
    let part = PartSurfaces::grouped(vec![Group::new(mesh, Allowance::none(), true)]);
    let tool = EffectiveTool::new(&Ball::new(2.0).unwrap(), Allowance::none()).unwrap();
    let seat = part.under(&tool).unwrap();
    let pass = across(&seat, 0.0);
    let (kept, report) = machined_only(&pass, &seat, TOLERANCE).unwrap();
    assert_eq!(kept, pass);
    assert_eq!((report.before, report.after), (1, 1));
}
