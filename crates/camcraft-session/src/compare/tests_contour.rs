//! camcraft's Contour held to Fusion's, with a face avoided.
//!
//! The part is `fixtures/step/ops_3d.step` (`scripts/fusion/ops_3d/part.py`),
//! as for Parallel: a 60 x 40 x 20 mm block centred on the origin, its bottom
//! at Z0, with a pocket 38 x 26 and 10 deep from (-26, -13) to (12, 13), its
//! floor at Z10 and its floor's edges R4; a dome 14 across and 5 tall on its
//! floor about (-8, 0); and the block's top edge at +X rounded R8. Fusion's
//! Contour was set on it as a new one starts, with the sample library's 6 mm
//! ball, the stock 5 mm round the part and level with its top and work zero at
//! the model's origin, and its surface groups as its program's name says
//! (`scripts/fusion/ops_3d/cam.py`), posted with Fusion's Masso post to
//! `fixtures/nc/ops_3d_*.nc`:
//!
//! - `contour`, no groups;
//! - `contour_avoid_dome`, the dome avoided, 1 mm sideways and 1 mm down.
//!
//! The tool at 18,000 rev/min and 1500 mm/min, plunging and ramping at 500.
//! Each job here roughs the part first with camcraft's adaptive clearing and a
//! 6 mm flat, as a new one starts, leaving half a millimetre, then makes one
//! Contour as the app makes a new one ([`Operation::fresh`]), with the same
//! tool, feeds and groups, and cuts Fusion's program from the material the
//! roughing left, with the same tool.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use camcraft_core::job::{Operation, SurfaceAction};

use super::tests_parallel::{DOME, group, job};
use super::tests_pencil::{Bounds, held_to};

/// `name`, a new Contour but for `change`, held to Fusion's program for it,
/// to `bounds`; Fusion's own rapids may meet the roughing's stock `contacts`
/// times. camcraft's run passes every check, CC111 among them, and neither
/// program cuts into the part.
///
/// The bounds are loose for what camcraft's Contour does with no groups: a
/// new one reaches in from outside the part's silhouette
/// (`StrategySpec::starting_containment`), so it goes round the block's
/// outside walls as well, down to Z0, where Fusion's, its tool's centre kept
/// on the silhouette (decision 0027), stops at the pocket's floor, Z10. That
/// is most of the difference: 18 mm at the 95th percentile, half as much
/// again removed, three times as far fed and 15 lifts to Fusion's 3.
/// Inside the pocket the rings are Fusion's levels, a millimetre apart and at
/// the floor. With the dome avoided, measured:
///
/// - Neither goes round the dome above its foot.
/// - At the floor, Z10, Fusion cuts its ring out where the ball touches the
///   floor machined and the dome's clearance at once, 10.04 mm from the
///   dome's axis, and goes round the dome's foot nowhere; camcraft keeps that
///   ring, as it keeps every such tie, and so takes the roughing's half
///   millimetre off the floor all round the dome, where Fusion leaves it.
/// - camcraft lifts 15 times, as it does with nothing avoided; Fusion 4.
///
/// Each bound is this run's figure with a little room.
fn holds(name: &str, change: impl FnOnce(&mut Operation), bounds: &Bounds, contacts: u64) {
    held_to(&job(name, "contour", change), name, bounds, contacts);
}

/// A new Contour leaves what Fusion's does in the pocket: 55.4% of the
/// 701,863 columns either cuts alike, the median difference 0.0073 mm and the
/// 95th percentile 17.75, the block's outside walls, which only camcraft's
/// goes down; 45% more removed and 197% more fed.
#[test]
fn a_new_contour_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.55,
        median: 0.009,
        p95: 18.0,
        removed: 0.46,
        fed: 2.0,
        lowest: 10.001,
    };
    holds("contour", |_| {}, &bounds, 0);
}

/// The dome avoided, 1 mm sideways and 1 mm down: 43.8% of 621,653 columns
/// alike, the median 0.0174 mm and the 95th percentile 17.93, 64% more
/// removed and 238% more fed: the outside walls, and the floor round the
/// dome, which only camcraft's ring there finishes.
#[test]
fn a_contour_avoiding_the_dome_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.43,
        median: 0.02,
        p95: 18.1,
        removed: 0.65,
        fed: 2.42,
        lowest: 10.001,
    };
    holds(
        "contour_avoid_dome",
        |op| {
            op.surfaces
                .groups
                .push(group(SurfaceAction::Avoid, DOME, (1.0, 1.0)));
        },
        &bounds,
        0,
    );
}
