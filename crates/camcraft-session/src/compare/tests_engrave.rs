//! camcraft's Engrave held to Fusion's.
//!
//! `fixtures/step/ops_engrave.step` was made in Fusion for this
//! (`scripts/fusion/ops_engrave/part.py`): a plate 80 x 50 x 10 mm, centred on
//! the origin with its bottom at Z0, with three shapes sunk 3 mm into its top,
//! deeper than the V goes: a triangle (-30, -15), (-10, -15), (-20, 10); an L
//! 4 mm wide through (0, -15), (20, -15), (20, -11), (4, -11), (4, 10),
//! (0, 10); and a rectangle from (24, -12) to (38, 12). Fusion's Engrave was
//! set on it as a new operation starts, with a 6 mm 90° V (18,000 rev/min,
//! 1000 mm/min, plunging at 300), the stock the plate's own box and work zero
//! at the model's origin, each shape picked by its rim at the plate's top: on
//! all three, and on each alone (`scripts/fusion/ops_engrave/cam.py`). Each was
//! posted with Fusion's Masso post to `fixtures/nc/ops_engrave_*.nc`.
//!
//! Each job here makes one Engrave as the app makes a new one
//! ([`Operation::fresh`]), with the same tool, feeds and picks, then cuts
//! Fusion's program from the material camcraft had before it, with the same
//! tool, and requires the material each leaves to be the same.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::Path;

use camcraft_core::job::{FeatureRef, Operation, StrategySpec};

use super::*;
use crate::test_support::fixture;
use crate::{job_text, load_models, models_of, parse_job, run_setup};

/// The plate, its stock and setup, and the V, with no operations.
fn base() -> String {
    format!(
        r#"
version = 1
tolerance = 0.01

[tools.vee6]
kind = "cone"
diameter = 6.0
angle = 90.0
number = 4
rpm = 18000
flutes = 2
shank_diameter = 6.0
overall_length = 50.0
stickout = 20.0

[[model]]
name = "plate"
path = "{}"

[[stock]]
name = "box"
kind = "around-models"
margin = [0.0, 0.0, 0.0]

[[setup]]
name = "top"
models = ["plate"]
stocks = ["box"]
post = "masso"
"#,
        fixture("ops_engrave.step")
            .display()
            .to_string()
            .replace('\\', "/")
    )
}

/// The edges round each shape's rim, at the plate's top, by a point on each.
fn rim(shape: &str) -> Vec<[f64; 2]> {
    match shape {
        "triangle" => vec![[-20.0, -15.0], [-15.0, -2.5], [-25.0, -2.5]],
        "stroke" => vec![
            [10.0, -15.0],
            [20.0, -13.0],
            [12.0, -11.0],
            [4.0, -0.5],
            [2.0, 10.0],
            [0.0, -2.5],
        ],
        "rectangle" => vec![[31.0, -12.0], [38.0, 0.0], [31.0, 12.0], [24.0, 0.0]],
        _ => Vec::new(),
    }
}

/// A job with one Engrave, `name`, of `shapes`, a new operation as the app
/// makes one, read back, so its expressions are worked out as a job read from
/// disk has them.
fn job(name: &str, shapes: &[&str]) -> Job {
    let mut job = parse_job(&base(), "ops_engrave").unwrap();
    let spec = job.tools.get("vee6");
    let mut op = Operation::fresh(
        name,
        StrategySpec::named("engrave").unwrap(),
        (Some("vee6".to_string()), spec, None),
    );
    op.passes.feed = 1000.0;
    // Fusion's tool plunges at 300, not a third of its feed.
    op.expressions.remove("linking.plunge_feed");
    op.linking.plunge_feed = 300.0;
    op.selection.edges = shapes
        .iter()
        .flat_map(|s| rim(s))
        .map(|[x, y]| FeatureRef {
            model: "plate".into(),
            index: 0,
            at: [x, y, 10.0],
        })
        .collect();
    job.setups[0].operations.push(op);
    parse_job(&job_text(&job).unwrap(), "ops_engrave").unwrap()
}

fn fusion(op: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/nc")
            .join(format!("ops_engrave_{op}.nc")),
    )
    .unwrap()
}

