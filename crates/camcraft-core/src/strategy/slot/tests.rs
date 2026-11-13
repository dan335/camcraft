use std::f64::consts::{FRAC_PI_2, PI};

use glam::{dvec2, dvec3};

use super::*;
use crate::path::toolpath::MoveKind;

/// The loop round a straight slot from (-`half`, 0) to (`half`, 0), `r`
/// either side, anticlockwise, at `z`: its ends each in `split` arcs.
fn stadium(half: f64, r: f64, z: f64, split: u32) -> Curve {
    let p = |x: f64, y: f64| dvec3(x, y, z);
    let mut pieces = vec![Piece::Line { to: p(half, -r) }];
    let end = |pieces: &mut Vec<Piece>, c: DVec2, from: f64| {
        for k in 1..=split {
            let a = from + PI * f64::from(k) / f64::from(split);
            let to = c + DVec2::from_angle(a) * r;
            pieces.push(Piece::Arc {
                to: p(to.x, to.y),
                centre: c,
                sweep: PI / f64::from(split),
            });
        }
    };
    end(&mut pieces, dvec2(half, 0.0), -FRAC_PI_2);
    pieces.push(Piece::Line { to: p(-half, r) });
    end(&mut pieces, dvec2(-half, 0.0), FRAC_PI_2);
    Curve {
        start: p(-half, -r),
        pieces,
    }
}

/// The loop round a slot curving a quarter turn about the origin, its middle
/// `rho` from it, `r` either side, anticlockwise.
fn curved(rho: f64, r: f64) -> Curve {
    let at = |a: f64, k: f64| DVec2::from_angle(a) * k;
    let p = |q: DVec2| dvec3(q.x, q.y, 0.0);
    Curve {
        start: p(at(0.0, rho - r)),
        pieces: vec![
            Piece::Arc {
                to: p(at(0.0, rho + r)),
                centre: at(0.0, rho),
                sweep: PI,
            },
            Piece::Arc {
                to: p(at(FRAC_PI_2, rho + r)),
                centre: DVec2::ZERO,
                sweep: FRAC_PI_2,
            },
            Piece::Arc {
                to: p(at(FRAC_PI_2, rho - r)),
                centre: at(FRAC_PI_2, rho),
                sweep: PI,
            },
            Piece::Arc {
                to: p(at(0.0, rho - r)),
                centre: DVec2::ZERO,
                sweep: -FRAC_PI_2,
            },
        ],
    }
}

/// A straight slot's middle runs between its ends' centres, half its width
/// from each side, whichever way its loop runs and however its ends are split.
#[test]
fn a_straight_slot_is_found_from_its_loop() {
    for split in [1, 2] {
        for c in [
            stadium(10.0, 3.0, 2.0, split),
            stadium(10.0, 3.0, 2.0, split).reversed(),
        ] {
            let (middle, r) = slot_of(&c, 0.01).unwrap();
            assert!((r - 3.0).abs() < 1e-9);
            let ends = [middle.start.truncate(), middle.end().truncate()];
            assert!(
                ends.contains(&dvec2(-10.0, 0.0)) && ends.contains(&dvec2(10.0, 0.0)),
                "{middle:?}"
            );
            assert!((middle.flat_length() - 20.0).abs() < 1e-9);
            assert_eq!(middle.start.z, 2.0);
        }
    }
}

/// A curved slot's middle is the arc between its sides, about their centre.
#[test]
fn a_curved_slot_runs_round_its_middle() {
    let (middle, r) = slot_of(&curved(20.0, 3.0), 0.01).unwrap();
    assert!((r - 3.0).abs() < 1e-9);
    assert_eq!(middle.pieces.len(), 1);
    let Piece::Arc { centre, sweep, .. } = middle.pieces[0] else {
        panic!("{middle:?}");
    };
    assert!(centre.length() < 1e-9);
    assert!((sweep.abs() - FRAC_PI_2).abs() < 1e-9);
    assert!((middle.start.truncate().length() - 20.0).abs() < 1e-9);
}

