use super::*;
use crate::path::toolpath::{Move, MoveKind};
use glam::dvec2;

fn hole(radius: f64, top: f64, bottom: f64) -> Round {
    Round {
        at: dvec2(10.0, -5.0),
        radius,
        top,
        bottom,
        boss: false,
    }
}

fn params(radius: f64) -> RoundParams {
    RoundParams {
        radius,
        leave: 0.0,
        stepovers: 1,
        stepover: 0.95 * 2.0 * radius,
        finishing: None,
        repeat: false,
        feed: 1000.0,
        kept_on_right: true,
        leads: None,
        to_centre: false,
    }
}

/// Every move with where it starts.
fn steps(path: &Toolpath) -> Vec<(DVec3, Move)> {
    path.steps()
        .filter_map(|(from, m)| Some((from?, *m)))
        .collect()
}

/// The cuts' arcs: where each starts, the arc, and where it ends.
fn cut_arcs(path: &Toolpath) -> Vec<(DVec3, Arc, DVec3)> {
    steps(path)
        .into_iter()
        .filter(|(_, m)| m.role == MoveRole::Cut)
        .filter_map(|(from, m)| match m.kind {
            MoveKind::Arc(a) => Some((from, a, m.to)),
            MoveKind::Linear => None,
        })
        .collect()
}

/// The radii of the circles cut round, each once, in the order cut.
fn radii(path: &Toolpath, centre: DVec2) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    for (from, a, _) in cut_arcs(path) {
        assert!(a.centre.truncate().distance(centre) < 1e-9);
        let r = a.radius(from);
        if out.last().is_none_or(|l| (l - r).abs() > 1e-9) {
            out.push(r);
        }
    }
    out
}

/// A bore goes round the circle its radius in from the wall, down a helix at
/// the pitch from the top to the bottom, then once round level there; every
/// cut is an arc about the hole's axis.
#[test]
fn a_bore_goes_down_a_helix_at_the_pitch() {
    let h = hole(6.0, 20.0, 12.0);
    let (path, report) = bore(&[h], Down::Pitch(1.5), &params(2.0)).unwrap();
    // One pass, at the wall, leaving the middle standing.
    assert_eq!((report.holes, report.passes, report.cores), (1, 1, 1));
    assert!((report.widest_core - 4.0).abs() < 1e-9);
    assert_eq!(radii(&path, h.at), vec![4.0]);
    let arcs = cut_arcs(&path);
    let (first, ..) = arcs[0];
    assert!((first.z - 20.0).abs() < 1e-12, "starts at the top: {first}");
    // Down the helix: as far down per turn as the pitch.
    let helix: Vec<_> = arcs.iter().filter(|(f, _, t)| t.z < f.z).collect();
    let turned: f64 = helix.iter().map(|(_, a, _)| a.sweep.abs()).sum();
    let fell: f64 = helix.iter().map(|(f, _, t)| f.z - t.z).sum();
    assert!((fell - 8.0).abs() < 1e-9);
    assert!((fell / (turned / TAU) - 1.5).abs() < 1e-9);
    // No arc turns more than half a turn, so none ends where it starts.
    assert!(arcs.iter().all(|(f, a, t)| {
        a.sweep.abs() <= PI + 1e-12 && f.truncate().distance(t.truncate()) > 1e-6
    }));
    // Then once round at the bottom.
    let level: Vec<_> = arcs
        .iter()
        .filter(|(f, _, t)| (f.z - 12.0).abs() < 1e-12 && (t.z - 12.0).abs() < 1e-12)
        .collect();
    let round: f64 = level.iter().map(|(_, a, _)| a.sweep).sum();
    assert!((round - TAU).abs() < 1e-12, "{round}");
    assert!(level[0].0.distance(level.last().unwrap().2) < 1e-9);
    // Nothing lower than the bottom.
    let lowest = path.moves().iter().map(|m| m.to.z).fold(f64::MAX, f64::min);
    assert!((lowest - 12.0).abs() < 1e-12);
}

/// By angle, each circle's helix falls as far a turn as the angle takes it
/// round that circle.
#[test]
fn a_bore_by_angle_falls_with_its_circle() {
    let h = hole(6.0, 20.0, 12.0);
    let angle = 3f64.to_radians();
    let (path, _) = bore(&[h], Down::Angle(angle), &params(2.0)).unwrap();
    let arcs = cut_arcs(&path);
    let (from, a, to) = arcs[0];
    let flat = a.radius(from) * a.sweep.abs();
    assert!(((from.z - to.z) / flat - angle.tan()).abs() < 1e-9);
}

