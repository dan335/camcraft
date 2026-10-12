use super::*;
use glam::dvec3;

#[test]
fn rejects_non_finite_targets() {
    let mut t = Toolpath::new();
    assert!(matches!(
        t.rapid_to(dvec3(f64::NAN, 0.0, 0.0)),
        Err(ToolpathError::NonFiniteTarget(_))
    ));
    assert!(t.is_empty(), "a rejected move must not be stored");
}

/// A feeding move without a feed would reach the post and become a rapid into
/// the work. The type refuses to hold one.
#[test]
fn feeding_moves_must_carry_a_feed() {
    let mut t = Toolpath::new();
    let bad = Move {
        to: dvec3(1.0, 0.0, 0.0),
        kind: MoveKind::Linear,
        role: MoveRole::Cut,
        feed: None,
    };
    assert_eq!(
        t.push(bad),
        Err(ToolpathError::FeedMismatch {
            role: MoveRole::Cut,
            expected: true
        })
    );
    assert!(matches!(
        t.feed_to(dvec3(1.0, 0.0, 0.0), MoveRole::Cut, 0.0),
        Err(ToolpathError::BadFeed(_))
    ));
    assert!(
        t.feed_to(dvec3(1.0, 0.0, 0.0), MoveRole::Cut, 500.0)
            .is_ok()
    );
}

#[test]
fn rapids_must_not_carry_a_feed() {
    let mut t = Toolpath::new();
    let bad = Move {
        to: dvec3(1.0, 0.0, 0.0),
        kind: MoveKind::Linear,
        role: MoveRole::Rapid,
        feed: Some(500.0),
    };
    assert_eq!(
        t.push(bad),
        Err(ToolpathError::FeedMismatch {
            role: MoveRole::Rapid,
            expected: false
        })
    );
}

#[test]
fn only_a_link_is_unplanned() {
    assert!(MoveRole::Link.is_unplanned());
    assert!(!MoveRole::Rapid.is_unplanned());
}

#[test]
fn unplanned_links_are_detectable() {
    let mut t = Toolpath::new();
    t.rapid_to(dvec3(0.0, 0.0, 5.0)).unwrap();
    assert!(!t.has_unplanned_links());
    t.feed_to(dvec3(1.0, 0.0, 0.0), MoveRole::Link, 100.0)
        .unwrap();
    assert!(
        t.has_unplanned_links(),
        "a finished program must be able to prove it has none"
    );
}

#[test]
fn measures_only_cutting_moves() {
    let mut t = Toolpath::new();
    t.rapid_to(dvec3(0.0, 0.0, 0.0)).unwrap();
    t.feed_to(dvec3(3.0, 0.0, 0.0), MoveRole::Cut, 500.0)
        .unwrap();
    t.rapid_to(dvec3(3.0, 10.0, 0.0)).unwrap();
    t.feed_to(dvec3(3.0, 14.0, 0.0), MoveRole::Cut, 500.0)
        .unwrap();
    assert_eq!(t.cutting_length(dvec3(0.0, 0.0, 0.0)), 7.0);
    assert_eq!(t.end(), Some(dvec3(3.0, 14.0, 0.0)));
    assert_eq!(t.len(), 4);
}

/// The check that a geometrically perfect path can still be unmachinable.
#[test]
fn detects_cut_moves_that_are_really_plunges() {
    let mut t = Toolpath::new();
    // A gentle pass, then a wall: correct points, unmachinable connection.
    t.feed_to(dvec3(0.0, 0.0, 10.0), MoveRole::Cut, 500.0)
        .unwrap();
    t.feed_to(dvec3(5.0, 0.0, 10.0), MoveRole::Cut, 500.0)
        .unwrap();
    t.feed_to(dvec3(5.01, 0.0, -7.0), MoveRole::Cut, 500.0)
        .unwrap();

    let (count, deepest) = t.steep_cut_descents(dvec3(0.0, 0.0, 10.0), 1.0);
    assert_eq!(count, 1, "the wall crossing must be caught");
    assert!((deepest - 17.0).abs() < 1e-9, "deepest was {deepest}");

    // Climbing a wall is not the same hazard as diving down one.
    let mut up = Toolpath::new();
    up.feed_to(dvec3(0.0, 0.0, 10.0), MoveRole::Cut, 500.0)
        .unwrap();
    up.feed_to(dvec3(0.01, 0.0, 27.0), MoveRole::Cut, 500.0)
        .unwrap();
    assert_eq!(up.steep_cut_descents(dvec3(0.0, 0.0, 10.0), 1.0).0, 0);
}

