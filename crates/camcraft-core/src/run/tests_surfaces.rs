//! Finishing with faces machined and faces avoided.

use super::*;
use crate::arrange::Stand;
use crate::fixture::JOB;
use crate::job::{FeatureRef, SurfaceGroup};
use crate::path::toolpath::{MoveRole, Toolpath};
use camcraft_geom::mesh::TriMesh;
use camcraft_geom::synthetic;
use camcraft_geom::topology::Body;
use glam::{DVec3, dvec3};

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
