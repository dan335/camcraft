use super::*;
use crate::path::toolpath::Move;

/// A closed loop through `points`, level at `z`.
fn polygon(points: &[[f64; 2]], z: f64) -> Curve {
    let p = |q: &[f64; 2]| DVec3::new(q[0], q[1], z);
    let mut pieces: Vec<Piece> = points
        .iter()
        .skip(1)
        .map(|q| Piece::Line { to: p(q) })
        .collect();
    pieces.push(Piece::Line { to: p(&points[0]) });
    Curve {
        start: p(&points[0]),
        pieces,
    }
}

/// A 6 mm 90° V-bit with a point, as Fusion's test plate was engraved with:
/// down 0.95 of its flank, 2.85 mm.
fn vee() -> EngraveParams {
    EngraveParams {
        tip: 0.0,
        rise: 1.0,
        radius: 3.0,
        depth: 2.85,
        sharp_corner: 165f64.to_radians(),
        stepdown: None,
        feed: 1000.0,
        tolerance: 0.01,
    }
}

const TRIANGLE: [[f64; 2]; 3] = [[-30.0, -15.0], [-10.0, -15.0], [-20.0, 10.0]];
const L: [[f64; 2]; 6] = [
    [0.0, -15.0],
    [20.0, -15.0],
    [20.0, -11.0],
    [4.0, -11.0],
    [4.0, 10.0],
    [0.0, 10.0],
];
const RECTANGLE: [[f64; 2]; 4] = [[24.0, -12.0], [38.0, -12.0], [38.0, 12.0], [24.0, 12.0]];

fn engraved(loops: &[Curve], p: &EngraveParams) -> (Toolpath, EngraveReport) {
    let given: Vec<(Curve, f64)> = loops.iter().map(|c| (c.clone(), 10.0)).collect();
    let s = shapes(&given, p.tolerance).unwrap();
    engrave(&s, p).unwrap()
}

fn cuts(path: &Toolpath) -> Vec<&Move> {
    path.moves()
        .iter()
        .filter(|m| m.role == MoveRole::Cut)
        .collect()
}

fn passes_through(path: &Toolpath, x: f64, y: f64, z: f64) -> bool {
    path.moves()
        .iter()
        .any(|m| m.to.distance(DVec3::new(x, y, z)) <= 2e-3)
}

/// The triangle as Fusion's program cuts it: from its lowest left corner at
/// the surface down the bisector, round the outline's offset 2.85 mm in at
/// the Bottom, up into each corner and back, every wall one move, and the
/// way back up to where it started left off.
#[test]
fn a_triangle_is_cut_round_its_offset_and_into_its_corners() {
    let (path, report) = engraved(&[polygon(&TRIANGLE, 10.0)], &vee());
    let c = cuts(&path);
    assert_eq!(path.moves()[0].to, DVec3::new(-30.0, -15.0, 10.0));
    for [x, y, z] in [
        [-25.79, -12.15, 7.15],
        [-14.21, -12.15, 7.15],
        [-10.0, -15.0, 10.0],
        [-20.0, 2.326, 7.15],
        [-20.0, 10.0, 10.0],
    ] {
        assert!(passes_through(&path, x, y, z), "({x}, {y}, {z}): {c:?}");
    }
    assert_eq!(c.len(), 8, "{c:?}");
    let end = path.end().unwrap();
    assert!(
        end.distance(DVec3::new(-25.79, -12.15, 7.15)) < 2e-3,
        "{end}"
    );
    assert_eq!(report.corners, 3);
    assert!((report.deepest - 2.85).abs() < 1e-9);
}

/// The L, a stroke 4 wide, along its middle 2 mm deep, round the parabola
/// its inside corner pulls the middle into, into each outside corner, and
/// back along the middle: its end, already cut, left off.
#[test]
fn a_stroke_is_cut_along_its_middle() {
    let (path, report) = engraved(&[polygon(&L, 10.0)], &vee());
    for [x, y, z] in [
        [2.343, -12.657, 7.657],
        [4.0, -13.0, 8.0],
        [18.0, -13.0, 8.0],
        [20.0, -15.0, 10.0],
        [20.0, -11.0, 10.0],
        [2.0, -11.0, 8.0],
        [2.0, 8.0, 8.0],
        [4.0, 10.0, 10.0],
    ] {
        assert!(passes_through(&path, x, y, z), "({x}, {y}, {z})");
    }
    let end = path.end().unwrap();
    assert!(end.distance(DVec3::new(0.0, 10.0, 10.0)) < 1e-6, "{end}");
    assert!((report.deepest - (4.0 * 2f64.sqrt() / (1.0 + 2f64.sqrt()))).abs() < 1e-6);
    // The middle at its depth: each point as far from the nearest wall as it
    // is deep.
    let outline = polygon(&L, 10.0);
    for m in cuts(&path) {
        let p = m.to;
        let near = outline.distance_to(p.truncate());
        assert!((10.0 - p.z - near).abs() < 0.01, "{p}: {near}");
    }
}

