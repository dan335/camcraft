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

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use glam::DVec2;

use crate::contour::{Contour, Region};
pub use crate::mesh::segment_distance2;

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
            let mut cuts = [a, b, 0.0, PI, TAU];
            cuts.sort_by(f64::total_cmp);
            let mut from = None;
            for y in cuts.into_iter().filter(|&c| c >= a && c <= b) {
                if let Some(x) = from.replace(y) {
                    let mid = wrap((x + y) * 0.5);
                    if mid < 0.0 {
                        right += y - x;
                    } else {
                        left += y - x;
                    }
                }
            }
        }
        (right, left)
    }
}

/// An angle in (-π, π].
fn wrap(a: f64) -> f64 {
    let w = a.rem_euclid(TAU);
    if w > PI { w - TAU } else { w }
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
    for (a, b) in uncut.rings().iter().flat_map(Contour::edges) {
        circle_segment(centre, radius, a, b, &mut cuts);
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
fn near_trail(p: DVec2, trail: &[DVec2], swept: f64) -> bool {
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

/// Angles at which the circle crosses the segment `a`–`b`.
fn circle_segment(c: DVec2, r: f64, a: DVec2, b: DVec2, out: &mut Vec<f64>) {
    // A segment wholly beyond the circle, or wholly inside it, cannot cross it.
    if segment_distance2(c, a, b) > r * r || a.distance(c).max(b.distance(c)) < r {
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
mod tests;
