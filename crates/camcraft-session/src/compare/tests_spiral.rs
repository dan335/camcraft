//! camcraft's Spiral held to Fusion's, with a face avoided.
//!
//! The part is `fixtures/step/ops_3d.step` (`scripts/fusion/ops_3d/part.py`),
//! as for Parallel: a 60 x 40 x 20 mm block centred on the origin, its bottom
//! at Z0, with a pocket 38 x 26 and 10 deep from (-26, -13) to (12, 13), its
//! floor at Z10 and its floor's edges R4; a dome 14 across and 5 tall on its
//! floor about (-8, 0); and the block's top edge at +X rounded R8. Fusion's
//! Spiral was set on it as a new one starts, with the sample library's 6 mm
//! ball, the stock 5 mm round the part and level with its top and work zero at
//! the model's origin, and its surface groups as its program's name says
//! (`scripts/fusion/ops_3d/cam.py`), posted with Fusion's Masso post to
//! `fixtures/nc/ops_3d_*.nc`:
//!
//! - `spiral`, no groups;
//! - `spiral_avoid_dome`, the dome avoided, 1 mm sideways and 1 mm down.
//!
//! The tool at 18,000 rev/min and 1500 mm/min, plunging and ramping at 500.
//! Each job here roughs the part first with camcraft's adaptive clearing and a
//! 6 mm flat, as a new one starts, leaving half a millimetre, then makes one
//! Spiral as the app makes a new one ([`Operation::fresh`]), with the same
//! tool, feeds and groups, and cuts Fusion's program from the material the
//! roughing left, with the same tool.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use camcraft_core::job::{Operation, SurfaceAction};

use super::tests_parallel::{DOME, group, job};
use super::tests_pencil::{Bounds, held_to};

/// `name`, a new Spiral but for `change`, held to Fusion's program for it, to
/// `bounds`; Fusion's own rapids may meet the roughing's stock `contacts`
/// times. camcraft's run passes every check, CC111 among them, and neither
/// program cuts into the part.
///
/// The spiral is Fusion's: about the part's middle, the origin, 0.345 mm a
/// lap, out to the part's corners. But a new one of camcraft's is cut from
/// its outer radius in, and Fusion's from the middle out; and camcraft's
/// laps out at the part's corners, where the part cuts them into short
/// arcs, are each lifted between and come down onto 2° along them over what
/// the roughing left, where Fusion's stay down: camcraft lifts 71 times to
/// Fusion's 5, and two thirds of all it feeds is down its ramps. Its rings
/// round the walls (decision 0014) take down the steps the laps leave on the
/// pocket's walls and the R8 round, up to 6.8 mm. With the dome avoided,
/// measured:
///
/// - Fusion turns each lap back where the ball meets the dome's clearance,
///   10.04 mm from its axis, and runs the next lap the other way, staying
///   down; camcraft cuts each lap out where it rests on the dome alone and
///   lifts over to its other side, every lap turning the same way: 121 lifts
///   to Fusion's 6.
/// - Round the dome's foot Fusion's laps run on up the dome past where the
///   ball rests on floor and dome at once, as its Parallel's passes do,
///   cutting what the roughing left on the dome's flank up to 3.0 mm lower.
///
/// Each bound is this run's figure with a little room.
fn holds(name: &str, change: impl FnOnce(&mut Operation), bounds: &Bounds, contacts: u64) {
    held_to(&job(name, "spiral", change), name, bounds, contacts);
}

/// A new Spiral leaves what Fusion's does: 84.9% of the 734,626 columns
/// either cuts alike, the median difference 0.0036 mm and the 95th
/// percentile 0.036, 0.6% more removed and four times as far fed: 12,865 mm
/// at the cutting feed to Fusion's 8,317, and 23,166 down camcraft's ramps.
/// Fusion's rapids meet the roughing's stock once, 1.27 mm deep.
#[test]
fn a_new_spiral_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.84,
        median: 0.005,
        p95: 0.04,
        removed: 0.008,
        fed: 3.5,
        lowest: 1e-3,
    };
    holds("spiral", |_| {}, &bounds, 1);
}

/// The dome avoided, 1 mm sideways and 1 mm down: 80.6% of 641,428 columns
/// alike, the median 0.0035 mm and the 95th percentile 0.054, 0.7% more
/// removed and six times as far fed, 34,766 mm of it down camcraft's ramps.
/// Fusion's rapids meet the roughing's stock twice, 0.48 mm deep at most.
#[test]
fn a_spiral_avoiding_the_dome_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.79,
        median: 0.005,
        p95: 0.06,
        removed: 0.009,
        fed: 5.6,
        lowest: 1e-3,
    };
    holds(
        "spiral_avoid_dome",
        |op| {
            op.surfaces
                .groups
                .push(group(SurfaceAction::Avoid, DOME, (1.0, 1.0)));
        },
        &bounds,
        2,
    );
}
