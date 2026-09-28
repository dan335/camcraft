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
//!   ([`crate::parallel::sweep_polyline`]).
//! - Nothing is smoothed. A line is only thinned where a point is within a
//!   quarter of the tolerance of the line through its neighbours, seen from above.
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
//! ([`PLUNGE_RATIO`](crate::toolpath::PLUNGE_RATIO)) is left out, for a waterline pass, and the slope limits leave
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
use camcraft_geom::isoline::{IsoLine, crossings, lines, nest};
use camcraft_geom::mesh::TriMesh;
use camcraft_geom::shadow::shadow_above;
use camcraft_geom::trigrid::TriGrid;
use glam::DVec2;
use rayon::prelude::*;

use crate::adaptive::Direction;
use crate::dropcutter::{DropCutter, DropTest};
use crate::field::{AreaGrid, count};
pub use crate::nesting::Order;
use crate::nesting::{Cuts, Nested, distance, nearest};
use crate::parallel::{ParallelParams, ParallelReport, Run, sweep_polyline};
use crate::planar::pieces;
use crate::projected::{Emit, simplify, start_distance, start_near};
use crate::slope::{self, SlopeCounts, Sloped};
use crate::strategy::{StrategyError, check};
use crate::toolpath::Toolpath;

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

/// Where the tool touches the part: within its widest radius of the mesh seen from
/// above, a tolerance short so as never to reach past it. Fusion's *contact only*.
pub fn contact_region(
    mesh: &TriMesh,
    max_radius: f64,
    tolerance: f64,
) -> Result<Region, StrategyError> {
    let Some(bounds) = mesh.bounds() else {
        return Ok(Region::default());
    };
    let shadow = shadow_above(mesh, bounds.min.z - 1.0)?;
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
    let buckets = crossings(grid, d, s, levels);
    let level_lines: Vec<Vec<IsoLine>> = buckets
        .par_iter()
        .enumerate()
        .map(|(k, tris)| {
            if k == 0 {
                return Vec::new();
            }
            let level = f64::from(u32::try_from(k).unwrap_or(0)) * s;
            lines(grid, d, level, tris.iter().copied())
        })
        .collect();
    let regions = nest(grid, d, s, &level_lines);
    report.levels = u64::from(levels);
    report.lines = level_lines.iter().map(|l| l.len() as u64).sum();
    report.top_ups = regions.iter().filter(|r| !r.top_up.is_empty()).count() as u64;

    // The nodes of the tree the passes are cut by: one a region, one for each
    // region's top-up, and one for each piece of the area with no region of its
    // own. Each with the lines it cuts, and the slope of each segment.
    let mut nodes: Vec<Plan> = regions
        .iter()
        .map(|r| {
            let lines = level_lines
                .get(usize::try_from(r.level).unwrap_or(usize::MAX))
                .map(|all| {
                    r.lines
                        .iter()
                        .filter_map(|&i| all.get(i))
                        .map(|l| sloped(grid, l))
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
    for (i, r) in regions.iter().enumerate() {
        if r.top_up.is_empty() {
            continue;
        }
        let id = nodes.len();
        nodes.push(Plan {
            lines: r.top_up.iter().map(|l| sloped(grid, l)).collect(),
            children: Vec::new(),
            root: false,
        });
        if let Some(plan) = nodes.get_mut(i) {
            plan.children.push(id);
        }
    }
    // The boundary's rings, to the level-0 region of the piece they bound.
    for piece in pieces(area) {
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
        .par_iter()
        .map(|plan| {
            let mut w = Worked::default();
            let mut stretches: Vec<(Vec<DVec2>, bool)> = Vec::new();
            for line in &plan.lines {
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
    let mut nesting = Vec::new();
    for (i, (plan, w)) in nodes.into_iter().zip(worked).enumerate() {
        let w = w?;
        report.too_steep += w.slopes.too_steep;
        report.outside_slope += w.slopes.outside_slope;
        report.fragments += w.fragments;
        report.sweep += w.sweep;
        tree.push(w.runs);
        nesting.extend(plan.children.into_iter().map(|c| (i, c)));
        if plan.root {
            roots.push(i);
        }
    }
    for (parent, child) in nesting {
        tree.nest(parent, child);
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
        let pitch = (s / 3.0).min(tool.max_radius() / 2.0).max(1e-3);
        let on = AreaGrid::over(
            area,
            (lo, hi),
            pitch,
            s / 2.0,
            params.max_nodes,
            |x, y, scratch| cutter.drop_at_in(tool, x, y, scratch),
        )?;
        report.pitch = on.pitch;
        report.nodes = on.nodes;
        report.coarsened = on.coarsened;
        let grid = &on.grid;

        // The seeds: each node within a cell's diagonal of the boundary, at its
        // exact distance to the nearest point of it, over the part, one drop a
        // node.
        let seeds: Vec<Seed> = on
            .boundary_feet(area)
            .par_iter()
            .enumerate()
            .filter_map(|(n, foot)| Some((u32::try_from(n).ok()?, (*foot)?)))
            .map_init(Vec::new, |scratch, (n, (flat, foot))| {
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

/// A line of the field, with its triangles' slopes.
fn sloped(grid: &TriGrid, line: &IsoLine) -> Sloped {
    Sloped {
        points: line.points.clone(),
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
        for run in &mut runs {
            if self.reverse {
                run.points.reverse();
            }
        }
        while !runs.is_empty() {
            let at = self.emit.at;
            let both_ways = self.both_ways;
            let k = nearest(&runs, |r| match at {
                Some(at) => start_distance(r, at, both_ways),
                None => 0.0,
            });
            let mut run = runs.remove(k);
            if let Some(at) = self.emit.at {
                if run.closed {
                    start_near(&mut run, at);
                } else if self.both_ways {
                    let (Some(first), Some(last)) = (run.points.first(), run.points.last()) else {
                        continue;
                    };
                    if last.truncate().distance(at) < first.truncate().distance(at) {
                        run.points.reverse();
                    }
                }
            }
            self.emit.emit(&run)?;
        }
        Ok(())
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
mod tests {
    use super::*;
    use crate::tool::{Ball, Flat};
    use crate::toolpath::MoveRole;
    use camcraft_geom::contour::Contour;
    use camcraft_geom::synthetic;
    use glam::{DVec3, dvec2};

    /// Each pass in `path`: its points, in order.
    fn passes(path: &Toolpath) -> Vec<Vec<DVec3>> {
        let mut out: Vec<Vec<DVec3>> = Vec::new();
        for m in path.moves() {
            match m.role {
                MoveRole::Link => out.push(vec![m.to]),
                _ => {
                    if let Some(p) = out.last_mut() {
                        p.push(m.to);
                    }
                }
            }
        }
        out
    }

    fn signed_area(points: &[DVec3]) -> f64 {
        let flat: Vec<DVec2> = points.iter().map(|p| p.truncate()).collect();
        Contour::new(flat).map_or(0.0, |c| c.signed_area())
    }

    fn disc(r: f64) -> Region {
        let points = (0..360)
            .map(|i| {
                let a = f64::from(i).to_radians();
                dvec2(a.cos(), a.sin()) * r
            })
            .collect();
        Region::new(vec![Contour::new(points).unwrap()])
    }

    fn square(half: f64) -> Region {
        Region::rectangle(DVec2::splat(-half), DVec2::splat(half))
    }

    fn params(stepover: f64) -> ScallopParams {
        ScallopParams {
            stepover,
            tolerance: 0.01,
            direction: Direction::Climb,
            ..Default::default()
        }
    }

    /// A plate, a square boundary: the passes are the square shrunk by whole
    /// stepovers, the first the boundary itself, and cut from the boundary in.
    #[test]
    fn on_a_plate_the_passes_are_offsets_of_the_boundary() {
        let mesh = synthetic::plane(0.0, 20.0);
        let cutter = DropCutter::new(&mesh);
        let tool = Flat::new(1.0).unwrap();
        let s = 1.0;
        let (path, report) = scallop_finish(&cutter, &tool, &square(8.0), &params(s)).unwrap();
        assert!(report.is_clean(), "{report:?}");
        let found = passes(&path);
        assert!(found.len() >= 8, "{}", found.len());
        let pitch = report.pitch;
        let mut levels = Vec::new();
        for pass in &found {
            // Away from the corners' diagonals, where the offsets meet.
            let ins: Vec<f64> = pass
                .iter()
                .filter(|p| (p.x.abs() - p.y.abs()).abs() > 2.0 * pitch)
                .map(|p| 8.0 - p.x.abs().max(p.y.abs()))
                .collect();
            let Some(&first) = ins.first() else { continue };
            let k = (first / s).round();
            for d in &ins {
                assert!((d - k * s).abs() < 1e-3, "{d} is not at {k} stepovers in");
            }
            levels.push(k);
            for p in pass {
                assert!(p.z.abs() < 1e-9);
            }
        }
        assert_eq!(levels.first(), Some(&0.0), "the boundary first");
        for w in levels.windows(2) {
            assert!(w[1] >= w[0], "outside in: {levels:?}");
        }
    }

    /// On a dome the passes are a stepover apart along the surface, however steep:
    /// a ball's tip rides a sphere 12 across, so a pass at `r` from the middle is
    /// `12·acos(r/12)` from the equator along it.
    #[test]
    fn on_a_dome_the_passes_are_a_stepover_apart_along_the_surface() {
        let (lat, lon) = (160, 240);
        let mesh = synthetic::uv_sphere(10.0, lat, lon);
        let sag = synthetic::uv_sphere_max_sag(10.0, lat, lon);
        let cutter = DropCutter::new(&mesh);
        let tool = Ball::new(2.0).unwrap();
        let s = 0.5;
        // A boundary at 7 from the middle, 36° down the dome.
        let edge = 12.0 * (7.0f64 / 12.0).acos();
        let (path, report) = scallop_finish(&cutter, &tool, &disc(7.0), &params(s)).unwrap();
        assert!(report.is_clean(), "{report:?}");
        let along = |p: &DVec3| 12.0 * (p.truncate().length().min(12.0) / 12.0).acos() - edge;
        let mut found = passes(&path);
        // The last is the top-up, halfway from the last level to the crest.
        let top_up = found.pop().unwrap();
        let crest = 12.0 * 7f64.atan2((144.0f64 - 49.0).sqrt());
        let top_level = (crest / s).floor() * s;
        for p in &top_up {
            assert!(
                (along(p) - (top_level + crest) / 2.0).abs() < 0.25 * s,
                "{p:?}"
            );
        }
        let mut means = Vec::new();
        for pass in &found {
            let at: Vec<f64> = pass.iter().map(along).collect();
            let mean = at.iter().sum::<f64>() / at.len() as f64;
            let level = (mean / s).round() * s;
            for a in &at {
                // First order in the grid's pitch, most where the rings close in.
                assert!((a - level).abs() < 0.25 * s + sag, "{a} is not at {level}");
            }
            means.push(mean);
        }
        assert!(means.len() >= 10, "{}", means.len());
        for w in means.windows(2) {
            let gap = w[1] - w[0];
            assert!(
                (gap - s).abs() < 0.1 * s,
                "passes {gap} apart, not {s}: {means:?}"
            );
        }
    }

    /// Where the surface is outside the slope range, nothing is cut.
    #[test]
    fn slope_limits_are_kept_to() {
        let mesh = synthetic::uv_sphere(10.0, 120, 180);
        let cutter = DropCutter::new(&mesh);
        let tool = Ball::new(2.0).unwrap();
        let p = ScallopParams {
            slope_from: 20f64.to_radians(),
            slope_to: 50f64.to_radians(),
            ..params(0.5)
        };
        let (path, report) = scallop_finish(&cutter, &tool, &disc(11.0), &p).unwrap();
        assert!(report.outside_slope > 0, "{report:?}");
        let (lo, hi) = (
            12.0 * 20f64.to_radians().sin(),
            12.0 * 50f64.to_radians().sin(),
        );
        let slack = 2.0 * report.pitch;
        let found = passes(&path);
        assert!(!found.is_empty());
        for pass in found {
            for q in pass {
                let r = q.truncate().length();
                assert!(r > lo - slack && r < hi + slack, "{r} outside {lo}..{hi}");
            }
        }
    }

    /// Climbing with a clockwise spindle from the boundary in has the side still
    /// to finish, inward, on the right: clockwise. Conventional, or inside out,
    /// the other way.
    #[test]
    fn direction_follows_climb_and_order() {
        let mesh = synthetic::plane(0.0, 20.0);
        let cutter = DropCutter::new(&mesh);
        let tool = Flat::new(1.0).unwrap();
        for (direction, order, clockwise) in [
            (Direction::Climb, Order::OutsideIn, true),
            (Direction::Conventional, Order::OutsideIn, false),
            (Direction::Climb, Order::InsideOut, false),
        ] {
            let p = ScallopParams {
                direction,
                order,
                ..params(1.0)
            };
            let (path, _) = scallop_finish(&cutter, &tool, &square(6.0), &p).unwrap();
            let rings: Vec<f64> = passes(&path)
                .iter()
                .map(|pass| signed_area(pass))
                .filter(|a| a.abs() > 1.0)
                .collect();
            assert!(!rings.is_empty());
            for a in rings {
                assert_eq!(a < 0.0, clockwise, "{direction:?} {order:?}: {a}");
            }
        }
    }

    /// Inside out, the innermost passes come first and the boundary last.
    #[test]
    fn inside_out_ends_on_the_boundary() {
        let mesh = synthetic::plane(0.0, 20.0);
        let cutter = DropCutter::new(&mesh);
        let tool = Flat::new(1.0).unwrap();
        let p = ScallopParams {
            order: Order::InsideOut,
            ..params(1.0)
        };
        let (path, _) = scallop_finish(&cutter, &tool, &square(6.0), &p).unwrap();
        let depth = |pass: &Vec<DVec3>| {
            pass.iter()
                .map(|q| 6.0 - q.x.abs().max(q.y.abs()))
                .fold(f64::MAX, f64::min)
        };
        let depths: Vec<f64> = passes(&path).iter().map(depth).collect();
        for w in depths.windows(2) {
            assert!(w[1] <= w[0] + 1e-6, "{depths:?}");
        }
        assert!(depths.last().unwrap().abs() < 1e-6);
    }

    /// A step's wall is left for a waterline pass, and every point is on the
    /// part and every cut between points within tolerance of it.
    #[test]
    fn a_wall_is_left_and_nothing_cuts_below_the_surface() {
        let mesh = synthetic::step(6.0, 20.0);
        let cutter = DropCutter::new(&mesh);
        let tool = Ball::new(1.5).unwrap();
        let (path, report) = scallop_finish(&cutter, &tool, &square(8.0), &params(0.8)).unwrap();
        assert!(report.too_steep > 0, "{report:?}");
        for m in path.moves() {
            let exact = cutter.drop_at(&tool, m.to.x, m.to.y).unwrap();
            assert!(
                (m.to.z - exact).abs() < 1e-9,
                "{:?} is not on the part",
                m.to
            );
        }
        let mut prev: Option<DVec3> = None;
        for m in path.moves() {
            if m.role == MoveRole::Cut
                && let Some(a) = prev
            {
                for t in [0.25, 0.5, 0.75] {
                    let q = a.lerp(m.to, t);
                    let floor = cutter.drop_at(&tool, q.x, q.y).unwrap();
                    assert!(q.z >= floor - 0.01 - 1e-9, "{q:?} below {floor}");
                }
            }
            prev = Some(m.to);
        }
    }

    /// A strip whose middle rises more than half a stepover past its last pass
    /// gets one more, near its middle.
    #[test]
    fn a_crest_gets_a_top_up() {
        let mesh = synthetic::plane(0.0, 60.0);
        let cutter = DropCutter::new(&mesh);
        let tool = Flat::new(0.5).unwrap();
        let strip = Region::rectangle(dvec2(-1.7, -20.0), dvec2(1.7, 20.0));
        let (path, report) = scallop_finish(&cutter, &tool, &strip, &params(1.0)).unwrap();
        assert_eq!(report.top_ups, 1, "{report:?}");
        let nearest = passes(&path)
            .iter()
            .flatten()
            .filter(|q| q.y.abs() < 15.0)
            .map(|q| q.x.abs())
            .fold(f64::MAX, f64::min);
        assert!(
            nearest < 0.5,
            "nothing within half a stepover of the middle: {nearest}"
        );
    }

    #[test]
    fn a_field_too_large_is_coarsened_then_refused() {
        let mesh = synthetic::plane(0.0, 20.0);
        let cutter = DropCutter::new(&mesh);
        let tool = Flat::new(1.0).unwrap();
        let coarse = ScallopParams {
            max_nodes: 2_000,
            ..params(1.0)
        };
        let (_, report) = scallop_finish(&cutter, &tool, &square(8.0), &coarse).unwrap();
        assert!(report.coarsened && report.nodes <= 2_000, "{report:?}");
        let tiny = ScallopParams {
            max_nodes: 50,
            ..params(1.0)
        };
        assert!(matches!(
            scallop_finish(&cutter, &tool, &square(8.0), &tiny),
            Err(StrategyError::FieldTooLarge { .. })
        ));
    }

    #[test]
    fn rejects_unusable_parameters() {
        let mesh = synthetic::plane(0.0, 20.0);
        let cutter = DropCutter::new(&mesh);
        let tool = Flat::new(1.0).unwrap();
        for bad in [
            params(0.0),
            ScallopParams {
                tolerance: f64::NAN,
                ..params(1.0)
            },
            ScallopParams {
                slope_from: 1.0,
                slope_to: 0.5,
                ..params(1.0)
            },
            ScallopParams {
                slope_to: 2.0,
                ..params(1.0)
            },
        ] {
            assert!(scallop_finish(&cutter, &tool, &square(5.0), &bad).is_err());
        }
    }

    /// The same passes on one thread or four.
    #[test]
    fn is_deterministic() {
        let mesh = synthetic::uv_sphere(10.0, 60, 90);
        let cutter = DropCutter::new(&mesh);
        let tool = Ball::new(2.0).unwrap();
        let run = |threads: usize| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap()
                .install(|| {
                    scallop_finish(&cutter, &tool, &disc(9.0), &params(0.8))
                        .unwrap()
                        .0
                })
        };
        assert_eq!(run(1).moves(), run(4).moves());
    }

    /// Only where the tool touches the part.
    #[test]
    fn contact_is_the_part_grown_by_the_tool() {
        let mesh = synthetic::plane(0.0, 5.0);
        let contact = contact_region(&mesh, 2.0, 0.01).unwrap();
        let (lo, hi) = contact.bounds().unwrap();
        assert!(
            (lo.x + 7.0).abs() < 0.02 && (hi.y - 7.0).abs() < 0.02,
            "{lo:?} {hi:?}"
        );
        assert!(lo.x > -7.0 && hi.y < 7.0);
    }
}
