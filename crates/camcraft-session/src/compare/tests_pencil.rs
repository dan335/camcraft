//! camcraft's Pencil held to Fusion's.
//!
//! The part is `fixtures/step/ops_3d.step` (`scripts/fusion/ops_3d/part.py`),
//! as for Ramp: a 60 x 40 x 20 mm block centred on the origin, its bottom at
//! Z0, with a pocket 38 x 26 and 10 deep from (-26, -13) to (12, 13), its -X
//! wall drafted 15°, its corner at (12, -13) R5 and the other three sharp, its
//! floor's edges R4; a dome 14 across and 5 tall on its floor about (-8, 0);
//! and the block's top edge at +X rounded R8. Fusion's Pencil was set on it as
//! a new one starts, but for what its program's name says, with the stock 5 mm
//! round the part and level with its top and work zero at the model's origin
//! (`scripts/fusion/ops_3d/cam.py`), and posted with Fusion's Masso post to
//! `fixtures/nc/ops_3d_*.nc`:
//!
//! - `pencil`, a new Pencil with the sample library's 12 mm ball, larger than
//!   the floor's R4 fillets and the R5 corner: it touches twice all round the
//!   pocket's floor, a radius from its walls, up its corners and round the
//!   dome's foot;
//! - `pencil_ball6`, the same with its 6 mm ball, which fits the fillets: it
//!   touches twice round the dome's foot and up the sharp corners only;
//! - `pencil_bitangency`, the 12 mm ball, Bitangency Angle 40°;
//! - `pencil_stepovers`, the 6 mm ball, Number of Stepovers 2.
//!
//! Each tool at 18,000 rev/min and 1500 mm/min, plunging and ramping at 500.
//! Each job here roughs the part first with camcraft's adaptive clearing and a
//! 6 mm flat, as a new one starts, leaving half a millimetre, then makes one
//! Pencil as the app makes a new one ([`Operation::fresh`]), with the same tool,
//! feeds and settings, and cuts Fusion's program from the material the
//! roughing left, with the same tool.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)]

use std::path::Path;
use std::sync::{Arc, OnceLock};

use camcraft_core::job::{Operation, StrategySpec};
use camcraft_core::run::{Kept, Plan};

use super::*;
use crate::test_support::fixture;
use crate::{job_text, load_models, models_of, parse_job, run_setup, run_setup_planned};

/// The part, its stock and setup, the roughing flat and the two balls, with no
/// operations: Ramp's, Project's, Morph's and Parallel's tests' too.
pub(super) fn base() -> String {
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

/// A job that roughs the part and then pencils it with `tool`, as `name`: a
/// new Pencil but for `change`, read back, so its expressions are worked out
/// as a job read from disk has them.
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
    let mut pencil = make(name, "pencil", tool);
    change(&mut pencil);
    job.setups[0].operations.extend([rough, pencil]);
    parse_job(&job_text(&job).unwrap(), "ops_3d").unwrap()
}

fn fusion(op: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/nc")
            .join(format!("ops_3d_{op}.nc")),
    )
    .unwrap()
}

/// The roughing every job here starts with, and the job as far as it: run
/// once, for them all.
static ROUGHED: OnceLock<(Job, Kept)> = OnceLock::new();

