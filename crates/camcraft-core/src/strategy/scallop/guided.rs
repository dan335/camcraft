//! A line of the field laid on the part: dropped onto it where the ground is
//! gentle, and found round a wall at a height where it is steep.
//!
//! A point found by dropping the tool at a place seen from above is ill
//! placed on a wall: a hundredth of a millimetre sideways is tenths in
//! height, and on an upright wall the drop goes from its top to its foot at
//! once, so passes laid that way bunch and wobble there. The field's grid
//! knows the height of every node, so where a line crosses a grid edge its
//! height is as well known as its place along the edge: the field's own
//! interpolation gives both. On steep ground the height is taken as given,
//! and the place found across the slope where the tool, dropped, rests at
//! that height: a bisection on the exact drop, which goes down monotonely
//! across a slope. On an upright wall it closes on the wall, the tool beside
//! it at that height, touching it. Between two such points a line is refined
//! as a curve round the wall, its height between theirs, not by dropping at
//! the middle.

use camcraft_geom::mesh::closest_point_on_segment;
use camcraft_geom::trigrid::TriSurface;
use glam::{DVec2, DVec3};

use crate::cutter::surfaces::Seat;
use crate::strategy::parallel::{Line, ParallelParams, ParallelReport, Run, runs_of};

use super::MAX_HALVINGS;

/// Steeper than this, as a rise over a run, a point is found at its height
/// round the wall, not dropped: 45°, where a millimetre sideways is more than
/// a millimetre up.
const STEEP: f64 = 1.0;

/// How many steps either way across the slope a point's height is looked for
/// in, before the search gives up and drops it where it is.
const SEARCH: u32 = 6;

/// How many pieces a bent grid edge is measured in.
const PROFILE_PIECES: u32 = 8;

/// The surface the tool rides from `a` to `b`, seen along the grid edge
/// between them, where it falls steeper than 45° and is bent there: where the
/// tool dropped at the edge's middle rests more than `tolerance` off the
/// straight line between its ends, as at a wall's foot or top inside the cell,
/// or down a cliff, the straight line between the nodes cuts the corner and is
/// no measure of the way along it. Then the tool dropped at [`PROFILE_PIECES`]
/// even steps, the nodes at the ends. `None` where the edge is gentle or
/// straight, or the tool is off the part anywhere along it.
pub(super) fn bent(
    seat: &Seat<'_>,
    (a, b): (DVec3, DVec3),
    tolerance: f64,
    scratch: &mut Vec<u32>,
) -> Option<Vec<DVec3>> {
    if (a.z - b.z).abs() <= STEEP * a.truncate().distance(b.truncate()) {
        return None;
    }
    let m = a.midpoint(b);
    let rest = seat.drop_at_in(m.x, m.y, scratch)?;
    if (rest - m.z).abs() <= tolerance {
        return None;
    }
    let mut out = Vec::new();
    out.push(a);
    for k in 1..PROFILE_PIECES {
        let q = a
            .truncate()
            .lerp(b.truncate(), f64::from(k) / f64::from(PROFILE_PIECES));
        out.push(q.extend(seat.drop_at_in(q.x, q.y, scratch)?));
    }
    out.push(b);
    Some(out)
}

/// The length of a polyline.
pub(super) fn length(points: &[DVec3]) -> f64 {
    points
        .iter()
        .zip(points.iter().skip(1))
        .map(|(a, b)| a.distance(*b))
        .sum()
}

/// The point `along` the polyline `points` from its first.
fn at_length(points: &[DVec3], mut along: f64) -> Option<DVec3> {
    for (&a, &b) in points.iter().zip(points.iter().skip(1)) {
        let piece = a.distance(b);
        if along <= piece {
            return Some(if piece > 0.0 {
                a.lerp(b, along / piece)
            } else {
                a
            });
        }
        along -= piece;
    }
    points.last().copied()
}

/// Where the grid's line point `p` is on the part, by the grid: on a bent
/// edge ([`bent`]), as far along the surface down it as `p` is along the
/// edge, the way the field measured it; elsewhere, `p` at the grid's height
/// there.
fn lifted(
    seat: &Seat<'_>,
    grid: &impl TriSurface,
    p: DVec2,
    tolerance: f64,
    scratch: &mut Vec<u32>,
) -> DVec3 {
    let flat = p.extend(grid.height_at(p).unwrap_or(f64::NAN));
    let Some((a, b)) = edge_at(grid, p).and_then(|(a, b)| Some((grid.point(a)?, grid.point(b)?)))
    else {
        return flat;
    };
    let span = a.truncate().distance(b.truncate());
    let f = (p.distance(a.truncate()) / span).clamp(0.0, 1.0);
    bent(seat, (a, b), tolerance, scratch)
        .and_then(|line| at_length(&line, f * length(&line)))
        .unwrap_or(flat)
}

