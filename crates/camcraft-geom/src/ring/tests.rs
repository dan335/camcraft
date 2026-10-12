use super::*;
use glam::dvec2;
use std::f64::consts::PI;

/// A rounded rectangle `w` by `h`, corners of radius `r`, counter-clockwise,
/// as a tessellation gives it: each corner as `steps` chords.
fn rounded(w: f64, h: f64, r: f64, steps: u32) -> (Vec<DVec2>, Vec<(DVec2, f64)>) {
    let centres = [
        dvec2(w - r, r),
        dvec2(w - r, h - r),
        dvec2(r, h - r),
        dvec2(r, r),
    ];
    let mut pts = Vec::new();
    for (k, c) in centres.iter().enumerate() {
        let a0 = -FRAC_PI_2 + f64::from(u32::try_from(k).unwrap()) * FRAC_PI_2;
        for s in 0..=steps {
            let a = a0 + FRAC_PI_2 * f64::from(s) / f64::from(steps);
            pts.push(*c + DVec2::from_angle(a) * r);
        }
    }
    (pts, centres.iter().map(|c| (*c, r)).collect())
}

/// A tessellated rounded rectangle snaps to four lines and four exact
/// quarter arcs of the model's corners.
#[test]
fn a_tessellated_round_corner_snaps_to_its_circle() {
    let (pts, circles) = rounded(100.0, 50.0, 3.175, 9);
    let ring = Ring::snapped(&pts, &circles, 1e-3, 0.05).unwrap();
    let arcs: Vec<&Span> = ring
        .spans()
        .iter()
        .filter(|s| matches!(s, Span::Arc { .. }))
        .collect();
    assert_eq!(arcs.len(), 4, "{ring:?}");
    for a in arcs {
        let Span::Arc { sweep, .. } = a else {
            unreachable!()
        };
        assert!((sweep - FRAC_PI_2).abs() < 1e-9, "{sweep}");
    }
    assert_eq!(ring.spans().len(), 8);
    assert!((ring.length() - (2.0 * (100.0 + 50.0 - 4.0 * 3.175) + TAU * 3.175)).abs() < 1e-9);
    assert!(!ring.starts_at_corner(), "a round ring has no corner");
}

/// A chord across a circle is not an arc of it: a D whose flat's ends are on
/// the circle keeps its flat.
#[test]
fn a_chord_is_not_an_arc() {
    let c = dvec2(0.0, 0.0);
    let mut pts: Vec<DVec2> = (0..=18)
        .map(|k| DVec2::from_angle(-FRAC_PI_2 + PI * f64::from(k) / 18.0) * 10.0)
        .collect();
    pts.dedup();
    let ring = Ring::snapped(&pts, &[(c, 10.0)], 1e-3, 0.05).unwrap();
    assert_eq!(ring.spans().len(), 2, "{ring:?}");
    assert!(
        matches!(ring.spans()[0], Span::Arc { .. }) != matches!(ring.spans()[1], Span::Arc { .. })
    );
}

/// Moved out, a rounded rectangle's arcs grow by the distance and its lines
/// move out square to themselves: still tangent, still eight spans.
#[test]
fn an_offset_moves_lines_and_grows_arcs() {
    let (pts, circles) = rounded(100.0, 50.0, 3.175, 9);
    let ring = Ring::snapped(&pts, &circles, 1e-3, 0.05).unwrap();
    // Counter-clockwise: its right is outside.
    let out = ring.offset(3.175).unwrap();
    assert_eq!(out.spans().len(), 8, "{out:?}");
    for (from, s) in out.pieces() {
        if let Span::Arc { centre, .. } = s {
            assert!((from.distance(*centre) - 6.35).abs() < 1e-9);
        }
    }
    let expected = 2.0 * (100.0 + 50.0 - 4.0 * 3.175) + TAU * 6.35;
    assert!((out.length() - expected).abs() < 1e-9, "{}", out.length());
    assert!(!out.starts_at_corner());
    // Back in again, the same ring.
    let back = out.offset(-3.175).unwrap();
    assert!((back.length() - ring.length()).abs() < 1e-9);
}

/// Round a sharp corner the tool's centre turns an arc about the corner,
/// the tool's radius; inside one, the moved sides are cut where they cross.
#[test]
fn corners_open_round_and_close_cut() {
    let square = Ring::from_points(&[
        dvec2(0.0, 0.0),
        dvec2(10.0, 0.0),
        dvec2(10.0, 10.0),
        dvec2(0.0, 10.0),
    ])
    .unwrap();
    let out = square.offset(2.0).unwrap();
    assert_eq!(out.spans().len(), 8);
    let expected = 40.0 + TAU * 2.0;
    assert!((out.length() - expected).abs() < 1e-9);
    let pts: Vec<DVec2> = out.points(0.001);
    assert!(
        pts.iter()
            .all(|p| (p.x.max(p.y) - 12.0 < 1e-9) && (p.x.min(p.y) + 2.0 > -1e-9))
    );
    let inside = square.offset(-2.0).unwrap();
    assert_eq!(inside.spans().len(), 4);
    assert!((inside.length() - 24.0).abs() < 1e-9);
    // Too far in, it turns inside out.
    assert!(square.offset(-6.0).is_none());
}

/// Round a ring by a distance, as often as it takes, ends that far on.
#[test]
fn along_a_ring() {
    let square = Ring::from_points(&[
        dvec2(0.0, 0.0),
        dvec2(10.0, 0.0),
        dvec2(10.0, 10.0),
        dvec2(0.0, 10.0),
    ])
    .unwrap();
    let (spans, end) = square.along(15.0);
    assert_eq!(spans.len(), 2);
    assert!((end - dvec2(10.0, 5.0)).length() < 1e-12);
    let (spans, end) = square.along(45.0);
    assert_eq!(spans.len(), 5, "once round and a quarter more");
    assert!((end - dvec2(5.0, 0.0)).length() < 1e-12);
    let (_, end) = square.along(40.0);
    assert!(end.length() < 1e-12, "exactly round is back at the start");
}

/// A ring is the same shape as a polygon only where every point of each is
/// near the other.
#[test]
fn close_to_checks_both_ways() {
    let (pts, circles) = rounded(100.0, 50.0, 3.175, 9);
    let ring = Ring::snapped(&pts, &circles, 1e-3, 0.05).unwrap();
    assert!(ring.close_to(&pts, 0.02));
    let mut short = pts.clone();
    short.truncate(pts.len() / 2);
    assert!(!ring.close_to(&short, 0.02));
}

/// Started anywhere, reversed, or halfway along its longest span, a ring is
/// the same shape.
#[test]
fn restarting_and_reversing() {
    let (pts, circles) = rounded(100.0, 50.0, 3.175, 9);
    let ring = Ring::snapped(&pts, &circles, 1e-3, 0.05).unwrap();
    let length = ring.length();
    for r in [
        ring.starting_near(dvec2(50.0, -5.0)),
        ring.starting_near(dvec2(101.0, -1.0)),
        ring.longest_middle(),
        ring.reversed(),
    ] {
        assert!((r.length() - length).abs() < 1e-9);
        assert!(r.close_to(&pts, 0.02));
    }
    let mid = ring.longest_middle();
    assert!(
        (mid.start() - dvec2(50.0, 0.0)).length() < 1e-9,
        "{:?}",
        mid.start()
    );
    let near = ring.starting_near(dvec2(101.0, -1.0));
    assert!(
        near.start()
            .distance(dvec2(100.0 - 3.175, 3.175) + DVec2::from_angle(-PI / 4.0) * 3.175)
            < 1e-9
    );
}