/// `job`'s roughing, its first operation, as the one run of it made it, with
/// the material it left, for a run of `job` to keep rather than work out
/// again ([`Plan::Keep`]). Every job here starts with the same roughing of the
/// same part, stock and setup, and nothing after an operation changes what it
/// makes, so it is the same computation.
fn roughed(job: &Job, dir: &Path) -> Kept {
    let mut prefix = job.clone();
    prefix.setups[0].operations.truncate(1);
    let (first, kept) = ROUGHED.get_or_init(|| {
        let loaded = load_models(&prefix, dir, &[0]).unwrap();
        let ran = run_setup(&prefix, 0, &models_of(&loaded)).unwrap();
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

/// `job`'s operation `name`, after the roughing, run and compared with
/// Fusion's program for it.
pub(super) fn held(job: &Job, name: &str) -> Compared {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let loaded = load_models(job, dir, &[0]).unwrap();
    let kept = roughed(job, dir);
    let after = kept.after.clone();
    let plans = [Plan::Keep(Box::new(kept)), Plan::Generate];
    let ran = run_setup_planned(job, 0, &models_of(&loaded), &plans, &mut |_| {}).unwrap();
    // The material the roughing left, put back as it was, not cut again.
    let put_back = after.as_ref().zip(ran.result.keyframes.get(1));
    assert!(put_back.is_some_and(|(a, b)| Arc::ptr_eq(a, b)), "{name}");
    assert!(
        ran.checks.failures().count() == 0,
        "{name}: {:?}",
        ran.checks
    );
    let c = compare(job, 0, dir, &ran, name, &fusion(name), job.tolerance).unwrap();
    eprintln!(
        "{name}: {:?}\nours {:?}\ntheirs {:?}\nours cut {:?}\ntheirs cut {:?}\n{}",
        c.difference,
        c.ours,
        c.theirs,
        c.ours_cut,
        c.theirs_cut,
        c.difference.map.join("\n")
    );
    c
}

/// How near a Pencil's material and program are held to Fusion's, or a
/// Project's or a Morph's: the least share of the columns either cuts that
/// agree to the tolerance, the most the median and the 95th percentile of the
/// difference may be, in millimetres, how far apart the volumes removed and
/// the lengths fed may be, as shares of Fusion's, and how far apart the lowest
/// each goes, in millimetres.
pub(super) struct Bounds {
    pub(super) agree: f64,
    pub(super) median: f64,
    pub(super) p95: f64,
    pub(super) removed: f64,
    pub(super) fed: f64,
    pub(super) lowest: f64,
}

/// `name`, a new Pencil with `tool` but for `change`, held to Fusion's program
/// for it, to `bounds`; Fusion's own rapids may meet the roughing's stock
/// `contacts` times.
///
/// The bounds are not the 2D operations' (all but a column in a thousand
/// alike). Fusion's lines round the dome's foot lie 0.006 mm nearer the dome
/// than where the ball touches floor and dome, which is where camcraft's are,
/// a band of columns a tolerance apart along them; the lines up the corners
/// run on to the bitangency angle, where Fusion's stop a degree or two short
/// of it; and the links between lines, which both make
/// staying down over the floor through what the roughing left, take other
/// ways, which with the 6 mm ball cut through the roughing's leftovers by the
/// drafted wall and in the corners. Each bound is this run's figure with a
/// little room (decision 0028).
fn holds(
    (name, tool): (&str, &str),
    change: impl FnOnce(&mut Operation),
    bounds: &Bounds,
    contacts: u64,
) {
    held_to(&job(name, tool, change), name, bounds, contacts);
}

/// `job`'s operation `name` held to Fusion's program for it, to `bounds`;
/// Fusion's own rapids may meet the stock `contacts` times.
pub(super) fn held_to(job: &Job, name: &str, bounds: &Bounds, contacts: u64) {
    let c = held(job, name);
    let d = &c.difference;
    let say = format!("{name}: {d:?}\n{:?}\n{:?}", c.ours, c.theirs);
    assert!(d.cut > 0, "{say}");
    // Neither cuts the part, and camcraft's rapids meet nothing.
    assert_eq!(c.ours_verify.gouged, 0, "{say}");
    assert_eq!(c.theirs_verify.gouged, 0, "{say}");
    assert_eq!(c.ours_cut.rapid_contacts, 0, "{say}");
    assert_eq!(c.theirs_cut.rapid_contacts, contacts, "{say}");
    // The material, column by column.
    let cut = d.cut as f64;
    assert!(d.agree as f64 >= bounds.agree * cut, "{say}");
    assert!(d.quantiles[0] <= bounds.median, "{say}");
    assert!(d.quantiles[1] <= bounds.p95, "{say}");
    let (ours, theirs) = d.removed;
    assert!((ours - theirs).abs() <= bounds.removed * theirs, "{say}");
    // As deep, and about as far.
    assert!(
        (c.ours.lowest - c.theirs.lowest).abs() <= bounds.lowest,
        "{say}"
    );
    assert!(
        (c.ours.fed - c.theirs.fed).abs() <= bounds.fed * c.theirs.fed,
        "{say}"
    );
}

/// A new Pencil with the 12 mm ball leaves what Fusion's does: 95.3% of the
/// 524,959 columns either cuts alike, the median difference 0.0015 mm and the
/// 95th percentile 0.0098, 0.09% less removed and 7.3% less fed.
#[test]
fn a_new_pencil_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.94,
        median: 0.003,
        p95: 0.015,
        removed: 0.005,
        fed: 0.09,
        lowest: 1e-3,
    };
    holds(("pencil", "ball12"), |_| {}, &bounds, 0);
}

/// With the 6 mm ball, which fits the fillets: 74.5% of the 266,227 columns
/// alike, the median 0.0021 mm and the 95th percentile 2.35, 4.5% more
/// removed and 9.6% more fed. The lines are Fusion's; the links between them
/// are not, and cut through what the roughing left beside the drafted wall,
/// which Fusion's lift over, and in the corners (decision 0028).
#[test]
fn a_pencil_with_a_ball_that_fits_the_fillets_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.73,
        median: 0.004,
        p95: 2.6,
        removed: 0.06,
        fed: 0.12,
        lowest: 1e-3,
    };
    holds(("pencil_ball6", "ball6"), |_| {}, &bounds, 0);
}

/// With a bitangency angle of 40°, the lines up the corners shorter: 95.3% of
/// 522,675 columns alike, the median 0.0015 mm and the 95th percentile
/// 0.0098, 0.09% less removed and 0.9% more fed.
#[test]
fn a_pencil_at_forty_degrees_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.94,
        median: 0.003,
        p95: 0.015,
        removed: 0.005,
        fed: 0.03,
        lowest: 1e-3,
    };
    holds(
        ("pencil_bitangency", "ball12"),
        |op| {
            if let StrategySpec::Pencil {
                bitangency_angle, ..
            } = &mut op.strategy
            {
                *bitangency_angle = 40.0;
            }
        },
        &bounds,
        0,
    );
}

/// With two stepovers either side of the lines, the 6 mm ball: 69.3% of
/// 522,840 columns alike, the median 0.0024 mm and the 95th percentile 0.73,
/// 2.3% more removed and 8.0% more fed. The passes beside the lines run as
/// Fusion's do, round the dome, over the floor, up the walls and round the
/// corners past the block's edge; their order is not Fusion's, nor the links
/// between them. Fusion's rapid down onto one meets the roughing's stock,
/// 3.33 mm deep: once.
#[test]
fn a_pencil_with_stepovers_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.68,
        median: 0.004,
        p95: 0.85,
        removed: 0.035,
        fed: 0.10,
        lowest: 1e-3,
    };
    holds(
        ("pencil_stepovers", "ball6"),
        |op| {
            if let StrategySpec::Pencil { stepovers, .. } = &mut op.strategy {
                *stepovers = 2;
            }
        },
        &bounds,
        1,
    );
}
