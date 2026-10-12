use super::*;
use glam::dvec2;

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Region {
    Region::rectangle(dvec2(x0, y0), dvec2(x1, y1))
}

/// The same, found by walking round the circle a point at a time.
fn sampled(centre: DVec2, r: f64, uncut: &Region, trail: &[DVec2], swept: f64) -> f64 {
    let n = 200_000;
    let hits = (0..n)
        .filter(|&k| {
            let a = TAU * (f64::from(k) + 0.5) / f64::from(n);
            let p = centre + DVec2::from_angle(a) * r;
            uncut.contains(p) && !near_trail(p, trail, swept)
        })
        .count();
    TAU * hits as f64 / f64::from(n)
}

#[test]
fn wholly_in_material_is_a_full_circle() {
    let e = engaged(DVec2::ZERO, 3.0, &rect(-10.0, -10.0, 10.0, 10.0), &[], 3.0);
    assert!((e.angle() - TAU).abs() < 1e-12);
}

#[test]
fn nothing_to_cut_is_nothing() {
    let e = engaged(DVec2::ZERO, 3.0, &Region::default(), &[], 3.0);
    assert_eq!(e.angle(), 0.0);
    let e = engaged(DVec2::ZERO, 3.0, &rect(20.0, 20.0, 30.0, 30.0), &[], 3.0);
    assert_eq!(e.angle(), 0.0);
}

/// Feeding straight up a slot of its own cutting: a half circle ahead.
#[test]
fn a_full_slot_is_a_half_circle() {
    let uncut = rect(-50.0, -50.0, 50.0, 50.0);
    let trail = [dvec2(0.0, -20.0), dvec2(0.0, 0.0)];
    let e = engaged(DVec2::ZERO, 3.0, &uncut, &trail, 3.0);
    assert!((e.angle() - PI).abs() < 1e-9, "{}", e.angle());
    // Split evenly either side of the way it goes.
    let (right, left) = e.sides(dvec2(0.0, 1.0));
    assert!((right - FRAC_PI_2).abs() < 1e-9 && (left - FRAC_PI_2).abs() < 1e-9);
}

/// A wall `ae` into the tool, alongside a pass already cut: the classic
/// radial engagement.
#[test]
fn a_radial_width_gives_its_angle() {
    let r = 3.0;
    for ae in [0.3, 0.6, 1.5, 3.0, 4.5] {
        // Material to the right of x = r − ae, the tool going up +Y.
        let uncut = rect(r - ae, -50.0, 50.0, 50.0);
        let trail = [dvec2(0.0, -20.0), dvec2(0.0, 0.0)];
        let e = engaged(DVec2::ZERO, r, &uncut, &trail, r);
        let want = (1.0 - ae / r).acos();
        assert!((e.angle() - want).abs() < 1e-9, "ae {ae}: {}", e.angle());
        // Climbing, on the right, up to a quarter; past the tool's middle, the
        // rest is on the left.
        let (right, left) = e.sides(dvec2(0.0, 1.0));
        let climb = want.min(FRAC_PI_2);
        assert!((right - climb).abs() < 1e-9 && (left - (want - climb)).abs() < 1e-9);
    }
}

#[test]
fn a_region_with_a_hole_counts_the_hole_as_air() {
    let outer = Contour::new(vec![
        dvec2(-20.0, -20.0),
        dvec2(20.0, -20.0),
        dvec2(20.0, 20.0),
        dvec2(-20.0, 20.0),
    ])
    .unwrap();
    let hole = Contour::new(vec![
        dvec2(-1.0, -1.0),
        dvec2(-1.0, 1.0),
        dvec2(1.0, 1.0),
        dvec2(1.0, -1.0),
    ])
    .unwrap();
    let uncut = Region::new(vec![outer, hole]);
    // A circle of radius 1 round the hole's corner: a quarter is in the hole.
    let c = dvec2(1.0, 1.0);
    let e = engaged(c, 1.0, &uncut, &[], 1.0);
    assert!((e.angle() - 1.5 * PI).abs() < 1e-9, "{}", e.angle());
}

/// Turning a corner, against a trail of several segments: agrees with
/// walking round the circle.
#[test]
fn agrees_with_sampling_round_a_corner() {
    let uncut = rect(-10.0, -10.0, 30.0, 30.0).difference(&rect(-10.0, -10.0, 2.0, 30.0));
    let uncut = uncut.unwrap();
    let trail = [
        dvec2(0.0, -8.0),
        dvec2(0.0, 0.0),
        dvec2(0.8, 1.2),
        dvec2(2.0, 2.0),
        dvec2(3.1, 2.4),
    ];
    let centre = *trail.last().unwrap();
    for r in [1.0, 2.5, 3.0, 6.0] {
        let e = engaged(centre, r, &uncut, &trail, r);
        let s = sampled(centre, r, &uncut, &trail, r);
        assert!((e.angle() - s).abs() < 1e-3, "r {r}: {} vs {s}", e.angle());
    }
    // And from a point off the trail's end.
    for c in [dvec2(4.0, 4.0), dvec2(-1.0, 3.0), dvec2(2.0, -3.0)] {
        let e = engaged(c, 3.0, &uncut, &trail, 3.0);
        let s = sampled(c, 3.0, &uncut, &trail, 3.0);
        assert!((e.angle() - s).abs() < 1e-3, "{c}: {} vs {s}", e.angle());
    }
}

/// Folding the trail into the region, as clearing does from time to time,
/// changes nothing but the region's own rounding.
#[test]
fn a_trail_folded_in_gives_the_same_angle() {
    let uncut = rect(-40.0, -40.0, 40.0, 40.0);
    let old = [dvec2(-20.0, 0.0), dvec2(0.0, 0.0), dvec2(10.0, 5.0)];
    let r = 3.0;
    let folded = uncut
        .difference(&Region::around_polylines(&[old.to_vec()], r, 1e-4))
        .unwrap();
    let next = [dvec2(10.0, 5.0), dvec2(11.0, 7.2)];
    let centre = dvec2(11.0, 7.2);
    let mut whole = old.to_vec();
    whole.push(centre);
    let a = engaged(centre, r, &uncut, &whole, r).angle();
    let b = engaged(centre, r, &folded, &next, r).angle();
    assert!((a - b).abs() < 1e-2, "{a} vs {b}");
}

#[test]
fn sides_split_an_arc_across_the_heading() {
    // A half circle ahead of a tool going +X, from -90° to +90°.
    let e = Engaged {
        arcs: vec![(-FRAC_PI_2, PI)],
    };
    let (right, left) = e.sides(dvec2(1.0, 0.0));
    assert!((right - FRAC_PI_2).abs() < 1e-12 && (left - FRAC_PI_2).abs() < 1e-12);
    // Going +Y, the same arc is wholly on the right.
    let (right, left) = e.sides(dvec2(0.0, 1.0));
    assert!((right - PI).abs() < 1e-12 && left.abs() < 1e-12);
}
