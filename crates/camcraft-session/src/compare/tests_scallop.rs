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
/// the floor, the top and the dome, and since decision 0035 down the pocket's
/// walls and round the R8 too: camcraft's passes cut every slope themselves,
/// as Fusion's do, where rings round the walls took down the steps the lines
/// left there (decision 0014). 94.6% of the columns alike where it was 89.2%,
/// the 95th percentile 0.010 mm where it was 0.015, and 2% further fed where
/// it was 16%. Its passes are cut in the field's order, not the nearest next
/// (decision 0035), and it lifts 12 times to Fusion's once where, reordered,
/// it lifted twice: where the rings close in on the block's top, the pocket's
/// floor and the dome, the field's regions are cut one after another, and
/// some of the ways between them are further than it stays down. With
/// groups, measured:
///
/// - camcraft lays its lines anew round what it avoids, as Fusion does
///   (decision 0035): the edge of the ground machined is an edge of the field,
///   a pass along it. Fusion's start from 10.2 mm out from the dome's axis,
///   where the ball kept clear of the dome stops; camcraft's from where the
///   ball goes from resting on the floor to resting on the dome, found
///   exactly (decision 0037), so round the dome the cusps fall elsewhere:
///   58.7% of the columns alike where, laid across the dome and cut out of
///   it, 61.8% were. The bound is loosened for that, a choice of where the
///   first ring runs that is camcraft's own.
/// - Fusion's passes run on up the faces avoided past where the ball rests
///   on one machined and one avoided at once, as its Parallel's do: with only
///   the floor machined, up the fillets and walls round it, cutting what the
///   roughing left there up to 7.8 mm lower than camcraft, which stops where
///   the ball touches both.
/// - Laid round what is avoided, camcraft's lines are no longer cut into
///   pieces by it: with only the floor machined it lifts 3 times, as Fusion's
///   does, where it lifted 12, and feeds 24% less than Fusion's, its passes
///   stopping at the tie where Fusion's run on up the walls; with the dome
///   avoided 7 times to Fusion's 8, where it lifted 15, and 3% less. With
///   only the floor machined 42.4% of the columns alike where 42.8% were, and
///   the bounds on that and on what it feeds are loosened for it: its passes
///   stopping at the tie are camcraft's own (decision 0033). Since decision
///   0037 the tie is found exactly, where it was found to within half the
///   tolerance of the floor, a little further up the fillet: the median
///   difference there went from 0.020 mm to 0.034, and 2% less is removed
///   and fed.
///
/// Each bound is this run's figure with a little room.
fn holds(name: &str, change: impl FnOnce(&mut Operation), bounds: &Bounds, contacts: u64) {
    held_to(&job(name, "scallop", change), name, bounds, contacts);
}

/// A new Scallop leaves what Fusion's does: 94.6% of the 733,643 columns
/// either cuts alike, the median difference 0.0029 mm and the 95th
/// percentile 0.010, 0.01% less removed and 2.0% more fed. It was 89.2%,
/// 0.0031, 0.015, 0.03% more and 16% more.
#[test]
fn a_new_scallop_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.94,
        median: 0.0035,
        p95: 0.012,
        removed: 0.001,
        fed: 0.03,
        lowest: 1e-3,
    };
    holds("scallop", |_| {}, &bounds, 0);
}

/// The dome avoided, 1 mm sideways and 1 mm down: 58.7% of 685,934 columns
/// alike, the median 0.0065 mm and the 95th percentile 0.25, 1.1% less
/// removed and 2.6% less fed; it lifts 7 times to Fusion's 8. It was 61.8%,
/// 0.0056, 0.24, 1.2% less and 12% more, lifting 15 times: its rings now go
/// round the dome, as Fusion's do, but from a ring of their own, found
/// exactly where the ball goes from the floor to the dome (decision 0037),
/// where it lifted 10 times.
#[test]
fn a_scallop_avoiding_the_dome_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.58,
        median: 0.007,
        p95: 0.27,
        removed: 0.015,
        fed: 0.04,
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

/// The model avoided, the floor alone machined: 42.4% of 447,140 columns
/// alike, the median 0.034 mm and the 95th percentile 3.7, 12.1% less
/// removed, Fusion's passes running on up the fillets and walls, and 24% less
/// fed; it lifts 3 times, as Fusion's does. It was 42.8%, 0.028, 3.6, 11.6%
/// less and 9% less, lifting 12 times; then, the floor's edge found to
/// within half the tolerance, 42.0%, 0.020, 3.4, 10.1% and 22%. Fusion's
/// rapids meet the roughing's stock once, 1.96 mm deep.
#[test]
fn a_scallop_machining_the_floor_alone_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.41,
        median: 0.04,
        p95: 3.75,
        removed: 0.13,
        fed: 0.26,
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
