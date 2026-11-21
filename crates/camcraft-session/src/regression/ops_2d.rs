//! camcraft's 2D operations on `fixtures/step/ops_2d.step`: a 140 x 100 x
//! 20 mm block centred on the origin, its bottom at Z0, with a through hole 16
//! across at (-55, 35), a blind hole 14 across and 10 deep at (-30, 35), a round
//! pocket 30 across and 8 deep at (0, 32), two slots, two V grooves, a pocket
//! with an island, an open notch and a chamfered rim. The stock is 5 mm round
//! the part and level with its top, work zero at the model's origin, and the
//! tools 6 mm and 8 mm flats (18,000 rev/min, 1500 mm/min, plunging and ramping
//! at 500), a 6 mm 90° engraving tool (1000 mm/min) and an M12 x 1.75 thread
//! mill (600 mm/min).

use camcraft_core::job::{FeatureRef, Job, Operation, Outline, StockToLeave, StrategySpec};
use camcraft_core::path::toolpath::{MoveKind, MoveRole};
use camcraft_core::run::{OperationResult, StrategyDetail};
use camcraft_post::readback::{self, Action, Motion, Step};
use camcraft_sim::stock::Stock;

use super::{Measured, Recorded, here, holds, measure, run};
use crate::test_support::fixture;
use crate::{Ran, job_text, parse_job};

/// The part, its stock `margin` round it, its one setup, `top`, and its tools:
/// `flat6` (T1), `flat8` (T3), `vee`, the 6 mm 90° engraving tool (T4), and
/// `mill`, an M12 x 1.75 thread mill, a single 60° tooth 9.14 mm across on a
/// neck 22 mm long (T5).
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
fn pick(at: [f64; 3]) -> FeatureRef {
    FeatureRef {
        model: "part".into(),
        index: 0,
        at,
    }
}

