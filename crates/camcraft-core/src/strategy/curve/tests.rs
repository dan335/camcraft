use std::f64::consts::{FRAC_PI_2, PI};

use glam::{dvec2, dvec3};

use super::*;
use crate::path::toolpath::MoveKind;

/// The polyline through `points`, at `z`.
fn lines(points: &[[f64; 2]], z: f64) -> Curve {
    let at = |p: &[f64; 2]| dvec3(p[0], p[1], z);
    Curve {
        start: at(&points[0]),
        pieces: points[1..]
            .iter()
            .map(|p| Piece::Line { to: at(p) })
            .collect(),
    }
}

/// A square `half` each way about the origin, anticlockwise, at `z`.
fn square(half: f64, z: f64) -> Curve {
    lines(
        &[
            [-half, -half],
            [half, -half],
            [half, half],
            [-half, half],
            [-half, -half],
        ],
        z,
    )
}

/// A rectangle `w` by `h` about the origin with corners of radius `r`,
/// anticlockwise, at height 0.
fn rounded(w: f64, h: f64, r: f64) -> Curve {
    let (x, y) = (w / 2.0, h / 2.0);
    let p = |a: f64, b: f64| dvec3(a, b, 0.0);
    Curve {
        start: p(-x + r, -y),
        pieces: vec![
            Piece::Line { to: p(x - r, -y) },
            Piece::Arc {
                to: p(x, -y + r),
                centre: dvec2(x - r, -y + r),
                sweep: FRAC_PI_2,
            },
            Piece::Line { to: p(x, y - r) },
            Piece::Arc {
                to: p(x - r, y),
                centre: dvec2(x - r, y - r),
                sweep: FRAC_PI_2,
            },
            Piece::Line { to: p(-x + r, y) },
            Piece::Arc {
                to: p(-x, y - r),
                centre: dvec2(-x + r, y - r),
                sweep: FRAC_PI_2,
            },
            Piece::Line { to: p(-x, -y + r) },
            Piece::Arc {
                to: p(-x + r, -y),
                centre: dvec2(-x + r, -y + r),
                sweep: FRAC_PI_2,
            },
        ],
    }
}

/// A line offset either way moves square to itself, keeping its heights.
#[test]
fn a_line_moves_square_to_itself() {
    let mut c = lines(&[[0.0, 0.0], [10.0, 0.0]], 0.0);
    c.pieces[0] = Piece::Line {
        to: dvec3(10.0, 0.0, -2.0),
    };
    let left = offset(&c, 1.5, 1e-6).unwrap();
    assert_eq!(left.start, dvec3(0.0, 1.5, 0.0));
    assert_eq!(left.end(), dvec3(10.0, 1.5, -2.0));
    let right = offset(&c, -1.5, 1e-6).unwrap();
    assert_eq!(right.start, dvec3(0.0, -1.5, 0.0));
}

/// A square run anticlockwise, moved in, is trimmed at each corner where its
/// sides cross; moved out, each corner is rounded about it, a quarter turn.
/// Either way, every point of it is as far from the square as asked.
#[test]
fn corners_are_trimmed_inside_and_rounded_outside() {
    let sq = square(5.0, 0.0);
    assert!((sq.signed_area() - 100.0).abs() < 1e-9);
    let inside = offset(&sq, 1.0, 1e-6).unwrap();
    assert!(inside.is_closed(1e-9), "{inside:?}");
    assert!(
        inside
            .pieces
            .iter()
            .all(|p| matches!(p, Piece::Line { .. }))
    );
    assert!((inside.signed_area() - 64.0).abs() < 1e-9);
    for p in inside.sample(1e-3) {
        assert!((sq.distance_to(p.truncate()) - 1.0).abs() < 1e-9, "{p}");
    }
    let outside = offset(&sq, -1.0, 1e-6).unwrap();
    assert!(outside.is_closed(1e-9));
    let arcs = outside
        .pieces
        .iter()
        .filter(|p| matches!(p, Piece::Arc { sweep, .. } if (sweep - FRAC_PI_2).abs() < 1e-9))
        .count();
    assert_eq!(arcs, 4);
    for p in outside.sample(1e-3) {
        assert!((sq.distance_to(p.truncate()) - 1.0).abs() < 1e-6, "{p}");
    }
    assert!((outside.signed_area() - (144.0 - 4.0 + PI)).abs() < 1e-6);
}

