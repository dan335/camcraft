//! Scallop finishing: passes a constant distance apart along the surface, offset
//! in from the boundary. Fusion's Scallop.
//!
//! Parallel finishing lays its passes a stepover apart seen from above, so on a
//! slope they are further apart along the surface, and the cusps between them
//! taller, the steeper it is. Scallop's are a stepover apart along the surface
//! everywhere, so the cusp is the same everywhere.
//!
//! # A field of distances, and its lines
//!
//! Where the tool sits over the part (its cutter location, or CL) is sampled on a
//! grid by exact drop-cutter queries, and the distance along that surface from the
//! boundary worked out across it by fast marching ([`camcraft_geom::geodesic`]).
//! The passes are the lines where that distance is a whole number of stepovers
//! ([`camcraft_geom::isoline`]). The grid decides where a pass runs, seen from
//! above, and nothing else: decision 0009, a narrow exception to 0003.
//!
//! - The first pass is the boundary itself: the machining boundary, seen from
//!   above, and where the tool leaves the part and where it leaves a face it
//!   machines for faces it avoids alone, each found exactly along the grid's
//!   edges and the grid's cells cut there, so the field runs right to them
//!   (decision 0037).
//! - Every point of every pass is an exact cutter location, refined along the
//!   pass until it is within tolerance of the part, as a parallel pass is
//!   ([`crate::strategy::parallel::sweep_polyline`]).
//! - Nothing is smoothed. A line is only thinned where a point is within a
//!   quarter of the tolerance, and a twentieth of the grid's pitch, of the line
//!   through its neighbours.
//!
//! # Steep ground (decision 0035)
//!
//! The passes cut every slope themselves, walls included. Where a line crosses
//! a grid edge its height is as well known as its place along it, so on ground
//! steeper than 45° a point is found round the wall at that height, across the
//! slope, rather than dropped where it falls seen from above (`guided`). The
//! field measures a grid edge down a cliff along the surface the tool rides,
//! from drops along it, not by the straight line that cuts its corner. What the
//! tool cannot reach from above, down a part's outside below where it last
//! touches it, the operation finishes with level rings
//! ([`ScallopReport::walls`]). The slope limits leave out what is outside the
//! range asked for.
//!
//! # Top-ups
//!
//! Where the passes close in on a crest, the last one round it may be nearly two
//! stepovers from the one on its other side, and the cusp between four times as
//! tall. A region rising more than half a stepover past its last pass gets one more,
//! halfway up: Fusion's passes run on until they collapse, and this is where they
//! do.
//!
//! # Order and direction
//!
//! The regions each pass bounds nest, and are cut region by region, the nearest
//! next: from the boundary in (outside in), or each region's inner ones first
//! (inside out). The edges of faces avoided are edges of the field, as the
//! area's holes are: it is not measured across what is avoided. Inside out,
//! where the ground has inner edges, the field is measured out from them alone
//! and cut in its own order, the first pass along them. Climbing with a clockwise
//! spindle has the side not yet finished on the tool's right. Both ways keeps
//! closed rings climbing, and starts an open pass at whichever end is nearer.
//! Each closed ring starts where it crosses the line out from where the first
//! started, so the seam where each steps to the next is straight seen from
//! above.

use std::collections::HashMap;

use camcraft_geom::contour::{Contour, Region, polyline_length};
use camcraft_geom::cut::{CutGrid, NO_BAND};
use camcraft_geom::geodesic::{Seed, march_along};
use camcraft_geom::isoline::{IsoLine, nest};
use camcraft_geom::shadow::Shadow;
use camcraft_geom::trigrid::TriSurface;
use glam::{DVec2, DVec3};
use rayon::prelude::*;

use crate::control::{Watch, share};
use crate::cutter::surfaces::{Rest, Seat};
use crate::cutter::tool::ToolProfile;
use crate::path::toolpath::Toolpath;
use crate::strategy::adaptive::Direction;
use crate::strategy::field::{self, AreaGrid, OFF_PART, count, level_lines};
pub use crate::strategy::nesting::Order;
use crate::strategy::nesting::{Cuts, Nested, distance, take_nearest};
use crate::strategy::parallel::{ParallelParams, ParallelReport, Run, sweep_polyline};
use crate::strategy::projected::Emit;
use crate::strategy::slope::{self, SlopeCounts, Sloped};
use crate::strategy::{StrategyError, check, with_edge};
use camcraft_geom::contour::simplify;

/// The most levels of passes: far past any real part.
const MAX_LEVELS: u32 = 100_000;

