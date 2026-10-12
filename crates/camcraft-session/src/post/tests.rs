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
    let dir = std::env::temp_dir().join(format!("camcraft-posting-{}-{name}", std::process::id()));
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
    let err = plan_posting(&[(&job.setups[0], &ran)], &job_dir, &dir, Some("no-such")).unwrap_err();
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
    use camcraft_core::strategy::drilling::{DrillReport, Hole};
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
