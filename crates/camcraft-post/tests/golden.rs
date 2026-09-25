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

use camcraft_core::drilling::Hole;
use camcraft_core::job::{Coolant, SpindleDirection};
use camcraft_core::toolpath::{MoveRole, Toolpath};
use camcraft_post::op::{DrillCycles, ProgramSpec, Units, lower};
use camcraft_post::profile::Profile;
use camcraft_post::render::render;
use glam::{dvec2, dvec3};

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
        spin: SpindleDirection::Clockwise,
        coolant: Coolant::Flood,
        units: Units::Millimetres,
        drill: None,
    }
}

fn check(profile_name: &str) {
    let profile = Profile::builtin(profile_name)
        .unwrap_or_else(|| panic!("no builtin profile {profile_name}"))
        .unwrap();
    check_against(&profile, &fixture(), &spec(), profile_name);
}

/// Render `path` for `profile` and compare with `tests/golden/<file>.nc`.
fn check_against(profile: &Profile, path: &Toolpath, spec: &ProgramSpec, file: &str) {
    let profile_name = file;
    let ops = lower(path, spec).unwrap();
    let (got, report) = render(&ops, profile, "golden");

    assert!(report.lines > 10, "suspiciously short program");
    assert_eq!(
        report.reserved_used, 0,
        "the fixture should not use reserved instructions"
    );

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{file}.nc"));

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

/// Three holes, drilled as canned cycles on a controller that takes them: across
/// to the first at the clearance height, one line a hole returning there, then
/// G80. The path is ignored when the holes are given.
fn drilling(peck: Option<f64>, dwell: Option<f64>) -> ProgramSpec {
    let hole = |x: f64, y: f64| Hole {
        at: dvec2(x, y),
        top: 5.0,
        bottom: -1.5,
        radius: 3.0,
    };
    ProgramSpec {
        op_name: "holes".into(),
        strategy: "drill".into(),
        tool: 4,
        tool_comment: "6mm drill".into(),
        coolant: Coolant::Off,
        drill: Some(DrillCycles {
            holes: vec![hole(10.0, 10.0), hole(30.0, 10.0), hole(-12.345, 40.0)],
            peck,
            dwell,
            feed: 250.0,
            clearance: 20.0,
        }),
        ..spec()
    }
}

fn masso_with_cycles() -> Profile {
    let mut p = Profile::builtin("masso").unwrap().unwrap();
    p.drill_cycles = true;
    p
}

#[test]
fn drilling_cycles_are_stable() {
    let p = masso_with_cycles();
    check_against(&p, &Toolpath::new(), &drilling(None, None), "masso-drill");
    check_against(
        &p,
        &Toolpath::new(),
        &drilling(Some(2.0), None),
        "masso-peck",
    );
    check_against(
        &p,
        &Toolpath::new(),
        &drilling(None, Some(0.5)),
        "masso-dwell",
    );
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
