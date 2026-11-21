//! The ground the field runs over, and its edges.
//!
//! The tool is dropped at every node of a grid over the area, exactly, and
//! each node is classed by what holds it there: a face it machines
//! ([`MACHINED`]), faces it avoids alone ([`AVOIDED`]), or nothing, off the
//! part ([`OFF_PART`]). The ground is where a face machined holds it. Where
//! the class changes along a grid edge, the place is found by halving the
//! exact drop, and the cells there are cut, each side triangulated right up
//! to it ([`camcraft_geom::cut`]): so the ground, and the field measured
//! over it, run right to where it ends (decision 0037).
//!
//! Its edges are lines through those cuts, the ground on their left, in
//! pieces by what lies past them: the edge of a face avoided, or the line
//! where the tool leaves the part.

use std::collections::HashMap;

use camcraft_geom::contour::Region;
use camcraft_geom::cut::{CutGrid, NO_BAND};
use camcraft_geom::trigrid::TriSurface;
use glam::{DVec2, DVec3};

use crate::cutter::surfaces::{Rest, Seat};
use crate::cutter::tool::ToolProfile;
use crate::strategy::StrategyError;
use crate::strategy::field::{self, AreaGrid, OFF_PART};

use super::{MAX_HALVINGS, ScallopParams, ScallopReport};

/// The class of a node the tool rests on a face it machines at.
pub(super) const MACHINED: u32 = 0;

/// The class of a node the tool rests on faces it avoids alone at.
pub(super) const AVOIDED: u32 = 1;

/// How many times, at most, islands finer than the grid are taken into the
/// ground round them and the grid cut again: once takes every island there
/// is, and taking one in makes no other.
const MAX_ISLAND_ROUNDS: u32 = 4;

/// The ground: the area's grid, and that grid cut where the ground ends.
pub(super) struct Ground {
    /// The grid over the area, a node's height given inside it and beside it.
    pub(super) on: AreaGrid,
    /// The grid, its cells cut where the tool goes from resting on a face it
    /// machines to resting on faces it avoids alone, or off the part.
    pub(super) cut: CutGrid,
}

/// A piece of an edge of the ground: a line through cuts of its grid, the
/// ground on its left.
pub(super) struct Edge {
    /// The cuts, as nodes of the grid cut.
    pub(super) nodes: Vec<u32>,
    /// Their places, at their heights.
    pub(super) points: Vec<DVec3>,
    /// Whether it closes on itself.
    pub(super) closed: bool,
    /// The class of what lies past it: [`AVOIDED`] or [`OFF_PART`].
    pub(super) past: u32,
}

impl Ground {
    /// The ground over `area`, whose bounds are `lo` to `hi`: a third of a
    /// stepover apart, and fine enough for the tool's own curvature;
    /// coarsened, if it must be, to stay within the budget, and no further
    /// than half a stepover. The tool's height at each node, dropped exactly,
    /// and whether a face it machines holds it there.
    pub(super) fn sampled(
        seat: &Seat<'_>,
        area: &Region,
        (lo, hi): (DVec2, DVec2),
        params: &ScallopParams,
        report: &mut ScallopReport,
    ) -> Result<Self, StrategyError> {
        let s = params.stepover;
        let class_of = |rest: Rest| if rest.machined { MACHINED } else { AVOIDED };
        let (on, classes) = AreaGrid::sampled(
            area,
            (lo, hi),
            field::pitch(s, seat.tool().max_radius()),
            s / 2.0,
            params.max_nodes,
            // The drops are most of the field's work; the field, a third of
            // the whole.
            &params.watch.part(0.0, 0.3),
            |x, y, scratch| seat.rest_in(x, y, scratch).map(|r| (r.z, class_of(r))),
        )?;
        report.pitch = on.grid.pitch();
        report.nodes = u64::from(on.grid.nodes());
        report.coarsened = on.coarsened;
        params.watch.check()?;
        let mut cut = cut_where_it_ends(seat, &on, classes, params.tolerance);
        level_where_it_leaves(&mut cut, params.ramp.tan());
        Ok(Self { on, cut })
    }

    /// Its edges, each piece by what lies past it.
    pub(super) fn edges(&self) -> Vec<Edge> {
        let cut = &self.cut;
        (0..cut.bands())
            .flat_map(|b| cut.edges(b))
            .flat_map(|(chain, closed)| pieces(cut, &chain, closed))
            .filter(|e| e.past == AVOIDED || e.past == OFF_PART)
            .collect()
    }

    /// The highest point, inside `area`, of where the tool leaves the part
    /// from above.
    pub(super) fn leaves_at(&self, area: &Region) -> Option<f64> {
        let cut = &self.cut;
        cut.cuts()
            .iter()
            .filter(|c| {
                c.bands.1 == NO_BAND
                    && cut.class(c.edge.1) == OFF_PART
                    && area.contains(c.at.truncate())
            })
            .map(|c| c.at.z)
            .filter(|z| z.is_finite())
            .reduce(f64::max)
    }
}