/// The rectangle, wider than the V reaches: round its offset at the Bottom
/// and into its four corners, eleven moves.
#[test]
fn a_rectangle_is_cut_round_and_into_its_corners() {
    let (path, _) = engraved(&[polygon(&RECTANGLE, 10.0)], &vee());
    let c = cuts(&path);
    assert_eq!(c.len(), 11, "{c:?}");
    for [x, y] in [[26.85, -9.15], [35.15, -9.15], [35.15, 9.15], [26.85, 9.15]] {
        assert!(passes_through(&path, x, y, 7.15), "({x}, {y})");
    }
}

/// All three, as Fusion's program cuts them: in the order given, each its own
/// pass.
#[test]
fn shapes_are_cut_in_the_order_given() {
    let loops = [
        polygon(&TRIANGLE, 10.0),
        polygon(&L, 10.0),
        polygon(&RECTANGLE, 10.0),
    ];
    let (path, report) = engraved(&loops, &vee());
    assert_eq!(report.shapes, 3);
    assert_eq!(report.passes, 3);
    let starts: Vec<DVec3> = path
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Link)
        .map(|m| m.to)
        .collect();
    assert_eq!(
        starts,
        [
            DVec3::new(-30.0, -15.0, 10.0),
            DVec3::new(0.0, -15.0, 10.0),
            DVec3::new(24.0, -12.0, 10.0)
        ]
    );
}

/// A loop given clockwise is walked anticlockwise all the same, and one
/// inside another is an island: walked clockwise, the shape on its left, and
/// never nearer it than the outline.
#[test]
fn an_island_is_part_of_the_outline() {
    let outer = polygon(&[[0.0, 0.0], [0.0, 30.0], [30.0, 30.0], [30.0, 0.0]], 10.0);
    let island = polygon(
        &[[10.0, 10.0], [20.0, 10.0], [20.0, 20.0], [10.0, 20.0]],
        10.0,
    );
    let s = shapes(&[(outer, 10.0), (island, 10.0)], 0.01).unwrap();
    assert_eq!(s.len(), 1);
    assert!(s[0].loops[0].signed_area() > 0.0);
    assert!(s[0].loops[1].signed_area() < 0.0);
    let (path, report) = engrave(&s, &vee()).unwrap();
    assert_eq!(report.loops, 2);
    for m in cuts(&path) {
        let p = m.to;
        let out = p.x.min(p.y).min(30.0 - p.x).min(30.0 - p.y);
        let dx = (10.0 - p.x).max(p.x - 20.0).max(0.0);
        let dy = (10.0 - p.y).max(p.y - 20.0).max(0.0);
        let from_island = (dx * dx + dy * dy).sqrt();
        // Not inside the island, and as deep as the nearer wall lets it.
        assert!(dx > 0.0 || dy > 0.0, "{p}");
        let near = out.min(from_island).min(2.85);
        assert!((10.0 - p.z - near).abs() < 0.01, "{p}: {near}");
    }
}

/// An island in a channel narrower than twice the V's reach: its walk meets
/// the outline's along the channel's middle, and is gone round from there, one
/// pass with the outline's, every point as deep as the nearer wall lets it.
#[test]
fn an_island_near_the_outline_is_one_pass_with_it() {
    let outer = polygon(&[[0.0, 0.0], [30.0, 0.0], [30.0, 30.0], [0.0, 30.0]], 10.0);
    let island = polygon(&[[4.0, 4.0], [4.0, 26.0], [26.0, 26.0], [26.0, 4.0]], 10.0);
    let (path, report) = engraved(&[outer, island], &vee());
    assert_eq!((report.loops, report.passes), (2, 1));
    let mut round_island = false;
    for m in cuts(&path) {
        let p = m.to;
        let out = p.x.min(p.y).min(30.0 - p.x).min(30.0 - p.y);
        let dx = (4.0 - p.x).max(p.x - 26.0).max(0.0);
        let dy = (4.0 - p.y).max(p.y - 26.0).max(0.0);
        let from_island = (dx * dx + dy * dy).sqrt();
        assert!(dx > 0.0 || dy > 0.0, "{p}");
        let near = out.min(from_island).min(2.85);
        assert!((10.0 - p.z - near).abs() < 0.01, "{p}: {near}");
        // Round the island's corners, 2 mm off them.
        round_island |= dx > 1.0 && dy > 1.0;
    }
    assert!(round_island);
}

