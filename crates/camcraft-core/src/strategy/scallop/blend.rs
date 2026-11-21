//! Rings that blend from the last whole ring into an edge they meet at a slant.
//!
//! The field's rings are a stepover apart from the edges it is measured from.
//! Where another closed edge of the ground lies at a slant to them, as where
//! the tool leaves a bowl's bulging outside, lower on some sides than others,
//! that edge cuts the rings before it into arcs, each lifted to and from. On
//! the piece of ground between the last ring that goes all the way round and
//! that edge, the band, the rings turn gradually from the one into the other
//! instead: closed rings, as many as the band's widest place needs, the last
//! of them the edge itself (decision 0038).
//!
//! # Above the edge
//!
//! With `a` the field less the last whole ring's level, the distance along the
//! surface from that ring, and `b` the distance from the edge, the rings are
//! the lines of `t = a / (a + b)` at whole fractions of it. Where the two
//! edges face each other, `a + b` is the band's width there, and lines of `t`
//! a unit apart are that far apart; elsewhere further or nearer, as the
//! gradient of `t` says. So the count is measured, not guessed: on each whole
//! triangle of the band, how far apart along the surface lines of `t` a unit
//! apart are, `1 / |∇t|`, the most of it over the stepover, rounded up. That
//! is `(a + b)² / |b ∇a − a ∇b|`, and `∇a` and `∇b` are taken as unit, as a
//! distance's are: across a ridge of either field, where its rings turn a
//! corner, a triangle's plane averages the fronts either side, shorter than
//! either, and that shortness is no width (decision 0038, amended). A band
//! with no whole triangle in it, thin down a wall so steep every cell there
//! is cut, has `a + b`, its width, at its widest instead (decision 0039).
//!
//! The band starts from the last ring at least half a stepover from the edge
//! all round ([`THIN_END`]): nearer, where the ring comes close to the edge,
//! `t` is nought over nought.
//!
//! # Below it
//!
//! Where the tool leaves the part, rings below it go down the wall beside it,
//! where the tool cannot rest from above: level rings, which the operation
//! lays, start half a stepover below the edge's lowest point, and between the
//! edge and the first of them, rings each a fraction of the way down from the
//! edge to it, as many as its deepest place needs. Each point is found beside the
//! wall at its height as the level rings' are ([`Puller`]).
//!
//! # Bounded
//!
//! Blending a band costs about half its width over the stepover in laps more
//! than the arcs it replaces. So it is done only where the edge is closed, the
//! band is ring-shaped between the last whole ring and the edge, each blended
//! ring coming out one closed line, and no wider than [`BAND_LIMIT`] tools;
//! below, only where the edge is the one line the tool leaves the part along.
//! Otherwise the rings are cut by the edge as they were.

use camcraft_geom::contour::Region;
use camcraft_geom::cut::View;
use camcraft_geom::geodesic::Seed;
use camcraft_geom::isoline::IsoLine;
use camcraft_geom::trigrid::TriSurface;
use glam::DVec3;

use crate::cutter::surfaces::Seat;
use crate::cutter::tool::ToolProfile;
use crate::strategy::field::count;
use crate::strategy::parallel::Run;
use crate::strategy::waterline::Puller;

use super::distance::{Field, at, only_line};
use super::ground::Edge;

/// The widest band blended, in tool diameters: twice, as far as the tool
/// stays down between passes over the part. A user's bulging bowl's band is
/// 12.4 mm wide against 19 for its 9.5 mm ball.
const BAND_LIMIT: f64 = 2.0;

/// The least a band is wide at its thin end, in stepovers: where the last
/// whole ring meets the edge, `t` is nought over nought, and the rings
/// between bunch together there for nothing.
const THIN_END: f64 = 0.5;

/// The rings that blend from the last whole ring into a far edge.
pub(super) struct Band {
    /// The level of the last whole ring.
    pub(super) level: u32,
    /// Whether each node is on the band: where the field is at least that
    /// level, on the piece of ground that reaches the edge.
    on: Vec<bool>,
    /// The rings between, out from the last whole ring.
    pub(super) rings: Vec<IsoLine>,
}

impl Band {
    /// Whether node `n` is on the band.
    pub(super) fn holds(&self, n: u32) -> bool {
        usize::try_from(n)
            .ok()
            .and_then(|i| self.on.get(i))
            .copied()
            .unwrap_or(false)
    }
}

