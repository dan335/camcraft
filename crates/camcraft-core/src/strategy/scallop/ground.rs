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
//! pieces by what lies past them: the edge of a face avoided, the line where
//! the tool leaves the part, or a strip where it rolls over a sharp edge.
//!
//! # Strips where the tool rolls over a sharp edge
//!
//! Where the tool rests on the part by a sharp convex edge alone, its contact
//! stays on the edge while its tip goes round it, a tool's radius times the
//! angle the faces turn through ([`Seat::rest_in`]). Rings that meet such an
//! edge at a slant climb over it and back, each cutting the same line again.
//! Where those nodes make a ring-shaped strip, closed round one hole, the
//! strip is a break in the ground ([`ROLLING`]): the ground stops at its near
//! border and starts again at its far one, each found exactly as the ground's
//! other edges are, and each a ring of its own, the tool touching the edge
//! from one side, then from the other. The ground either side is a zone of its
//! own, a band of the grid cut (decision 0039). Rolling nodes that make no
//! such strip stay ground, and the rings cross them as before.

use std::collections::HashMap;

use camcraft_geom::contour::Region;
use camcraft_geom::cut::{CutGrid, NO_BAND};
use camcraft_geom::geodesic::{Seed, march_along};
use camcraft_geom::trigrid::{TriGrid, TriSurface};
use glam::{DVec2, DVec3};
use rayon::prelude::*;

use crate::cutter::surfaces::{Rest, Seat};
use crate::cutter::tool::ToolProfile;
use crate::strategy::StrategyError;
use crate::strategy::field::{self, AreaGrid, OFF_PART};

use super::{MAX_HALVINGS, ScallopParams, ScallopReport, guided, length};

/// The class of a node the tool rests on a face it machines at.
pub(super) const MACHINED: u32 = 0;

/// The class of a node the tool rests on faces it avoids alone at.
pub(super) const AVOIDED: u32 = 1;

/// The class of a node where the tool rests by a sharp convex edge alone, in
/// a strip ring-shaped round one hole: a break between the ground either
/// side.
pub(super) const ROLLING: u32 = 2;

/// How many nodes deep the ground lies round a strip that breaks it, at the
/// least: so the cells cut where the ground ends against the strip are cut by
/// nothing else. Up a wall nearly upright, seen from above a node or two wide,
/// the cells at its top are cut at its foot too, and the ground between, a
/// sliver, breaks into pieces: on a bowl in the tests, its wall 0.6 mm wide at
/// its steepest, its rings up the wall came out open.
const BESIDE: i64 = 2;

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
    /// The length along the surface of each grid edge bent down a cliff
    /// ([`cliff_edges`]), by its key, lower node first.
    cliffs: HashMap<(u32, u32), f64>,
    /// Each lattice node's class as sampled, the strips' [`ROLLING`].
    classes: Vec<u32>,
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
    /// The class of what lies past it: [`AVOIDED`], [`OFF_PART`] or
    /// [`ROLLING`].
    pub(super) past: u32,
    /// The band of the grid cut it bounds.
    pub(super) band: u32,
}

impl Ground {
    /// The ground over `area`, whose bounds are `lo` to `hi`: a third of a
    /// stepover apart, and fine enough for the tool's own curvature;
    /// coarsened, if it must be, to stay within the budget, and no further
    /// than half a stepover. The tool's height at each node, dropped exactly,
    /// and whether a face it machines holds it there. The strips where it
    /// rolls over a sharp edge round a hole are breaks in it.
    pub(super) fn sampled(
        seat: &Seat<'_>,
        area: &Region,
        (lo, hi): (DVec2, DVec2),
        params: &ScallopParams,
        report: &mut ScallopReport,
    ) -> Result<Self, StrategyError> {
        let s = params.stepover;
        let class_of = |rest: Rest, rolling: bool| match (rest.machined, rolling) {
            (false, _) => AVOIDED,
            (true, true) => ROLLING,
            (true, false) => MACHINED,
        };
        let (on, mut classes) = AreaGrid::sampled(
            area,
            (lo, hi),
            field::pitch(s, seat.tool().max_radius()),
            s / 2.0,
            params.max_nodes,
            // The drops are most of the field's work; the field, a third of
            // the whole.
            &params.watch.part(0.0, 0.3),
            |x, y, scratch| {
                let (r, rolling) = seat.rest_in(x, y, scratch)?;
                Some((r.z, class_of(r, rolling)))
            },
        )?;
        report.pitch = on.grid.pitch();
        report.nodes = u64::from(on.grid.nodes());
        report.coarsened = on.coarsened;
        params.watch.check()?;
        keep_ring_shaped(&on.grid, &mut classes);
        Self::cut(seat, on, classes, params)
    }

