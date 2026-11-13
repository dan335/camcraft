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

fn params(radius: f64) -> SlotParams {
    SlotParams {
        radius,
        stepdown: Some(2.0),
        feed: 1000.0,
        backoff: 0.0,
        repeat: false,
        kept_on_right: true,
        tolerance: 0.01,
    }
}

fn one_slot(r: f64) -> Slot {
    let (middle, radius) = slot_of(&stadium(10.0, r, 0.0, 1), 0.01).unwrap();
    Slot {
        middle,
        radius,
        top: 5.0,
        bottom: 0.0,
    }
}

/// With a tool as wide as the slot, each level is one pass along its middle,
/// a whole stepdown down and the last what is left, each the other way from the
/// one above, so it starts where the last ended.
#[test]
fn a_tool_as_wide_as_the_slot_runs_along_its_middle() {
    let s = one_slot(3.0);
    let (path, report) = slot(std::slice::from_ref(&s), &params(3.0)).unwrap();
    assert_eq!(
        (report.slots, report.passes, report.levels, report.walls),
        (1, 3, 3, 0)
    );
    let links: Vec<DVec3> = path
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Link)
        .map(|m| m.to)
        .collect();
    assert_eq!(links.len(), 3);
    let zs: Vec<f64> = links.iter().map(|p| p.z).collect();
    assert_eq!(zs, [3.0, 1.0, 0.0]);
    assert_eq!(links[1].truncate(), s.middle.end().truncate());
    assert_eq!(links[2].truncate(), s.middle.start.truncate());
    for m in path.moves().iter().filter(|m| m.role == MoveRole::Cut) {
        assert!(m.to.y.abs() < 1e-9, "{}", m.to);
    }
}

/// A narrower tool goes along the middle and then round the walls, climbing,
/// its centre its radius in from them; one more than three times narrower
/// than the slot says how wide a band it leaves; a wider one is refused.
#[test]
fn a_narrower_tool_goes_round_the_walls_too() {
    let s = one_slot(3.0);
    let (path, report) = slot(std::slice::from_ref(&s), &params(2.0)).unwrap();
    assert_eq!((report.walls, report.bands, report.passes), (1, 0, 6));
    let mut at = DVec3::ZERO;
    let mut round = 0.0;
    for m in path.moves() {
        if m.role == MoveRole::Cut {
            let d = s.middle.distance_to(m.to.truncate());
            assert!(d < 1.0 + 1e-6, "{} is {d} out", m.to);
            if let MoveKind::Arc(a) = m.kind {
                round += a.sweep;
            }
        }
        at = m.to;
    }
    let _ = at;
    // Two half turns round the ends at every level, anticlockwise.
    assert!((round - 3.0 * 2.0 * PI).abs() < 1e-6, "{round}");
    let (_, narrow) = slot(std::slice::from_ref(&s), &params(0.5)).unwrap();
    assert_eq!(narrow.bands, 1);
    assert!((narrow.widest_band - 1.5).abs() < 1e-9);
    assert!(matches!(
        slot(&[s], &params(3.5)),
        Err(SlotError::TooWide { .. })
    ));
}

/// Without multiple depths it ramps straight to the floor; the last pass is
/// cut twice where asked, and the tool backs off along the middle at the end.
#[test]
fn one_level_repeated_and_backed_off() {
    let s = one_slot(3.0);
    let p = SlotParams {
        stepdown: None,
        repeat: true,
        backoff: 1.0,
        ..params(3.0)
    };
    let (path, report) = slot(std::slice::from_ref(&s), &p).unwrap();
    assert_eq!((report.levels, report.passes), (1, 2));
    let last = path.moves().last().unwrap();
    assert_eq!(last.role, MoveRole::Lead);
    let end = s.middle.end();
    assert!((last.to.truncate().distance(end.truncate()) - 1.0).abs() < 1e-9);
    assert!(matches!(
        slot(&[Slot { bottom: 5.0, ..s }], &p),
        Err(SlotError::NoDepth { .. })
    ));
}
