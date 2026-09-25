//! Running a setup: the one engine path, and the one verification policy.

use camcraft_core::job::{Job, JobError};
use camcraft_core::run::{Made, Plan, SetupResult, Stage, run_prepared_planned};
use camcraft_core::setup::{Bodies, prepare};
use camcraft_geom::mesh::TriMesh;

use crate::checks::{self, Checks};
use crate::{Models, SessionError};

/// A setup, run.
#[derive(Debug, Clone)]
pub struct Ran {
    /// What the engine produced.
    pub result: SetupResult,
    /// The part as the setup machined it: its models, merged, turned and measured
    /// from its work zero.
    pub part: TriMesh,
    /// What happened, in order, for a log.
    pub notes: Vec<String>,
    /// Everything worth a person's attention: the setup's and its models'.
    pub warnings: Vec<String>,
    /// What the checks found. The gate is "no failures here".
    pub checks: Checks,
}

/// The one engine path: setup `index` of `job`, with its models.
pub fn run_setup(job: &Job, index: usize, models: &Models<'_>) -> Result<Ran, SessionError> {
    run_setup_planned(job, index, models, &[], &mut |_| {})
}

/// Whether setup `index` of `job` can run, and if not, why.
///
/// Only the operations the run will work out or keep are checked, those for which
/// `runs` holds by their place: one it skips is left out, and may be unfinished.
/// [`run_setup_planned`] asks this before computing anything, and the app asks it
/// to say why a run is not offered, so the two cannot disagree. The app asks every
/// frame, so it says which operations run without building the plans themselves.
pub fn check_runnable(
    job: &Job,
    index: usize,
    runs: &dyn Fn(usize) -> bool,
) -> Result<(), JobError> {
    job.check_common()?;
    job.check_setup_where(index, runs)
}

/// As [`run_setup`], doing with each operation what `plans` says, by its place
/// (see [`run_prepared_planned`]), and telling `progress` each [`Stage`] as it
/// starts. Only the operations that will run are checked beforehand.
pub fn run_setup_planned(
    job: &Job,
    index: usize,
    models: &Models<'_>,
    plans: &[Plan],
    progress: &mut dyn FnMut(Stage),
) -> Result<Ran, SessionError> {
    // Validation happens before anything is computed, so a bad job fails on its own
    // terms rather than part-way through an operation.
    check_runnable(job, index, &|i| !matches!(plans.get(i), Some(Plan::Skip)))?;
    let setup = job.setups.get(index).ok_or(JobError::NoSetups)?;
    let mut notes = vec![format!(
        "setup '{}': {} operation(s), tolerance {} mm",
        setup.name,
        setup.operations.len(),
        job.tolerance
    )];
    let bodies: Bodies<'_> = models.iter().map(|(n, m)| (*n, &m.body)).collect();
    progress(Stage::Preparing);
    let prepared = prepare(job, index, &bodies)?;
    let result = run_prepared_planned(job, index, &prepared, &bodies, plans, progress)?;
    notes.extend(summary(&result));

    let mut model_warnings = Vec::new();
    for m in job.models_for(index) {
        if let Some(model) = models.get(m.name.as_str()) {
            model_warnings.extend(model.warnings.iter().map(|w| format!("{}: {w}", m.name)));
        }
    }
    progress(Stage::Checking);
    let checks = checks::check(
        job,
        index,
        &prepared.part,
        &result.operations,
        &result.verify,
        result.verify_budget,
        &model_warnings,
    );
    let mut warnings = result.warnings();
    warnings.extend(model_warnings);
    Ok(Ran {
        result,
        part: prepared.part,
        notes,
        warnings,
        checks,
    })
}

/// A line per operation and one for the verdict.
fn summary(result: &SetupResult) -> Vec<String> {
    let mut out: Vec<String> = result
        .operations
        .iter()
        .map(|op| match op.made {
            Made::Skipped => format!("{}: not generated", op.name),
            Made::Disabled => format!("{}: turned off", op.name),
            Made::Generated | Made::Kept => format!(
                "{}: {} passes, removed {:.0} mm3, travel saved {:.0}%, rapid contacts {}{}",
                op.name,
                op.passes,
                op.sim.removed_mm3,
                op.travel_saved * 100.0,
                op.sim.rapid_contacts,
                if op.made == Made::Kept {
                    ", kept from before"
                } else {
                    ""
                }
            ),
        })
        .collect();
    out.push(format!(
        "verified: {} column(s) compared, gouges {}, deepest {:.3} mm, budget {:.3} mm",
        result.verify.compared,
        result.verify.gouged,
        result.verify.deepest_gouge_mm,
        result.verify_budget
    ));
    out
}

/// How a setup got past the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Passed {
    /// It verified.
    Verified,
    /// It did not, and the caller insisted.
    Forced,
}

impl Ran {
    /// Whether it may be posted: it verified, and no check fails.
    pub fn passed(&self) -> bool {
        self.result.is_clean() && self.checks.passed()
    }
}

/// The one verification policy: no program from a setup that did not verify, or
/// that any check fails, unless the caller insists.
pub fn gate(ran: &Ran, force: bool) -> Result<Passed, SessionError> {
    if ran.passed() {
        Ok(Passed::Verified)
    } else if force {
        Ok(Passed::Forced)
    } else {
        Err(SessionError::Refused)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::indexing_slicing)]
mod tests {
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
    /// is written as version 2 and read back the same.
    #[test]
    fn a_real_part_kept_to_a_picked_face() {
        use camcraft_core::job::{FeatureRef, Origin, PointRef, Snap};
        use camcraft_core::toolpath::MoveRole;
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
        job.setups[0].operations[0].geometry.faces.push(FeatureRef {
            model: "hinge".into(),
            index: face,
            at: at.to_array(),
        });
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
                p.x >= lo.x - 0.03
                    && p.x <= hi.x + 0.03
                    && p.y >= lo.y - 0.03
                    && p.y <= hi.y + 0.03,
                "a cut at {p} leaves the face ({lo} to {hi})"
            );
        }

        let text = job_text(&job).unwrap();
        assert!(
            text.contains(
                "version = 2
"
            ),
            "{text}"
        );
        assert_eq!(parse_job(&text, "picked").unwrap(), job);
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
        use camcraft_core::toolpath::MoveRole;
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
}