/// The grid of `on`, its nodes of `classes`, cut where the ground machined
/// ends ([`ends_between`]). The grid resolves what the tool rests on to its
/// pitch and no finer: where the faces' facets turn it on and off an edge a
/// hair either side of a node, ground may come out lying wholly in cells its
/// edges cut, an island of a node or two. It is taken into the ground round
/// it, and the grid cut again.
fn cut_where_it_ends(
    seat: &Seat<'_>,
    on: &AreaGrid,
    mut classes: Vec<u32>,
    tolerance: f64,
) -> CutGrid {
    let grid = &on.grid;
    let find = |a: u32, b: u32| ends_between(seat, grid, (a, b), tolerance);
    let machined = |class: u32| class == MACHINED;
    let mut cut = CutGrid::new(grid.clone(), classes.clone(), machined, find);
    for _ in 0..MAX_ISLAND_ROUNDS {
        let islands: HashMap<u32, u32> = (0..cut.bands())
            .filter(|&b| !cut.resolved(b))
            .filter_map(|b| Some((b, cut.class_round(b)?)))
            .collect();
        if islands.is_empty() {
            break;
        }
        for (n, class) in (0u32..).zip(classes.iter_mut()) {
            if let Some(&round) = islands.get(&cut.band_of(n)) {
                *class = round;
            }
        }
        cut = CutGrid::new(grid.clone(), classes.clone(), machined, find);
    }
    cut
}

/// Where the tool leaves the part, each run of its cuts given the highest
/// heights no higher than its own and rising or falling along it no more
/// steeply than `slope`, as a rise over a run.
///
/// There the tool's tip is where its widest circle passes the part's widest
/// point: a flat maximum, whose height on the part's facets jumps from one
/// facet to the next, as far as the square root of twice the bulge's radius
/// times the facets' deviation. Dropped at the cuts along the grid's edges, a
/// user's bowl's line alternated by 0.1 to 0.3 mm from cut to cut. Beside
/// that line nothing is under the tool, and it touches the part at the same
/// place from that height down (to a ten-thousandth of a millimetre over 0.6
/// mm, measured), so any height under it cuts the same: the highest that rise
/// and fall no faster than the tool ramps is taken, each cut's own height or,
/// lower, another's raised by the slope over how far along the line it is.
fn level_where_it_leaves(cut: &mut CutGrid, slope: f64) {
    let leaving: Vec<(Vec<u32>, Vec<DVec3>, bool)> = (0..cut.bands())
        .flat_map(|b| cut.edges(b))
        .flat_map(|(chain, closed)| pieces(cut, &chain, closed))
        .filter(|e| e.past == OFF_PART)
        .map(|e| {
            let own: Vec<(u32, DVec3)> = e
                .nodes
                .iter()
                .zip(&e.points)
                .filter(|(n, _)| {
                    cut.cut(**n)
                        .is_some_and(|c| cut.class(c.edge.1) == OFF_PART)
                })
                .map(|(&n, &p)| (n, p))
                .collect();
            let (nodes, points) = own.into_iter().unzip();
            (nodes, points, e.closed)
        })
        .collect();
    for (nodes, points, closed) in leaving {
        for (n, z) in nodes.into_iter().zip(lowest_within(&points, closed, slope)) {
            cut.set_height(n, z);
        }
    }
}

