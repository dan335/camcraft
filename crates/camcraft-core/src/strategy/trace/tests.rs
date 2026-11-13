use std::f64::consts::{FRAC_PI_2, PI};

use glam::{dvec2, dvec3};

use super::*;
use crate::path::toolpath::MoveKind;

fn params() -> TraceParams {
    TraceParams {
        beside: Beside::On,
        offset: 3.0,
        raise: 0.0,
        extension: 0.0,
        max_angle: FRAC_PI_2,
        levels: None,
        by_depth: true,
        both_ways: false,
        repeat: false,
        preserve_order: false,
        feed: 800.0,
        tolerance: 0.01,
    }
}

/// The polyline through `points`.
fn lines(points: &[[f64; 3]]) -> Curve {
    let at = |p: &[f64; 3]| dvec3(p[0], p[1], p[2]);
    Curve {
        start: at(&points[0]),
        pieces: points[1..]
            .iter()
            .map(|p| Piece::Line { to: at(p) })
            .collect(),
    }
}

fn square() -> Curve {
    lines(&[
        [0.0, 0.0, 1.0],
        [0.0, 10.0, 1.0],
        [10.0, 10.0, 1.0],
        [10.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
    ])
}

/// Where each pass starts, and its cuts.
fn passes(path: &Toolpath) -> Vec<(DVec3, Vec<DVec3>)> {
    let mut out: Vec<(DVec3, Vec<DVec3>)> = Vec::new();
    for m in path.moves() {
        match m.role {
            MoveRole::Link => out.push((m.to, Vec::new())),
            _ => out.last_mut().unwrap().1.push(m.to),
        }
    }
    out
}

/// On the curve, the tip follows it at its own heights, moved up by the axial
/// offset; an open curve runs from its end lowest in X, and its passes run on
/// past each end along it.
#[test]
fn on_the_curve_at_its_own_height() {
    let c = lines(&[[10.0, 0.0, -1.0], [0.0, 0.0, -2.0]]);
    let p = TraceParams {
        raise: 0.5,
        extension: 2.0,
        ..params()
    };
    let (path, report) = trace(&[c], &p).unwrap();
    assert_eq!((report.curves, report.closed, report.passes), (1, 0, 1));
    let got = passes(&path);
    assert_eq!(got.len(), 1);
    let (start, cuts) = &got[0];
    let along = dvec3(10.0, 0.0, 1.0).normalize();
    assert!(
        start.distance(dvec3(0.0, 0.0, -1.5) - along * 2.0) < 1e-9,
        "{start}"
    );
    assert!(cuts[0].distance(dvec3(0.0, 0.0, -1.5)) < 1e-9);
    assert!(cuts[1].distance(dvec3(10.0, 0.0, -0.5)) < 1e-9);
    assert!(cuts[2].distance(dvec3(10.0, 0.0, -0.5) + along * 2.0) < 1e-9);
}

/// Left of a closed curve, which runs anticlockwise, is inside it; right is
/// outside, its corners rounded. Every cut is the offset from the curve.
#[test]
fn beside_the_curve_left_or_right() {
    for (beside, inside) in [(Beside::Left, true), (Beside::Right, false)] {
        let p = TraceParams {
            beside,
            offset: 2.0,
            ..params()
        };
        let sq = square();
        let (path, report) = trace(std::slice::from_ref(&sq), &p).unwrap();
        assert_eq!(report.closed, 1);
        let mut arcs = 0;
        for m in path.moves().iter().filter(|m| m.role == MoveRole::Cut) {
            assert!(
                (sq.distance_to(m.to.truncate()) - 2.0).abs() < 1e-9,
                "{}",
                m.to
            );
            assert_eq!(m.to.z, 1.0);
            let within = (0.0..=10.0).contains(&m.to.x) && (0.0..=10.0).contains(&m.to.y);
            assert_eq!(inside, within, "{beside:?} {}", m.to);
            arcs += usize::from(matches!(m.kind, MoveKind::Arc(_)));
        }
        assert_eq!(arcs, if inside { 0 } else { 4 });
    }
}

/// With multiple depths, each curve that many stepdowns above itself and then
/// on it: by depth every curve at a level first, by area each curve all the way
/// down; both ways, an open curve's levels go back and forth; repeated, the
/// last of each twice.
#[test]
fn levels_above_the_curve() {
    let a = lines(&[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]);
    let b = lines(&[[0.0, 5.0, 0.0], [10.0, 5.0, 0.0]]);
    let p = TraceParams {
        levels: Some((1.0, 2)),
        ..params()
    };
    let starts = |p: &TraceParams| -> Vec<DVec3> {
        let (path, _) = trace(&[a.clone(), b.clone()], p).unwrap();
        passes(&path).into_iter().map(|(s, _)| s).collect()
    };
    let by_depth = starts(&p);
    assert_eq!(
        by_depth.iter().map(|s| (s.y, s.z)).collect::<Vec<_>>(),
        [
            (0.0, 2.0),
            (5.0, 2.0),
            (0.0, 1.0),
            (5.0, 1.0),
            (0.0, 0.0),
            (5.0, 0.0)
        ]
    );
    let by_area = starts(&TraceParams {
        by_depth: false,
        ..p
    });
    assert_eq!(
        by_area.iter().map(|s| (s.y, s.z)).collect::<Vec<_>>(),
        [
            (0.0, 2.0),
            (0.0, 1.0),
            (0.0, 0.0),
            (5.0, 2.0),
            (5.0, 1.0),
            (5.0, 0.0)
        ]
    );
    let both = starts(&TraceParams {
        by_depth: false,
        both_ways: true,
        ..p
    });
    assert_eq!(
        both.iter().take(3).map(|s| s.x).collect::<Vec<_>>(),
        [0.0, 10.0, 0.0]
    );
    let (_, report) = trace(&[a.clone(), b.clone()], &TraceParams { repeat: true, ..p }).unwrap();
    assert_eq!((report.levels, report.passes), (3, 8));
}

/// What of a curve is steeper than the maximum angle is left out, the pass
/// broken there, and said.
#[test]
fn steeper_than_the_maximum_angle_is_left_out() {
    let c = lines(&[
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.5, 0.0, -5.0],
        [20.0, 0.0, -5.0],
    ]);
    let (path, report) = trace(
        std::slice::from_ref(&c),
        &TraceParams {
            max_angle: 45f64.to_radians(),
            ..params()
        },
    )
    .unwrap();
    assert_eq!((report.steep, report.passes), (1, 2));
    assert_eq!(passes(&path).len(), 2);
    let (_, all) = trace(&[c], &params()).unwrap();
    assert_eq!((all.steep, all.passes), (0, 1));
}

