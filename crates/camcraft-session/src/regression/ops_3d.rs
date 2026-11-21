//! camcraft's 3D finishing on `fixtures/step/ops_3d.step`: a 60 x 40 x 20 mm
//! block centred on the origin, its bottom at Z0, with a pocket 38 x 26 and 10
//! deep from (-26, -13) to (12, 13), its -X wall drafted 15°, its corner at
//! (12, -13) R5 and the other three sharp, its floor at Z10 and its floor's
//! edges R4; a dome 14 across and 5 tall on its floor about (-8, 0); and the
//! block's top edge at +X rounded R8. The stock is 5 mm round the part and
//! level with its top, and work zero at the model's origin.
//!
//! Every job here roughs the part first with a new Adaptive and a 6 mm flat,
//! leaving half a millimetre, then finishes it with one new operation, `finish`,
//! changed as its test says: with a 6 mm ball, but where a test names the
//! 12 mm ball or, for Flat, the 6 mm flat, each at 18,000 rev/min and
//! 1500 mm/min, plunging and ramping at 500. The roughing is the same in every
//! job, so it is run once and kept for them all.

use std::sync::{Arc, OnceLock};

use camcraft_core::job::{
    FeatureRef, Job, Operation, ProjectDirection, StrategySpec, SurfaceAction, SurfaceGroup,
};
use camcraft_core::run::{Kept, Plan};

use super::{Recorded, here, measure, passed, run};
use crate::test_support::fixture;
use crate::{job_text, load_models, models_of, parse_job, run_setup_planned};

/// The part, its stock and setup, the roughing flat and the two balls, with no
/// operations.
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

[tools.ball6]
kind = "ball"
diameter = 6.0
number = 2
rpm = 18000
flutes = 3
flute_length = 20.0
shoulder_length = 20.0
shank_diameter = 6.0
overall_length = 63.0
stickout = 22.5

[tools.ball12]
kind = "ball"
diameter = 12.0
number = 3
rpm = 18000
flutes = 3
flute_length = 25.0
shoulder_length = 25.0
shank_diameter = 12.0
overall_length = 76.0
stickout = 27.5

[[model]]
name = "part"
path = "{}"

[[stock]]
name = "block"
kind = "around-models"
margin = [5.0, 5.0, 0.0]

[[setup]]
name = "top"
models = ["part"]
stocks = ["block"]
post = "masso"
"#,
        fixture("ops_3d.step")
            .display()
            .to_string()
            .replace('\\', "/")
    )
}

/// A point on the part.
fn on_part(at: [f64; 3]) -> FeatureRef {
    FeatureRef {
        model: "part".into(),
        index: 0,
        at,
    }
}

/// A job that roughs the part and then finishes it with `tool` by `strategy`:
/// a new operation but for `change`, read back, so its expressions are worked
/// out as a job read from disk has them.
fn job(strategy: &str, tool: &str, change: impl FnOnce(&mut Operation)) -> Job {
    let mut job = parse_job(&base(), "ops_3d").unwrap();
    let make = |name: &str, strategy: &str, tool: &str| {
        let spec = job.tools.get(tool);
        let mut op = Operation::fresh(
            name,
            StrategySpec::named(strategy).unwrap(),
            (Some(tool.to_string()), spec, None),
        );
        op.passes.feed = 1500.0;
        op
    };
    let rough = make("rough", "adaptive", "flat6");
    let mut finish = make("finish", strategy, tool);
    change(&mut finish);
    job.setups[0].operations.extend([rough, finish]);
    parse_job(&job_text(&job).unwrap(), "ops_3d").unwrap()
}

/// The roughing every job here starts with, and the job as far as it: run
/// once, for them all.
static ROUGHED: OnceLock<(Job, Kept)> = OnceLock::new();

