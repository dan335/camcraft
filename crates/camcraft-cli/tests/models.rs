//! Reading models: which formats, and what the numbers in them mean.
//!
//! A unit read wrongly makes a part 25.4 times the wrong size, and nothing
//! downstream can tell. So an STL, which carries no units, is refused without
//! `--units`, and a STEP file, which states its own, is refused if `--units` says
//! something else.

#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod common;

use std::path::Path;
use std::process::Output;

use common::{camcraft, fixture, scratch};

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn a_step_file_is_read_in_its_own_units() {
    let dir = scratch("a_step_file_is_read_in_its_own_units");
    let out = camcraft(&[
        "post",
        &fixture("hinge_jig.step"),
        "--tolerance",
        "0.05",
        "--out",
        dir.join("hinge.nc").to_str().unwrap(),
    ]);
    let err = stderr(&out);
    assert!(
        err.contains("read STEP in inch"),
        "exited {:?}: {err}",
        out.status.code()
    );
    // An inch part at its real size, 25.4 x 31.8 x 33.3 mm, not 25.4 times smaller.
    assert!(err.contains("model is 25.4 x 31.8 x 33.3 mm"), "{err}");
}

#[test]
fn units_that_contradict_a_step_file_are_refused() {
    let out = camcraft(&[
        "post",
        &fixture("hinge_jig.step"),
        "--units",
        "mm",
        "--out",
        "never-written.nc",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let err = stderr(&out);
    assert!(err.contains("--units mm") && err.contains("inch"), "{err}");
    assert!(!Path::new("never-written.nc").exists());
}

#[test]
fn units_that_agree_with_a_step_file_are_accepted() {
    let dir = scratch("units_that_agree_with_a_step_file_are_accepted");
    let out = camcraft(&[
        "post",
        &fixture("hinge_jig.step"),
        "--units",
        "inch",
        "--tolerance",
        "0.05",
        "--out",
        dir.join("hinge.nc").to_str().unwrap(),
    ]);
    assert!(
        stderr(&out).contains("read STEP in inch"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn an_stl_without_units_is_refused() {
    let dir = scratch("an_stl_without_units_is_refused");
    let stl = dir.join("empty.stl");
    std::fs::write(&stl, "solid t\nendsolid t\n").unwrap();
    let out = camcraft(&[
        "post",
        stl.to_str().unwrap(),
        "--out",
        dir.join("x.nc").to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("--units"), "{}", stderr(&out));
}

#[test]
fn an_unknown_format_is_refused() {
    let out = camcraft(&["post", "part.obj", "--units", "mm", "--out", "x.nc"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains(".step"), "{}", stderr(&out));
}

/// Two tools a job uses may not share a T number: the machine would load one cutter
/// and run the other's program. Refused before anything is computed.
#[test]
fn two_tools_on_one_number_are_refused() {
    let dir = scratch("two_tools_on_one_number_are_refused");
    let job = dir.join("job.camcraft");
    let example = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/rail-jig.camcraft"),
    )
    .unwrap();
    let clash = example.replace("number = 2", "number = 1").replace(
        "path = \"../fixtures/step/sled_base.step\"",
        &format!("path = {:?}", fixture("hinge_jig.step")),
    );
    assert_ne!(clash, example, "the edit must land");
    std::fs::write(&job, clash).unwrap();
    let out = camcraft(&["job", job.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let err = stderr(&out);
    assert!(err.contains("T1") && err.contains("load one"), "{err}");
    assert!(
        !err.contains("checks passed") && !err.contains("check(s) FAILED"),
        "nothing is computed for a refused job: {err}"
    );
}
