//! Finishing with faces machined and faces avoided.

use super::*;
use crate::arrange::Stand;
use crate::fixture::JOB;
use crate::job::{FeatureRef, SurfaceGroup};
use crate::path::toolpath::{MoveRole, Toolpath};
use camcraft_geom::mesh::TriMesh;
use camcraft_geom::synthetic;
use camcraft_geom::topology::Body;
use glam::{DVec2, DVec3, dvec2, dvec3};

/// The step, 5 high over `x <= 0` and 0 beyond, its wall at `x = 0`, with its
/// faces named: the plateau 0, the shelf 1 and the wall 2.
fn step() -> Body {
    Body::with_faces(synthetic::step(5.0, 10.0), vec![0, 0, 1, 1, 2, 2], 3).unwrap()
}

/// The step's plateau, wall and shelf, by a point on each.
const PLATEAU: [f64; 3] = [-5.0, 0.0, 5.0];
const WALL: [f64; 3] = [0.0, 0.0, 2.5];

/// A plate 16 across at Z0 with a ridge 2 wide and 1 high standing across it
/// along Y, with its faces named: the plate 0, the ridge's top 1 and its
/// sides 2.
fn ridge() -> Body {
    let plate = synthetic::plane(0.0, 8.0);
    let ridge = synthetic::cuboid(dvec3(-1.0, -9.0, 0.0), dvec3(1.0, 9.0, 1.0));
    // Its bottom, on the plate, is not part of it.
    let sides: Vec<[u32; 3]> = ridge.triangles()[2..].to_vec();
    let ridge = TriMesh::new(ridge.vertices().to_vec(), sides).unwrap();
    let mesh = TriMesh::merged(&[&plate, &ridge]).unwrap();
    let mut labels = vec![0; plate.len()];
    labels.extend([1, 1]);
    labels.extend(std::iter::repeat_n(2, ridge.len() - 2));
    Body::with_faces(mesh, labels, 3).unwrap()
}

/// The ridge's top and one of its sides, by a point on each.
const RIDGE_TOP: [f64; 3] = [0.0, 0.0, 1.0];
const RIDGE_SIDE: [f64; 3] = [1.0, 0.0, 0.5];

fn face(at: [f64; 3]) -> FeatureRef {
    FeatureRef {
        model: "dome".into(),
        index: 0,
        at,
    }
}

/// `JOB`'s parallel finish alone, its 4 mm ball across X, its faces at `avoid`
/// avoided with a clearance of `(radial, axial)` and the rest machined.
fn finishing(avoid: &[[f64; 3]], (radial, axial): (f64, f64)) -> Job {
    let mut job: Job = toml::from_str(JOB).unwrap();
    // Coarser than `JOB`, to be quick: none of it is near the tolerance.
    job.tolerance = 0.1;
    job.setups[0].operations.retain(|op| op.name == "finish");
    let op = &mut job.setups[0].operations[0];
    op.passes.stepover = 1.0;
    op.surfaces.groups.push(SurfaceGroup {
        action: SurfaceAction::Avoid,
        radial,
        axial,
        faces: avoid.iter().map(|&at| face(at)).collect(),
    });
    job
}

/// How far into an avoided face's clearance any move of `result`'s first
/// operation that feeds goes, past the tolerance, as `job` has its faces:
/// what CC111 measures.
fn into_clearance(job: &Job, bodies: &Bodies<'_>, result: &SetupResult) -> f64 {
    let prepared = crate::setup::prepare(job, 0, bodies).unwrap();
    let op = &job.setups[0].operations[0];
    let surfaces = part_surfaces(job, op, &prepared, bodies).unwrap();
    let seat = surfaces.under(&cutting_tool(job, op).unwrap()).unwrap();
    let path = &result.operations[0].path;
    // And no cut is where the tool would only rub faces it avoids.
    let rubbing: Vec<DVec3> = path
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Cut && seat.avoids(m.to, job.tolerance))
        .map(|m| m.to)
        .collect();
    assert!(
        rubbing.is_empty(),
        "cuts rubbing faces avoided: {rubbing:?}"
    );
    let mut at = path.moves()[0].to;
    let mut deepest = 0.0_f64;
    for m in path.moves() {
        if m.role != MoveRole::Rapid {
            for k in 0..=20 {
                let p = m.point_at(at, f64::from(k) / 20.0);
                deepest = deepest.max(seat.into_avoided(p, job.tolerance));
            }
        }
        at = m.to;
    }
    deepest
}

/// The points `path` cuts to over the part standing about `(x, 0)`, away from
/// its edges, 10 out each way, round which the rings beyond the part go down
/// past it.
fn cut_points(path: &Toolpath, x: f64) -> Vec<DVec3> {
    path.moves()
        .iter()
        .filter(|m| m.role == MoveRole::Cut && (m.to.x - x).abs() < 8.0 && m.to.y.abs() < 8.0)
        .map(|m| m.to)
        .collect()
}