/// Climbing with a clockwise spindle goes anticlockwise round a hole and
/// clockwise round a boss; conventional, the other way.
#[test]
fn climbing_goes_round_each_side_its_own_way() {
    let h = hole(6.0, 20.0, 12.0);
    let boss = Round { boss: true, ..h };
    let sweep = |r: Round, kept_on_right: bool| {
        let p = RoundParams {
            kept_on_right,
            ..params(2.0)
        };
        let (path, _) = bore(&[r], Down::Pitch(1.0), &p).unwrap();
        cut_arcs(&path)[0].1.sweep
    };
    assert!(sweep(h, true) > 0.0);
    assert!(sweep(h, false) < 0.0);
    assert!(sweep(boss, true) < 0.0);
    assert!(sweep(boss, false) > 0.0);
    // Round a boss, the tool's centre is outside its wall.
    let (path, report) = bore(&[boss], Down::Pitch(1.0), &params(2.0)).unwrap();
    assert_eq!((report.holes, report.bosses), (0, 1));
    assert_eq!(radii(&path, h.at), vec![8.0]);
}

/// A hole no wider than the tool is refused, saying so, rather than cut as a
/// plunge.
#[test]
fn a_hole_too_small_for_the_tool_is_refused() {
    for tool in [3.0, 3.5] {
        let e = bore(&[hole(3.0, 20.0, 8.0)], Down::Pitch(1.0), &params(tool)).unwrap_err();
        assert!(matches!(e, RoundError::TooSmall { .. }), "{e:?}");
        let said = e.to_string();
        assert!(said.contains("6.000 mm across"), "{said}");
        assert!(said.contains("drill it"), "{said}");
        let levels = Levels {
            stepdown: None,
            even: false,
        };
        let e = circular(&[hole(3.0, 20.0, 8.0)], levels, &params(tool)).unwrap_err();
        assert!(matches!(e, RoundError::TooSmall { .. }), "{e:?}");
    }
    // Stock to leave beside counts against it.
    let p = RoundParams {
        leave: 1.0,
        ..params(2.0)
    };
    assert!(bore(&[hole(3.0, 20.0, 8.0)], Down::Pitch(1.0), &p).is_err());
    // A pitch too coarse for a small circle is a plunge, and refused.
    let e = bore(&[hole(3.0, 20.0, 8.0)], Down::Pitch(50.0), &params(2.9)).unwrap_err();
    assert!(matches!(e, RoundError::TooSteep { .. }), "{e:?}");
}

/// Passes out to the wall a stepover apart, from the middle out, the last at
/// the wall; one past the middle is put half the tool's radius from it, so the
/// middle is cut. Too few leave a core, and say how wide.
#[test]
fn stepovers_go_out_to_the_wall() {
    let h = hole(12.0, 20.0, 12.0);
    let p = RoundParams {
        stepovers: 3,
        stepover: 5.7,
        ..params(3.0)
    };
    let (path, report) = bore(&[h], Down::Pitch(1.0), &p).unwrap();
    let r = radii(&path, h.at);
    assert_eq!(r.len(), 3, "{r:?}");
    assert!((r[0] - 1.5).abs() < 1e-9 && (r[1] - 3.3).abs() < 1e-9 && r[2] == 9.0);
    assert_eq!(report.cores, 0);
    assert_eq!(report.passes, 3);

    let (_, report) = bore(&[h], Down::Pitch(1.0), &params(3.0)).unwrap();
    assert_eq!(report.cores, 1);
    assert!((report.widest_core - 12.0).abs() < 1e-9);

    // Round a boss, from outside in.
    let boss = Round { boss: true, ..h };
    let p = RoundParams {
        stepovers: 2,
        stepover: 4.0,
        ..params(3.0)
    };
    let (path, _) = bore(&[boss], Down::Pitch(1.0), &p).unwrap();
    assert_eq!(radii(&path, h.at), vec![19.0, 15.0]);
}

