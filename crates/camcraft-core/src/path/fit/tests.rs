use super::*;
use glam::dvec3;

/// A path from a rapid to the first point, then cuts through the rest.
fn cut_through(points: &[DVec3], feed: f64) -> Toolpath {
    let mut p = Toolpath::new();
    p.rapid_to(points[0]).unwrap();
    for &q in &points[1..] {
        p.feed_to(q, MoveRole::Cut, feed).unwrap();
    }
    p
}

/// Points every `step` radians round a circle of `r` about `c`, level at `z`,
/// from `from` to `to` radians.
fn round(c: DVec2, r: f64, z: f64, from: f64, to: f64, n: usize) -> Vec<DVec3> {
    (0..=n)
        .map(|k| {
            let a = from + (to - from) * k as f64 / n as f64;
            (c + DVec2::from_angle(a) * r).extend(z)
        })
        .collect()
}

/// Points along `path` within `tolerance` of each other, from its start.
fn sampled(path: &Toolpath) -> Vec<DVec3> {
    let mut at = path.moves()[0].to;
    let mut out = vec![at];
    for m in &path.moves()[1..] {
        let n = (m.length(at) / 0.01).ceil().max(1.0) as usize;
        for k in 1..=n {
            out.push(m.point_at(at, k as f64 / n as f64));
        }
        at = m.to;
    }
    out
}

/// How far the furthest point of `a` is from `b`, each sampled finely.
fn strays(a: &Toolpath, b: &Toolpath) -> f64 {
    let (pa, pb) = (sampled(a), sampled(b));
    let near = |p: DVec3, q: &[DVec3]| {
        q.windows(2)
            .map(|w| {
                let d = w[1] - w[0];
                let t = ((p - w[0]).dot(d) / d.length_squared().max(1e-18)).clamp(0.0, 1.0);
                p.distance(w[0] + d * t)
            })
            .fold(f64::INFINITY, f64::min)
    };
    let one = pa.iter().map(|&p| near(p, &pb)).fold(0.0, f64::max);
    let other = pb.iter().map(|&p| near(p, &pa)).fold(0.0, f64::max);
    one.max(other)
}

/// A level circle's chords, a quarter turn at a time, come out as arcs, each
/// at most a half turn, ending where the chords did; within the tolerance.
#[test]
fn chords_round_a_circle_become_arcs() {
    let points = round(DVec2::new(5.0, 5.0), 10.0, -3.0, 0.0, 6.0, 300);
    let path = cut_through(&points, 1000.0);
    let (fitted, report) = fit_arcs(&path, 0.01).unwrap();
    assert!(report.arcs >= 2 && report.after <= 4, "{report:?}");
    assert_eq!(report.before, 301);
    assert_eq!(fitted.end(), path.end());
    for m in fitted.moves() {
        if let MoveKind::Arc(a) = m.kind {
            assert!(a.sweep.abs() <= PI + 1e-9);
            assert_eq!(a.axis, DVec3::Z);
            assert!(a.sweep > 0.0, "counter-clockwise, as the points go");
        }
    }
    assert!(strays(&path, &fitted) < 0.01 + 1e-6);
}

/// Points along a line become one move; a zigzag wider than the tolerance
/// stays as it is.
#[test]
fn a_line_is_one_move_and_a_zigzag_is_kept() {
    let line: Vec<DVec3> = (0..=100).map(|k| dvec3(k as f64 * 0.1, 2.0, 1.0)).collect();
    let (fitted, report) = fit_arcs(&cut_through(&line, 900.0), 0.01).unwrap();
    assert_eq!(report.after, 2, "{report:?}");
    assert_eq!(fitted.end(), Some(dvec3(10.0, 2.0, 1.0)));

    let zigzag: Vec<DVec3> = (0..=40)
        .map(|k| dvec3(k as f64, if k % 2 == 0 { 0.0 } else { 1.0 }, 0.0))
        .collect();
    let path = cut_through(&zigzag, 900.0);
    let (fitted, report) = fit_arcs(&path, 0.01).unwrap();
    assert_eq!(fitted, path, "{report:?}");
}

/// A run is a role and a feed: where either changes, or a move is a rapid,
/// the path keeps a point there, and moves that cannot be arcs stay straight.
#[test]
fn runs_end_where_the_role_or_feed_changes() {
    let points = round(DVec2::ZERO, 8.0, 0.0, 0.0, 3.0, 120);
    let mut path = Toolpath::new();
    path.rapid_to(points[0]).unwrap();
    for (k, &q) in points[1..].iter().enumerate() {
        let feed = if k < 60 { 1000.0 } else { 800.0 };
        path.feed_to(q, MoveRole::Cut, feed).unwrap();
    }
    path.feed_to(points[120] + DVec3::Z * 5.0, MoveRole::Retract, 500.0)
        .unwrap();
    let (fitted, _) = fit_arcs(&path, 0.01).unwrap();
    // The point where the feed changed is still one.
    assert!(fitted.moves().iter().any(|m| m.to == points[60]));
    let last = fitted.moves().last().unwrap();
    assert_eq!(last.role, MoveRole::Retract);
    assert_eq!(last.kind, MoveKind::Linear);
    assert!(strays(&path, &fitted) < 0.01 + 1e-6);
}

/// A pass over the part along X, in a plane square to Y, is fitted with arcs
/// in that plane, as G18 cuts them; one along neither axis gets lines only.
#[test]
fn arcs_are_fitted_square_to_x_or_y_too() {
    // A bump, z = sqrt(r^2 - x^2), seen side on along X.
    let bump: Vec<DVec3> = (0..=200)
        .map(|k| {
            let x = -9.0 + 18.0 * k as f64 / 200.0;
            dvec3(x, 3.0, (100.0 - x * x).sqrt())
        })
        .collect();
    let path = cut_through(&bump, 1000.0);
    let (fitted, report) = fit_arcs(&path, 0.01).unwrap();
    assert!(report.arcs >= 1, "{report:?}");
    assert!(fitted.moves().iter().all(|m| match m.kind {
        MoveKind::Arc(a) => a.axis == DVec3::Y,
        MoveKind::Linear => true,
    }));
    assert!(strays(&path, &fitted) < 0.01 + 1e-6);

    // The same, at 30 degrees to X: no plane a machine has.
    let (s, c) = 30f64.to_radians().sin_cos();
    let turned: Vec<DVec3> = bump.iter().map(|p| dvec3(p.x * c, p.x * s, p.z)).collect();
    let (_, report) = fit_arcs(&cut_through(&turned, 1000.0), 0.01).unwrap();
    assert_eq!(report.arcs, 0, "{report:?}");
}

/// Adaptive's passes curve by a little at every step, and not on one circle:
/// fitted, they are fewer moves, and stray no more than the tolerance.
#[test]
fn a_wandering_pass_is_fewer_moves_within_the_tolerance() {
    let mut points = Vec::new();
    let mut at = DVec2::ZERO;
    let mut heading: f64 = 0.0;
    for k in 0..600 {
        heading += 0.02 * (k as f64 * 0.03).sin();
        at += DVec2::from_angle(heading) * 0.3;
        points.push(at.extend(-6.0));
    }
    let path = cut_through(&points, 3000.0);
    for tolerance in [0.005, 0.02, 0.05] {
        let (fitted, report) = fit_arcs(&path, tolerance).unwrap();
        assert!(report.after * 4 < report.before, "{tolerance}: {report:?}");
        let off = strays(&path, &fitted);
        assert!(off < tolerance + 1e-6, "{tolerance}: strayed {off}");
    }
}