    /// `on`, its nodes of `classes`, cut where the ground ends.
    fn cut(
        seat: &Seat<'_>,
        on: AreaGrid,
        classes: Vec<u32>,
        params: &ScallopParams,
    ) -> Result<Self, StrategyError> {
        let mut cut = cut_where_it_ends(seat, &on, classes.clone(), params.tolerance);
        level_where_it_leaves(&mut cut, params.ramp.tan());
        let cliffs = cliff_edges(seat, &cut, params.tolerance);
        params.watch.check()?;
        Ok(Self {
            on,
            cut,
            cliffs,
            classes,
        })
    }

    /// Whether strips break it ([`ROLLING`]).
    pub(super) fn has_strips(&self) -> bool {
        self.classes.contains(&ROLLING)
    }

    /// The same ground with its strips ground again, every rolling node
    /// machined, as it was before strips broke it: the drops kept, the grid
    /// cut again.
    pub(super) fn without_strips(
        &self,
        seat: &Seat<'_>,
        params: &ScallopParams,
    ) -> Result<Self, StrategyError> {
        let classes = self
            .classes
            .iter()
            .map(|&c| if c == ROLLING { MACHINED } else { c })
            .collect();
        Self::cut(seat, self.on.clone(), classes, params)
    }

    /// Whether node `n` of the grid cut is where the ground ends against a
    /// strip the tool rolls over a sharp edge along.
    pub(super) fn against_a_strip(&self, n: u32) -> bool {
        against_a_strip(&self.cut, n)
    }

    /// Each node's distance along the surface from `seeds`, down each cliff
    /// along the surface the tool rides.
    pub(super) fn measure(&self, seeds: &[Seed]) -> Vec<f64> {
        let view = self.cut.view(None);
        march_along(
            &view,
            seeds,
            |n| view.point(n),
            |a, b| self.cliffs.get(&(a.min(b), a.max(b))).copied(),
        )
    }

    /// Its edges, each piece by what lies past it.
    pub(super) fn edges(&self) -> Vec<Edge> {
        let cut = &self.cut;
        (0..cut.bands())
            .flat_map(|b| cut.edges(b))
            .flat_map(|(chain, closed)| pieces(cut, &chain, closed))
            .filter(|e| e.past == AVOIDED || e.past == OFF_PART || e.past == ROLLING)
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
    let cut_by = |classes: &[u32]| {
        // Toward a strip, the ground ends where the tool starts rolling.
        let rolls = |n: u32| usize::try_from(n).ok().and_then(|i| classes.get(i)) == Some(&ROLLING);
        let find = |a: u32, b: u32| ends_between(seat, grid, (a, b), rolls(b), tolerance);
        CutGrid::new(
            grid.clone(),
            classes.to_vec(),
            |class| class == MACHINED,
            find,
        )
    };
    let mut cut = cut_by(&classes);
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
        cut = cut_by(&classes);
    }
    cut
}

