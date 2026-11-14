//! Ramp finishing: the tool going down each wall without stopping, round and
//! round it, a stepdown each time round, from the wall's top to its foot.
//! Fusion's Ramp.
//!
//! # Rings, and walls
//!
//! The rings are contour finishing's (`waterline::passes_at_levels`): at each
//! level a stepdown apart from the top, and at each flat face, the rings of where the
//! tool may not be, kept to the area and pulled onto the part. A wall is the
//! rings one above another round the same piece of the part, or round the same
//! hole in it: a ring round a piece of the part is followed by the ring below
//! it round the piece that holds it, a ring round a hole by the ring above it
//! round the hole that holds it. Where pieces meet going down, the one nearest
//! the ring below goes on down round it and the others end; where a hole parts
//! in two, its wall ends, and each ring below starts one of its own.
//!
//! # Down a wall
//!
//! As Fusion's programs for `fixtures/step/ops_3d.step` go ([decision 0027]):
//!
//! - From the level above the wall's top ring, or the Top for the first level,
//!   along that ring, which is clear of the part at every height above it, down
//!   to its level, at no more than the ramp angle, and no more than the step
//!   down that ring's length. It starts at the ring's point nearest the lower
//!   left of the box round it.
//! - Once round it at its level, but on the first wall the operation cuts.
//! - Down to each ring below from the one above, the tool on the part at every
//!   height between: the ring below pulled onto the part at the height the tool
//!   is at as it goes. It falls at no more than the ramp angle, and no more than
//!   the step down the longer of the two rings, measured along the way it
//!   goes: round a boss a stepdown each time round the ring below, in a pocket
//!   each time round the ring above.
//! - Once round the wall's last ring, at its level.
//!
//! Every point is on the part at its own height, never in it: the ring below is
//! clear of the part at every height above it, and is only ever pulled towards
//! the part until the tool first touches it. The chords between are kept on it
//! as contour finishing's are, and each point's height is worked out along the
//! lap as cut, chords and all, then the point put on the part there.
//!
//! The next wall is the one whose top ring is nearest where the tool is; the
//! first, of those starting highest, the largest.
//!
//! # What is not a wall
//!
//! A ring the area or the slope range breaks open is cut at its level, as
//! contour finishing cuts it, after the walls: Fusion's programs cut what its
//! slope range leaves open so.
//!
//! [decision 0027]: ../../../../docs/decisions/0027-ramp.md

use camcraft_geom::contour::{Region, closed_edges};
use camcraft_geom::mesh::distance_to_segment;
use glam::{DVec2, DVec3};
use rayon::prelude::*;

use crate::control::Watch;
use crate::cutter::dropcutter::{DropCutter, DropTest};
use crate::path::toolpath::{MoveRole, Toolpath};
use crate::strategy::adaptive::Direction;
use crate::strategy::parallel::Run;
use crate::strategy::projected::Emit;
use crate::strategy::reach::LevelReach;
use crate::strategy::waterline::{
    Found, Order, PULL_TO, Puller, WaterlineParams, passes_at_levels,
};
use crate::strategy::{StrategyError, check, slope};

/// How many times a chord is halved at most, to put it on the part.
const MAX_SPLITS: u32 = 16;

/// The most times round a ring one step down takes: far past any ramp angle
/// a job asks for on a ring of any size.
const MAX_LAPS: u16 = 4096;

/// How far a point of a step is looked for either way when it is put on the
/// part again at its height, in millimetres: its height only changes by what
/// the points added between it and the last change the way round.
const REPULL_REACH: f64 = 2.0;

/// How to lay out the passes.
#[derive(Debug, Clone, PartialEq)]
pub struct RampParams {
    /// Distance between levels, in millimetres: how far down each time round.
    pub stepdown: f64,
    /// Where the levels start: the first is a stepdown below it.
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
    /// Where the surface is at least this steep, in radians from level.
    pub slope_from: f64,
    /// And at most this steep.
    pub slope_to: f64,
    /// The steepest the tool goes down, in radians from level.
    pub ramp: f64,
    /// What it tells how far it has got, and asks whether to stop.
    pub watch: Watch,
}