/// A new operation called `name` of `strategy` with `tool`, as the app makes
/// one, cutting at the tool's feed: 1500 mm/min for the flats, 1000 for the
/// engraving tool, 600 for the thread mill, plunging at a third of it.
fn new_operation(job: &Job, name: &str, strategy: &str, tool: &str) -> Operation {
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
fn ops_2d_job(make: impl FnOnce(&Job) -> Vec<Operation>) -> Job {
    ops_2d_job_in([5.0, 5.0, 0.0], make)
}

/// [`ops_2d_job`] with the stock `margin` round the part instead of 5 mm.
fn ops_2d_job_in(margin: [f64; 3], make: impl FnOnce(&Job) -> Vec<Operation>) -> Job {
    let mut job = parse_job(&base(margin), "ops_2d").unwrap();
    let ops = make(&job);
    job.setups[0].operations = ops;
    parse_job(&job_text(&job).unwrap(), "ops_2d").unwrap()
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

/// Each helix falls as `want` says, in order: about the radius, as the tool's
/// centre goes, a turn falling the circle its edge sweeps, `across`, times the
/// tangent of 2°.
fn falls_as(op: &OperationResult, want: &[(f64, f64)]) {
    let tan = 2f64.to_radians().tan();
    let falls = helices(op);
    assert_eq!(falls.len(), want.len(), "{falls:?}");
    for ((radius, fall), (want_radius, across)) in falls.iter().zip(want) {
        assert!((radius - want_radius).abs() < 1e-9, "{falls:?}");
        assert!((fall - across * tan).abs() < 1e-9, "{falls:?}");
    }
}

/// A circular's path neither ramps nor stays down between passes: every pass
/// comes down its own lead, and lifts clear of the material after. So it
/// lifts once after each pass but the last.
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

/// A new Bore of the two holes and a new Circular of the round pocket. The
/// bore's helix falls the circle its tool's edge sweeps, times the tangent of
/// 2°, a turn; the circular cuts eight levels a millimetre apart, from 19 down
/// to the floor at 12, lifting between them.
#[test]
fn new_bore_and_circular() {
    let job = ops_2d_job(|job| {
        let mut bore = new_operation(job, "bore", "bore", "flat6");
        bore.selection.faces = HOLES.iter().map(|&at| pick(at)).collect();
        let mut circular = new_operation(job, "circular", "circular", "flat6");
        circular.selection.faces = vec![pick(POCKET)];
        vec![bore, circular]
    });
    let ran = holds(
        &job,
        &[
            (
                "bore",
                Recorded {
                    lifts: 2,
                    fed: 1711.0,
                    lowest: 0.0,
                    most_left: 10.0,
                    left: 26499.4,
                },
            ),
            (
                "circular",
                Recorded {
                    lifts: 8,
                    fed: 677.5,
                    lowest: 12.0,
                    most_left: 10.0,
                    left: 22831.3,
                },
            ),
        ],
    );
    falls_as(made(&ran, "bore"), &[(5.0, 16.0), (4.0, 14.0)]);
    lifts_between_passes(made(&ran, "circular"), 8);
}

/// With Multiple Passes on: three stepovers half the tool apart leave each
/// hole two passes, the one past its middle left out and said; four round the
/// pocket, at every level, from the middle out. They need a job of their own,
/// as a bore before them would have cut what they cut.
#[test]
fn bore_and_circular_in_several_passes() {
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
    let ran = holds(
        &job,
        &[
            (
                "bore_passes",
                Recorded {
                    lifts: 4,
                    fed: 2690.1,
                    lowest: 0.0,
                    most_left: 8.0,
                    left: 26491.1,
                },
            ),
            (
                "circular_passes",
                Recorded {
                    lifts: 32,
                    fed: 1788.6,
                    lowest: 12.0,
                    most_left: 8.0,
                    left: 20841.7,
                },
            ),
        ],
    );
    let bore = made(&ran, "bore_passes");
    falls_as(bore, &[(2.0, 10.0), (5.0, 16.0), (1.0, 8.0), (4.0, 14.0)]);
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
    lifts_between_passes(made(&ran, "circular_passes"), 8 * 4);
}

/// The holes bored to size first, as a thread is milled into a hole already
/// made: a Bore in three stepovers, which leaves each hole clear to its wall
/// and its floor.
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

/// The moves of `steps`, as read back.
fn motions(steps: &[Step]) -> Vec<&Motion> {
    steps
        .iter()
        .filter_map(|s| match &s.action {
            Action::Move(m) => Some(m),
            _ => None,
        })
        .collect()
}

/// The lowest and highest the tool's tip goes at the cutting feed, `feed`,
/// as `steps` read back have it: along its leads and its helices, and not
/// what links them.
fn fed_heights(steps: &[Step], feed: f64) -> (f64, f64) {
    motions(steps)
        .iter()
        .filter(|m| m.feed.is_some_and(|f| (f - feed).abs() < 1e-9))
        .filter_map(|m| m.to[2])
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| {
            (lo.min(z), hi.max(z))
        })
}

/// Where each helix starts, as `steps` read back have it: the end of each
/// lead's arc, the point after a straight fed from the hole's centre and an
/// arc, where an arc about the hole's own axis follows.
fn helix_starts(steps: &[Step]) -> Vec<[f64; 3]> {
    motions(steps)
        .windows(3)
        .filter(|w| w[0].feed.is_some() && w[0].arc.is_none() && w[1].arc.is_some())
        .filter(|w| w[2].arc.is_some())
        .filter_map(|w| match w[1].to {
            [Some(x), Some(y), Some(z)] => Some([x, y, z]),
            _ => None,
        })
        .collect()
}

/// camcraft's program for `op` as `ran` made it, read back.
fn posted(job: &Job, ran: &Ran, op: &str) -> Vec<Step> {
    let s = &job.setups[0];
    let profile = crate::setup_profile(s, here(), None).unwrap();
    let (text, _) = crate::program(made(ran, op), s.wcs, &profile, op).unwrap();
    readback::read(&text).unwrap()
}

/// The columns of `material` the part check calls a gouge, judged against
/// `ran`'s part as the check judges them, outside the circle of `radius`
/// about `at`.
fn gouged_outside(ran: &Ran, material: &Stock, (at, radius): ([f64; 2], f64)) -> usize {
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

/// The blind hole's floor, where a thread's lead goes below it.
const BLIND: ([f64; 2], f64) = ([-30.0, 35.0], 7.0);

/// `job`'s thread `op`, run and measured. A thread leads in a turn's pitch
/// below its bottom, so its lead goes `dip` into the blind hole's floor: the
/// run says why (CC224) and the part check that it is cut into (CC107), and
/// nothing else fails. The floor is gouged `dip` deep, and nothing outside it;
/// no rapid meets material.
fn thread_run(job: &Job, op: &str, dip: f64) -> (Ran, Measured) {
    let ran = run(job);
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
    let m = measure(job, &ran, op);
    assert!(m.verify.gouged > 0, "{:?}", m.verify);
    assert!(
        (m.verify.deepest_gouge_mm - dip).abs() < 0.002,
        "{:?}",
        m.verify
    );
    assert_eq!(m.rapid_contacts, 0);
    assert_eq!(gouged_outside(&ran, &m.material, BLIND), 0);
    (ran, m)
}

/// `ran`'s thread `op` feeds from `heights`, lowest to highest, and starts
/// its helix in each hole at `starts`, in order, to a micron.
fn moves_as_recorded(job: &Job, ran: &Ran, op: &str, heights: (f64, f64), starts: [[f64; 3]; 2]) {
    let steps = posted(job, ran, op);
    let (lo, hi) = fed_heights(&steps, 600.0);
    let say = format!("{op}: fed from Z{lo} to Z{hi}");
    assert!((lo - heights.0).abs() < 1e-6, "{say}");
    assert!((hi - heights.1).abs() < 1e-6, "{say}");
    let got = helix_starts(&steps);
    assert_eq!(got.len(), starts.len(), "{got:?}");
    for (a, b) in got.iter().zip(&starts) {
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() < 1e-6, "{got:?}");
        }
    }
}