/// A bore's finishing passes go on out from where the stepovers stop, the
/// last at the wall at the finishing feed; a repeat cuts the last twice.
#[test]
fn finishing_passes_end_at_the_wall() {
    let h = hole(10.0, 20.0, 12.0);
    let p = RoundParams {
        stepovers: 2,
        stepover: 3.0,
        finishing: Some(WallFinish {
            passes: 2,
            stepover: 0.5,
            feed: 400.0,
            repeat: false,
        }),
        repeat: true,
        ..params(3.0)
    };
    let (path, report) = bore(&[h], Down::Pitch(1.0), &p).unwrap();
    // 7 - 1 = 6 and 3 roughing, then 6.5 and 7, the last twice.
    let r = radii(&path, h.at);
    assert_eq!(r, vec![3.0, 6.0, 6.5, 7.0], "{r:?}");
    assert_eq!(report.passes, 5);
    let at_wall: Vec<f64> = cut_arcs(&path)
        .iter()
        .filter(|(f, a, _)| (a.radius(*f) - 7.0).abs() < 1e-9)
        .map(|_| 0.0)
        .collect();
    assert!(at_wall.len() > 2);
    let feeds: Vec<f64> = steps(&path)
        .iter()
        .filter(|(f, m)| {
            m.role == MoveRole::Cut
                && matches!(m.kind, MoveKind::Arc(a) if (a.radius(*f) - 7.0).abs() < 1e-9)
        })
        .filter_map(|(_, m)| m.feed)
        .collect();
    assert!(feeds.iter().all(|&f| f == 400.0), "{feeds:?}");
}

/// A circular cuts each level all the way round, a whole stepdown each and the
/// last what is left, or evenly; each level's passes from the middle out.
#[test]
fn a_circular_cuts_level_by_level() {
    let h = hole(8.0, 20.0, 12.0);
    let z_of =
        |even: bool| {
            let levels = Levels {
                stepdown: Some(3.0),
                even,
            };
            let (path, report) = circular(&[h], levels, &params(2.0)).unwrap();
            assert_eq!(report.levels, 3);
            // Each ring two half turns, level.
            let arcs = cut_arcs(&path);
            assert!(arcs.iter().all(|(f, a, t)| {
                (f.z - t.z).abs() < 1e-12 && (a.sweep.abs() - PI).abs() < 1e-12
            }));
            arcs.chunks(2).map(|r| r[0].0.z).collect::<Vec<f64>>()
        };
    assert_eq!(z_of(false), vec![17.0, 14.0, 12.0]);
    let even = z_of(true);
    assert!((even[0] - (20.0 - 8.0 / 3.0)).abs() < 1e-9, "{even:?}");
    assert_eq!(even[2], 12.0);

    let p = RoundParams {
        stepovers: 2,
        stepover: 3.0,
        repeat: true,
        ..params(2.0)
    };
    let levels = Levels {
        stepdown: Some(4.0),
        even: false,
    };
    let (path, report) = circular(&[h], levels, &p).unwrap();
    let rings: Vec<(f64, f64)> = cut_arcs(&path)
        .chunks(2)
        .map(|r| (r[0].0.z, r[0].1.radius(r[0].0)))
        .collect();
    // The wall's pass is cut twice at the last level only.
    assert_eq!(
        rings,
        vec![
            (16.0, 3.0),
            (16.0, 6.0),
            (12.0, 3.0),
            (12.0, 6.0),
            (12.0, 6.0)
        ]
    );
    assert_eq!(report.passes, 5);
}

/// The leads onto each circle and off it stay inside the hole, as the tool's
/// centre goes, and are tangent to the circle; to the centre, they start and
/// end there.
#[test]
fn leads_stay_in_the_hole() {
    let h = hole(5.0, 20.0, 12.0);
    let leads = LeadArc {
        radius: 0.6,
        sweep: 0.5 * PI,
        straight: 0.6,
    };
    for to_centre in [false, true] {
        let p = RoundParams {
            leads: Some(leads),
            to_centre,
            ..params(3.0)
        };
        for path in [
            bore(&[h], Down::Pitch(1.0), &p).unwrap().0,
            circular(
                &[h],
                Levels {
                    stepdown: None,
                    even: false,
                },
                &p,
            )
            .unwrap()
            .0,
        ] {
            let lead: Vec<(DVec3, Move)> = steps(&path)
                .into_iter()
                .filter(|(_, m)| m.role == MoveRole::Lead)
                .collect();
            assert!(!lead.is_empty());
            for (from, m) in &lead {
                for q in m.chords(*from, 1e-3) {
                    assert!(q.truncate().distance(h.at) <= 2.0 + 1e-9, "{q}");
                }
            }
            let first = path.moves()[0].to.truncate();
            if to_centre {
                assert!(first.distance(h.at) < 1e-9, "{first}");
                let end = path.moves().last().unwrap().to.truncate();
                assert!(end.distance(h.at) < 1e-9, "{end}");
            }
        }
    }
    // No room even for a quarter of it: without, and said.
    let tight = hole(3.05, 20.0, 12.0);
    let p = RoundParams {
        leads: Some(LeadArc {
            radius: 4.0,
            sweep: 0.5 * PI,
            straight: 4.0,
        }),
        ..params(3.0)
    };
    let (_, report) = bore(&[tight], Down::Pitch(0.1), &p).unwrap();
    assert_eq!(report.no_room_to_lead, 1);
}

