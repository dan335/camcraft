//! What every module's tests share: the small fixture job, run.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};

use camcraft_core::job::{Job, ModelDef};

use crate::checks::{self, Checks};
use crate::{FinishSpec, FinishTool, Models, Ran, load_job_model, run_setup, single_operation};

/// A STEP file from the repository's fixtures.
pub(crate) fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/step")
        .join(name)
}

fn hinge() -> ModelDef {
    ModelDef {
        name: "hinge".into(),
        path: fixture("hinge_jig.step").display().to_string(),
        units: None,
        transform: Default::default(),
    }
}

/// One finishing pass over the small hinge fixture, so tests run quickly.
pub(crate) fn small_job() -> Job {
    single_operation(
        &FinishSpec {
            tool: FinishTool::Ball,
            diameter: 6.0,
            stepover: 2.0,
            tolerance: 0.05,
            stock_radial: 0.0,
            stock_axial: 0.0,
            max_cut_step: 0.5,
            clearance: 5.0,
            feed: 1200.0,
            wcs: 54,
            rpm: 18000,
            number: 1,
        },
        hinge(),
    )
}

/// [`small_job`], run.
pub(crate) fn run_small() -> Ran {
    let job = small_job();
    let model = load_job_model(Path::new("."), &job.models[0], job.tolerance).unwrap();
    let models: Models<'_> = [("hinge", &model)].into_iter().collect();
    run_setup(&job, 0, &models).unwrap()
}

/// Re-check a run after its first operation's path has been tampered with.
pub(crate) fn recheck(
    ran: &Ran,
    tamper: impl FnOnce(&mut camcraft_core::toolpath::Toolpath),
) -> Checks {
    let job = small_job();
    let mut ops = ran.result.operations.clone();
    tamper(&mut ops[0].path);
    checks::check(
        &job,
        0,
        &ran.part,
        &ops,
        &ran.result.verify,
        ran.result.verify_budget,
        &[],
    )
}