/// A wall avoided 1 mm sideways: the ball stops its radius and the clearance
/// from it, 3 mm, as Fusion's stops 10 + 3 + 2 from its boss's axis, and runs
/// right up to there along the shelf it machines. Nothing of the plateau or
/// the wall is cut, rings round the wall above its foot included, and no move
/// that feeds goes into the clearance.
#[test]
fn the_tool_stops_its_radius_and_the_clearance_from_an_avoided_wall() {
    let body = step();
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let job = finishing(&[PLATEAU, WALL], (1.0, 0.0));
    let result = run_setup(&job, 0, &bodies).unwrap();
    let cuts = cut_points(&result.operations[0].path, 0.0);
    assert!(cuts.len() > 20, "{} cuts", cuts.len());
    let tol = job.tolerance;
    for p in &cuts {
        assert!(p.x >= 3.0 - tol && p.z.abs() <= tol, "a cut at {p}");
    }
    // The rings at the wall's foot run along it there; a pass broken where
    // it falls off the clearance goes on from its next sample.
    let nearest = cuts.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    assert!(nearest < 3.0 + tol, "stops at {nearest}");
    assert_eq!(into_clearance(&job, &bodies, &result), 0.0);
    assert_eq!(result.verify.gouged, 0);
}

/// A top avoided 1 mm down: the tool keeps a millimetre above it, so cuts
/// nothing over it, nor rolling over its edge, where it would rest on the top
/// raised; and machines the shelf right up to the foot of the wall, which is
/// machined, the ball touching it.
#[test]
fn the_tool_keeps_the_axial_clearance_above_an_avoided_face() {
    let body = step();
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let job = finishing(&[PLATEAU], (0.0, 1.0));
    let result = run_setup(&job, 0, &bodies).unwrap();
    let cuts = cut_points(&result.operations[0].path, 0.0);
    let tol = job.tolerance;
    let over: Vec<&DVec3> = cuts.iter().filter(|p| p.x < 2.0 - tol).collect();
    assert!(
        over.is_empty(),
        "cuts over the plateau or its edge: {over:?}"
    );
    // A pass broken where it falls off the edge goes on from its next sample,
    // a stepover on at most, as it does with nothing avoided.
    let stepover = job.setups[0].operations[0].passes.stepover;
    let nearest = cuts.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    assert!(nearest < 2.0 + stepover + tol, "stops at {nearest}");
    assert_eq!(into_clearance(&job, &bodies, &result), 0.0);
}

/// Passes cut either side of a ridge avoided, where the ball touches its top
/// edge and the plate, are not joined across it by a link that stays down on
/// it, though they are well within the stay-down distance: the tool lifts
/// over it ([`crate::path::linker`]'s own test has it stay down with nothing
/// avoided). Links between passes along the ridge's foot stay down.
#[test]
fn a_stay_down_link_over_an_avoided_face_lifts() {
    let body = ridge();
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let mut job = finishing(&[RIDGE_TOP, RIDGE_SIDE], (0.0, 0.0));
    // Each line's two pieces one after the other, so each is linked across.
    job.setups[0].operations[0].linking.reorder = false;
    let result = run_setup(&job, 0, &bodies).unwrap();
    let moves = result.operations[0].path.moves();
    // The 2 mm ball on the plate touches the 1 mm ridge's top edge 1 + √3
    // from its middle.
    let foot = 1.0 + 3.0_f64.sqrt();
    let over: Vec<DVec3> = moves
        .iter()
        .filter(|m| m.role == MoveRole::Transition && m.to.x.abs() < foot - job.tolerance)
        .map(|m| m.to)
        .collect();
    assert!(
        over.is_empty(),
        "a link stays down over the ridge: {over:?}"
    );
    let mut at = moves[0].to;
    let mut lifted = 0;
    for m in moves {
        if m.role == MoveRole::Rapid && (at.x < -1.0 && m.to.x > 1.0 || at.x > 1.0 && m.to.x < -1.0)
        {
            lifted += 1;
        }
        at = m.to;
    }
    assert!(lifted > 10, "{lifted} lifted across the ridge");
    assert_eq!(into_clearance(&job, &bodies, &result), 0.0);
}

/// The step on a sheet twice, the second copy a quarter turn round and moved:
/// its wall and plateau, picked once, are avoided on both copies.
#[test]
fn a_face_picked_once_is_avoided_on_every_copy() {
    let body = step();
    let stands = [
        Stand::NONE,
        Stand {
            turn: camcraft_geom::turn::Turn::from_degrees(90.0).unwrap(),
            offset: DVec3::new(40.0, 0.0, 0.0),
        },
    ];
    let bodies = Bodies::standing([("dome", &body, &stands[..])]);
    let job = finishing(&[PLATEAU, WALL], (1.0, 0.0));
    let result = run_setup(&job, 0, &bodies).unwrap();
    // Each copy's shelf is cut, and nothing above it.
    let path = &result.operations[0].path;
    for x in [0.0, 40.0] {
        let cuts = cut_points(path, x);
        assert!(!cuts.is_empty(), "nothing cut on the copy at X{x}");
        let high: Vec<&DVec3> = cuts.iter().filter(|p| p.z > job.tolerance).collect();
        assert!(high.is_empty(), "cuts off the shelf at X{x}: {high:?}");
    }
    assert_eq!(into_clearance(&job, &bodies, &result), 0.0);
}

