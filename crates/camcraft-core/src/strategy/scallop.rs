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
//! - The first pass is the boundary itself: the machining boundary, and where the
//!   tool stops touching the part, from the mesh's shadow. Not from the grid.
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

use std::collections::{HashMap, HashSet};

use camcraft_geom::contour::{Contour, Region, polyline_length};
use camcraft_geom::geodesic::{Seed, march_along};
use camcraft_geom::isoline::{self, IsoLine, nest};
use camcraft_geom::shadow::Shadow;
use camcraft_geom::trigrid::TriGrid;
use glam::{DVec2, DVec3};
use rayon::prelude::*;

use crate::control::{Watch, share};
use crate::cutter::surfaces::Seat;
use crate::cutter::tool::ToolProfile;
use crate::path::toolpath::Toolpath;
use crate::strategy::adaptive::Direction;
use crate::strategy::field::{self, AreaGrid, count, level_lines};
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
    let outward = field.outward;
    let grid = &field.grid;
    let d = &field.d;
    let on_edge = with_edge(area, params.tolerance);
    // Measured out from the inner edges, the field runs on to the boundary at
    // any distance, so some of its lines near it are slivers along it, in bits
    // where the distance there wavers. A line all of whose points are within
    // half a stepover of the boundary along the surface is left to the
    // boundary's ring, cut last; any other is kept whole, cutting none into
    // bits where it runs along the boundary at about that distance.
    let beyond = field.beyond.take();
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
        Some(first) => *first = std::mem::take(&mut field.edges),
        None => level_lines.push(std::mem::take(&mut field.edges)),
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
            // At 0, the edges of the ground avoided, each point where the
            // edge was found: placed seen from above, as the area's edge is.
            let (lines, edges) = if r.level == 0 {
                (Vec::new(), lines)
            } else {
                (lines, Vec::new())
            };
            Plan {
                lines,
                edges,
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
            children: Vec::new(),
            root: false,
        });
        if let Some(plan) = nodes.get_mut(i) {
            plan.children.push(id);
        }
    }
    // The boundary's rings, to the level-0 region of the piece they bound: but
    // measured out from the inner edges, its outer rings are the far end, cut
    // last, each a root of its own.
    let mut last = Vec::new();
    for piece in area.pieces() {
        let (outer, inner): (Vec<&Contour>, Vec<&Contour>) = piece
            .rings()
            .iter()
            .partition(|c| !outward || c.signed_area() > 0.0);
        let ring = |rings: Vec<&Contour>| -> Vec<Sloped> {
            rings.iter().map(|c| boundary(grid, c.points())).collect()
        };
        let (home_rings, far_rings) = if outward {
            (ring(inner), ring(outer))
        } else {
            (ring(outer), Vec::new())
        };
        report.lines += (home_rings.len() + far_rings.len()) as u64;
        if !far_rings.is_empty() {
            last.push(nodes.len());
            nodes.push(Plan {
                lines: Vec::new(),
                edges: far_rings,
                children: Vec::new(),
                root: false,
            });
        }
        if home_rings.is_empty() {
            continue;
        }
        let home = regions
            .iter()
            .position(|r| r.level == 0 && grid.xy(r.rep).is_some_and(|p| piece.contains(p)));
        match home.and_then(|i| nodes.get_mut(i)) {
            Some(plan) => plan.edges.extend(home_rings),
            None => nodes.push(Plan {
                lines: Vec::new(),
                edges: home_rings,
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
            for (stretches, of_field) in [(lines, true), (edges, false)] {
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
                    if of_field {
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

/// The field of distances along the surface from the edges it is measured
/// from, on its grid.
struct Field {
    grid: TriGrid,
    /// Each node's distance: negative outside the ground it is measured over,
    /// infinite where unknown.
    d: Vec<f64>,
    /// Measured out from the inner edges, each node's distance along the
    /// surface from the outer boundary.
    beyond: Option<Vec<f64>>,
    /// Whether it is measured out from the ground's inner edges alone: inside
    /// out, where there are any.
    outward: bool,
    /// The edges of the ground avoided it is measured out from, its lines at 0.
    edges: Vec<IsoLine>,
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
        // A third of a stepover, and fine enough for the tool's own curvature;
        // coarsened, if it must be, to stay within the budget, and no further
        // than half a stepover. The tool's height at each node, dropped exactly.
        let on = AreaGrid::over(
            area,
            (lo, hi),
            field::pitch(s, seat.tool().max_radius()),
            s / 2.0,
            params.max_nodes,
            // The drops are most of the field's work; the field, a third of
            // the whole.
            &params.watch.part(0.0, 0.3),
            |x, y, scratch| seat.drop_at_in(x, y, scratch),
        )?;
        report.pitch = on.grid.pitch();
        report.nodes = u64::from(on.grid.nodes());
        report.coarsened = on.coarsened;
        let grid = &on.grid;

        // The ground it machines ends where the tool goes from resting on a
        // face it machines to resting on faces it avoids alone: an edge of
        // the field, as the area's are, its first pass along it, the field
        // never measured across what it avoids. Inside out, the ground's
        // inner edges, if it has any, the area's holes and those: measured
        // out from them alone, the passes are rings round them from the
        // first, outward.
        let inside_out = params.order == Order::InsideOut;
        // Drawn where the tool comes within half the tolerance of a face it
        // machines: the first pass runs along it, and what comes within the
        // tolerance of faces avoided and not of one machined is cut out
        // ([`crate::path::avoid`]), so a pass on the edge itself would be cut
        // into bits wherever it strays a little to the side avoided.
        let edge_tolerance = 0.5 * params.tolerance;
        let avoided = if seat.avoids_any() {
            avoided_nodes(seat, grid, edge_tolerance)
        } else {
            Vec::new()
        };
        let is_avoided = |n: u32| {
            usize::try_from(n)
                .ok()
                .and_then(|i| avoided.get(i))
                .copied()
                .unwrap_or(false)
        };
        let holes: Vec<Contour> = area
            .rings()
            .iter()
            .filter(|r| r.signed_area() <= 0.0)
            .cloned()
            .collect();
        let (edge_seeds, crossings) = if avoided.is_empty() {
            (Vec::new(), HashMap::new())
        } else {
            avoided_edges(seat, grid, &is_avoided, edge_tolerance)
        };
        let outward = inside_out && (!holes.is_empty() || !edge_seeds.is_empty());

        // The seeds: each node within a cell's diagonal of `rings`, at its
        // exact distance to the nearest point of them, over the part, one drop
        // a node.
        let feet = |rings: Region| -> Vec<Seed> {
            on.boundary_feet(&rings)
                .par_iter()
                .map_init(Vec::new, |scratch, &(n, flat, foot)| {
                    let rest = seat.drop_at_in(foot.x, foot.y, scratch);
                    let distance = match (grid.point(n), rest) {
                        (Some(p), Some(zf)) => p.distance(foot.extend(zf)),
                        _ => flat,
                    };
                    Seed { node: n, distance }
                })
                .collect()
        };
        params.watch.check()?;
        let seeded: HashSet<u32> = edge_seeds.iter().map(|s| s.node).collect();
        let cliffs = cliff_edges(seat, grid, params.tolerance);
        // Not measured across the ground avoided: the field goes round it,
        // from its edge.
        let measure = |seeds: &[Seed]| {
            march_along(
                grid,
                seeds,
                |n| {
                    (!is_avoided(n) || seeded.contains(&n))
                        .then(|| grid.point(n))
                        .flatten()
                },
                |a, b| cliffs.get(&(a.min(b), a.max(b))).copied(),
            )
            .d
        };
        let (mut d, beyond) = if outward {
            // From the inner edges; and how far the outer boundary is, along
            // the surface, which its ring finishes up to.
            let mut seeds = feet(Region::new(holes));
            seeds.extend(&edge_seeds);
            let outer: Vec<Contour> = area
                .rings()
                .iter()
                .filter(|r| r.signed_area() > 0.0)
                .cloned()
                .collect();
            (measure(&seeds), Some(measure(&feet(Region::new(outer)))))
        } else {
            let mut seeds = feet(area.clone());
            seeds.extend(&edge_seeds);
            (measure(&seeds), None)
        };
        report.unreached = d
            .iter()
            .zip(&on.inside)
            .enumerate()
            .filter(|&(n, (d, &inside))| {
                let n = u32::try_from(n).unwrap_or(u32::MAX);
                inside && !d.is_finite() && grid.defined(n) && !is_avoided(n)
            })
            .count() as u64;
        for (n, (d, &inside)) in d.iter_mut().zip(&on.inside).enumerate() {
            let n = u32::try_from(n).unwrap_or(u32::MAX);
            // Over the ground avoided, negative, so its edge is the line at 0;
            // and outside the area, so only the lines inside it are passes.
            // Measured out from the inner edges, the field runs on past the
            // area's outer edge from inside it, and its lines are cut there:
            // it does not fall to 0 there.
            if is_avoided(n) || (!inside && !outward) {
                *d = -*d;
            }
        }
        // The edges of the ground avoided, as lines of the field: each point
        // put where the edge was found along the grid edge it crosses, on the
        // side machined, where the line through the field's values at its ends
        // could put it a little either side, and the side avoided is cut out.
        let mut edges = if edge_seeds.is_empty() {
            Vec::new()
        } else {
            let across = grid.triangles().filter_map(|(t, nodes)| {
                let avoided = nodes.iter().filter(|&&n| is_avoided(n)).count();
                (avoided > 0 && avoided < 3).then_some(t)
            });
            isoline::lines(grid, &d, 0.0, across)
        };
        for p in edges.iter_mut().flat_map(|line| line.points.iter_mut()) {
            if let Some(found) = guided::edge_at(grid, *p).and_then(|e| crossings.get(&e)) {
                *p = *found;
            }
        }
        Ok(Self {
            grid: on.grid,
            d,
            beyond,
            outward,
            edges,
        })
    }
}

/// The length along the surface the tool rides of each bent edge of `grid`
/// ([`guided::bent`]), by its key, lower node first: the straight line
/// between the nodes cuts the corner where a wall turns onto the ground above
/// or below it inside the cell, by up to the cell's width, and the rings
/// beside it would be that much too far apart.
fn cliff_edges(seat: &Seat<'_>, grid: &TriGrid, tolerance: f64) -> HashMap<(u32, u32), f64> {
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

/// Whether, at each node of `grid`, the tool rests only on faces it avoids,
/// to within `tolerance` ([`Seat::rest_at`]).
fn avoided_nodes(seat: &Seat<'_>, grid: &TriGrid, tolerance: f64) -> Vec<bool> {
    (0..grid.nodes())
        .into_par_iter()
        .map(|n| {
            grid.xy(n)
                .filter(|_| grid.defined(n))
                .and_then(|p| seat.rest_at(p.x, p.y, tolerance))
                .is_some_and(|rest| !rest.machined)
        })
        .collect()
}

/// How many halvings place an edge of the ground avoided along a grid edge:
/// to a thousandth of the pitch.
const EDGE_HALVINGS: u32 = 10;

/// The nodes either side of each edge of the ground avoided, by `avoided`,
/// each at its distance to it over the part: found along every grid edge from
/// a node machined to one avoided, where the tool there stops resting on a
/// face machined, halved [`EDGE_HALVINGS`] times. A node beside more than one
/// such edge takes the nearest.
fn avoided_edges(
    seat: &Seat<'_>,
    grid: &TriGrid,
    avoided: &(impl Fn(u32) -> bool + Sync),
    tolerance: f64,
) -> (Vec<Seed>, HashMap<(u32, u32), DVec2>) {
    let machined = |p: DVec2| {
        seat.rest_at(p.x, p.y, tolerance)
            .is_some_and(|rest| rest.machined)
    };
    let found: Vec<(Seed, Seed, DVec2)> = (0..grid.nodes())
        .into_par_iter()
        .filter(|&m| grid.defined(m) && !avoided(m))
        .flat_map_iter(|m| {
            let mut out = Vec::new();
            let Some(pm) = grid.point(m) else {
                return out;
            };
            let mut seen = Vec::new();
            for (_, nodes) in grid.triangles_at(m) {
                for a in nodes {
                    if a == m || !avoided(a) || seen.contains(&a) {
                        continue;
                    }
                    seen.push(a);
                    let Some(pa) = grid.point(a) else { continue };
                    let (mut lo, mut hi) = (pm.truncate(), pa.truncate());
                    for _ in 0..EDGE_HALVINGS {
                        let mid = lo.midpoint(hi);
                        if machined(mid) {
                            lo = mid;
                        } else {
                            hi = mid;
                        }
                    }
                    let Some(z) = seat.drop_at(lo.x, lo.y) else {
                        continue;
                    };
                    let edge = lo.extend(z);
                    let near = |node: u32, at: DVec3| Seed {
                        node,
                        distance: at.distance(edge),
                    };
                    out.push((near(m, pm), near(a, pa), lo));
                }
            }
            out
        })
        .collect();
    let crossings = found
        .iter()
        .map(|(m, a, at)| ((m.node.min(a.node), m.node.max(a.node)), *at))
        .collect();
    // Each node once, at its nearest; in node order, as found on any thread.
    let mut seeds: Vec<Seed> = found.into_iter().flat_map(|(m, a, _)| [m, a]).collect();
    seeds.sort_by(|a, b| a.node.cmp(&b.node).then(a.distance.total_cmp(&b.distance)));
    seeds.dedup_by_key(|s| s.node);
    (seeds, crossings)
}

/// A line of the field, its points taken, with its triangles' slopes.
fn sloped(grid: &TriGrid, line: &mut IsoLine) -> Sloped {
    Sloped {
        own: vec![true; line.points.len()],
        points: std::mem::take(&mut line.points),
        closed: line.closed,
        slopes: line.triangles.iter().map(|&t| grid.slope(t)).collect(),
    }
}

/// A ring of the boundary, split so no segment is longer than the grid's pitch,
/// with the slope under the middle of each.
fn boundary(grid: &TriGrid, ring: &[DVec2]) -> Sloped {
    Sloped::split(ring, true, grid.pitch(), |m, _| {
        grid.locate(m).and_then(|t| grid.slope(t))
    })
}

/// A node of the tree before its lines are sampled.
struct Plan {
    /// The field's lines, placed by it, height and all.
    lines: Vec<Sloped>,
    /// The area's edge, and the edges of faces avoided, placed seen from
    /// above.
    edges: Vec<Sloped>,
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