/// How to lay out the passes.
#[derive(Debug, Clone, PartialEq)]
pub struct ScallopParams {
    /// Distance between neighbouring passes along the surface, in millimetres.
    pub stepover: f64,
    /// Chordal tolerance along each pass, in millimetres.
    pub tolerance: f64,
    /// Cutting feed, in mm/min.
    pub feed: f64,
    /// Largest Z drop, in millimetres, that may be cut rather than linked.
    pub max_cut_step: f64,
    /// Which way round.
    pub order: Order,
    /// Which way each pass cuts.
    pub direction: Direction,
    /// Whether climbing puts the side still to finish on the tool's right, as it
    /// does with a clockwise spindle.
    pub climb_right: bool,
    /// Where the surface is at least this steep, in radians from level.
    pub slope_from: f64,
    /// And at most this steep.
    pub slope_to: f64,
    /// The most grid nodes the field may have; past it the grid is coarsened.
    pub max_nodes: u32,
    /// How steeply, in radians from level, the tool goes down along a ring
    /// onto it where going straight down onto it from the ring above would
    /// plunge further than `max_cut_step`, as down a wall.
    pub ramp: f64,
    /// What it tells how far it has got, and asks whether to stop.
    pub watch: Watch,
}

impl Default for ScallopParams {
    fn default() -> Self {
        Self {
            stepover: 0.5,
            tolerance: 0.01,
            feed: 1000.0,
            max_cut_step: 0.5,
            order: Order::OutsideIn,
            direction: Direction::BothWays,
            climb_right: true,
            slope_from: 0.0,
            slope_to: std::f64::consts::FRAC_PI_2,
            max_nodes: 1 << 23,
            ramp: 2f64.to_radians(),
            watch: Watch::default(),
        }
    }
}

/// What happened while laying out the passes.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ScallopReport {
    /// Passes cut.
    pub passes: u64,
    /// Lines of equal distance found, the boundary's included.
    pub lines: u64,
    /// Levels of passes in from the boundary.
    pub levels: u64,
    /// Regions given a pass halfway up to their crest.
    pub top_ups: u64,
    /// Grid nodes the field was worked out on.
    pub nodes: u64,
    /// Their spacing, in millimetres.
    pub pitch: f64,
    /// Whether the grid was coarsened to stay within its budget.
    pub coarsened: bool,
    /// Level rings a stepover apart down the walls below where the tool rests
    /// from above, which the field cannot cover: by the operation that runs
    /// it, which has the part and its Bottom to find them on.
    pub walls: u64,
    /// Stretches left out for being outside the slope range asked for.
    pub outside_slope: u64,
    /// Stretches too short to be worth cutting.
    pub fragments: u64,
    /// Nodes inside the boundary the field never reached.
    pub unreached: u64,
    /// The highest point of where the tool leaves the part from above, if it
    /// does inside the area: the level rings below start a stepover under it.
    pub leaves_at: Option<f64>,
    /// What sampling the passes on the part found, as for parallel finishing.
    pub sweep: ParallelReport,
}

impl ScallopReport {
    /// Whether anything needs the caller's attention.
    pub fn is_clean(&self) -> bool {
        self.sweep.is_clean() && self.unreached == 0
    }
}

/// Where the tool touches the part: within its widest radius of the part's
/// `shadow` seen from above, a tolerance short so as never to reach past it.
/// Fusion's *contact only*.
pub fn contact_region(
    shadow: &Shadow,
    max_radius: f64,
    tolerance: f64,
) -> Result<Region, StrategyError> {
    shadow
        .grown((max_radius - tolerance).max(tolerance), tolerance * 0.5)
        .map_err(StrategyError::from)
}

/// Up to a tolerance past where the tool touches the part: within its widest
/// radius of the part's `shadow` seen from above, and a tolerance more. For
/// a pattern that finds where the tool leaves the part itself, exactly, as
/// scallop's field does: drawn short of it, the area's edge would be a
/// tolerance short, down a wall's vertical fall to where the tool leaves the
/// part a millimetre or more of height.
pub fn past_contact(
    shadow: &Shadow,
    max_radius: f64,
    tolerance: f64,
) -> Result<Region, StrategyError> {
    shadow
        .grown(max_radius + tolerance, tolerance * 0.5)
        .map_err(StrategyError::from)
}