/// A face in two groups is refused at the run, where the models say what it
/// is, by the two groups it is in.
#[test]
fn a_face_in_two_groups_is_refused() {
    let body = step();
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let mut job = finishing(&[PLATEAU], (0.0, 0.0));
    job.setups[0].operations[0]
        .surfaces
        .groups
        .push(SurfaceGroup {
            action: SurfaceAction::Machine,
            radial: 0.0,
            axial: 0.0,
            // Another point on the same face.
            faces: vec![face([-2.0, 3.0, 5.0])],
        });
    let err = run_setup(&job, 0, &bodies).unwrap_err();
    assert!(
        matches!(
            &err,
            RunError::Feature {
                source: FeatureError::TwoGroups {
                    first: 1,
                    second: 2,
                    ..
                },
                ..
            }
        ),
        "{err}"
    );
}

/// A V-bit cannot be grown and stay a cone: a group machined with stock of its
/// own is refused as stock to leave is, and faces avoided at all, since how
/// near a cone comes to one, beside it as well as above, cannot be told.
#[test]
fn a_vbit_is_refused_what_it_cannot_keep_to() {
    let body = step();
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let mut job = finishing(&[PLATEAU, WALL], (0.0, 0.0));
    job.tools.insert(
        "vee".into(),
        toml::from_str("kind = \"cone\"\ndiameter = 6.0\nangle = 90.0\nnumber = 3\n").unwrap(),
    );
    job.setups[0].operations[0].tool = Some("vee".into());
    let err = run_setup(&job, 0, &bodies).unwrap_err();
    assert!(
        matches!(&err, RunError::Job(JobError::WrongTool { why, .. }) if why.contains("faces avoided")),
        "{err}"
    );
    let group = &mut job.setups[0].operations[0].surfaces.groups[0];
    group.action = SurfaceAction::Machine;
    group.radial = 0.5;
    let err = run_setup(&job, 0, &bodies).unwrap_err();
    assert!(
        matches!(
            err,
            RunError::Tool(crate::cutter::tool::ToolError::ConeStock)
        ),
        "{err}"
    );
}

/// A plate 16 across at Z0 with two ridges 2 wide and 1 high standing across
/// it along Y, about X-3.5 and X3.5, with their faces named: the plate 0, the
/// first ridge's top 1 and its sides 2, the second's top 3 and its sides 4.
fn ridges() -> Body {
    let plate = synthetic::plane(0.0, 8.0);
    let ridge = |x: f64| {
        let r = synthetic::cuboid(dvec3(x - 1.0, -9.0, 0.0), dvec3(x + 1.0, 9.0, 1.0));
        // Its bottom, on the plate, is not part of it.
        TriMesh::new(r.vertices().to_vec(), r.triangles()[2..].to_vec()).unwrap()
    };
    let (first, second) = (ridge(-3.5), ridge(3.5));
    let mesh = TriMesh::merged(&[&plate, &first, &second]).unwrap();
    let mut labels = vec![0; plate.len()];
    for (top, sides) in [(1, 2), (3, 4)] {
        labels.extend([top, top]);
        labels.extend(std::iter::repeat_n(sides, first.len() - 2));
    }
    Body::with_faces(mesh, labels, 5).unwrap()
}

/// The plate, and the second ridge's top and sides, by a point on each.
const PLATE: [f64; 3] = [0.0, 0.0, 0.0];
const SECOND_TOP: [f64; 3] = [3.5, 0.0, 1.0];
const SECOND_SIDE: [f64; 3] = [4.5, 0.0, 0.5];

/// `JOB`'s finish as a new operation of `strategy`, its 4 mm ball, the first
/// ridge machined and the rest of the part avoided half a millimetre each
/// way; or, not `grouped`, all of it machined. A project runs along the long
/// edges of both ridges' tops, and a morph between their outer ones.
fn on_ridges(strategy: &str, grouped: bool) -> Job {
    let mut job = finishing(&[PLATE, SECOND_TOP, SECOND_SIDE], (0.5, 0.5));
    let op = &mut job.setups[0].operations[0];
    op.strategy = StrategySpec::named(strategy).unwrap();
    let edges: &[[f64; 3]] = match strategy {
        "project" => &[
            [-4.5, 0.0, 1.0],
            [-2.5, 0.0, 1.0],
            [2.5, 0.0, 1.0],
            [4.5, 0.0, 1.0],
        ],
        "morph" => &[[-4.5, 0.0, 1.0], [4.5, 0.0, 1.0]],
        _ => &[],
    };
    op.selection.edges = edges.iter().map(|&at| face(at)).collect();
    if !grouped {
        op.surfaces = crate::job::Surfaces::default();
    }
    job
}

