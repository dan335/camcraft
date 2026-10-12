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
//! - Every point of every pass is an exact drop-cutter query, refined along the
//!   pass until it is within tolerance of the part, exactly as a parallel pass is
//!   ([`crate::strategy::parallel::sweep_polyline`]).
//! - Nothing is smoothed. A line is only thinned where a point is within a
//!   quarter of the tolerance, and a twentieth of the grid's pitch, of the line
//!   through its neighbours, seen from above.
//!
//! # Top-ups
//!
//! Where the passes close in on a crest, the last one round it may be nearly two
//! stepovers from the one on its other side, and the cusp between four times as
//! tall. A region rising more than half a stepover past its last pass gets one more,
//! halfway up: Fusion's passes run on until they collapse, and this is where they
//! do.
//!
//! # Steep faces
//!
//! The drop-cutter's surface cannot show a wall: the tool goes from its top to its
//! foot at once. Passes bunch up there; any stretch steeper than a plunge
//! ([`PLUNGE_RATIO`](crate::path::toolpath::PLUNGE_RATIO)) is left out, for a waterline pass, and the slope limits leave
//! out what is outside the range asked for.
//!
//! # Order and direction
//!
//! The regions each pass bounds nest, and are cut region by region, the nearest
//! next: from the boundary in (outside in), or each region's inner ones first
//! (inside out). Climbing with a clockwise spindle has the side not yet finished on
//! the tool's right: inward cutting from the outside in, outward from the inside
//! out. Both ways keeps closed rings climbing, and starts an open pass at whichever
//! end is nearer.

use camcraft_geom::contour::{Region, polyline_length};
use camcraft_geom::geodesic::{Seed, march};
use camcraft_geom::isoline::{IsoLine, nest};
use camcraft_geom::shadow::Shadow;
use camcraft_geom::trigrid::TriGrid;
use glam::DVec2;
use rayon::prelude::*;

use crate::cutter::dropcutter::{DropCutter, DropTest};
use crate::path::toolpath::Toolpath;
use crate::strategy::adaptive::Direction;
use crate::strategy::field::{self, AreaGrid, count, level_lines};
pub use crate::strategy::nesting::Order;
use crate::strategy::nesting::{Cuts, Nested, distance, nearest};
use crate::strategy::parallel::{ParallelParams, ParallelReport, Run, sweep_polyline};
use crate::strategy::projected::{Emit, simplify};
use crate::strategy::slope::{self, SlopeCounts, Sloped};
use crate::strategy::{StrategyError, check};

/// The most levels of passes: far past any real part.
const MAX_LEVELS: u32 = 100_000;

