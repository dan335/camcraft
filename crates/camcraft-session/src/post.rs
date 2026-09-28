//! Posting: a setup's programs named, rendered and written.
//!
//! A [`Posting`] is made first, with every program rendered, then written in one
//! go. It says what stands in the way — setups that did not pass their checks, and
//! files that would be replaced — so a front end can ask before anything is
//! written. Several setups are all posted or none are: half a job's programs are a
//! trap on the machine. The command line's `--out` and the app's Post both come
//! through here, so they write the same files under the same names.

use std::path::{Path, PathBuf};

use camcraft_core::job::Setup;
use camcraft_core::run::{OperationResult, StrategyDetail};
use camcraft_post::op::{DrillCycles, ProgramSpec, Units as PostUnits, lower};
use camcraft_post::profile::{Profile, ProgramUnits};
use camcraft_post::render::{RenderReport, render};

use crate::{Ran, SessionError, fs};

/// A builtin machine profile by name, or a TOML profile by path. A relative path
/// is found from `job_dir`, when there is one.
pub fn load_profile(name: &str, job_dir: Option<&Path>) -> Result<Profile, SessionError> {
    Ok(match Profile::builtin(name) {
        Some(p) => p?,
        None => match job_dir {
            Some(dir) => Profile::load(fs::resolve(dir, name))?,
            None => Profile::load(name)?,
        },
    })
}

/// The machine profiles built in, by name.
pub fn builtin_posts() -> &'static [&'static str] {
    Profile::builtin_names()
}