/// A rounded rectangle, a square and a loop not lying level are not slots.
#[test]
fn what_is_not_a_slot_is_refused() {
    let square = Curve {
        start: dvec3(0.0, 0.0, 0.0),
        pieces: [[10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]]
            .iter()
            .map(|p| Piece::Line {
                to: dvec3(p[0], p[1], 0.0),
            })
            .collect(),
    };
    assert!(matches!(
        slot_of(&square, 0.01),
        Err(SlotError::NotASlot { .. })
    ));
    // One side leaning out: the width is not constant.
    let mut skewed = stadium(10.0, 3.0, 0.0, 1);
    skewed.pieces[2] = Piece::Line {
        to: dvec3(-10.0, 3.5, 0.0),
    };
    assert!(slot_of(&skewed, 0.01).is_err());
    let mut tilted = stadium(10.0, 3.0, 0.0, 1);
    tilted.start.z = 1.0;
    assert!(matches!(
        slot_of(&tilted, 0.01),
        Err(SlotError::NotLevel { .. })
    ));
}

/// Fusion's own for a 6 mm flat in one of `ops_2d`'s slots: no levels, a
/// ramp stepdown of a tenth of the tool, starting 2.5 mm over the top, at 2°.
fn params(radius: f64) -> SlotParams {
    SlotParams {
        radius,
        stepdown: None,
        feed: 1500.0,
        ramp_feed: 500.0,
        ramp_angle: 2f64.to_radians(),
        ramp_stepdown: 0.6,
        ramp_clearance: 2.5,
        backoff: 0.0,
        repeat: false,
        tolerance: 0.01,
    }
}

/// A slot 30 mm between its ends' centres and `r` either side, its top at 20
/// and its floor at 16, as `ops_2d`'s angled one.
fn one_slot(r: f64) -> Slot {
    let (middle, radius) = slot_of(&stadium(15.0, r, 16.0, 1), 0.01).unwrap();
    let middle = if middle.start.x > middle.end().x {
        middle.reversed()
    } else {
        middle
    };
    Slot {
        middle,
        radius,
        top: 20.0,
        bottom: 16.0,
    }
}

/// Each move's role, where it ends and its feed.
fn moves(path: &Toolpath) -> Vec<(MoveRole, DVec3, f64)> {
    path.moves()
        .iter()
        .map(|m| (m.role, m.to, m.feed.unwrap_or(0.0)))
        .collect()
}

/// As Fusion's program cuts it: from 2.5 mm over the top, back and forth
/// along the whole middle, end to end, every leg falling as far and no more
/// than half the ramp stepdown, 22 legs for 6.5 mm at 0.3 a leg, at the ramp
/// feed; then once along the floor to the other end at the cutting feed. A
/// slot is one pass. A tool narrower than the slot leaves its walls, and says
/// how much is left on each.
#[test]
fn a_slot_is_ramped_back_and_forth_then_cut_along_its_floor() {
    let s = one_slot(4.0);
    let (path, report) = slot(std::slice::from_ref(&s), &params(3.0)).unwrap();
    assert_eq!(
        (report.slots, report.passes, report.levels, report.legs),
        (1, 1, 1, 22)
    );
    assert_eq!(report.narrower, 1);
    assert!((report.most_left - 1.0).abs() < 1e-9);
    let m = moves(&path);
    assert_eq!(m.len(), 1 + 22 + 1);
    assert_eq!(m[0].0, MoveRole::Link);
    assert!(
        m[0].1.distance(dvec3(-15.0, 0.0, 22.5)) < 1e-9,
        "{:?}",
        m[0]
    );
    let fall = 6.5 / 22.0;
    for (k, &(role, to, feed)) in m[1..23].iter().enumerate() {
        assert_eq!((role, feed), (MoveRole::Ramp, 500.0));
        let x = if k % 2 == 0 { 15.0 } else { -15.0 };
        let z = 22.5 - fall * f64::from(u32::try_from(k).unwrap() + 1);
        assert!(to.distance(dvec3(x, 0.0, z)) < 1e-9, "leg {k}: {to}");
    }
    // 22 legs, an even number, end where they started: the floor the other
    // way.
    assert_eq!(m[23].0, MoveRole::Cut);
    assert_eq!(m[23].2, 1500.0);
    assert!(m[23].1.distance(dvec3(15.0, 0.0, 16.0)) < 1e-9);
    // With an 8 mm tool, the slot's own width, 0.4 a leg: 17 legs, ending at
    // the far end, so the floor comes back.
    let wide = SlotParams {
        ramp_stepdown: 0.8,
        ..params(4.0)
    };
    let (path, report) = slot(std::slice::from_ref(&s), &wide).unwrap();
    assert_eq!((report.legs, report.narrower), (17, 0));
    assert!(path.end().unwrap().distance(dvec3(-15.0, 0.0, 16.0)) < 1e-9);
    assert!(matches!(
        slot(&[s], &params(4.5)),
        Err(SlotError::TooWide { .. })
    ));
}

