//! camcraft's 2D operations held to Fusion's, on a part made for them.
//!
//! `fixtures/step/ops_2d.step` was made in Fusion (`scripts/fusion/ops_2d/`): a
//! 140 x 100 x 20 mm block centred on the origin, its bottom at Z0, with a
//! through hole 16 across at (-55, 35), a blind hole 14 across and 10 deep at
//! (-30, 35), a round pocket 30 across and 8 deep at (0, 32), two slots, two V
//! grooves, a pocket with an island, an open notch and a chamfered rim. Each of
//! Fusion's operations on it was set as a new one starts, but for what its
//! program's name says, with the stock 5 mm round the part and level with its
//! top, work zero at the model's origin, and the sample library's 6 mm and 8 mm
//! flats (18,000 rev/min, 1500 mm/min, plunging and ramping at 500) and 6 mm
//! 90° engraving tool; each was posted with Fusion's Masso post to
//! `fixtures/nc/ops_2d_*.nc`.
//!
//! [`ops_2d_job`] is the part, its stock, setup and tools, with the
//! operations a test gives it, each made as the app makes a new one; and
//! [`held_to_fusion`] holds each of them to Fusion's program for it.

use std::path::Path;

use camcraft_core::job::{FeatureRef, Job, Operation, StrategySpec};
use camcraft_core::path::toolpath::{MoveKind, MoveRole};
use camcraft_core::run::OperationResult;

use crate::compare::compare;
use crate::test_support::fixture;
use crate::{Ran, job_text, load_models, models_of, parse_job, run_setup};

/// The part, its stock and its one setup, `top`, and Fusion's tools: `flat6`
/// (T1), `flat8` (T3) and `vee`, the 6 mm 90° engraving tool (T4).
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

[tools.flat8]
kind = "flat"
diameter = 8.0
number = 3
rpm = 18000
flutes = 3
flute_length = 20.0
shoulder_length = 20.0
shank_diameter = 8.0
overall_length = 63.0
stickout = 22.5

[tools.vee]
kind = "cone"
diameter = 6.0
angle = 90.0
number = 4
rpm = 18000
flutes = 2
flute_length = 3.0
shank_diameter = 6.0
overall_length = 50.0
stickout = 20.0

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
        fixture("ops_2d.step")
            .display()
            .to_string()
            .replace('\\', "/")
    )
}

/// A pick on the part at `at`, found there.
pub(super) fn pick(at: [f64; 3]) -> FeatureRef {
    FeatureRef {
        model: "part".into(),
        index: 0,
        at,
    }
}

/// A new operation called `name` of `strategy` with `tool`, as the app makes
/// one, cutting at Fusion's feed for the tool: 1500 mm/min for the flats, 1000
/// for the engraving tool, plunging at a third of it.
pub(super) fn new_operation(job: &Job, name: &str, strategy: &str, tool: &str) -> Operation {
    let mut op = Operation::fresh(
        name,
        StrategySpec::named(strategy).unwrap(),
        (Some(tool.to_string()), job.tools.get(tool), None),
    );
    op.passes.feed = if tool == "vee" { 1000.0 } else { 1500.0 };
    op
}

/// The part's job with the operations `make` gives it, each made with
/// [`new_operation`], read back so its expressions are worked out as a job
/// read from disk has them.
pub(super) fn ops_2d_job(make: impl FnOnce(&Job) -> Vec<Operation>) -> Job {
    let mut job = parse_job(&base(), "ops_2d").unwrap();
    let ops = make(&job);
    job.setups[0].operations = ops;
    parse_job(&job_text(&job).unwrap(), "ops_2d").unwrap()
}

/// Fusion's program `fixtures/nc/ops_2d_{name}.nc`.
fn fusion(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/nc")
            .join(format!("ops_2d_{name}.nc")),
    )
    .unwrap()
}