/// A round shape wider than the V reaches is cut round an arc at the Bottom.
#[test]
fn a_round_shape_is_cut_round_an_arc() {
    let start = DVec3::new(10.0, 0.0, 10.0);
    let circle = Curve {
        start,
        pieces: vec![Piece::Arc {
            to: start,
            centre: DVec2::ZERO,
            sweep: TAU,
        }],
    };
    let (path, _) = engraved(&[circle], &vee());
    let c = cuts(&path);
    assert!(c.len() <= 3, "{c:?}");
    for m in &c {
        assert!((m.to.truncate().length() - 7.15).abs() < 1e-6, "{}", m.to);
        assert!((m.to.z - 7.15).abs() < 1e-9);
    }
}

/// A corner blunter than the sharp corner angle is gone round, not run into.
#[test]
fn a_blunt_corner_is_not_run_into() {
    // A bend of 170° in the bottom wall.
    let bend = 10f64.to_radians();
    let shape = polygon(
        &[
            [0.0, 0.0],
            [20.0, 0.0],
            [20.0 + 20.0 * bend.cos(), 20.0 * bend.sin()],
            [20.0 + 20.0 * bend.cos(), 30.0],
            [0.0, 30.0],
        ],
        10.0,
    );
    let (path, report) = engraved(&[shape], &vee());
    assert_eq!(report.corners, 4);
    assert!(!passes_through(&path, 20.0, 0.0, 10.0));
    // Every move at the Bottom stays there but where it runs into a corner.
    let sharp = vee();
    let less = EngraveParams {
        sharp_corner: 175f64.to_radians(),
        ..sharp
    };
    let shape = polygon(
        &[
            [0.0, 0.0],
            [20.0, 0.0],
            [20.0 + 20.0 * bend.cos(), 20.0 * bend.sin()],
            [20.0 + 20.0 * bend.cos(), 30.0],
            [0.0, 30.0],
        ],
        10.0,
    );
    let (path, report) = engraved(&[shape], &less);
    assert_eq!(report.corners, 5);
    assert!(passes_through(&path, 20.0, 0.0, 10.0));
}

/// A flat tip goes as deep as its flank touches both walls, and its runs up
/// into each corner stop where the tip would leave the material.
#[test]
fn a_flat_tip_goes_less_deep() {
    let p = EngraveParams {
        tip: 0.5,
        depth: 2.0,
        ..vee()
    };
    let (path, report) = engraved(&[polygon(&L, 10.0)], &p);
    // The stroke's middle: 2 mm from each wall, 1.5 mm deep.
    assert!(passes_through(&path, 18.0, -13.0, 8.5));
    assert!((report.deepest - (4.0 * 2f64.sqrt() / (1.0 + 2f64.sqrt()) - 0.5)).abs() < 1e-6);
    assert!(path.moves().iter().all(|m| m.to.z <= 10.0 + 1e-9));
    assert!(!passes_through(&path, 20.0, -15.0, 10.0));
}

/// Deeper than the flank reaches is refused, rather than cut with the shank.
#[test]
fn deeper_than_the_flank_is_refused() {
    let p = EngraveParams {
        depth: 3.5,
        ..vee()
    };
    let s = shapes(&[(polygon(&RECTANGLE, 10.0), 10.0)], 0.01).unwrap();
    assert!(matches!(engrave(&s, &p), Err(EngraveError::TooDeep { .. })));
}

/// An open run of edges is refused.
#[test]
fn an_open_outline_is_refused() {
    let mut open = polygon(&RECTANGLE, 10.0);
    open.pieces.pop();
    assert!(matches!(
        shapes(&[(open, 10.0)], 0.01),
        Err(EngraveError::Open { .. })
    ));
}

/// With multiple depths, each level is the walk no deeper than it, one pass
/// down through them all.
#[test]
fn multiple_depths_walk_each_level() {
    let p = EngraveParams {
        stepdown: Some(1.0),
        ..vee()
    };
    let (path, report) = engraved(&[polygon(&RECTANGLE, 10.0)], &p);
    assert_eq!(report.levels, 3);
    assert_eq!(report.passes, 1);
    for z in [9.05, 8.1, 7.15] {
        assert!(cuts(&path).iter().any(|m| (m.to.z - z).abs() < 1e-9), "{z}");
    }
}
