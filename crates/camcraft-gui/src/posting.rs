//! Posting from the app: the programs `camcraft job --out` would write, with its
//! names and its refusal.
//!
//! A plan is made first, with every program rendered, then written in one go. The
//! plan says what stands in the way — setups that did not pass their checks, and
//! files that would be replaced — so the app can ask before anything is written.
//! Several setups are all posted or none are: half a job's programs are a trap on
//! the machine.

use std::path::{Path, PathBuf};

use camcraft_core::job::Setup;
use camcraft_session::{Ran, load_profile, program, program_name};

/// One program, rendered.
#[derive(Debug, Clone)]
pub struct Program {
    /// Where it goes.
    pub path: PathBuf,
    /// What it says.
    pub text: String,
}

/// What posting would write, and what stands in the way.
#[derive(Debug, Clone)]
pub struct Plan {
    /// The folder the programs go into.
    pub dir: PathBuf,
    /// Every program, in setup and operation order.
    pub programs: Vec<Program>,
    /// Setups that did not pass their checks, each with what failed.
    pub refused: Vec<(String, Vec<String>)>,
    /// Programs that would replace a file already there.
    pub overwrites: Vec<PathBuf>,
}

impl Plan {
    /// Whether anything must be confirmed before writing.
    pub fn needs_confirming(&self) -> bool {
        !self.refused.is_empty() || !self.overwrites.is_empty()
    }
}

/// Render every program of `setups` (each with its run) into `dir`.
///
/// A setup's `post` names a builtin profile or a file, found from `job_dir`.
pub fn plan(setups: &[(&Setup, &Ran)], job_dir: &Path, dir: &Path) -> Result<Plan, String> {
    let mut out = Plan {
        dir: dir.to_path_buf(),
        programs: Vec::new(),
        refused: Vec::new(),
        overwrites: Vec::new(),
    };
    for (setup, ran) in setups {
        let profile = load_profile(&setup.post, Some(job_dir))
            .map_err(|e| format!("setup '{}': {e}", setup.name))?;
        if !ran.passed() {
            let mut why = ran.checks.failure_lines();
            if why.is_empty() {
                why.push("the simulator did not verify it".into());
            }
            out.refused.push((setup.name.clone(), why));
        }
        for (k, op) in ran.result.operations.iter().enumerate() {
            let name = program_name(&setup.name, k, &op.name);
            let (text, _) =
                program(op, setup.wcs, &profile, &name).map_err(|e| format!("{name}: {e}"))?;
            let path = dir.join(format!("{name}.{}", profile.extension));
            if path.exists() {
                out.overwrites.push(path.clone());
            }
            out.programs.push(Program { path, text });
        }
    }
    Ok(out)
}

/// Write every program in `plan`. Returns how many were written.
pub fn write(plan: &Plan) -> Result<usize, String> {
    std::fs::create_dir_all(&plan.dir)
        .map_err(|e| format!("cannot create {}: {e}", plan.dir.display()))?;
    for p in &plan.programs {
        std::fs::write(&p.path, &p.text)
            .map_err(|e| format!("cannot write {}: {e}", p.path.display()))?;
    }
    Ok(plan.programs.len())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::test_jobs::run_setup_zero;
    use camcraft_core::job::Job;

    fn example() -> (Job, PathBuf) {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let text = std::fs::read_to_string(dir.join("rail-jig.camcraft")).unwrap();
        // The small fixture, so the test runs quickly.
        let text = text.replace("sled_base.step", "hinge_jig.step");
        (camcraft_session::parse_job(&text, "rail-jig").unwrap(), dir)
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("camcraft-posting-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// The same names and programs as the command line writes, into the folder.
    #[test]
    fn a_plan_writes_a_program_per_operation() {
        let (job, job_dir) = example();
        let ran = run_setup_zero(&job, &job_dir);
        let dir = scratch("writes");
        let p = plan(&[(&job.setups[0], &ran)], &job_dir, &dir).unwrap();
        assert!(!p.needs_confirming(), "{:?}", p.refused);
        let names: Vec<String> = p
            .programs
            .iter()
            .map(|g| g.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["top-01-rough.nc", "top-02-finish.nc"]);
        assert_eq!(write(&p).unwrap(), 2);
        let text = std::fs::read_to_string(&p.programs[0].path).unwrap();
        assert!(text.contains("G54"));

        // Posting again would replace them, and says so first.
        let again = plan(&[(&job.setups[0], &ran)], &job_dir, &dir).unwrap();
        assert_eq!(again.overwrites.len(), 2);
        assert!(again.needs_confirming());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A setup that did not pass is named, with what failed, before anything is
    /// written.
    #[test]
    fn a_refused_setup_is_named_with_its_failures() {
        let (job, job_dir) = example();
        let mut ran = run_setup_zero(&job, &job_dir);
        ran.result.operations[0].sim.rapid_contacts = 1;
        ran.checks.groups[1].findings[0].level = camcraft_session::checks::Level::Fail;
        let dir = scratch("refused");
        let p = plan(&[(&job.setups[0], &ran)], &job_dir, &dir).unwrap();
        assert_eq!(p.refused.len(), 1);
        assert_eq!(p.refused[0].0, "top");
        assert!(!p.refused[0].1.is_empty());
        assert!(!dir.exists(), "planning writes nothing");
    }
}