/// The band between the last whole ring of `field` over `grid`, `step` apart,
/// and its far edge `far`, inside `area`, for a tool `diameter` across;
/// `None` where it is not to be blended.
pub(super) fn band(
    grid: &View<'_>,
    (field, area): (&Field<'_>, &Region),
    far: &Edge,
    step: f64,
    diameter: f64,
) -> Option<Band> {
    if !far.closed {
        return None;
    }
    let d = &field.d;
    let along: Vec<f64> = far.nodes.iter().filter_map(|&n| at(d, n)).collect();
    let nearest = along.iter().copied().reduce(f64::min)?;
    let furthest = along.iter().copied().reduce(f64::max)?;
    // The last ring half a stepover or more from the edge all round: the
    // field is a distance, so it is that far from every point of the edge.
    let level = start(nearest, furthest, step, diameter)?;
    let base = f64::from(level) * step;
    let on = reaching(grid, d, far, base);
    // Ring-shaped between the two, not running out of the area.
    if (0u32..)
        .zip(&on)
        .any(|(n, &on)| on && !field.inside(area, n))
    {
        return None;
    }
    let seeds: Vec<Seed> = far
        .nodes
        .iter()
        .map(|&node| Seed {
            node,
            distance: 0.0,
        })
        .collect();
    let from_far = field.measure(&seeds);
    let lattice = field.ground.cut.grid().triangle_slots();
    between(grid, (d, &from_far), (level, on), step, (diameter, lattice))
}

/// The level of the last whole ring before a far line whose distance, along
/// it, runs from `nearest` to `furthest`, rings `step` apart, for a tool
/// `diameter` across: the last half a stepover or more from it all round,
/// the field being a distance. `None` where there is none, or the band past it
/// is wider than [`BAND_LIMIT`] tools.
pub(super) fn start(nearest: f64, furthest: f64, step: f64, diameter: f64) -> Option<u32> {
    let level = count(((nearest - THIN_END * step) / step).floor()).filter(|&k| k > 0)?;
    (furthest - f64::from(level) * step <= BAND_LIMIT * diameter).then_some(level)
}

/// The rings that blend from the line of `d` at `level` stepovers, `step`
/// apart, into a far line, `far` each node's distance from it, over the nodes
/// `on`, for a tool `diameter` across; the triangles of the lattice those
/// below `lattice`. `None` where the band is too wide, or a ring between does
/// not come out one closed line.
pub(super) fn between(
    grid: &View<'_>,
    (d, from_far): (&[f64], &[f64]),
    (level, on): (u32, Vec<bool>),
    step: f64,
    (diameter, lattice): (f64, u32),
) -> Option<Band> {
    let base = f64::from(level) * step;
    // `t` on the band and the nodes beside it, so the last whole ring's
    // triangles have it at every corner.
    let mut t = vec![f64::NAN; on.len()];
    let mut triangles: Vec<u32> = Vec::new();
    for n in (0u32..).zip(&on).filter_map(|(n, &on)| on.then_some(n)) {
        for (id, corners) in grid.triangles_at(n) {
            triangles.push(id);
            for m in corners {
                let (Some(a), Some(b)) = (at(d, m), at(from_far, m)) else {
                    continue;
                };
                if let Some(slot) = usize::try_from(m).ok().and_then(|i| t.get_mut(i))
                    && a - base + b > 0.0
                {
                    *slot = (a - base) / (a - base + b);
                }
            }
        }
    }
    triangles.sort_unstable();
    triangles.dedup();
    let from_ring = |n: u32| at(d, n).map(|v| v - base);
    let from_edge = |n: u32| at(from_far, n);
    let measured = triangles
        .iter()
        .filter(|&&id| id < lattice)
        .filter_map(|&id| apart(grid, (&from_ring, &from_edge), &t, id))
        .fold(0.0, f64::max);
    // A band with no whole triangle of the lattice in it, thin down a wall
    // nearly upright where every cell is cut, has its width at its widest.
    let widest = if measured > 0.0 {
        measured
    } else {
        (0u32..)
            .zip(&on)
            .filter(|&(_, &on)| on)
            .filter_map(|(n, _)| Some(at(d, n)? - base + at(from_far, n)?))
            .fold(0.0, f64::max)
    };
    if widest > BAND_LIMIT * diameter {
        return None;
    }
    let rings = count((widest / step).ceil()).filter(|&n| n > 0)?;
    let mut out = Vec::new();
    for k in 1..rings {
        let level = f64::from(k) / f64::from(rings);
        out.push(only_line(grid, &t, level, triangles.iter().copied()).filter(|r| r.closed)?);
    }
    Some(Band {
        level,
        on,
        rings: out,
    })
}

/// Whether each node of `grid` is where the field `d` is at least `base` and
/// reaches the edge `far` that way.
fn reaching(grid: &View<'_>, d: &[f64], far: &Edge, base: f64) -> Vec<bool> {
    let at_least = |n: u32| at(d, n).is_some_and(|v| v >= base);
    let mut on = vec![false; usize::try_from(grid.nodes()).unwrap_or(0)];
    let mut stack: Vec<u32> = far.nodes.iter().copied().filter(|&n| at_least(n)).collect();
    for &n in &stack {
        if let Some(slot) = usize::try_from(n).ok().and_then(|i| on.get_mut(i)) {
            *slot = true;
        }
    }
    while let Some(n) = stack.pop() {
        for (_, corners) in grid.triangles_at(n) {
            for m in corners {
                if let Some(slot) = usize::try_from(m).ok().and_then(|i| on.get_mut(i))
                    && !*slot
                    && at_least(m)
                {
                    *slot = true;
                    stack.push(m);
                }
            }
        }
    }
    on
}

/// A node's distance from a line, where it has one.
type Distance<'a> = &'a dyn Fn(u32) -> Option<f64>;

/// How far apart along the surface lines of `t` a unit apart are across
/// triangle `id` of `grid`, lifted, `a` each node's distance from the last
/// whole ring and `b` from the far edge: `1 / |∇t|`, which is
/// `(a + b)² / |b ∇a − a ∇b|`, with `∇a` and `∇b` taken as unit, as a
/// distance's are, at the triangle's middle. Across a ridge of a field, where
/// its rings turn a corner, a plane through three nodes either side is the
/// mean of the two fronts, shorter than either (`cos 45°` long where they
/// meet square): the rings of `t` either side are spaced by their own front,
/// so that shortness is no width, and only the gradients' directions are
/// kept. `None` where `t` is not between 0 and 1 at every corner. Asked only
/// of whole triangles of the lattice: the slivers a cut cell is fanned into
/// are too thin to say.
fn apart(grid: &View<'_>, (a, b): (Distance<'_>, Distance<'_>), t: &[f64], id: u32) -> Option<f64> {
    let corners = grid.triangle(id)?;
    if corners
        .iter()
        .any(|&n| at(t, n).is_none_or(|v| !(0.0..=1.0).contains(&v)))
    {
        return None;
    }
    let [p, q, r] = corners.map(|n| grid.point(n));
    let (e1, e2) = (q? - p?, r? - p?);
    // The gradient in the triangle's plane of what `f` is at its corners,
    // from its two edges, as unit.
    let (g11, g12, g22) = (e1.dot(e1), e1.dot(e2), e2.dot(e2));
    let det = g11 * g22 - g12 * g12;
    if det <= f64::MIN_POSITIVE {
        return None;
    }
    let unit = |f: Distance<'_>| -> Option<(DVec3, f64)> {
        let [f0, f1, f2] = corners.map(f);
        let (d1, d2) = (f1? - f0?, f2? - f0?);
        let g = e1 * ((d1 * g22 - d2 * g12) / det) + e2 * ((d2 * g11 - d1 * g12) / det);
        Some((g.try_normalize()?, (f0? + f1? + f2?) / 3.0))
    };
    let ((ga, a), (gb, b)) = (unit(a)?, unit(b)?);
    let across = (ga * b - gb * a).length();
    (across > 0.0).then(|| (a + b) * (a + b) / across)
}

