use super::*;

fn params() -> FaceParams {
    FaceParams {
        area: (dvec2(0.0, 0.0), dvec2(100.0, 50.0)),
        angle: 0.0,
        stepover: 9.5,
        extension: 2.0,
        radius: 5.0,
        levels: LevelsAsked {
            top: 3.0,
            bottom: 0.0,
            stepdown: None,
        },
        direction: FaceDirection::BothWays,
        clockwise: true,
        feed: 1000.0,
    }
}

fn cuts(path: &Toolpath) -> Vec<DVec3> {
    path.moves()
        .iter()
        .filter(|m| m.role == MoveRole::Cut)
        .map(|m| m.to)
        .collect()
}

/// Passes cover the area edge to edge, evenly no more than the stepover
/// apart, and each runs off it by the tool's radius and the extension, at
/// the bottom alone.
#[test]
fn passes_cover_the_area_and_run_off_it() {
    let (path, report) = face(&params()).unwrap();
    // 50 wide at 9.5 apart: 6 steps, 7 passes.
    assert_eq!(report.rings, 7);
    assert_eq!(report.levels, 1);
    assert_eq!(report.passes, 7);
    let moves = path.moves();
    let ys: Vec<f64> = moves.iter().map(|m| m.to.y).collect();
    assert_eq!(ys.first(), Some(&0.0));
    assert_eq!(ys.last(), Some(&50.0));
    for m in moves {
        assert_eq!(m.to.z, 0.0);
        assert!(m.to.x == -7.0 || m.to.x == 107.0, "{}", m.to);
    }
    // Both ways: one link in, and every step over is cut without lifting.
    assert_eq!(moves[0].role, MoveRole::Link);
    assert!(moves[1..].iter().all(|m| m.role == MoveRole::Cut));
    let gap = 50.0 / 6.0;
    let mut rows: Vec<f64> = cuts(&path).iter().map(|p| p.y).collect();
    rows.dedup();
    for w in rows.windows(2) {
        assert!(((w[1] - w[0]) - gap).abs() < 1e-9, "{w:?}");
    }
}

/// Multiple depths are evenly spaced down to the bottom, each level starting
/// where the one above finished; with none, the bottom alone; with the
/// bottom not below the top, nothing, and said.
#[test]
fn levels_step_down_to_the_bottom() {
    let mut p = params();
    p.levels.stepdown = Some(1.0);
    let (path, report) = face(&p).unwrap();
    assert_eq!(report.levels, 3);
    let moves = path.moves();
    let links: Vec<&DVec3> = moves
        .iter()
        .filter(|m| m.role == MoveRole::Link)
        .map(|m| &m.to)
        .collect();
    assert_eq!(links.len(), 3);
    assert_eq!(links[0].z, 2.0);
    assert_eq!(links[2].z, 0.0);
    // The second level starts on the side the first ended on.
    assert_eq!(links[1].y, 50.0);

    p.levels.bottom = 3.0;
    let (path, report) = face(&p).unwrap();
    assert!(path.moves().is_empty());
    assert_eq!(report.too_shallow, 1);
}

/// Climb and conventional cut every pass the same way, each the opposite of
/// the other, the linker bringing the tool back between; climb with a
/// clockwise spindle keeps the uncut side on the right.
#[test]
fn one_way_passes_all_go_the_same_way() {
    for (direction, clockwise, way) in [
        (FaceDirection::Climb, true, -1.0),
        (FaceDirection::Conventional, true, 1.0),
        (FaceDirection::Climb, false, 1.0),
    ] {
        let p = FaceParams {
            direction,
            clockwise,
            ..params()
        };
        let (path, _) = face(&p).unwrap();
        let moves = path.moves();
        assert_eq!(moves.iter().filter(|m| m.role == MoveRole::Link).count(), 7);
        for pair in moves.chunks(2) {
            assert_eq!(pair[0].role, MoveRole::Link);
            let dx = pair[1].to.x - pair[0].to.x;
            assert_eq!(dx.signum(), way, "{direction:?} {clockwise}");
        }
    }
}

/// At an angle, every pass still runs off the area grown by the tool's
/// radius at both ends, and the passes reach its far corners.
#[test]
fn angled_passes_run_off_the_area() {
    let p = FaceParams {
        angle: 30f64.to_radians(),
        ..params()
    };
    let (path, _) = face(&p).unwrap();
    let (lo, hi) = (dvec2(-5.0, -5.0), dvec2(105.0, 55.0));
    let inside = |q: DVec3| q.x > lo.x && q.x < hi.x && q.y > lo.y && q.y < hi.y;
    for m in path.moves() {
        // Each end of a pass is at least the extension off the grown area.
        assert!(!inside(m.to), "{} is on the stock", m.to);
    }
    let (_, v) = frame(p.angle);
    let across: Vec<f64> = cuts(&path).iter().map(|q| q.truncate().dot(v)).collect();
    let most = across.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let least = across.iter().copied().fold(f64::INFINITY, f64::min);
    let far = [
        dvec2(0.0, 0.0),
        dvec2(100.0, 0.0),
        dvec2(100.0, 50.0),
        dvec2(0.0, 50.0),
    ]
    .map(|c| c.dot(v));
    let (want_lo, want_hi) = (
        far.iter().copied().fold(f64::INFINITY, f64::min),
        far.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    );
    assert!((most - want_hi).abs() < 1e-9 && (least - want_lo).abs() < 1e-9);
}

/// A pass extension below zero, or no area, is refused or faces nothing.
#[test]
fn a_bad_extension_is_refused() {
    let p = FaceParams {
        extension: -1.0,
        ..params()
    };
    assert!(face(&p).is_err());
    let p = FaceParams {
        area: (dvec2(0.0, 0.0), dvec2(0.0, 10.0)),
        ..params()
    };
    assert!(face(&p).unwrap().0.moves().is_empty());
}
