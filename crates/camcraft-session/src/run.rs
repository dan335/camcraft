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
                "{}: {} passes, {} links stayed down, {} ramped, removed {:.0} mm3, \
                 travel saved {:.0}%, rapid contacts {}{}",
                op.name,
                op.passes,
                op.links.stayed_down,
                op.links.ramps,
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
mod tests;
