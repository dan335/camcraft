//! Running a setup: the one engine path, and the one verification policy.

use camcraft_core::control::Control;
use camcraft_core::job::{Job, JobError};
use camcraft_core::run::{Made, Plan, Progress, SetupResult, Stage, run_prepared_planned};
use camcraft_core::setup::{Bodies, prepare};
use camcraft_geom::mesh::TriMesh;
use camcraft_geom::shadow::Shadow;

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
    /// Whether the run got to the end: every operation made, the material
    /// verified and the checks run. What a run tells while it goes is not,
    /// and never passes ([`Ran::passed`]).
    pub finished: bool,
}

/// What a setup's run tells as it goes ([`run_setup_watched`]).
#[derive(Debug, Clone)]
pub enum Underway {
    /// It has started on a [`Stage`].
    Stage(Stage),
    /// What it has made so far: every operation made, in order, with the
    /// material after each, drawn against the part as the finished result
    /// will be; neither verified nor checked, so not [`Ran::finished`].
    SoFar(Box<Ran>),
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
    let operations = job.setups.get(index).map_or(0, |s| s.operations.len());
    let control = Control::new(operations);
    run_setup_watched(job, index, models, plans, None, &control, &mut |u| {
        if let Underway::Stage(stage) = u {
            progress(stage);
        }
    })
}

/// As [`run_setup_planned`], as `control` lets it and telling it how far each
/// operation has got, and telling `underway` each stage it starts and what it
/// has made so far, each time an operation is made. Stopped, it ends with
/// an error that [`SessionError::is_stopped`]; what it made before was told.
///
/// The part's shadow from above, already cast in the setup's turned frame, is
/// `turned`, as
/// [`Prepared::turned_shadow`](camcraft_core::setup::Prepared::turned_shadow)
/// gives it for the same models turned the same way: taken rather than cast
/// again.
pub fn run_setup_watched(
    job: &Job,
    index: usize,
    models: &Models<'_>,
    plans: &[Plan],
    turned: Option<&Shadow>,
    control: &Control,
    underway: &mut dyn FnMut(Underway),
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
    underway(Underway::Stage(Stage::Preparing));
    let prepared = prepare(job, index, &bodies)?;
    if let Some(turned) = turned {
        prepared.adopt_shadow(turned);
    }
    let mut model_warnings = Vec::new();
    for m in job.models_for(index) {
        if let Some(model) = models.get(m.name.as_str()) {
            model_warnings.extend(model.warnings.iter().map(|w| format!("{}: {w}", m.name)));
        }
    }
    // What it has made so far, grown as each operation is made.
    let mut so_far: Option<SetupResult> = None;
    let result =
        run_prepared_planned(
            job,
            index,
            &prepared,
            &bodies,
            plans,
            control,
            &mut |p| match p {
                Progress::Stage(stage) => underway(Underway::Stage(stage)),
                Progress::Begun(begun) => so_far = Some(begun.clone()),
                Progress::Made { op, after, .. } => {
                    let Some(result) = so_far.as_mut() else {
                        return;
                    };
                    result.operations.push(op.clone());
                    result.keyframes.push(std::sync::Arc::clone(after));
                    let mut warnings = result.warnings();
                    warnings.extend(model_warnings.iter().cloned());
                    underway(Underway::SoFar(Box::new(Ran {
                        result: result.clone(),
                        part: prepared.part.clone(),
                        notes: notes.clone(),
                        warnings,
                        checks: Checks::default(),
                        finished: false,
                    })));
                }
            },
        )?;
    notes.extend(summary(&result));

    underway(Underway::Stage(Stage::Checking));
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
        finished: true,
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
                "{}: {} passes, {} links stayed down, {} ramped, {} down a helix, \
                 removed {:.0} mm3, travel saved {:.0}%, rapid contacts {}{}{}",
                op.name,
                op.passes,
                op.links.stayed_down,
                op.links.ramps,
                op.links.helixes,
                op.sim.removed_mm3,
                op.travel_saved * 100.0,
                op.sim.rapid_contacts,
                op.smoothing.map_or(String::new(), |f| format!(
                    ", smoothed {} moves to {} ({} arcs)",
                    f.before, f.after, f.arcs
                )),
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
    /// Whether it may be posted: it got to the end, it verified, and no check
    /// fails.
    pub fn passed(&self) -> bool {
        self.finished && self.result.is_clean() && self.checks.passed()
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
mod tests;
