//! camcraft's Parallel held to Fusion's, with faces machined and avoided.
//!
//! The part is `fixtures/step/ops_3d.step` (`scripts/fusion/ops_3d/part.py`),
//! as for Ramp, Pencil, Project and Morph: a 60 x 40 x 20 mm block centred on
//! the origin, its bottom at Z0, with a pocket 38 x 26 and 10 deep from
//! (-26, -13) to (12, 13), its floor at Z10 and its floor's edges R4; a dome
//! 14 across and 5 tall on its floor about (-8, 0); and the block's top edge at
//! +X rounded R8. Fusion's Parallel was set on it as a new one starts, with
//! the sample library's 6 mm ball, the stock 5 mm round the part and level
//! with its top and work zero at the model's origin, and its surface groups as
//! its program's name says (`scripts/fusion/ops_3d/cam.py`), posted with
//! Fusion's Masso post to `fixtures/nc/ops_3d_*.nc`:
//!
//! - `parallel`, no groups;
//! - `parallel_avoid_dome`, the dome avoided, 1 mm sideways and 1 mm down;
//! - `parallel_avoid_dome_0`, the dome avoided, no clearance;
//! - `parallel_floor_only`, the model avoided, the pocket's floor machined;
//! - `parallel_floor_leave`, the floor machined with 0.5 mm of stock of its
//!   own each way;
//! - `parallel_avoid_top`, the block's top avoided, 2 mm down.
//!
//! The tool at 18,000 rev/min and 1500 mm/min, plunging and ramping at 500.
//! Each job here roughs the part first with camcraft's adaptive clearing and a
//! 6 mm flat, as a new one starts, leaving half a millimetre, then makes one
//! Parallel as the app makes a new one ([`Operation::fresh`]), with the same
//! tool, feeds and groups, and cuts Fusion's program from the material the
//! roughing left, with the same tool.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use camcraft_core::job::{FeatureRef, Operation, StrategySpec, SurfaceAction, SurfaceGroup};

use super::tests_pencil::{Bounds, base, held_to};
use super::*;
use crate::{job_text, parse_job};

/// The faces grouped, each by a point on it: the dome's top, the pocket's
/// floor and the block's top. Scallop's and Spiral's tests' too.
pub(super) const DOME: [f64; 3] = [-8.0, 0.0, 15.0];
pub(super) const FLOOR: [f64; 3] = [5.0, 5.0, 10.0];
const TOP: [f64; 3] = [-28.0, 18.0, 20.0];

/// A group of the face at `at`, machined or avoided as `action`, with offsets
/// `(radial, axial)`.
pub(super) fn group(
    action: SurfaceAction,
    at: [f64; 3],
    (radial, axial): (f64, f64),
) -> SurfaceGroup {
    SurfaceGroup {
        action,
        radial,
        axial,
        faces: vec![FeatureRef {
            model: "part".into(),
            index: 0,
            at,
        }],
    }
}

/// A job that roughs the part and then finishes it with the 6 mm ball by
/// `strategy`, as `name`: a new one but for `change`, read back, so its
/// expressions are worked out as a job read from disk has them. Scallop's,
/// Spiral's and Contour's tests' too.
pub(super) fn job(name: &str, strategy: &str, change: impl FnOnce(&mut Operation)) -> Job {
    job_with(name, (strategy, "ball6"), change)
}

/// [`job`], finishing with `tool`: Flat's tests', with the 6 mm flat.
pub(super) fn job_with(
    name: &str,
    (strategy, tool): (&str, &str),
    change: impl FnOnce(&mut Operation),
) -> Job {
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
    let mut finish = make(name, strategy, tool);
    change(&mut finish);
    job.setups[0].operations.extend([rough, finish]);
    parse_job(&job_text(&job).unwrap(), "ops_3d").unwrap()
}

/// `name`, a new Parallel but for `change`, held to Fusion's program for it,
/// to `bounds`; Fusion's own rapids may meet the roughing's stock `contacts`
/// times, each 1.35 mm deep at most. camcraft's run passes every check, CC111
/// among them, and neither program cuts into the part.
///
/// The bounds are not the 2D operations' (all but a column in a thousand
/// alike), for what camcraft's Parallel does with no groups as well: it
/// leaves the walls to rings round them (decision 0014), which take down the
/// steps Fusion's passes leave on the walls they run along, up to 6.7 mm, and
/// feed a third as far again; and over the dome Fusion's leaves up to 0.13 mm
/// less, in a band a tolerance apart over all of it, which is Parallel's
/// alone, there with no groups. With groups, three things more, measured:
///
/// - Fusion's passes run on past where the ball rests on a face machined and
///   one avoided at once, up the face avoided: round the dome's foot its
///   points reach Z10.69 with no clearance, where camcraft's tie is at Z10,
///   8.89 from the dome's axis, and Z10.93 with 1 mm, where camcraft's is at
///   Z10 again, 10.04 out. So Fusion cuts what the roughing left on the
///   dome's flank there, up to 3.2 mm lower.
/// - camcraft's ring round the foot of a wall, touching the floor machined
///   and the wall avoided, is kept, and sweeps what the roughing left along
///   the wall all the way, where Fusion's passes touch it only where each
///   ends: with only the floor machined, up to 3.7 mm lower along the walls
///   Fusion's passes run along.
/// - camcraft lifts over a face avoided and comes straight down onto the
///   next piece, as Fusion does, each piece taken from its nearer end
///   (decision 0034). With only the floor machined it lifts 9 times to
///   Fusion's 2, the rest of its links refused as resting on a face avoided,
///   and feeds 11% further.
/// - Its links between pieces are not Fusion's, and staying down over the part
///   they take a little of what the roughing left where Fusion's do not.
///   Taken in the nearest order and from either end, rather than each ramped
///   onto, the pieces leave 0.1 to 0.4% fewer of the columns alike, and with
///   only the floor machined 0.59% more removed than Fusion's, where it was
///   0.38%.
///
/// Each bound is this run's figure with a little room.
fn holds(name: &str, change: impl FnOnce(&mut Operation), bounds: &Bounds, contacts: u64) {
    held_to(&job(name, "parallel", change), name, bounds, contacts);
}

