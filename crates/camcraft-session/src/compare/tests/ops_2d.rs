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
//! flats (18,000 rev/min, 1500 mm/min, plunging and ramping at 500), 6 mm
//! 90° engraving tool and largest thread mill (M12 x 1.75, 600 mm/min); each was
//! posted with Fusion's Masso post to `fixtures/nc/ops_2d_*.nc`.
//!
//! [`ops_2d_job`] is the part, its stock, setup and tools, with the
//! operations a test gives it, each made as the app makes a new one; and
//! [`held_to_fusion`] holds each of them to Fusion's program for it, as
//! `thread_held` holds a thread's, but for the blind hole's floor its lead goes
//! into.

use std::path::Path;

use camcraft_core::job::{FeatureRef, Job, Operation, Outline, StockToLeave, StrategySpec};
use camcraft_core::path::toolpath::{MoveKind, MoveRole};
use camcraft_core::run::{OperationResult, StrategyDetail};

use crate::compare::{Compared, compare};
use crate::test_support::fixture;
use crate::{Ran, job_text, load_models, models_of, parse_job, run_setup};

/// The part, its stock `margin` round it, its one setup, `top`, and Fusion's
/// tools: `flat6` (T1), `flat8` (T3), `vee`, the 6 mm 90° engraving tool
/// (T4), and `mill`, the sample library's largest thread mill, M12 x 1.75, a
/// single 60° tooth 9.14 mm across on a neck 22 mm long (T5).
fn base(margin: [f64; 3]) -> String {
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

[tools.mill]
kind = "thread-mill"
diameter = 9.14
pitch = 1.75
angle = 60.0
number = 5
rpm = 18000
comment = "M12-1.75 Thread Mill"
flutes = 3
flute_length = 1.75
shoulder_length = 22.0
shank_diameter = 9.525
overall_length = 77.0
stickout = 30.0

[[model]]
name = "part"
path = "{}"

[[stock]]
name = "block"
kind = "around-models"
margin = [{:?}, {:?}, {:?}]

[[setup]]
name = "top"
models = ["part"]
stocks = ["block"]
post = "masso"
"#,
        fixture("ops_2d.step")
            .display()
            .to_string()
            .replace('\\', "/"),
        margin[0],
        margin[1],
        margin[2],
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
/// for the engraving tool, 600 for the thread mill, plunging at a third of it.
pub(super) fn new_operation(job: &Job, name: &str, strategy: &str, tool: &str) -> Operation {
    let mut op = Operation::fresh(
        name,
        StrategySpec::named(strategy).unwrap(),
        (Some(tool.to_string()), job.tools.get(tool), None),
    );
    op.passes.feed = match tool {
        "vee" => 1000.0,
        "mill" => 600.0,
        _ => 1500.0,
    };
    op
}

/// The part's job with the operations `make` gives it, each made with
/// [`new_operation`], read back so its expressions are worked out as a job
/// read from disk has them.
pub(super) fn ops_2d_job(make: impl FnOnce(&Job) -> Vec<Operation>) -> Job {
    ops_2d_job_in([5.0, 5.0, 0.0], make)
}

/// [`ops_2d_job`] with the stock `margin` round the part instead of Fusion's
/// 5 mm.
pub(super) fn ops_2d_job_in(margin: [f64; 3], make: impl FnOnce(&Job) -> Vec<Operation>) -> Job {
    let mut job = parse_job(&base(margin), "ops_2d").unwrap();
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

/// The holes bored to size first, as a thread is milled into a hole already
/// made: Fusion's Bore in three stepovers, held to its program above, which
/// leaves each hole clear to its wall and its floor.
fn bored(job: &Job) -> Operation {
    let mut op = new_operation(job, "bored", "bore", "flat6");
    op.selection.faces = HOLES.iter().map(|&at| pick(at)).collect();
    if let StrategySpec::Bore {
        multiple_passes,
        stepovers,
        ..
    } = &mut op.strategy
    {
        *multiple_passes = true;
        *stepovers = 3;
    }
    op
}

/// A new Thread of both holes with the thread mill, its pitch diameter offset
/// `offset`, after [`bored`].
fn threaded(job: &Job, name: &str, offset: f64) -> Vec<Operation> {
    let mut op = new_operation(job, name, "thread", "mill");
    op.selection.faces = HOLES.iter().map(|&at| pick(at)).collect();
    if let StrategySpec::Thread {
        pitch_diameter_offset,
        ..
    } = &mut op.strategy
    {
        *pitch_diameter_offset = offset;
    }
    vec![bored(job), op]
}

/// The lowest and highest the tool's tip goes at the cutting feed, `feed`,
/// as `steps` read back have it: along its leads and its helices, and not
/// what links them.
fn fed_heights(steps: &[camcraft_post::readback::Step], feed: f64) -> (f64, f64) {
    use camcraft_post::readback::Action;
    steps
        .iter()
        .filter_map(|s| match &s.action {
            Action::Move(m) if m.feed.is_some_and(|f| (f - feed).abs() < 1e-9) => m.to[2],
            _ => None,
        })
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| {
            (lo.min(z), hi.max(z))
        })
}

/// Where each helix starts, as `steps` read back have it: the end of each arc
/// whose next move is an arc about the hole's own axis, a turn's worth of them
/// a hole, here the end of each lead's arc, the point after a straight from
/// the hole's centre and an arc.
fn helix_starts(steps: &[camcraft_post::readback::Step]) -> Vec<[f64; 3]> {
    use camcraft_post::readback::Action;
    let moves: Vec<&camcraft_post::readback::Motion> = steps
        .iter()
        .filter_map(|s| match &s.action {
            Action::Move(m) => Some(m),
            _ => None,
        })
        .collect();
    moves
        .windows(3)
        .filter(|w| w[0].feed.is_some() && w[0].arc.is_none() && w[1].arc.is_some())
        .filter(|w| w[2].arc.is_some())
        .filter_map(|w| match w[1].to {
            [Some(x), Some(y), Some(z)] => Some([x, y, z]),
            _ => None,
        })
        .collect()
}

/// The columns of `material` the part check calls a gouge, judged against
/// `ran`'s part as the check judges them, outside the circle of `radius`
/// about `at`.
fn gouged_outside(
    ran: &Ran,
    material: &camcraft_sim::stock::Stock,
    (at, radius): ([f64; 2], f64),
) -> usize {
    let field = material.field();
    let budget = ran.result.verify_budget;
    let part = ran.result.part.lookup(field);
    let mut out = 0;
    for iy in 0..field.ny() {
        for ix in 0..field.nx() {
            let (Some(p), Some(h)) = (field.position(ix, iy), field.get(ix, iy)) else {
                continue;
            };
            let Some((_, under)) = part(ix, iy, p, h, budget) else {
                continue;
            };
            let inside = (p.x - at[0]).hypot(p.y - at[1]) <= radius;
            if under > budget && !inside {
                out += 1;
            }
        }
    }
    out
}

/// The blind hole's floor, where each program's lead goes below it.
const BLIND: ([f64; 2], f64) = ([-30.0, 35.0], 7.0);

/// `job`'s thread `op` held to Fusion's program for it, as
/// [`held_to_fusion`] holds the others, but for the blind hole's floor: both
/// programs' leads go below it, as Fusion's Thread leads in a turn's pitch
/// below a thread's bottom, so both gouge it, as deep as each other, and
/// nowhere else; the run says why (CC224) and the part check says it is cut
/// into (CC107), as nothing else fails. Every column either cuts agrees but
/// for `odd` in a thousand, and they remove as much; they go as low and as
/// high, feed as far, and start each helix at the same point.
fn thread_held(job: &Job, op: &str, program: &str, odd: usize, dip: f64) -> (Ran, Compared) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let loaded = load_models(job, dir, &[0]).unwrap();
    let ran = run_setup(job, 0, &models_of(&loaded)).unwrap();
    let failed: Vec<&str> = ran.checks.failures().map(|(_, f)| f.code).collect();
    assert_eq!(failed, ["CC107"], "{:?}", ran.checks);
    let said = ran
        .checks
        .groups
        .iter()
        .flat_map(|g| &g.findings)
        .find(|f| f.code == "CC224");
    assert!(
        said.is_some_and(|f| f.message.contains(&format!("{dip:.3} mm"))),
        "{:?}",
        ran.checks
    );
    let theirs = fusion(program);
    let c = compare(job, 0, dir, &ran, op, &theirs, job.tolerance).unwrap();
    let d = &c.difference;
    let say = format!(
        "{op}: {d:?}\n{:?}\n{:?}\n{:?}\n{:?}",
        c.ours, c.theirs, c.ours_verify, c.theirs_verify
    );
    assert!(d.cut > 0, "{say}");
    // The floor gouged as deep by each, its lead's dip, and nothing else.
    for v in [&c.ours_verify, &c.theirs_verify] {
        assert!(v.gouged > 0, "{say}");
        assert!((v.deepest_gouge_mm - dip).abs() < 0.002, "{say}");
    }
    assert_eq!(c.ours_cut.rapid_contacts, 0, "{say}");
    assert_eq!(c.theirs_cut.rapid_contacts, 0, "{say}");
    assert!(1000 * (d.cut - d.agree) < odd * d.cut, "{say}");
    assert!(d.quantiles[2] <= job.tolerance, "{say}");
    // A thread removes little, so the odd column is a share of it: the
    // volumes differ by no more than the columns that agree may, each by the
    // tolerance, and what the odd ones hold.
    let (ours, theirs_removed) = d.removed;
    let area = d.pitch * d.pitch;
    // Counts of columns, a few hundred thousand at most.
    #[allow(clippy::cast_precision_loss)]
    let most =
        d.agree as f64 * area * job.tolerance + (d.cut - d.agree) as f64 * area * d.quantiles[3];
    assert!((ours - theirs_removed).abs() <= most, "{say}");
    assert!((c.ours.lowest - c.theirs.lowest).abs() < 1e-6, "{say}");
    let fed = c.ours.fed / c.theirs.fed;
    assert!(
        (0.85..1.15).contains(&fed),
        "fed {fed:.3} of Fusion's: {say}"
    );
    (ran, c)
}