/// Each strip of `classes` over `grid` where the tool rolls over a sharp edge
/// ([`ROLLING`]) kept a break in the ground only where it is ring-shaped,
/// one piece, its nodes joined across cells, corners included, round one hole,
/// with ground all round it, [`BESIDE`] nodes deep: a break between two pieces
/// of ground. Any other is ground, as one that runs off the grid, or to where
/// the tool leaves the part, or to a face avoided, or beside ground too narrow
/// seen from above, as up a wall nearly upright.
///
/// A piece's holes are counted from the squares of four nodes round each cell,
/// the grid's own edge a cell wide of nodes not of it: with `q1` the squares
/// holding one node of it, `q3` three, and `qd` two across a corner, its
/// Euler number, pieces less holes, is `(q1 - q3 - 2 qd) / 4` (Gray's). Nought
/// is one piece round one hole.
fn keep_ring_shaped(grid: &TriGrid, classes: &mut [u32]) {
    let (piece, pieces) = strips(grid, classes);
    let between = ground_round(grid, classes, (&piece, pieces));
    let euler = euler_numbers(grid, (&piece, pieces));
    for (class, p) in classes.iter_mut().zip(&piece) {
        if *class == ROLLING && (euler.get(*p) != Some(&0) || between.get(*p) != Some(&true)) {
            *class = MACHINED;
        }
    }
}

/// Lattice node `(x, y)` of `grid`, by its index, where it is on the grid.
fn node_at(grid: &TriGrid, x: i64, y: i64) -> Option<usize> {
    let (nx, ny) = (i64::from(grid.nx()), i64::from(grid.ny()));
    (x >= 0 && y >= 0 && x < nx && y < ny)
        .then(|| usize::try_from(y * nx + x).ok())
        .flatten()
}

/// Where lattice node `i` of `grid` is, as `(x, y)`.
fn xy_of(grid: &TriGrid, i: usize) -> (i64, i64) {
    let nx = i64::from(grid.nx());
    (i as i64 % nx, i as i64 / nx)
}

/// Each node's piece of the strips of `classes` over `grid`, its nodes joined
/// across cells, corners included, or `usize::MAX`; and how many there are.
fn strips(grid: &TriGrid, classes: &[u32]) -> (Vec<usize>, usize) {
    let rolls = |i: usize| classes.get(i) == Some(&ROLLING);
    let mut piece = vec![usize::MAX; classes.len()];
    let mut pieces = 0;
    for start in 0..classes.len() {
        if !rolls(start) || piece.get(start) != Some(&usize::MAX) {
            continue;
        }
        let mut stack = vec![start];
        if let Some(p) = piece.get_mut(start) {
            *p = pieces;
        }
        while let Some(i) = stack.pop() {
            let (x, y) = xy_of(grid, i);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if let Some(j) = node_at(grid, x + dx, y + dy)
                        && rolls(j)
                        && let Some(p) = piece.get_mut(j)
                        && *p == usize::MAX
                    {
                        *p = pieces;
                        stack.push(j);
                    }
                }
            }
        }
        pieces += 1;
    }
    (piece, pieces)
}

/// For each of the `pieces` of the strips, `piece` each node's, whether the
/// ground lies all round it, [`BESIDE`] nodes deep, on `grid`.
fn ground_round(grid: &TriGrid, classes: &[u32], (piece, pieces): (&[usize], usize)) -> Vec<bool> {
    let mut round = vec![true; pieces];
    for (i, &p) in piece.iter().enumerate() {
        let Some(ok) = round.get_mut(p) else {
            continue;
        };
        let (x, y) = xy_of(grid, i);
        for dy in -BESIDE..=BESIDE {
            for dx in -BESIDE..=BESIDE {
                *ok &= node_at(grid, x + dx, y + dy).is_some_and(|j| {
                    let class = classes.get(j);
                    class == Some(&ROLLING) || class == Some(&MACHINED)
                });
            }
        }
    }
    round
}