impl Default for RampParams {
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
            slope_from: 0.0,
            slope_to: std::f64::consts::FRAC_PI_2,
            ramp: 2f64.to_radians(),
            watch: Watch::default(),
        }
    }
}

/// What happened while laying out the passes.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RampReport {
    /// Passes cut: walls, and rings cut at their level.
    pub passes: u64,
    /// Levels.
    pub levels: u64,
    /// Walls gone down.
    pub walls: u64,
    /// Closed rings the walls are made of.
    pub rings: u64,
    /// Stretches cut at their level, open.
    pub open: u64,
    /// Points pulled onto the part, at the levels and between them.
    pub pulled: u64,
    /// Points that could not be, left where they were, clear of it.
    pub unpulled: u64,
    /// Stretches left out for being outside the slope range asked for.
    pub outside_slope: u64,
    /// Stretches too short to be worth cutting.
    pub fragments: u64,
}

/// Ramp passes over `area`, where the tool's centre may go.
///
/// The path holds [`MoveRole::Cut`] passes separated by [`MoveRole::Link`]
/// moves, in the order they are cut. The linker resolves them.
pub fn ramp<T: DropTest + Sync>(
    reach: &mut LevelReach<'_>,
    cutter: &DropCutter<'_>,
    tool: &T,
    area: &Region,
    params: &RampParams,
) -> Result<(Toolpath, RampReport), StrategyError> {
    check("stepdown", params.stepdown)?;
    check("tolerance", params.tolerance)?;
    check("feed", params.feed)?;
    if !(params.ramp > 0.0 && params.ramp < std::f64::consts::FRAC_PI_2) {
        return Err(StrategyError::BadParameter {
            name: "ramp",
            value: params.ramp,
        });
    }
    slope::check_range(params.slope_from, params.slope_to)?;
    if !(params.top.is_finite() && params.bottom.is_finite()) {
        return Err(StrategyError::BadParameter {
            name: "top",
            value: params.top,
        });
    }
    let found = passes_at_levels(
        reach,
        cutter,
        tool,
        area,
        &WaterlineParams {
            stepdown: params.stepdown,
            top: params.top,
            bottom: params.bottom,
            flats: params.flats.clone(),
            tolerance: params.tolerance,
            feed: params.feed,
            direction: params.direction,
            climb_right: params.climb_right,
            order: Order::ByDepth,
            slope_from: params.slope_from,
            slope_to: params.slope_to,
            ramp: None,
            watch: params.watch.part(0.0, 0.5),
        },
    )?;
    let reach_by = params.tolerance.max(2.0 * reach.overstates());
    let against = params.climb_right != (params.direction == Direction::Conventional);
    let mut report = RampReport {
        levels: found.levels.len() as u64,
        ..RampReport::default()
    };
    let Found { levels, passes } = found;

    // Each level's closed rings, and what is open.
    let mut rings: Vec<Vec<Ring>> = Vec::with_capacity(levels.len());
    let mut open: Vec<Vec<Run>> = Vec::with_capacity(levels.len());
    for (i, level) in passes.into_iter().enumerate() {
        report.pulled += level.pulled;
        report.unpulled += level.unpulled;
        report.outside_slope += level.slopes.outside_slope + level.slopes.too_steep;
        report.fragments += level.fragments;
        let (mut closed, mut at_level) = (Vec::new(), Vec::new());
        for (_, run) in level.runs {
            match run
                .closed
                .then(|| Ring::new(i, &run, against, params.tolerance))
                .flatten()
            {
                Some(ring) => closed.push(ring),
                None => at_level.push(run),
            }
        }
        report.rings += closed.len() as u64;
        report.open += at_level.len() as u64;
        rings.push(closed);
        open.push(at_level);
    }

    let walls = walls(&rings);
    report.walls = walls.len() as u64;
    let mut down = Down {
        cutter,
        tool,
        params,
        reach_by,
        side: if against { -1.0 } else { 1.0 },
        path: Toolpath::new(),
        at: None,
        pulled: 0,
        unpulled: 0,
    };
    let mut left: Vec<Vec<(usize, usize)>> = walls;
    let share = 0.5 * crate::control::share(1, left.len().max(1));
    let mut first = true;
    while let Some(k) = next_wall(&left, &rings, down.at) {
        params.watch.check()?;
        let wall = left.swap_remove(k);
        let wall: Vec<&Ring> = wall
            .iter()
            .filter_map(|&(i, j)| rings.get(i)?.get(j))
            .collect();
        let above = match wall.first().map(|r| r.level) {
            Some(0) | None => params.top,
            Some(i) => levels.get(i - 1).copied().unwrap_or(params.top),
        };
        down.wall(&wall, above, first)?;
        first = false;
        report.passes += 1;
        params.watch.add(share);
    }
    report.pulled += down.pulled;
    report.unpulled += down.unpulled;

    // What is open, at its level, level by level, as contour finishing cuts it.
    let mut emit = Emit::new(params.feed);
    emit.at = down.at;
    let both_ways = params.direction == Direction::BothWays;
    for runs in open {
        emit.nearest_first(runs, both_ways)?;
    }
    report.passes += emit.passes;
    let mut path = down.path;
    path.append(emit.path.moves())?;
    Ok((path, report))
}

