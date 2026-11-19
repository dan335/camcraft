//! camcraft's Scallop held to Fusion's, with faces machined and avoided.
//!
//! The part is `fixtures/step/ops_3d.step` (`scripts/fusion/ops_3d/part.py`),
//! as for Parallel: a 60 x 40 x 20 mm block centred on the origin, its bottom
//! at Z0, with a pocket 38 x 26 and 10 deep from (-26, -13) to (12, 13), its
//! floor at Z10 and its floor's edges R4; a dome 14 across and 5 tall on its
//! floor about (-8, 0); and the block's top edge at +X rounded R8. Fusion's
//! Scallop was set on it as a new one starts, with the sample library's 6 mm
//! ball, the stock 5 mm round the part and level with its top and work zero at
//! the model's origin, and its surface groups as its program's name says
//! (`scripts/fusion/ops_3d/cam.py`), posted with Fusion's Masso post to
//! `fixtures/nc/ops_3d_*.nc`:
//!
//! - `scallop`, no groups;
//! - `scallop_avoid_dome`, the dome avoided, 1 mm sideways and 1 mm down;
//! - `scallop_floor_only`, the model avoided, the pocket's floor machined.
//!
//! The tool at 18,000 rev/min and 1500 mm/min, plunging and ramping at 500.
//! Each job here roughs the part first with camcraft's adaptive clearing and a
//! 6 mm flat, as a new one starts, leaving half a millimetre, then makes one
//! Scallop as the app makes a new one ([`Operation::fresh`]), with the same
//! tool, feeds and groups, and cuts Fusion's program from the material the
//! roughing left, with the same tool.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use camcraft_core::job::{Operation, SurfaceAction};

use super::tests_parallel::{DOME, FLOOR, group, job};
use super::tests_pencil::{Bounds, held_to};

/// `name`, a new Scallop but for `change`, held to Fusion's program for it,
/// to `bounds`; Fusion's own rapids may meet the roughing's stock `contacts`
/// times. camcraft's run passes every check, CC111 among them, and neither
/// program cuts into the part.
///
/// With no groups the lines are Fusion's to within a few thousandths over
/// the floor, the top and the dome; the material differs where camcraft's
/// rings round the walls take down the steps the lines leave on the pocket's
/// walls and the R8 round (decision 0014), up to 6.1 mm. camcraft feeds about
/// twice as far, two fifths of it at the ramp feed: it lifts 44 times to
/// Fusion's once, and comes back down onto each pass 2° along it over what
/// the roughing left. With groups, measured:
///
/// - Fusion lays its lines anew round what it avoids: on the floor round the
///   dome avoided its points stand in rings about the dome's axis a stepover
///   apart, from 10.2 mm out, where the ball kept clear of the dome stops, as
///   from a boundary of its own. camcraft's are laid as they are with nothing
///   avoided, and cut out where they rest on the dome alone, its ring at the
///   dome's foot kept; so round the dome the cusps fall elsewhere.
/// - Fusion's passes run on up the faces avoided past where the ball rests
///   on one machined and one avoided at once, as its Parallel's do: with only
///   the floor machined, up the fillets and walls round it, cutting what the
///   roughing left there up to 7.8 mm lower than camcraft, which stops where
///   the ball touches both.
/// - Cut into many pieces, camcraft's lines are each ramped onto: with only
///   the floor machined it feeds 1,133 mm at the cutting feed to Fusion's
///   1,173, and 9,517 mm more down its ramps.
///
/// Each bound is this run's figure with a little room.
fn holds(name: &str, change: impl FnOnce(&mut Operation), bounds: &Bounds, contacts: u64) {
    held_to(&job(name, "scallop", change), name, bounds, contacts);
}

/// A new Scallop leaves what Fusion's does: 89.2% of the 733,638 columns
/// either cuts alike, the median difference 0.0031 mm and the 95th
/// percentile 0.015, 0.03% more removed and 99% more fed, 8,065 mm of it
/// down camcraft's ramps.
#[test]
fn a_new_scallop_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.88,
        median: 0.004,
        p95: 0.02,
        removed: 0.003,
        fed: 1.05,
        lowest: 1e-3,
    };
    holds("scallop", |_| {}, &bounds, 0);
}

/// The dome avoided, 1 mm sideways and 1 mm down: 61.7% of 685,840 columns
/// alike, the median 0.0056 mm and the 95th percentile 0.24, 1.1% less
/// removed and 221% more fed. Fusion's lines round the dome are its own,
/// and it lifts 8 times to camcraft's 83.
#[test]
fn a_scallop_avoiding_the_dome_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.60,
        median: 0.007,
        p95: 0.27,
        removed: 0.015,
        fed: 2.3,
        lowest: 1e-3,
    };
    holds(
        "scallop_avoid_dome",
        |op| {
            op.surfaces
                .groups
                .push(group(SurfaceAction::Avoid, DOME, (1.0, 1.0)));
        },
        &bounds,
        0,
    );
}

/// The model avoided, the floor alone machined: 42.9% of 447,140 columns
/// alike, the median 0.028 mm and the 95th percentile 3.6, 11.3% less
/// removed, Fusion's passes running on up the fillets and walls, and nine
/// times as far fed, camcraft's ramps. Fusion's rapids meet the roughing's
/// stock once, 1.96 mm deep.
#[test]
fn a_scallop_machining_the_floor_alone_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.42,
        median: 0.035,
        p95: 3.9,
        removed: 0.125,
        fed: 8.5,
        lowest: 1e-3,
    };
    holds(
        "scallop_floor_only",
        |op| {
            op.surfaces.model.action = SurfaceAction::Avoid;
            op.surfaces
                .groups
                .push(group(SurfaceAction::Machine, FLOOR, (0.0, 0.0)));
        },
        &bounds,
        1,
    );
}