/// Scallop passes over `area`, where the tool's centre may go and touches the part.
///
/// The path holds [`MoveRole::Cut`] passes separated by [`MoveRole::Link`] moves, in
/// the order they are cut. The linker resolves them.
pub fn scallop_finish(
    seat: &Seat<'_>,
    area: &Region,
    params: &ScallopParams,
) -> Result<(Toolpath, ScallopReport), StrategyError> {
    check("stepover", params.stepover)?;
    check("tolerance", params.tolerance)?;
    check("feed", params.feed)?;
    check("max_cut_step", params.max_cut_step)?;
    slope::check_range(params.slope_from, params.slope_to)?;
    let mut report = ScallopReport::default();
    let Some((lo, hi)) = area.bounds() else {
        return Ok((Toolpath::new(), report));
    };
    let s = params.stepover;

    let mut field = Field::build(seat, area, lo, hi, params, &mut report)?;
    report.leaves_at = field.leaves_at;
    let beyond = field.beyond.take();
    let outward = field.outward;
    let view = field.cut.view(None);
    let grid = &view;
    let d = &field.d;
    let on_edge = with_edge(area, params.tolerance);
    // Measured out from the inner edges, the field runs on to the boundary at
    // any distance, so some of its lines near it are slivers along it, in bits
    // where the distance there wavers. A line all of whose points are within
    // half a stepover of the boundary along the surface is left to the
    // boundary's ring, cut last; any other is kept whole, cutting none into
    // bits where it runs along the boundary at about that distance.
    let far_enough = |(points, _): &(Vec<DVec2>, bool)| match &beyond {
        Some(beyond) => points.iter().any(|&p| {
            let value = |n: u32| {
                usize::try_from(n)
                    .ok()
                    .and_then(|i| beyond.get(i))
                    .copied()
                    .filter(|v| v.is_finite())
            };
            // A line's point is on a grid edge: as far as the field says
            // along it, whichever of the triangles beside it is whole.
            let along = guided::edge_at(grid, p).and_then(|(a, b)| {
                let (pa, pb) = (grid.xy(a)?, grid.xy(b)?);
                let t = (p.distance(pa) / pa.distance(pb)).clamp(0.0, 1.0);
                Some(value(a)? + t * (value(b)? - value(a)?))
            });
            along
                .or_else(|| grid.interpolate(p, value))
                .is_none_or(|v| v >= 0.5 * s)
        }),
        None => true,
    };

    // The lines at each level, found level by level in parallel; at 0, the
    // edges of the ground avoided the field is measured out from.
    let top = d
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(0.0, f64::max);
    let levels = count((top / s).floor()).unwrap_or(0).min(MAX_LEVELS);
    let mut level_lines = level_lines(grid, d, s, levels);
    match level_lines.first_mut() {
        Some(first) => first.clear(),
        None => level_lines.push(Vec::new()),
    }
    let mut regions = nest(grid, d, s, &level_lines);
    report.levels = u64::from(levels);
    report.lines = level_lines.iter().map(|l| l.len() as u64).sum();
    report.top_ups = regions.iter().filter(|r| !r.top_up.is_empty()).count() as u64;

    // The nodes of the tree the passes are cut by: one a region, one for each
    // region's top-up, and one for each piece of the area with no region of its
    // own. Each with the lines it cuts, and the slope of each segment: each line
    // bounds one region, so is taken by one.
    let mut nodes: Vec<Plan> = regions
        .iter()
        .map(|r| {
            let lines = level_lines
                .get_mut(usize::try_from(r.level).unwrap_or(usize::MAX))
                .map(|all| {
                    r.lines
                        .iter()
                        .filter_map(|&i| all.get_mut(i).map(|l| sloped(grid, l)))
                        .collect()
                })
                .unwrap_or_default();
            Plan {
                lines,
                edges: Vec::new(),
                found: Vec::new(),
                children: r.children.clone(),
                root: r.level == 0,
            }
        })
        .collect();
    for (i, r) in regions.iter_mut().enumerate() {
        if r.top_up.is_empty() {
            continue;
        }
        let id = nodes.len();
        nodes.push(Plan {
            lines: r.top_up.iter_mut().map(|l| sloped(grid, l)).collect(),
            edges: Vec::new(),
            found: Vec::new(),
            children: Vec::new(),
            root: false,
        });
        if let Some(plan) = nodes.get_mut(i) {
            plan.children.push(id);
        }
    }
    // The boundary's rings, to the level-0 region of the piece they bound, as
    // the ground's edges found in it are: but measured out from the inner
    // edges, its outer rings, and where the tool leaves the part, are the far
    // end, cut last, each a root of its own.
    let mut last = Vec::new();
    let found = |edges: &[(Vec<DVec3>, bool)], piece: &Region| -> Vec<Sloped> {
        edges
            .iter()
            .filter(|(points, _)| points.first().is_some_and(|p| piece.contains(p.truncate())))
            .map(|(points, closed)| {
                let flat: Vec<DVec2> = points.iter().map(|p| p.truncate()).collect();
                boundary(grid, &flat, *closed)
            })
            .collect()
    };
    for piece in area.pieces() {
        let (outer, inner): (Vec<&Contour>, Vec<&Contour>) = piece
            .rings()
            .iter()
            .partition(|c| !outward || c.signed_area() > 0.0);
        let ring = |rings: Vec<&Contour>| -> Vec<Sloped> {
            rings
                .iter()
                .map(|c| boundary(grid, c.points(), true))
                .collect()
        };
        let (home_rings, far_rings) = if outward {
            (ring(inner), ring(outer))
        } else {
            (ring(outer), Vec::new())
        };
        let (home_found, far_found) =
            (found(&field.edges, &piece), found(&field.far_edges, &piece));
        report.lines +=
            (home_rings.len() + far_rings.len() + home_found.len() + far_found.len()) as u64;
        if !far_rings.is_empty() || !far_found.is_empty() {
            last.push(nodes.len());
            nodes.push(Plan {
                lines: Vec::new(),
                edges: far_rings,
                found: far_found,
                children: Vec::new(),
                root: false,
            });
        }
        if home_rings.is_empty() && home_found.is_empty() {
            continue;
        }
        let home = regions
            .iter()
            .position(|r| r.level == 0 && grid.xy(r.rep).is_some_and(|p| piece.contains(p)));
        match home.and_then(|i| nodes.get_mut(i)) {
            Some(plan) => {
                plan.edges.extend(home_rings);
                plan.found.extend(home_found);
            }
            None => nodes.push(Plan {
                lines: Vec::new(),
                edges: home_rings,
                found: home_found,
                children: Vec::new(),
                root: true,
            }),
        }
    }

    // Each node's lines, kept to the slope range and the area, sampled on the part.
    let sweep = ParallelParams {
        stepover: s,
        tolerance: params.tolerance,
        feed: params.feed,
        max_cut_step: params.max_cut_step,
        ..Default::default()
    };
    let thin = (0.25 * params.tolerance).min(0.05 * grid.pitch());
    // The field and its lines are done: what is left is laying each node's
    // lines on the part, an equal share of the rest each.
    params.watch.check()?;
    params.watch.report(0.4);
    let each = params.watch.part(0.4, 1.0);
    let share = share(1, nodes.len());
    let worked: Vec<Result<Worked, StrategyError>> = nodes
        .par_iter_mut()
        .map(|plan| {
            params.watch.check()?;
            let mut w = Worked::default();
            // The field's lines, and the area's edge, each kept to the slopes.
            let kept = |lines: Vec<Sloped>, w: &mut Worked| {
                lines
                    .into_iter()
                    .flat_map(|line| {
                        line.within(
                            params.slope_from,
                            params.slope_to,
                            params.slope_to,
                            &mut w.slopes,
                        )
                    })
                    .collect::<Vec<_>>()
            };
            let lines = kept(std::mem::take(&mut plan.lines), &mut w);
            let edges = kept(std::mem::take(&mut plan.edges), &mut w);
            let found = kept(std::mem::take(&mut plan.found), &mut w);
            // The field's lines, and the edges of the ground found on its grid,
            // placed by it; the area's edge, seen from above.
            for (stretches, of_field, by_field) in [
                (lines, true, true),
                (found, false, true),
                (edges, false, false),
            ] {
                // The area's own edge: a point on it is in, or clipping it to
                // the area cuts it into bits ([`with_edge`]).
                let stretches: Vec<(Vec<DVec2>, bool)> = if of_field {
                    stretches.into_iter().filter(&far_enough).collect()
                } else {
                    stretches
                };
                for (points, closed) in on_edge.clip_lines(&stretches)? {
                    if !closed && polyline_length(&points) < 2.0 * grid.pitch() {
                        w.fragments += 1;
                        continue;
                    }
                    if by_field {
                        // Placed by the field, height and all: round a wall at
                        // its height where the ground is steep.
                        let line = (points.as_slice(), closed);
                        for run in guided::sweep(seat, grid, line, thin, &sweep, &mut w.sweep) {
                            // Broken where the part ends under the tool, along
                            // the area's edge, a line may leave bits shorter
                            // than the tool's radius: dimples between the
                            // passes either side, each lifted to and from.
                            let along: f64 = run
                                .points
                                .iter()
                                .zip(run.points.iter().skip(1))
                                .map(|(a, b)| a.distance(*b))
                                .sum();
                            if run.closed || along >= seat.tool().max_radius() {
                                w.runs.push(run);
                            } else {
                                w.fragments += 1;
                            }
                        }
                    } else {
                        // Placed seen from above: the tool dropped along it.
                        let points = simplify(&points, closed, thin);
                        w.runs
                            .extend(sweep_polyline(seat, &points, closed, &sweep, &mut w.sweep));
                    }
                }
            }
            if params.watch.showing() {
                params.watch.show(w.runs.iter().map(Run::shown));
            }
            each.add(share);
            Ok(w)
        })
        .collect();
    // Measured from the outer boundary, inside out is each region's inner
    // passes first; measured out from the inner edges, it is the field's own
    // order, as outside in is from the boundary.
    let toward_higher = params.order == Order::OutsideIn || outward;
    let mut tree = Nested::new(!toward_higher);
    let mut roots = Vec::new();
    for (plan, w) in nodes.into_iter().zip(worked) {
        let w = w?;
        report.outside_slope += w.slopes.outside_slope;
        report.fragments += w.fragments;
        report.sweep += w.sweep;
        let i = tree.push(w.runs);
        // A child nested before it is pushed: only the parent must be there.
        for c in plan.children {
            tree.nest(i, c);
        }
        if plan.root {
            roots.push(i);
        }
    }
    tree.settle(|r| r.points.first().map(|p| p.truncate()));

    // Cut from the roots, the nearest next.
    let conventional = params.direction == Direction::Conventional;
    // The higher side of a line as found is on its left: the side still to
    // finish, cutting toward the higher.
    let reverse = toward_higher == (params.climb_right != conventional);
    let mut emit = Emit::new(params.feed);
    // Each ring started where it crosses the line out from where the first
    // started to the side still to finish, so the seam where each steps to
    // the next is straight seen from above.
    emit.seam_left = Some(toward_higher != reverse);
    // Where the seam runs straight down a wall, the way from one ring onto
    // the next below is a plunge further than a cut step: down along the ring
    // instead, at the linking's default ramp angle, as contour finishing goes
    // from level to level, onto a ring that starts within half the tool of
    // where the last ended.
    emit.ramp = Some((params.ramp, 0.5 * seat.tool().max_radius()));
    emit.ramp_past = Some(params.max_cut_step);
    let mut cut = Cut {
        emit,
        reverse,
        both_ways: params.direction == Direction::BothWays,
        seat,
        tolerance: params.tolerance,
    };
    for mut roots in [roots, last] {
        loop {
            let at = cut.emit.at;
            let Some(root) = take_nearest(&mut roots, |&r| distance(tree.anchor(r), at)) else {
                break;
            };
            tree.cut(root, &mut cut)?;
        }
    }
    report.passes = cut.emit.passes;
    Ok((cut.emit.path, report))
}