/// `strategy` run on the ridges with the first alone machined, and with all
/// of it machined. Run grouped, no move that feeds goes into the clearance
/// of what it avoids, no cut is where the tool would only rub it, and it
/// cuts nothing over the plate or the second ridge, which it does machining
/// all of it: everything it cuts is beside the first ridge, the tool
/// touching it.
fn kept_to_the_first_ridge(strategy: &str) -> (SetupResult, SetupResult) {
    let body = ridges();
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let job = on_ridges(strategy, true);
    let grouped = run_setup(&job, 0, &bodies).unwrap();
    let whole = run_setup(&on_ridges(strategy, false), 0, &bodies).unwrap();
    assert_eq!(into_clearance(&job, &bodies, &grouped), 0.0, "{strategy}");
    assert_eq!(grouped.verify.gouged, 0, "{strategy}");
    let past = |r: &SetupResult| -> Vec<DVec3> {
        r.operations[0]
            .path
            .moves()
            .iter()
            .filter(|m| m.role == MoveRole::Cut && m.to.x > -0.5)
            .map(|m| m.to)
            .collect()
    };
    assert!(!past(&whole).is_empty(), "{strategy} machining all of it");
    let cut = past(&grouped);
    assert!(cut.is_empty(), "{strategy} cuts what it avoids: {cut:?}");
    (grouped, whole)
}

/// How far above the first ridge's top what `result` leaves stands, along
/// its middle.
fn left_on_the_first_ridge(result: &SetupResult) -> f64 {
    let stock = result.keyframes.last().unwrap().unpack().unwrap();
    let field = stock.field();
    let mut most = f64::NEG_INFINITY;
    for iy in 0..field.ny() {
        for ix in 0..field.nx() {
            let (Some(p), Some(h)) = (field.position(ix, iy), field.get(ix, iy)) else {
                continue;
            };
            if (p.x + 3.5).abs() <= 0.5 && p.y.abs() <= 6.0 {
                most = most.max(h - 1.0);
            }
        }
    }
    most
}

/// A finish over the area, the first ridge machined, finishes its top: no
/// more is left on it than its stepover's cusps, and the grid the material
/// is measured on.
fn finishes_the_first_ridges_top(strategy: &str) {
    let (grouped, _) = kept_to_the_first_ridge(strategy);
    let left = left_on_the_first_ridge(&grouped);
    assert!(left < 0.2, "{strategy} leaves {left} on the ridge's top");
}

#[test]
fn a_scallop_keeps_to_the_faces_it_machines() {
    finishes_the_first_ridges_top("scallop");
}

#[test]
fn a_spiral_keeps_to_the_faces_it_machines() {
    finishes_the_first_ridges_top("spiral");
}

#[test]
fn a_radial_keeps_to_the_faces_it_machines() {
    finishes_the_first_ridges_top("radial");
}

#[test]
fn a_morphed_spiral_keeps_to_the_faces_it_machines() {
    finishes_the_first_ridges_top("morphed-spiral");
}

/// Its passes between the two ridges' outer edges cross the first's top.
#[test]
fn a_morph_keeps_to_the_faces_it_machines() {
    finishes_the_first_ridges_top("morph");
}

/// The first ridge's edges are cut as they are with nothing avoided: the
/// tool rests on it there, the plate's clearance lower.
#[test]
fn a_project_keeps_to_the_faces_it_machines() {
    let (grouped, whole) = kept_to_the_first_ridge("project");
    let first = |r: &SetupResult| -> Vec<DVec3> {
        r.operations[0]
            .path
            .moves()
            .iter()
            .filter(|m| m.role == MoveRole::Cut && m.to.x < 0.0)
            .map(|m| m.to)
            .collect()
    };
    assert!(!first(&grouped).is_empty());
    assert_eq!(first(&grouped), first(&whole));
}

/// The lines at the first ridge's feet, where the tool touches it and the
/// plate's clearance, are cut, half a millimetre up; those at the second's,
/// where it touches nothing machined, are not.
#[test]
fn a_pencil_keeps_to_the_faces_it_machines() {
    let (grouped, _) = kept_to_the_first_ridge("pencil");
    // Where each cut crosses Y0, along the ridge's middle.
    let moves = grouped.operations[0].path.moves();
    let mut at = moves[0].to;
    let mut crossing = Vec::new();
    for m in moves {
        if m.role == MoveRole::Cut && (at.y < 0.0) != (m.to.y < 0.0) {
            crossing.push(at.lerp(m.to, at.y / (at.y - m.to.y)));
        }
        at = m.to;
    }
    for side in [-1.0, 1.0] {
        let line: Vec<&DVec3> = crossing
            .iter()
            .filter(|p| ((p.x + 3.5) * side - 2.3).abs() < 0.8)
            .collect();
        assert_eq!(line.len(), 1, "beside the ridge at {side}: {crossing:?}");
        assert!((line[0].z - 0.5).abs() < 0.1, "a line at {}", line[0]);
    }
}

/// How many times `op` lifted between its passes: the links it did not stay
/// down for.
fn lifts(op: &OperationResult) -> u64 {
    op.links.links_resolved - op.links.stayed_down
}