/// Four times each of the `pieces`' Euler number, `piece` each node's, from
/// the squares of four nodes round each cell of `grid`, the grid's own edge a
/// cell wide of nodes of none. Two pieces never share a square: they would be
/// joined across its corner.
fn euler_numbers(grid: &TriGrid, (piece, pieces): (&[usize], usize)) -> Vec<i64> {
    let (nx, ny) = (i64::from(grid.nx()), i64::from(grid.ny()));
    let mut euler = vec![0i64; pieces];
    for y in -1..ny {
        for x in -1..nx {
            let corners = [(x, y), (x + 1, y), (x + 1, y + 1), (x, y + 1)].map(|(x, y)| {
                node_at(grid, x, y)
                    .and_then(|i| piece.get(i).copied())
                    .filter(|&p| p != usize::MAX)
            });
            let Some(p) = corners.iter().flatten().next().copied() else {
                continue;
            };
            let held = corners.iter().flatten().count();
            let [first, _, opposite, _] = corners;
            let across = held == 2 && first.is_some() == opposite.is_some();
            if let Some(e) = euler.get_mut(p) {
                *e += match held {
                    1 => 1,
                    3 => -1,
                    2 if across => -2,
                    _ => 0,
                };
            }
        }
    }
    euler
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
/// along it. Toward a strip where the tool rolls over a sharp edge (`rolls`),
/// where it starts rolling. NaN where either end is off the grid or `a` is off
/// the part.
fn ends_between(
    seat: &Seat<'_>,
    grid: &impl TriSurface,
    (a, b): (u32, u32),
    rolls: bool,
    tolerance: f64,
) -> DVec3 {
    let mut scratch = Vec::new();
    let (Some(pa), Some(pb)) = (grid.xy(a), grid.xy(b)) else {
        return DVec3::NAN;
    };
    let rest = |p: DVec2, scratch: &mut Vec<u32>| seat.rest_in(p.x, p.y, scratch);
    let Some((start, _)) = rest(pa, &mut scratch) else {
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
            Some((r, rolling)) if r.machined == start.machined && !(rolls && rolling) => {
                moved = (r.z - here.z).abs();
                (near, here) = (mid, r);
            }
            r => (away, there) = (mid, r.map(|(r, _)| r)),
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
    let band = cut.cut(first).map_or(NO_BAND, |c| c.bands.0);
    if chain.iter().all(|&c| past(c) == past(first)) {
        return vec![Edge {
            nodes: chain.to_vec(),
            points: chain.iter().filter_map(|&c| at(c)).collect(),
            closed,
            past: past(first),
            band,
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
                    band,
                });
            }
        }
        last = Some(class);
    }
    out
}

/// The length along the surface the tool rides of each bent edge of `grid`
/// ([`guided::bent`]), by its key, lower node first: the straight line
/// between the nodes cuts the corner where a wall turns onto the ground above
/// or below it inside the cell, by up to the cell's width, and the rings
/// beside it would be that much too far apart.
/// Edges with an end where the ground ends against a strip are measured
/// within their ends' heights ([`guided::bent`]).
fn cliff_edges(seat: &Seat<'_>, cut: &CutGrid, tolerance: f64) -> HashMap<(u32, u32), f64> {
    let grid = &cut.view(None);
    let found: Vec<((u32, u32), f64)> = (0..grid.nodes())
        .into_par_iter()
        .map_init(Vec::new, |scratch, a| {
            let mut out = Vec::new();
            let Some(pa) = grid.point(a) else {
                return out;
            };
            let mut seen = Vec::new();
            for (_, nodes) in grid.triangles_at(a) {
                for b in nodes {
                    let Some(pb) = grid.point(b).filter(|_| b > a && !seen.contains(&b)) else {
                        continue;
                    };
                    seen.push(b);
                    let strip = against_a_strip(cut, a) || against_a_strip(cut, b);
                    if let Some(line) = guided::bent(seat, (pa, pb), tolerance, strip, scratch) {
                        out.push(((a, b), length(&line)));
                    }
                }
            }
            out
        })
        .flatten_iter()
        .collect();
    found.into_iter().collect()
}

/// Whether node `n` of `cut` is a cut where the ground ends against a strip.
fn against_a_strip(cut: &CutGrid, n: u32) -> bool {
    cut.cut(n).is_some_and(|c| cut.class(c.edge.1) == ROLLING)
}