/// A round edge stays an arc; kept outside it, it is the larger circle; kept
/// inside one smaller than the tool, it is refused.
#[test]
fn round_edges_stay_arcs() {
    let circle = Curve {
        start: dvec3(5.0, 0.0, 0.0),
        pieces: vec![Piece::Arc {
            to: dvec3(5.0, 0.0, 0.0),
            centre: dvec2(0.0, 0.0),
            sweep: 2.0 * PI,
        }],
    };
    let (path, _) = trace(
        std::slice::from_ref(&circle),
        &TraceParams {
            beside: Beside::Right,
            ..params()
        },
    )
    .unwrap();
    let cuts: Vec<_> = path
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Cut)
        .collect();
    assert_eq!(cuts.len(), 2);
    for m in cuts {
        assert!(matches!(m.kind, MoveKind::Arc(_)));
        assert!((m.to.truncate().length() - 8.0).abs() < 1e-9);
    }
    assert!(matches!(
        trace(
            &[circle],
            &TraceParams {
                beside: Beside::Left,
                offset: 6.0,
                ..params()
            }
        ),
        Err(TraceError::TooTight { .. })
    ));
    assert!(matches!(
        trace(&[], &params()),
        Err(TraceError::NothingPicked)
    ));
}

/// Unless the order picked is kept, the nearest curve is followed next.
#[test]
fn the_nearest_next_unless_the_order_is_kept() {
    let far = lines(&[[100.0, 0.0, 0.0], [110.0, 0.0, 0.0]]);
    let first = lines(&[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]);
    let near = lines(&[[12.0, 0.0, 0.0], [20.0, 0.0, 0.0]]);
    let order = |keep: bool| -> Vec<f64> {
        let (path, _) = trace(
            &[first.clone(), far.clone(), near.clone()],
            &TraceParams {
                preserve_order: keep,
                ..params()
            },
        )
        .unwrap();
        passes(&path).iter().map(|(s, _)| s.x).collect()
    };
    assert_eq!(order(false), [0.0, 12.0, 100.0]);
    assert_eq!(order(true), [0.0, 100.0, 12.0]);
}