/// Round corners shrink about their centres moved in and grow moved out; one
/// shrunk exactly to its centre is left out, and one that would turn inside
/// out is refused.
#[test]
fn round_corners_shrink_and_grow_about_their_centres() {
    let r = rounded(20.0, 10.0, 3.0);
    let shrunk = offset(&r, 1.0, 1e-6).unwrap();
    let radii: Vec<f64> = shrunk
        .steps()
        .filter_map(|(f, p)| match p {
            Piece::Arc { centre, .. } => Some(f.truncate().distance(*centre)),
            Piece::Line { .. } => None,
        })
        .collect();
    assert_eq!(radii.len(), 4);
    assert!(radii.iter().all(|r| (r - 2.0).abs() < 1e-9), "{radii:?}");
    let to_centres = offset(&r, 3.0, 1e-6).unwrap();
    assert!(
        to_centres
            .pieces
            .iter()
            .all(|p| matches!(p, Piece::Line { .. })),
        "{to_centres:?}"
    );
    for p in to_centres.sample(1e-3) {
        assert!((r.distance_to(p.truncate()) - 3.0).abs() < 1e-9);
    }
    assert!(offset(&r, 3.5, 1e-6).is_err());
    let grown = offset(&r, -2.0, 1e-6).unwrap();
    for p in grown.sample(1e-3) {
        assert!((r.distance_to(p.truncate()) - 2.0).abs() < 1e-6);
    }
}

/// A corner whose two sides, moved in, no longer cross is too tight.
#[test]
fn a_corner_too_tight_inside_is_refused() {
    // A narrow V, its arms 1 mm long: moved 2 mm inside, they miss.
    let v = lines(&[[0.0, 0.0], [1.0, 0.2], [0.0, 0.4]], 0.0);
    let err = offset(&v, 2.0, 1e-6).unwrap_err();
    assert!(err.at.distance(dvec2(1.0, 0.2)) < 1e-9, "{err:?}");
}

/// Edges are joined end to end where two meet, either way round, into one
/// open curve or round into a closed one; three meeting at a corner end the
/// curve there.
#[test]
fn edges_join_where_two_meet() {
    let edge = |ends: [u32; 2], a: [f64; 2], b: [f64; 2]| Edge {
        model: "m".into(),
        ends: Some(ends),
        curve: lines(&[a, b], 0.0),
    };
    // 1 -> 2 -> 3, the middle one given backwards.
    let open = join(&[
        edge([1, 2], [0.0, 0.0], [1.0, 0.0]),
        edge([3, 2], [1.0, 1.0], [1.0, 0.0]),
    ]);
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].start, dvec3(0.0, 0.0, 0.0));
    assert_eq!(open[0].end(), dvec3(1.0, 1.0, 0.0));
    // A triangle, given out of order: one closed curve.
    let closed = join(&[
        edge([2, 3], [1.0, 0.0], [0.0, 1.0]),
        edge([1, 2], [0.0, 0.0], [1.0, 0.0]),
        edge([3, 1], [0.0, 1.0], [0.0, 0.0]),
    ]);
    assert_eq!(closed.len(), 1);
    assert!(closed[0].is_closed(1e-9));
    assert_eq!(closed[0].pieces.len(), 3);
    // A fork: three meet at 2, so nothing joins there.
    let fork = join(&[
        edge([1, 2], [0.0, 0.0], [1.0, 0.0]),
        edge([2, 3], [1.0, 0.0], [2.0, 0.0]),
        edge([2, 4], [1.0, 0.0], [1.0, 1.0]),
    ]);
    assert_eq!(fork.len(), 3);
    // On another model, the same corner numbers are other corners.
    let mut other = edge([2, 3], [5.0, 0.0], [6.0, 0.0]);
    other.model = "n".into();
    let two = join(&[edge([1, 2], [0.0, 0.0], [1.0, 0.0]), other]);
    assert_eq!(two.len(), 2);
}

/// A whole circle is cut as arcs of half a turn, each ending where it should,
/// about its centre, and the area it closes is the circle's.
#[test]
fn a_circle_is_cut_in_half_turns() {
    let c = Curve {
        start: dvec3(4.0, 0.0, -1.0),
        pieces: vec![Piece::Arc {
            to: dvec3(4.0, 0.0, -1.0),
            centre: dvec2(0.0, 0.0),
            sweep: -2.0 * PI,
        }],
    };
    assert!((c.signed_area() + PI * 16.0).abs() < 1e-9);
    let mut path = Toolpath::new();
    path.feed_to(c.start, MoveRole::Link, 100.0).unwrap();
    c.cut_into(&mut path, MoveRole::Cut, 100.0).unwrap();
    let arcs: Vec<_> = path.moves()[1..]
        .iter()
        .map(|m| match m.kind {
            MoveKind::Arc(a) => a,
            MoveKind::Linear => panic!("a straight move"),
        })
        .collect();
    assert_eq!(arcs.len(), 2);
    assert!(arcs.iter().all(|a| (a.sweep + PI).abs() < 1e-9));
    assert_eq!(path.end(), Some(c.start));
    let reversed = c.reversed();
    assert!((reversed.signed_area() - PI * 16.0).abs() < 1e-9);
}

/// Run back from its end, a curve goes back along itself as far as asked,
/// round its arcs too, and no further than its start.
#[test]
fn back_from_the_end_along_the_curve() {
    let r = rounded(20.0, 10.0, 3.0);
    let back = r.back_from_end(2.0);
    assert_eq!(back.start, r.end());
    assert!((back.flat_length() - 2.0).abs() < 1e-9);
    let all = lines(&[[0.0, 0.0], [3.0, 0.0]], 0.0).back_from_end(10.0);
    assert_eq!(all.end(), dvec3(0.0, 0.0, 0.0));
}
