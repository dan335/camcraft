//! Jobs for tests: run the way the app runs them.

use std::path::Path;

use camcraft_core::job::Job;
use camcraft_session::{Ran, load_models, models_of, run_setup};

/// Load `job`'s models from `dir` and run its first setup.
///
/// # Panics
///
/// When a model does not load or the setup does not run: a test's job must.
#[allow(clippy::unwrap_used)]
pub fn run_setup_zero(job: &Job, dir: &Path) -> Ran {
    let loaded = load_models(job, dir, &[0]).unwrap();
    run_setup(job, 0, &models_of(&loaded)).unwrap()
}
