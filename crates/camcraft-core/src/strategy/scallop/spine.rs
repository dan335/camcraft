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
//! Where there is no `R*`, the spine would be shorter than a stepover or leave
//! `R*`, its middle comes out in pieces, or the band into `R*` is refused, the
//! field's rings are left as they are.
//!
//! [`blend::between`]: super::blend::between

use camcraft_geom::cut::View;
use camcraft_geom::geodesic::Seed;
use camcraft_geom::isoline::{self, IsoLine};
use camcraft_geom::mesh::closest_point_on_segment;
use camcraft_geom::topology::CREASE_DEGREES;
use camcraft_geom::trigrid::TriSurface;
use glam::{DVec2, DVec3};

use crate::strategy::slope::Sloped;

use super::blend;
use super::distance::{Field, at, only_line};
use super::rings::Plan;
use super::{length, turns_left};

/// What `R*` holds laid out from its spine: the spine, the rings out from it
/// in the order they are cut, and how many of those blend into `R*`.
pub(super) struct Inside {
    pub(super) spine: Vec<DVec3>,
    pub(super) rings: Vec<IsoLine>,
    pub(super) blended: usize,
}

/// Going in down each chain of `regions`, `plans` holding their lines, the
/// region of the last ring with no corner where the next in has one
/// ([`smooth`]), rings `step` apart: `R*`, where its ring is one closed ring
/// with nothing inside it but the regions further in. None where the
/// outermost ring of a chain already has a corner.
pub(super) fn last_smooth(regions: &[isoline::Region], plans: &[Plan], step: f64) -> Vec<usize> {
    let smooth = |i: usize| ring_of(plans, i).filter(|l| smooth(&l.points, step));
    let mut stars = Vec::new();
    let mut stack: Vec<usize> = (0..regions.len())
        .filter(|&i| regions.get(i).is_some_and(|r| r.level == 1))
        .collect();
    while let Some(i) = stack.pop() {
        let Some(region) = regions.get(i) else {
            continue;
        };
        if smooth(i).is_some() {
            stack.extend(region.children.iter().copied());
            continue;
        }
        if ring_of(plans, i).is_some()
            && let Some(parent) = region.parent
            && regions.get(parent).is_some_and(|p| p.level >= 1)
            && smooth(parent).is_some()
            && !stars.contains(&parent)
        {
            stars.push(parent);
        }
    }
    stars.sort_unstable();
    stars
}

/// The line of `plans`' region `i`, where it is one closed ring with the
/// region inside it: no hole, no arc, no edge.
pub(super) fn ring_of(plans: &[Plan], i: usize) -> Option<&Sloped> {
    match plans.get(i)?.lines.as_slice() {
        [line] if line.closed && turns_left(&line.points) => Some(line),
        _ => None,
    }
}

/// Inside `ring`, the ring of `R*`, the field's `region` of it over `grid`,
/// what it holds laid out from its spine, rings `step` apart for a tool
/// `diameter` across. `None` where there is no spine a stepover long inside
/// it, or the band into it is refused.
pub(super) fn inside(
    grid: &View<'_>,
    field: &Field<'_>,
    (region, ring): (&isoline::Region, &[DVec2]),
    (step, diameter): (f64, f64),
) -> Option<Inside> {
    let depth = depth_inside(grid, &field.d, region.rep, f64::from(region.level) * step);
    let within = |n: u32| at(&depth, n).is_some_and(|v| v >= 0.0);
    let line = spine(field, (ring, &depth), step)?;
    let seeds = seeds(field, &line, within);
    if seeds.is_empty() {
        return None;
    }
    let from_spine = field.measure(&seeds);
    let (level, band) = band(grid, field, ring, (&from_spine, &depth), (step, diameter))?;
    // The whole rings out from the spine, each one closed line inside `R*`.
    let mut triangles: Vec<u32> = (0..grid.nodes())
        .filter(|&n| within(n))
        .flat_map(|n| grid.triangles_at(n).map(|(t, _)| t).collect::<Vec<_>>())
        .collect();
    triangles.sort_unstable();
    triangles.dedup();
    let mut rings = Vec::new();
    for k in 1..=level {
        let level = f64::from(k) * step;
        rings.push(
            only_line(grid, &from_spine, level, triangles.iter().copied()).filter(|r| r.closed)?,
        );
    }
    let blended = band.rings.len();
    rings.extend(band.rings);
    Some(Inside {
        spine: line,
        rings,
        blended,
    })
}

