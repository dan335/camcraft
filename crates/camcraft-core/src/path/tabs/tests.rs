use super::*;

/// A square ring `side` across, cut at each of `levels`, as a profile emits
/// it: a link to its corner, then round.
fn square(side: f64, levels: &[f64]) -> Toolpath {
    let mut p = Toolpath::new();
    let corners = [
        dvec3(0.0, 0.0, 0.0),
        dvec3(side, 0.0, 0.0),
        dvec3(side, side, 0.0),
        dvec3(0.0, side, 0.0),
        dvec3(0.0, 0.0, 0.0),
    ];
    for &z in levels {
        let at = |c: DVec3| dvec3(c.x, c.y, z);
        p.feed_to(at(corners[0]), MoveRole::Link, 1000.0).unwrap();
        for c in &corners[1..] {
            p.feed_to(at(*c), MoveRole::Cut, 1000.0).unwrap();
        }
    }
    p
}

fn params(shape: TabShape, spacing: f64) -> TabParams {
    TabParams {
        tabs: Tabs {
            shape,
            width: 4.0,
            height: 2.0,
            spacing,
        },
        tool_radius: 1.0,
        plunge_feed: 300.0,
    }
}

/// The highest the tool is anywhere within `r` of `c`, seen from above,
/// at or below `z`.
fn highest_near(path: &Toolpath, c: DVec2, r: f64) -> f64 {
    path.moves()
        .iter()
        .filter(|m| m.to.truncate().distance(c) <= r)
        .map(|m| m.to.z)
        .fold(f64::MIN, f64::max)
}

/// A round ring, one full turn about the origin at `z`, as a single arc.
fn circle(radius: f64, z: f64) -> Toolpath {
    let mut p = Toolpath::new();
    let start = dvec3(radius, 0.0, z);
    p.feed_to(start, MoveRole::Link, 1000.0).unwrap();
    let turn = Arc {
        centre: dvec3(0.0, 0.0, z),
        axis: DVec3::Z,
        sweep: std::f64::consts::TAU,
    };
    p.arc_to(start, turn, MoveRole::Cut, 1000.0).unwrap();
    p
}

/// An arc is lifted over a tab and stays an arc: the ring comes back as arcs
/// on the same circle, on the floor and over each tab, joined by steps
/// straight up and down, and goes all the way round.
#[test]
fn an_arc_is_lifted_over_a_tab_as_arcs() {
    let (radius, z) = (20.0, -2.0);
    let (out, report) = add_tabs(&circle(radius, z), &params(TabShape::Rectangular, 60.0)).unwrap();
    assert_eq!(report.tabs, 3, "{report:?}");
    let mut at = out.moves()[0].to;
    let mut turned = 0.0;
    let (mut floor, mut top) = (0, 0);
    for m in &out.moves()[1..] {
        match m.kind {
            MoveKind::Arc(a) => {
                assert!((a.radius(at) - radius).abs() < 1e-9, "{m:?}");
                assert!(a.centre.truncate().length() < 1e-9);
                assert!((m.to.z - at.z).abs() < 1e-9, "level over a rectangular tab");
                turned += a.sweep;
                if (m.to.z - z).abs() < 1e-9 {
                    floor += 1;
                } else {
                    assert!((m.to.z - (z + 2.0)).abs() < 1e-9, "{m:?}");
                    top += 1;
                }
            }
            MoveKind::Linear => {
                assert!(
                    m.to.truncate().distance(at.truncate()) < 1e-9,
                    "a step: {m:?}"
                );
            }
        }
        at = m.to;
    }
    assert!((turned - std::f64::consts::TAU).abs() < 1e-9, "{turned}");
    // Over each tab, in two halves either side of its middle.
    assert_eq!(top, 2 * report.tabs, "{report:?}");
    assert_eq!(floor, report.tabs + 1, "between them, and on to the end");
    assert!(at.distance(dvec3(radius, 0.0, z)) < 1e-9);
}

/// A lead runs on from the ring towards the first tab, at the ring's depth:
/// it rides over the tab as the cut does, rather than cutting into it.
#[test]
fn a_lead_rides_over_a_tab_as_the_cut_does() {
    // A ring started halfway along a side, as a profile starts one, and a
    // lead-out carrying on along that side into the waste.
    let z = -2.0;
    let mut p = Toolpath::new();
    p.feed_to(dvec3(10.0, 0.0, z), MoveRole::Link, 1000.0)
        .unwrap();
    for c in [
        (20.0, 0.0),
        (20.0, 20.0),
        (0.0, 20.0),
        (0.0, 0.0),
        (10.0, 0.0),
    ] {
        p.feed_to(dvec3(c.0, c.1, z), MoveRole::Cut, 1000.0)
            .unwrap();
    }
    for c in [(11.5, -0.1), (12.5, -0.4), (13.5, -1.0), (14.2, -1.8)] {
        p.feed_to(dvec3(c.0, c.1, z), MoveRole::Lead, 1000.0)
            .unwrap();
    }
    let (out, report) = add_tabs(&p, &params(TabShape::Rectangular, 10.0)).unwrap();
    assert!(report.tabs > 0);
    // The first tab, 80 mm round in eight: its centre 5 mm on from the start.
    let first = dvec2(15.0, 0.0);
    for m in out.moves().iter().filter(|m| m.role == MoveRole::Lead) {
        if m.to.truncate().distance(first) < 3.0 - 1e-6 {
            assert!(
                m.to.z >= z + 2.0 - 1e-9,
                "a lead cuts into the tab at {}",
                m.to
            );
        }
    }
}