/// The class of a node the tool rests on a face it machines at.
const MACHINED: u32 = 0;

/// The class of a node the tool rests on faces it avoids alone at.
const AVOIDED: u32 = 1;

/// How many halvings a change in what the tool rests on is found in along a
/// grid edge, at most: far past any tolerance.
const MAX_HALVINGS: u32 = 48;

/// How many times, at most, islands finer than the grid are taken into the
/// ground round them and the grid cut again: once takes every island there
/// is, and taking one in makes no other.
const MAX_ISLAND_ROUNDS: u32 = 4;

/// Whether ground of `class` is the field's: what the tool rests on there
/// is a face it machines.
fn machined(class: u32) -> bool {
    class == MACHINED
}

/// The field of distances along the surface from the edges it is measured
/// from, on its grid cut where the ground it machines ends.
struct Field {
    /// The grid, its cells cut where the tool goes from resting on a face it
    /// machines to resting on faces it avoids alone, or off the part.
    cut: CutGrid,
    /// Each node's distance, of the grid cut's: negative outside the area,
    /// infinite where unknown.
    d: Vec<f64>,
    /// Measured out from the inner edges, each node's distance along the
    /// surface from the outer boundary.
    beyond: Option<Vec<f64>>,
    /// Whether it is measured out from the ground's inner edges alone: inside
    /// out, where there are any.
    outward: bool,
    /// The ground's edges the field is measured from, each a line through
    /// its cuts, closed or not: where faces avoided begin, and, unless
    /// measured out from those, where the tool leaves the part.
    edges: Vec<(Vec<DVec3>, bool)>,
    /// Measured out from the inner edges, where the tool leaves the part:
    /// the field's far end, cut last.
    far_edges: Vec<(Vec<DVec3>, bool)>,
    /// The highest point of where the tool leaves the part from above.
    leaves_at: Option<f64>,
}