/// A new Thread with the thread mill in both holes, bored first: the tooth
/// only touches the walls, so what it cuts is where its leads go below each
/// hole's bottom: below the part, through the hole 16 across, and 0.165 mm
/// into the floor of the blind one, 14 across.
#[test]
fn new_thread() {
    let job = ops_2d_job(|job| threaded(job, "thread", 0.0));
    let (ran, m) = thread_run(&job, "thread", 0.165);
    m.as_recorded(&Recorded {
        lifts: 2,
        fed: 357.9,
        lowest: -0.117,
        most_left: 8.0,
        left: 26491.1,
    });
    let op = made(&ran, "thread");
    let StrategyDetail::Thread(report) = &op.detail else {
        unreachable!("a thread: {:?}", op.detail);
    };
    assert_eq!((report.holes, report.passes), (2, 2));
    assert!(report.rising);
    // Touching the walls, it means to cut nothing.
    assert!(op.meant().is_none());
    moves_as_recorded(
        &job,
        &ran,
        "thread",
        (-0.117, 20.165),
        [[-51.57, 35.0, 0.0], [-27.57, 35.0, 10.0]],
    );
}

/// With a pitch diameter offset of a millimetre the tooth cuts half a
/// millimetre into each hole's wall, all the way down, which the model's plain
/// hole does not have. It is meant: the part is checked as threaded where the
/// thread cut, and only the blind hole's floor fails, where its lead goes into
/// it. Asked for half that offset, it cuts nothing past it.
#[test]
fn a_thread_cut_into_the_wall_is_meant() {
    let job = ops_2d_job(|job| threaded(job, "thread_cut", 1.0));
    let (ran, m) = thread_run(&job, "thread_cut", 0.136);
    m.as_recorded(&Recorded {
        lifts: 2,
        fed: 413.3,
        lowest: -0.102,
        most_left: 8.0,
        left: 26563.7,
    });
    assert!(made(&ran, "thread_cut").meant().is_some());
    moves_as_recorded(
        &job,
        &ran,
        "thread_cut",
        (-0.102, 20.136),
        [[-51.07, 35.0, 0.0], [-27.07, 35.0, 10.0]],
    );

    let half = ops_2d_job(|job| threaded(job, "thread_cut", 0.5));
    let ran = run(&half);
    let ours = ran.result.keyframes.last().unwrap().unpack().unwrap();
    assert_eq!(gouged_outside(&ran, &ours, BLIND), 0);
}

/// The floors of the two slots: the straight one's, 6 deep, and the angled
/// one's, 4 deep.
const SLOTS: [[f64; 3]; 2] = [[40.0, 35.0, 14.0], [45.0, 8.0, 16.0]];