/// `ran`'s thread `op` moves as Fusion's `program` does: as low and as high
/// at the cutting feed, and each helix starting where Fusion's does, in the
/// same order.
fn moves_as_fusions(job: &Job, ran: &Ran, op: &str, program: &str) {
    let theirs = camcraft_post::readback::read_as(
        &fusion(program),
        camcraft_post::readback::Convention::Fanuc,
    )
    .unwrap();
    let ours = ours_steps(job, ran, op);
    let ((lo, hi), (their_lo, their_hi)) = (fed_heights(&ours, 600.0), fed_heights(&theirs, 600.0));
    let say = format!("{op}: fed from Z{lo} to Z{hi}, Fusion's from Z{their_lo} to Z{their_hi}");
    assert!((lo - their_lo).abs() < 1e-6, "{say}");
    assert!((hi - their_hi).abs() < 1e-6, "{say}");
    let (starts, theirs) = (helix_starts(&ours), helix_starts(&theirs));
    assert_eq!(starts.len(), 2, "{starts:?}");
    assert_eq!(starts.len(), theirs.len(), "{starts:?} {theirs:?}");
    for (a, b) in starts.iter().zip(&theirs) {
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() < 1e-6, "{starts:?} {theirs:?}");
        }
    }
}

/// Fusion's Thread, a new one with its thread mill in both holes, bored first:
/// the tooth only touches the walls, so what either program cuts is where its
/// leads go below each hole's bottom: below the part, through the hole 16
/// across, and 0.165 mm into the floor of the blind one, 14 across, which the
/// run says (CC224) and the part check fails (CC107), Fusion's program as
/// much as camcraft's. Move for move, its helix and leads are Fusion's
/// (`strategy::thread`'s tests): here they go as low and as high, feed about
/// as far, and start each helix where Fusion's does.
///
/// Two of the 16,861 columns either cuts differ, by the hole's depth: the two
/// whose centres lie on the through hole's wall itself, 8 mm out along X from
/// its axis, where the tooth reaches exactly. Fusion's tooth, at the radius
/// its program's thousandths put it, takes them; camcraft's, the tool's
/// radius short of the wall by the last bit of a sum, does not; the bore
/// before it took neither. Each is a column of the wall, cut whole or not as
/// its centre falls a hair either side of where both programs' tooth is.
#[test]
fn new_thread_leaves_what_fusions_does() {
    let job = ops_2d_job(|job| threaded(job, "thread", 0.0));
    let (ran, _) = thread_held(&job, "thread", "thread", 1, 0.165);
    let op = made(&ran, "thread");
    let StrategyDetail::Thread(report) = &op.detail else {
        unreachable!("a thread: {:?}", op.detail);
    };
    assert_eq!((report.holes, report.passes), (2, 2));
    assert!(report.rising);
    // Touching the walls, it means to cut nothing.
    assert!(op.meant().is_none());
    moves_as_fusions(&job, &ran, "thread", "thread");
}

