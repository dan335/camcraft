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
//! ([`PLUNGE_RATIO`]) is left out, for a waterline pass, and the slope limits leave
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

use camcraft_geom::contour::Region;
use camcraft_geom::geodesic::{Seed, march};
use camcraft_geom::isoline::{IsoLine, crossings, lines, nest};
use camcraft_geom::mesh::TriMesh;
use camcraft_geom::shadow::shadow_above;
use camcraft_geom::trigrid::TriGrid;
use glam::{DVec2, DVec3};
use rayon::prelude::*;

use crate::adaptive::Direction;
use crate::dropcutter::{DropCutter, DropTest};
use crate::parallel::{ParallelParams, ParallelReport, Run, sweep_polyline};
use crate::planar::pieces;
use crate::strategy::{StrategyError, check};
use crate::toolpath::{MoveRole, PLUNGE_RATIO, Toolpath};

/// The most levels of passes: far past any real part.
const MAX_LEVELS: u32 = 100_000;

/// Which way round the passes are cut.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Order {
    /// From the boundary in. Fusion's default.
    #[default]
    OutsideIn,
    /// Each region's innermost passes first, out to the boundary.
    InsideOut,
}

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
    let shadow = shadow_above(mesh, bounds.min.z - 1.0).map_err(region_err)?;
    shadow
        .grown((max_radius - tolerance).max(tolerance), tolerance * 0.5)
        .map_err(region_err)
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
    let right = std::f64::consts::FRAC_PI_2 + 1e-9;
    if !(params.slope_from >= 0.0
        && params.slope_from < params.slope_to
        && params.slope_to <= right)
    {
        return Err(StrategyError::BadParameter {
            name: "slope range",
            value: params.slope_to,
        });
    }
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
                        .map(|l| Sloped::of(grid, l))
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
            lines: r.top_up.iter().map(|l| Sloped::of(grid, l)).collect(),
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
            .map(|c| Sloped::boundary(grid, c.points()))
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
    let ceiling = PLUNGE_RATIO.atan().min(params.slope_to);
    let thin = (0.25 * params.tolerance).min(0.05 * grid.pitch());
    let worked: Vec<Result<Worked, StrategyError>> = nodes
        .par_iter()
        .map(|plan| {
            let mut w = Worked::default();
            let mut stretches: Vec<(Vec<DVec2>, bool)> = Vec::new();
            for line in &plan.lines {
                stretches.extend(line.within(params.slope_from, ceiling, params.slope_to, &mut w));
            }
            let clipped = area.clip_lines(&stretches).map_err(region_err)?;
            for (points, closed) in clipped {
                if !closed && length(&points) < 2.0 * grid.pitch() {
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
    let mut tree = Tree {
        nodes: Vec::with_capacity(worked.len()),
        children_first: params.order == Order::InsideOut,
    };
    for (plan, w) in nodes.iter().zip(worked) {
        let w = w?;
        report.too_steep += w.too_steep;
        report.outside_slope += w.outside_slope;
        report.fragments += w.fragments;
        add(&mut report.sweep, &w.sweep);
        tree.nodes.push(Node {
            runs: w.runs,
            children: plan.children.clone(),
            anchor: None,
        });
    }
    tree.anchor();

    // Cut from the roots, the nearest next.
    let conventional = params.direction == Direction::Conventional;
    let mut cut = Cut {
        path: Toolpath::new(),
        at: None,
        feed: params.feed,
        // The side still to finish is to the left of a line as found: inward.
        reverse: (params.order == Order::OutsideIn) == (params.climb_right != conventional),
        both_ways: params.direction == Direction::BothWays,
        passes: 0,
    };
    let mut roots: Vec<usize> = nodes
        .iter()
        .enumerate()
        .filter(|(_, p)| p.root)
        .map(|(i, _)| i)
        .collect();
    while !roots.is_empty() {
        let at = cut.at;
        let k = nearest(&roots, |&r| distance(tree.anchor_of(r), at));
        cut.tree(&mut tree, roots.remove(k))?;
    }
    report.passes = cut.passes;
    Ok((cut.path, report))
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
        // coarsened, if it must be, to stay within the budget.
        let mut pitch = (s / 3.0).min(tool.max_radius() / 2.0).max(1e-3);
        let size = |pitch: f64| {
            let span = hi - lo + DVec2::splat(4.0 * pitch);
            let nx = count((span.x / pitch).ceil() + 1.0)?;
            let ny = count((span.y / pitch).ceil() + 1.0)?;
            Some((nx, ny, u64::from(nx) * u64::from(ny)))
        };
        let mut shape = size(pitch);
        for _ in 0..200 {
            match shape {
                Some((_, _, n)) if n <= u64::from(params.max_nodes) => break,
                _ => {
                    pitch *= 1.1;
                    report.coarsened = true;
                    shape = size(pitch);
                }
            }
        }
        let Some((nx, ny, total)) = shape.filter(|&(_, _, n)| n <= u64::from(params.max_nodes))
        else {
            return Err(StrategyError::FieldTooLarge { pitch });
        };
        if pitch > s / 2.0 {
            return Err(StrategyError::FieldTooLarge { pitch });
        }
        report.pitch = pitch;
        report.nodes = total;
        let origin = lo - DVec2::splat(2.0 * pitch);
        let (wide, count_nodes) = (
            usize::try_from(nx).unwrap_or(0),
            usize::try_from(total).unwrap_or(0),
        );

        // Inside the area, a row at a time; and the nodes beside them.
        let mut inside = vec![false; count_nodes];
        for iy in 0..ny {
            let y = origin.y + f64::from(iy) * pitch;
            for (a, b) in area.spans(DVec2::X, y) {
                let first = count(((a - origin.x) / pitch).ceil().max(0.0)).unwrap_or(0);
                let last = count(((b - origin.x) / pitch).floor())
                    .unwrap_or(0)
                    .min(nx - 1);
                for ix in first..=last {
                    if let Some(cell) =
                        index(u64::from(iy * nx + ix)).and_then(|i| inside.get_mut(i))
                    {
                        *cell = true;
                    }
                }
            }
        }
        let is_inside = |ix: i64, iy: i64| {
            let (Ok(x), Ok(y)) = (u32::try_from(ix), u32::try_from(iy)) else {
                return false;
            };
            x < nx
                && y < ny
                && index(u64::from(y * nx + x))
                    .and_then(|i| inside.get(i))
                    .copied()
                    .unwrap_or(false)
        };
        let active: Vec<bool> = (0..total)
            .map(|n| {
                let (ix, iy) = (
                    i64::try_from(n % u64::from(nx)).unwrap_or(0),
                    i64::try_from(n / u64::from(nx)).unwrap_or(0),
                );
                (-1..=1).any(|dy| (-1..=1).any(|dx| is_inside(ix + dx, iy + dy)))
            })
            .collect();

        // The tool's height at each, dropped exactly, row by row in parallel.
        let mut z = vec![f32::NAN; count_nodes];
        z.par_chunks_mut(wide.max(1))
            .enumerate()
            .for_each_init(Vec::new, |scratch, (iy, row)| {
                let y = origin.y + f64::from(u32::try_from(iy).unwrap_or(0)) * pitch;
                for (ix, cell) in row.iter_mut().enumerate() {
                    if !active.get(iy * wide + ix).copied().unwrap_or(false) {
                        continue;
                    }
                    let x = origin.x + f64::from(u32::try_from(ix).unwrap_or(0)) * pitch;
                    if let Some(v) = cutter.drop_at_in(tool, x, y, scratch) {
                        // Grid storage: f32, as every grid in the workspace.
                        #[allow(clippy::cast_possible_truncation)]
                        let v = v as f32;
                        *cell = v;
                    }
                }
            });
        let grid = TriGrid::new(origin, pitch, nx, ny, z, active)
            .ok_or_else(|| StrategyError::Region("the scallop grid could not be made".into()))?;

        // The seeds: each node within a cell's diagonal of the boundary, at its
        // exact distance to the nearest point of it, over the part.
        let reach = std::f64::consts::SQRT_2 * pitch + 1e-9;
        let segments: Vec<(DVec2, DVec2)> = area
            .rings()
            .iter()
            .flat_map(|r| {
                let p = r.points();
                p.iter()
                    .copied()
                    .zip(p.iter().copied().cycle().skip(1))
                    .take(p.len())
            })
            .collect();
        let near: Vec<Vec<(u32, f64)>> = segments
            .par_iter()
            .map_init(Vec::new, |scratch, &(a, b)| {
                let mut out = Vec::new();
                let (min, max) = (
                    a.min(b) - DVec2::splat(1.5 * pitch),
                    a.max(b) + DVec2::splat(1.5 * pitch),
                );
                let x0 = count(((min.x - origin.x) / pitch).floor().max(0.0)).unwrap_or(0);
                let y0 = count(((min.y - origin.y) / pitch).floor().max(0.0)).unwrap_or(0);
                let x1 = count(((max.x - origin.x) / pitch).ceil())
                    .unwrap_or(0)
                    .min(nx - 1);
                let y1 = count(((max.y - origin.y) / pitch).ceil())
                    .unwrap_or(0)
                    .min(ny - 1);
                for iy in y0..=y1 {
                    for ix in x0..=x1 {
                        let Some(n) = grid.node(ix, iy) else { continue };
                        let (Some(p), Some(zn)) = (grid.xy(n), grid.z(n)) else {
                            continue;
                        };
                        let foot = closest_on_segment(p, a, b);
                        let flat = p.distance(foot);
                        if flat > reach {
                            continue;
                        }
                        let d = cutter
                            .drop_at_in(tool, foot.x, foot.y, scratch)
                            .map_or(flat, |zf| {
                                DVec3::new(p.x, p.y, zn).distance(foot.extend(zf))
                            });
                        out.push((n, d));
                    }
                }
                out
            })
            .collect();
        let mut best = vec![f64::INFINITY; count_nodes];
        for (n, d) in near.into_iter().flatten() {
            if let Some(slot) = index(u64::from(n)).and_then(|i| best.get_mut(i))
                && d < *slot
            {
                *slot = d;
            }
        }
        let seeds: Vec<Seed> = best
            .iter()
            .enumerate()
            .filter(|(_, d)| d.is_finite())
            .filter_map(|(n, &d)| {
                Some(Seed {
                    node: u32::try_from(n).ok()?,
                    distance: d,
                })
            })
            .collect();

        let field = march(&grid, &seeds, |n| grid.point(n));
        // Outside the area, negative: only the lines inside it are passes.
        let d: Vec<f64> = field
            .d
            .iter()
            .zip(&inside)
            .map(|(&d, &inside)| if inside { d } else { -d })
            .collect();
        report.unreached = (0..grid.nodes())
            .filter(|&n| {
                grid.defined(n)
                    && index(u64::from(n))
                        .and_then(|i| inside.get(i))
                        .copied()
                        .unwrap_or(false)
                    && field.at(n).is_none()
            })
            .count() as u64;
        Ok(Self { grid, d })
    }
}

/// A line to cut, with the slope under each of its segments.
struct Sloped {
    points: Vec<DVec2>,
    closed: bool,
    /// One a segment: `points[i]` to the next. `None` where nothing is known.
    slopes: Vec<Option<f64>>,
}

impl Sloped {
    /// A line of the field, with its triangles' slopes.
    fn of(grid: &TriGrid, line: &IsoLine) -> Self {
        Self {
            points: line.points.clone(),
            closed: line.closed,
            slopes: line.triangles.iter().map(|&t| grid.slope(t)).collect(),
        }
    }

    /// A ring of the boundary, split so no segment is longer than the grid's pitch,
    /// with the slope under the middle of each.
    fn boundary(grid: &TriGrid, ring: &[DVec2]) -> Self {
        let pitch = grid.pitch();
        let mut points = Vec::new();
        for (&a, &b) in ring
            .iter()
            .zip(ring.iter().cycle().skip(1))
            .take(ring.len())
        {
            let pieces = count((a.distance(b) / pitch).ceil()).unwrap_or(1).max(1);
            for k in 0..pieces {
                points.push(a.lerp(b, f64::from(k) / f64::from(pieces)));
            }
        }
        let slopes = points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .take(points.len())
            .map(|(&a, &b)| grid.locate(a.midpoint(b)).and_then(|t| grid.slope(t)))
            .collect();
        Self {
            points,
            closed: true,
            slopes,
        }
    }

    /// The stretches of it whose slope is in `[from, to]` and not past `ceiling`,
    /// counting what is left out.
    fn within(&self, from: f64, ceiling: f64, to: f64, w: &mut Worked) -> Vec<(Vec<DVec2>, bool)> {
        let keep: Vec<bool> = self
            .slopes
            .iter()
            .map(|s| s.is_none_or(|s| s >= from - 1e-12 && s <= ceiling + 1e-12))
            .collect();
        if keep.iter().all(|&k| k) {
            return vec![(self.points.clone(), self.closed)];
        }
        // Each stretch left out, as too steep or out of the range asked for.
        let mut previous = self.closed && keep.last() == Some(&false);
        for (k, s) in keep.iter().zip(&self.slopes) {
            if !k && !previous {
                match s {
                    Some(s) if *s > ceiling && *s <= to + 1e-12 => w.too_steep += 1,
                    _ => w.outside_slope += 1,
                }
            }
            previous = !k;
        }
        let segments = self.slopes.len();
        // A closed line is started just after a stretch left out.
        let start = if self.closed {
            keep.iter().position(|&k| !k).map_or(0, |i| i + 1)
        } else {
            0
        };
        let mut out = Vec::new();
        let mut run: Vec<DVec2> = Vec::new();
        for step in 0..segments {
            let i = (start + step) % segments;
            let (Some(&a), Some(&b)) = (
                self.points.get(i),
                self.points.get((i + 1) % self.points.len()),
            ) else {
                continue;
            };
            if keep.get(i).copied().unwrap_or(true) {
                if run.is_empty() {
                    run.push(a);
                }
                run.push(b);
            } else if !run.is_empty() {
                out.push((std::mem::take(&mut run), false));
            }
        }
        if run.len() >= 2 {
            out.push((run, false));
        }
        out
    }
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
    too_steep: u64,
    outside_slope: u64,
    fragments: u64,
}

/// The passes, as the regions they bound nest.
struct Tree {
    nodes: Vec<Node>,
    /// Whether a node's children are cut before it: inside out.
    children_first: bool,
}

struct Node {
    runs: Vec<Run>,
    children: Vec<usize>,
    /// Where the node's subtree cuts, to go to the nearest by.
    anchor: Option<DVec2>,
}

impl Tree {
    /// Each node's anchor: its first run's start, or its first child's anchor.
    fn anchor(&mut self) {
        // A node's children may come before or after it, so settle them all.
        for _ in 0..self.nodes.len() {
            let mut changed = false;
            for i in 0..self.nodes.len() {
                if self.nodes.get(i).is_some_and(|n| n.anchor.is_some()) {
                    continue;
                }
                let anchor = self.nodes.get(i).and_then(|n| {
                    n.runs
                        .first()
                        .and_then(|r| r.points.first())
                        .map(|p| p.truncate())
                        .or_else(|| n.children.iter().find_map(|&c| self.anchor_of(c)))
                });
                if anchor.is_some()
                    && let Some(n) = self.nodes.get_mut(i)
                {
                    n.anchor = anchor;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    fn anchor_of(&self, i: usize) -> Option<DVec2> {
        self.nodes.get(i).and_then(|n| n.anchor)
    }
}

/// Cutting the passes into the path, in order, from where the tool is.
struct Cut {
    path: Toolpath,
    at: Option<DVec2>,
    feed: f64,
    /// Whether a line as found is reversed to cut the way asked for.
    reverse: bool,
    /// Whether an open pass may be started at either end.
    both_ways: bool,
    passes: u64,
}

impl Cut {
    /// The subtree at `root`, each node's runs before its children or after, and
    /// of the children the nearest next. Walked with a stack of its own, as a tree
    /// is as deep as the area is wide over the stepover.
    fn tree(&mut self, tree: &mut Tree, root: usize) -> Result<(), StrategyError> {
        let children = |tree: &Tree, n: usize| {
            tree.nodes
                .get(n)
                .map(|node| node.children.clone())
                .unwrap_or_default()
        };
        let mut stack: Vec<(usize, Vec<usize>, bool)> = vec![(root, children(tree, root), false)];
        while let Some((node, left, cut)) = stack.last_mut() {
            let node = *node;
            if !*cut && (!tree.children_first || left.is_empty()) {
                *cut = true;
                let runs = tree
                    .nodes
                    .get_mut(node)
                    .map(|n| std::mem::take(&mut n.runs))
                    .unwrap_or_default();
                self.runs(runs)?;
                continue;
            }
            if left.is_empty() {
                stack.pop();
                continue;
            }
            let at = self.at;
            let k = nearest(left, |&c| distance(tree.anchor_of(c), at));
            let child = left.remove(k);
            stack.push((child, children(tree, child), false));
        }
        Ok(())
    }

    /// Runs the same distance in, the nearest next, each the way it cuts.
    fn runs(&mut self, mut runs: Vec<Run>) -> Result<(), StrategyError> {
        for run in &mut runs {
            if self.reverse {
                run.points.reverse();
            }
        }
        while !runs.is_empty() {
            let at = self.at;
            let both_ways = self.both_ways;
            let k = nearest(&runs, |r| match at {
                Some(at) => start_distance(r, at, both_ways),
                None => 0.0,
            });
            let mut run = runs.remove(k);
            if let Some(at) = self.at {
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
            self.emit(&run)?;
        }
        Ok(())
    }

    fn emit(&mut self, run: &Run) -> Result<(), StrategyError> {
        let Some(&first) = run.points.first() else {
            return Ok(());
        };
        // The strategy does not plan how to arrive; the linker does.
        self.path.feed_to(first, MoveRole::Link, self.feed)?;
        for &p in run.points.iter().skip(1) {
            self.path.feed_to(p, MoveRole::Cut, self.feed)?;
        }
        if run.closed {
            self.path.feed_to(first, MoveRole::Cut, self.feed)?;
        }
        self.passes += 1;
        self.at = Some(if run.closed {
            first.truncate()
        } else {
            run.points.last().map_or(first, |p| *p).truncate()
        });
        Ok(())
    }
}

/// How far the tool at `at` is from where `run` would start.
fn start_distance(run: &Run, at: DVec2, both_ways: bool) -> f64 {
    let d = |p: &DVec3| p.truncate().distance(at);
    if run.closed {
        run.points.iter().map(d).fold(f64::MAX, f64::min)
    } else {
        let first = run.points.first().map_or(f64::MAX, d);
        let last = run.points.last().map_or(f64::MAX, d);
        if both_ways { first.min(last) } else { first }
    }
}

/// A closed run started at its point nearest `at`, going the same way.
fn start_near(run: &mut Run, at: DVec2) {
    let nearest = run
        .points
        .iter()
        .enumerate()
        .min_by(|a, b| {
            a.1.truncate()
                .distance(at)
                .total_cmp(&b.1.truncate().distance(at))
        })
        .map(|(i, _)| i);
    if let Some(i) = nearest {
        run.points.rotate_left(i);
    }
}

/// Which of `items` is nearest, by `distance`, ties to the earlier.
fn nearest<T>(items: &[T], distance: impl Fn(&T) -> f64) -> usize {
    items
        .iter()
        .enumerate()
        .fold((0, f64::MAX), |best, (i, item)| {
            let d = distance(item);
            if d < best.1 { (i, d) } else { best }
        })
        .0
}

/// How far `to` is from `at`: nowhere is as far as can be, and from nowhere
/// everything is as near.
fn distance(to: Option<DVec2>, at: Option<DVec2>) -> f64 {
    match (to, at) {
        (Some(to), Some(at)) => to.distance(at),
        (_, None) => 0.0,
        (None, Some(_)) => f64::MAX,
    }
}

/// `line`, thinned: each point dropped that is within `tolerance` of the line
/// through the ones kept either side of it, seen from above (Douglas and Peucker).
fn simplify(line: &[DVec2], closed: bool, tolerance: f64) -> Vec<DVec2> {
    let mut points = line.to_vec();
    if closed && let Some(&first) = line.first() {
        points.push(first);
    }
    if points.len() < 3 {
        return line.to_vec();
    }
    let mut keep = vec![false; points.len()];
    if let Some(k) = keep.first_mut() {
        *k = true;
    }
    if let Some(k) = keep.last_mut() {
        *k = true;
    }
    let mut spans = vec![(0usize, points.len() - 1)];
    // A closed line starts and ends at one point: split it at the point furthest
    // from there first, so the rest is judged against a real chord.
    if closed && let Some(&start) = points.first() {
        let far = points
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.distance(start).total_cmp(&b.1.distance(start)))
            .map_or(0, |(i, _)| i);
        if far > 0 && far < points.len() - 1 {
            if let Some(k) = keep.get_mut(far) {
                *k = true;
            }
            spans = vec![(0, far), (far, points.len() - 1)];
        }
    }
    while let Some((a, b)) = spans.pop() {
        let (Some(&pa), Some(&pb)) = (points.get(a), points.get(b)) else {
            continue;
        };
        let mut worst = (0usize, 0.0f64);
        for i in a + 1..b {
            let Some(&p) = points.get(i) else { continue };
            let d = closest_on_segment(p, pa, pb).distance(p);
            if d > worst.1 {
                worst = (i, d);
            }
        }
        if worst.1 > tolerance {
            if let Some(k) = keep.get_mut(worst.0) {
                *k = true;
            }
            spans.push((a, worst.0));
            spans.push((worst.0, b));
        }
    }
    let mut out: Vec<DVec2> = points
        .iter()
        .zip(&keep)
        .filter_map(|(&p, &k)| k.then_some(p))
        .collect();
    if closed {
        out.pop();
    }
    out
}

/// The point of segment `a`–`b` nearest `p`.
fn closest_on_segment(p: DVec2, a: DVec2, b: DVec2) -> DVec2 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 <= 0.0 {
        return a;
    }
    a + ab * ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
}

fn length(points: &[DVec2]) -> f64 {
    points
        .iter()
        .zip(points.iter().skip(1))
        .map(|(a, b)| a.distance(*b))
        .sum()
}

/// `a` with `b`'s counts added.
fn add(a: &mut ParallelReport, b: &ParallelReport) {
    a.refinement_limited += b.refinement_limited;
    a.discontinuities += b.discontinuities;
    a.passes += b.passes;
    a.coverage_breaks += b.coverage_breaks;
    a.wall_breaks += b.wall_breaks;
}

/// A whole, non-negative count that fits a `u32`.
fn count(v: f64) -> Option<u32> {
    // Checked in range first, so the narrowing is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (v.is_finite() && (0.0..=f64::from(u32::MAX)).contains(&v)).then_some(v as u32)
}

/// `n` as an index.
fn index(n: u64) -> Option<usize> {
    usize::try_from(n).ok()
}

fn region_err(e: camcraft_geom::contour::RegionError) -> StrategyError {
    StrategyError::Region(e.to_string())
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
    use camcraft_geom::contour::Contour;
    use camcraft_geom::synthetic;
    use glam::dvec2;

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