/// `job`'s roughing, its first operation, as the one run of it made it, with
/// the material it left, for a run of `job` to keep rather than work out
/// again ([`Plan::Keep`]). Every job here starts with the same roughing of the
/// same part, stock and setup, and nothing after an operation changes what it
/// makes, so it is the same computation.
fn roughed(job: &Job) -> Kept {
    let mut prefix = job.clone();
    prefix.setups[0].operations.truncate(1);
    let (first, kept) = ROUGHED.get_or_init(|| {
        let ran = run(&prefix);
        let kept = Kept {
            result: ran.result.operations[0].clone(),
            after: ran.result.keyframes.get(1).cloned(),
        };
        (prefix.clone(), kept)
    });
    assert_eq!(
        &prefix, first,
        "every job here starts with the same roughing"
    );
    kept.clone()
}

/// `job` run after the kept roughing, every check passed, and its finish
/// clean and as `recorded`.
fn holds(job: &Job, recorded: &Recorded) {
    let loaded = load_models(job, here(), &[0]).unwrap();
    let kept = roughed(job);
    let after = kept.after.clone();
    let plans = [Plan::Keep(Box::new(kept)), Plan::Generate];
    let ran = run_setup_planned(job, 0, &models_of(&loaded), &plans, &mut |_| {}).unwrap();
    // The material the roughing left, put back as it was, not cut again.
    let put_back = after.as_ref().zip(ran.result.keyframes.get(1));
    assert!(put_back.is_some_and(|(a, b)| Arc::ptr_eq(a, b)));
    passed(&ran);
    let finish = measure(job, &ran, "finish");
    finish.clean();
    finish.as_recorded(recorded);
}

/// The faces grouped, each by a point on it: the dome's top, the pocket's
/// floor and the block's top.
const DOME: [f64; 3] = [-8.0, 0.0, 15.0];
const FLOOR: [f64; 3] = [5.0, 5.0, 10.0];
const TOP: [f64; 3] = [-28.0, 18.0, 20.0];

/// A group of the face at `at`, machined or avoided as `action`, with offsets
/// `(radial, axial)`.
fn group(action: SurfaceAction, at: [f64; 3], (radial, axial): (f64, f64)) -> SurfaceGroup {
    SurfaceGroup {
        action,
        radial,
        axial,
        faces: vec![on_part(at)],
    }
}

/// The face at `at` avoided, `offsets` sideways and down.
fn avoiding(at: [f64; 3], offsets: (f64, f64)) -> impl FnOnce(&mut Operation) {
    move |op| {
        op.surfaces
            .groups
            .push(group(SurfaceAction::Avoid, at, offsets));
    }
}

/// The model avoided, the pocket's floor alone machined.
fn floor_alone(op: &mut Operation) {
    op.surfaces.model.action = SurfaceAction::Avoid;
    op.surfaces
        .groups
        .push(group(SurfaceAction::Machine, FLOOR, (0.0, 0.0)));
}

/// Nothing changed from a new operation.
fn new(_: &mut Operation) {}

// Ramp: down every wall and slope in laps, a stepdown apart.

#[test]
fn a_new_ramp() {
    holds(
        &job("ramp", "ball6", new),
        &Recorded {
            lifts: 3,
            fed: 6105.8,
            lowest: 0.0,
            most_left: 8.1907,
            left: 244.8,
        },
    );
}

#[test]
fn a_ramp_two_millimetres_a_lap() {
    let step2 = |op: &mut Operation| op.passes.stepdown = Some(2.0);
    holds(
        &job("ramp", "ball6", step2),
        &Recorded {
            lifts: 3,
            fed: 3665.2,
            lowest: 0.0,
            most_left: 8.4134,
            left: 376.0,
        },
    );
}

#[test]
fn a_ramp_at_half_a_degree() {
    let gentle = |op: &mut Operation| op.linking.ramp_angle = 0.5;
    holds(
        &job("ramp", "ball6", gentle),
        &Recorded {
            lifts: 3,
            fed: 6572.7,
            lowest: 0.0,
            most_left: 8.1907,
            left: 233.6,
        },
    );
}

#[test]
fn a_ramp_avoiding_the_dome() {
    let avoid = avoiding(DOME, (1.0, 1.0));
    holds(
        &job("ramp", "ball6", avoid),
        &Recorded {
            lifts: 4,
            fed: 5901.7,
            lowest: 0.0,
            most_left: 8.1907,
            left: 764.3,
        },
    );
}

