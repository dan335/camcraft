//! Inside the last smooth ring: rings out from a spine instead of in to a
//! point (decision 0040).
//!
//! Measured in from an edge, a field's rings are offsets of it, and offsets
//! inward of a smooth oval get a corner at each end once they are deeper than
//! the oval's end is round, and keep it to the middle: sharp points the tool
//! turns back at. Going in from the edge down a chain of the field's regions,
//! the last ring with no corner, `R*`, is an egg; inside it a stadium fits.
//! So inside `R*` the rings are measured out from a line along its middle,
//! its spine, and blend into `R*` as rings measured out from an inner edge
//! blend into their far edge ([`blend::between`]).
//!
//! The spine is the middle of `R*` less its depth at each end. Split at its
//! two points furthest apart along the surface, `R*` has two sides; its
//! middle is the line as far from the one as the other along the surface,
//! from end to end. Its depth `D*` is the furthest any node inside is from
//! `R*`: the field's own value less `R*`'s level. A stadium grown from the
//! spine by `D*` reaches the ends of `R*` and its widest place together.
//!
//! [`blend::between`]: super::blend::between

use camcraft_geom::geodesic::Seed;
use camcraft_geom::isoline;
use camcraft_geom::mesh::closest_point_on_segment;
use camcraft_geom::topology::CREASE_DEGREES;
use camcraft_geom::trigrid::TriSurface;
use glam::{DVec2, DVec3};

use super::distance::Field;

/// Whether closed `ring` turns nowhere sharper than the crease angle, seen
/// from above, between chords `step` long either side of each of its points.
pub(super) fn smooth(ring: &[DVec2], step: f64) -> bool {
    let n = ring.len();
    let crease = CREASE_DEGREES.to_radians().cos();
    // The point `step` or more along the ring from point `i`, either way.
    let reach = |i: usize, forward: bool| {
        let mut along = 0.0;
        let mut at = i;
        for _ in 0..n {
            let next = if forward {
                (at + 1) % n
            } else {
                (at + n - 1) % n
            };
            along += ring.get(at)?.distance(*ring.get(next)?);
            at = next;
            if along >= step {
                return ring.get(at).copied();
            }
        }
        None
    };
    n >= 3
        && (0..n).all(|i| {
            let (Some(&p), Some(back), Some(ahead)) =
                (ring.get(i), reach(i, false), reach(i, true))
            else {
                return false;
            };
            match ((p - back).try_normalize(), (ahead - p).try_normalize()) {
                (Some(u), Some(v)) => u.dot(v) >= crease,
                _ => false,
            }
        })
}

/// The spine inside closed `ring`, a line of `field`, `inside` each node's
/// distance from it inside it, less than nought just outside it, for rings `step` apart:
/// lifted onto the ground. `None` where it would be shorter than a stepover
/// or leave the ring.
pub(super) fn spine(
    field: &Field<'_>,
    (ring, inside): (&[DVec2], &[f64]),
    step: f64,
) -> Option<Vec<DVec3>> {
    let view = field.ground.cut.view(None);
    let value = |d: &[f64], n: u32| {
        usize::try_from(n)
            .ok()
            .and_then(|i| d.get(i))
            .copied()
            .filter(|v| v.is_finite())
    };
    let depth = inside
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(0.0, f64::max);
    // The distance along the surface from the points `at` of the ring: from
    // the corners of the triangle under each, each at its own distance.
    let from = |at: &[DVec2]| -> Option<Vec<f64>> {
        let mut seeds: Vec<Seed> = Vec::new();
        for &q in at {
            let p = q.extend(view.height_at(q)?);
            for n in view.triangle(view.locate(q)?)? {
                seeds.push(Seed {
                    node: n,
                    distance: view.point(n)?.distance(p),
                });
            }
        }
        Some(field.measure(&seeds))
    };
    // The point of the ring furthest from `field` along the surface.
    let furthest = |d: &[f64]| {
        (0..ring.len())
            .filter_map(|i| Some((view.interpolate(*ring.get(i)?, |n| value(d, n))?, i)))
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, i)| i)
    };
    // The two points of the ring furthest apart along the surface: the one
    // furthest from any, then the one furthest from that. They split it into
    // its two sides.
    let one = furthest(&from(ring.get(..1)?)?)?;
    let other = furthest(&from(ring.get(one..=one)?)?)?;
    let (lo, hi) = (one.min(other), one.max(other));
    let side: Vec<DVec2> = ring.get(lo..=hi)?.to_vec();
    let rest: Vec<DVec2> = ring
        .get(hi..)?
        .iter()
        .chain(ring.get(..=lo)?)
        .copied()
        .collect();
    // The middle: as far from the one side as the other, along the surface.
    let (near, far) = (from(&side)?, from(&rest)?);
    let apart: Vec<f64> = near.iter().zip(&far).map(|(a, b)| a - b).collect();
    let triangles: Vec<u32> = view
        .triangles()
        .filter(|(_, corners)| {
            corners
                .iter()
                .all(|&n| value(inside, n).is_some_and(|v| v >= 0.0))
        })
        .map(|(t, _)| t)
        .collect();
    let mut middle = isoline::lines(&view, &apart, 0.0, triangles);
    let (Some(line), true) = (middle.pop(), middle.is_empty()) else {
        return None;
    };
    let lifted: Vec<DVec3> = line
        .points
        .iter()
        .map(|&p| Some(p.extend(view.height_at(p)?)))
        .collect::<Option<_>>()?;
    let spine = trimmed(&lifted, depth)?;
    let within = spine.iter().all(|p| {
        view.interpolate(p.truncate(), |n| value(inside, n))
            .is_some_and(|v| v > 0.0)
    });
    (within && length(&spine) >= step).then_some(spine)
}

