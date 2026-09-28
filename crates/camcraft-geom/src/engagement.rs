//! How much of a cutter's edge is in material: the measure adaptive clearing
//! steers by.
//!
//! A cutter at a level cuts wherever its edge meets material, and its load is the
//! arc of that edge in material, the **engagement angle**. A tool feeding straight
//! along a wall `ae` away from the last pass is engaged over `acos(1 − ae/R)`; one
//! in a full slot, over a half circle. Holding that angle is what keeps the chip
//! load, and so the cutter's life, constant.
//!
//! # Exactly, from the regions
//!
//! Material is what is left of a region, [`Region`], less everything within a
//! radius of the path the tool has just followed, its trail. The trail is kept
//! apart from the region until it is worth folding in, because the region is
//! polygons that Clipper rebuilds, and the trail is the last few steps, which
//! change every step.
//!
//! The circle is cut wherever it crosses the region's edges or the boundary of
//! the trail's sweep, and each arc between is in material or not as a point in
//! its middle is. So the angle is exact to the region, with no sampling.

use std::f64::consts::{FRAC_PI_2, TAU};

use glam::DVec2;

use crate::contour::Region;

/// Arcs of a circle in material, as `(start, sweep)` angles, counter-clockwise
/// from +X, in radians.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Engaged {
    arcs: Vec<(f64, f64)>,
}

impl Engaged {
    /// The arcs, each its start angle and its counter-clockwise sweep.
    pub fn arcs(&self) -> &[(f64, f64)] {
        &self.arcs
    }

    /// The whole angle in material.
    pub fn angle(&self) -> f64 {
        self.arcs.iter().map(|a| a.1).sum()
    }

    /// The angle in material to the right of `heading`, the direction the tool
    /// travels, and to its left.
    ///
    /// A tool turning clockwise, as nearly every spindle does, climbs through what
    /// is on its right and cuts conventionally through what is on its left.
    pub fn sides(&self, heading: DVec2) -> (f64, f64) {
        let h = heading.to_angle();
        let mut right = 0.0;
        let mut left = 0.0;
        for &(start, sweep) in &self.arcs {
            // Relative to the heading, in (-π, π]: the right side is negative.
            let a = wrap(start - h);
            let b = a + sweep;
            // Split at 0 and at π (and 2π, for an arc that wraps past it).
            let cuts = [a, b, 0.0, std::f64::consts::PI, TAU];
            let mut ends: Vec<f64> = cuts.into_iter().filter(|&c| c >= a && c <= b).collect();
            ends.sort_by(f64::total_cmp);
            for w in ends.windows(2) {
                let (Some(&x), Some(&y)) = (w.first(), w.get(1)) else {
                    continue;
                };
                let mid = wrap((x + y) * 0.5);
                if mid < 0.0 {
                    right += y - x;
                } else {
                    left += y - x;
                }
            }
        }
        (right, left)
    }
}

/// An angle in (-π, π].
fn wrap(a: f64) -> f64 {
    let w = a.rem_euclid(TAU);
    if w > std::f64::consts::PI { w - TAU } else { w }
}

/// The arcs of the circle about `centre` of `radius` that lie in material: in
/// `uncut`, and further than `swept` from every point of `trail`, the polyline the
/// tool has followed since `uncut` was last brought up to date.
///
/// The trail may end at `centre`, as a tool's does: the circle is then the
/// leading edge of the trail's own sweep, and what lies ahead on it counts as in
/// material if `uncut` has it.
pub fn engaged(centre: DVec2, radius: f64, uncut: &Region, trail: &[DVec2], swept: f64) -> Engaged {
    if !(radius.is_finite() && radius > 0.0) || !centre.is_finite() {
        return Engaged::default();
    }
    // Far enough from the centre that the circle cannot reach: tested first,
    // since most of the region's edges are.
    let mut cuts: Vec<f64> = Vec::new();
    for ring in uncut.rings() {
        let pts = ring.points();
        let n = pts.len();
        for i in 0..n {
            let (Some(&a), Some(&b)) = (pts.get(i), pts.get((i + 1) % n)) else {
                continue;
            };
            circle_segment(centre, radius, a, b, &mut cuts);
        }
    }
    let at_centre = |p: DVec2| p.distance_squared(centre) <= (radius * 1e-9).powi(2);
    for (i, &p) in trail.iter().enumerate() {
        if p.distance(centre) > radius + swept {
            continue;
        }
        if at_centre(p) {
            // The circle is this disc's own edge: the trail's sweep meets it
            // where the segments into it run beside it, square to them.
            for q in [
                i.checked_sub(1).and_then(|k| trail.get(k)),
                trail.get(i + 1),
            ]
            .into_iter()
            .flatten()
            {
                if let Some(d) = (*q - p).try_normalize() {
                    cuts.push(d.to_angle() + FRAC_PI_2);
                    cuts.push(d.to_angle() - FRAC_PI_2);
                }
            }
        } else {
            circle_circle(centre, radius, p, swept, &mut cuts);
        }
        if let Some(&q) = trail.get(i + 1)
            && let Some(d) = (q - p).try_normalize()
        {
            // The sides of this segment's sweep.
            let side = d.perp() * swept;
            for off in [side, -side] {
                circle_line_within(centre, radius, p + off, q + off, &mut cuts);
            }
        }
    }

    let in_material = |angle: f64| {
        let p = centre + DVec2::from_angle(angle) * radius;
        uncut.contains(p) && !near_trail(p, trail, swept)
    };
    if cuts.is_empty() {
        return if in_material(0.0) {
            Engaged {
                arcs: vec![(0.0, TAU)],
            }
        } else {
            Engaged::default()
        };
    }
    for c in &mut cuts {
        *c = c.rem_euclid(TAU);
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-12);

    let mut arcs: Vec<(f64, f64)> = Vec::new();
    let n = cuts.len();
    for i in 0..n {
        let (Some(&a), Some(&b)) = (cuts.get(i), cuts.get((i + 1) % n)) else {
            continue;
        };
        let sweep = if i + 1 == n { b + TAU - a } else { b - a };
        if sweep <= 0.0 {
            continue;
        }
        if in_material(a + sweep * 0.5) {
            // Joined to the arc before, where they meet.
            match arcs.last_mut() {
                Some(last) if (last.0 + last.1 - a).abs() < 1e-12 => last.1 += sweep,
                _ => arcs.push((a, sweep)),
            }
        }
    }
    // An arc running through +X is one arc, not two.
    if arcs.len() > 1
        && let (Some(&first), Some(&last)) = (arcs.first(), arcs.last())
        && first.0.abs() < 1e-12
        && (last.0 + last.1 - TAU).abs() < 1e-12
    {
        arcs.pop();
        if let Some(f) = arcs.first_mut() {
            *f = (last.0, last.1 + first.1);
        }
    }
    Engaged { arcs }
}