/// The same with a pitch diameter offset of a millimetre, as Fusion's
/// `thread_cut`: the tooth cuts half a millimetre into each hole's wall, all
/// the way down, which the model's plain hole does not have. It is meant: the
/// part is checked as threaded where the thread cut, and only the blind
/// hole's floor fails, where both programs' leads go into it. Every column
/// either cuts agrees. A thread cut deeper than asked is not meant: Fusion's
/// program, held to a thread asked for half its offset, gouges the walls.
#[test]
fn a_thread_cut_into_the_wall_is_meant() {
    let job = ops_2d_job(|job| threaded(job, "thread_cut", 1.0));
    let (ran, c) = thread_held(&job, "thread_cut", "thread_cut", 1, 0.136);
    assert_eq!(c.difference.agree, c.difference.cut, "{:?}", c.difference);
    assert!(made(&ran, "thread_cut").meant().is_some());
    let ours = ran.result.keyframes.last().unwrap().unpack().unwrap();
    assert_eq!(gouged_outside(&ran, &ours, BLIND), 0);
    // Fusion's leaves the floor gouged as camcraft's does, and nothing else.
    assert!(
        c.theirs_verify.gouged.abs_diff(c.ours_verify.gouged) <= 5,
        "{c:?}"
    );
    moves_as_fusions(&job, &ran, "thread_cut", "thread_cut");

    // Asked for half the offset, camcraft's cuts nothing past it, and the
    // same program as before cuts a quarter of a millimetre past it into
    // every wall, all the way down, which is a gouge.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let half = ops_2d_job(|job| threaded(job, "thread_cut", 0.5));
    let loaded = load_models(&half, dir, &[0]).unwrap();
    let ran = run_setup(&half, 0, &models_of(&loaded)).unwrap();
    let ours = ran.result.keyframes.last().unwrap().unpack().unwrap();
    assert_eq!(gouged_outside(&ran, &ours, BLIND), 0);
    let deeper = compare(
        &half,
        0,
        dir,
        &ran,
        "thread_cut",
        &fusion("thread_cut"),
        half.tolerance,
    )
    .unwrap();
    let say = format!("{:?}\n{:?}", deeper.ours_verify, deeper.theirs_verify);
    assert!(
        deeper.theirs_verify.gouged > deeper.ours_verify.gouged + 1000,
        "{say}"
    );
    assert!(deeper.theirs_verify.deepest_gouge_mm > 9.0, "{say}");
}

