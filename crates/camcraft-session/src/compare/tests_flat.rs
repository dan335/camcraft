//! camcraft's Flat held to Fusion's, with a face avoided.
//!
//! The part is `fixtures/step/ops_3d.step` (`scripts/fusion/ops_3d/part.py`),
//! as for Parallel: a 60 x 40 x 20 mm block centred on the origin, its bottom
//! at Z0, with a pocket 38 x 26 and 10 deep from (-26, -13) to (12, 13), its
//! floor at Z10 and its floor's edges R4; a dome 14 across and 5 tall on its
//! floor about (-8, 0); and the block's top edge at +X rounded R8. Fusion's
//! Flat was set on it as a new one starts, with the sample library's 6 mm
//! flat end mill, the stock 5 mm round the part and level with its top and
//! work zero at the model's origin, and its surface groups as its program's
//! name says (`scripts/fusion/ops_3d/cam.py`), posted with Fusion's Masso post
//! to `fixtures/nc/ops_3d_*.nc`:
//!
//! - `flat`, no groups;
//! - `flat_avoid_dome`, the dome avoided, 1 mm sideways and 1 mm down.
//!
//! The tool at 18,000 rev/min and 1500 mm/min, plunging and ramping at 500.
//! Each job here roughs the part first with camcraft's adaptive clearing and a
//! 6 mm flat, as a new one starts, leaving half a millimetre, then makes one
//! Flat as the app makes a new one ([`Operation::fresh`]), with the same tool,
//! feeds and groups, and cuts Fusion's program from the material the roughing
//! left, with the same tool.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use camcraft_core::job::{Operation, SurfaceAction};

use super::tests_parallel::{DOME, group, job_with};
use super::tests_pencil::{Bounds, held_to};

/// `name`, a new Flat but for `change`, held to Fusion's program for it, to
/// `bounds`; Fusion's own rapids may meet the roughing's stock `contacts`
/// times. camcraft's run passes every check, CC111 among them, and neither
/// program cuts into the part.
///
/// Both clear the block's top and the pocket's floor, each at its height,
/// the top having nothing on it to take off. The bounds are loose for what
/// camcraft's Flat does with no groups, never held to Fusion's before this:
///
/// - It clears the tessellated dome's top, at Z15, as a flat, where Fusion's
///   leaves the dome alone.
/// - It ramps into each ring along it, where Fusion's comes down onto each
///   piece of flat once and stays down: it feeds about four times as far.
/// - Where the floor is narrowest between the dome's foot and the fillets
///   at its corners, Fusion's rings reach a little further.
///
/// With the dome avoided, measured: neither cuts the dome, and both keep the
/// tool 11 mm from its axis, its radius, the dome's and the clearance. But
/// where that leaves room for the tool only in slivers, a tenth of a
/// millimetre round, between the dome's clearance and the fillets either
/// side of it at X-17.2, camcraft clears each, plunging into the 5.5 mm the
/// roughing left there; Fusion clears none. So camcraft removes three times
/// as much.
///
/// Each bound is this run's figure with a little room.
fn holds(name: &str, change: impl FnOnce(&mut Operation), bounds: &Bounds, contacts: u64) {
    held_to(
        &job_with(name, ("flat", "flat6"), change),
        name,
        bounds,
        contacts,
    );
}

/// A new Flat leaves what Fusion's does: 65.9% of the 235,274 columns either
/// cuts alike, the median difference nothing and the 95th percentile 0.5 mm,
/// 4.3% less removed and 357% more fed. Fusion's rapids meet the roughing's
/// stock three times, 3.9 mm deep at most.
#[test]
fn a_new_flat_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.65,
        median: 0.001,
        p95: 0.51,
        removed: 0.05,
        fed: 3.65,
        lowest: 1e-3,
    };
    holds("flat", |_| {}, &bounds, 3);
}

/// The dome avoided, 1 mm sideways and 1 mm down: 70.5% of 126,216 columns
/// alike, the median nothing and the 95th percentile 5.5 mm, the slivers
/// beside the dome; 210% more removed and 258% more fed. Fusion's rapids
/// meet the roughing's stock once, 3.68 mm deep.
#[test]
fn a_flat_avoiding_the_dome_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.70,
        median: 0.001,
        p95: 5.51,
        removed: 2.15,
        fed: 2.65,
        lowest: 1e-3,
    };
    holds(
        "flat_avoid_dome",
        |op| {
            op.surfaces
                .groups
                .push(group(SurfaceAction::Avoid, DOME, (1.0, 1.0)));
        },
        &bounds,
        1,
    );
}