/// A closed ring at a level, as it is cut.
struct Ring {
    /// Which level.
    level: usize,
    /// Its height.
    z: f64,
    /// Its points, in the order cut; the last is not the first again.
    points: Vec<DVec2>,
    /// How far along it each point is, from the first.
    along: Vec<f64>,
    /// How long it is, round.
    length: f64,
    /// Whether it goes round a piece of the part, rather than a hole in it.
    boss: bool,
    /// A point just inside the part beside it, and one just outside.
    probe_part: DVec2,
    probe_free: DVec2,
    /// The area it encloses.
    area: f64,
}

impl Ring {
    /// `run`, closed, at level `level`: the part on its right if `against`,
    /// as it is cut, otherwise on its left. `None` if it has no length.
    fn new(level: usize, run: &Run, against: bool, tolerance: f64) -> Option<Self> {
        let z = run.points.first()?.z;
        let points: Vec<DVec2> = run.points.iter().map(|p| p.truncate()).collect();
        let n = points.len();
        if n < 3 {
            return None;
        }
        let mut along = Vec::with_capacity(n);
        let mut length = 0.0;
        for (a, b) in closed_edges(&points) {
            along.push(length);
            length += a.distance(b);
        }
        let signed = signed_area(&points);
        // The part is on a ring's left as it is found, so a ring going round
        // anticlockwise as found goes round the part.
        let boss = (signed > 0.0) != against;
        let side = if against { -1.0 } else { 1.0 };
        let (a, b) = closed_edges(&points).find(|(a, b)| a.distance(*b) > 4.0 * tolerance)?;
        let normal = (b - a).normalize().perp() * side;
        let mid = a.midpoint(b);
        Some(Self {
            level,
            z,
            points,
            along,
            length,
            boss,
            probe_part: mid + normal * tolerance,
            probe_free: mid - normal * tolerance,
            area: signed.abs(),
        })
        .filter(|r| r.length > 0.0)
    }

    /// The point `u` along it, round from its first.
    fn at(&self, u: f64) -> DVec2 {
        let u = u.rem_euclid(self.length);
        let i = self.along.partition_point(|&a| a <= u).saturating_sub(1);
        let n = self.points.len();
        match (
            self.points.get(i),
            self.points.get((i + 1) % n),
            self.along.get(i),
        ) {
            (Some(&a), Some(&b), Some(&s)) => {
                let piece = a.distance(b);
                if piece > 0.0 {
                    a.lerp(b, ((u - s) / piece).clamp(0.0, 1.0))
                } else {
                    a
                }
            }
            _ => self.points.first().copied().unwrap_or(DVec2::ZERO),
        }
    }