/// Whether `p` is strictly nearer than `swept` to the polyline `trail`: inside
/// what the tool has swept, not on its edge.
pub fn near_trail(p: DVec2, trail: &[DVec2], swept: f64) -> bool {
    // Its edge, to a hair, counts as outside: the circle ahead of a tool is the
    // edge of what it has swept, and is where it cuts.
    let limit = swept * (1.0 - 1e-9);
    let limit2 = limit * limit;
    if trail.len() == 1 {
        return trail
            .first()
            .is_some_and(|q| q.distance_squared(p) < limit2);
    }
    trail
        .windows(2)
        .any(|w| matches!(w, [a, b] if segment_distance2(p, *a, *b) < limit2))
}

/// The square of the distance from `p` to the segment `a`–`b`.
pub fn segment_distance2(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 > 0.0 {
        ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + ab * t).distance_squared(p)
}

/// Angles at which the circle crosses the segment `a`–`b`.
fn circle_segment(c: DVec2, r: f64, a: DVec2, b: DVec2, out: &mut Vec<f64>) {
    // A segment wholly inside the circle, or wholly beyond it, cannot cross it.
    let far = a.distance(c).max(b.distance(c));
    if far < r || segment_distance2(c, a, b) > r * r {
        return;
    }
    line_hits(c, r, a, b, out);
}

/// Angles at which the circle crosses the segment `a`–`b`, from the line's
/// intersections, kept where they fall within it.
fn line_hits(c: DVec2, r: f64, a: DVec2, b: DVec2, out: &mut Vec<f64>) {
    let d = b - a;
    let f = a - c;
    let qa = d.length_squared();
    if qa <= 0.0 {
        return;
    }
    let qb = 2.0 * f.dot(d);
    let qc = f.length_squared() - r * r;
    let disc = qb * qb - 4.0 * qa * qc;
    if disc < 0.0 {
        return;
    }
    let root = disc.sqrt();
    for t in [(-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa)] {
        if (-1e-12..=1.0 + 1e-12).contains(&t) {
            out.push((a + d * t - c).to_angle());
        }
    }
}

/// Angles at which the circle crosses the segment `a`–`b`, which may lie wholly
/// outside or inside it.
fn circle_line_within(c: DVec2, r: f64, a: DVec2, b: DVec2, out: &mut Vec<f64>) {
    if segment_distance2(c, a, b) > r * r {
        return;
    }
    line_hits(c, r, a, b, out);
}

/// Angles on the first circle at which it crosses the second.
fn circle_circle(c0: DVec2, r0: f64, c1: DVec2, r1: f64, out: &mut Vec<f64>) {
    let d = c1 - c0;
    let dist = d.length();
    if dist <= 0.0 || dist > r0 + r1 || dist < (r0 - r1).abs() {
        return;
    }
    // Along the line between the centres, and either side of it.
    let along = (dist * dist + r0 * r0 - r1 * r1) / (2.0 * dist);
    let cos = (along / r0).clamp(-1.0, 1.0);
    let spread = cos.acos();
    let base = d.to_angle();
    out.push(base + spread);
    out.push(base - spread);
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)]
mod tests {
    use super::*;
    use crate::contour::Contour;
    use glam::dvec2;
    use std::f64::consts::PI;

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
}