/// The edge of the grid `p` lies on, by its nodes, lower first, if it lies on
/// one: as a point of a line of the field does.
pub(super) fn edge_at(grid: &impl TriSurface, p: DVec2) -> Option<(u32, u32)> {
    let nodes = grid.triangle(grid.locate(p)?)?;
    let flat = |n: u32| grid.xy(n).map(|q| q.extend(0.0));
    [(0, 1), (1, 2), (2, 0)]
        .into_iter()
        .filter_map(|(i, j)| {
            let (a, b) = (*nodes.get(i)?, *nodes.get(j)?);
            let at = p.extend(0.0);
            let off = at.distance(closest_point_on_segment(at, flat(a)?, flat(b)?));
            Some((off, (a.min(b), a.max(b))))
        })
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .filter(|(off, _)| *off <= 1e-6 * grid.pitch())
        .map(|(_, edge)| edge)
}

/// A point of a line of the field: where it is, at the grid's height there,
/// and on steep ground the way down, seen from above.
#[derive(Debug, Clone, Copy)]
struct Guide {
    at: DVec3,
    fall: Option<DVec2>,
}

/// The tool's tip on the part along `points`, a line of the field over
/// `grid`, closed back to its first if `closed`: thinned to `thin` of the
/// line through its neighbours, then sampled and refined to the tolerance as
/// a parallel pass is, and broken where one is. On ground steeper than
/// [`STEEP`] each point is found at the grid's height there, round the wall
/// across the slope ([`beside`]).
pub(super) fn sweep(
    seat: &Seat<'_>,
    grid: &impl TriSurface,
    (points, closed): (&[DVec2], bool),
    thin: f64,
    params: &ParallelParams,
    report: &mut ParallelReport,
) -> Vec<Run> {
    let mut scratch = Vec::new();
    let lifted: Vec<DVec3> = points
        .iter()
        .map(|&p| lifted(seat, grid, p, params.tolerance, &mut scratch))
        .collect();
    let guides: Vec<Guide> = thinned(&lifted, closed, thin)
        .into_iter()
        .map(|at| Guide {
            at,
            fall: at
                .z
                .is_finite()
                .then(|| grid.locate(at.truncate()))
                .flatten()
                .and_then(|t| grid.fall(t))
                .filter(|&(_, steep)| steep > STEEP)
                .map(|(way, _)| way),
        })
        .collect();
    // Far enough across the slope to reach past the cell the line crosses.
    let reach = 1.5 * grid.pitch();
    let mut samples: Vec<Option<DVec3>> = Vec::new();
    let ends = guides.iter().zip(
        guides
            .iter()
            .skip(1)
            .chain(guides.first().filter(|_| closed)),
    );
    for (a, b) in ends {
        // Round a wall where both ends are on steep ground, along its length
        // as it rises and falls; dropped along the line seen from above
        // otherwise, as every point is where the ground is gentle.
        let round = match (a.fall, b.fall) {
            (Some(fa), Some(fb)) => Some((fa + fb).try_normalize().unwrap_or(fa)),
            _ => None,
        };
        let length = match round {
            Some(_) => a.at.distance(b.at),
            None => a.at.truncate().distance(b.at.truncate()),
        };
        if length < 1e-12 {
            continue;
        }
        let start = samples.last().copied();
        let tolerance = params.tolerance;
        let mut line = Line {
            probe: |u: f64| {
                let q = a.at.lerp(b.at, u / length);
                let p = q.truncate();
                match round {
                    Some(fall) => beside(seat, (p, q.z), fall, (reach, tolerance), &mut scratch),
                    None => seat.drop_at_in(p.x, p.y, &mut scratch).map(|z| p.extend(z)),
                }
            },
            params,
            out: &mut samples,
            report,
        };
        match start {
            Some(first) => line.sweep_on(first, 0.0, length),
            None => line.sweep(0.0, length),
        }
    }
    runs_of(samples, closed, params, report)
}