    /// How far along it the point nearest `p` is.
    fn nearest(&self, p: DVec2) -> f64 {
        let mut best = (f64::INFINITY, 0.0);
        for ((a, b), &s) in closed_edges(&self.points).zip(&self.along) {
            let d = b - a;
            let t = if d.length_squared() > 0.0 {
                ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let gap = p.distance(a + d * t);
            if gap < best.0 {
                best = (gap, s + t * d.length());
            }
        }
        best.1
    }

    /// The lower left corner of the box round it.
    fn lower_left(&self) -> DVec2 {
        self.points
            .iter()
            .fold(DVec2::splat(f64::INFINITY), |m, p| m.min(*p))
    }

    /// How far `p` is from it.
    fn distance(&self, p: DVec2) -> f64 {
        p.distance(self.at(self.nearest(p)))
    }

    /// Whether `p` is inside it.
    fn contains(&self, p: DVec2) -> bool {
        let crossed = |&(a, b): &(DVec2, DVec2)| {
            (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x)
        };
        closed_edges(&self.points).filter(crossed).count() % 2 == 1
    }

    /// Its points after `u` and up to `u + to`, round as far as that goes,
    /// each with how far along it is from `u`.
    fn ahead(&self, u: f64, to: f64) -> Vec<(f64, usize)> {
        let n = self.points.len();
        let u = u.rem_euclid(self.length);
        let mut i = self.along.partition_point(|&a| a <= u);
        let mut lap = 0.0;
        if i >= n {
            i = 0;
            lap = self.length;
        }
        let mut out = Vec::new();
        let most = n.saturating_mul(usize::from(MAX_LAPS) + 2);
        for _ in 0..most {
            let Some(&s) = self.along.get(i) else {
                break;
            };
            let gone = s + lap - u;
            if gone > to {
                break;
            }
            if gone > 0.0 {
                out.push((gone, i));
            }
            i += 1;
            if i == n {
                i = 0;
                lap += self.length;
            }
        }
        out
    }

    /// Towards the part from its point `i`: to the left of it as cut, across
    /// the points either side far enough off to give it a direction, times
    /// `side`.
    fn inward(&self, i: usize, side: f64, apart: f64) -> Option<DVec2> {
        let n = self.points.len();
        let p = *self.points.get(i)?;
        let neighbour = |forward: bool| {
            let mut j = i;
            for _ in 1..n {
                j = if forward {
                    (j + 1) % n
                } else {
                    (j + n - 1) % n
                };
                if let Some(&q) = self.points.get(j)
                    && q.distance(p) > apart
                {
                    return q;
                }
            }
            p
        };
        (neighbour(true) - neighbour(false))
            .try_normalize()
            .map(|d| d.perp() * side)
    }
}

/// Twice the signed area of a closed polygon, anticlockwise positive.
fn signed_area(points: &[DVec2]) -> f64 {
    closed_edges(points)
        .map(|(a, b)| a.perp_dot(b))
        .sum::<f64>()
        * 0.5
}

/// The walls: each the rings one below another round the same piece or hole,
/// as `(level, ring)`, from the top down.
fn walls(rings: &[Vec<Ring>]) -> Vec<Vec<(usize, usize)>> {
    // Which ring of the level below each ring is followed by, where it is the
    // only one followed by it.
    let mut next: Vec<Vec<Option<usize>>> = rings.iter().map(|l| vec![None; l.len()]).collect();
    let innermost = |level: &[Ring], p: DVec2, boss: bool| {
        level
            .iter()
            .enumerate()
            .filter(|(_, r)| r.boss == boss && r.contains(p))
            .min_by(|a, b| a.1.area.total_cmp(&b.1.area))
            .map(|(j, _)| j)
    };
    for i in 0..rings.len().saturating_sub(1) {
        let (Some(upper), Some(lower)) = (rings.get(i), rings.get(i + 1)) else {
            continue;
        };
        // Each pair: (upper, lower).
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        for (a, ring) in upper.iter().enumerate() {
            // The part only grows going down: a piece is in a piece below.
            if ring.boss
                && let Some(b) = innermost(lower, ring.probe_part, true)
            {
                pairs.push((a, b));
            }
        }
        for (b, ring) in lower.iter().enumerate() {
            // And a hole only shrinks: a hole is in a hole above.
            if !ring.boss
                && let Some(a) = innermost(upper, ring.probe_free, false)
            {
                pairs.push((a, b));
            }
        }
        // A hole that parts in two going down ends there. Pieces that meet
        // going down are followed by the ring below round them all from the
        // one nearest it, as round a boss whose foot meets the walls beside
        // it the outside carries on: the others end.
        for &(a, b) in &pairs {
            let downs = pairs.iter().filter(|p| p.0 == a).count();
            let nearest = pairs
                .iter()
                .filter(|p| p.1 == b)
                .filter_map(|p| {
                    let ring = upper.get(p.0)?;
                    Some((p.0, lower.get(b)?.distance(ring.probe_part)))
                })
                .min_by(|x, y| x.1.total_cmp(&y.1))
                .map(|(a, _)| a);
            if downs == 1
                && nearest == Some(a)
                && let Some(slot) = next.get_mut(i).and_then(|l| l.get_mut(a))
            {
                *slot = Some(b);
            }
        }
    }
    let mut followed: Vec<Vec<bool>> = rings.iter().map(|l| vec![false; l.len()]).collect();
    for (i, level) in next.iter().enumerate() {
        for b in level.iter().flatten() {
            if let Some(f) = followed.get_mut(i + 1).and_then(|l| l.get_mut(*b)) {
                *f = true;
            }
        }
    }
    let mut out = Vec::new();
    for (i, level) in rings.iter().enumerate() {
        for j in 0..level.len() {
            if followed
                .get(i)
                .and_then(|l| l.get(j))
                .copied()
                .unwrap_or(true)
            {
                continue;
            }
            let mut wall = vec![(i, j)];
            let (mut li, mut lj) = (i, j);
            while let Some(b) = next.get(li).and_then(|l| l.get(lj)).copied().flatten() {
                li += 1;
                lj = b;
                wall.push((li, lj));
            }
            out.push(wall);
        }
    }
    out
}

/// Which of `walls` to go down next: the one whose top ring is nearest `at`,
/// or, before any, of those starting highest, the largest.
fn next_wall(
    walls: &[Vec<(usize, usize)>],
    rings: &[Vec<Ring>],
    at: Option<DVec2>,
) -> Option<usize> {
    let ring = |w: &Vec<(usize, usize)>| w.first().and_then(|&(i, j)| rings.get(i)?.get(j));
    walls
        .iter()
        .enumerate()
        .filter_map(|(k, w)| Some((k, ring(w)?)))
        .min_by(|a, b| match at {
            Some(p) => a.1.distance(p).total_cmp(&b.1.distance(p)),
            None => {
                a.1.level
                    .cmp(&b.1.level)
                    .then(b.1.area.total_cmp(&a.1.area))
            }
        })
        .map(|(k, _)| k)
}

/// The walls written into a path, one after another.
struct Down<'a, 'm, T> {
    cutter: &'a DropCutter<'m>,
    tool: &'a T,
    params: &'a RampParams,
    reach_by: f64,
    /// Which side of a ring as cut the part is: 1 on its left, -1 on its
    /// right.
    side: f64,
    path: Toolpath,
    /// Where the tool was left, seen from above.
    at: Option<DVec2>,
    pulled: u64,
    unpulled: u64,
}

impl<T: DropTest + Sync> Down<'_, '_, T> {
    fn puller(&self) -> Puller<'_, '_, T> {
        Puller {
            cutter: self.cutter,
            tool: self.tool,
            z: 0.0,
            reach_by: self.reach_by,
            tolerance: self.params.tolerance,
            scratch: Vec::new(),
        }
    }