fn file_safe(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// The program name for the `index`-th operation (from zero) of a setup:
/// `top-01-rough`.
///
/// The name is kept usable as a file name.
pub fn program_name(setup: &str, index: usize, op_name: &str) -> String {
    format!(
        "{}-{:02}-{}",
        file_safe(setup),
        index + 1,
        file_safe(op_name)
    )
}

/// Render one operation as a program for `profile`.
pub fn program(
    op: &OperationResult,
    wcs: u8,
    profile: &Profile,
    name: &str,
) -> Result<(String, RenderReport), SessionError> {
    let spec = ProgramSpec {
        name: name.to_string(),
        op_name: op.name.clone(),
        strategy: op.strategy.to_string(),
        tool: op.tool_number,
        tool_comment: op.tool_comment.clone(),
        wcs,
        rpm: op.rpm,
        spin: op.spindle,
        coolant: op.coolant,
        units: match profile.units {
            ProgramUnits::Mm => PostUnits::Millimetres,
            ProgramUnits::Inch => PostUnits::Inches,
        },
        drill: match &op.detail {
            StrategyDetail::Drill(r) if profile.drill_cycles => Some(DrillCycles {
                holes: r.holes.clone(),
                peck: r.peck,
                dwell: r.dwell,
                feed: r.feed,
                clearance: op.clearance_z,
            }),
            _ => None,
        },
    };
    let ops = lower(&op.path, &spec)?;
    Ok(render(&ops, profile, name))
}

/// One program, rendered.
#[derive(Debug, Clone)]
pub struct Program {
    /// Where it goes.
    pub path: PathBuf,
    /// What it says.
    pub text: String,
    /// The operation it cuts.
    pub op: String,
    /// The machine profile it was rendered for.
    pub profile: String,
    /// What rendering it wrote.
    pub report: RenderReport,
}

/// What posting would write, and what stands in the way.
#[derive(Debug, Clone)]
pub struct Posting {
    /// The folder the programs go into.
    pub dir: PathBuf,
    /// Every program, in setup and operation order.
    pub programs: Vec<Program>,
    /// Setups that did not pass their checks, each with what failed.
    pub refused: Vec<(String, Vec<String>)>,
    /// Programs that would replace a file already there.
    pub overwrites: Vec<PathBuf>,
}

impl Posting {
    /// Whether anything must be confirmed before writing.
    pub fn needs_confirming(&self) -> bool {
        !self.refused.is_empty() || !self.overwrites.is_empty()
    }
}

/// Render every program of `setups` (each with its run) into `dir`, writing
/// nothing.
///
/// A setup's `post` names a builtin profile or a file, found from `job_dir`;
/// `profile`, when given, is used for every setup instead.
pub fn plan_posting(
    setups: &[(&Setup, &Ran)],
    job_dir: &Path,
    dir: &Path,
    profile: Option<&str>,
) -> Result<Posting, SessionError> {
    let mut out = Posting {
        dir: dir.to_path_buf(),
        programs: Vec::new(),
        refused: Vec::new(),
        overwrites: Vec::new(),
    };
    // Each program's file, by what a file system that ignores case sees, and
    // whose it is: two names can come to one file.
    let mut taken: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (setup, ran) in setups {
        let in_setup = |source| SessionError::InSetup {
            setup: setup.name.clone(),
            source: Box::new(source),
        };
        let loaded =
            load_profile(profile.unwrap_or(&setup.post), Some(job_dir)).map_err(in_setup)?;
        if !ran.passed() {
            let mut why = ran.checks.failure_lines();
            if why.is_empty() {
                why.push("the simulator did not verify it".into());
            }
            out.refused.push((setup.name.clone(), why));
        }
        // An operation turned off or left out of the run has no program.
        let made = ran.result.operations.iter().enumerate();
        for (k, op) in made.filter(|(_, op)| op.made.has_paths()) {
            let name = program_name(&setup.name, k, &op.name);
            let (text, report) = program(op, setup.wcs, &loaded, &name).map_err(in_setup)?;
            let path = dir.join(format!("{name}.{}", loaded.extension));
            let whose = format!("setup '{}' operation '{}'", setup.name, op.name);
            let key = path.display().to_string().to_lowercase();
            if let Some(first) = taken.insert(key, whose.clone()) {
                return Err(SessionError::SameProgram {
                    path: path.display().to_string(),
                    first,
                    second: whose,
                });
            }
            if path.exists() {
                out.overwrites.push(path.clone());
            }
            out.programs.push(Program {
                path,
                text,
                op: op.name.clone(),
                profile: loaded.name.clone(),
                report,
            });
        }
    }
    Ok(out)
}

/// Write every program in `posting`: each beside its place first, and only once
/// all are written is each moved into it, so a failure writing any of them
/// leaves every program as it was, not some of this posting's and some of the
/// last's. Returns how many were written.
pub fn write_posting(posting: &Posting) -> Result<usize, SessionError> {
    let failed = |path: &Path| {
        let path = path.display().to_string();
        move |source| SessionError::ProgramWrite { path, source }
    };
    std::fs::create_dir_all(&posting.dir).map_err(failed(&posting.dir))?;
    let temp = |path: &Path| {
        let mut t = path.as_os_str().to_owned();
        t.push(".camcraft-posting");
        std::path::PathBuf::from(t)
    };
    let clean = || {
        for p in &posting.programs {
            let _ = std::fs::remove_file(temp(&p.path));
        }
    };
    for p in &posting.programs {
        if let Err(e) = std::fs::write(temp(&p.path), &p.text) {
            clean();
            return Err(failed(&p.path)(e));
        }
    }
    for p in &posting.programs {
        if let Err(e) = std::fs::rename(temp(&p.path), &p.path) {
            clean();
            return Err(failed(&p.path)(e));
        }
    }
    Ok(posting.programs.len())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::test_support::run_small;
    use crate::{load_models, parse_job, run_setup};
    use camcraft_core::job::{Coolant, SpindleDirection};

    #[test]
    fn program_names_are_numbered_and_file_safe() {
        assert_eq!(program_name("top", 0, "rough"), "top-01-rough");
        assert_eq!(
            program_name("side 2", 11, "finish / walls"),
            "side-2-12-finish---walls"
        );
    }

    /// The example job on the small fixture, run.
    fn example() -> (camcraft_core::job::Job, PathBuf, Ran) {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let text = std::fs::read_to_string(dir.join("rail-jig.camcraft")).unwrap();
        // The small fixture, so the test runs quickly.
        let text = text.replace("sled_base.step", "hinge_jig.step");
        let job = parse_job(&text, "rail-jig").unwrap();
        let loaded = load_models(&job, &dir, &[0]).unwrap();
        let ran = run_setup(&job, 0, &crate::models_of(&loaded)).unwrap();
        (job, dir, ran)
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("camcraft-posting-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// The same names and programs as the command line writes, into the folder.
    #[test]
    fn a_posting_writes_a_program_per_operation() {
        let (job, job_dir, ran) = example();
        let dir = scratch("writes");
        let p = plan_posting(&[(&job.setups[0], &ran)], &job_dir, &dir, None).unwrap();
        assert!(!p.needs_confirming(), "{:?}", p.refused);
        let names: Vec<String> = p
            .programs
            .iter()
            .map(|g| g.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["top-01-rough.nc", "top-02-finish.nc"]);
        assert_eq!(write_posting(&p).unwrap(), 2);
        let text = std::fs::read_to_string(&p.programs[0].path).unwrap();
        assert!(text.contains("G54"));

        // Posting again would replace them, and says so first.
        let again = plan_posting(&[(&job.setups[0], &ran)], &job_dir, &dir, None).unwrap();
        assert_eq!(again.overwrites.len(), 2);
        assert!(again.needs_confirming());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two setups whose names come to the same file name, as "flip 1" and
    /// "Flip_1" do, are refused before either is written, rather than the second
    /// silently replacing the first.
    #[test]
    fn two_programs_for_one_file_are_refused() {
        let (job, job_dir, ran) = example();
        let mut other = job.setups[0].clone();
        other.name = "TOP".into();
        let dir = scratch("same");
        let err = plan_posting(
            &[(&job.setups[0], &ran), (&other, &ran)],
            &job_dir,
            &dir,
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("setup 'top'") && err.contains("setup 'TOP'"),
            "{err}"
        );
    }

    /// A setup that did not pass is named, with what failed, before anything is
    /// written.
    #[test]
    fn a_refused_setup_is_named_with_its_failures() {
        let (job, job_dir, mut ran) = example();
        ran.result.operations[0].sim.rapid_contacts = 1;
        ran.checks.groups[1].findings[0].level = crate::checks::Level::Fail;
        let dir = scratch("refused");
        let p = plan_posting(&[(&job.setups[0], &ran)], &job_dir, &dir, None).unwrap();
        assert_eq!(p.refused.len(), 1);
        assert_eq!(p.refused[0].0, "top");
        assert!(!p.refused[0].1.is_empty());
        assert!(!dir.exists(), "planning writes nothing");
    }

    /// A profile that cannot be loaded stops the posting before anything is
    /// rendered or written, and says which setup.
    #[test]
    fn a_bad_profile_writes_nothing() {
        let (job, job_dir, ran) = example();
        let dir = scratch("bad-profile");
        let err =
            plan_posting(&[(&job.setups[0], &ran)], &job_dir, &dir, Some("no-such")).unwrap_err();
        assert!(err.to_string().contains("setup 'top'"), "{err}");
        assert!(!dir.exists());
    }

    #[test]
    fn a_program_renders_from_a_run() {
        let ran = run_small();
        let profile = load_profile("masso", None).unwrap();
        let op = ran.result.operations.first().unwrap();
        let (text, report) = program(op, 54, &profile, "01-finish").unwrap();
        assert!(text.contains("G54"));
        assert!(report.motion_blocks > 10);
        let word = |w: &str| text.split_whitespace().any(|x| x == w);
        assert!(word("M3") && !word("M8"), "{text}");
    }

    /// A drilling operation is posted as canned cycles only for a profile that
    /// takes them; otherwise as its moves.
    #[test]
    fn holes_are_cycles_only_where_the_profile_takes_them() {
        use camcraft_core::drilling::{DrillReport, Hole};
        let ran = run_small();
        let mut op = ran.result.operations.first().unwrap().clone();
        op.detail = StrategyDetail::Drill(DrillReport {
            holes: vec![Hole {
                at: glam::dvec2(3.0, 4.0),
                top: 5.0,
                bottom: 0.0,
                radius: 2.0,
            }],
            peck: Some(1.5),
            dwell: None,
            feed: 200.0,
        });
        let mut profile = load_profile("masso", None).unwrap();
        let (moves, _) = program(&op, 54, &profile, "01-holes").unwrap();
        assert!(!moves.contains("G83"), "{moves}");
        profile.drill_cycles = true;
        let (cycles, _) = program(&op, 54, &profile, "01-holes").unwrap();
        assert!(cycles.contains("G83 X3. Y4. Z0. R7."), "{cycles}");
        assert!(cycles.contains("G80"), "{cycles}");
    }

    /// A tool's spindle direction and coolant reach the program.
    #[test]
    fn a_tools_direction_and_coolant_are_posted() {
        let ran = run_small();
        let profile = load_profile("masso", None).unwrap();
        let mut op = ran.result.operations.first().unwrap().clone();
        op.spindle = SpindleDirection::CounterClockwise;
        op.coolant = Coolant::Mist;
        let (text, _) = program(&op, 54, &profile, "01-finish").unwrap();
        let word = |w: &str| text.split_whitespace().any(|x| x == w);
        assert!(word("M4") && !word("M3"), "{text}");
        assert!(word("M7") && word("M9"), "{text}");
    }
}
