//! Reading models: which formats, and what the numbers in them mean.
//!
//! A unit read wrongly makes a part 25.4 times the wrong size, and nothing
//! downstream can tell. So an STL, which carries no units, is refused without
//! `--units`, and a STEP file, which states its own, is refused if `--units` says
//! something else.

#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/step")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn camcraft(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_camcraft"))
        .args(args)
        .output()
        .unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn a_step_file_is_read_in_its_own_units() {
    let dir = scratch("a_step_file_is_read_in_its_own_units");
    let json = dir.join("hinge.json");
    let out = camcraft(&[
        "inspect",
        &fixture("hinge_jig.step"),
        "--tolerance",
        "0.05",
        "--out",
        json.to_str().unwrap(),
    ]);
    assert!(
        matches!(out.status.code(), Some(0 | 2)),
        "exited {:?}: {}",
        out.status.code(),
        stderr(&out)
    );
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&json).unwrap()).unwrap();
    assert_eq!(doc["job"]["units"], "inch (from file)");
    // An inch part at its real size, 25.4 x 31.8 x 33.3 mm, not 25.4 times smaller.
    let (min, max) = (&doc["part"]["min"], &doc["part"]["max"]);
    let x = max[0].as_f64().unwrap() - min[0].as_f64().unwrap();
    assert!((x - 25.4).abs() < 0.1, "X extent {x}");
}

#[test]
fn units_that_contradict_a_step_file_are_refused() {
    let out = camcraft(&[
        "inspect",
        &fixture("hinge_jig.step"),
        "--units",
        "mm",
        "--out",
        "never-written.json",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let err = stderr(&out);
    assert!(err.contains("--units mm") && err.contains("inch"), "{err}");
    assert!(!Path::new("never-written.json").exists());
}

#[test]
fn units_that_agree_with_a_step_file_are_accepted() {
    let dir = scratch("units_that_agree_with_a_step_file_are_accepted");
    let json = dir.join("hinge.json");
    let out = camcraft(&[
        "inspect",
        &fixture("hinge_jig.step"),
        "--units",
        "inch",
        "--tolerance",
        "0.05",
        "--out",
        json.to_str().unwrap(),
    ]);
    assert!(matches!(out.status.code(), Some(0 | 2)), "{}", stderr(&out));
}

#[test]
fn an_stl_without_units_is_refused() {
    let dir = scratch("an_stl_without_units_is_refused");
    let stl = dir.join("empty.stl");
    std::fs::write(&stl, "solid t\nendsolid t\n").unwrap();
    let out = camcraft(&[
        "inspect",
        stl.to_str().unwrap(),
        "--out",
        dir.join("x.json").to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("--units"), "{}", stderr(&out));
}

#[test]
fn an_unknown_format_is_refused() {
    let out = camcraft(&["inspect", "part.obj", "--units", "mm", "--out", "x.json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains(".step"), "{}", stderr(&out));
}