/// Points round a vertical cylinder of `radius` about `at` from `z0` to `z1`,
/// as triangles facing out of it, or into it for a hole's wall.
fn cylinder(at: DVec2, radius: f64, (z0, z1): (f64, f64), hole: bool, half: bool) -> Wall {
    let n = 24;
    let span = if half { PI } else { TAU };
    let p = |k: u32, z: f64| {
        let a = span * f64::from(k) / f64::from(n);
        (at + DVec2::from_angle(a) * radius).extend(z)
    };
    let mut triangles = Vec::new();
    for k in 0..n {
        let (a, b, c, d) = (p(k, z0), p(k + 1, z0), p(k + 1, z1), p(k, z1));
        // Anticlockwise seen from outside faces out.
        if hole {
            triangles.push([a, c, b]);
            triangles.push([a, d, c]);
        } else {
            triangles.push([a, b, c]);
            triangles.push([a, c, d]);
        }
    }
    Wall {
        model: "part".into(),
        face: 7,
        circles: vec![(at.extend(z0), radius, true), (at.extend(z1), radius, true)],
        triangles,
    }
}

/// A face whose edges are circles about one upright axis is a round wall: a
/// hole's where it faces its axis, a boss's where it faces out. Two halves of
/// one are one, from the highest to the lowest; anything else is refused by
/// name.
#[test]
fn round_walls_are_read_from_their_faces() {
    let at = dvec2(3.0, 4.0);
    let found = rounds(&[cylinder(at, 5.0, (12.0, 20.0), true, false)], 0.01).unwrap();
    assert_eq!(
        found,
        vec![Round {
            at,
            radius: 5.0,
            top: 20.0,
            bottom: 12.0,
            boss: false
        }]
    );
    let boss = rounds(&[cylinder(at, 5.0, (0.0, 6.0), false, false)], 0.01).unwrap();
    assert!(boss[0].boss);

    // Two halves, one above the other, are one wall.
    let upper = cylinder(at, 5.0, (16.0, 20.0), true, true);
    let lower = Wall {
        face: 8,
        ..cylinder(at, 5.0, (10.0, 16.0), true, true)
    };
    let one = rounds(&[upper, lower], 0.01).unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!((one[0].top, one[0].bottom), (20.0, 10.0));

    let mut tapered = cylinder(at, 5.0, (12.0, 20.0), true, false);
    tapered.circles[1].1 = 4.0;
    assert!(matches!(
        rounds(&[tapered], 0.01),
        Err(RoundError::Tapered { face: 7, .. })
    ));
    let mut flat = cylinder(at, 5.0, (12.0, 20.0), true, false);
    flat.circles.clear();
    let e = rounds(&[flat], 0.01).unwrap_err();
    assert_eq!(e.to_string(), "face 7 of 'part' is not a round wall");
    let mut lying = cylinder(at, 5.0, (12.0, 20.0), true, false);
    lying.circles[0].2 = false;
    assert!(matches!(
        rounds(&[lying], 0.01),
        Err(RoundError::NotUpright { .. })
    ));
    assert_eq!(rounds(&[], 0.01), Err(RoundError::NothingPicked));
}

/// Walls are cut from the first picked, then always the nearest not yet cut.
#[test]
fn walls_are_cut_nearest_first() {
    let at = |x: f64| Round {
        at: dvec2(x, 0.0),
        ..hole(3.0, 1.0, 0.0)
    };
    let order: Vec<f64> = nearest_first(&[at(0.0), at(10.0), at(1.0), at(-2.0)])
        .iter()
        .map(|r| r.at.x)
        .collect();
    assert_eq!(order, vec![0.0, 1.0, -2.0, 10.0]);
}

/// The same walls give the same path, move for move.
#[test]
fn the_path_is_deterministic() {
    let h = hole(9.0, 20.0, 11.0);
    let p = RoundParams {
        stepovers: 3,
        stepover: 2.5,
        leads: Some(LeadArc {
            radius: 0.6,
            sweep: 0.5 * PI,
            straight: 0.6,
        }),
        ..params(3.0)
    };
    let a = bore(&[h], Down::Pitch(0.7), &p).unwrap();
    let b = bore(&[h], Down::Pitch(0.7), &p).unwrap();
    assert_eq!(a, b);
}
