use std::path::Path;

use camcraft_core::job::StrategySpec;
use camcraft_core::run::Kept;

use super::*;
use crate::test_support::{recheck, run_small, small_job};
use crate::{job_text, load_job_model, parse_job};

/// One operation generated alone, the other kept: the kept one is cut into
/// the material again, so the result is what generating both gives. One
/// skipped or turned off has no paths and cuts nothing, and only what runs
/// is checked, so a skipped operation may be unfinished.
#[test]
fn operations_are_kept_skipped_and_turned_off() {
    let mut job = small_job();
    let mut second = job.setups[0].operations[0].clone();
    second.name = "across".into();
    second.strategy = StrategySpec::Parallel { angle: 90.0 };
    job.setups[0].operations.push(second);
    let model = load_job_model(Path::new("."), &job.models[0], job.tolerance).unwrap();
    let models: Models<'_> = [("hinge", &model)].into_iter().collect();
    let final_heights = |ran: &Ran| {
        let last = ran.result.keyframes.last().unwrap().unpack().unwrap();
        last.field().data().to_vec()
    };

    let both = run_setup(&job, 0, &models).unwrap();
    assert!(
        both.result
            .operations
            .iter()
            .all(|o| o.made == Made::Generated)
    );

    // Kept, cut again; and kept with the material it left, put back: the
    // same either way.
    for after in [None, both.result.keyframes.get(1).cloned()] {
        let first = Box::new(Kept {
            result: both.result.operations[0].clone(),
            after,
        });
        let plans = [Plan::Keep(first), Plan::Generate];
        let kept = run_setup_planned(&job, 0, &models, &plans, &mut |_| {}).unwrap();
        assert_eq!(kept.result.operations[0].made, Made::Kept);
        assert_eq!(kept.result.operations[1].made, Made::Generated);
        assert_eq!(
            kept.result.operations[1].path.moves(),
            both.result.operations[1].path.moves()
        );
        assert_eq!(kept.result.keyframes[1], both.result.keyframes[1]);
        assert_eq!(final_heights(&kept), final_heights(&both));
    }

    // The second's passes, linked again over the material the first leaves:
    // the same program, and the same material, as working them out again.
    let planned = both.result.operations[1].planned.clone().unwrap();
    let plans = [Plan::Generate, Plan::Relink(planned)];
    let relinked = run_setup_planned(&job, 0, &models, &plans, &mut |_| {}).unwrap();
    assert_eq!(relinked.result.operations[1].made, Made::Relinked);
    assert_eq!(
        relinked.result.operations[1].path.moves(),
        both.result.operations[1].path.moves()
    );
    assert_eq!(final_heights(&relinked), final_heights(&both));

    // Skipped: nothing of it, and it need not be finished to run the other.
    job.setups[0].operations[0].tool = None;
    let plans = [Plan::Skip, Plan::Generate];
    let skipped = run_setup_planned(&job, 0, &models, &plans, &mut |_| {}).unwrap();
    assert_eq!(skipped.result.operations[0].made, Made::Skipped);
    assert!(skipped.result.operations[0].path.is_empty());
    assert_eq!(skipped.result.operations[0].sim.removed_mm3, 0.0);
    assert!(skipped.checks.groups.iter().all(|g| g.op != Some(0)));
    assert!(
        run_setup(&job, 0, &models).is_err(),
        "unfinished, it cannot run"
    );

    // Turned off: left out of any run, whatever the plan.
    job.setups[0].operations[0].enabled = false;
    let off = run_setup(&job, 0, &models).unwrap();
    assert_eq!(off.result.operations[0].made, Made::Disabled);
    assert!(off.result.operations[0].path.is_empty());
    assert_eq!(off.result.operations[1].made, Made::Generated);
}