/// The rings below the far edge `far`, `step` apart at their widest, down to
/// half a stepover below its lowest point or `bottom`, whichever is higher, each
/// point beside the wall at its height; and the height the level rings start
/// at. `None` where the edge reaches the Bottom, or a point has no wall beside
/// it within the tool's radius.
pub(super) fn below(
    seat: &Seat<'_>,
    far: &Edge,
    step: f64,
    bottom: f64,
    tolerance: f64,
) -> Option<(Vec<Run>, f64)> {
    let lowest = far.points.iter().map(|p| p.z).reduce(f64::min)?;
    let highest = far.points.iter().map(|p| p.z).reduce(f64::max)?;
    if !far.closed || lowest - tolerance <= bottom {
        return None;
    }
    let to = (lowest - THIN_END * step).max(bottom);
    let rings = count(((highest - to) / step).ceil())?;
    let n = far.points.len();
    let mut out = Vec::new();
    for k in 1..rings {
        let f = f64::from(k) / f64::from(rings);
        let mut points = Vec::with_capacity(n);
        for (i, &p) in far.points.iter().enumerate() {
            let (Some(prev), Some(next)) =
                (far.points.get((i + n - 1) % n), far.points.get((i + 1) % n))
            else {
                return None;
            };
            // The ground on the edge's left: the wall that way.
            let inward = (next.truncate() - prev.truncate()).try_normalize()?.perp();
            let z = p.z + f * (to - p.z);
            let mut puller = Puller {
                seat,
                z,
                reach_by: seat.tool().max_radius(),
                tolerance,
                scratch: Vec::new(),
            };
            points.push(puller.onto(p.truncate(), inward)?.extend(z));
        }
        out.push(Run {
            points,
            closed: true,
        });
    }
    Some((out, to))
}
