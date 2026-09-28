//! Contour finishing: passes round the part at levels a stepdown apart, each
//! where the tool touches the walls at its height. Fusion's Contour, a waterline.
//!
//! # Where the passes run
//!
//! At each level the tool's tip may not be inside [`LevelReach::forbidden`]: the
//! part's shadow above the level, grown by the tool, band by band. Its boundary
//! is where the tool at that height meets the part, so the passes are its rings,
//! kept to the area. The region is conservative, a little larger than the true
//! one, so each point of a ring is then pulled in towards the part, by exact
//! drop-cutter queries, to within a small fraction of the tolerance of where the
//! tool first touches. It is never pulled past: every point is one where the
//! tool's tip at that height is clear. Between each two, the chord's middle is
//! pulled onto the part too, and kept where the chord would stray more than a
//! quarter of the tolerance into the part or off it. So the region need only
//! find the walls: it may be drawn far coarser than the tolerance.
//!
//! Every point of a pass is at its level: the part is followed by going round,
//! not over it, so a pass never runs down a wall and needs no breaking there.
//!
//! # Levels
//!
//! From the top down a stepdown apart, the last at the bottom, and one at each
//! flat face's height, so a pass runs round the foot of every wall that stands on
//! one.
//!
//! # Slopes
//!
//! Where the part is shallow, levels a stepdown apart are far apart along it. The
//! slope range keeps the passes to where the surface is at least so steep, as
//! Fusion's contact angle does, for a scallop or parallel pass to finish the rest.
//!
//! # Order and direction
//!
//! Each ring has the part on its left. Climbing with a clockwise spindle has the
//! part on the tool's right, so a ring is cut against the way it runs. By depth,
//! each level is cut before the next down; by area, each piece of the part's
//! footprint is cut all the way down before the next. Within a level the nearest
//! pass is cut next, each started nearest where the tool is.

use camcraft_geom::contour::{Region, polyline_length};
use camcraft_geom::mesh::distance_to_segment;
use glam::DVec2;
use rayon::prelude::*;

use crate::adaptive::Direction;
use crate::dropcutter::{DropCutter, DropTest};
use crate::nesting::nearest;
use crate::parallel::Run;
use crate::planar::pieces;
use crate::projected::{Emit, simplify, start_distance, start_near};
use crate::reach::LevelReach;
use crate::slope::{self, SlopeCounts, Sloped};
use crate::strategy::{StrategyError, check};
use crate::toolpath::Toolpath;

/// Levels closer than this are one, in millimetres.
const SAME_LEVEL: f64 = 1.0e-6;

/// How many times a chord is halved at most, to put it on the part.
const MAX_SPLITS: u32 = 16;

/// The most levels: far past any real part.
const MAX_LEVELS: usize = 100_000;

/// How far a point is pulled onto the part each time it is refined: the most
/// it is left short by is this fraction of the tolerance.
const PULL_TO: f64 = 0.02;

/// Which order the levels are cut in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Order {
    /// Each level all the way round before the next down. Fusion's default.
    #[default]
    ByDepth,
    /// Each piece of the part's footprint all the way down before the next.
    ByArea,
}

/// How to lay out the passes.
#[derive(Debug, Clone, PartialEq)]
pub struct WaterlineParams {
    /// Distance between levels, in millimetres.
    pub stepdown: f64,
    /// Highest level.
    pub top: f64,
    /// Lowest level.
    pub bottom: f64,
    /// Heights of flat faces, a level at each.
    pub flats: Vec<f64>,
    /// Chordal tolerance, in millimetres.
    pub tolerance: f64,
    /// Cutting feed, in mm/min.
    pub feed: f64,
    /// Which way each pass cuts.
    pub direction: Direction,
    /// Whether climbing puts the part on the tool's right, as it does with a
    /// clockwise spindle.
    pub climb_right: bool,
    /// Which order the levels are cut in.
    pub order: Order,
    /// Where the surface is at least this steep, in radians from level.
    pub slope_from: f64,
    /// And at most this steep.
    pub slope_to: f64,
}

