use super::*;
use crate::cutter::tool::Ball;
use camcraft_geom::synthetic;

const TOLERANCE: f64 = 0.01;

/// A ball of radius `r`, leaving nothing.
fn ball(r: f64) -> EffectiveTool {
    EffectiveTool::new(&Ball::new(r).unwrap(), Allowance::none()).unwrap()
}

/// The step, 5 high over `x <= 0` and 0 beyond, its wall at `x = 0`, as
/// three groups: its plateau, its wall and its shelf, each machined or not
/// with offsets `(radial, axial)` as given.
fn step(
    plateau: (bool, f64, f64),
    wall: (bool, f64, f64),
    shelf: (bool, f64, f64),
) -> PartSurfaces<'static> {
    let mesh = synthetic::step(5.0, 10.0);
    let points = mesh.vertices().to_vec();
    // Its triangles: two of plateau, two of shelf, two of wall.
    let of = |first: usize, (machined, radial, axial): (bool, f64, f64)| {
        let triangles = mesh.triangles()[first..first + 2].to_vec();
        Group::new(
            TriMesh::new(points.clone(), triangles).unwrap(),
            Allowance::new(radial, axial).unwrap(),
            machined,
        )
    };
    PartSurfaces::grouped(vec![of(0, plateau), of(4, wall), of(2, shelf)])
}

/// A face avoided with an axial clearance holds the tool that much above it,
/// and a face machined with stock of its own leaves it: each group is met by
/// the tool grown by its own offsets.
#[test]
fn each_group_is_met_by_the_tool_grown_by_its_own_offsets() {
    let part = step((false, 0.0, 1.0), (true, 0.0, 0.0), (true, 0.3, 0.3));
    let seat = part.under(&ball(2.0)).unwrap();
    assert!((seat.drop_at(-5.0, 0.0).unwrap() - 6.0).abs() < 1e-9);
    assert!((seat.drop_at(5.0, 0.0).unwrap() - 0.3).abs() < 1e-9);
    // Held up by the face avoided alone: not machined, and kept clear of.
    let rest = seat.rest_at(-5.0, 0.0, TOLERANCE).unwrap();
    assert!(!rest.machined, "{rest:?}");
    assert!(seat.avoids(dvec3(-5.0, 0.0, rest.z), TOLERANCE));
    let rest = seat.rest_at(5.0, 0.0, TOLERANCE).unwrap();
    assert!(rest.machined, "{rest:?}");
    assert!(!seat.avoids(dvec3(5.0, 0.0, rest.z), TOLERANCE));
}

/// A wall avoided with a radial clearance holds the tool off it by its
/// radius and the clearance: past that the tool rests on the shelf.
#[test]
fn a_wall_avoided_holds_the_tool_its_radius_and_its_clearance_off() {
    let part = step((false, 1.0, 0.0), (false, 1.0, 0.0), (true, 0.0, 0.0));
    let seat = part.under(&ball(2.0)).unwrap();
    // A 2 mm ball, 1 mm clear: 3 mm from the wall.
    let past = seat.rest_at(3.0 + 1e-6, 0.0, TOLERANCE).unwrap();
    assert!(past.z.abs() < 1e-9 && past.machined, "{past:?}");
    let short = seat.rest_at(3.0 - 0.02, 0.0, TOLERANCE).unwrap();
    assert!(short.z > 2.5 && !short.machined, "{short:?}");
    assert!(seat.avoids(dvec3(3.0 - 0.02, 0.0, short.z), TOLERANCE));
}

