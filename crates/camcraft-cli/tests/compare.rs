//! `camcraft compare`: any G-code program cut from the same stock as one of a
//! job's operations, and compared with camcraft's own program for it.
//!
//! Given camcraft's own program for the operation, the two must leave the same
//! material, column for column; given a program that cuts nothing, every
//! column camcraft cut is camcraft's alone.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::path::Path;

use common::{camcraft, fixture, scratch};

/// A job facing the top of the block part with a 6 mm flat, posted for a
/// Masso, written to `dir`.
fn job(dir: &Path) -> String {
    let text = format!(
        r#"
version = 1
tolerance = 0.01

[tools.flat6]
kind = "flat"
diameter = 6.0
number = 1
rpm = 18000
flute_length = 20.0
shank_diameter = 6.0
stickout = 22.5

[[model]]
name = "part"
path = "{}"

[[stock]]
name = "block"
kind = "around-models"
margin = [5.0, 5.0, 2.0]

[[setup]]
name = "top"
models = ["part"]
stocks = ["block"]
post = "masso"

[[setup.operation]]
name = "face"
strategy = "face"
tool = "flat6"
passes = {{ feed = 1500.0 }}
"#,
        fixture("ops_block.step").replace('\\', "/")
    );
    let path = dir.join("block.camcraft");
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().into_owned()
}

/// What `camcraft compare` printed for the job at `job` and `face=program`,
/// having passed.
fn compared(job: &str, program: &Path) -> String {
    let pair = format!("face={}", program.display());
    let run = camcraft(&["compare", job, &pair]);
    assert!(
        run.status.code() == Some(0),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8(run.stdout).unwrap()
}

#[test]
fn camcrafts_own_program_leaves_the_same_material() {
    let dir = scratch("compare-own");
    let job = job(&dir);
    let out = dir.join("out");
    let run = camcraft(&["job", &job, "--out", &out.to_string_lossy()]);
    assert!(
        run.status.code() == Some(0),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let report = compared(&job, &out.join("top-01-face.nc"));
    assert!(report.contains("== face"), "{report}");
    assert!(
        report.contains("alike within 0.01 mm 100.0%, camcraft lower 0.0% (most 0.000), other lower 0.0% (most 0.000)"),
        "{report}"
    );
    assert!(
        report.contains("difference, mm: p50 0.000  p95 0.000  p99 0.000  most 0.000"),
        "{report}"
    );
}

/// A few hand-written lines that only move over the stock cut nothing: every
/// column camcraft's facing cut, camcraft cut lower.
#[test]
fn a_program_that_cuts_nothing_leaves_every_column_to_camcraft() {
    let dir = scratch("compare-nothing");
    let job = job(&dir);
    let program = dir.join("air.nc");
    std::fs::write(
        &program,
        "G90 G94 G17\nG21\nG54\nS18000 M3\nG0 X0 Y0 Z40.\nG1 X20. Y10. F1500.\nG0 Z50.\nM30\n",
    )
    .unwrap();
    let report = compared(&job, &program);
    assert!(
        report.contains("alike within 0.01 mm 0.0%, camcraft lower 100.0%"),
        "{report}"
    );
}