// Pencil: along the valleys where the ball touches two faces at once.

/// With the 12 mm ball, larger than the floor's R4 fillets and the R5 corner,
/// it touches twice all round the pocket's floor, a radius from its walls, up
/// its corners and round the dome's foot.
#[test]
fn a_new_pencil() {
    holds(
        &job("pencil", "ball12", new),
        &Recorded {
            lifts: 1,
            fed: 223.2,
            lowest: 10.0,
            most_left: 8.699,
            left: 1606.6,
        },
    );
}

/// With the 6 mm ball, which fits the fillets, it touches twice round the
/// dome's foot and up the sharp corners only.
#[test]
fn a_pencil_with_a_ball_that_fits_the_fillets() {
    holds(
        &job("pencil", "ball6", new),
        &Recorded {
            lifts: 1,
            fed: 136.2,
            lowest: 10.0,
            most_left: 8.1764,
            left: 2422.8,
        },
    );
}

#[test]
fn a_pencil_at_forty_degrees() {
    let forty = |op: &mut Operation| {
        if let StrategySpec::Pencil {
            bitangency_angle, ..
        } = &mut op.strategy
        {
            *bitangency_angle = 40.0;
        }
    };
    holds(
        &job("pencil", "ball12", forty),
        &Recorded {
            lifts: 1,
            fed: 214.0,
            lowest: 10.0,
            most_left: 8.699,
            left: 1607.0,
        },
    );
}

#[test]
fn a_pencil_with_two_stepovers() {
    let two = |op: &mut Operation| {
        if let StrategySpec::Pencil { stepovers, .. } = &mut op.strategy {
            *stepovers = 2;
        }
    };
    holds(
        &job("pencil", "ball6", two),
        &Recorded {
            lifts: 2,
            fed: 727.0,
            lowest: 10.0,
            most_left: 8.1764,
            left: 1032.6,
        },
    );
}

// Project: along curves picked on the part.

/// The curves projected, each edge by a point on it: the dome's foot, the
/// circle where it meets the floor; the floor's edge, the loop at Z10 where
/// the fillets round it start, its five edges from the one along +Y; and the
/// round's lower edge along the block's side at X30, open.
const CURVES: [[f64; 3]; 7] = [
    [-15.0, 0.0, 10.0],
    [-6.13, 9.0, 10.0],
    [-20.251, 0.0, 10.0],
    [-6.63, -9.0, 10.0],
    [7.707, -8.707, 10.0],
    [8.0, 0.5, 10.0],
    [30.0, 0.0, 12.0],
];

/// A Project along [`CURVES`], but for `change`.
fn projecting(change: impl FnOnce(&mut Operation)) -> impl FnOnce(&mut Operation) {
    move |op| {
        op.selection.edges = CURVES.iter().map(|&at| on_part(at)).collect();
        change(op);
    }
}

#[test]
fn a_new_project() {
    holds(
        &job("project", "ball6", projecting(new)),
        &Recorded {
            lifts: 3,
            fed: 209.9,
            lowest: 10.0,
            most_left: 9.1441,
            left: 1478.4,
        },
    );
}

#[test]
fn a_project_the_other_way() {
    let other = |op: &mut Operation| {
        if let StrategySpec::Project { direction, .. } = &mut op.strategy {
            *direction = ProjectDirection::OtherWay;
        }
    };
    holds(
        &job("project", "ball6", projecting(other)),
        &Recorded {
            lifts: 3,
            fed: 209.9,
            lowest: 10.0,
            most_left: 9.1441,
            left: 1478.4,
        },
    );
}

#[test]
fn a_project_a_millimetre_up() {
    let raised = |op: &mut Operation| {
        if let StrategySpec::Project { axial_offset, .. } = &mut op.strategy {
            *axial_offset = 1.0;
        }
    };
    holds(
        &job("project", "ball6", projecting(raised)),
        &Recorded {
            lifts: 3,
            fed: 205.7,
            lowest: 11.0,
            most_left: 9.1441,
            left: 2201.2,
        },
    );
}

