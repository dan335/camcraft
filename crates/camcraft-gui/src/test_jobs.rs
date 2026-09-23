//! Jobs for tests: run the way the app runs them.

use std::collections::BTreeMap;
use std::path::Path;

use camcraft_core::job::Job;
use camcraft_session::{Model, Models, Ran, load_job_model, run_setup};

/// Load `job`'s models from `dir` and run its first setup.
///
/// # Panics
///
/// When a model does not load or the setup does not run: a test's job must.
#[allow(clippy::unwrap_used)]
pub fn run_setup_zero(job: &Job, dir: &Path) -> Ran {
    let loaded: BTreeMap<String, Model> = job
        .models_for(0)
        .into_iter()
        .map(|m| {
            (
                m.name.clone(),
                load_job_model(dir, m, job.tolerance).unwrap(),
            )
        })
        .collect();
    let models: Models<'_> = loaded.iter().map(|(n, m)| (n.as_str(), m)).collect();
    run_setup(job, 0, &models).unwrap()
}