impl Default for WaterlineParams {
    fn default() -> Self {
        Self {
            stepdown: 1.0,
            top: 0.0,
            bottom: 0.0,
            flats: Vec::new(),
            tolerance: 0.01,
            feed: 1000.0,
            direction: Direction::Climb,
            climb_right: true,
            order: Order::ByDepth,
            slope_from: 0.0,
            slope_to: std::f64::consts::FRAC_PI_2,
        }
    }
}

/// What happened while laying out the passes.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WaterlineReport {
    /// Passes cut.
    pub passes: u64,
    /// Levels.
    pub levels: u64,
    /// Rings the part has at them, within the area.
    pub rings: u64,
    /// Points pulled onto the part.
    pub pulled: u64,
    /// Points that could not be, where the part was not where the region said;
    /// left where the region put them, clear of it.
    pub unpulled: u64,
    /// Stretches left out for being outside the slope range asked for.
    pub outside_slope: u64,
    /// Stretches too short to be worth cutting.
    pub fragments: u64,
}

/// Contour passes over `area`, where the tool's centre may go.
///
/// The path holds [`crate::toolpath::MoveRole::Cut`] passes separated by
/// [`crate::toolpath::MoveRole::Link`] moves, in the order they are cut. The
/// linker resolves them.
pub fn waterline<T: DropTest + Sync>(
    reach: &mut LevelReach<'_>,
    cutter: &DropCutter<'_>,
    tool: &T,
    area: &Region,
    params: &WaterlineParams,
) -> Result<(Toolpath, WaterlineReport), StrategyError> {
    check("stepdown", params.stepdown)?;
    check("tolerance", params.tolerance)?;
    check("feed", params.feed)?;
    slope::check_range(params.slope_from, params.slope_to)?;
    if !(params.top.is_finite() && params.bottom.is_finite()) {
        return Err(StrategyError::BadParameter {
            name: "top",
            value: params.top,
        });
    }
    let mut report = WaterlineReport::default();
    let levels = levels(params);
    report.levels = levels.len() as u64;
    if levels.is_empty() || area.is_empty() {
        return Ok((Toolpath::new(), report));
    }

    // Where the tool may not be at each, all at once: the shadows are built
    // once, from the top down.
    let forbidden = reach.forbidden_all(&levels)?;
    // The pieces of the footprint at the lowest level, which every ring above
    // lies in, to cut by area.
    let footprint = forbidden.last().map(pieces).unwrap_or_default();

    let pitch = (tool.max_radius() / 2.0).max(params.tolerance);
    // The region overstates by its bands and the arcs it is drawn to: twice
    // both, and at least a tolerance, is well past them.
    let reach_by = params
        .tolerance
        .max(2.0 * (reach.band_step() + 2.0 * reach.arc_tolerance()));
    let everything = slope::is_everything(params.slope_from, params.slope_to);
    let ceiling = params.slope_to;
    let thin = 0.25 * params.tolerance;
    let cut_against = params.direction != Direction::Conventional && params.climb_right
        || params.direction == Direction::Conventional && !params.climb_right;

    // Each level's passes, in parallel.
    let worked: Vec<Result<Level, StrategyError>> = levels
        .par_iter()
        .zip(&forbidden)
        .map(|(&z, region)| {
            let mut level = Level::default();
            for ring in region.rings() {
                let island = island_of(ring.points(), &footprint);
                // Kept to the area first: the clipper rounds what it hands back
                // to its tick, which the region's own rings already lie on, and
                // would move a point pulled onto the part.
                let stretches = area.clip_lines(&[(ring.points().to_vec(), true)])?;
                if !stretches.is_empty() {
                    level.rings += 1;
                }
                for (points, closed) in stretches {
                    // Thinned first: the region's rings carry every facet of
                    // the part's shadow, and each point costs a dozen drops.
                    // Pulling puts back what the part needs between.
                    let points = simplify(&points, closed, thin);
                    let pulled = pull(
                        cutter,
                        tool,
                        (&points, closed),
                        z,
                        reach_by,
                        params.tolerance,
                    );
                    level.pulled += pulled.pulled;
                    level.unpulled += pulled.unpulled;
                    let points = simplify(&pulled.points, closed, thin);
                    let kept = if everything {
                        vec![(points, closed)]
                    } else {
                        Sloped::split(&points, closed, pitch, |m, scratch| {
                            slope::cl_slope(cutter, tool, m, pitch / 2.0, scratch)
                        })
                        .within(
                            params.slope_from,
                            ceiling,
                            params.slope_to,
                            &mut level.slopes,
                        )
                    };
                    for (mut points, closed) in kept {
                        let length = polyline_length(&points)
                            + if closed {
                                points
                                    .first()
                                    .zip(points.last())
                                    .map_or(0.0, |(a, b)| a.distance(*b))
                            } else {
                                0.0
                            };
                        if length < 2.0 * pitch {
                            level.fragments += 1;
                            continue;
                        }
                        if cut_against {
                            points.reverse();
                        }
                        level.runs.push((
                            island,
                            Run {
                                points: points.iter().map(|p| p.extend(z)).collect(),
                                closed,
                            },
                        ));
                    }
                }
            }
            Ok(level)
        })
        .collect();

    // The passes, each with its level and the piece of the footprint it is in.
    let mut passes: Vec<(usize, usize, Run)> = Vec::new();
    for (k, level) in worked.into_iter().enumerate() {
        let level = level?;
        report.rings += level.rings;
        report.pulled += level.pulled;
        report.unpulled += level.unpulled;
        report.outside_slope += level.slopes.outside_slope + level.slopes.too_steep;
        report.fragments += level.fragments;
        passes.extend(level.runs.into_iter().map(|(island, run)| (island, k, run)));
    }

    let mut cut = Cut {
        emit: Emit::new(params.feed),
        both_ways: params.direction == Direction::BothWays,
    };
    match params.order {
        Order::ByDepth => {
            for k in 0..levels.len() {
                let (here, rest): (Vec<_>, Vec<_>) = passes.into_iter().partition(|p| p.1 == k);
                passes = rest;
                cut.nearest_first(here.into_iter().map(|p| p.2).collect())?;
            }
        }
        Order::ByArea => {
            while !passes.is_empty() {
                // The piece whose highest pass is nearest next.
                let at = cut.emit.at;
                let top = passes.iter().map(|p| p.1).min().unwrap_or(0);
                let first = nearest(&passes, |p| {
                    if p.1 > top {
                        f64::MAX
                    } else {
                        at.map_or(0.0, |a| start_distance(&p.2, a, true))
                    }
                });
                let island = passes.get(first).map_or(0, |p| p.0);
                let (mine, rest): (Vec<_>, Vec<_>) =
                    passes.into_iter().partition(|p| p.0 == island);
                passes = rest;
                let mut mine = mine;
                for k in 0..levels.len() {
                    let (here, below): (Vec<_>, Vec<_>) = mine.into_iter().partition(|p| p.1 == k);
                    mine = below;
                    cut.nearest_first(here.into_iter().map(|p| p.2).collect())?;
                }
            }
        }
    }
    report.passes = cut.emit.passes;
    Ok((cut.emit.path, report))
}