/// A new Parallel leaves what Fusion's does: 74.4% of the 735,504 columns
/// either cuts alike, the median difference 0.0046 mm and the 95th percentile
/// 0.061, 1.8% more removed and 35% more fed, the rings round the walls; each
/// lifts once.
#[test]
fn a_new_parallel_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.74,
        median: 0.006,
        p95: 0.07,
        removed: 0.02,
        fed: 0.37,
        lowest: 1e-3,
    };
    holds("parallel", |_| {}, &bounds, 1);
}

/// The dome avoided, 1 mm sideways and 1 mm down: 69.7% of 642,657 columns
/// alike, the median 0.0048 mm and the 95th percentile 0.106, 1.9% more
/// removed and 39% more fed; camcraft lifts 11 times to Fusion's 15. Neither
/// touches the dome but round its foot.
#[test]
fn a_parallel_avoiding_the_dome_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.69,
        median: 0.006,
        p95: 0.12,
        removed: 0.022,
        fed: 0.41,
        lowest: 1e-3,
    };
    holds(
        "parallel_avoid_dome",
        |op| {
            op.surfaces
                .groups
                .push(group(SurfaceAction::Avoid, DOME, (1.0, 1.0)));
        },
        &bounds,
        8,
    );
}

/// The dome avoided with no clearance: 72.0% of 676,257 columns alike, the
/// median 0.0047 mm and the 95th percentile 0.077, 1.6% more removed and 37%
/// more fed; camcraft lifts 9 times to Fusion's 2.
#[test]
fn a_parallel_avoiding_the_dome_without_clearance_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.71,
        median: 0.006,
        p95: 0.085,
        removed: 0.02,
        fed: 0.40,
        lowest: 1e-3,
    };
    holds(
        "parallel_avoid_dome_0",
        |op| {
            op.surfaces
                .groups
                .push(group(SurfaceAction::Avoid, DOME, (0.0, 0.0)));
        },
        &bounds,
        1,
    );
}

/// The model avoided, the floor alone machined: 55.2% of 413,366 columns
/// alike, the median 0.0078 mm and the 95th percentile 0.53, 0.59% more
/// removed and 11% more fed: both cut the floor, up to touching the fillets
/// round it and the dome, and nothing else; camcraft's ring round its edge,
/// Fusion's passes past it at their ends.
#[test]
fn a_parallel_machining_the_floor_alone_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.54,
        median: 0.01,
        p95: 0.56,
        removed: 0.006,
        fed: 0.13,
        lowest: 1e-3,
    };
    holds(
        "parallel_floor_only",
        |op| {
            op.surfaces.model.action = SurfaceAction::Avoid;
            op.surfaces
                .groups
                .push(group(SurfaceAction::Machine, FLOOR, (0.0, 0.0)));
        },
        &bounds,
        0,
    );
}

/// The floor machined with 0.5 mm of stock of its own each way: 65.1% of
/// 643,537 columns alike, the median 0.0055 mm and the 95th percentile 0.13,
/// 1.9% more removed and 33% more fed. Both leave the floor at Z10.5, which
/// the roughing had left it at already; the lowest each goes is where the
/// ball rests on the fillets beside it, Z10.38 for camcraft's and Z10.337 for
/// Fusion's, which runs on a little past where the two meet.
#[test]
fn a_parallel_leaving_stock_on_the_floor_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.645,
        median: 0.007,
        p95: 0.14,
        removed: 0.021,
        fed: 0.36,
        lowest: 0.05,
    };
    holds(
        "parallel_floor_leave",
        |op| {
            op.surfaces
                .groups
                .push(group(SurfaceAction::Machine, FLOOR, (0.5, 0.5)));
        },
        &bounds,
        1,
    );
}

/// The block's top avoided, 2 mm down: 73.7% of 705,115 columns alike, the
/// median 0.0047 mm and the 95th percentile 0.062, 1.9% more removed and 89%
/// more fed. Neither cuts the top, nor rolls over its edges into the pocket,
/// where the ball would rest on the top raised. camcraft lifts 3 times to
/// Fusion's 2: in the order it now takes the pieces, two of its links would
/// rest on the top.
#[test]
fn a_parallel_avoiding_the_top_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.73,
        median: 0.006,
        p95: 0.07,
        removed: 0.021,
        fed: 0.91,
        lowest: 1e-3,
    };
    holds(
        "parallel_avoid_top",
        |op| {
            op.surfaces
                .groups
                .push(group(SurfaceAction::Avoid, TOP, (0.0, 2.0)));
        },
        &bounds,
        1,
    );
}
