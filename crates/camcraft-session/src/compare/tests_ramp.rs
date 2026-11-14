//! camcraft's Ramp held to Fusion's.
//!
//! `fixtures/step/ops_3d.step` was made in Fusion for camcraft's 3D finishing
//! (`scripts/fusion/ops_3d/part.py`): a 60 x 40 x 20 mm block centred on the
//! origin, its bottom at Z0, with a pocket 38 x 26 and 10 deep from (-26, -13)
//! to (12, 13), its -X wall drafted 15°, its corner at (12, -13) R5 and the
//! other three sharp, its floor's edges R4; a dome 14 across and 5 tall on its floor
//! about (-8, 0); and the block's top edge at +X rounded R8. Fusion's Ramp was
//! set on it as a new one starts, but for what its program's name says, with
//! the sample library's 6 mm ball (18,000 rev/min, 1500 mm/min, plunging and
//! ramping at 500), the stock 5 mm round the part and level with its top and
//! work zero at the model's origin (`scripts/fusion/ops_3d/cam.py`), and posted
//! with Fusion's Masso post to `fixtures/nc/ops_3d_*.nc`:
//!
//! - `ramp`, a new Ramp: Maximum Stepdown 1 mm, Ramping Angle the tool's 2°;
//! - `ramp_step2`, Maximum Stepdown 2 mm;
//! - `ramp_angle`, Ramping Angle 0.5°.
//!
//! Each job here roughs the part first with camcraft's adaptive clearing and a
//! 6 mm flat, as a new one starts, leaving half a millimetre, then makes one
//! Ramp as the app makes a new one ([`Operation::fresh`]), with the same tool,
//! feeds and settings, and cuts Fusion's program from the material the roughing
//! left, with the same tool.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)]

use std::path::Path;

use camcraft_core::job::{Operation, StrategySpec};

use super::*;
use crate::test_support::fixture;
use crate::{job_text, load_models, models_of, parse_job, run_setup};

/// The part, its stock and setup, the roughing flat and the ball, with no
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

/// A job that roughs the part and then ramps it, as `name`: a new Ramp but for
/// `change`, read back, so its expressions are worked out as a job read from
/// disk has them.
fn job(name: &str, change: impl FnOnce(&mut Operation)) -> Job {
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
    let mut ramp = make(name, "ramp", "ball6");
    change(&mut ramp);
    job.setups[0].operations.extend([rough, ramp]);
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

/// `job`'s ramp, `name`, run and compared with Fusion's program for it.
fn held(job: &Job, name: &str) -> Compared {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let loaded = load_models(job, dir, &[0]).unwrap();
    let ran = run_setup(job, 0, &models_of(&loaded)).unwrap();
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

/// How near a Ramp's material and program are held to Fusion's: the least
/// share of the columns either cuts that agree to the tolerance, the most the
/// median and the 95th percentile of the difference may be, in millimetres,
/// and how far apart the volumes removed and the lengths fed may be, as
/// shares of Fusion's.
struct Bounds {
    agree: f64,
    median: f64,
    p95: f64,
    removed: f64,
    fed: f64,
}

/// `name`, a new Ramp but for `change`, held to Fusion's program for it, to
/// `bounds`; Fusion's own rapids may meet the roughing's stock `contacts`
/// times.
///
/// The bounds are not the 2D operations' (all but a column in a thousand
/// alike): a helix leaves cusps on the curved ground, the fillets, the dome
/// and the rounded edge, whose places follow where each lap starts and how
/// fast it falls, and those are Fusion's only to within what decision 0027
/// says; and Fusion ends the pocket's wall above its floor where camcraft goes
/// on down. Each bound is this run's figure with a little room.
fn holds(name: &str, change: impl FnOnce(&mut Operation), bounds: &Bounds, contacts: u64) {
    let job = job(name, change);
    let c = held(&job, name);
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
    assert!((c.ours.lowest - c.theirs.lowest).abs() <= 1e-3, "{say}");
    assert!(
        (c.ours.fed - c.theirs.fed).abs() <= bounds.fed * c.theirs.fed,
        "{say}"
    );
}

/// A new Ramp leaves what Fusion's does: 61.7% of the 690,830 columns either
/// cuts alike, the median difference 0.005 mm and the 95th percentile 0.31,
/// 0.6% more removed and 1.5% less fed.
#[test]
fn a_new_ramp_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.60,
        median: 0.01,
        p95: 0.35,
        removed: 0.01,
        fed: 0.03,
    };
    holds("ramp", |_| {}, &bounds, 0);
}

/// With a stepdown of 2 mm: 46.2% of 663,776 alike, the median 0.012 mm and
/// the 95th percentile 0.82, 1.8% more removed and 5.8% more fed. Fusion's
/// pocket wall ends at Z12 here, two levels above the floor, so camcraft's
/// helix down the floor's fillets has the more to differ from.
#[test]
fn a_ramp_two_millimetres_a_lap_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.45,
        median: 0.015,
        p95: 0.9,
        removed: 0.025,
        fed: 0.07,
    };
    holds(
        "ramp_step2",
        |op| op.passes.stepdown = Some(2.0),
        &bounds,
        0,
    );
}

/// With a ramp angle of 0.5°, gentler than a stepdown a lap round every wall
/// but the outside: 60.6% of 691,354 alike, the median 0.006 mm and the 95th
/// percentile 0.32, 0.7% more removed and 1.4% less fed. Fusion's rapid down
/// to Z12 at (-15.36, 7.83), onto the wall it starts from Z11.6 above the
/// pocket's floor, meets the stock the roughing here leaves round the dome's
/// foot, 3.37 mm deep: once.
#[test]
fn a_ramp_at_half_a_degree_leaves_what_fusions_does() {
    let bounds = Bounds {
        agree: 0.59,
        median: 0.01,
        p95: 0.35,
        removed: 0.01,
        fed: 0.03,
    };
    holds("ramp_angle", |op| op.linking.ramp_angle = 0.5, &bounds, 1);
}