/// `strategy`, as a new one starts, run on the ridges with the first alone
/// machined, on 1 and 8 threads: the same moves either way.
fn new_on_ridges(strategy: &str) -> SetupResult {
    let body = ridges();
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let mut job = on_ridges(strategy, true);
    let op = &mut job.setups[0].operations[0];
    op.strategy = op.strategy.fresh();
    let on = |threads: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| run_setup(&job, 0, &bodies).unwrap())
    };
    let (one, eight) = (on(1), on(8));
    assert_eq!(
        one.operations[0].path, eight.operations[0].path,
        "{strategy}"
    );
    one
}

/// A radial or a morphed spiral cut into short pieces by the faces it
/// avoids, across the first ridge's top: no piece is ramped onto, and the
/// nearest is cut next, so the tool stays down from most to the next rather
/// than lifting for each.
fn pieces_are_entered_without_ramps_and_linked_nearest_next(strategy: &str) {
    let result = new_on_ridges(strategy);
    let op = &result.operations[0];
    assert_eq!(op.links.ramps, 0, "{strategy}");
    assert!(
        op.path.moves().iter().all(|m| m.role != MoveRole::Ramp),
        "{strategy}"
    );
    assert!(op.passes > 10, "{strategy}: {} passes", op.passes);
    assert!(
        2 * lifts(op) < op.passes,
        "{strategy}: {} lifts for {} passes",
        lifts(op),
        op.passes
    );
    assert_eq!(result.verify.gouged, 0, "{strategy}");
    assert_eq!(op.sim.rapid_contacts, 0, "{strategy}");
}

#[test]
fn a_radial_is_entered_without_ramps_and_linked_nearest_next() {
    pieces_are_entered_without_ramps_and_linked_nearest_next("radial");
}

#[test]
fn a_morphed_spiral_is_entered_without_ramps_and_linked_nearest_next() {
    pieces_are_entered_without_ramps_and_linked_nearest_next("morphed-spiral");
}

/// Which way each cut across the middle of the first ridge's top turns about
/// the spiral's centre, the part's middle: anticlockwise or clockwise, as
/// many of each.
fn turns_across_the_first_ridge(result: &SetupResult) -> (usize, usize) {
    let moves = result.operations[0].path.moves();
    let mut at = moves[0].to;
    let (mut anticlockwise, mut clockwise) = (0, 0);
    for m in moves {
        let across = |p: DVec3| (p.x + 3.5).abs() < 0.7 && p.y.abs() < 7.0;
        if m.role == MoveRole::Cut && across(at) && across(m.to) {
            let turn = at.truncate().perp_dot(m.to.truncate() - at.truncate());
            if turn > 1e-9 {
                anticlockwise += 1;
            } else if turn < -1e-9 {
                clockwise += 1;
            }
        }
        at = m.to;
    }
    (anticlockwise, clockwise)
}

/// A spiral cut into pieces across the first ridge, as a new one starts,
/// both ways: each piece taken from its nearer end, so they turn both ways
/// round, and the tool goes back and forth across the ridge. Asked to climb,
/// every piece turns the way the spiral does and the tool goes back to the
/// start of each, as Fusion's One Way: no fewer lifts.
#[test]
fn a_spiral_turns_its_pieces_round_only_where_it_cuts_both_ways() {
    let both = new_on_ridges("spiral");
    let (ccw, cw) = turns_across_the_first_ridge(&both);
    assert!(ccw > 5 && cw > 5, "both ways: {ccw} and {cw}");
    let body = ridges();
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let mut job = on_ridges("spiral", true);
    if let StrategySpec::Spiral { direction, .. } = &mut job.setups[0].operations[0].strategy {
        *direction = crate::job::CutDirection::Climb;
    }
    let climbing = run_setup(&job, 0, &bodies).unwrap();
    let (ccw, cw) = turns_across_the_first_ridge(&climbing);
    assert!(
        ccw.min(cw) == 0 && ccw.max(cw) > 10,
        "one way: {ccw} and {cw}"
    );
    assert!(lifts(&both.operations[0]) <= lifts(&climbing.operations[0]));
    assert_eq!(climbing.operations[0].links.ramps, 0);
}

/// A scallop finish keeping clear of faces gives the same moves on any
/// number of threads.
#[test]
fn grouped_scallops_are_deterministic() {
    let body = ridges();
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let job = on_ridges("scallop", true);
    let on = |threads: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| run_setup(&job, 0, &bodies).unwrap())
    };
    let (one, eight) = (on(1), on(8));
    assert!(!one.operations[0].path.is_empty());
    assert_eq!(one.operations[0].path, eight.operations[0].path);
}

/// A plate 16 across at Z0 with a boss 4 square and 3 tall standing in its
/// middle, with its faces named: the plate 0, the boss's top 1, its sides
/// facing X 2 and those facing Y 3.
fn boss() -> Body {
    let plate = synthetic::plane(0.0, 8.0);
    let boss = synthetic::cuboid(dvec3(-2.0, -2.0, 0.0), dvec3(2.0, 2.0, 3.0));
    // Its bottom, on the plate, is not part of it.
    let boss = TriMesh::new(boss.vertices().to_vec(), boss.triangles()[2..].to_vec()).unwrap();
    let mesh = TriMesh::merged(&[&plate, &boss]).unwrap();
    let mut labels = vec![0; plate.len()];
    // Its top, its sides facing -Y and +Y, then -X and +X.
    labels.extend([1, 1, 3, 3, 3, 3, 2, 2, 2, 2]);
    Body::with_faces(mesh, labels, 4).unwrap()
}