/// camcraft's program for `op` as `ran` made it, read back.
fn ours_steps(job: &Job, ran: &Ran, op: &str) -> Vec<camcraft_post::readback::Step> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let s = &job.setups[0];
    let profile = crate::setup_profile(s, dir, None).unwrap();
    let (text, _) = crate::program(made(ran, op), s.wcs, &profile, op).unwrap();
    camcraft_post::readback::read(&text).unwrap()
}

/// The floors of the two slots: the straight one's, 6 deep, and the angled
/// one's, 4 deep.
const SLOTS: [[f64; 3]; 2] = [[40.0, 35.0, 14.0], [45.0, 8.0, 16.0]];

/// Fusion's Slot of both slots, with the 6 mm flat in them, 8 wide, and with
/// the 8 mm, as wide as they are, each a new operation, leaves what Fusion's
/// does: the middle only, so the 6 mm leaves a millimetre on each wall. Each
/// ramps from 2.5 mm over the stock back and forth along the whole slot, every
/// leg falling as far and no more than half a tenth of the tool, 0.3 or 0.4 a
/// leg, then once along the floor: the angled slot first from its lower-left
/// end, then the straight one from its left. Each needs a job of its own, to be
/// held to Fusion's program from the uncut stock.
#[test]
fn new_slot_leaves_what_fusions_does() {
    for (name, tool, legs) in [("slot", "flat6", 22 + 29), ("slot8", "flat8", 17 + 22)] {
        let job = ops_2d_job(|job| {
            let mut op = new_operation(job, name, "slot", tool);
            op.selection.faces = SLOTS.iter().map(|&at| pick(at)).collect();
            vec![op]
        });
        let ran = held_to_fusion(&job, &[(name, name)], 1);
        let op = made(&ran, name);
        let StrategyDetail::Slot(report) = op.detail else {
            unreachable!("{name} is a slot: {:?}", op.detail);
        };
        assert_eq!((report.slots, report.legs), (2, legs), "{name}");
        // The 6 mm leaves a millimetre on both slots' walls.
        let (narrower, left) = if tool == "flat6" { (2, 1.0) } else { (0, 0.0) };
        assert_eq!(report.narrower, narrower, "{name}");
        assert!((report.most_left - left).abs() < 1e-6, "{name}: {report:?}");
        // Where each slot's ramp starts, in order.
        let starts: Vec<[f64; 3]> = op
            .path
            .steps()
            .filter_map(|(from, m)| {
                let from = from?;
                (m.role == MoveRole::Ramp && from.z > 22.5 - 1e-9).then_some(from.to_array())
            })
            .collect();
        assert_eq!(starts.len(), 2, "{name}: {starts:?}");
        for (got, want) in starts.iter().zip([[32.01, 0.5, 22.5], [25.0, 35.0, 22.5]]) {
            for (g, w) in got.iter().zip(want) {
                assert!((g - w).abs() < 0.01, "{name}: {starts:?}");
            }
        }
    }
}