/// The levels, from the top down: a stepdown apart, the last at the bottom, and
/// one at each flat face between.
fn levels(params: &WaterlineParams) -> Vec<f64> {
    if params.top < params.bottom {
        return Vec::new();
    }
    let mut out: Vec<f64> =
        crate::strategy::slabs(params.top, params.bottom, params.stepdown, &params.flats)
            .into_iter()
            .flat_map(|(z, flats)| std::iter::once(z).chain(flats))
            .collect();
    out.sort_by(|a, b| b.total_cmp(a));
    out.dedup_by(|a, b| (*a - *b).abs() < SAME_LEVEL);
    out.truncate(MAX_LEVELS);
    out
}

/// One level's passes, each with the piece of the footprint it is in; and what
/// was found on the way.
#[derive(Default)]
struct Level {
    runs: Vec<(usize, Run)>,
    rings: u64,
    pulled: u64,
    unpulled: u64,
    slopes: SlopeCounts,
    fragments: u64,
}

/// Which of `footprint` the ring `points` lies in, by a point just to its left,
/// in the part: its index, or one past the last if none.
fn island_of(points: &[DVec2], footprint: &[Region]) -> usize {
    let probe = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .find(|(a, b)| a.distance(**b) > 1e-6)
        .map(|(a, b)| a.midpoint(*b) + (*b - *a).normalize().perp() * 1e-4);
    probe
        .and_then(|p| footprint.iter().position(|piece| piece.contains(p)))
        .unwrap_or(footprint.len())
}