/// `job` run, every check passed, and each of its operations, by name, held
/// to Fusion's program for it, by its fixture's name: the same material
/// column for column, but for no more than `odd` in a thousand columns cut,
/// on a wall where each is only within its tolerance; the same volume
/// removed, to 0.2%; nearly every column within the tolerance; down to the
/// same lowest point; no gouge and no rapid through material either side; and
/// fed as far, to 15%. The run, for what more a test asks of it.
///
/// `odd` is 1 for most, as `new_basic_operations_leave_what_fusions_do`
/// holds them. A deep round wall has more columns on its edge for the area it
/// cuts: a hole 16 across has some 600 columns of the 0.08 mm grid along its
/// wall, each cut 20 mm deep or not at all as its centre falls a micron either
/// side, so a few dozen of them differ, either way, between two programs each
/// only within its tolerance.
pub(super) fn held_to_fusion(job: &Job, ops: &[(&str, &str)], odd: usize) -> Ran {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let loaded = load_models(job, dir, &[0]).unwrap();
    let ran = run_setup(job, 0, &models_of(&loaded)).unwrap();
    assert!(ran.checks.failures().count() == 0, "{:?}", ran.checks);
    for &(op, program) in ops {
        let c = compare(job, 0, dir, &ran, op, &fusion(program), job.tolerance).unwrap();
        let d = &c.difference;
        let say = format!("{op}: {d:?}\n{:?}\n{:?}", c.ours, c.theirs);
        assert!(d.cut > 0, "{say}");
        assert_eq!(c.ours_verify.gouged, 0, "{say}");
        assert_eq!(c.theirs_verify.gouged, 0, "{say}");
        assert_eq!(c.ours_cut.rapid_contacts, 0, "{say}");
        assert_eq!(c.theirs_cut.rapid_contacts, 0, "{say}");
        assert!(1000 * (d.cut - d.agree) < odd * d.cut, "{say}");
        assert!(d.quantiles[2] <= job.tolerance, "{say}");
        let (ours, theirs) = d.removed;
        assert!((ours - theirs).abs() <= 0.002 * theirs, "{say}");
        assert!((c.ours.lowest - c.theirs.lowest).abs() < 1e-6, "{say}");
        let fed = c.ours.fed / c.theirs.fed;
        assert!(
            (0.85..1.15).contains(&fed),
            "fed {fed:.3} of Fusion's: {say}"
        );
    }
    ran
}

/// The operation called `name` as `ran` made it.
fn made<'a>(ran: &'a Ran, name: &str) -> &'a OperationResult {
    ran.result
        .operations
        .iter()
        .find(|o| o.name == name)
        .unwrap()
}

/// Each helix of a bore's path, from the middle out and hole by hole: the
/// radius it turns at, as the tool's centre goes, and how far it falls a
/// turn.
fn helices(op: &OperationResult) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (from, m) in op.path.steps().filter_map(|(f, m)| Some((f?, m))) {
        let MoveKind::Arc(a) = m.kind else { continue };
        if m.role != MoveRole::Cut || m.to.z >= from.z {
            continue;
        }
        let radius = a.radius(from);
        let per_turn = (from.z - m.to.z) / (a.sweep.abs() / std::f64::consts::TAU);
        if out.last().is_none_or(|(r, _)| (r - radius).abs() > 1e-9) {
            out.push((radius, per_turn));
        }
    }
    out
}

/// A circular's path neither ramps nor stays down between passes: every pass
/// comes down its own lead, and lifts clear of the material after, as
/// Fusion's does. So it lifts once after each pass but the last.
fn lifts_between_passes(op: &OperationResult, passes: u64) {
    let moves = op.path.moves();
    assert!(
        moves.iter().all(|m| m.role != MoveRole::Ramp),
        "{}",
        op.name
    );
    assert_eq!(
        (op.links.ramps, op.links.stayed_down),
        (0, 0),
        "{}",
        op.name
    );
    // Each pass is once round its circle, two half turns, between leads.
    let rings = moves
        .windows(2)
        .filter(|w| w[0].role == MoveRole::Cut && w[1].role != MoveRole::Cut)
        .count() as u64;
    assert_eq!(rings, passes, "{}", op.name);
    let lifts = moves
        .windows(2)
        .filter(|w| w[0].role != MoveRole::Rapid && w[1].role == MoveRole::Rapid)
        .count() as u64;
    assert!(
        lifts >= passes,
        "{}: {lifts} lifts for {passes} passes",
        op.name
    );
}