/// The nodes of `field`'s grid beside `spine`, within a cell's diagonal of it
/// seen from above, of those `keep` says, each at its distance from it: where
/// the field measured out from it starts.
pub(super) fn seeds(field: &Field<'_>, spine: &[DVec3], keep: impl Fn(u32) -> bool) -> Vec<Seed> {
    let grid = field.ground.cut.grid();
    let pitch = grid.pitch();
    let reach = pitch * std::f64::consts::SQRT_2;
    let Some(origin) = grid.node(0, 0).and_then(|n| grid.xy(n)) else {
        return Vec::new();
    };
    let index = |v: f64, most: u32| -> u32 {
        // Clamped onto the grid, so the cast holds.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let i = (v / pitch).floor().clamp(0.0, f64::from(most)) as u32;
        i
    };
    let (nx, ny) = (grid.nx().saturating_sub(1), grid.ny().saturating_sub(1));
    let mut near: Vec<u32> = Vec::new();
    for pair in spine.windows(2) {
        let [a, b] = pair else { continue };
        let lo = a.truncate().min(b.truncate()) - DVec2::splat(reach) - origin;
        let hi = a.truncate().max(b.truncate()) + DVec2::splat(reach) - origin;
        for iy in index(lo.y, ny)..=index(hi.y, ny).saturating_add(1).min(ny) {
            for ix in index(lo.x, nx)..=index(hi.x, nx).saturating_add(1).min(nx) {
                if let Some(n) = grid.node(ix, iy) {
                    near.push(n);
                }
            }
        }
    }
    near.sort_unstable();
    near.dedup();
    let view = field.ground.cut.view(None);
    let flat = |q: DVec3| q.truncate().extend(0.0);
    near.into_iter()
        .filter(|&n| keep(n))
        .filter_map(|n| {
            let p = view.point(n)?;
            let mut beside = false;
            let mut distance = f64::INFINITY;
            for pair in spine.windows(2) {
                let [a, b] = pair else { continue };
                let seen = flat(p).distance(closest_point_on_segment(flat(p), flat(*a), flat(*b)));
                beside |= seen <= reach;
                distance = distance.min(p.distance(closest_point_on_segment(p, *a, *b)));
            }
            beside.then_some(Seed { node: n, distance })
        })
        .collect()
}

/// `way` with `trim` of its length along the surface left off each end;
/// `None` where nothing is left.
fn trimmed(way: &[DVec3], trim: f64) -> Option<Vec<DVec3>> {
    let total = length(way);
    if total <= 2.0 * trim {
        return None;
    }
    let (from, to) = (trim, total - trim);
    let mut out = Vec::new();
    let mut along = 0.0;
    for pair in way.windows(2) {
        let [a, b] = pair else { continue };
        let piece = a.distance(*b);
        let (start, end) = (along, along + piece);
        if end > from && start < to && piece > 0.0 {
            if out.is_empty() {
                out.push(a.lerp(*b, ((from - start) / piece).max(0.0)));
            }
            if end >= to {
                out.push(a.lerp(*b, (to - start) / piece));
                break;
            }
            out.push(*b);
        }
        along = end;
    }
    (out.len() >= 2).then_some(out)
}

/// The length of a polyline.
pub(super) fn length(points: &[DVec3]) -> f64 {
    points
        .windows(2)
        .map(|w| match w {
            [a, b] => a.distance(*b),
            _ => 0.0,
        })
        .sum()
}