/// A ring's points, each pulled onto the part at level `z`.
struct Pulled {
    points: Vec<DVec2>,
    pulled: u64,
    unpulled: u64,
}

/// Moves points onto the part at level `z`: towards it until the tool's tip is
/// within [`PULL_TO`] of the tolerance of first touching it, looking no further
/// than `reach_by`; and never past.
struct Puller<'a, 'm, T> {
    cutter: &'a DropCutter<'m>,
    tool: &'a T,
    z: f64,
    reach_by: f64,
    tolerance: f64,
    scratch: Vec<u32>,
}

impl<T: DropTest> Puller<'_, '_, T> {
    /// Whether the tool's tip at the level over `p` is in the part.
    fn touches(&mut self, p: DVec2) -> bool {
        self.cutter
            .drop_at_in(self.tool, p.x, p.y, &mut self.scratch)
            .is_some_and(|h| h > self.z)
    }

    /// `p` moved along `inward`, towards the part, onto it: `None` if it is not
    /// within reach either way.
    fn onto(&mut self, p: DVec2, inward: DVec2) -> Option<DVec2> {
        // Between a point clear of the part and one in it: in from `p`, or,
        // where it is already in it, out.
        let (mut clear, mut into) = if self.touches(p) {
            if self.touches(p - inward * self.reach_by) {
                return None;
            }
            (-self.reach_by, 0.0)
        } else {
            if !self.touches(p + inward * self.reach_by) {
                return None;
            }
            (0.0, self.reach_by)
        };
        let close = PULL_TO * self.tolerance;
        while into - clear > close {
            let mid = 0.5 * (clear + into);
            if self.touches(p + inward * mid) {
                into = mid;
            } else {
                clear = mid;
            }
        }
        Some(p + inward * clear)
    }

    /// Points on the part between `a` and `b`, both on it, so that no chord
    /// between them strays more than a quarter of the tolerance into the part
    /// or off it: each chord's middle pulled onto the part, and kept where it is
    /// further, the halves then looked at the same way.
    fn between(&mut self, a: DVec2, b: DVec2, depth: u32, out: &mut Vec<DVec2>) {
        let Some(inward) = (b - a).try_normalize().map(DVec2::perp) else {
            return;
        };
        if depth > MAX_SPLITS || a.distance(b) < 4.0 * PULL_TO * self.tolerance {
            return;
        }
        // A chord into a convex wall has its middle's point on the part outside
        // it, as one off a concave wall inside: either way it is how far off
        // the chord strays.
        let Some(on) = self.onto(a.midpoint(b), inward) else {
            return;
        };
        if distance_to_segment(on, a, b) > 0.25 * self.tolerance {
            self.between(a, on, depth + 1, out);
            out.push(on);
            self.between(on, b, depth + 1, out);
        }
    }
}