/// A real part, finished only on its highest flat face, with its work zero on
/// a corner of that face: it runs and verifies clean, every cut keeps the
/// tool's centre over the face, the corner is the setup's zero, and the job
/// is written and read back the same.
#[test]
fn a_real_part_kept_to_a_picked_face() {
    use camcraft_core::job::{FeatureRef, Origin, PointRef, Snap};
    use camcraft_core::path::toolpath::MoveRole;
    use camcraft_geom::topology::FaceKind;
    let mut job = small_job();
    let model = load_job_model(Path::new("."), &job.models[0], job.tolerance).unwrap();
    let (mesh, t) = (model.mesh(), model.body.topology());
    let (face, _) = t
        .faces()
        .iter()
        .enumerate()
        .filter_map(|(i, f)| match f.kind {
            FaceKind::Plane { normal, offset } if normal.z > 0.99 && f.area > 20.0 => {
                Some((u32::try_from(i).ok()?, offset))
            }
            _ => None,
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    let f = t.face(face).unwrap();
    let tri = mesh.triangle(f.triangles[0] as usize).unwrap();
    let at = (tri[0] + tri[1] + tri[2]) / 3.0;
    job.setups[0].operations[0]
        .selection
        .faces
        .push(FeatureRef {
            model: "hinge".into(),
            index: face,
            at: at.to_array(),
        });
    job.setups[0].operations[0].boundary.kind = camcraft_core::job::Boundary::Selection;
    let corner = t
        .edge(f.edges[0])
        .and_then(|e| e.ends)
        .map(|[a, _]| a)
        .unwrap();
    let position = t.vertex(corner).unwrap().position;
    job.setups[0].origin = Origin::Point {
        point: PointRef {
            model: "hinge".into(),
            snap: Snap::Vertex,
            index: corner,
            at: position.to_array(),
        },
        offset: [0.0; 3],
    };
    job.validate().unwrap();

    let models: Models<'_> = [("hinge", &model)].into_iter().collect();
    let ran = run_setup(&job, 0, &models).unwrap();
    assert!(ran.result.is_clean(), "{:?}", ran.warnings);
    let frame = ran.result.frame;
    assert!(frame.to_setup(position).length() < 1e-9);

    // The face's extent in the setup, which every cut must keep over.
    let (lo, hi) = f
        .triangles
        .iter()
        .filter_map(|&i| mesh.triangle(i as usize))
        .flatten()
        .map(|p| frame.to_setup(p).truncate())
        .fold(
            (glam::DVec2::splat(f64::MAX), glam::DVec2::splat(f64::MIN)),
            |(lo, hi), p| (lo.min(p), hi.max(p)),
        );
    let cuts: Vec<_> = ran.result.operations[0]
        .path
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Cut)
        .map(|m| m.to.truncate())
        .collect();
    assert!(cuts.len() > 10, "only {} cuts", cuts.len());
    for p in cuts {
        assert!(
            p.x >= lo.x - 0.03 && p.x <= hi.x + 0.03 && p.y >= lo.y - 0.03 && p.y <= hi.y + 0.03,
            "a cut at {p} leaves the face ({lo} to {hi})"
        );
    }

    let text = job_text(&job).unwrap();
    assert_eq!(parse_job(&text, "picked").unwrap(), job);
}

/// Three copies of a part, each moved, turned and scaled its own way, one
/// unevenly, finished on each one's highest flat face with the work zero on a
/// corner of the second: the picks, saved where the file has them, are found
/// on each placed copy; every face is cut over where it now is, the work zero
/// is the placed corner, and the run verifies clean against all three.
#[test]
fn several_placed_models_are_cut_where_they_are() {
    use camcraft_core::job::{Boundary, FeatureRef, ModelDef, Origin, PointRef, Snap, Transform};
    use camcraft_core::path::toolpath::MoveRole;
    use camcraft_geom::topology::FaceKind;
    let mut job = small_job();
    let hinge = job.models[0].clone();
    let places = [
        ("a", [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0; 3]),
        ("b", [90.0, 10.0, 3.0], [0.0, 0.0, 90.0], [0.8; 3]),
        ("c", [30.0, 90.0, -2.0], [0.0, 0.0, -45.0], [1.2, 1.0, 0.7]),
    ];
    job.models = places
        .iter()
        .map(|&(name, position, rotation, scale)| ModelDef {
            name: name.into(),
            transform: Transform {
                position,
                rotation,
                scale,
            },
            ..hinge.clone()
        })
        .collect();
    job.setups[0].models = places.iter().map(|p| p.0.to_string()).collect();
    let loaded: Vec<_> = job
        .models
        .iter()
        .map(|m| load_job_model(Path::new("."), m, job.tolerance).unwrap())
        .collect();

    // The highest flat face up, from the file, the same on every copy.
    let file = &loaded[0].file;
    let (mesh, t) = (file.mesh(), file.topology());
    let face = t
        .faces()
        .iter()
        .enumerate()
        .filter_map(|(i, f)| match f.kind {
            FaceKind::Plane { normal, offset } if normal.z > 0.99 && f.area > 20.0 => {
                Some((u32::try_from(i).ok()?, offset))
            }
            _ => None,
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap()
        .0;
    let f = t.face(face).unwrap();
    let tri = mesh.triangle(f.triangles[0] as usize).unwrap();
    let at = ((tri[0] + tri[1] + tri[2]) / 3.0).to_array();
    let op = &mut job.setups[0].operations[0];
    op.selection.faces = places
        .iter()
        .map(|p| FeatureRef {
            model: p.0.into(),
            index: face,
            at,
        })
        .collect();
    op.boundary.kind = Boundary::Selection;
    let corner = t
        .edge(f.edges[0])
        .and_then(|e| e.ends)
        .map(|[a, _]| a)
        .unwrap();
    job.setups[0].origin = Origin::Point {
        point: PointRef {
            model: "b".into(),
            snap: Snap::Vertex,
            index: corner,
            at: t.vertex(corner).unwrap().position.to_array(),
        },
        offset: [0.0; 3],
    };
    job.validate().unwrap();

    let models: Models<'_> = places.iter().map(|p| p.0).zip(&loaded).collect();
    let ran = run_setup(&job, 0, &models).unwrap();
    assert!(ran.result.is_clean(), "{:?}", ran.warnings);
    let frame = ran.result.frame;
    // Corners are numbered by where they are, so the turned copy's differ:
    // the pick is found by its point.
    let placed_corner = loaded[1]
        .body
        .placement()
        .transform_point3(t.vertex(corner).unwrap().position);
    assert!(frame.to_setup(placed_corner).length() < 1e-9);

    let cuts: Vec<_> = ran.result.operations[0]
        .path
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Cut)
        .map(|m| m.to.truncate())
        .collect();
    for (model, (name, ..)) in loaded.iter().zip(&places) {
        // The placed face's extent in the setup.
        let body = &model.body;
        let placed = body.topology().face(face).unwrap();
        let (lo, hi) = placed
            .triangles
            .iter()
            .filter_map(|&i| body.mesh().triangle(i as usize))
            .flatten()
            .map(|p| frame.to_setup(p).truncate())
            .fold(
                (glam::DVec2::splat(f64::MAX), glam::DVec2::splat(f64::MIN)),
                |(lo, hi), p| (lo.min(p), hi.max(p)),
            );
        let over = cuts
            .iter()
            .filter(|p| p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y)
            .count();
        assert!(over > 10, "'{name}': only {over} cuts over its face");
    }
    let text = job_text(&job).unwrap();
    assert_eq!(parse_job(&text, "placed").unwrap(), job);
}

/// The gate is the one policy both front ends share: a clean job passes, and
/// an unclean one passes only when forced.
#[test]
fn the_gate_refuses_an_unverified_job_unless_forced() {
    let mut ran = run_small();
    assert_eq!(gate(&ran, false).unwrap(), Passed::Verified);

    // Make it unclean the way the simulator would: a rapid through material.
    let op = ran.result.operations.first_mut().unwrap();
    op.sim.rapid_contacts = 1;
    assert!(matches!(gate(&ran, false), Err(SessionError::Refused)));
    assert_eq!(gate(&ran, true).unwrap(), Passed::Forced);
}

/// The gate is "no failures": a path the engine is happy with, but the checks
/// are not, is refused.
#[test]
fn a_failed_check_closes_the_gate() {
    use camcraft_core::path::toolpath::MoveRole;
    use glam::dvec3;
    let mut ran = run_small();
    assert!(ran.result.is_clean());
    ran.checks = recheck(&ran, |p| {
        let at = p.moves().last().unwrap().to;
        p.feed_to(at + dvec3(3.0, 0.0, -1.0), MoveRole::Plunge, 300.0)
            .unwrap();
    });
    assert!(matches!(gate(&ran, false), Err(SessionError::Refused)));
    assert_eq!(gate(&ran, true).unwrap(), Passed::Forced);
}

/// Watched, a run tells what it has made each time it makes an operation:
/// one more each time, the material after each, never finished and never
/// passing; and the result at the end is finished.
#[test]
fn a_watched_run_tells_what_it_has_made_so_far() {
    let mut job = small_job();
    let mut second = job.setups[0].operations[0].clone();
    second.name = "across".into();
    second.strategy = StrategySpec::Parallel { angle: 90.0 };
    job.setups[0].operations.push(second);
    let model = load_job_model(Path::new("."), &job.models[0], job.tolerance).unwrap();
    let models: Models<'_> = [("hinge", &model)].into_iter().collect();
    let control = Control::new(2);
    let mut so_far = Vec::new();
    let ran = run_setup_watched(
        &job,
        0,
        &models,
        &[],
        LaidOut::Nothing,
        &control,
        &mut |u| {
            if let Underway::SoFar(ran) = u {
                so_far.push(ran);
            }
        },
    )
    .unwrap();
    assert_eq!(so_far.len(), 2);
    for (k, partial) in so_far.iter().enumerate() {
        assert_eq!(partial.result.operations.len(), k + 1);
        assert_eq!(partial.result.keyframes.len(), k + 2);
        assert!(!partial.finished && !partial.passed());
    }
    assert!(ran.finished);
    assert_eq!(
        so_far[1].result.operations[1].path,
        ran.result.operations[1].path
    );
}

/// Stopped from its second operation, a watched run has told its first, and
/// ends stopped.
#[test]
fn a_watched_run_stopped_ends_stopped_with_what_it_made_told() {
    let mut job = small_job();
    let mut second = job.setups[0].operations[0].clone();
    second.name = "across".into();
    job.setups[0].operations.push(second);
    let model = load_job_model(Path::new("."), &job.models[0], job.tolerance).unwrap();
    let models: Models<'_> = [("hinge", &model)].into_iter().collect();
    let control = Control::new(2);
    control.stop_from(1);
    let mut told = 0;
    let err = run_setup_watched(
        &job,
        0,
        &models,
        &[],
        LaidOut::Nothing,
        &control,
        &mut |u| {
            if let Underway::SoFar(ran) = u {
                told = ran.result.operations.len();
            }
        },
    )
    .unwrap_err();
    assert!(err.is_stopped(), "{err}");
    assert_eq!(told, 1);
}

/// Fusion's Bore and Circular, each a new operation as the app makes one, cut
/// the part made in Fusion for the basic operations (`ops_block.step`): its
/// round pocket, 24 mm across and 8 deep, with a 6 mm flat end mill in
/// multiple passes, and its two blind holes, 6 mm across, with a 4 mm one, after the
/// stock's top is faced. Every check passes, every program reads back with
/// its circles as arcs, and the material each leaves is the hole it was
/// picked by, to the job's tolerance.
#[test]
fn bore_and_circular_cut_the_holes_they_are_picked_by() {
    use camcraft_core::job::{FeatureRef, Operation};

    use crate::test_support::fixture;
    use crate::{load_models, models_of, program, setup_profile};

    let model = fixture("ops_block.step")
        .display()
        .to_string()
        .replace('\\', "/");
    let setup = |name: &str| {
        format!(
            "\n[[setup]]\nname = \"{name}\"\nmodels = [\"part\"]\nstocks = [\"block\"]\npost = \"masso\"\n"
        )
    };
    let text = format!(
        r#"
version = 1
tolerance = 0.01

[tools.flat6]
kind = "flat"
diameter = 6.0
number = 1
rpm = 18000
flutes = 3
flute_length = 20.0
shoulder_length = 20.0
shank_diameter = 6.0
overall_length = 63.0
stickout = 22.5

[tools.flat4]
kind = "flat"
diameter = 4.0
number = 2
rpm = 18000
flutes = 2
flute_length = 14.0
shoulder_length = 16.0
shank_diameter = 4.0
overall_length = 50.0
stickout = 20.0

[[model]]
name = "part"
path = "{model}"

[[stock]]
name = "block"
kind = "around-models"
margin = [5.0, 5.0, 2.0]
{}{}"#,
        setup("bore"),
        setup("circular")
    );
    let mut job = parse_job(&text, "ops_block").unwrap();
    let pick = |at: [f64; 3]| FeatureRef {
        model: "part".into(),
        index: 0,
        at,
    };
    for (i, strategy) in ["bore", "circular"].into_iter().enumerate() {
        let make = |name: &str, strategy: &str, tool: &str| {
            let mut op = Operation::fresh(
                name,
                StrategySpec::named(strategy).unwrap(),
                (Some(tool.to_string()), job.tools.get(tool), None),
            );
            op.passes.feed = 1500.0;
            op
        };
        let face = make("face", "face", "flat6");
        let mut pocket = make("pocket", strategy, "flat6");
        pocket.selection.faces = vec![pick([37.0, 10.0, 16.0])];
        // Multiple passes, three, half the tool apart: in to its middle.
        if let StrategySpec::Bore {
            multiple_passes,
            stepovers,
            ..
        }
        | StrategySpec::Circular {
            multiple_passes,
            stepovers,
            ..
        } = &mut pocket.strategy
        {
            *multiple_passes = true;
            *stepovers = 3;
        }
        let mut holes = make("holes", strategy, "flat4");
        holes.selection.faces = vec![pick([18.0, -22.0, 14.0]), pick([38.0, -22.0, 14.0])];
        job.setups[i].operations = vec![face, pocket, holes];
    }
    // Read back, so its expressions are worked out as a job read from disk has
    // them.
    let job = parse_job(&job_text(&job).unwrap(), "ops_block").unwrap();
    job.validate().unwrap();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let loaded = load_models(&job, dir, &[0, 1]).unwrap();
    // The round pocket, and the holes, each a centre, radius and floor.
    let walls = [
        (1, vec![([25.0, 10.0], 12.0, 12.0)]),
        (
            2,
            vec![([15.0, -22.0], 3.0, 8.0), ([35.0, -22.0], 3.0, 8.0)],
        ),
    ];
    for (i, strategy) in ["bore", "circular"].into_iter().enumerate() {
        let ran = run_setup(&job, i, &models_of(&loaded)).unwrap();
        assert!(
            ran.checks.failures().count() == 0,
            "{strategy}: {:?}",
            ran.checks
        );
        assert_eq!(ran.result.verify.gouged, 0, "{strategy}");
        let profile = setup_profile(&job.setups[i], dir, None).unwrap();
        for (index, holes) in &walls {
            let op = &ran.result.operations[*index];
            assert_eq!(op.strategy, strategy);
            assert_eq!(op.sim.rapid_contacts, 0, "{strategy}/{}", op.name);
            assert!(
                op.warnings.is_empty(),
                "{strategy}/{}: {:?}",
                op.name,
                op.warnings
            );
            let (program, _) = program(op, 54, &profile, &op.name).unwrap();
            let arcs = program
                .lines()
                .filter(|l| l.contains("G2") || l.contains("G3"))
                .count();
            assert!(arcs > 10, "{strategy}/{}: {arcs} arcs\n{program}", op.name);
            // Nothing is left in the hole, above its floor, but where the grid
            // meets its wall.
            let stock = ran.result.keyframes[index + 1].unpack().unwrap();
            let field = stock.field();
            let part = &ran.result.part;
            for &(centre, radius, floor) in holes {
                let centre = glam::DVec2::from_array(centre);
                let mut inside = 0;
                for iy in 0..field.ny() {
                    for ix in 0..field.nx() {
                        let p = field.position(ix, iy).unwrap();
                        if p.distance(centre) > radius - 2.0 * field.pitch() {
                            continue;
                        }
                        inside += 1;
                        let left = field.get(ix, iy).unwrap();
                        let under = part.height_at(p.x, p.y).unwrap();
                        assert!(
                            (under - floor).abs() < 1e-3,
                            "{strategy}: the part at {p} is at {under}"
                        );
                        assert!(
                            left - under <= job.tolerance,
                            "{strategy}/{}: {:.4} left at {p}",
                            op.name,
                            left - under
                        );
                    }
                }
                assert!(inside > 100, "{strategy}: {inside} cells in the hole");
            }
        }
    }
}

/// Fusion's Trace and Slot, each a new operation as the app makes one, on the
/// part made in Fusion for the basic operations (`ops_block.step`). The trace
/// follows the edges round its pocket's floor, 8 mm down, a 6 mm flat end mill
/// kept to their left, inside: every check passes, its program has the
/// pocket's round corners as arcs, and it cuts the band beside the pocket's
/// wall down to its floor. The block has no slot: a slot picked by the same
/// edges is refused, as the loop of a pocket, not a slot.
#[test]
fn trace_follows_a_pocket_floor_and_slot_refuses_it() {
    use camcraft_core::job::{Compensation, FeatureRef, Operation};

    use crate::test_support::fixture;
    use crate::{load_models, models_of, program, setup_profile};

    let model = fixture("ops_block.step")
        .display()
        .to_string()
        .replace('\\', "/");
    let text = format!(
        r#"
version = 1
tolerance = 0.01

[tools.flat6]
kind = "flat"
diameter = 6.0
number = 1
rpm = 18000
flutes = 3
flute_length = 20.0
shoulder_length = 20.0
shank_diameter = 6.0
overall_length = 63.0
stickout = 22.5

[[model]]
name = "part"
path = "{model}"

[[stock]]
name = "block"
kind = "around-models"
margin = [5.0, 5.0, 2.0]

[[setup]]
name = "trace"
models = ["part"]
stocks = ["block"]
post = "masso"
"#
    );
    let mut job = parse_job(&text, "ops_block").unwrap();
    // The pocket's floor, 14 mm up, from -40 to 0 in X and -18 to 18 in Y,
    // its corners 6 mm round: a point on each of its eight edges.
    let s = 6.0 * std::f64::consts::FRAC_1_SQRT_2;
    let edges: Vec<FeatureRef> = [
        [-20.0, 18.0],
        [0.0, 0.0],
        [-20.0, -18.0],
        [-40.0, 0.0],
        [-34.0 - s, 12.0 + s],
        [-6.0 + s, 12.0 + s],
        [-34.0 - s, -12.0 - s],
        [-6.0 + s, -12.0 - s],
    ]
    .into_iter()
    .map(|[x, y]| FeatureRef {
        model: "part".into(),
        index: 0,
        at: [x, y, 14.0],
    })
    .collect();
    let mut trace = Operation::fresh(
        "trace",
        StrategySpec::named("trace").unwrap(),
        (Some("flat6".into()), job.tools.get("flat6"), None),
    );
    trace.passes.feed = 1500.0;
    trace.selection.edges.clone_from(&edges);
    if let StrategySpec::Trace { compensation, .. } = &mut trace.strategy {
        *compensation = Compensation::Left;
    }
    job.setups[0].operations = vec![trace];
    // Read back, so its expressions are worked out as a job read from disk has
    // them.
    let job = parse_job(&job_text(&job).unwrap(), "ops_block").unwrap();
    job.validate().unwrap();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let loaded = load_models(&job, dir, &[0]).unwrap();
    let ran = run_setup(&job, 0, &models_of(&loaded)).unwrap();
    assert!(ran.checks.failures().count() == 0, "{:?}", ran.checks);
    assert_eq!(ran.result.verify.gouged, 0);
    let op = &ran.result.operations[0];
    assert_eq!(op.strategy, "trace");
    assert_eq!(op.sim.rapid_contacts, 0);
    assert!(op.warnings.is_empty(), "{:?}", op.warnings);
    let profile = setup_profile(&job.setups[0], dir, None).unwrap();
    let (program, _) = program(op, 54, &profile, &op.name).unwrap();
    let arcs = program
        .lines()
        .filter(|l| l.contains("G2") || l.contains("G3"))
        .count();
    assert!(arcs >= 4, "{arcs} arcs\n{program}");
    // How far inside the pocket's wall a point is, seen from above.
    let inside = |p: glam::DVec2| {
        let q = (p - glam::dvec2(-20.0, 0.0)).abs() - glam::dvec2(20.0 - 6.0, 18.0 - 6.0);
        let d = q.max(glam::DVec2::ZERO).length() + q.x.max(q.y).min(0.0) - 6.0;
        -d
    };
    let stock = ran.result.keyframes[1].unpack().unwrap();
    let field = stock.field();
    let edge = 2.0 * field.pitch();
    let mut band = 0;
    for iy in 0..field.ny() {
        for ix in 0..field.nx() {
            let p = field.position(ix, iy).unwrap();
            let d = inside(p);
            if d > edge && d < 6.0 - edge {
                band += 1;
                let left = field.get(ix, iy).unwrap();
                assert!(left <= 14.0 + job.tolerance, "{left:.4} left at {p}");
            }
        }
    }
    assert!(band > 100, "{band} cells in the band");

    // The same edges picked for a slot: a pocket's floor, not a slot's.
    let mut slotted = job.clone();
    let mut slot = Operation::fresh(
        "slot",
        StrategySpec::named("slot").unwrap(),
        (Some("flat6".into()), job.tools.get("flat6"), None),
    );
    slot.selection.edges = edges;
    slotted.setups[0].operations = vec![slot];
    let err = run_setup(&slotted, 0, &models_of(&loaded)).unwrap_err();
    assert!(err.to_string().contains("is not a slot"), "{err}");
}
