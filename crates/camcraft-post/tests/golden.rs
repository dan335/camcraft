//! Golden-file tests for the post processors.
//!
//! One committed instruction stream, rendered through every profile, diffed against
//! committed output. This is what catches dialect drift: a change to the renderer
//! that looks harmless in isolation shows up here as a line that moved.
//!
//! It also proves the IR stays machine-independent. The same fixture feeds every
//! profile; if a profile needed the fixture changed, the abstraction would be
//! leaking.
//!
//! Regenerate deliberately, never casually:
//!
//! ```sh
//! UPDATE_GOLDEN=1 cargo test -p camcraft-post --test golden
//! ```
//!
//! Then read the diff. A golden file updated without reading the diff is a test
//! that has been switched off.

#![allow(clippy::unwrap_used, clippy::panic)]

use camcraft_core::toolpath::{MoveRole, Toolpath};
use camcraft_post::op::{Coolant, ProgramSpec, Spin, Units, lower};
use camcraft_post::profile::Profile;
use camcraft_post::render::render;
use glam::dvec3;

/// A path exercising every move role the engine currently produces, plus the
/// coordinate cases that formatting gets wrong: negatives, a value that rounds, and
/// a move that changes only one axis.
fn fixture() -> Toolpath {
    let mut t = Toolpath::new();
    t.rapid_to(dvec3(0.0, 0.0, 25.0)).unwrap();
    t.rapid_to(dvec3(-12.3456, 7.5, 25.0)).unwrap();
    t.feed_to(dvec3(-12.3456, 7.5, 1.0), MoveRole::Plunge, 250.0)
        .unwrap();
    t.feed_to(dvec3(-12.3456, 7.5, -0.0001), MoveRole::Plunge, 250.0)
        .unwrap();
    t.feed_to(dvec3(10.0, 7.5, -0.0001), MoveRole::Cut, 1200.0)
        .unwrap();
    t.feed_to(dvec3(10.0, 9.5, -0.0001), MoveRole::Cut, 1200.0)
        .unwrap();
    t.feed_to(dvec3(10.0, 9.5, -2.5), MoveRole::Cut, 800.0)
        .unwrap();
    t.feed_to(dvec3(10.0, 9.5, 25.0), MoveRole::Retract, 900.0)
        .unwrap();
    t
}

fn spec() -> ProgramSpec {
    ProgramSpec {
        name: "golden".into(),
        op_name: "finish".into(),
        strategy: "parallel".into(),
        tool: 3,
        tool_comment: "6mm ball (carbide)".into(),
        wcs: 54,
        rpm: 18000,
        spin: Spin::Clockwise,
        coolant: Some(Coolant::Flood),
        units: Units::Millimetres,
    }
}

fn check(profile_name: &str) {
    let profile = Profile::builtin(profile_name)
        .unwrap_or_else(|| panic!("no builtin profile {profile_name}"))
        .unwrap();
    let ops = lower(&fixture(), &spec()).unwrap();
    let (got, report) = render(&ops, &profile, "golden");

    assert!(report.lines > 10, "suspiciously short program");
    assert_eq!(
        report.reserved_used, 0,
        "the fixture should not use reserved instructions"
    );

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{profile_name}.nc"));

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(&path, &got).unwrap();
        return;
    }

    let want = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nrun with UPDATE_GOLDEN=1 to create it",
            path.display()
        )
    });
    // Compare line by line so a failure names the line rather than dumping two files.
    let want = want.replace("\r\n", "\n");
    for (i, (a, b)) in want.lines().zip(got.lines()).enumerate() {
        assert_eq!(
            a,
            b,
            "{profile_name}.nc line {}: golden {a:?} but rendered {b:?}",
            i + 1
        );
    }
    assert_eq!(
        want.lines().count(),
        got.lines().count(),
        "{profile_name}.nc has a different number of lines"
    );
}

#[test]
fn masso_output_is_stable() {
    check("masso");
}

#[test]
fn grbl_output_is_stable() {
    check("grbl");
}

/// Every builtin profile must have a golden file. Adding a profile without one
/// would leave it untested by exactly the test meant to cover it.
#[test]
fn every_builtin_profile_has_a_golden_file() {
    for name in Profile::builtin_names() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(format!("{name}.nc"));
        assert!(
            path.exists() || std::env::var("UPDATE_GOLDEN").is_ok(),
            "profile {name} has no golden file at {}",
            path.display()
        );
    }
}

/// The same instruction stream drives every profile. If one needed a different
/// fixture, the IR would not be machine-independent.
#[test]
fn one_fixture_serves_every_profile() {
    let ops = lower(&fixture(), &spec()).unwrap();
    let mut seen = Vec::new();
    for name in Profile::builtin_names() {
        let p = Profile::builtin(name).unwrap().unwrap();
        let (text, _) = render(&ops, &p, "golden");
        assert!(!text.is_empty());
        seen.push(text);
    }
    let first = seen.first().unwrap();
    assert!(
        seen.iter().any(|t| t != first),
        "all profiles produced identical output, so none of them is doing anything"
    );
}
