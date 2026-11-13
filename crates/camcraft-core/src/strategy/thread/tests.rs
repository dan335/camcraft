use super::*;
use crate::path::toolpath::{Move, MoveKind};
use glam::dvec2;

/// Fusion's thread mill: 9.14 mm, a single 60° tooth for 1.75 mm.
const TOOL: f64 = 9.14;
const PITCH: f64 = 1.75;

/// A new Thread of Fusion's, with its thread mill: right hand, climbing, its
/// leads a tenth of the tool round a quarter turn, the straight a twentieth,
/// helical, from the centre.
fn fusions(offset: f64) -> ThreadParams {
    ThreadParams {
        radius: TOOL / 2.0,
        pitch: PITCH,
        offset,
        right_hand: true,
        kept_on_right: true,
        stepovers: 1,
        stepover: 1.0,
        leads: Some(LeadArc {
            radius: 0.1 * TOOL,
            sweep: 0.5 * PI,
            straight: 0.05 * TOOL,
        }),
        helical: true,
        to_centre: true,
        feed: 600.0,
    }
}

/// The ops_2d part's through hole, 16 across, and its blind one, 14 across
/// with its floor at Z10.
fn holes() -> [Round; 2] {
    [
        Round {
            at: dvec2(-55.0, 35.0),
            radius: 8.0,
            top: 20.0,
            bottom: 0.0,
            boss: false,
        },
        Round {
            at: dvec2(-30.0, 35.0),
            radius: 7.0,
            top: 20.0,
            bottom: 10.0,
            boss: false,
        },
    ]
}

/// Each move's end, with where it starts.
fn ends(path: &Toolpath) -> Vec<(DVec3, &Move)> {
    path.steps()
        .filter_map(|(from, m)| Some((from?, m)))
        .collect()
}

fn near(a: DVec3, b: [f64; 3]) -> bool {
    // Fusion's program is written to the thousandth.
    a.distance(DVec3::from_array(b)) < 1.0e-3
}

/// The thread Fusion's program cuts in each hole, move for move, to the
/// thousandth it is written to: down the centre to the lead's start, a
/// straight to the lead's arc, the arc rising onto the helix at +X, the helix
/// up from the bottom anticlockwise in half turns and what is left, the arc
/// off rising as far again, and a straight back to the centre.
#[test]
fn fusions_thread_move_for_move() {
    let (path, report) = thread(&holes(), &fusions(0.0)).unwrap();
    assert_eq!((report.holes, report.passes), (2, 2));
    assert!(report.rising);
    let moves = path.moves();
    // Through hole: the link, the straight, the arc, 22 half turns and what
    // is left, the arc off and the straight back; the blind hole the same
    // with 11 half turns.
    assert_eq!(moves.len(), (3 + 23 + 2) + (3 + 12 + 2));
    let want_through = [
        [-55.0, 35.0, -0.117],
        [-52.484, 34.086, -0.117],
        [-51.57, 35.0, 0.0],
        [-58.43, 35.0, 0.875],
        [-51.57, 35.0, 1.75],
    ];
    for (m, w) in moves.iter().zip(want_through) {
        assert!(near(m.to, w), "{} not {w:?}", m.to);
    }
    assert_eq!(moves[0].role, MoveRole::Link);
    assert_eq!(
        (moves[1].role, moves[2].role),
        (MoveRole::Lead, MoveRole::Lead)
    );
    // Where its helix ends, and its leads off.
    for (m, w) in moves[24..28].iter().zip([
        [-51.57, 35.0, 19.25],
        [-58.09, 36.488, 20.0],
        [-57.663, 35.268, 20.117],
        [-55.0, 35.0, 20.117],
    ]) {
        assert!(near(m.to, w), "{} not {w:?}", m.to);
    }
    // The blind hole's: 0.165 below its floor where the lead starts.
    let blind = &moves[28..];
    for (m, w) in blind.iter().zip([
        [-30.0, 35.0, 9.835],
        [-28.484, 34.086, 9.835],
        [-27.57, 35.0, 10.0],
    ]) {
        assert!(near(m.to, w), "{} not {w:?}", m.to);
    }
    for (m, w) in blind[blind.len() - 3..].iter().zip([
        [-30.541, 32.631, 20.0],
        [-29.446, 33.319, 20.165],
        [-30.0, 35.0, 20.165],
    ]) {
        assert!(near(m.to, w), "{} not {w:?}", m.to);
    }
    // Every cut anticlockwise round the hole's axis, the tool's radius in
    // from the wall, none more than half a turn.
    for (from, m) in ends(&path) {
        if m.role != MoveRole::Cut {
            continue;
        }
        let MoveKind::Arc(a) = m.kind else {
            panic!("a straight cut to {}", m.to)
        };
        assert!(a.sweep > 0.0 && a.sweep <= PI + 1e-9, "{}", a.sweep);
        let r = a.radius(from);
        assert!((r - 3.43).abs() < 1e-9 || (r - 2.43).abs() < 1e-9, "{r}");
        assert!(m.to.z > from.z);
    }
    let lows: Vec<f64> = report.cuts.iter().map(|c| c.lowest).collect();
    assert!(
        (lows[0] + 0.117).abs() < 1e-3 && (lows[1] - 9.835).abs() < 1e-3,
        "{lows:?}"
    );
    // The helix turns: 20 / 1.75 and 10 / 1.75.
    assert!((report.turns - 30.0 / PITCH).abs() < 1e-9);
}

