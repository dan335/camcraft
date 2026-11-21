//! camcraft's basic operations on `fixtures/step/ops_block.step`: a 100 x 70 x
//! 20 mm block with a pocket 40 x 36 x 6 mm with R6 corners, a round one 24
//! across and 8 deep, and two blind holes 6 across and 12 deep. A Face, a 2D
//! Contour, a 2D Pocket and a Drill, each a new operation, with a 6 mm flat
//! (18,000 rev/min, 1500 mm/min, plunging at a third of that) and a 6 mm drill
//! (300 mm/min), the stock 5 mm round it and 2 mm over, and work zero at the
//! model's origin.

use camcraft_core::job::{FeatureRef, Job, Operation, Outline, StrategySpec};

use super::{Recorded, holds, run};
use crate::test_support::fixture;
use crate::{job_text, parse_job};

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

/// Face, 2D Contour, 2D Pocket of both pockets' floors and Drill of both holes'
/// rims, each a new operation as the app makes one, and the job read back, so
/// its expressions are worked out as a job read from disk has them. The drill
/// feeds down at 300 mm/min, its plunge feed.
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
    let mut drill = make("drill", "drill", "drill6", 300.0);
    drill.linking.plunge_feed = 300.0;
    drill.selection.edges = vec![pick([18.0, -22.0, 20.0]), pick([38.0, -22.0, 20.0])];
    job.setups[0]
        .operations
        .extend([face, contour, pocket, drill]);
    parse_job(&job_text(&job).unwrap(), "ops_block").unwrap()
}

/// Each new basic operation cuts the block as recorded: the face across its
/// top, the contour round it, both pockets to their floors and both holes.
#[test]
fn new_basic_operations_cut_the_block_as_recorded() {
    holds(
        &job(),
        &[
            (
                "face",
                Recorded {
                    lifts: 1,
                    fed: 2520.4,
                    lowest: 20.0,
                    most_left: 12.0,
                    left: 12754.6,
                },
            ),
            (
                "contour",
                Recorded {
                    lifts: 1,
                    fed: 387.2,
                    lowest: 0.0,
                    most_left: 12.0,
                    left: 12754.6,
                },
            ),
            (
                "pocket",
                Recorded {
                    lifts: 1,
                    fed: 981.2,
                    lowest: 12.5,
                    most_left: 12.0,
                    left: 2282.4,
                },
            ),
            (
                "drill",
                Recorded {
                    lifts: 1,
                    fed: 62.0,
                    lowest: 8.0,
                    most_left: 8.0,
                    left: 1671.9,
                },
            ),
        ],
    );
}

/// A blind hole picked by its top rim alone is drilled to its own floor, 12 mm
/// down, not through the part to the operation's default bottom.
#[test]
fn a_blind_hole_is_drilled_to_its_floor() {
    let ran = run(&job());
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