/// With the 12 mm ball, which rides up the fillets and the walls along the
/// floor's edge.
#[test]
fn a_project_with_the_larger_ball() {
    holds(
        &job("project", "ball12", projecting(new)),
        &Recorded {
            lifts: 3,
            fed: 211.7,
            lowest: 13.026,
            most_left: 8.8204,
            left: 3378.8,
        },
    );
}

// Morph: passes laid between two curves picked on the part.

/// The dome's foot, then the floor's edge, both closed.
const CLOSED: [[f64; 3]; 6] = [
    [-15.0, 0.0, 10.0],
    [-6.13, 9.0, 10.0],
    [-20.251, 0.0, 10.0],
    [-6.63, -9.0, 10.0],
    [7.707, -8.707, 10.0],
    [8.0, 0.5, 10.0],
];

/// The R8 round's upper edge, along the top at X22, then its lower edge,
/// along the block's side at X30, both open.
const ROUND: [[f64; 3]; 2] = [[22.0, 0.0, 20.0], [30.0, 0.0, 12.0]];

/// A Morph between the curves `picked`, but for `change`.
fn morphing(
    picked: &[[f64; 3]],
    change: impl FnOnce(&mut Operation),
) -> impl FnOnce(&mut Operation) {
    let edges: Vec<FeatureRef> = picked.iter().map(|&at| on_part(at)).collect();
    move |op| {
        op.selection.edges = edges;
        change(op);
    }
}

#[test]
fn a_new_morph() {
    holds(
        &job("morph", "ball6", morphing(&CLOSED, new)),
        &Recorded {
            lifts: 1,
            fed: 2315.1,
            lowest: 10.0,
            most_left: 9.1441,
            left: 1482.2,
        },
    );
}

/// Every pass clockwise.
#[test]
fn a_morph_one_way() {
    let one_way = |op: &mut Operation| {
        if let StrategySpec::Morph { direction, .. } = &mut op.strategy {
            *direction = ProjectDirection::OneWay;
        }
    };
    holds(
        &job("morph", "ball6", morphing(&CLOSED, one_way)),
        &Recorded {
            lifts: 1,
            fed: 2312.7,
            lowest: 10.0,
            most_left: 9.1441,
            left: 1482.2,
        },
    );
}

#[test]
fn a_morph_between_open_curves() {
    holds(
        &job("morph", "ball6", morphing(&ROUND, new)),
        &Recorded {
            lifts: 1,
            fed: 1017.1,
            lowest: 16.549,
            most_left: 9.1441,
            left: 3985.4,
        },
    );
}

// Parallel: passes along one direction, seen from above, with faces machined
// and avoided.

#[test]
fn a_new_parallel() {
    holds(
        &job("parallel", "ball6", new),
        &Recorded {
            lifts: 1,
            fed: 11054.8,
            lowest: 10.0,
            most_left: 8.1792,
            left: 161.5,
        },
    );
}

#[test]
fn a_parallel_avoiding_the_dome() {
    let avoid = avoiding(DOME, (1.0, 1.0));
    holds(
        &job("parallel", "ball6", avoid),
        &Recorded {
            lifts: 9,
            fed: 10096.5,
            lowest: 10.0,
            most_left: 8.1792,
            left: 635.8,
        },
    );
}

#[test]
fn a_parallel_avoiding_the_dome_without_clearance() {
    let avoid = avoiding(DOME, (0.0, 0.0));
    holds(
        &job("parallel", "ball6", avoid),
        &Recorded {
            lifts: 9,
            fed: 10193.8,
            lowest: 10.0,
            most_left: 8.1792,
            left: 362.1,
        },
    );
}

#[test]
fn a_parallel_machining_the_floor_alone() {
    holds(
        &job("parallel", "ball6", floor_alone),
        &Recorded {
            lifts: 9,
            fed: 1005.6,
            lowest: 10.0,
            most_left: 9.1441,
            left: 1600.3,
        },
    );
}