/// The boss's top, its sides facing X and those facing Y, and the plate, by
/// a point on each.
const BOSS_TOP: [f64; 3] = [0.0, 0.0, 3.0];
const BOSS_X: [f64; 3] = [2.0, 0.0, 1.5];
const BOSS_Y: [f64; 3] = [0.0, 2.0, 1.5];
const PLATE_BESIDE: [f64; 3] = [5.0, 5.0, 0.0];

/// A group of the boss's faces: what is done with them, the faces, and the
/// offsets `(radial, axial)`.
type Grouping<'a> = (SurfaceAction, &'a [[f64; 3]], (f64, f64));

/// `JOB`'s finish as a new operation of `strategy`, a stepdown of 1 mm, with
/// its 4 mm ball, or flat finishing with the 6 mm flat, its faces in
/// `groups` and the rest machined.
fn on_boss(strategy: &str, groups: &[Grouping<'_>]) -> Job {
    let mut job: Job = toml::from_str(JOB).unwrap();
    // Coarser than `JOB`, to be quick: none of it is near the tolerance.
    job.tolerance = 0.1;
    job.setups[0].operations.retain(|op| op.name == "finish");
    let op = &mut job.setups[0].operations[0];
    op.strategy = StrategySpec::named(strategy).unwrap();
    op.passes.stepdown = Some(1.0);
    op.passes.stepover = 1.0;
    if strategy == "flat" {
        op.tool = Some("rough".into());
    }
    op.surfaces.groups = groups
        .iter()
        .map(|&(action, faces, (radial, axial))| SurfaceGroup {
            action,
            radial,
            axial,
            faces: faces.iter().map(|&at| face(at)).collect(),
        })
        .collect();
    job
}

/// `strategy` on the boss, its faces as `groups` have them, and with all of
/// it machined. Run grouped, no move that feeds goes into the clearance of
/// what it avoids, no cut is where the tool would only rub it, and nothing
/// is cut into the part.
fn run_on_boss(strategy: &str, groups: &[Grouping<'_>]) -> (SetupResult, SetupResult) {
    let body = boss();
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let job = on_boss(strategy, groups);
    let grouped = run_setup(&job, 0, &bodies).unwrap();
    let whole = run_setup(&on_boss(strategy, &[]), 0, &bodies).unwrap();
    assert_eq!(into_clearance(&job, &bodies, &grouped), 0.0, "{strategy}");
    assert_eq!(grouped.verify.gouged, 0, "{strategy}");
    (grouped, whole)
}

/// The points `result` cuts to, beside the boss and on the plate.
fn cuts(result: &SetupResult) -> Vec<DVec3> {
    result.operations[0]
        .path
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Cut && m.to.x.abs() < 7.0 && m.to.y.abs() < 7.0)
        .map(|m| m.to)
        .collect()
}

/// How far `p` is from the boss, seen from above.
fn off_the_boss(p: DVec3) -> f64 {
    let off = (dvec2(p.x.abs(), p.y.abs()) - dvec2(2.0, 2.0)).max(DVec2::ZERO);
    if off.x > 0.0 && off.y > 0.0 {
        off.length()
    } else {
        off.x.max(off.y)
    }
}

/// Of `cuts`, those within the tolerance of height `z`.
fn at_height(cuts: &[DVec3], z: f64, tolerance: f64) -> Vec<DVec3> {
    cuts.iter()
        .copied()
        .filter(|p| (p.z - z).abs() <= tolerance)
        .collect()
}

/// The boss avoided, 1 mm sideways: what goes round it above its foot, which
/// machining all of it cuts, is cut out; the ring at its foot, where the
/// ball touches the plate machined and the boss's clearance at once, stays,
/// the ball its radius and the clearance off it. The cuts at the foot, run
/// grouped and machining all of it.
fn keeps_off_the_boss_but_round_its_foot(strategy: &str) -> (Vec<DVec3>, Vec<DVec3>) {
    let all = [BOSS_TOP, BOSS_X, BOSS_Y];
    let (grouped, whole) = run_on_boss(strategy, &[(SurfaceAction::Avoid, &all, (1.0, 0.0))]);
    let tol = 0.1;
    let high =
        |r: &SetupResult| -> Vec<DVec3> { cuts(r).into_iter().filter(|p| p.z > tol).collect() };
    assert!(!high(&whole).is_empty(), "{strategy} machining all of it");
    let left = high(&grouped);
    assert!(left.is_empty(), "{strategy} cuts round the boss: {left:?}");
    let foot = at_height(&cuts(&grouped), 0.0, tol);
    assert!(
        foot.len() > 10,
        "{strategy}: {} cuts at its foot",
        foot.len()
    );
    for p in &foot {
        let off = off_the_boss(*p);
        assert!(
            (off - 3.0).abs() < 2.0 * tol,
            "{strategy}: {p} is {off} off"
        );
    }
    (foot, at_height(&cuts(&whole), 0.0, tol))
}

/// Which way `points` go round, seen from above: twice the area they sweep,
/// anticlockwise positive.
fn turning(points: &[DVec3]) -> f64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| a.truncate().perp_dot(b.truncate()))
        .sum()
}