/// A plunge split into short moves is the same plunge: one descent, measured
/// in full, so no single move's smallness lets it through.
#[test]
fn a_split_plunge_is_one_descent() {
    let mut t = Toolpath::new();
    t.feed_to(dvec3(5.0, 0.0, 10.0), MoveRole::Cut, 500.0)
        .unwrap();
    for k in 1..=6 {
        let k = f64::from(k);
        t.feed_to(
            dvec3(5.0 + k * 0.01, 0.0, 10.0 - k * 0.4),
            MoveRole::Cut,
            500.0,
        )
        .unwrap();
    }
    // Along, then down again: a second, separate descent.
    t.feed_to(dvec3(8.0, 0.0, 7.6), MoveRole::Cut, 500.0)
        .unwrap();
    t.feed_to(dvec3(8.01, 0.0, 7.3), MoveRole::Cut, 500.0)
        .unwrap();

    let all = t.steep_descents(dvec3(5.0, 0.0, 10.0), 10.0);
    assert_eq!(all.len(), 2, "{all:?}");
    assert!((all[0].depth - 2.4).abs() < 1e-9, "{all:?}");
    assert_eq!((all[0].first, all[0].last), (1, 6));
    assert!((all[1].depth - 0.3).abs() < 1e-9, "{all:?}");
    assert_eq!(t.steep_cut_descents(dvec3(5.0, 0.0, 10.0), 10.0).0, 2);
}

#[test]
fn a_shallow_pass_has_no_steep_descents() {
    let mut t = Toolpath::new();
    for i in 1..20 {
        let x = f64::from(i);
        t.feed_to(dvec3(x, 0.0, 10.0 - x * 0.1), MoveRole::Cut, 500.0)
            .unwrap();
    }
    assert_eq!(t.steep_cut_descents(dvec3(0.0, 0.0, 10.0), 1.0), (0, 0.0));
}

