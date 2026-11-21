//! camcraft's Engrave on `fixtures/step/ops_engrave.step`: a plate 80 x 50 x
//! 10 mm, centred on the origin with its bottom at Z0, with three shapes sunk
//! 3 mm into its top, deeper than the V goes: a triangle (-30, -15), (-10, -15),
//! (-20, 10); an L 4 mm wide through (0, -15), (20, -15), (20, -11), (4, -11),
//! (4, 10), (0, 10); and a rectangle from (24, -12) to (38, 12). Each Engrave
//! is a new operation with a 6 mm 90° V (18,000 rev/min, 1000 mm/min, plunging
//! at 300), the stock the plate's own box and work zero at the model's origin,
//! each shape picked by its rim at the plate's top.

use camcraft_core::job::{FeatureRef, Job, Operation, StrategySpec};

use super::{Recorded, holds, run};
use crate::checks::Level;
use crate::test_support::fixture;
use crate::{job_text, parse_job};

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

/// A point on the plate at `at`.
fn on_plate(at: [f64; 3]) -> FeatureRef {
    FeatureRef {
        model: "plate".into(),
        index: 0,
        at,
    }
}

/// A job with one Engrave, `name`, of `shapes`, a new operation as the app
/// makes one, plunging at 300 mm/min, read back, so its expressions are worked
/// out as a job read from disk has them.
fn job(name: &str, shapes: &[&str]) -> Job {
    let mut job = parse_job(&base(), "ops_engrave").unwrap();
    let spec = job.tools.get("vee6");
    let mut op = Operation::fresh(
        name,
        StrategySpec::named("engrave").unwrap(),
        (Some("vee6".to_string()), spec, None),
    );
    op.passes.feed = 1000.0;
    op.expressions.remove("linking.plunge_feed");
    op.linking.plunge_feed = 300.0;
    op.selection.edges = shapes
        .iter()
        .flat_map(|s| rim(s))
        .map(|[x, y]| on_plate([x, y, 10.0]))
        .collect();
    job.setups[0].operations.push(op);
    parse_job(&job_text(&job).unwrap(), "ops_engrave").unwrap()
}

/// The plate's top picked, a face with nothing sunk into it but the three
/// shapes, which are islands of it: the V cuts into the plate, and the checks
/// say so twice, why (CC223) and that the part is cut into (CC107). A floor
/// picked is engraved as its rim is, from the top of the walls round it.
#[test]
fn an_engraving_into_the_part_fails_its_checks() {
    let mut top = job("engrave", &[]);
    top.setups[0].operations[0].selection.faces = vec![on_plate([-35.0, 20.0, 10.0])];
    let ran = run(&top);
    let found = |code: &str, level: Level| {
        ran.checks
            .groups
            .iter()
            .flat_map(|g| &g.findings)
            .any(|f| f.code == code && f.level == level)
    };
    assert!(found("CC223", Level::Warn), "{:?}", ran.checks);
    assert!(found("CC107", Level::Fail), "{:?}", ran.checks);

    let mut floor = job("rectangle", &[]);
    floor.setups[0].operations[0].selection.faces = vec![on_plate([31.0, 0.0, 7.0])];
    let rim = job("rectangle", &["rectangle"]);
    let path = |job: &Job| {
        let ran = run(job);
        assert!(ran.checks.passed(), "{:?}", ran.checks);
        ran.result.operations[0].path.clone()
    };
    assert_eq!(path(&floor), path(&rim));
}

/// A new Engrave of all three shapes, and of each alone, as recorded: down to
/// where the V's sides meet each shape's walls, cutting nothing the model
/// keeps, lifting between shapes.
#[test]
fn a_new_engrave() {
    for (name, shapes, recorded) in [
        (
            "engrave",
            &["triangle", "stroke", "rectangle"][..],
            Recorded {
                lifts: 3,
                fed: 262.6,
                lowest: 7.15,
                most_left: 3.0,
                left: 1312.1,
            },
        ),
        (
            "triangle",
            &["triangle"][..],
            Recorded {
                lifts: 1,
                fed: 83.5,
                lowest: 7.15,
                most_left: 3.0,
                left: 1903.6,
            },
        ),
        (
            "stroke",
            &["stroke"][..],
            Recorded {
                lifts: 1,
                fed: 84.5,
                lowest: 7.657,
                most_left: 3.0,
                left: 2091.6,
            },
        ),
        (
            "rectangle",
            &["rectangle"][..],
            Recorded {
                lifts: 1,
                fed: 94.6,
                lowest: 7.15,
                most_left: 3.0,
                left: 1818.8,
            },
        ),
    ] {
        holds(&job(name, shapes), &[(name, recorded)]);
    }
}