/// The stretch of a ring `points`, closed if `closed`, the part on its left,
/// each point pulled onto the part, and points added between where a chord
/// would stray into the part or off it: the points, and the chords, each in
/// parallel and in their order.
///
/// The region the ring comes from need only be near the part, and never inside
/// what the tool may reach: every point of the pass is placed by the part
/// itself.
fn pull<T: DropTest + Sync>(
    cutter: &DropCutter<'_>,
    tool: &T,
    (points, closed): (&[DVec2], bool),
    z: f64,
    reach_by: f64,
    tolerance: f64,
) -> Pulled {
    let n = points.len();
    let puller = || Puller {
        cutter,
        tool,
        z,
        reach_by,
        tolerance,
        scratch: Vec::new(),
    };
    // Each point on the part, or where it was if it could not be put there.
    let placed: Vec<(DVec2, bool)> = (0..n)
        .into_par_iter()
        .map_init(puller, |on, i| {
            let Some(&p) = points.get(i) else {
                return (DVec2::ZERO, false);
            };
            // Towards the part: the left of the ring, across the neighbours
            // either side far enough off to give it a direction, as the
            // clipper's points can be a tick apart. An open stretch's ends lean
            // on one side.
            let apart = 0.5 * tolerance;
            let neighbour = |step: usize| {
                let mut j = i;
                for _ in 1..n {
                    j = if closed {
                        (j + step) % n
                    } else if step == 1 {
                        (j + 1).min(n - 1)
                    } else {
                        j.saturating_sub(1)
                    };
                    match points.get(j) {
                        Some(&q) if q.distance(p) > apart => return q,
                        _ if !closed && (j == 0 || j == n - 1) => break,
                        _ => {}
                    }
                }
                points.get(j).copied().unwrap_or(p)
            };
            let (prev, next) = (neighbour(n - 1), neighbour(1));
            (next - prev)
                .try_normalize()
                .map(DVec2::perp)
                .and_then(|inward| on.onto(p, inward))
                .map_or((p, false), |q| (q, true))
        })
        .collect();
    let pulled = placed.iter().filter(|p| p.1).count() as u64;
    // Between each two on the part.
    let ends = if closed { n } else { n.saturating_sub(1) };
    let between: Vec<Vec<DVec2>> = (0..ends)
        .into_par_iter()
        .map_init(puller, |on, i| {
            let mut added = Vec::new();
            if let (Some(&(a, true)), Some(&(b, true))) = (placed.get(i), placed.get((i + 1) % n)) {
                on.between(a, b, 0, &mut added);
            }
            added
        })
        .collect();
    let mut out = Vec::with_capacity(n + between.iter().map(Vec::len).sum::<usize>());
    for (i, added) in between.into_iter().enumerate() {
        if let Some(&(a, _)) = placed.get(i) {
            out.push(a);
        }
        out.extend(added);
    }
    if !closed && let Some(&(last, _)) = placed.last() {
        out.push(last);
    }
    Pulled {
        points: out,
        pulled,
        unpulled: n as u64 - pulled,
    }
}

/// Cutting passes into the path, the nearest next, from where the tool is.
struct Cut {
    emit: Emit,
    /// Whether an open pass may be started at either end.
    both_ways: bool,
}