/// With a pitch diameter offset of a millimetre the circle is half a
/// millimetre wider, as Fusion's is, and the leads rise less on it: what the
/// thread is meant to cut is the wall moved half a millimetre out, down to the
/// hole's bottom, and nothing else.
#[test]
fn an_offset_cuts_half_of_it_into_the_wall() {
    let (path, report) = thread(&holes(), &fusions(1.0)).unwrap();
    let moves = path.moves();
    for (m, w) in moves.iter().zip([
        [-55.0, 35.0, -0.102],
        [-51.984, 34.086, -0.102],
        [-51.07, 35.0, 0.0],
        [-58.93, 35.0, 0.875],
    ]) {
        assert!(near(m.to, w), "{} not {w:?}", m.to);
    }
    let cut = report.cuts[0];
    assert!((cut.reach - 8.5).abs() < 1e-9 && cut.hole == 8.0 && cut.bottom == 0.0);
    let at = |d: f64| cut.height_at(dvec2(-55.0 + d, 35.0));
    assert_eq!(at(7.9), None, "inside the hole");
    assert_eq!(at(8.2), Some(0.0));
    assert_eq!(at(8.5), Some(0.0));
    assert_eq!(at(8.6), None, "past what was asked");
    let blind = report.cuts[1];
    assert!((blind.lowest - 9.864).abs() < 1e-3, "{}", blind.lowest);
    assert_eq!(blind.height_at(dvec2(-30.0, 42.4)), Some(10.0));
    // None: it only touches the wall, and nothing is meant.
    let (_, none) = thread(&holes(), &fusions(0.0)).unwrap();
    assert_eq!(none.cuts[0].height_at(dvec2(-47.0, 35.0)), None);
}

/// The hand and the direction decide which way round each helix goes and
/// whether it rises: a right-hand thread climbing rises anticlockwise from
/// the bottom (Fusion's); conventional falls clockwise from the top; a
/// left-hand thread mirrors them: climbing falls anticlockwise, conventional
/// rises clockwise. Each helix starts on +X, and its leads come in and go out
/// the way it turns.
#[test]
fn hand_and_direction() {
    let one = [holes()[0]];
    for (right_hand, climb, ccw, rising) in [
        (true, true, true, true),
        (true, false, false, false),
        (false, true, true, false),
        (false, false, false, true),
    ] {
        let p = ThreadParams {
            right_hand,
            kept_on_right: climb,
            ..fusions(0.0)
        };
        assert_eq!(turning(&p), (ccw, rising));
        let (path, report) = thread(&one, &p).unwrap();
        assert_eq!(report.rising, rising);
        let say = format!("right {right_hand}, climb {climb}");
        let cuts: Vec<(DVec3, &Move)> = ends(&path)
            .into_iter()
            .filter(|(_, m)| m.role == MoveRole::Cut)
            .collect();
        let (first, _) = cuts[0];
        assert!(
            near(first, [-51.57, 35.0, if rising { 0.0 } else { 20.0 }]),
            "{say}: {first}"
        );
        for (from, m) in &cuts {
            let MoveKind::Arc(a) = m.kind else {
                panic!("{say}")
            };
            assert_eq!(a.sweep > 0.0, ccw, "{say}");
            assert_eq!(m.to.z > from.z, rising, "{say}");
        }
        // The leads continue the helix: in below its start rising, above it
        // falling; out past its end the same way.
        let moves = path.moves();
        let (z_in, z_out) = (moves[0].to.z, moves[moves.len() - 1].to.z);
        if rising {
            assert!(
                (z_in + 0.117).abs() < 1e-3 && (z_out - 20.117).abs() < 1e-3,
                "{say}"
            );
        } else {
            assert!(
                (z_in - 20.117).abs() < 1e-3 && (z_out + 0.117).abs() < 1e-3,
                "{say}"
            );
        }
        for m in moves.iter().filter(|m| m.role == MoveRole::Lead) {
            if let MoveKind::Arc(a) = m.kind {
                assert_eq!(a.sweep > 0.0, ccw, "{say}");
            }
        }
    }
}

