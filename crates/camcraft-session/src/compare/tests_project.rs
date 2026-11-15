//! camcraft's Project held to Fusion's.
//!
//! The part is `fixtures/step/ops_3d.step` (`scripts/fusion/ops_3d/part.py`),
//! as for Ramp and Pencil: a 60 x 40 x 20 mm block centred on the origin, its
//! bottom at Z0, with a pocket 38 x 26 and 10 deep from (-26, -13) to
//! (12, 13), its corner at (12, -13) R5; a dome 14 across and 5 tall on its
//! floor about (-8, 0); and the block's top edge at +X rounded R8. Fusion's
//! Project was set on it as a new one starts, but for what its program's name
//! says, along three curves picked on the part: the dome's foot, the circle
//! where it meets the floor; the floor's edge, the loop at Z10 where the
//! fillets round it start; and the round's lower edge along the block's side
//! at X30, open. With the
//! stock 5 mm round the part and level with its top and work zero at the
//! model's origin (`scripts/fusion/ops_3d/cam.py`), posted with Fusion's Masso
//! post to `fixtures/nc/ops_3d_*.nc`:
//!
//! - `project`, a new Project with the sample library's 6 mm ball;
//! - `project_other_way`, the same, Direction Other Way;
//! - `project_axial`, the same, Axial Offset 1 mm;
//! - `project_ball12`, the same with the 12 mm ball.
//!
//! Each tool at 18,000 rev/min and 1500 mm/min, plunging and ramping at 500.
//! Each job here roughs the part first with camcraft's adaptive clearing and a
//! 6 mm flat, as a new one starts, leaving half a millimetre, then makes one
//! Project as the app makes a new one ([`Operation::fresh`]), with the same
//! tool, feeds, settings and curves, and cuts Fusion's program from the
//! material the roughing left, with the same tool.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use camcraft_core::job::{FeatureRef, Operation, ProjectDirection, StrategySpec};

use super::tests_pencil::{Bounds, base, held_to};
use super::*;
use crate::{job_text, parse_job};

/// The edges picked, each by a point on it: the dome's foot; the floor's
/// edge, its five edges, from the one along +Y; and the round's lower edge.
const CURVES: [[f64; 3]; 7] = [
    [-15.0, 0.0, 10.0],
    [-6.13, 9.0, 10.0],
    [-20.251, 0.0, 10.0],
    [-6.63, -9.0, 10.0],
    [7.707, -8.707, 10.0],
    [8.0, 0.5, 10.0],
    [30.0, 0.0, 12.0],
];

/// A job that roughs the part and then projects the curves onto it with
/// `tool`, as `name`: a new Project but for `change`, read back, so its
/// expressions are worked out as a job read from disk has them.
fn job(name: &str, tool: &str, change: impl FnOnce(&mut Operation)) -> Job {
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
    let mut project = make(name, "project", tool);
    project.selection.edges = CURVES
        .iter()
        .map(|&at| FeatureRef {
            model: "part".into(),
            index: 0,
            at,
        })
        .collect();
    change(&mut project);
    job.setups[0].operations.extend([rough, project]);
    parse_job(&job_text(&job).unwrap(), "ops_3d").unwrap()
}

/// `name`, a new Project with `tool` but for `change`, held to Fusion's
/// program for it, to `bounds`.
///
/// The curves are cut where Fusion's are, but for a hair: round the dome's
/// foot camcraft's ball rests on the dome exactly, at Z12.2916 with the 6 mm
/// ball and Z13.0263 with the 12 mm, where Fusion's points lie up to 0.006 and
/// 0.008 mm below, and its chords a little outside the circle; along the
/// floor's edge Fusion's ball is 0.004 mm above the floor. Each is a band of
/// columns just past the tolerance where the ball's side is steep, which is
/// what the figures lose. Fusion's rapids down to 2 mm above where it comes
/// onto a curve meet what the roughing left twice, 0.77 mm deep at most
/// (0.26 with the 12 mm ball). Each bound is this run's figure with a little
/// room (decision 0029).
fn holds((name, tool): (&str, &str), change: impl FnOnce(&mut Operation), bounds: &Bounds) {
    held_to(&job(name, tool, change), name, bounds, 2);
}

/// A new Project with the 6 mm ball leaves what Fusion's does: 98.8% of the
/// 432,937 columns either cuts alike, the median difference 0.0038 mm and
/// the 95th percentile 0.0045, 0.05% more removed and 7.6% more fed.
#[test]
fn a_new_project_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.98,
        median: 0.005,
        p95: 0.007,
        removed: 0.002,
        fed: 0.09,
        lowest: 0.005,
    };
    holds(("project", "ball6"), |_| {}, &bounds);
}

/// The other way round each curve leaves the same: 98.8% of 432,937 columns
/// alike, the median 0.0038 mm and the 95th percentile 0.0045, 0.05% more
/// removed and 7.6% more fed.
#[test]
fn a_project_the_other_way_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.98,
        median: 0.005,
        p95: 0.007,
        removed: 0.002,
        fed: 0.09,
        lowest: 0.005,
    };
    let other = |op: &mut Operation| {
        if let StrategySpec::Project { direction, .. } = &mut op.strategy {
            *direction = ProjectDirection::OtherWay;
        }
    };
    holds(("project_other_way", "ball6"), other, &bounds);
}

/// A millimetre above where the tool rests: 99.1% of 403,553 columns alike,
/// the median 0.0038 mm and the 95th percentile 0.0044, 0.06% more removed
/// and 7.1% more fed.
#[test]
fn a_project_with_an_axial_offset_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.98,
        median: 0.005,
        p95: 0.007,
        removed: 0.002,
        fed: 0.09,
        lowest: 0.005,
    };
    let raised = |op: &mut Operation| {
        if let StrategySpec::Project { axial_offset, .. } = &mut op.strategy {
            *axial_offset = 1.0;
        }
    };
    holds(("project_axial", "ball6"), raised, &bounds);
}

/// With the 12 mm ball, which rides up the fillets and the walls along the
/// floor's edge: 90.1% of 358,704 columns alike, the median 0.0049 mm and the
/// 95th percentile 0.0120, 0.30% less removed and 9.2% more fed. Round the
/// dome's foot Fusion's points lie 0.008 mm below where the ball rests, a
/// band all round the dome's flank.
#[test]
fn a_project_with_the_larger_ball_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.89,
        median: 0.007,
        p95: 0.015,
        removed: 0.005,
        fed: 0.11,
        lowest: 0.01,
    };
    holds(("project_ball12", "ball12"), |_| {}, &bounds);
}