/// The plate avoided, half a millimetre up, and the boss machined: the boss
/// is still finished from above its top's edge down to the plate's
/// clearance, the ball touching its sides below where its middle is level
/// with the top, and what goes round it there, touching the boss and the
/// clearance at once, stays; nothing is cut lower.
fn keeps_above_the_plate_and_finishes_the_boss(strategy: &str) {
    let (grouped, _) = run_on_boss(
        strategy,
        &[(SurfaceAction::Avoid, &[PLATE_BESIDE], (0.0, 0.5))],
    );
    let tol = 0.1;
    let cut = cuts(&grouped);
    let lowest = cut.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
    assert!(
        (lowest - 0.5).abs() < tol,
        "{strategy} goes down to {lowest}"
    );
    assert!(
        cut.iter().any(|p| p.z > 1.5),
        "{strategy} finishes the top's edge"
    );
    let foot = at_height(&cut, 0.5, tol);
    assert!(
        foot.len() > 10,
        "{strategy}: {} cuts at its foot",
        foot.len()
    );
    for p in cut.iter().filter(|p| p.z <= 1.0) {
        let off = off_the_boss(*p);
        assert!((off - 2.0).abs() < tol, "{strategy}: {p} is {off} off");
    }
}

#[test]
fn a_contour_keeps_off_an_avoided_boss_but_round_its_foot() {
    keeps_off_the_boss_but_round_its_foot("contour");
}

#[test]
fn a_contour_keeps_above_an_avoided_floor() {
    keeps_above_the_plate_and_finishes_the_boss("contour");
}

/// Its way down the boss is cut out but for its last time round, at the
/// foot, which goes round the way it does machining all of it.
#[test]
fn a_ramp_keeps_off_an_avoided_boss_but_round_its_foot() {
    let (grouped, whole) = keeps_off_the_boss_but_round_its_foot("ramp");
    let (grouped, whole) = (turning(&grouped), turning(&whole));
    assert!(grouped * whole > 0.0, "{grouped} {whole}");
}

#[test]
fn a_ramp_keeps_above_an_avoided_floor() {
    keeps_above_the_plate_and_finishes_the_boss("ramp");
}

/// Flat finishing on the boss and the plate from the whole stock: a new one
/// goes straight down onto each ring, as Fusion's Flat starts; one whose
/// entry a job file leaves out ramps along its ring into the material, as it
/// always has. Both clear the same flats, every check passing.
#[test]
fn a_new_flat_plunges_and_one_read_ramps() {
    let body = boss();
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let mut job = on_boss("flat", &[]);
    let op = &mut job.setups[0].operations[0];
    op.linking.entry = StrategySpec::named("flat").unwrap().fresh_entry();
    let plunged = run_setup(&job, 0, &bodies).unwrap();
    job.setups[0].operations[0].linking.entry = None;
    let ramped = run_setup(&job, 0, &bodies).unwrap();
    let (p, r) = (&plunged.operations[0], &ramped.operations[0]);
    assert_eq!(p.links.ramps, 0);
    assert!(r.links.ramps > 0, "{:?}", r.links);
    assert!(p.links.links_resolved > 0);
    assert_eq!(cuts(&plunged), cuts(&ramped));
    for (result, op) in [(&plunged, p), (&ramped, r)] {
        assert_eq!(result.verify.gouged, 0);
        assert_eq!(op.sim.rapid_contacts, 0);
    }
}

/// A parallel finish whose job file still carries ramp settings, from when
/// it ramped into material, comes straight down into the stock as one
/// without: the settings are not read.
#[test]
fn a_parallel_with_ramp_settings_is_entered_as_one_without() {
    let body = step();
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let mut plain = finishing(&[], (0.0, 0.0));
    plain.setups[0].operations[0].surfaces = crate::job::Surfaces::default();
    let mut ramped = plain.clone();
    let linking = &mut ramped.setups[0].operations[0].linking;
    linking.ramp_angle = 5.0;
    linking.ramp_feed = Some(100.0);
    linking.ramp_stepdown = Some(0.5);
    ramped.validate().unwrap();
    let with = run_setup(&ramped, 0, &bodies).unwrap();
    let without = run_setup(&plain, 0, &bodies).unwrap();
    let op = &with.operations[0];
    assert_eq!(op.links.ramps, 0);
    assert!(op.path.moves().iter().all(|m| m.role != MoveRole::Ramp));
    assert_eq!(op.path, without.operations[0].path);
    assert_eq!(with.verify.gouged, 0);
    assert_eq!(op.sim.rapid_contacts, 0);
}