/// Tabs go evenly round the deepest loop, clear of its start, and lift only
/// the cuts below their top: the level above them is left alone.
#[test]
fn a_ring_gets_tabs_on_its_deepest_level() {
    let path = square(100.0, &[-2.0, -4.0, -6.0]);
    let (out, report) = add_tabs(&path, &params(TabShape::Rectangular, 150.0)).unwrap();
    // 400 mm round, 150 apart: three, at 200/3 intervals from 200/3 / 2.
    assert_eq!(
        report,
        TabReport {
            loops: 1,
            tabs: 3,
            too_short: 0
        }
    );
    let first = dvec2(400.0 / 6.0, 0.0);
    // The tool rides over the tab at its top, 2 mm above the floor.
    assert_eq!(highest_near(&out, first, 3.0), -4.0);
    let deepest: Vec<&Move> = out.moves().iter().filter(|m| m.to.z == -4.0).collect();
    assert!(!deepest.is_empty());
    // Up onto it and down off it: straight down, at the plunge feed.
    let plunges: Vec<&Move> = out
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Plunge)
        .collect();
    assert_eq!(plunges.len(), 3);
    assert!(
        plunges
            .iter()
            .all(|m| m.to.z == -6.0 && m.feed == Some(300.0))
    );
    // Over a tab, 3 mm each side of its centre: the tab's width and the tool.
    let on: Vec<f64> = out
        .moves()
        .iter()
        .filter(|m| m.to.z == -4.0 && m.to.y == 0.0)
        .map(|m| m.to.x)
        .collect();
    assert!(
        on.iter().any(|x| (x - (first.x - 3.0)).abs() < 1e-6),
        "{on:?}"
    );
    assert!(
        on.iter().any(|x| (x - (first.x + 3.0)).abs() < 1e-6),
        "{on:?}"
    );
    // The levels above the tab's top are cut as they were.
    let above = |p: &Toolpath| {
        p.moves()
            .iter()
            .filter(|m| m.to.z > -4.0 + 1e-9)
            .map(|m| m.to)
            .collect::<Vec<_>>()
    };
    assert_eq!(above(&out), above(&path));
    // No pass starts on a tab.
    for m in out.moves().iter().filter(|m| m.role == MoveRole::Link) {
        assert_eq!(m.to.truncate(), DVec2::ZERO);
    }
}

/// A triangular tab is climbed and left at 45°, nothing steep.
#[test]
fn a_triangular_tab_is_ramped_over() {
    let path = square(100.0, &[-6.0]);
    let (out, report) = add_tabs(&path, &params(TabShape::Triangular, 1000.0)).unwrap();
    assert_eq!(report.tabs, 2);
    assert!(out.moves().iter().all(|m| m.role != MoveRole::Plunge));
    assert_eq!(out.steep_cut_descents(DVec3::ZERO, PLUNGE_RATIO).0, 0);
    let first = dvec2(100.0, 0.0);
    // Two tabs round 400 mm: at 100 and 300 along, the second corner and the
    // opposite one. Its top is 2 mm up.
    assert!((highest_near(&out, first, 0.5) - -4.0).abs() < 1e-6);
    // Its flank reaches 2 mm past the flat top, 3 + 2 from the middle.
    assert!((highest_near(&out, dvec2(95.5, 0.0), 0.01) - -6.0).abs() > 1e-6);
    assert!((highest_near(&out, dvec2(94.0, 0.0), 1.0) - -6.0).abs() < 1e-6);
}

/// A loop too small for a tab clear of its start gets none, and a path with
/// no closed loop at its deepest is left as it was.
#[test]
fn too_small_or_open_is_left_alone() {
    let small = square(1.0, &[-1.0]);
    let (out, report) = add_tabs(&small, &params(TabShape::Rectangular, 10.0)).unwrap();
    assert_eq!(report.too_short, 1);
    assert_eq!(out, small);

    let mut open = Toolpath::new();
    open.feed_to(dvec3(0.0, 0.0, -1.0), MoveRole::Link, 1000.0)
        .unwrap();
    open.feed_to(dvec3(50.0, 0.0, -1.0), MoveRole::Cut, 1000.0)
        .unwrap();
    let (out, report) = add_tabs(&open, &params(TabShape::Rectangular, 10.0)).unwrap();
    assert_eq!(report, TabReport::default());
    assert_eq!(out, open);
}

/// A tab too far from a move to reach it changes nothing about it, even a
/// triangular one, whose flank is some height everywhere.
#[test]
fn a_tab_out_of_reach_changes_nothing() {
    let moves = square(20.0, &[-2.0, -4.0]);
    for shape in [TabShape::Rectangular, TabShape::Triangular] {
        let near = TabField {
            centres: vec![dvec2(10.0, 0.0), dvec2(20.0, 10.0)],
            top: -2.5,
            shape,
            half: 3.0,
        };
        let with_far = TabField {
            centres: vec![
                dvec2(-40.0, 10.0),
                dvec2(10.0, 0.0),
                dvec2(70.0, 70.0),
                dvec2(20.0, 10.0),
                dvec2(10.0, -9.0),
            ],
            ..near.clone()
        };
        let lift = |field: &TabField| {
            let mut out = Toolpath::new();
            let mut at = DVec3::ZERO;
            for m in moves.moves() {
                field.over(&mut out, at, m, 300.0).unwrap();
                at = m.to;
            }
            out
        };
        let lifted = lift(&near);
        assert!(lifted.len() > moves.len(), "{shape:?} lifted nothing");
        assert_eq!(lift(&with_far), lifted, "{shape:?}");
    }
}

use glam::dvec2;