/// The highest heights over `points` no higher than each point's own and
/// changing from one point to the next by no more than `slope` times how far
/// apart they are seen from above: twice along the line, and round again
/// where it is `closed`, each the least of its own and the last's raised.
fn lowest_within(points: &[DVec3], closed: bool, slope: f64) -> Vec<f64> {
    let n = points.len();
    let mut z: Vec<f64> = points.iter().map(|p| p.z).collect();
    let rounds = if closed { 2 } else { 1 };
    for forward in [true, false] {
        for k in 1..rounds * n {
            let (i, j) = if forward {
                (k % n, (k - 1) % n)
            } else {
                ((rounds * n - k - 1) % n, (rounds * n - k) % n)
            };
            let (Some(&p), Some(&q), Some(&zj)) = (points.get(i), points.get(j), z.get(j)) else {
                continue;
            };
            let raised = zj + slope * p.truncate().distance(q.truncate());
            if let Some(zi) = z.get_mut(i)
                && raised < *zi
            {
                *zi = raised;
            }
        }
    }
    z
}
/// Where the ground machined ends along the grid edge from node `a`, of the
/// ground's class, to node `b`: where the tool goes from resting on a face it
/// machines to resting on faces it avoids alone, or off the part, found by
/// halving, to a hundredth of the tolerance seen from above and until the
/// height the tool rests at there moves by less than that from one halving to
/// the next. Where the tool leaves the part, the way down to the edge is steep
/// as a square root, and the height is what matters: the last ring is laid
/// along it. NaN where either end is off the grid or `a` is off the part.
fn ends_between(
    seat: &Seat<'_>,
    grid: &impl TriSurface,
    (a, b): (u32, u32),
    tolerance: f64,
) -> DVec3 {
    let mut scratch = Vec::new();
    let (Some(pa), Some(pb)) = (grid.xy(a), grid.xy(b)) else {
        return DVec3::NAN;
    };
    let rest = |p: DVec2, scratch: &mut Vec<u32>| seat.rest_in(p.x, p.y, scratch);
    let Some(start) = rest(pa, &mut scratch) else {
        return DVec3::NAN;
    };
    let close = 0.01 * tolerance;
    let (mut near, mut away, mut moved) = (pa, pb, f64::INFINITY);
    let (mut here, mut there) = (start, None);
    for _ in 0..MAX_HALVINGS {
        if near.distance(away) <= close && moved <= close {
            break;
        }
        let mid = near.midpoint(away);
        match rest(mid, &mut scratch) {
            Some(r) if r.machined == start.machined => {
                moved = (r.z - here.z).abs();
                (near, here) = (mid, r);
            }
            r => (away, there) = (mid, r),
        }
    }
    match there {
        Some(there) if near.distance(away) <= close && (there.z - here.z).abs() > tolerance => near
            .extend(beside_a_wall(
                seat,
                near,
                (here, there),
                tolerance,
                &mut scratch,
            )),
        _ => near.extend(here.z),
    }
}

/// The height the ground ends at `at`, where the halving closed on where the
/// tool's path stands upright, its height jumping a hair across from `here`
/// to `there`: beside a wall, seen from above. Dropped there the tool rests at
/// the foot or at the top, whichever side of the jump it lands, and finds
/// nothing between; moved up beside the wall it touches the face it rises
/// beside ([`Seat::beside`]). The stretch is that face's: machined, as a
/// pocket's wall above a floor avoided, the ground ends at the wall's foot and
/// the wall is the field's; avoided, at its top. Where nothing is within the
/// tool's radius of it at its widest, under an overhang, it touches the
/// overhang above with its side: the top's.
fn beside_a_wall(
    seat: &Seat<'_>,
    at: DVec2,
    (here, there): (Rest, Rest),
    tolerance: f64,
    scratch: &mut Vec<u32>,
) -> f64 {
    let (foot, top) = if here.z < there.z {
        (here, there)
    } else {
        (there, here)
    };
    let middle = 0.5 * (foot.z + top.z);
    let face = seat
        .upright_toward(at, middle, tolerance, scratch)
        .and_then(|toward| seat.beside(at, toward, middle))
        .map_or(top.machined, |(_, machined)| machined);
    if face == foot.machined { top.z } else { foot.z }
}

/// `chain`, a line through cuts of `cut` with the ground on its left, closed
/// back to its first if `closed`, in pieces by what lies past each cut, each
/// beginning where the last ended so none leaves a gap.
fn pieces(cut: &CutGrid, chain: &[u32], closed: bool) -> Vec<Edge> {
    let past = |n: u32| cut.cut(n).map_or(OFF_PART, |c| cut.class(c.edge.1));
    let at = |n: u32| cut.cut(n).map(|c| c.at);
    let Some(&first) = chain.first() else {
        return Vec::new();
    };
    if chain.iter().all(|&c| past(c) == past(first)) {
        return vec![Edge {
            nodes: chain.to_vec(),
            points: chain.iter().filter_map(|&c| at(c)).collect(),
            closed,
            past: past(first),
        }];
    }
    // A closed chain started where what lies past it changes, so no piece
    // runs round its join.
    let n = chain.len();
    let past_at = |i: usize| chain.get(i % n).map_or(OFF_PART, |&c| past(c));
    let start = if closed {
        (0..n)
            .find(|&i| past_at(i) != past_at(i + n - 1))
            .unwrap_or(0)
    } else {
        0
    };
    let order: Vec<u32> = chain.iter().cycle().skip(start).take(n).copied().collect();
    let mut out: Vec<Edge> = Vec::new();
    let mut last: Option<u32> = None;
    for (i, &c) in order.iter().enumerate() {
        let class = past(c);
        let Some(p) = at(c) else { continue };
        match out.last_mut() {
            Some(piece) if last == Some(class) => {
                piece.nodes.push(c);
                piece.points.push(p);
            }
            _ => {
                let before = i.checked_sub(1).and_then(|j| order.get(j)).copied();
                let mut nodes: Vec<u32> = before.filter(|&q| at(q).is_some()).into_iter().collect();
                let mut points: Vec<DVec3> = before.and_then(at).into_iter().collect();
                nodes.push(c);
                points.push(p);
                out.push(Edge {
                    nodes,
                    points,
                    closed: false,
                    past: class,
                });
            }
        }
        last = Some(class);
    }
    out
}