impl Cut {
    fn nearest_first(&mut self, mut runs: Vec<Run>) -> Result<(), StrategyError> {
        while !runs.is_empty() {
            let at = self.emit.at;
            let both_ways = self.both_ways;
            let k = nearest(&runs, |r| {
                at.map_or(0.0, |a| start_distance(r, a, both_ways))
            });
            let mut run = runs.remove(k);
            if let Some(at) = self.emit.at {
                if run.closed {
                    start_near(&mut run, at);
                } else if both_ways
                    && let (Some(first), Some(last)) = (run.points.first(), run.points.last())
                    && last.truncate().distance(at) < first.truncate().distance(at)
                {
                    run.points.reverse();
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
    clippy::expect_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::allowance::{Allowance, EffectiveTool};
    use crate::tool::{Ball, Flat};
    use crate::toolpath::MoveRole;
    use camcraft_geom::mesh::TriMesh;
    use camcraft_geom::synthetic;
    use glam::DVec3;
    use glam::dvec2;

    const TOLERANCE: f64 = 0.02;

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

    /// A pass's level, if every point of it is at it.
    fn at_level(run: &[DVec3]) -> Option<f64> {
        let z = run.first()?.z;
        run.iter().all(|p| (p.z - z).abs() < 1e-12).then_some(z)
    }

    fn run(
        mesh: &TriMesh,
        tool: &EffectiveTool,
        params: &WaterlineParams,
    ) -> (Toolpath, WaterlineReport) {
        let cutter = DropCutter::new(mesh);
        let mut reach = LevelReach::new(mesh, tool, params.tolerance).unwrap();
        let area = Region::rectangle(dvec2(-30.0, -30.0), dvec2(30.0, 30.0));
        waterline(&mut reach, &cutter, tool, &area, params).unwrap()
    }

    /// A 10 mm square boss, 5 mm tall.
    fn boss() -> TriMesh {
        synthetic::cuboid(DVec3::new(-5.0, -5.0, 0.0), DVec3::new(5.0, 5.0, 5.0))
    }

    /// A sphere of radius 10 about the origin: a dome above the XY plane.
    fn dome() -> TriMesh {
        synthetic::uv_sphere(10.0, 60, 90)
    }

    fn ball(r: f64) -> EffectiveTool {
        EffectiveTool::new(&Ball::new(r).unwrap(), Allowance::none()).unwrap()
    }

    fn flat(r: f64) -> EffectiveTool {
        EffectiveTool::new(&Flat::new(r).unwrap(), Allowance::none()).unwrap()
    }

    fn params(top: f64, bottom: f64, stepdown: f64) -> WaterlineParams {
        WaterlineParams {
            stepdown,
            top,
            bottom,
            tolerance: TOLERANCE,
            ..Default::default()
        }
    }

    #[test]
    fn a_boss_has_a_ring_at_every_level_on_its_wall() {
        // A 10 mm square boss 5 mm tall on a plate.
        let mesh = boss();
        let tool = flat(1.5);
        let (path, report) = run(&mesh, &tool, &params(5.0, 0.0, 1.0));
        let passes = passes(&path);
        assert!(report.levels >= 5, "{report:?}");
        assert!(!passes.is_empty());
        for pass in &passes {
            let z = at_level(pass).expect("every pass at its level");
            assert!((0.0..=5.0 + 1e-9).contains(&z));
            // Each point is a tool radius off the boss's side, within tolerance.
            for p in pass {
                let d = (p.x.abs().max(p.y.abs())) - 5.0;
                let corner = (dvec2(p.x.abs(), p.y.abs()) - dvec2(5.0, 5.0)).max(DVec2::ZERO);
                let off = if corner.x > 0.0 && corner.y > 0.0 {
                    corner.length()
                } else {
                    d
                };
                assert!(
                    (off - 1.5).abs() <= TOLERANCE,
                    "{p} is {off} off the boss, not 1.5"
                );
            }
        }
    }

    #[test]
    fn every_point_is_where_the_tool_first_touches() {
        let mesh = dome();
        let tool = ball(2.0);
        let cutter = DropCutter::new(&mesh);
        let (path, report) = run(&mesh, &tool, &params(9.0, 0.5, 1.0));
        assert!(report.pulled > 0, "{report:?}");
        for pass in passes(&path) {
            for &p in &pass {
                let h = cutter.drop_at(&tool, p.x, p.y).unwrap();
                // Clear, and touching within the tolerance.
                assert!(h <= p.z + 1e-9, "{p} is in the part: {h}");
                assert!(p.z - h <= TOLERANCE, "{p} is {} off the part", p.z - h);
            }
        }
    }

    #[test]
    fn a_coarse_region_still_puts_every_point_on_the_part() {
        let mesh = dome();
        let tool = ball(2.0);
        let cutter = DropCutter::new(&mesh);
        // Bands and arcs ten times coarser than the tolerance asked for.
        let mut reach = LevelReach::new(&mesh, &tool, 10.0 * TOLERANCE).unwrap();
        let area = Region::rectangle(dvec2(-30.0, -30.0), dvec2(30.0, 30.0));
        let (path, report) =
            waterline(&mut reach, &cutter, &tool, &area, &params(9.0, 0.5, 1.0)).unwrap();
        assert!(report.passes > 5, "{report:?}");
        for pass in passes(&path) {
            for w in pass.windows(2) {
                let (a, b) = (w[0], w[1]);
                // Each point clear of the part and touching it within the
                // tolerance; a chord between, as any pass's, within the
                // tolerance of it either way.
                for p in [a, b] {
                    let h = cutter.drop_at(&tool, p.x, p.y).unwrap();
                    assert!(h <= p.z + 1e-9, "{p} is in the part: {h}");
                    assert!(p.z - h <= TOLERANCE, "{p} is {} off the part", p.z - h);
                }
                // Measured across the chord, as a wall's is: on a steep one a
                // hair across is a long way up or down.
                let m = a.midpoint(b);
                let across = (b - a).truncate().normalize().perp() * TOLERANCE;
                let clear = |q: glam::DVec2| {
                    cutter
                        .drop_at(&tool, q.x, q.y)
                        .is_none_or(|h| h <= m.z + 1e-9)
                };
                let (here, left, right) =
                    (m.truncate(), m.truncate() + across, m.truncate() - across);
                assert!(
                    clear(here) || clear(left) || clear(right),
                    "{m} is more than the tolerance into the part"
                );
                assert!(
                    !clear(left) || !clear(right),
                    "{m} is more than the tolerance off the part"
                );
            }
        }
    }

    #[test]
    fn climbing_goes_round_the_part_clockwise() {
        let mesh = boss();
        let tool = flat(1.5);
        let (path, _) = run(&mesh, &tool, &params(5.0, 0.0, 2.5));
        for pass in passes(&path) {
            let flat: Vec<DVec2> = pass.iter().map(|p| p.truncate()).collect();
            let area = camcraft_geom::contour::Contour::new(flat)
                .unwrap()
                .signed_area();
            assert!(area < 0.0, "a climbing ring round a boss goes clockwise");
        }
    }

    #[test]
    fn a_slope_range_leaves_the_shallow_top() {
        let mesh = dome();
        let tool = ball(1.0);
        let all = run(&mesh, &tool, &params(9.9, 0.5, 0.5)).1;
        let steep = run(
            &mesh,
            &tool,
            &WaterlineParams {
                slope_from: 45f64.to_radians(),
                ..params(9.9, 0.5, 0.5)
            },
        );
        assert!(steep.1.outside_slope > 0, "{:?}", steep.1);
        for pass in passes(&steep.0) {
            // Steeper than 45 degrees on a hemisphere is below its radius / √2.
            assert!(pass[0].z < 10.0 / 2f64.sqrt() + 0.5, "{:?}", pass[0]);
        }
        assert!(steep.1.passes < all.passes);
    }

    #[test]
    fn by_area_cuts_each_boss_all_the_way_down_first() {
        let left = synthetic::cuboid(DVec3::new(-11.0, -3.0, 0.0), DVec3::new(-5.0, 3.0, 4.0));
        let right = synthetic::cuboid(DVec3::new(5.0, -3.0, 0.0), DVec3::new(11.0, 3.0, 4.0));
        let mesh = TriMesh::merged(&[&left, &right]).unwrap();
        let tool = flat(1.0);
        let by_area = WaterlineParams {
            order: Order::ByArea,
            ..params(4.0, 0.0, 1.0)
        };
        let (path, _) = run(&mesh, &tool, &by_area);
        let sides: Vec<bool> = passes(&path).iter().map(|p| p[0].x > 0.0).collect();
        let changes = sides.windows(2).filter(|w| w[0] != w[1]).count();
        assert_eq!(changes, 1, "{sides:?}");
    }

    #[test]
    fn is_deterministic() {
        let mesh = dome();
        let tool = ball(2.0);
        let p = params(9.0, 0.5, 1.0);
        let a = run(&mesh, &tool, &p).0;
        let b = run(&mesh, &tool, &p).0;
        assert_eq!(a.moves(), b.moves());
    }
}