/// Each node's distance from the line of `d` at `base`, inside it, round the
/// node `rep`: `d` less `base` on the nodes joined to `rep` at or past that
/// level, and on those beside them outside it, less than nought; nothing
/// elsewhere.
fn depth_inside(grid: &View<'_>, d: &[f64], rep: u32, base: f64) -> Vec<f64> {
    let mut depth = vec![f64::INFINITY; d.len()];
    let mut stack = vec![rep];
    while let Some(n) = stack.pop() {
        let Some(v) = at(d, n) else {
            continue;
        };
        match usize::try_from(n).ok().and_then(|i| depth.get_mut(i)) {
            Some(slot) if !slot.is_finite() => *slot = v - base,
            _ => continue,
        }
        if v >= base {
            for (_, corners) in grid.triangles_at(n) {
                stack.extend(corners);
            }
        }
    }
    depth
}

/// The level of the last whole ring out from the spine, `from_spine` each
/// node's distance from it, and the rings blending from it into `ring`,
/// `depth` each node's distance from that inside it, rings `step` apart for a
/// tool `diameter` across ([`blend::start`], [`blend::between`]).
fn band(
    grid: &View<'_>,
    field: &Field<'_>,
    ring: &[DVec2],
    (from_spine, depth): (&[f64], &[f64]),
    (step, diameter): (f64, f64),
) -> Option<(u32, blend::Band)> {
    let at_ring: Vec<f64> = ring
        .iter()
        .filter_map(|&p| grid.interpolate(p, |n| at(from_spine, n)))
        .collect();
    let nearest = at_ring.iter().copied().reduce(f64::min)?;
    let furthest = at_ring.iter().copied().reduce(f64::max)?;
    let level = blend::start(nearest, furthest, step, diameter)?;
    let base = f64::from(level) * step;
    let on: Vec<bool> = (0u32..)
        .zip(from_spine)
        .map(|(n, &v)| at(depth, n).is_some_and(|v| v >= 0.0) && v >= base)
        .collect();
    let lattice = field.ground.cut.grid().triangle_slots();
    let band = blend::between(
        grid,
        (from_spine, depth),
        (level, on),
        step,
        (diameter, lattice),
    )?;
    Some((level, band))
}
/// Whether closed `ring` turns nowhere sharper than the crease angle, seen
/// from above, between chords `step` long either side of each of its points.
fn smooth(ring: &[DVec2], step: f64) -> bool {
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
/// distance from it inside it, less than nought just outside it, for rings
/// `step` apart: lifted onto the ground. `None` where it would be shorter than
/// a stepover or leave the ring, or the middle of the ring comes out in
/// pieces.
fn spine(field: &Field<'_>, (ring, inside): (&[DVec2], &[f64]), step: f64) -> Option<Vec<DVec3>> {
    let view = field.ground.cut.view(None);
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
            .filter_map(|i| Some((view.interpolate(*ring.get(i)?, |n| at(d, n))?, i)))
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
                .all(|&n| at(inside, n).is_some_and(|v| v >= 0.0))
        })
        .map(|(t, _)| t)
        .collect();
    let line = only_line(&view, &apart, 0.0, triangles)?;
    let lifted: Vec<DVec3> = line
        .points
        .iter()
        .map(|&p| Some(p.extend(view.height_at(p)?)))
        .collect::<Option<_>>()?;
    let spine = trimmed(&lifted, depth)?;
    let within = spine.iter().all(|p| {
        view.interpolate(p.truncate(), |n| at(inside, n))
            .is_some_and(|v| v > 0.0)
    });
    (within && length(&spine) >= step).then_some(spine)
}

/// The nodes of `field`'s grid beside `spine`, within a cell's diagonal of it
/// seen from above, of those `keep` says, each at its distance from it: where
/// the field measured out from it starts. The spine is on the part already,
/// so each node is measured to it in space, where the area's rings, seen from
/// above, have each node's foot on them dropped onto the part first
/// (`distance::feet`).
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
