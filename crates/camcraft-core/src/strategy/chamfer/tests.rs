use super::*;
use crate::path::toolpath::{MoveKind, MoveRole};
use glam::{dvec2, dvec3};

/// A square 10 mm across about the origin at height `z`, anticlockwise.
fn square(z: f64) -> Curve {
    let corners = [(5.0, -5.0), (5.0, 5.0), (-5.0, 5.0), (-5.0, -5.0)];
    Curve {
        start: dvec3(-5.0, -5.0, z),
        pieces: corners
            .iter()
            .map(|&(x, y)| Piece::Line { to: dvec3(x, y, z) })
            .collect(),
    }
}

/// A 6 mm 90° V-bit, chamfering 1 mm wide with its tip half a millimetre
/// below the chamfer.
fn params() -> ChamferParams<'static> {
    ChamferParams {
        tip: 0.0,
        radius: 3.0,
        rise: 1.0,
        width: 1.0,
        tip_offset: 0.5,
        overlap: 0.0,
        lead: 0.6,
        preserve_order: false,
        kept_on_right: true,
        feed: 1000.0,
        tolerance: 0.01,
        lift: None,
    }
}

/// How far `p` is outside the square, seen from above: negative inside.
fn outside(p: DVec2) -> f64 {
    let q = p.abs() - DVec2::splat(5.0);
    q.max(DVec2::ZERO).length() + q.x.max(q.y).min(0.0)
}

/// The square's top edge, the part inside it: the tool runs round it outside,
/// its axis half a millimetre out (the tip offset along a 45° flank), its tip
/// 1.5 mm down (the width's depth and the tip offset), the corners rolled
/// round as arcs, climbing: the part on its right, so clockwise.
#[test]
fn a_sharp_edge_is_chamfered_from_beside_it() {
    let edge = ChamferEdge {
        curve: square(0.0),
        part_on_left: true,
    };
    let (path, report) = chamfer(&[edge], &params()).unwrap();
    assert_eq!((report.edges, report.closed, report.passes), (1, 1, 1));
    assert!((report.depth - 1.5).abs() < 1e-12);
    let cuts: Vec<_> = path
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Cut)
        .collect();
    let arcs = cuts
        .iter()
        .filter(|m| matches!(m.kind, MoveKind::Arc(_)))
        .count();
    assert_eq!(arcs, 4, "each corner rolled round");
    for m in &cuts {
        assert!((m.to.z + 1.5).abs() < 1e-9, "{}", m.to);
    }
    // All but the lead off it are half a millimetre out.
    for m in &cuts[..cuts.len() - 1] {
        assert!((outside(m.to.truncate()) - 0.5).abs() < 1e-6, "{}", m.to);
    }
    // Onto it and off it square to it, from the waste, a lead's length out.
    let first = path.moves().first().unwrap().to;
    let last = cuts.last().unwrap().to;
    assert!((outside(first.truncate()) - 1.1).abs() < 1e-6, "{first}");
    assert!((outside(last.truncate()) - 1.1).abs() < 1e-6, "{last}");
    // Clockwise, with the part on its right.
    let pts: Vec<DVec3> = cuts.iter().map(|m| m.to).collect();
    let area: f64 = pts
        .windows(2)
        .map(|w| w[0].truncate().perp_dot(w[1].truncate()))
        .sum();
    assert!(area < 0.0, "clockwise");
    // Conventional, the other way.
    let p = ChamferParams {
        kept_on_right: false,
        ..params()
    };
    let edge = ChamferEdge {
        curve: square(0.0),
        part_on_left: true,
    };
    let (other, _) = chamfer(&[edge], &p).unwrap();
    let pts: Vec<DVec3> = other
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Cut)
        .map(|m| m.to)
        .collect();
    let area: f64 = pts
        .windows(2)
        .map(|w| w[0].truncate().perp_dot(w[1].truncate()))
        .sum();
    assert!(area > 0.0, "anticlockwise");
}

/// A pocket's rim, the part outside the loop: the tool runs inside it.
#[test]
fn a_rim_is_chamfered_from_inside_the_pocket() {
    let edge = ChamferEdge {
        curve: square(0.0),
        part_on_left: false,
    };
    let (path, _) = chamfer(&[edge], &params()).unwrap();
    let cuts: Vec<_> = path
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Cut)
        .collect();
    for m in &cuts[..cuts.len() - 1] {
        let d = -outside(m.to.truncate());
        assert!(d >= 0.5 - 1e-6, "{} is {d} inside", m.to);
    }
}

/// What the chamfer means to leave, its bevel: the flank's line across from
/// the edge on the part's side, the width in at the edge's height and the
/// width down at the edge; nothing on the other side or past the flank's
/// reach.
#[test]
fn the_bevel_is_the_chamfer_asked_and_only_on_the_part() {
    let edge = ChamferEdge {
        curve: square(10.0),
        part_on_left: true,
    };
    let (_, report) = chamfer(&[edge], &params()).unwrap();
    let bevel = &report.bevels[0];
    assert!((bevel.reach - 2.5).abs() < 1e-12);
    for (d, z) in [(0.0, 9.0), (0.25, 9.25), (1.0, 10.0), (2.0, 11.0)] {
        let h = bevel.height_at(dvec2(5.0 - d, 1.0)).unwrap();
        assert!((h - z).abs() < 1e-9, "{d}: {h}");
    }
    assert_eq!(bevel.height_at(dvec2(5.2, 1.0)), None, "outside the part");
    assert_eq!(bevel.height_at(dvec2(2.4, 1.0)), None, "past the reach");
    // An open edge along +X with the part on its left, above it.
    let open = Bevel {
        points: vec![dvec3(0.0, 0.0, 0.0), dvec3(10.0, 0.0, 0.0)],
        closed: false,
        part_on_left: true,
        width: 1.0,
        rise: 1.0,
        reach: 2.0,
    };
    assert!((open.height_at(dvec2(5.0, 0.5)).unwrap() + 0.5).abs() < 1e-9);
    assert_eq!(open.height_at(dvec2(5.0, -0.5)), None);
}