/// A tool resting on a face machined and touching one avoided at once is
/// machined: at the foot of the wall, the ball is on the shelf and against
/// the wall.
#[test]
fn resting_on_a_face_machined_and_one_avoided_is_machined() {
    let part = step((false, 0.0, 0.0), (false, 0.0, 0.0), (true, 0.0, 0.0));
    let seat = part.under(&ball(2.0)).unwrap();
    // Just past touching it: exactly touching a wall is resting on its top.
    let foot = seat.rest_at(2.0 + 1e-6, 0.0, TOLERANCE).unwrap();
    assert!(foot.z.abs() < 1e-9 && foot.machined, "{foot:?}");
    assert!(!seat.avoids(dvec3(2.0 + 1e-6, 0.0, foot.z), TOLERANCE));
    // A little nearer, it rests on the wall's top alone.
    let up = seat.rest_at(1.9, 0.0, TOLERANCE).unwrap();
    assert!(up.z > 3.0 && !up.machined, "{up:?}");
    // Beside the wall, off the shelf, it touches the wall alone.
    assert!(seat.avoids(dvec3(2.0, 0.0, 2.0), TOLERANCE));
    assert!(!seat.avoids(dvec3(2.1, 0.0, 2.0), TOLERANCE));
}

/// How far into an avoided face's clearance a tool is, past the slack: none
/// where it keeps the clearance, the depth where it does not.
#[test]
fn the_depth_into_a_clearance_is_measured() {
    let part = step((false, 0.0, 1.0), (true, 0.0, 0.0), (true, 0.0, 0.0));
    let seat = part.under(&ball(2.0)).unwrap();
    assert_eq!(seat.into_avoided(dvec3(-5.0, 0.0, 6.0), TOLERANCE), 0.0);
    let into = seat.into_avoided(dvec3(-5.0, 0.0, 5.5), TOLERANCE);
    assert!((into - 0.49).abs() < 1e-9, "{into}");
    assert_eq!(seat.into_avoided(dvec3(5.0, 0.0, 0.0), TOLERANCE), 0.0);
}

/// The whole part is all machined and none avoided, and is dropped onto as
/// its drop-cutter drops onto it.
#[test]
fn the_whole_part_is_machined() {
    let mesh = synthetic::step(5.0, 10.0);
    let part = PartSurfaces::whole(DropCutter::new(&mesh));
    let tool = ball(2.0);
    let seat = part.under(&tool).unwrap();
    assert!(!part.avoids_any());
    for x in [-5.0, 1.0, 2.0, 5.0] {
        let rest = seat.rest_at(x, 0.0, TOLERANCE).unwrap();
        assert_eq!(Some(rest.z), DropCutter::new(&mesh).drop_at(&tool, x, 0.0));
        assert!(rest.machined);
        assert!(!seat.avoids(dvec3(x, 0.0, rest.z), TOLERANCE));
    }
}

/// At a level, the tool may not be where any group forbids it: in groups
/// with no offsets, where the whole part does.
#[test]
fn a_level_is_forbidden_where_any_group_forbids_it() {
    let mesh = synthetic::step(5.0, 10.0);
    let whole = PartSurfaces::whole(DropCutter::new(&mesh));
    let grouped = step((true, 0.0, 0.0), (true, 0.0, 0.0), (true, 0.0, 0.0));
    let tool = ball(2.0);
    for z in [2.0, 4.0] {
        let a = whole.reach(&tool, TOLERANCE).unwrap().forbidden(z).unwrap();
        let b = grouped
            .reach(&tool, TOLERANCE)
            .unwrap()
            .forbidden(z)
            .unwrap();
        assert!((a.area() - b.area()).abs() < 1e-4 * a.area(), "{z}");
    }
    // The plateau avoided 1 mm sideways forbids a millimetre more round it:
    // its 60 mm round, and the corners' quarter circles grown from 2 to 3.
    let wider = step((false, 1.0, 0.0), (false, 1.0, 0.0), (true, 0.0, 0.0));
    let a = whole
        .reach(&tool, TOLERANCE)
        .unwrap()
        .forbidden(2.0)
        .unwrap();
    let b = wider
        .reach(&tool, TOLERANCE)
        .unwrap()
        .forbidden(2.0)
        .unwrap();
    let more = 60.0 + 5.0 * std::f64::consts::PI;
    assert!(
        (b.area() - a.area() - more).abs() < 0.1,
        "{} {}",
        a.area(),
        b.area()
    );
    assert!((wider.widest(&tool) - 3.0).abs() < 1e-12);
}