/// Where the tool rests at height `z`, looked for from `p` along `fall`, the
/// way down, as far as `reach` either way: the place, nearest `p`, where
/// dropped it comes to rest at or above `z` and a little further down below
/// it, the two closed in on until the drop is within a hundredth of
/// `tolerance` of `z` or they are a thousandth of it apart, the tool then
/// beside an upright wall. The tip there at `z`, so never below where the
/// tool rests. Where nothing across the slope is found, as where the ground
/// turns, the tool is dropped at `p`.
fn beside(
    seat: &Seat<'_>,
    (p, z): (DVec2, f64),
    fall: DVec2,
    (reach, tolerance): (f64, f64),
    scratch: &mut Vec<u32>,
) -> Option<DVec3> {
    // How far above `z` the tool rests at `u` along the way down; as far below
    // as can be where nothing is under it, as past a part's edge.
    let mut above = |u: f64| {
        let q = p + fall * u;
        seat.drop_at_in(q.x, q.y, scratch)
            .map_or(f64::NEG_INFINITY, |h| h - z)
    };
    let step = reach / f64::from(SEARCH);
    let at = |k: i64| f64::from(i32::try_from(k).unwrap_or(0)) * step;
    let mut known: Vec<(i64, f64)> = Vec::new();
    let mut ask = |k: i64, above: &mut dyn FnMut(f64) -> f64| {
        if let Some(&(_, v)) = known.iter().find(|(j, _)| *j == k) {
            return v;
        }
        let v = above(at(k));
        known.push((k, v));
        v
    };
    // Outward from `p`, either way, the first step that goes from at or
    // above `z` to below it.
    let mut bracket = None;
    'out: for k in 0..i64::from(SEARCH) {
        for (lo, hi) in [(k, k + 1), (-k - 1, -k)] {
            let (fl, fh) = (ask(lo, &mut above), ask(hi, &mut above));
            if fl >= 0.0 && fh < 0.0 {
                bracket = Some((at(lo), at(hi), fh));
                break 'out;
            }
        }
    }
    let Some((mut lo, mut hi, mut below)) = bracket else {
        return seat.drop_at_in(p.x, p.y, scratch).map(|h| p.extend(h));
    };
    for _ in 0..MAX_HALVINGS {
        if -below <= 0.01 * tolerance || hi - lo <= 1e-3 * tolerance {
            break;
        }
        let mid = 0.5 * (lo + hi);
        let v = above(mid);
        if v >= 0.0 {
            lo = mid;
        } else {
            (hi, below) = (mid, v);
        }
    }
    Some((p + fall * hi).extend(z))
}

/// `points` thinned: each dropped that is within `within` of the line
/// through the points kept either side, in space where both have a height
/// and seen from above where one has none. Douglas and Peucker's: a closed
/// line is first split at the point furthest from its start.
fn thinned(points: &[DVec3], closed: bool, within: f64) -> Vec<DVec3> {
    let mut line = points.to_vec();
    if closed && let Some(&first) = points.first() {
        line.push(first);
    }
    let n = line.len();
    if n < 3 {
        return points.to_vec();
    }
    let off = |p: DVec3, a: DVec3, b: DVec3| {
        if p.z.is_finite() && a.z.is_finite() && b.z.is_finite() {
            p.distance(closest_point_on_segment(p, a, b))
        } else {
            // Seen from above; a point with no height is always kept.
            let flat = |q: DVec3| q.truncate().extend(0.0);
            if p.z.is_finite() {
                flat(p).distance(closest_point_on_segment(flat(p), flat(a), flat(b)))
            } else {
                f64::INFINITY
            }
        }
    };
    let mut keep = vec![false; n];
    if let Some(k) = keep.first_mut() {
        *k = true;
    }
    if let Some(k) = keep.last_mut() {
        *k = true;
    }
    let mut spans = vec![(0usize, n - 1)];
    if closed && let Some(&start) = line.first() {
        let far = line
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.distance(start).total_cmp(&b.1.distance(start)))
            .map_or(0, |(i, _)| i);
        if far > 0 && far < n - 1 {
            if let Some(k) = keep.get_mut(far) {
                *k = true;
            }
            spans = vec![(0, far), (far, n - 1)];
        }
    }
    while let Some((a, b)) = spans.pop() {
        let (Some(&pa), Some(&pb)) = (line.get(a), line.get(b)) else {
            continue;
        };
        let mut worst = (0usize, 0.0f64);
        for i in a + 1..b {
            let Some(&p) = line.get(i) else { continue };
            let d = off(p, pa, pb);
            if d > worst.1 {
                worst = (i, d);
            }
        }
        if worst.1 > within {
            if let Some(k) = keep.get_mut(worst.0) {
                *k = true;
            }
            spans.push((a, worst.0));
            spans.push((worst.0, b));
        }
    }
    if closed {
        line.pop();
        keep.pop();
    }
    line.into_iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(p))
        .collect()
}