/// The boss's top avoided: flat finishing cuts nothing on it, which it
/// does machining all of it, and still clears the plate.
#[test]
fn a_flat_avoided_gets_no_passes() {
    let (grouped, whole) = run_on_boss("flat", &[(SurfaceAction::Avoid, &[BOSS_TOP], (0.0, 0.0))]);
    let tol = 0.1;
    assert!(!at_height(&cuts(&whole), 3.0, tol).is_empty());
    let on_top = at_height(&cuts(&grouped), 3.0, tol);
    assert!(on_top.is_empty(), "cuts on the top: {on_top:?}");
    assert!(at_height(&cuts(&grouped), 0.0, tol).len() > 10);
}

/// The boss's sides avoided 1 mm sideways: the plate is cleared round it,
/// the flat end mill its radius and the clearance off it.
#[test]
fn a_flat_keeps_its_clearance_off_an_avoided_wall() {
    let (grouped, whole) = run_on_boss(
        "flat",
        &[(SurfaceAction::Avoid, &[BOSS_X, BOSS_Y], (1.0, 0.0))],
    );
    let nearest = |r: &SetupResult| {
        at_height(&cuts(r), 0.0, 0.1)
            .into_iter()
            .map(off_the_boss)
            .fold(f64::INFINITY, f64::min)
    };
    let (grouped, whole) = (nearest(&grouped), nearest(&whole));
    assert!((whole - 3.0).abs() < 0.1, "machining all of it: {whole}");
    assert!((grouped - 4.0).abs() < 0.1, "{grouped}");
}

/// The plate machined with half a millimetre of stock of its own: flat
/// finishing clears it half a millimetre up, and the boss's top where it is.
#[test]
fn a_flat_machined_with_stock_of_its_own_is_cleared_above_it() {
    let (grouped, _) = run_on_boss(
        "flat",
        &[(SurfaceAction::Machine, &[PLATE_BESIDE], (0.0, 0.5))],
    );
    let cut = cuts(&grouped);
    assert!(at_height(&cut, 0.0, 0.1).is_empty());
    assert!(at_height(&cut, 0.5, 1e-6).len() > 10);
    assert!(!at_height(&cut, 3.0, 1e-6).is_empty());
}

/// The boss's sides facing Y avoided with no clearance, those facing X
/// machined: each ring round it is cut in two, either side, and the link
/// from one piece to the other at its level, though short, does not stay
/// down along the side avoided, rubbing it all the way, but lifts.
#[test]
fn a_contour_lifts_over_a_side_avoided() {
    let body = boss();
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let job = on_boss(
        "contour",
        &[(SurfaceAction::Avoid, &[BOSS_TOP, BOSS_Y], (0.0, 0.0))],
    );
    let result = run_setup(&job, 0, &bodies).unwrap();
    assert_eq!(into_clearance(&job, &bodies, &result), 0.0);
    let ring = at_height(&cuts(&result), 1.0, 1e-6);
    assert!(ring.iter().any(|p| p.x > 3.0) && ring.iter().any(|p| p.x < -3.0));
    let prepared = crate::setup::prepare(&job, 0, &bodies).unwrap();
    let op = &job.setups[0].operations[0];
    let surfaces = part_surfaces(&job, op, &prepared, &bodies).unwrap();
    let seat = surfaces.under(&cutting_tool(&job, op).unwrap()).unwrap();
    let moves = result.operations[0].path.moves();
    let mut at = moves[0].to;
    let mut rubbing = Vec::new();
    for m in moves {
        if m.role == MoveRole::Transition {
            for k in 0..=20 {
                let p = m.point_at(at, f64::from(k) / 20.0);
                if seat.avoids(p, job.tolerance) {
                    rubbing.push(p);
                }
            }
        }
        at = m.to;
    }
    assert!(
        rubbing.is_empty(),
        "links rubbing a side avoided: {rubbing:?}"
    );
}

/// A contour finish keeping clear of faces gives the same moves on any
/// number of threads.
#[test]
fn grouped_contours_are_deterministic() {
    let body = boss();
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let job = on_boss(
        "contour",
        &[(SurfaceAction::Avoid, &[BOSS_TOP, BOSS_Y], (0.5, 0.5))],
    );
    let on = |threads: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| run_setup(&job, 0, &bodies).unwrap())
    };
    let (one, eight) = (on(1), on(8));
    assert!(!one.operations[0].path.is_empty());
    assert_eq!(one.operations[0].path, eight.operations[0].path);
}

/// A parallel finish keeping clear of faces gives the same moves on any
/// number of threads.
#[test]
fn grouped_runs_are_deterministic() {
    let body = ridge();
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let job = finishing(&[RIDGE_TOP, RIDGE_SIDE], (0.5, 0.5));
    let on = |threads: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| run_setup(&job, 0, &bodies).unwrap())
    };
    let (one, eight) = (on(1), on(8));
    assert!(!one.operations[0].path.is_empty());
    assert_eq!(one.operations[0].path, eight.operations[0].path);
}
