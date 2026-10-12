use std::path::Path;

use camcraft_core::job::StrategySpec;

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

    let first = Box::new(both.result.operations[0].clone());
    let plans = [Plan::Keep(first), Plan::Generate];
    let kept = run_setup_planned(&job, 0, &models, &plans, &mut |_| {}).unwrap();
    assert_eq!(kept.result.operations[0].made, Made::Kept);
    assert_eq!(kept.result.operations[1].made, Made::Generated);
    assert_eq!(
        kept.result.operations[1].path.moves(),
        both.result.operations[1].path.moves()
    );
    assert_eq!(final_heights(&kept), final_heights(&both));

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