/// An arc starts where the path is and must end where its move does; it is
/// never a rapid, plunge or retract, and never a path's first move.
#[test]
fn arcs_are_checked_against_where_the_path_is() {
    let quarter = Arc {
        centre: dvec3(1.0, 0.0, 0.0),
        axis: DVec3::Z,
        sweep: -std::f64::consts::FRAC_PI_2,
    };
    let mut t = Toolpath::new();
    assert_eq!(
        t.arc_to(dvec3(1.0, 1.0, 0.0), quarter, MoveRole::Cut, 400.0),
        Err(ToolpathError::ArcWithoutStart)
    );
    t.feed_to(DVec3::ZERO, MoveRole::Cut, 400.0).unwrap();
    // Clockwise from the origin about (1, 0) is up to (1, 1).
    t.arc_to(dvec3(1.0, 1.0, 0.0), quarter, MoveRole::Cut, 400.0)
        .unwrap();
    let m = t.moves()[1];
    let length = m.length(DVec3::ZERO);
    assert!((length - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert_eq!(t.cutting_length(DVec3::ZERO), length);
    // It does not end there.
    assert_eq!(
        t.arc_to(dvec3(3.0, 0.0, 0.0), quarter, MoveRole::Cut, 400.0),
        Err(ToolpathError::BadArc(dvec3(3.0, 0.0, 0.0)))
    );
    let broken = Arc {
        centre: dvec3(f64::INFINITY, 0.0, 0.0),
        ..quarter
    };
    assert!(
        t.arc_to(dvec3(2.0, 0.0, 0.0), broken, MoveRole::Cut, 400.0)
            .is_err()
    );
    let plunge = Move {
        to: dvec3(2.0, 0.0, 0.0),
        kind: MoveKind::Arc(quarter),
        role: MoveRole::Plunge,
        feed: Some(100.0),
    };
    assert_eq!(
        t.push(plunge),
        Err(ToolpathError::CurvedMove(MoveRole::Plunge))
    );
}

/// Split at each link, each pass starting at its own, and what comes before
/// the first link a pass of its own.
#[test]
fn a_path_splits_into_passes_at_its_links() {
    let mut t = Toolpath::new();
    assert_eq!(t.passes().count(), 0);
    t.rapid_to(dvec3(0.0, 0.0, 5.0)).unwrap();
    t.feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Link, 500.0)
        .unwrap();
    t.feed_to(dvec3(1.0, 0.0, 0.0), MoveRole::Cut, 500.0)
        .unwrap();
    t.feed_to(dvec3(1.0, 1.0, 0.0), MoveRole::Link, 500.0)
        .unwrap();
    t.feed_to(dvec3(1.0, 2.0, 0.0), MoveRole::Link, 500.0)
        .unwrap();
    t.feed_to(dvec3(2.0, 2.0, 0.0), MoveRole::Cut, 500.0)
        .unwrap();
    let lengths: Vec<usize> = t.passes().map(<[Move]>::len).collect();
    assert_eq!(lengths, [1, 2, 1, 2]);
    assert_eq!(t.passes().next().unwrap()[0].role, MoveRole::Rapid);
    assert!(t.passes().skip(1).all(|p| p[0].role == MoveRole::Link));
    assert_eq!(t.passes().flatten().count(), t.len());
}

/// Seen from above, an arc about Z runs round its circle, measured exactly
/// however it climbs; one about another axis, along its chords.
#[test]
fn an_arc_is_measured_as_seen_from_above() {
    let helix = Move {
        to: dvec3(2.0, 0.0, -1.0),
        kind: MoveKind::Arc(Arc {
            centre: dvec3(1.0, 0.0, 0.0),
            axis: DVec3::Z,
            sweep: std::f64::consts::PI,
        }),
        role: MoveRole::Ramp,
        feed: Some(400.0),
    };
    assert_eq!(helix.flat_length(DVec3::ZERO), std::f64::consts::PI);
    // A quarter turn down from straight above its centre to level with it:
    // seen from above, it runs out one radius.
    let quarter = Arc {
        centre: DVec3::ZERO,
        axis: DVec3::Y,
        sweep: std::f64::consts::FRAC_PI_2,
    };
    let from = dvec3(0.0, 0.0, 1.0);
    assert!(quarter.ends_at(from, DVec3::X, 1e-12));
    let lead = Move {
        to: DVec3::X,
        kind: MoveKind::Arc(quarter),
        role: MoveRole::Lead,
        feed: Some(400.0),
    };
    assert!((lead.flat_length(from) - 1.0).abs() < 1e-9);
    let straight = Move {
        kind: MoveKind::Linear,
        ..lead
    };
    assert_eq!(straight.flat_length(from), 1.0);
}

/// Run backwards, an arc passes the same points the other way.
#[test]
fn a_reversed_arc_runs_back_over_itself() {
    let from = DVec3::ZERO;
    let m = Move {
        to: dvec3(2.0, 0.0, 0.0),
        kind: MoveKind::Arc(Arc {
            centre: dvec3(1.0, 0.0, 0.0),
            axis: DVec3::Z,
            sweep: std::f64::consts::PI,
        }),
        role: MoveRole::Cut,
        feed: Some(400.0),
    };
    let back = m.reversed(from);
    assert_eq!(back.to, from);
    assert!(back.point_at(m.to, 0.25).distance(m.point_at(from, 0.75)) < 1e-12);
}