/// Fusion's Trace along both V grooves' valleys, with the 90° engraving tool on
/// them, a new operation, leaves what Fusion's does. Fusion writes the round
/// groove as some fifty short lines where camcraft writes one arc, so the
/// programs are held by what they cut and how far they feed, not by their
/// moves.
#[test]
fn new_trace_leaves_what_fusions_does() {
    let job = ops_2d_job(|job| {
        let mut op = new_operation(job, "trace", "trace", "vee");
        op.linking.plunge_feed = 300.0;
        op.expressions.remove("linking.plunge_feed");
        op.selection.edges = vec![pick([12.5, -15.0, 18.0]), pick([10.0, -21.0, 18.0])];
        vec![op]
    });
    held_to_fusion(&job, &[("trace", "trace")], 1);
}

/// The floor of the pocket with the island.
const POCKET_FLOOR: [f64; 3] = [-47.0, -22.0, 12.0];

/// A new 2D Adaptive with the 6 mm flat, kept to the floor picked of the
/// pocket with the island, leaves what Fusion's does.
#[test]
fn new_2d_adaptive_leaves_what_fusions_does() {
    let job = ops_2d_job(|job| {
        let mut op = new_operation(job, "adaptive", "adaptive-2d", "flat6");
        op.outline = Outline::Selection;
        op.selection.faces = vec![pick(POCKET_FLOOR)];
        vec![op]
    });
    // Some 320 of its 250,900 columns differ, by up to the pocket's depth,
    // all of them on the walls each program leaves its half millimetre on:
    // camcraft's cut lower in the pocket's round corners, Fusion's round the
    // island. Each is a column whose centre falls within the tolerance of
    // where the two programs' walls are, so it is cut or left whole.
    held_to_fusion(&job, &[("adaptive", "adaptive")], 2);
}

