use std::f64::consts::FRAC_PI_4;

use super::*;
use crate::fixture::cut_through;
use glam::dvec3;

fn params(material_on_right: Option<bool>) -> CornerParams {
    CornerParams {
        most_turn: FRAC_PI_4,
        radius: 3.0,
        distance: 2.0,
        feed: 300.0,
        only_inner: true,
        material_on_right,
    }
}

/// Each move's feed, where it ends, after the first.
fn feeds(path: &Toolpath) -> Vec<(DVec3, f64)> {
    path.moves()[1..]
        .iter()
        .map(|m| (m.to, m.feed.unwrap()))
        .collect()
}

/// Round a 20 mm square anticlockwise, the material on the right, as a pocket
/// climbs round its walls: each corner turns left, into it, and is slowed from
/// 2 mm before to 2 mm after, the moves cut where the feed changes and going
/// where they went.
#[test]
fn a_pockets_corners_are_slowed_into_and_out_of() {
    let square = [
        dvec3(0.0, 0.0, -1.0),
        dvec3(20.0, 0.0, -1.0),
        dvec3(20.0, 20.0, -1.0),
        dvec3(0.0, 20.0, -1.0),
        dvec3(0.0, 0.0, -1.0),
    ];
    let path = cut_through(&square, 1000.0);
    let (slowed, report) = slow_corners(&path, &params(Some(true))).unwrap();
    assert_eq!(report.corners, 3, "the ends of the run are not corners");
    assert!((report.slowed - 12.0).abs() < 1e-9, "{report:?}");
    assert_eq!(slowed.end(), path.end());
    let f = feeds(&slowed);
    assert_eq!(f[0], (dvec3(18.0, 0.0, -1.0), 1000.0));
    assert_eq!(f[1], (dvec3(20.0, 0.0, -1.0), 300.0));
    assert_eq!(f[2], (dvec3(20.0, 2.0, -1.0), 300.0));
    assert_eq!(f[3], (dvec3(20.0, 18.0, -1.0), 1000.0));
    assert_eq!(f.len(), 10, "2 + 3 + 3 + 2 pieces");

    // Round a boss the other way, its corners turn away from the material:
    // none is slowed, unless every corner is asked for.
    let (_, report) = slow_corners(&path, &params(Some(false))).unwrap();
    assert_eq!(report.corners, 0);
    let every = CornerParams {
        only_inner: false,
        ..params(Some(false))
    };
    assert_eq!(slow_corners(&path, &every).unwrap().1.corners, 3);
    // Where which side the material is on is not known, every corner is.
    assert_eq!(slow_corners(&path, &params(None)).unwrap().1.corners, 3);
}

/// Spans slowed that meet are one; a turn gentler than the most allowed is not
/// a corner; and a cut already slower than the reduced feed keeps its own.
#[test]
fn spans_meet_and_gentle_turns_are_left() {
    let zig = [
        dvec3(0.0, 0.0, 0.0),
        dvec3(10.0, 0.0, 0.0),
        dvec3(10.0, 3.0, 0.0),
        dvec3(20.0, 3.0, 0.0),
        dvec3(30.0, 5.0, 0.0),
    ];
    let (slowed, report) = slow_corners(&cut_through(&zig, 1000.0), &params(None)).unwrap();
    assert_eq!(report.corners, 2, "{report:?}");
    // 8 to 15 along: the two corners' spans, 3 mm apart, are one.
    assert!((report.slowed - 7.0).abs() < 1e-9, "{report:?}");
    let f = feeds(&slowed);
    assert_eq!(f.iter().filter(|(_, feed)| *feed == 300.0).count(), 3);

    let (slow, _) = slow_corners(&cut_through(&zig, 200.0), &params(None)).unwrap();
    assert!(feeds(&slow).iter().all(|(_, f)| *f == 200.0));
}

/// An arc tighter than the radius is slowed along, and the distance either
/// side; one wider is not; and a slowed arc's pieces end where it did.
#[test]
fn tight_arcs_are_slowed_along() {
    for (r, slowed) in [(2.0, true), (5.0, false)] {
        let mut p = Toolpath::new();
        p.rapid_to(dvec3(-10.0, 0.0, 0.0)).unwrap();
        p.feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Cut, 1000.0)
            .unwrap();
        // Half round anticlockwise, about (0, r).
        let arc = Arc {
            centre: dvec3(0.0, r, 0.0),
            axis: DVec3::Z,
            sweep: std::f64::consts::PI,
        };
        p.arc_to(dvec3(0.0, 2.0 * r, 0.0), arc, MoveRole::Cut, 1000.0)
            .unwrap();
        p.feed_to(dvec3(-10.0, 2.0 * r, 0.0), MoveRole::Cut, 1000.0)
            .unwrap();
        let (out, report) = slow_corners(&p, &params(Some(true))).unwrap();
        assert_eq!(report.arcs, u64::from(slowed));
        assert_eq!(out.end(), p.end());
        if slowed {
            let along = std::f64::consts::PI * r + 4.0;
            assert!((report.slowed - along).abs() < 1e-9, "{report:?}");
            let arc = out
                .moves()
                .iter()
                .find(|m| matches!(m.kind, MoveKind::Arc(_)));
            assert_eq!(arc.unwrap().feed, Some(300.0));
        } else {
            assert_eq!(out.len(), p.len());
        }
    }
}

/// Along a valley, turning up, a pass meets the slope ahead: inner. Over a
/// ridge, turning down: not. Rapids and links are left as they are.
#[test]
fn a_valley_is_inner_and_a_ridge_is_not() {
    let valley = [
        dvec3(0.0, 0.0, 5.0),
        dvec3(5.0, 0.0, 0.0),
        dvec3(10.0, 0.0, 5.0),
    ];
    let ridge = [
        dvec3(0.0, 0.0, 0.0),
        dvec3(5.0, 0.0, 5.0),
        dvec3(10.0, 0.0, 0.0),
    ];
    let p = params(Some(true));
    assert_eq!(
        slow_corners(&cut_through(&valley, 900.0), &p)
            .unwrap()
            .1
            .corners,
        1
    );
    assert_eq!(
        slow_corners(&cut_through(&ridge, 900.0), &p)
            .unwrap()
            .1
            .corners,
        0
    );
    let path = cut_through(&valley, 900.0);
    let (out, _) = slow_corners(&path, &p).unwrap();
    assert_eq!(out.moves()[0], path.moves()[0]);
}