impl Field {
    fn build(
        seat: &Seat<'_>,
        area: &Region,
        lo: DVec2,
        hi: DVec2,
        params: &ScallopParams,
        report: &mut ScallopReport,
    ) -> Result<Self, StrategyError> {
        let s = params.stepover;
        let tolerance = params.tolerance;
        // A third of a stepover, and fine enough for the tool's own curvature;
        // coarsened, if it must be, to stay within the budget, and no further
        // than half a stepover. The tool's height at each node, dropped
        // exactly, and whether a face it machines holds it there.
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

        // The ground it machines ends where the tool goes from resting on a
        // face it machines to resting on faces it avoids alone, and where it
        // leaves the part: each found along the grid's edges by halving, to a
        // hundredth of the tolerance seen from above, and until the height
        // the tool rests at there moves by less than that from one halving to
        // the next. Where the tool leaves the part, the way down to the edge
        // is steep as a square root, and the height is what matters: the
        // last ring is laid along it.
        let grid = &on.grid;
        let find = |a: u32, b: u32| {
            let mut scratch = Vec::new();
            let (Some(pa), Some(pb)) = (grid.xy(a), grid.xy(b)) else {
                return DVec3::NAN;
            };
            let rest = |p: DVec2, scratch: &mut Vec<u32>| seat.rest_in(p.x, p.y, scratch);
            let Some(start) = rest(pa, &mut scratch) else {
                return DVec3::NAN;
            };
            let (mut near, mut away, mut moved) = (pa, pb, f64::INFINITY);
            let (mut here, mut there) = (start, None);
            for _ in 0..MAX_HALVINGS {
                if near.distance(away) <= 0.01 * tolerance && moved <= 0.01 * tolerance {
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
            // Closed in on where the tool's path stands upright, its height
            // jumping a hair across: beside a wall, seen from above. Dropped
            // there it rests at the foot or at the top, whichever side of the
            // jump it lands, and finds nothing between; moved up beside the
            // wall it touches the face it rises beside ([`Seat::beside`]).
            // The stretch is that face's: machined, as a pocket's wall above
            // a floor avoided, the change at the foot of it and the wall the
            // field's; avoided, at the top of it. Where nothing is within
            // the tool's radius of it at its widest, under an overhang, it
            // touches the overhang above with its side: the top's.
            if let Some(there) = there
                && near.distance(away) <= 0.01 * tolerance
                && (there.z - here.z).abs() > tolerance
            {
                let (foot, top) = if here.z < there.z {
                    (here, there)
                } else {
                    (there, here)
                };
                let middle = 0.5 * (foot.z + top.z);
                let face = seat
                    .upright_toward(near, middle, tolerance, &mut scratch)
                    .and_then(|toward| seat.beside(near, toward, middle))
                    .map_or(top.machined, |(_, machined)| machined);
                return near.extend(if face == foot.machined { top.z } else { foot.z });
            }
            near.extend(here.z)
        };
        // The grid resolves what the tool rests on to its pitch and no finer:
        // where the faces' facets turn it on and off an edge a hair either
        // side of a node, ground may come out lying wholly in cells its edges
        // cut, an island of a node or two. It is taken into the ground round
        // it, and the grid cut again.
        let mut classes = classes;
        let mut cut = CutGrid::new(on.grid.clone(), classes.clone(), machined, find);
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
            cut = CutGrid::new(on.grid.clone(), classes.clone(), machined, find);
        }
        let view = cut.view(None);
        let cliffs = cliff_edges(seat, &view, tolerance);
        params.watch.check()?;

        // The seeds: each node within a cell's diagonal of `rings` where
        // they cross the part, at its exact distance to the nearest point of
        // them, over the part, one drop a node. Off the part the area's edge
        // is no edge of the ground: it ends where the tool leaves the part,
        // found exactly, and a node up the steep way down to there measured
        // to the area's edge seen from above would be a fraction of its way
        // along the surface from it.
        let feet = |rings: Region| -> Vec<Seed> {
            on.boundary_feet(&rings)
                .par_iter()
                .map_init(Vec::new, |scratch, &(n, _, foot)| {
                    let rest = seat.drop_at_in(foot.x, foot.y, scratch)?;
                    Some(Seed {
                        node: n,
                        distance: grid.point(n)?.distance(foot.extend(rest)),
                    })
                })
                .flatten()
                .collect()
        };
        // And the ground's edges, each cut at no distance: where faces
        // avoided begin, and where the tool leaves the part.
        let lattice = cut.grid().nodes();
        let edge_of = |class: u32| -> Vec<Seed> {
            cut.cuts()
                .iter()
                .enumerate()
                .filter(|(_, c)| c.bands.1 == NO_BAND && cut.class(c.edge.1) == class)
                .map(|(i, _)| Seed {
                    node: lattice + u32::try_from(i).unwrap_or(0),
                    distance: 0.0,
                })
                .collect()
        };
        let (inner, leaving) = (edge_of(AVOIDED), edge_of(OFF_PART));
        let holes: Vec<Contour> = area
            .rings()
            .iter()
            .filter(|r| r.signed_area() <= 0.0)
            .cloned()
            .collect();
        let inside_out = params.order == Order::InsideOut;
        let outward = inside_out && (!holes.is_empty() || !inner.is_empty());
        let measure = |seeds: &[Seed]| {
            march_along(
                &view,
                seeds,
                |n| view.point(n),
                |a, b| cliffs.get(&(a.min(b), a.max(b))).copied(),
            )
            .d
        };
        let (mut d, beyond) = if outward {
            // From the inner edges; and how far the outer boundary is, along
            // the surface, which its ring finishes up to. Where the tool
            // leaves the part the field reaches exactly, its ring along it:
            // a line near it is no sliver in bits along the area's edge, and
            // left out, it would leave the way down to it, as steep as a
            // square root, wider than a stepover.
            let mut seeds = feet(Region::new(holes));
            seeds.extend(&inner);
            let outer: Vec<Contour> = area
                .rings()
                .iter()
                .filter(|r| r.signed_area() > 0.0)
                .cloned()
                .collect();
            (measure(&seeds), Some(measure(&feet(Region::new(outer)))))
        } else {
            let mut seeds = feet(area.clone());
            seeds.extend(&inner);
            seeds.extend(&leaving);
            (measure(&seeds), None)
        };
        let is_inside = |n: u32| match cut.cut(n) {
            Some(c) => area.contains(c.at.truncate()),
            None => usize::try_from(n)
                .ok()
                .and_then(|i| on.inside.get(i))
                .copied()
                .unwrap_or(false),
        };
        report.unreached = (0..lattice)
            .filter(|&n| {
                is_inside(n)
                    && machined(cut.class(n))
                    && view.triangles_at(n).next().is_some()
                    && !usize::try_from(n)
                        .ok()
                        .and_then(|i| d.get(i))
                        .is_some_and(|v| v.is_finite())
            })
            .count() as u64;
        // Outside the area, negative, so only the lines inside it are
        // passes. Measured out from the inner edges, the field runs on past
        // the area's outer edge from inside it, and its lines are cut there:
        // it does not fall to 0 there.
        if !outward {
            for (n, v) in (0u32..).zip(d.iter_mut()) {
                if !is_inside(n) {
                    *v = -*v;
                }
            }
        }
        // The edges as lines through their cuts, each piece of one by what
        // lies past it.
        let mut edges = Vec::new();
        let mut far_edges = Vec::new();
        for b in 0..cut.bands() {
            for (chain, closed) in cut.edges(b) {
                for (points, closed, past) in pieces(&cut, &chain, closed) {
                    if past == OFF_PART && outward {
                        far_edges.push((points, closed));
                    } else if past == AVOIDED || past == OFF_PART {
                        edges.push((points, closed));
                    }
                }
            }
        }
        let leaves_at = cut
            .cuts()
            .iter()
            .filter(|c| {
                c.bands.1 == NO_BAND
                    && cut.class(c.edge.1) == OFF_PART
                    && area.contains(c.at.truncate())
            })
            .map(|c| c.at.z)
            .filter(|z| z.is_finite())
            .reduce(f64::max);
        Ok(Self {
            cut,
            d,
            beyond,
            outward,
            edges,
            far_edges,
            leaves_at,
        })
    }
}

/// `chain`, a line through cuts of `cut` with the ground on its left,
/// closed back to its first if `closed`, in pieces by what lies past each
/// cut: each piece its cuts' places, whether it is closed, and that.
fn pieces(cut: &CutGrid, chain: &[u32], closed: bool) -> Vec<(Vec<DVec3>, bool, u32)> {
    let past = |n: u32| cut.cut(n).map(|c| cut.class(c.edge.1));
    let at = |n: u32| cut.cut(n).map(|c| c.at);
    let n = chain.len();
    let same = (0..n).all(|i| {
        past(chain.get(i).copied().unwrap_or(0)) == past(chain.first().copied().unwrap_or(0))
    });
    if same {
        let points: Vec<DVec3> = chain.iter().filter_map(|&c| at(c)).collect();
        let class = chain.first().and_then(|&c| past(c)).unwrap_or(OFF_PART);
        return vec![(points, closed, class)];
    }
    // A closed chain started where what lies past it changes, so no piece
    // runs round its join.
    let start = if closed {
        (0..n)
            .find(|&i| {
                past(chain.get(i).copied().unwrap_or(0))
                    != past(chain.get((i + n - 1) % n).copied().unwrap_or(0))
            })
            .unwrap_or(0)
    } else {
        0
    };
    let order: Vec<u32> = (0..n)
        .filter_map(|k| chain.get((start + k) % n).copied())
        .collect();
    let mut out: Vec<(Vec<DVec3>, bool, u32)> = Vec::new();
    let mut last: Option<u32> = None;
    for (i, &c) in order.iter().enumerate() {
        let class = past(c).unwrap_or(OFF_PART);
        let Some(p) = at(c) else { continue };
        match out.last_mut() {
            Some(piece) if last == Some(class) => piece.0.push(p),
            _ => {
                // Each piece begins where the last ended, so none leaves a gap.
                let mut points = Vec::new();
                if i > 0
                    && let Some(prev) = order.get(i - 1).and_then(|&q| at(q))
                {
                    points.push(prev);
                }
                points.push(p);
                out.push((points, false, class));
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
fn cliff_edges(
    seat: &Seat<'_>,
    grid: &(impl TriSurface + Sync),
    tolerance: f64,
) -> HashMap<(u32, u32), f64> {
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
                    if let Some(line) = guided::bent(seat, (pa, pb), tolerance, scratch) {
                        out.push(((a, b), guided::length(&line)));
                    }
                }
            }
            out
        })
        .flatten_iter()
        .collect();
    found.into_iter().collect()
}

/// A line of the field, its points taken, with its triangles' slopes.
fn sloped(grid: &impl TriSurface, line: &mut IsoLine) -> Sloped {
    Sloped {
        own: vec![true; line.points.len()],
        points: std::mem::take(&mut line.points),
        closed: line.closed,
        slopes: line.triangles.iter().map(|&t| grid.slope(t)).collect(),
    }
}

/// A ring of the boundary, split so no segment is longer than the grid's pitch,
/// with the slope under the middle of each.
fn boundary(grid: &(impl TriSurface + Sync), ring: &[DVec2], closed: bool) -> Sloped {
    Sloped::split(ring, closed, grid.pitch(), |m, _| {
        grid.locate(m).and_then(|t| grid.slope(t))
    })
}

/// A node of the tree before its lines are sampled.
struct Plan {
    /// The field's lines, placed by it, height and all.
    lines: Vec<Sloped>,
    /// The area's edge, placed seen from above.
    edges: Vec<Sloped>,
    /// The ground's edges found on the field's grid, where faces avoided
    /// begin and where the tool leaves the part, placed by it as its lines
    /// are.
    found: Vec<Sloped>,
    children: Vec<usize>,
    root: bool,
}

/// A node's lines, sampled, and what was left out.
#[derive(Default)]
struct Worked {
    runs: Vec<Run>,
    sweep: ParallelReport,
    slopes: SlopeCounts,
    fragments: u64,
}

/// Cutting the passes into the path, in order, from where the tool is.
struct Cut<'s> {
    emit: Emit,
    /// Whether a line as found is reversed to cut the way asked for.
    reverse: bool,
    /// Whether an open pass may be started at either end.
    both_ways: bool,
    /// The part, to put a point of each ring where it crosses the seam.
    seat: &'s Seat<'s>,
    tolerance: f64,
}