/// A new Slot of both slots, 8 wide, with the 6 mm flat and with the 8 mm, as
/// wide as they are: the middle only, so the 6 mm leaves a millimetre on each
/// wall. Each ramps from 2.5 mm over the stock back and forth along the whole
/// slot, every leg falling no more than half a tenth of the tool, 0.3 or 0.4 a
/// leg, then once along the floor: the angled slot first from its lower-left
/// end, then the straight one from its left. Each needs a job of its own, cut
/// from the uncut stock.
#[test]
fn new_slot() {
    for (name, tool, legs, recorded) in [
        (
            "slot",
            "flat6",
            22 + 29,
            Recorded {
                lifts: 2,
                fed: 1594.1,
                lowest: 14.0,
                most_left: 10.0,
                left: 25947.4,
            },
        ),
        (
            "slot8",
            "flat8",
            17 + 22,
            Recorded {
                lifts: 2,
                fed: 1244.1,
                lowest: 14.0,
                most_left: 10.0,
                left: 25142.8,
            },
        ),
    ] {
        let job = ops_2d_job(|job| {
            let mut op = new_operation(job, name, "slot", tool);
            op.selection.faces = SLOTS.iter().map(|&at| pick(at)).collect();
            vec![op]
        });
        let ran = holds(&job, &[(name, recorded)]);
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

/// A new Trace along both V grooves' valleys, with the 90° engraving tool on
/// them, plunging at 300 mm/min.
#[test]
fn new_trace() {
    let job = ops_2d_job(|job| {
        let mut op = new_operation(job, "trace", "trace", "vee");
        op.linking.plunge_feed = 300.0;
        op.expressions.remove("linking.plunge_feed");
        op.selection.edges = vec![pick([12.5, -15.0, 18.0]), pick([10.0, -21.0, 18.0])];
        vec![op]
    });
    holds(
        &job,
        &[(
            "trace",
            Recorded {
                lifts: 2,
                fed: 88.7,
                lowest: 18.0,
                most_left: 10.0,
                left: 27722.9,
            },
        )],
    );
}

/// The floor of the pocket with the island.
const POCKET_FLOOR: [f64; 3] = [-47.0, -22.0, 12.0];

/// A new 2D Adaptive with the 6 mm flat, kept to the floor picked of the
/// pocket with the island, leaving its half millimetre on the walls.
#[test]
fn new_2d_adaptive() {
    let job = ops_2d_job(|job| {
        let mut op = new_operation(job, "adaptive", "adaptive-2d", "flat6");
        op.outline = Outline::Selection;
        op.selection.faces = vec![pick(POCKET_FLOOR)];
        vec![op]
    });
    holds(
        &job,
        &[(
            "adaptive",
            Recorded {
                lifts: 5,
                fed: 1538.0,
                lowest: 12.5,
                most_left: 10.0,
                left: 15995.1,
            },
        )],
    );
}

/// The same on the notch open to two sides, the stock flush with the part.
#[test]
fn new_2d_adaptive_on_an_open_pocket() {
    let job = ops_2d_job_in([0.0, 0.0, 0.0], |job| {
        let mut op = new_operation(job, "adaptive_open", "adaptive-2d", "flat6");
        op.outline = Outline::Selection;
        op.selection.faces = vec![pick([55.0, -39.0, 14.0])];
        vec![op]
    });
    holds(
        &job,
        &[(
            "adaptive_open",
            Recorded {
                lifts: 9,
                fed: 362.4,
                lowest: 14.5,
                most_left: 10.0,
                left: 24532.1,
            },
        )],
    );
}

/// The pocket with the island cleared to its floor and walls, nothing left on
/// them, so that what a chamfer cuts is the chamfer, and not stock still
/// standing in the pocket.
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

/// New 2D Chamfers with the engraving tool, the pocket cleared first: the
/// pocket's chamfer picked by its lower loop, with no clearance, as a chamfer
/// the model has needs; the island's sharp rim, one edge of it picked, a
/// millimetre wide; and, in a job of its own, since the lower one would have
/// cut it, the pocket's chamfer picked by its upper loop.
#[test]
fn new_2d_chamfer() {
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
    holds(
        &job,
        &[
            (
                "chamfer_lower",
                Recorded {
                    lifts: 1,
                    fed: 166.3,
                    lowest: 18.0,
                    most_left: 10.0,
                    left: 14364.9,
                },
            ),
            (
                "chamfer_width",
                Recorded {
                    lifts: 1,
                    fed: 46.9,
                    lowest: 18.0,
                    most_left: 10.0,
                    left: 14350.1,
                },
            ),
        ],
    );
    let job = ops_2d_job(|job| vec![cleared(job), chamfer(job, "chamfer_upper", &rim(20.0, 1.0))]);
    holds(
        &job,
        &[(
            "chamfer_upper",
            Recorded {
                lifts: 1,
                fed: 170.6,
                lowest: 19.0,
                most_left: 10.0,
                left: 14350.3,
            },
        )],
    );
}