/// The same on the notch open to two sides. Fusion's 2D Adaptive takes an
/// open edge as open to air: its program rapids down beside the part there,
/// into whatever stock stands beside it, where camcraft clears that stock
/// too. So this job's stock is flush with the part, and there is none there.
#[test]
fn new_2d_adaptive_on_an_open_pocket_leaves_what_fusions_does() {
    let job = ops_2d_job_in([0.0, 0.0, 0.0], |job| {
        let mut op = new_operation(job, "adaptive_open", "adaptive-2d", "flat6");
        op.outline = Outline::Selection;
        op.selection.faces = vec![pick([55.0, -39.0, 14.0])];
        vec![op]
    });
    held_to_fusion(&job, &[("adaptive_open", "adaptive_open")], 1);
}

/// The pocket with the island cleared to its floor and walls, nothing left on
/// them: what a chamfer is held to Fusion's on, so that what the two programs
/// cut is the chamfer, and not stock still standing in the pocket, which each
/// program's V would plough through where it leads onto the rim, a different
/// place on it in each.
fn cleared(job: &Job) -> Operation {
    let mut op = new_operation(job, "clear", "adaptive-2d", "flat6");
    op.outline = Outline::Selection;
    op.selection.faces = vec![pick(POCKET_FLOOR)];
    op.passes.stock_to_leave = StockToLeave::default();
    op.expressions
        .retain(|k, _| !k.starts_with("passes.stock_to_leave"));
    op
}

/// A new 2D Chamfer with the engraving tool of the edges picked at `at`.
fn chamfer(job: &Job, name: &str, at: &[[f64; 3]]) -> Operation {
    let mut op = new_operation(job, name, "chamfer-2d", "vee");
    op.selection.edges = at.iter().map(|&p| pick(p)).collect();
    op
}

/// A point on each edge of the pocket's chamfered rim at height `z`, `out`
/// further out than its walls: the straight ones at Y -40, X -12, Y -4 and X
/// -62, the corners 5 round about (-57, -35), (-17, -35), (-17, -9) and
/// (-57, -9).
fn rim(z: f64, out: f64) -> Vec<[f64; 3]> {
    let d = (5.0 + out) * std::f64::consts::FRAC_1_SQRT_2;
    vec![
        [-37.0, -40.0 - out, z],
        [-12.0 + out, -22.0, z],
        [-37.0, -4.0 + out, z],
        [-62.0 - out, -22.0, z],
        [-57.0 - d, -35.0 - d, z],
        [-17.0 + d, -35.0 - d, z],
        [-17.0 + d, -9.0 + d, z],
        [-57.0 - d, -9.0 + d, z],
    ]
}

/// Fusion's 2D Chamfer, each a new operation with the engraving tool, leaves
/// what Fusion's does, the pocket cleared first: the pocket's chamfer picked by
/// its lower loop, with no clearance, as Fusion needs for a chamfer the model
/// has; the island's sharp rim, one edge of it picked, a millimetre wide; and,
/// in a job of its own, since the lower one would have cut it, the pocket's
/// chamfer picked by its upper loop.
#[test]
fn new_2d_chamfer_leaves_what_fusions_does() {
    let job = ops_2d_job(|job| {
        let mut lower = chamfer(job, "chamfer_lower", &rim(19.0, 0.0));
        lower.expressions.remove("clearance");
        if let StrategySpec::Chamfer2d { clearance, .. } = &mut lower.strategy {
            *clearance = 0.0;
        }
        let mut width = chamfer(job, "chamfer_width", &[[-32.0, -22.0, 20.0]]);
        if let StrategySpec::Chamfer2d { width: w, .. } = &mut width.strategy {
            *w = 1.0;
        }
        vec![cleared(job), lower, width]
    });
    held_to_fusion(
        &job,
        &[
            ("chamfer_lower", "chamfer_lower"),
            ("chamfer_width", "chamfer_width"),
        ],
        1,
    );
    let job = ops_2d_job(|job| vec![cleared(job), chamfer(job, "chamfer_upper", &rim(20.0, 1.0))]);
    held_to_fusion(&job, &[("chamfer_upper", "chamfer_upper")], 1);
}