/// The floor machined with 0.5 mm of stock of its own each way, which the
/// roughing had left it at already.
#[test]
fn a_parallel_leaving_stock_on_the_floor() {
    let leave = |op: &mut Operation| {
        op.surfaces
            .groups
            .push(group(SurfaceAction::Machine, FLOOR, (0.5, 0.5)));
    };
    holds(
        &job("parallel", "ball6", leave),
        &Recorded {
            lifts: 1,
            fed: 10886.8,
            lowest: 10.38,
            most_left: 8.1792,
            left: 407.0,
        },
    );
}

/// The block's top avoided, 2 mm down: neither cut, nor rolled over at its
/// edges into the pocket.
#[test]
fn a_parallel_avoiding_the_top() {
    let avoid = avoiding(TOP, (0.0, 2.0));
    holds(
        &job("parallel", "ball6", avoid),
        &Recorded {
            lifts: 3,
            fed: 5892.0,
            lowest: 10.0,
            most_left: 8.4754,
            left: 166.0,
        },
    );
}

// Scallop: passes a constant distance apart along the surface.

#[test]
fn a_new_scallop() {
    holds(
        &job("scallop", "ball6", new),
        &Recorded {
            lifts: 1,
            fed: 9960.4,
            lowest: 10.0,
            most_left: 8.1846,
            left: 163.8,
        },
    );
}

#[test]
fn a_scallop_avoiding_the_dome() {
    let avoid = avoiding(DOME, (1.0, 1.0));
    holds(
        &job("scallop", "ball6", avoid),
        &Recorded {
            lifts: 4,
            fed: 8817.1,
            lowest: 10.0,
            most_left: 8.1846,
            left: 633.5,
        },
    );
}

#[test]
fn a_scallop_machining_the_floor_alone() {
    holds(
        &job("scallop", "ball6", floor_alone),
        &Recorded {
            lifts: 3,
            fed: 889.1,
            lowest: 10.0,
            most_left: 9.1441,
            left: 1633.2,
        },
    );
}

// Spiral: one spiral about the part's middle, seen from above.

#[test]
fn a_new_spiral() {
    holds(
        &job("spiral", "ball6", new),
        &Recorded {
            lifts: 1,
            fed: 10823.2,
            lowest: 10.0,
            most_left: 8.1792,
            left: 164.7,
        },
    );
}

#[test]
fn a_spiral_avoiding_the_dome() {
    let avoid = avoiding(DOME, (1.0, 1.0));
    holds(
        &job("spiral", "ball6", avoid),
        &Recorded {
            lifts: 8,
            fed: 9856.1,
            lowest: 10.0,
            most_left: 8.1792,
            left: 638.2,
        },
    );
}

// Contour: level rings round the part, a stepdown apart.

#[test]
fn a_new_contour() {
    holds(
        &job("contour", "ball6", new),
        &Recorded {
            lifts: 15,
            fed: 6254.9,
            lowest: 0.0,
            most_left: 8.1911,
            left: 273.3,
        },
    );
}

#[test]
fn a_contour_avoiding_the_dome() {
    let avoid = avoiding(DOME, (1.0, 1.0));
    holds(
        &job("contour", "ball6", avoid),
        &Recorded {
            lifts: 13,
            fed: 5991.7,
            lowest: 0.0,
            most_left: 8.1911,
            left: 756.4,
        },
    );
}

// Flat: the level faces cleared with a flat end mill.

#[test]
fn a_new_flat() {
    holds(
        &job("flat", "flat6", new),
        &Recorded {
            lifts: 5,
            fed: 1396.4,
            lowest: 10.0,
            most_left: 9.1441,
            left: 3656.7,
        },
    );
}

#[test]
fn a_flat_avoiding_the_dome() {
    let avoid = avoiding(DOME, (1.0, 1.0));
    holds(
        &job("flat", "flat6", avoid),
        &Recorded {
            lifts: 4,
            fed: 1332.3,
            lowest: 10.0,
            most_left: 9.1441,
            left: 4000.7,
        },
    );
}