/// Without helical leads, they are level: at the helix's start and end, and
/// never below the hole's bottom. Without Lead To Center, a pass starts the
/// straight's length before its arc, along it, and ends as far after.
#[test]
fn level_leads_and_leads_from_beside() {
    let one = [holes()[1]];
    let level = ThreadParams {
        helical: false,
        ..fusions(0.0)
    };
    let (path, report) = thread(&one, &level).unwrap();
    assert!((report.cuts[0].lowest - 10.0).abs() < 1e-12);
    for m in path.moves().iter().filter(|m| m.role != MoveRole::Cut) {
        assert!(m.to.z == 10.0 || m.to.z == 20.0, "{}", m.to);
    }
    let beside = ThreadParams {
        to_centre: false,
        ..fusions(0.0)
    };
    let (path, _) = thread(&one, &beside).unwrap();
    let moves = path.moves();
    // The arc's start, (-28.484, 34.086), less the straight along +X.
    assert!(
        near(moves[0].to, [-28.484 - 0.457, 34.086, 9.835]),
        "{}",
        moves[0].to
    );
    assert!(
        near(moves[1].to, [-28.484, 34.086, 9.835]),
        "{}",
        moves[1].to
    );
    let last = moves[moves.len() - 1].to;
    assert!(
        last.truncate().distance(dvec2(-30.0, 35.0)) > 1.0,
        "not back to the centre"
    );
    let none = ThreadParams {
        leads: None,
        ..fusions(0.0)
    };
    let (path, _) = thread(&one, &none).unwrap();
    let moves = path.moves();
    assert!(near(moves[0].to, [-30.0, 35.0, 10.0]));
    assert!(near(moves[1].to, [-27.57, 35.0, 10.0]));
}

/// With multiple passes, each a stepover further in than the next, the last
/// at the thread, each from the centre and back; one past the hole's middle is
/// left out and counted.
#[test]
fn multiple_passes_out_to_the_thread() {
    let one = [holes()[0]];
    let p = ThreadParams {
        stepovers: 3,
        stepover: 1.0,
        ..fusions(1.0)
    };
    let (path, report) = thread(&one, &p).unwrap();
    assert_eq!((report.passes, report.dropped), (3, 0));
    let radii: Vec<f64> = ends(&path)
        .into_iter()
        .filter(|(_, m)| m.role == MoveRole::Cut)
        .filter_map(|(from, m)| match m.kind {
            MoveKind::Arc(a) => Some(a.radius(from)),
            MoveKind::Linear => None,
        })
        .fold(Vec::new(), |mut out, r| {
            if out.last().is_none_or(|l: &f64| (l - r).abs() > 1e-9) {
                out.push(r);
            }
            out
        });
    assert_eq!(radii.len(), 3, "{radii:?}");
    for (r, want) in radii.iter().zip([1.93, 2.93, 3.93]) {
        assert!((r - want).abs() < 1e-9, "{radii:?}");
    }
    let links = path
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Link)
        .count();
    assert_eq!(links, 3);
    // Only the last pass's reach is what is meant.
    assert!((report.cuts[0].reach - 8.5).abs() < 1e-9);
    let past = ThreadParams { stepovers: 5, ..p };
    let (_, report) = thread(&one, &past).unwrap();
    assert_eq!((report.passes, report.dropped), (4, 1));
}

/// A boss, a hole no wider than the tool, an offset that leaves the tool no
/// room, a pitch of nothing, and a hole with no depth are refused, each
/// saying where.
#[test]
fn refuses_what_it_cannot_thread() {
    let [through, _] = holes();
    let boss = Round {
        boss: true,
        ..through
    };
    assert_eq!(
        thread(&[boss], &fusions(0.0)).unwrap_err(),
        ThreadError::Boss { x: -55.0, y: 35.0 }
    );
    let small = Round {
        radius: 4.5,
        ..through
    };
    assert!(matches!(
        thread(&[small], &fusions(0.0)),
        Err(ThreadError::TooSmall { .. })
    ));
    assert!(matches!(
        thread(&[through], &fusions(-7.0)),
        Err(ThreadError::NoRoom { .. })
    ));
    let flat = Round {
        top: 0.0,
        ..through
    };
    assert!(matches!(
        thread(&[flat], &fusions(0.0)),
        Err(ThreadError::NoDepth { .. })
    ));
    for pitch in [0.0, -1.0, f64::NAN] {
        let p = ThreadParams {
            pitch,
            ..fusions(0.0)
        };
        assert!(matches!(
            thread(&[through], &p),
            Err(ThreadError::Strategy(StrategyError::BadParameter {
                name: "pitch",
                ..
            }))
        ));
    }
    let steep = ThreadParams {
        pitch: 300.0,
        ..fusions(0.0)
    };
    assert!(matches!(
        thread(&[through], &steep),
        Err(ThreadError::TooSteep { .. })
    ));
    let message = ThreadError::Boss { x: 1.0, y: 2.0 }.to_string();
    assert!(message.contains("(1.000, 2.000)"), "{message}");
}

/// Worked out twice, the same path, to the bit.
#[test]
fn deterministic() {
    let p = ThreadParams {
        stepovers: 2,
        ..fusions(0.4)
    };
    let a = thread(&holes(), &p).unwrap();
    let b = thread(&holes(), &p).unwrap();
    assert_eq!(a, b);
}