    fn cut(&mut self, p: DVec3) -> Result<(), StrategyError> {
        if self.path.end().is_none_or(|e| e.distance(p) > 1e-9) {
            self.path.feed_to(p, MoveRole::Cut, self.params.feed)?;
        }
        Ok(())
    }

    /// One wall, `rings` from the top down, started from `above`, once round
    /// its top ring at its level unless it is the `first` cut.
    fn wall(&mut self, rings: &[&Ring], above: f64, first: bool) -> Result<(), StrategyError> {
        let Some(&top) = rings.first() else {
            return Ok(());
        };
        let tan = self.params.ramp.tan();
        // Started at its point nearest the lower left of the box round it,
        // as Fusion's start: the pocket's at its lower left corner, the
        // dome's at 227° round it.
        let mut u = top.nearest(top.lower_left());
        let start = top.at(u);
        self.path
            .feed_to(start.extend(above), MoveRole::Link, self.params.feed)?;
        // Down along the top ring, at its level's XY, from the level above.
        let drop = (above - top.z).max(0.0);
        if drop > 1e-9 {
            let slope = tan.min(drop / top.length);
            u = self.along(top, u, above, drop / slope, slope)?;
        }
        if !first || rings.len() == 1 {
            u = self.round(top, u)?;
        }
        for pair in rings.windows(2) {
            let &[upper, lower] = pair else {
                continue;
            };
            let drop = upper.z - lower.z;
            if drop <= 1e-9 {
                continue;
            }
            let slope = tan.min(drop / upper.length.max(lower.length));
            u = self.band(upper, lower, u, slope)?;
        }
        if rings.len() > 1
            && let Some(&last) = rings.last()
        {
            self.round(last, u)?;
        }
        Ok(())
    }