/// The plate's top picked, a face with nothing sunk into it but the three
/// shapes, which are islands of it: the V cuts into the plate, and the checks
/// say so twice, why (CC223) and that the part is cut into (CC107). A floor
/// picked is engraved as its rim is, from the top of the walls round it.
#[test]
fn an_engraving_into_the_part_fails_its_checks() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut top = job("engrave", &[]);
    top.setups[0].operations[0].selection.faces = vec![FeatureRef {
        model: "plate".into(),
        index: 0,
        at: [-35.0, 20.0, 10.0],
    }];
    let loaded = load_models(&top, dir, &[0]).unwrap();
    let ran = run_setup(&top, 0, &models_of(&loaded)).unwrap();
    let found = |code: &str, level: crate::checks::Level| {
        ran.checks
            .groups
            .iter()
            .flat_map(|g| &g.findings)
            .any(|f| f.code == code && f.level == level)
    };
    assert!(
        found("CC223", crate::checks::Level::Warn),
        "{:?}",
        ran.checks
    );
    assert!(
        found("CC107", crate::checks::Level::Fail),
        "{:?}",
        ran.checks
    );

    let mut floor = job("rectangle", &[]);
    floor.setups[0].operations[0].selection.faces = vec![FeatureRef {
        model: "plate".into(),
        index: 0,
        at: [31.0, 0.0, 7.0],
    }];
    let rim = job("rectangle", &["rectangle"]);
    let path = |job: &Job| {
        let loaded = load_models(job, dir, &[0]).unwrap();
        let ran = run_setup(job, 0, &models_of(&loaded)).unwrap();
        assert!(ran.checks.passed(), "{:?}", ran.checks);
        ran.result.operations[0].path.clone()
    };
    assert_eq!(path(&floor), path(&rim));
}

/// A new Engrave, of all three shapes and of each alone, leaves what Fusion's
/// does: the same material, as deep, cutting nothing the model keeps, about as
/// far, each wall one move.
#[test]
fn a_new_engrave_leaves_what_fusions_does() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (name, shapes) in [
        ("engrave", &["triangle", "stroke", "rectangle"][..]),
        ("triangle", &["triangle"][..]),
        ("stroke", &["stroke"][..]),
        ("rectangle", &["rectangle"][..]),
    ] {
        let job = job(name, shapes);
        let loaded = load_models(&job, dir, &[0]).unwrap();
        let ran = run_setup(&job, 0, &models_of(&loaded)).unwrap();
        assert!(
            ran.checks.failures().count() == 0,
            "{name}: {:?}",
            ran.checks
        );
        let c = compare(&job, 0, dir, &ran, name, &fusion(name), job.tolerance).unwrap();
        let d = &c.difference;
        let say = format!("{name}: {d:?}\n{:?}\n{:?}", c.ours, c.theirs);
        eprintln!("{say}");
        assert!(d.cut > 0, "{say}");
        assert_eq!(c.ours_verify.gouged, 0, "{say}");
        assert_eq!(c.theirs_verify.gouged, 0, "{say}");
        assert_eq!(c.ours_cut.rapid_contacts, 0, "{say}");
        assert_eq!(c.theirs_cut.rapid_contacts, 0, "{say}");
        // The same material, column for column: all but the odd column on a
        // wall may differ.
        assert!(1000 * (d.cut - d.agree) < d.cut, "{say}");
        assert!(d.quantiles[2] <= job.tolerance, "{say}");
        let (ours, theirs) = d.removed;
        assert!((ours - theirs).abs() <= 0.005 * theirs, "{say}");
        // As deep, to the thousandth Fusion's program is written to.
        assert!((c.ours.lowest - c.theirs.lowest).abs() <= 1e-3, "{say}");
        // About as far, lifting between shapes as Fusion's does.
        assert!(c.ours.fed < 1.15 * c.theirs.fed, "{say}");
        assert!(c.ours.fed > c.theirs.fed / 1.15, "{say}");
        assert_eq!(c.ours.lifts, c.theirs.lifts, "{say}");
        // In about as many moves, each wall one: camcraft's links take two
        // more a shape (down to the safe distance, and back up to it at the
        // retract feed), and its parabolas fewer, being within the tolerance
        // rather than evenly spaced.
        let links = 2 * shapes.len();
        assert!(c.ours.moves <= c.theirs.moves + links, "{say}");
        assert!(3 * c.ours.moves >= c.theirs.moves, "{say}");
    }
}
