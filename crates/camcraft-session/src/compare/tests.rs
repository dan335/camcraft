//! camcraft's basic operations held to Fusion's.
//!
//! `fixtures/step/ops_block.step` was made in Fusion for this: a 100 x 70 x 20 mm
//! block with a pocket 40 x 36 x 6 mm with R6 corners, a round one 24 across
//! and 8 deep, and two blind holes 6 across and 12 deep. Fusion's Face, 2D
//! Contour, 2D Pocket and Drill were set on it as a new operation of each starts,
//! with a 6 mm flat (18,000 rev/min, 1500 mm/min, plunging at a third of that)
//! and a 6 mm drill (300 mm/min), the stock 5 mm round it and 2 mm over, and work
//! zero at the model's origin; each was posted with Fusion's Masso post to
//! `fixtures/nc/ops_block_*.nc`.
//!
//! The job here makes each of camcraft's operations as the app makes a new one
//! ([`Operation::fresh`]), with the same tools, feeds and picks, then cuts each of
//! Fusion's programs from the material camcraft had before that operation, with
//! the same tool, and requires the material each leaves to be the same: a new
//! operation of camcraft's does what a new one of Fusion's does.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::Path;

use camcraft_core::job::{FeatureRef, Operation, Outline, StrategySpec};

use super::*;
use crate::test_support::fixture;
use crate::{job_text, load_models, models_of, parse_job, run_setup};

/// The part, its stock and setup, and the two tools, with no operations.
fn base() -> String {
    format!(
        r#"
version = 1
tolerance = 0.01

[tools.flat6]
kind = "flat"
diameter = 6.0
number = 1
rpm = 18000
flutes = 3
flute_length = 20.0
shoulder_length = 20.0
shank_diameter = 6.0
overall_length = 63.0
stickout = 22.5

[tools.drill6]
kind = "drill"
diameter = 6.0
angle = 118.0
number = 2
rpm = 3000
flute_length = 60.0
shank_diameter = 6.0
overall_length = 65.08
stickout = 62.54

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
"#,
        fixture("ops_block.step")
            .display()
            .to_string()
            .replace('\\', "/")
    )
}

fn pick(at: [f64; 3]) -> FeatureRef {
    FeatureRef {
        model: "part".into(),
        index: 0,
        at,
    }
}

/// Face, 2D Contour, 2D Pocket and Drill, each a new operation as the app
/// makes one, and the job read back, so its expressions are worked out as a
/// job read from disk has them.
fn job() -> Job {
    let mut job = parse_job(&base(), "ops_block").unwrap();
    let make = |name: &str, strategy: &str, tool: &str, feed: f64| {
        let spec = job.tools.get(tool);
        let mut op = Operation::fresh(
            name,
            StrategySpec::named(strategy).unwrap(),
            (Some(tool.to_string()), spec, None),
        );
        op.passes.feed = feed;
        op
    };
    let face = make("face", "face", "flat6", 1500.0);
    let contour = make("contour", "profile", "flat6", 1500.0);
    let mut pocket = make("pocket", "pocket", "flat6", 1500.0);
    pocket.outline = Outline::Selection;
    pocket.selection.faces = vec![pick([-20.0, 0.0, 14.0]), pick([25.0, 10.0, 12.0])];
    // Fusion's drilling feeds down at its plunge feed.
    let mut drill = make("drill", "drill", "drill6", 300.0);
    drill.linking.plunge_feed = 300.0;
    drill.selection.edges = vec![pick([18.0, -22.0, 20.0]), pick([38.0, -22.0, 20.0])];
    job.setups[0]
        .operations
        .extend([face, contour, pocket, drill]);
    parse_job(&job_text(&job).unwrap(), "ops_block").unwrap()
}

fn fusion(op: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/nc")
            .join(format!("ops_block_{op}.nc")),
    )
    .unwrap()
}

#[test]
fn new_basic_operations_leave_what_fusions_do() {
    let job = job();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let loaded = load_models(&job, dir, &[0]).unwrap();
    let ran = run_setup(&job, 0, &models_of(&loaded)).unwrap();
    assert!(ran.checks.failures().count() == 0, "{:?}", ran.checks);

    for op in ["face", "contour", "pocket", "drill"] {
        let c = compare(&job, 0, dir, &ran, op, &fusion(op), job.tolerance).unwrap();
        let d = &c.difference;
        let say = format!("{op}: {d:?}\n{:?}\n{:?}", c.ours, c.theirs);
        assert!(d.cut > 0, "{say}");
        assert_eq!(c.ours_verify.gouged, 0, "{say}");
        assert_eq!(c.theirs_verify.gouged, 0, "{say}");
        assert_eq!(c.theirs_cut.rapid_contacts, 0, "{say}");
        // The same material, column for column: all but the odd column on a
        // wall, where Fusion's arcs and camcraft's are each only within their
        // tolerance (Fusion's 2D Pocket's is 0.1 mm), may differ.
        assert!(1000 * (d.cut - d.agree) < d.cut, "{say}");
        assert!(d.quantiles[2] <= job.tolerance, "{say}");
        let (ours, theirs) = d.removed;
        assert!((ours - theirs).abs() <= 0.002 * theirs, "{say}");
        // Both go down to the same depth.
        assert!((c.ours.lowest - c.theirs.lowest).abs() < 1e-6, "{say}");
        // And cut about as far: one depth, not levels, and passes ending
        // where Fusion's do. Drilling feeds back out of each hole at the
        // retract feed, where Fusion's G81 comes out at rapid, so it is held
        // to its depth and material alone.
        if op != "drill" {
            assert!(c.ours.fed < 1.15 * c.theirs.fed, "{say}");
        }
    }
}

/// A blind hole picked by its top rim alone is drilled to its own floor, as
/// Fusion's drilling goes "from hole bottom", not through the part to the
/// operation's default bottom.
#[test]
fn a_blind_hole_is_drilled_to_its_floor() {
    let job = job();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let loaded = load_models(&job, dir, &[0]).unwrap();
    let ran = run_setup(&job, 0, &models_of(&loaded)).unwrap();
    let drill = ran
        .result
        .operations
        .iter()
        .find(|o| o.name == "drill")
        .unwrap();
    let lowest = drill
        .path
        .moves()
        .iter()
        .map(|m| m.to.z)
        .fold(f64::INFINITY, f64::min);
    assert!((lowest - 8.0).abs() < 1e-6, "drilled to {lowest}");
}

mod ops_2d;