    /// Along `ring` from `u`, at its points, falling from `from` at `slope`
    /// for `length`: where along it that ends.
    fn along(
        &mut self,
        ring: &Ring,
        u: f64,
        from: f64,
        length: f64,
        slope: f64,
    ) -> Result<f64, StrategyError> {
        let length = length.min(f64::from(MAX_LAPS) * ring.length);
        for (gone, i) in ring.ahead(u, length) {
            if let Some(&p) = ring.points.get(i) {
                self.cut(p.extend(from - slope * gone))?;
            }
        }
        let end = u + length;
        self.cut(ring.at(end).extend(ring.z))?;
        self.at = Some(ring.at(end));
        Ok(end.rem_euclid(ring.length))
    }

    /// Once round `ring` at its level, from `u` back to it.
    fn round(&mut self, ring: &Ring, u: f64) -> Result<f64, StrategyError> {
        for (_, i) in ring.ahead(u, ring.length) {
            if let Some(&p) = ring.points.get(i) {
                self.cut(p.extend(ring.z))?;
            }
        }
        self.cut(ring.at(u).extend(ring.z))?;
        self.at = Some(ring.at(u));
        Ok(u)
    }

    /// Down from `upper`, where the tool is, at `u` along it, to `lower`,
    /// falling at `slope` along the way it goes: on the part at every height
    /// between, `lower` pulled onto it at the height the tool is at. Where
    /// along `lower` it ends.
    fn band(
        &mut self,
        upper: &Ring,
        lower: &Ring,
        u: f64,
        slope: f64,
    ) -> Result<f64, StrategyError> {
        let tolerance = self.params.tolerance;
        let (z_top, z_bottom) = (upper.z, lower.z);
        let need = (z_top - z_bottom) / slope;
        let mut cur = upper.at(u);
        let mut v = lower.nearest(cur);
        let mut gone = 0.0;
        // How far each point of the ring below may be from where the tool
        // touches the part, at most: as far as the ring above.
        let reach_by = self.reach_by;
        let caps: Vec<f64> = lower
            .points
            .par_iter()
            .map(|&p| upper.distance(p) + 2.0 * reach_by)
            .collect();
        let side = self.side;
        let apart = 0.5 * tolerance;
        for _ in 0..MAX_LAPS {
            self.params.watch.check()?;
            let ahead = lower.ahead(v, lower.length);
            if ahead.is_empty() {
                break;
            }
            // Pulled at the height the tool would be at going round the ring
            // below; then again at the height it is at going round the points
            // pulled.
            let pull = |heights: &[f64]| -> Vec<(DVec2, bool)> {
                ahead
                    .par_iter()
                    .zip(heights)
                    .map_init(
                        || self.puller(),
                        |on, (&(_, i), &z)| {
                            let Some(&p) = lower.points.get(i) else {
                                return (DVec2::ZERO, false);
                            };
                            let cap = caps.get(i).copied().unwrap_or(reach_by);
                            match lower
                                .inward(i, side, apart)
                                .and_then(|inward| onto_far(on, p, inward, z, cap))
                            {
                                Some(q) => (q, true),
                                None => (p, false),
                            }
                        },
                    )
                    .collect()
            };
            let heights = |points: &[(DVec2, bool)], from: DVec2| -> Vec<f64> {
                let mut s = gone;
                let mut last = from;
                points
                    .iter()
                    .map(|&(p, _)| {
                        s += last.distance(p);
                        last = p;
                        (z_top - slope * s).max(z_bottom)
                    })
                    .collect()
            };
            let guess: Vec<f64> = ahead
                .iter()
                .map(|&(g, _)| (z_top - slope * (gone + g)).max(z_bottom))
                .collect();
            // Pulled at the heights the way round the first pull gives: the
            // heights are put right on the lap as cut, below.
            let zs = heights(&pull(&guess), cur);
            let second = pull(&zs);
            for &(_, ok) in &second {
                if ok {
                    self.pulled += 1;
                } else {
                    self.unpulled += 1;
                }
            }
            // The lap as it is cut: between each two, kept on the part; then
            // each point at the height the way round it gives, along the
            // points added as well, and put on the part there.
            let mut lap: Vec<DVec3> = std::iter::once(cur.extend(z_top - slope * gone))
                .chain(second.iter().zip(&zs).map(|(&(p, _), &z)| p.extend(z)))
                .collect();
            lap = self.between_all(&lap);
            let along = |lap: &[DVec3]| -> Vec<f64> {
                let mut s = gone;
                let mut last = cur;
                lap.iter()
                    .map(|p| {
                        s += last.distance(p.truncate());
                        last = p.truncate();
                        s
                    })
                    .collect()
            };
            let gone_round = along(&lap);
            for (p, s) in lap.iter_mut().zip(gone_round) {
                p.z = (z_top - slope * s).max(z_bottom);
            }
            lap = self.onto_at_heights(&lap);
            // Moved, a chord may stray again: kept on the part as before,
            // the points added at its middle's height.
            lap = self.between_all(&lap);
            let along = along(&lap);
            // Up to where it reaches the ring below's level.
            let mut end: Option<DVec2> = None;
            let mut last = cur;
            let mut s = gone;
            for (k, &p) in lap.iter().enumerate().skip(1) {
                let reached = along.get(k).copied().unwrap_or(s);
                if reached >= need {
                    let before = along.get(k - 1).copied().unwrap_or(s);
                    let t = if reached > before {
                        (need - before) / (reached - before)
                    } else {
                        1.0
                    };
                    end = Some(last.lerp(p.truncate(), t.clamp(0.0, 1.0)));
                    break;
                }
                self.cut(p)?;
                s = reached;
                last = p.truncate();
            }
            if let Some(e) = end {
                // Onto the ring below, at its level, where it reaches it.
                let e = lower.nearest(e);
                let at = lower.at(e);
                self.cut(at.extend(z_bottom))?;
                self.at = Some(at);
                return Ok(e.rem_euclid(lower.length));
            }
            gone = s;
            cur = last;
            v = lower.nearest(cur);
        }
        // Far past any ramp asked for: down onto the ring where it is.
        let at = lower.at(v);
        self.cut(at.extend(z_bottom))?;
        self.at = Some(at);
        Ok(v)
    }

