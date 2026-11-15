//! camcraft's Morph held to Fusion's.
//!
//! The part is `fixtures/step/ops_3d.step` (`scripts/fusion/ops_3d/part.py`),
//! as for Ramp, Pencil and Project: a 60 x 40 x 20 mm block centred on the
//! origin, its bottom at Z0, with a pocket 38 x 26 and 10 deep from (-26, -13)
//! to (12, 13), its corner at (12, -13) R5 and its floor's edges R4; a dome 14
//! across and 5 tall on its floor about (-8, 0); and the block's top edge at +X
//! rounded R8. Fusion's Morph was set on it as a new one starts, but for what
//! its program's name says, between two curves picked on the part, with the
//! stock 5 mm round the part and level with its top and work zero at the
//! model's origin (`scripts/fusion/ops_3d/cam.py`), posted with Fusion's Masso
//! post to `fixtures/nc/ops_3d_*.nc`:
//!
//! - `morph`, a new Morph with the sample library's 6 mm ball between two
//!   closed curves: the dome's foot, the circle where it meets the floor, then
//!   the floor's edge, the loop at Z10 where the fillets round it start;
//! - `morph_one_way`, the same, Direction One Way;
//! - `morph_open`, a new Morph between two open curves, the R8 round's upper
//!   edge, along the top at X22, then its lower edge, along the block's side at
//!   X30, reverted to run as the upper does.
//!
//! The tool at 18,000 rev/min and 1500 mm/min, plunging and ramping at 500.
//! Each job here roughs the part first with camcraft's adaptive clearing and a
//! 6 mm flat, as a new one starts, leaving half a millimetre, then makes one
//! Morph as the app makes a new one ([`Operation::fresh`]), with the same
//! tool, feeds, settings and curves, and cuts Fusion's program from the
//! material the roughing left, with the same tool.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use camcraft_core::job::{FeatureRef, Operation, ProjectDirection, StrategySpec};

use super::tests_pencil::{Bounds, base, held_to};
use super::*;
use crate::{job_text, parse_job};

/// The dome's foot, by a point on it.
const DOME_FOOT: [f64; 3] = [-15.0, 0.0, 10.0];

/// The floor's edge, its five edges, from the one along +Y.
const FLOOR_EDGE: [[f64; 3]; 5] = [
    [-6.13, 9.0, 10.0],
    [-20.251, 0.0, 10.0],
    [-6.63, -9.0, 10.0],
    [7.707, -8.707, 10.0],
    [8.0, 0.5, 10.0],
];

/// The round's upper edge, then its lower.
const ROUND: [[f64; 3]; 2] = [[22.0, 0.0, 20.0], [30.0, 0.0, 12.0]];

/// A job that roughs the part and then morphs between the curves `picked`,
/// each edge by a point on it, with the 6 mm ball, as `name`: a new Morph but
/// for `change`, read back, so its expressions are worked out as a job read
/// from disk has them.
fn job(name: &str, picked: &[[f64; 3]], change: impl FnOnce(&mut Operation)) -> Job {
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
    let mut morph = make(name, "morph", "ball6");
    morph.selection.edges = picked
        .iter()
        .map(|&at| FeatureRef {
            model: "part".into(),
            index: 0,
            at,
        })
        .collect();
    change(&mut morph);
    job.setups[0].operations.extend([rough, morph]);
    parse_job(&job_text(&job).unwrap(), "ops_3d").unwrap()
}

/// The dome's foot, then the floor's edge.
fn closed() -> Vec<[f64; 3]> {
    std::iter::once(DOME_FOOT).chain(FLOOR_EDGE).collect()
}

/// `name`, a new Morph between `picked` but for `change`, held to Fusion's
/// program for it, to `bounds`; Fusion's own rapids may meet the roughing's
/// stock `contacts` times.
///
/// Between the round's edges the passes are Fusion's to within the
/// tolerance. Between the dome's foot and the floor's edge they are not:
/// Fusion matches the floor's corner at (8, 9) with the circle 6.4° round from
/// +X about the dome's axis, camcraft near 34.5°, where the matched points are
/// closest, and lays a few more passes than Fusion's 30, so none is more than
/// the stepover from the next. Both cut the floor alike; on the dome's steep
/// flank their cusps fall in other places, a band of columns a few hundredths
/// apart all round it, which is most of what the figures lose, and the extra
/// passes most of the length fed. One of Fusion's rapids meets what the
/// roughing left, 0.70 mm deep. Each bound is this run's figure with a little
/// room (decision 0030).
fn holds(
    (name, picked): (&str, &[[f64; 3]]),
    change: impl FnOnce(&mut Operation),
    bounds: &Bounds,
    contacts: u64,
) {
    held_to(&job(name, picked, change), name, bounds, contacts);
}

/// A new Morph between the dome's foot and the floor's edge leaves what
/// Fusion's does: 91.9% of the 436,148 columns either cuts alike, the median
/// difference 0.0038 mm and the 95th percentile 0.0137, 0.04% more removed
/// and 13.1% more fed.
#[test]
fn a_new_morph_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.91,
        median: 0.005,
        p95: 0.016,
        removed: 0.002,
        fed: 0.15,
        lowest: 0.005,
    };
    holds(("morph", &closed()), |_| {}, &bounds, 1);
}

/// One way, every pass clockwise, leaves the same: 91.9% of 436,148 columns
/// alike, the median 0.0038 mm and the 95th percentile 0.0137, 0.04% more
/// removed and 12.1% more fed.
#[test]
fn a_morph_one_way_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.91,
        median: 0.005,
        p95: 0.016,
        removed: 0.002,
        fed: 0.14,
        lowest: 0.005,
    };
    let one_way = |op: &mut Operation| {
        if let StrategySpec::Morph { direction, .. } = &mut op.strategy {
            *direction = ProjectDirection::OneWay;
        }
    };
    holds(("morph_one_way", &closed()), one_way, &bounds, 1);
}

/// Between the round's two edges, open, the passes are Fusion's: 99.4% of
/// the 171,445 columns alike, the median 0.0013 mm and the 95th percentile
/// 0.0048, 0.16% less removed and 0.3% more fed.
#[test]
fn a_morph_between_open_curves_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.99,
        median: 0.003,
        p95: 0.007,
        removed: 0.003,
        fed: 0.02,
        lowest: 0.005,
    };
    holds(("morph_open", &ROUND), |_| {}, &bounds, 0);
}