/// The holes' walls, and the round pocket's.
const HOLES: [[f64; 3]; 2] = [[-47.0, 35.0, 10.0], [-23.0, 35.0, 15.0]];
const POCKET: [f64; 3] = [15.0, 32.0, 16.0];

/// A new Bore of the two holes and a new Circular of the round pocket leave
/// what Fusion's do. The bore's helix falls as far a turn as Fusion's: the
/// circle its tool's edge sweeps, times the tangent of 2°.
#[test]
fn new_bore_and_circular_leave_what_fusions_do() {
    let job = ops_2d_job(|job| {
        let mut bore = new_operation(job, "bore", "bore", "flat6");
        bore.selection.faces = HOLES.iter().map(|&at| pick(at)).collect();
        let mut circular = new_operation(job, "circular", "circular", "flat6");
        circular.selection.faces = vec![pick(POCKET)];
        vec![bore, circular]
    });
    // A single bore's passes leave each hole's core standing, and its lead
    // off the bottom bites the core, through the vertical arc up from it:
    // Fusion's is ten straight moves, camcraft's five within the tolerance,
    // so a hundred and fifty columns of the core it leaves loose differ by
    // up to 0.06 mm.
    let ran = held_to_fusion(&job, &[("bore", "bore"), ("circular", "circular")], 5);
    let tan = 2f64.to_radians().tan();
    let falls = helices(made(&ran, "bore"));
    assert_eq!(falls.len(), 2, "{falls:?}");
    for ((radius, fall), (want_radius, across)) in falls.iter().zip([(5.0, 16.0), (4.0, 14.0)]) {
        assert!((radius - want_radius).abs() < 1e-9, "{falls:?}");
        assert!((fall - across * tan).abs() < 1e-9, "{falls:?}");
    }
    // Eight levels a millimetre apart, from 19 down to the floor at 12.
    let circular = made(&ran, "circular");
    lifts_between_passes(circular, 8);
}

/// With Multiple Passes on, as Fusion's: three stepovers half the tool apart
/// leave each hole two passes, the one past its middle left out and said; four
/// round the pocket, at every level, from the middle out. They need a job of
/// their own: each is held to Fusion's program cut from the material before
/// it, and a bore before them would have cut it.
#[test]
fn bore_and_circular_in_several_passes_leave_what_fusions_do() {
    let job = ops_2d_job(|job| {
        let mut bore = new_operation(job, "bore_passes", "bore", "flat6");
        bore.selection.faces = HOLES.iter().map(|&at| pick(at)).collect();
        let mut circular = new_operation(job, "circular_passes", "circular", "flat6");
        circular.selection.faces = vec![pick(POCKET)];
        for (op, n) in [(&mut bore, 3), (&mut circular, 4)] {
            if let StrategySpec::Bore {
                multiple_passes,
                stepovers,
                ..
            }
            | StrategySpec::Circular {
                multiple_passes,
                stepovers,
                ..
            } = &mut op.strategy
            {
                *multiple_passes = true;
                *stepovers = n;
            }
        }
        vec![bore, circular]
    });
    let ran = held_to_fusion(
        &job,
        &[
            ("bore_passes", "bore_passes"),
            ("circular_passes", "circular_passes"),
        ],
        2,
    );
    let tan = 2f64.to_radians().tan();
    let bore = made(&ran, "bore_passes");
    let falls = helices(bore);
    let want = [(2.0, 10.0), (5.0, 16.0), (1.0, 8.0), (4.0, 14.0)];
    assert_eq!(falls.len(), want.len(), "{falls:?}");
    for ((radius, fall), (want_radius, across)) in falls.iter().zip(want) {
        assert!((radius - want_radius).abs() < 1e-9, "{falls:?}");
        assert!((fall - across * tan).abs() < 1e-9, "{falls:?}");
    }
    assert!(
        bore.warnings.iter().any(|w| w.contains("past the middle")),
        "{:?}",
        bore.warnings
    );
    assert!(
        ran.checks
            .groups
            .iter()
            .flat_map(|g| &g.findings)
            .any(|f| f.code == "CC222"),
        "{:?}",
        ran.checks
    );
    let circular = made(&ran, "circular_passes");
    lifts_between_passes(circular, 8 * 4);
}