    /// `points`, but the first, each moved across the way it goes onto the
    /// part at its own height, from near it: looked for as far as
    /// [`REPULL_REACH`] either way. One not found there is left where it was.
    fn onto_at_heights(&self, points: &[DVec3]) -> Vec<DVec3> {
        let side = self.side;
        let n = points.len();
        (0..n)
            .into_par_iter()
            .map_init(
                || self.puller(),
                |on, i| {
                    let Some(&p) = points.get(i) else {
                        return DVec3::ZERO;
                    };
                    if i == 0 {
                        return p;
                    }
                    let prev = points.get(i - 1).copied().unwrap_or(p);
                    let next = points.get(i + 1).copied().unwrap_or(p);
                    (next - prev)
                        .truncate()
                        .try_normalize()
                        .map(|d| d.perp() * side)
                        .and_then(|inward| onto_either(on, p.truncate(), inward, p.z, REPULL_REACH))
                        .map_or(p, |q| q.extend(p.z))
                },
            )
            .collect()
    }

    /// `points`, each on the part at its height, with points added between
    /// where a chord would stray more than a quarter of the tolerance into the
    /// part or off it.
    fn between_all(&self, points: &[DVec3]) -> Vec<DVec3> {
        let side = self.side;
        let added: Vec<Vec<DVec3>> = points
            .par_windows(2)
            .map_init(
                || self.puller(),
                |on, pair| {
                    let mut out = Vec::new();
                    if let [a, b] = *pair {
                        between(on, a, b, side, 0, &mut out);
                    }
                    out
                },
            )
            .collect();
        let mut out = Vec::with_capacity(points.len());
        for (i, &p) in points.iter().enumerate() {
            out.push(p);
            if let Some(more) = added.get(i) {
                out.extend(more);
            }
        }
        out
    }
}

/// `p` moved along `inward`, towards the part, onto it at height `z`, looking
/// as far as `cap`: `None` if it is not within that.
fn onto_far<T: DropTest>(
    on: &mut Puller<'_, '_, T>,
    p: DVec2,
    inward: DVec2,
    z: f64,
    cap: f64,
) -> Option<DVec2> {
    on.z = z;
    if on.touches(p) {
        return on.onto(p, inward);
    }
    let close = PULL_TO * on.tolerance;
    let mut clear = 0.0;
    let mut into = on.reach_by.max(close).min(cap.max(close));
    loop {
        if on.touches(p + inward * into) {
            break;
        }
        if into >= cap {
            return None;
        }
        clear = into;
        into = (2.0 * into).min(cap);
    }
    while into - clear > close {
        let mid = 0.5 * (clear + into);
        if on.touches(p + inward * mid) {
            into = mid;
        } else {
            clear = mid;
        }
    }
    Some(p + inward * clear)
}

/// `p` moved along `inward` onto the part at height `z`, or back out of it,
/// looking as far as `cap` either way: `None` if it is not within that.
fn onto_either<T: DropTest>(
    on: &mut Puller<'_, '_, T>,
    p: DVec2,
    inward: DVec2,
    z: f64,
    cap: f64,
) -> Option<DVec2> {
    on.z = z;
    if !on.touches(p) {
        return onto_far(on, p, inward, z, cap);
    }
    // In the part: out along `-inward` until clear, then back in.
    let close = PULL_TO * on.tolerance;
    let mut into = 0.0;
    let mut clear = on.reach_by.max(close).min(cap.max(close));
    loop {
        if !on.touches(p - inward * clear) {
            break;
        }
        if clear >= cap {
            return None;
        }
        into = clear;
        clear = (2.0 * clear).min(cap);
    }
    while clear - into > close {
        let mid = 0.5 * (clear + into);
        if on.touches(p - inward * mid) {
            into = mid;
        } else {
            clear = mid;
        }
    }
    Some(p - inward * clear)
}

/// Points on the part between `a` and `b`, both on it, so that no chord
/// strays more than a quarter of the tolerance into the part or off it: each
/// chord's middle pulled onto the part at its height, and kept where it is
/// further, the halves then looked at the same way.
fn between<T: DropTest>(
    on: &mut Puller<'_, '_, T>,
    a: DVec3,
    b: DVec3,
    side: f64,
    depth: u32,
    out: &mut Vec<DVec3>,
) {
    let (a2, b2) = (a.truncate(), b.truncate());
    let Some(inward) = (b2 - a2).try_normalize().map(|d| d.perp() * side) else {
        return;
    };
    if depth > MAX_SPLITS || a2.distance(b2) < 4.0 * PULL_TO * on.tolerance {
        return;
    }
    let mid = a.lerp(b, 0.5);
    // Its ends are on the part at different heights, so its middle may be
    // further off it than a level ring's chords are: looked for as far as
    // half the chord, either way.
    let Some(p) = onto_either(on, mid.truncate(), inward, mid.z, 0.5 * a2.distance(b2)) else {
        return;
    };
    if distance_to_segment(p, a2, b2) > 0.25 * on.tolerance {
        let p = p.extend(mid.z);
        between(on, a, p, side, depth + 1, out);
        out.push(p);
        between(on, p, b, side, depth + 1, out);
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
mod tests;