impl Cut<'_> {
    /// `ring`, closed, with a point of its own where it crosses the seam
    /// ([`Seam::crossing`](crate::strategy::projected::Seam::crossing)), so
    /// it starts on it: on the chord there, raised onto the part where the
    /// chord dips into it, as it may by the tolerance. Where the tool there
    /// would be below where it rests, beside a wall the chord bows into, it
    /// starts at the nearer of the chord's ends instead.
    fn on_the_seam(&self, ring: &mut Run) {
        let (Some(seam), Some(at)) = (self.emit.seam(), self.emit.at) else {
            return;
        };
        let Some((i, t)) = seam.crossing(ring, at) else {
            return;
        };
        let n = ring.points.len();
        let (Some(&a), Some(&b)) = (ring.points.get(i), ring.points.get((i + 1) % n)) else {
            return;
        };
        let q = a.lerp(b, t);
        if q.distance(a) < 1e-6 || q.distance(b) < 1e-6 {
            return;
        }
        match self.seat.drop_at(q.x, q.y) {
            Some(rest) if rest > q.z + self.tolerance => {}
            rest => ring
                .points
                .insert(i + 1, q.truncate().extend(rest.map_or(q.z, |z| z.max(q.z)))),
        }
    }
}

impl Cuts<Run> for Cut<'_> {
    type Error = StrategyError;

    fn at(&self) -> Option<DVec2> {
        self.emit.at
    }

    /// Runs the same distance in, the nearest next, each the way it cuts.
    fn cut(&mut self, mut runs: Vec<Run>) -> Result<(), StrategyError> {
        for run in &mut runs {
            if self.reverse {
                run.points.reverse();
            }
            if run.closed {
                self.on_the_seam(run);
            }
        }
        self.emit.nearest_first(runs, self.both_ways)
    }
}

mod guided;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)]
mod tests;
