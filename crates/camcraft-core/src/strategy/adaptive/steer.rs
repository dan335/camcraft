//! Each step of a pass: which way to turn to hold the load, and what is
//! clear to go.

use super::tiles::Window;
use super::*;

/// Whether every point of the segment `a`–`b` is further than `distance` from
/// `region`, and outside it.
pub(super) fn clear_of(region: &Region, a: DVec2, b: DVec2, distance: f64) -> bool {
    use camcraft_geom::engagement::segment_distance2;
    if region.contains(a) || region.contains(b) {
        return false;
    }
    let d2 = distance * distance;
    let cross = |o: DVec2, p: DVec2, q: DVec2| (p - o).perp_dot(q - o);
    // An edge whose box, grown by the distance and a little more, misses the
    // segment's is too far to cross or come near it.
    let reach = DVec2::splat(distance + 1e-6);
    let (lo, hi) = (a.min(b) - reach, a.max(b) + reach);
    for ring in region.rings() {
        let pts = ring.points();
        let n = pts.len();
        for i in 0..n {
            let (Some(&c), Some(&d)) = (pts.get(i), pts.get((i + 1) % n)) else {
                continue;
            };
            if c.max(d).cmplt(lo).any() || c.min(d).cmpgt(hi).any() {
                continue;
            }
            // Crossing, or nearer than the distance at an end of either.
            if (cross(a, b, c) > 0.0) != (cross(a, b, d) > 0.0)
                && (cross(c, d, a) > 0.0) != (cross(c, d, b) > 0.0)
            {
                return false;
            }
            if segment_distance2(c, a, b) < d2
                || segment_distance2(d, a, b) < d2
                || segment_distance2(a, c, d) < d2
                || segment_distance2(b, c, d) < d2
            {
                return false;
            }
        }
    }
    true
}

/// A step decided on.
#[derive(Debug, Clone, Copy)]
pub(super) struct Step {
    pub(super) at: DVec2,
    pub(super) heading: DVec2,
    pub(super) turn: f64,
    pub(super) engagement: f64,
}

/// The step from `at`, heading along `heading`, `length` long, turned toward or
/// away from the material on `side` until its engagement is as asked; `None`
/// where every way would overload the tool or leave where it may be. What the
/// tool has swept since the material was brought up to date is `trail`, and
/// `path` is room to try each step after it in.
#[allow(clippy::too_many_arguments)]
pub(super) fn choose(
    w: &Window,
    (trail, path): (&[DVec2], &mut Vec<DVec2>),
    at: DVec2,
    heading: DVec2,
    side: Side,
    length: f64,
    lenient: bool,
    p: &LevelParams,
) -> Option<Step> {
    path.clear();
    path.extend_from_slice(trail);
    path.push(at);
    // Turned `t` toward the material: the step, and its engagement, or `None`
    // where it would leave where the tool may be, or cut mostly the wrong way.
    let mut eval = |t: f64| -> Option<Step> {
        let dir = DVec2::from_angle(side.toward() * t).rotate(heading);
        let next = at + dir * length;
        if !w.allowed.contains_segment(at, next) {
            return None;
        }
        path.push(next);
        let e = engaged(next, p.radius, &w.uncut, path.as_slice(), p.radius);
        path.pop();
        let total = e.angle();
        let (right, left) = e.sides(dir);
        let (with, against) = match side {
            Side::Right => (right, left),
            Side::Left => (left, right),
        };
        // Cutting mostly the wrong way is no way to go on, but a full slot's
        // halves are even.
        if against > with + 0.1 && against > p.air && !lenient {
            return None;
        }
        Some(Step {
            at: next,
            heading: dir,
            turn: t,
            engagement: total,
        })
    };
    let load = |s: &Option<Step>| s.as_ref().map_or(f64::INFINITY, |s| s.engagement);
    let target = p.target;
    let straight = eval(0.0);
    let g0 = load(&straight);
    if (g0 - target).abs() <= p.within {
        return straight;
    }
    // The bracket: `lo` under the target, `hi` over it.
    let (lo, hi) = if g0 < target {
        let most = eval(MAX_TURN);
        if load(&most) <= target + p.within {
            // Not enough even turned hard in: take what there is.
            return most;
        }
        ((0.0, straight), (MAX_TURN, most))
    } else {
        let away = eval(-MAX_TURN);
        let ga = load(&away);
        if ga >= target - p.within {
            // Too much even turned hard away: further, back the way it came if
            // need be, to anything the tool can take.
            if ga <= p.limit || (lenient && ga.is_finite()) {
                return away;
            }
            for t in [-1.5 * MAX_TURN, -2.0 * MAX_TURN, -3.0 * MAX_TURN] {
                let s = eval(t);
                let g = load(&s);
                if g < target - p.within {
                    return bisect((t, s), (-MAX_TURN, away), &mut eval, target, p);
                }
                if g <= p.limit || (lenient && g.is_finite()) {
                    return s;
                }
            }
            return None;
        }
        ((-MAX_TURN, away), (0.0, straight))
    };
    lo.1.as_ref()?;
    bisect(lo, hi, &mut eval, target, p)
}

/// Refine between `lo`, under the target, and `hi`, over it (or not a way at
/// all), to a step whose engagement is within reach of the target: the last one
/// found under it if none is.
pub(super) fn bisect(
    lo: (f64, Option<Step>),
    hi: (f64, Option<Step>),
    eval: &mut impl FnMut(f64) -> Option<Step>,
    target: f64,
    p: &LevelParams,
) -> Option<Step> {
    let load = |s: &Option<Step>| s.as_ref().map_or(f64::INFINITY, |s| s.engagement);
    let (mut lo, mut hi) = (lo, hi);
    for _ in 0..SEARCH_ITERATIONS {
        let (gl, gh) = (load(&lo.1), load(&hi.1));
        // Regula falsi where both ends are ways, kept off the ends; halving
        // where one is not.
        let f = if gh.is_finite() && gh > gl {
            ((target - gl) / (gh - gl)).clamp(0.2, 0.8)
        } else {
            0.5
        };
        let t = lo.0 + (hi.0 - lo.0) * f;
        let s = eval(t);
        let g = load(&s);
        if (g - target).abs() <= p.within {
            return s;
        }
        if g < target {
            lo = (t, s);
        } else {
            hi = (t, s);
        }
    }
    lo.1
}

/// Somewhere deep inside `region`: the middle of what is left of it shrunk as
/// far as it can be. `None` for an empty region.
pub(super) fn deepest(region: &Region, arc: f64) -> Option<DVec2> {
    let (lo, hi) = region.bounds()?;
    let best = innermost(region, (0.0, (hi - lo).length() * 0.5 + arc), arc, arc)
        .map_or_else(|| region.clone(), |(shrunk, _)| shrunk);
    // The middle of the largest ring left, if it is inside; else a corner of it.
    let ring = best
        .rings()
        .iter()
        .max_by(|a, b| a.signed_area().total_cmp(&b.signed_area()))?;
    let pts = ring.points();
    #[allow(clippy::cast_precision_loss)]
    let middle = pts.iter().copied().sum::<DVec2>() / pts.len() as f64;
    if best.contains(middle) {
        Some(middle)
    } else {
        pts.first().copied()
    }
}