/// How to lay out the passes.
#[derive(Debug, Clone, Copy, PartialEq)]
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
    /// Stretches left out for being steeper than a plunge.
    pub too_steep: u64,
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
pub fn scallop_finish<T: DropTest + Sync>(
    cutter: &DropCutter<'_>,
    tool: &T,
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

    let field = Field::build(cutter, tool, area, lo, hi, params, &mut report)?;
    let grid = &field.grid;
    let d = &field.d;

    // The lines at each level, found level by level in parallel.
    let top = d
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(0.0, f64::max);
    let levels = count((top / s).floor()).unwrap_or(0).min(MAX_LEVELS);
    let mut level_lines = level_lines(grid, d, s, levels);
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
            children: Vec::new(),
            root: false,
        });
        if let Some(plan) = nodes.get_mut(i) {
            plan.children.push(id);
        }
    }
    // The boundary's rings, to the level-0 region of the piece they bound.
    for piece in area.pieces() {
        let ring: Vec<Sloped> = piece
            .rings()
            .iter()
            .map(|c| boundary(grid, c.points()))
            .collect();
        report.lines += ring.len() as u64;
        let home = regions
            .iter()
            .position(|r| r.level == 0 && grid.xy(r.rep).is_some_and(|p| piece.contains(p)));
        match home.and_then(|i| nodes.get_mut(i)) {
            Some(plan) => plan.lines.extend(ring),
            None => nodes.push(Plan {
                lines: ring,
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
    let ceiling = slope::ceiling(params.slope_to);
    let thin = (0.25 * params.tolerance).min(0.05 * grid.pitch());
    let worked: Vec<Result<Worked, StrategyError>> = nodes
        .par_iter_mut()
        .map(|plan| {
            let mut w = Worked::default();
            let mut stretches: Vec<(Vec<DVec2>, bool)> = Vec::new();
            for line in std::mem::take(&mut plan.lines) {
                stretches.extend(line.within(
                    params.slope_from,
                    ceiling,
                    params.slope_to,
                    &mut w.slopes,
                ));
            }
            let clipped = area.clip_lines(&stretches)?;
            for (points, closed) in clipped {
                if !closed && polyline_length(&points) < 2.0 * grid.pitch() {
                    w.fragments += 1;
                    continue;
                }
                let points = simplify(&points, closed, thin);
                w.runs.extend(sweep_polyline(
                    cutter,
                    tool,
                    &points,
                    closed,
                    &sweep,
                    &mut w.sweep,
                ));
            }
            Ok(w)
        })
        .collect();
    let mut tree = Nested::new(params.order == Order::InsideOut);
    let mut roots = Vec::new();
    for (plan, w) in nodes.into_iter().zip(worked) {
        let w = w?;
        report.too_steep += w.slopes.too_steep;
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
    let mut cut = Cut {
        emit: Emit::new(params.feed),
        // The side still to finish is to the left of a line as found: inward.
        reverse: (params.order == Order::OutsideIn) == (params.climb_right != conventional),
        both_ways: params.direction == Direction::BothWays,
    };
    while !roots.is_empty() {
        let at = cut.emit.at;
        let k = nearest(&roots, |&r| distance(tree.anchor(r), at));
        tree.cut(roots.remove(k), &mut cut)?;
    }
    report.passes = cut.emit.passes;
    Ok((cut.emit.path, report))
}

/// The field of distances along the surface from the boundary, on its grid.
struct Field {
    grid: TriGrid,
    /// Each node's distance: negative outside the area, infinite where unknown.
    d: Vec<f64>,
}

impl Field {
    fn build<T: DropTest + Sync>(
        cutter: &DropCutter<'_>,
        tool: &T,
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
            field::pitch(s, tool.max_radius()),
            s / 2.0,
            params.max_nodes,
            |x, y, scratch| cutter.drop_at_in(tool, x, y, scratch),
        )?;
        report.pitch = on.grid.pitch();
        report.nodes = u64::from(on.grid.nodes());
        report.coarsened = on.coarsened;
        let grid = &on.grid;

        // The seeds: each node within a cell's diagonal of the boundary, at its
        // exact distance to the nearest point of it, over the part, one drop a
        // node.
        let seeds: Vec<Seed> = on
            .boundary_feet(area)
            .par_iter()
            .map_init(Vec::new, |scratch, &(n, flat, foot)| {
                let distance = match (
                    grid.point(n),
                    cutter.drop_at_in(tool, foot.x, foot.y, scratch),
                ) {
                    (Some(p), Some(zf)) => p.distance(foot.extend(zf)),
                    _ => flat,
                };
                Seed { node: n, distance }
            })
            .collect();

        let mut d = march(grid, &seeds, |n| grid.point(n)).d;
        report.unreached = on.unreached(&d);
        // Outside the area, negative: only the lines inside it are passes.
        for (d, &inside) in d.iter_mut().zip(&on.inside) {
            if !inside {
                *d = -*d;
            }
        }
        Ok(Self { grid: on.grid, d })
    }
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
    lines: Vec<Sloped>,
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
struct Cut {
    emit: Emit,
    /// Whether a line as found is reversed to cut the way asked for.
    reverse: bool,
    /// Whether an open pass may be started at either end.
    both_ways: bool,
}

impl Cuts<Run> for Cut {
    type Error = StrategyError;

    fn at(&self) -> Option<DVec2> {
        self.emit.at
    }

    /// Runs the same distance in, the nearest next, each the way it cuts.
    fn cut(&mut self, mut runs: Vec<Run>) -> Result<(), StrategyError> {
        if self.reverse {
            for run in &mut runs {
                run.points.reverse();
            }
        }
        self.emit.nearest_first(runs, self.both_ways)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)]
mod tests;