/// On a short slot the ramp's angle governs, not its stepdown: no leg falls
/// more steeply than it.
#[test]
fn the_ramp_is_no_steeper_than_its_angle() {
    let (middle, radius) = slot_of(&stadium(1.0, 3.0, 16.0, 1), 0.01).unwrap();
    let s = Slot {
        middle,
        radius,
        top: 20.0,
        bottom: 16.0,
    };
    let (path, report) = slot(&[s], &params(3.0)).unwrap();
    let most = 2.0 * 2f64.to_radians().tan();
    assert_eq!(
        f64::from(u32::try_from(report.legs).unwrap()),
        (6.5 / most).ceil()
    );
    let mut at = path.moves()[0].to;
    for m in &path.moves()[1..] {
        let run = at.truncate().distance(m.to.truncate());
        assert!(at.z - m.to.z <= run * 2f64.to_radians().tan() + 1e-9);
        at = m.to;
    }
}

/// With multiple depths each level is ramped down to from the one above, the
/// first from over the top, and cut along; all of it one pass.
#[test]
fn levels_are_ramped_down_to_one_after_another() {
    let s = one_slot(3.0);
    let p = SlotParams {
        stepdown: Some(2.0),
        ramp_stepdown: 2.0,
        ..params(3.0)
    };
    let (path, report) = slot(std::slice::from_ref(&s), &p).unwrap();
    // 22.5 to 18 at a millimetre a leg, 18 to 16 likewise.
    assert_eq!((report.levels, report.passes, report.legs), (2, 2, 5 + 2));
    let m = moves(&path);
    assert_eq!(m.iter().filter(|m| m.0 == MoveRole::Link).count(), 1);
    let floors: Vec<f64> = m
        .iter()
        .filter(|m| m.0 == MoveRole::Cut)
        .map(|m| m.1.z)
        .collect();
    assert_eq!(floors, [18.0, 16.0]);
}

/// The floor is cut again where asked, back the way it came, and the tool
/// backs off the end wall along the middle before it lifts; a slot whose
/// floor is not below its top is refused.
#[test]
fn repeated_and_backed_off() {
    let s = one_slot(3.0);
    let p = SlotParams {
        repeat: true,
        backoff: 1.0,
        ..params(3.0)
    };
    let (path, report) = slot(std::slice::from_ref(&s), &p).unwrap();
    assert_eq!(report.passes, 2);
    let m = moves(&path);
    let n = m.len();
    assert_eq!(m[n - 2].0, MoveRole::Cut);
    assert!(m[n - 2].1.distance(dvec3(-15.0, 0.0, 16.0)) < 1e-9);
    assert_eq!(m[n - 1].0, MoveRole::Lead);
    assert!(m[n - 1].1.distance(dvec3(-14.0, 0.0, 16.0)) < 1e-9);
    assert!(matches!(
        slot(&[Slot { bottom: 20.0, ..s }], &p),
        Err(SlotError::NoDepth { .. })
    ));
}

/// A curved slot is ramped along its arc: each leg a helix about its centre.
#[test]
fn a_curved_slot_ramps_down_helices() {
    let (middle, radius) = slot_of(&curved(20.0, 3.0), 0.01).unwrap();
    let s = Slot {
        middle,
        radius,
        top: 4.0,
        bottom: 0.0,
    };
    let (path, report) = slot(std::slice::from_ref(&s), &params(3.0)).unwrap();
    assert!(report.legs > 1);
    for m in path.moves().iter().filter(|m| m.role == MoveRole::Ramp) {
        let MoveKind::Arc(a) = m.kind else {
            panic!("a straight ramp leg to {}", m.to);
        };
        assert!(a.centre.truncate().length() < 1e-9);
        assert!((m.to.truncate().length() - 20.0).abs() < 1e-9);
    }
}