/// Refused: a chamfer wider than the flank reaches, nothing to chamfer, and an
/// edge too tight to keep beside.
#[test]
fn what_cannot_be_chamfered_is_refused() {
    let edge = ChamferEdge {
        curve: square(0.0),
        part_on_left: false,
    };
    let wide = ChamferParams {
        width: 2.6,
        ..params()
    };
    let err = chamfer(std::slice::from_ref(&edge), &wide).unwrap_err();
    assert!(matches!(err, ChamferError::TooWide { .. }), "{err}");
    assert!(err.to_string().contains("wider tool"), "{err}");
    assert_eq!(
        chamfer(&[], &params()).unwrap_err(),
        ChamferError::NothingPicked
    );
    // A circle 0.4 mm round, the part outside it: the tool's axis would have
    // to be 0.5 mm inside it.
    let hole = ChamferEdge {
        curve: Curve {
            start: dvec3(0.4, 0.0, 0.0),
            pieces: vec![Piece::Arc {
                to: dvec3(0.4, 0.0, 0.0),
                centre: DVec2::ZERO,
                sweep: std::f64::consts::TAU,
            }],
        },
        part_on_left: false,
    };
    let err = chamfer(&[hole], &params()).unwrap_err();
    assert!(matches!(err, ChamferError::TooTight { .. }), "{err}");
}

/// Where the clearance says the tool must be higher, it is lifted, and says
/// how many stretches; elsewhere it stays on the chamfer.
#[test]
fn the_clearance_lifts_the_tool_where_it_must() {
    let lift = |p: DVec2| (p.x > 3.0).then_some(-1.0);
    let p = ChamferParams {
        lift: Some(&lift),
        ..params()
    };
    let edge = ChamferEdge {
        curve: square(0.0),
        part_on_left: true,
    };
    let (path, report) = chamfer(&[edge], &p).unwrap();
    assert_eq!(report.lifted, 1);
    let mut low = 0;
    for m in path.moves().iter().filter(|m| m.role == MoveRole::Cut) {
        if m.to.x > 3.0 + 1e-9 {
            assert!(m.to.z >= -1.0 - 1e-9, "{}", m.to);
        } else {
            low += 1;
            assert!((m.to.z + 1.5).abs() < 1e-9, "{}", m.to);
        }
    }
    assert!(low > 0);
    // Nowhere higher than the chamfer: as it was.
    let never = |_: DVec2| Some(-5.0);
    let p = ChamferParams {
        lift: Some(&never),
        ..params()
    };
    let edge = ChamferEdge {
        curve: square(0.0),
        part_on_left: true,
    };
    let (kept, report) = chamfer(std::slice::from_ref(&edge), &p).unwrap();
    let (plain, _) = chamfer(&[edge], &params()).unwrap();
    assert_eq!(report.lifted, 0);
    assert_eq!(kept.moves(), plain.moves());
}

/// A closed edge's pass goes on past its start by the finishing overlap; the
/// edges are cut nearest next unless their order is kept.
#[test]
fn overlap_and_order() {
    let edge = ChamferEdge {
        curve: square(0.0),
        part_on_left: true,
    };
    let length = |p: &ChamferParams<'_>| {
        let (path, _) = chamfer(std::slice::from_ref(&edge), p).unwrap();
        let mut at = path.moves()[0].to;
        let mut run = 0.0;
        for m in path.moves().iter().skip(1) {
            run += m.length(at);
            at = m.to;
        }
        run
    };
    let more = ChamferParams {
        overlap: 2.0,
        ..params()
    };
    assert!((length(&more) - length(&params()) - 2.0).abs() < 1e-6);

    let far = ChamferEdge {
        curve: square(0.0).raised(0.0),
        part_on_left: true,
    };
    let mut moved = far.clone();
    moved.curve = Curve {
        start: far.curve.start + dvec3(30.0, 0.0, 0.0),
        pieces: far
            .curve
            .pieces
            .iter()
            .map(|p| Piece::Line {
                to: p.to() + dvec3(30.0, 0.0, 0.0),
            })
            .collect(),
    };
    let near = ChamferEdge {
        curve: Curve {
            start: far.curve.start + dvec3(15.0, 0.0, 0.0),
            pieces: far
                .curve
                .pieces
                .iter()
                .map(|p| Piece::Line {
                    to: p.to() + dvec3(15.0, 0.0, 0.0),
                })
                .collect(),
        },
        part_on_left: true,
    };
    let edges = [far, moved, near];
    let order = |p: &ChamferParams<'_>| {
        let (_, report) = chamfer(&edges, p).unwrap();
        report
            .bevels
            .iter()
            .map(|b| b.points[0].x.round())
            .collect::<Vec<_>>()
    };
    assert_eq!(order(&params()), [-5.0, 10.0, 25.0]);
    let kept = ChamferParams {
        preserve_order: true,
        ..params()
    };
    assert_eq!(order(&kept), [-5.0, 25.0, 10.0]);
}
