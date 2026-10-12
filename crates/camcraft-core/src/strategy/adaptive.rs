//! Adaptive clearing: roughing in Z levels with the cutter's load held constant.
//!
//! Z-level roughing ([`crate::strategy::pocket_clearing`]) offsets the reachable region inward by
//! the stepover, so the first ring into full material is a full slot, and every
//! inside corner buries the tool. Adaptive clearing, after Fusion's (Freesteel's
//! Adaptive Clearing, 2004, and Stori and Wright's constant-engagement paths,
//! 2000), steers the tool instead: step by step, it turns toward or away from the
//! material until the arc of its edge in material, the engagement angle, is the
//! one the **optimal load** gives. So it can take the whole flute length down at
//! once, at a light radial load, and never slot.
//!
//! # A level at a time, in two regions
//!
//! At each level there are two regions, and every step is judged by both:
//!
//! - **Allowed**: where the tool's centre may be, the complement of
//!   [`LevelReach::forbidden`], from the model. A step that would leave it turns
//!   along its edge instead, which is how walls are left exactly the stock to
//!   leave.
//! - **Uncut**: the material still standing at the level: the stock's block, less
//!   everything the tool has swept at this level. Engagement is measured against
//!   it, exactly ([`camcraft_geom::engagement`]).
//!
//! # Paths from the model, material from exact regions
//!
//! Grids are only for stock. The path is steered by the uncut region, so that
//! region is exact polygons: the stock's box, less the exact sweep of the path
//! itself. Nothing here reads the simulated stock. So an operation run after
//! another cuts air where that one cleared: rest material from earlier operations
//! is still to come.
//!
//! # Getting in, and moving on
//!
//! Where the tool can stand clear of material, beside the block or in what it
//! has cleared, a pass starts at the edge of the material, with the material on
//! its climbing side. Where it cannot, as in a closed pocket, it goes down a helix
//! in the widest place, and spirals out. A pass ends when it runs out of material,
//! or would overload the tool; the next starts at the nearest edge of what is
//! left, reached without lifting where the way is clear.
//!
//! # Levels, and flat areas
//!
//! Levels are a stepdown apart, from the top. Each is cleared on its own: the
//! slab above it was untouched when it is cut. Each flat face of the part adds a
//! level on it (raised by the stock to leave down), cut after the level below it,
//! so it only removes the terrace that level left over the face.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use camcraft_geom::arc::Arc;
use camcraft_geom::contour::{Contour, Region};
use camcraft_geom::engagement::engaged;
use camcraft_geom::mesh::TriMesh;
use glam::{DVec2, dvec3};
use rayon::prelude::*;

use crate::cutter::tool::{Cutter, ToolProfile};
use crate::path::toolpath::{MoveRole, Toolpath};
use crate::strategy::reach::LevelReach;
use crate::strategy::{MATERIAL_THICKNESS, StrategyError, check, check_levels, innermost, slabs};

mod clearing;
mod steer;
mod tiles;

use clearing::Clearing;
use tiles::Tiles;

/// The most a pass turns in one step, toward or away from the material, in
/// radians.
const MAX_TURN: f64 = FRAC_PI_4;

/// How many times a step's heading is refined toward the load asked for.
const SEARCH_ITERATIONS: usize = 14;

/// A level gives up after this many passes: bounded by construction, so a
/// pathological region cannot run away.
const MAX_PASSES: usize = 50_000;

/// A level gives up after this many steps, in all its passes together.
const MAX_STEPS: usize = 20_000_000;

/// A level gives up after this many passes in a row that clear nothing.
const MAX_FUTILE: usize = 64;

/// Which way the passes cut.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Direction {
    /// Every pass climbing. Fusion's default.
    #[default]
    Climb,
    /// Every pass conventional.
    Conventional,
    /// Climbing, or conventional where that saves going back: at the end of a
    /// pass, the tool turns round and cuts back rather than linking.
    BothWays,
}

/// How to clear the material.
#[derive(Debug, Clone, PartialEq)]
pub struct AdaptiveParams {
    /// Deepest each level may be, in millimetres.
    pub stepdown: f64,
    /// The radial width of cut the tool is held to, in millimetres: Fusion's
    /// optimal load.
    pub optimal_load: f64,
    /// Cutting feed, in mm/min.
    pub feed: f64,
    /// Highest level to cut at. Usually the top of the stock.
    pub top: f64,
    /// Lowest level to cut at. Usually the bottom of the part.
    pub bottom: f64,
    /// Chordal tolerance, in millimetres.
    pub tolerance: f64,
    /// Whether climbing puts the material on the tool's right, as it does with
    /// a clockwise spindle.
    pub climb_right: bool,
    /// Which way the passes cut.
    pub direction: Direction,
    /// The radius of the helix a closed area is entered down, as the tool's
    /// centre goes, in millimetres; zero to plunge.
    pub helix_radius: f64,
    /// The longest link made without lifting, in millimetres.
    pub stay_down: f64,
    /// How steeply a helix goes down, in radians from level.
    pub ramp_angle: f64,
    /// The feed down a helix, in mm/min.
    pub ramp_feed: f64,
    /// The most a helix goes down in one turn, in millimetres; `None` for no
    /// limit but the angle.
    pub ramp_stepdown: Option<f64>,
    /// How far above the material a helix starts, in millimetres.
    pub safe_distance: f64,
    /// Extra levels, each on a flat face of the part, raised by the stock to
    /// leave down: cut after the level below each, over what it left.
    pub flats: Vec<f64>,
}

impl Default for AdaptiveParams {
    fn default() -> Self {
        Self {
            stepdown: 6.0,
            optimal_load: 0.6,
            feed: 1500.0,
            top: 0.0,
            bottom: -10.0,
            tolerance: 0.01,
            climb_right: true,
            direction: Direction::Climb,
            helix_radius: 2.7,
            stay_down: 30.0,
            ramp_angle: 2f64.to_radians(),
            ramp_feed: 900.0,
            ramp_stepdown: None,
            safe_distance: 2.0,
            flats: Vec::new(),
        }
    }
}

/// What adaptive clearing did.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AdaptiveReport {
    /// Levels that produced cutting, flat ones included.
    pub levels: u64,
    /// Of them, levels on flat faces.
    pub flat_levels: u64,
    /// Passes emitted.
    pub passes: u64,
    /// Links between passes made without lifting.
    pub stayed_down: u64,
    /// Closed areas entered down a helix, or a plunge.
    pub entries: u64,
    /// Of them, entered down a helix.
    pub helixes: u64,
    /// Areas with no room to go down into, left uncut.
    pub no_room: u64,
    /// Material the tool could reach but did not clear, in square millimetres,
    /// summed over the levels.
    pub left_mm2: f64,
    /// Passes that ended because the next step would overload the tool.
    pub overloads: u64,
}

/// The heights of the part's flat faces that face up: where Fusion's flat area
/// detection adds a level.
pub fn flat_heights(mesh: &TriMesh, tolerance: f64) -> Vec<f64> {
    // Within half a degree of level.
    let level = (0.5f64).to_radians().cos();
    let mut zs: Vec<f64> = (0..mesh.len())
        .filter_map(|i| mesh.triangle(i))
        .filter_map(|[a, b, c]| {
            let n = (b - a).cross(c - a).try_normalize()?;
            (n.z >= level).then_some((a.z + b.z + c.z) / 3.0)
        })
        .collect();
    zs.sort_by(f64::total_cmp);
    let mut out: Vec<f64> = Vec::new();
    for z in zs {
        match out.last_mut() {
            // One face, tessellated at a rounding error's difference in height,
            // is one level: the highest of it, so the floor is never cut into.
            Some(last) if z - *last <= tolerance * 0.1 => *last = z,
            _ => out.push(z),
        }
    }
    out
}

/// Clear material in Z levels, holding the tool's engagement constant.
///
/// `reach` gives where the tool may not be, from the model. `area` is where its
/// centre may go: past the stock's sides, so passes can start outside it, and
/// within the operation's boundary. `stocks` are the blocks of material, each
/// its footprint's corners and its top. `cutter` is the real tool, whose width
/// at each height says how much of a terrace a level below cleared.
pub fn adaptive_rough(
    reach: &mut LevelReach<'_>,
    area: &Region,
    stocks: &[(DVec2, DVec2, f64)],
    cutter: &Cutter,
    params: &AdaptiveParams,
) -> Result<(Toolpath, AdaptiveReport), StrategyError> {
    check("stepdown", params.stepdown)?;
    check("optimal load", params.optimal_load)?;
    check("feed", params.feed)?;
    check("tolerance", params.tolerance)?;
    check_levels(params.top, params.bottom)?;
    if !(params.helix_radius.is_finite() && params.helix_radius >= 0.0) {
        return Err(StrategyError::BadParameter {
            name: "helix radius",
            value: params.helix_radius,
        });
    }
    let radius = reach.cutter_radius();
    check("tool radius", radius)?;

    // Levels from the top down, a stepdown apart, each flat level in the slab of
    // the one just below it, cut after it from the bottom up.
    let slabs = slabs(params.top, params.bottom, params.stepdown, &params.flats);

    // Where the tool may not be at every level, from the top: the shadows are
    // cheapest asked for in descending order. What that leaves of the area,
    // each level's own, in parallel.
    let mut all: Vec<f64> = slabs
        .iter()
        .flat_map(|(z, flats)| std::iter::once(*z).chain(flats.iter().copied()))
        .collect();
    all.sort_by(|a, b| b.total_cmp(a));
    let mut forbidden = Vec::with_capacity(all.len());
    for &z in &all {
        forbidden.push((z, reach.forbidden(z)?));
    }
    let allowed: Vec<(f64, Region)> = forbidden
        .into_par_iter()
        .map(|(z, forbidden)| Ok((z, area.difference(&forbidden)?)))
        .collect::<Result<_, StrategyError>>()?;
    let allowed_at = |z: f64| {
        allowed
            .iter()
            .find(|(at, _)| (at - z).abs() < 1e-9)
            .map(|(_, r)| r)
    };

    let level = LevelParams::new(params, radius);
    // Slabs depend on nothing but their own regions, so they are worked in
    // parallel and emitted in order: the result is the same on any number of
    // cores.
    let worked: Vec<Result<Vec<LevelCut>, StrategyError>> = slabs
        .par_iter()
        .map(|(z, flats)| {
            let mut cuts = Vec::with_capacity(1 + flats.len());
            let Some(region) = allowed_at(*z) else {
                return Ok(cuts);
            };
            cuts.push(clear_level(*z, region, &material(stocks, *z)?, &level)?);
            // Each flat level over what the levels of the slab below it left.
            for &f in flats {
                let Some(region) = allowed_at(f) else {
                    continue;
                };
                let cut = clear_flat(f, region, stocks, &cuts, cutter, &level)?;
                cuts.push(cut);
            }
            Ok(cuts)
        })
        .collect();

    let mut path = Toolpath::new();
    let mut report = AdaptiveReport::default();
    // The material stands up to the level above, or the top.
    let mut above = params.top;
    for (slab, (z, _)) in worked.into_iter().zip(&slabs) {
        // A slab's first level is its own; the rest are on flats.
        for (k, cut) in slab?.into_iter().enumerate() {
            report.left_mm2 += cut.left;
            report.no_room += cut.no_room;
            report.entries += cut.helixes.len() as u64;
            report.overloads += cut.overloads;
            if cut.passes.is_empty() {
                continue;
            }
            report.levels += 1;
            report.flat_levels += u64::from(k > 0);
            for (arrive, points) in &cut.passes {
                let Some(first) = points.first() else {
                    continue;
                };
                let first = dvec3(first.x, first.y, cut.z);
                match *arrive {
                    Arrive::Link => path.feed_to(first, MoveRole::Link, params.feed)?,
                    Arrive::Stay => {
                        report.stayed_down += 1;
                        path.feed_to(first, MoveRole::Transition, params.feed)?;
                    }
                    Arrive::Enter { centre } => {
                        match helix(
                            centre,
                            first,
                            above + params.safe_distance,
                            params,
                            level.side == Side::Right,
                        ) {
                            Some((start, turns)) => {
                                report.helixes += 1;
                                path.feed_to(start, MoveRole::Link, params.feed)?;
                                for (to, arc) in turns {
                                    path.arc_to(to, arc, MoveRole::Ramp, params.ramp_feed)?;
                                }
                            }
                            // No helix: the linker goes straight down.
                            None => path.feed_to(first, MoveRole::Link, params.feed)?,
                        }
                    }
                }
                for p in points.iter().skip(1) {
                    path.feed_to(dvec3(p.x, p.y, cut.z), MoveRole::Cut, params.feed)?;
                }
                report.passes += 1;
            }
        }
        above = *z;
    }
    Ok((path, report))
}

/// Clear flat level `f` of a slab over what the levels of the slab `below` it
/// left: the real tool, `cutter`, as wide at each as it is that far above its
/// tip.
fn clear_flat(
    f: f64,
    allowed: &Region,
    stocks: &[(DVec2, DVec2, f64)],
    below: &[LevelCut],
    cutter: &Cutter,
    p: &LevelParams,
) -> Result<LevelCut, StrategyError> {
    let mut uncut = Tiles::new(&material(stocks, f)?, 16.0 * p.radius)?;
    for cut in below {
        // The real tool's width that far above its tip.
        let wide = cutter.radius_at(f - cut.z);
        if wide <= 0.0 {
            continue;
        }
        // A little at a time, as the level itself was cleared: the whole sweep
        // at once is a very large polygon.
        cut.swept(|line| {
            for chunk in line.chunks(48).enumerate().map(|(k, c)| {
                // Each chunk from the last point of the one before.
                let from = (k * 48).saturating_sub(1);
                line.get(from..(k * 48 + c.len()).min(line.len()))
                    .unwrap_or(c)
            }) {
                uncut.subtract(&Region::around_polylines(&[chunk.to_vec()], wide, p.arc))?;
            }
            Ok(())
        })?;
        for &(c, r) in &cut.helixes {
            if let Some(d) = disc(c, r + wide, p.arc) {
                uncut.subtract(&Region::union_of(&[d])?)?;
            }
        }
    }
    clear_level(f, allowed, &uncut.whole()?, p)
}

/// A helix down onto `end` about `centre`, from `top`, as adaptive clearing
/// goes down into material: no steeper than the ramp angle, nor than the ramp's
/// most in one turn, counter-clockwise when `ccw`, then once round level at the
/// bottom, as Fusion's ends. Where it starts, and its turns. `None` for a helix
/// with no radius, or nowhere to go down.
fn helix(
    centre: DVec2,
    end: glam::DVec3,
    top: f64,
    p: &AdaptiveParams,
    ccw: bool,
) -> Option<(glam::DVec3, Vec<(glam::DVec3, Arc)>)> {
    let radius = end.truncate().distance(centre);
    if radius <= 1e-6 || top <= end.z + 1e-9 || !(p.ramp_angle > 0.0 && p.ramp_angle < FRAC_PI_2) {
        return None;
    }
    let lap = TAU * radius;
    let angle = p
        .ramp_stepdown
        .map_or(p.ramp_angle, |d| p.ramp_angle.min((d / lap).atan()));
    let turn = if ccw { 1.0 } else { -1.0 };
    let total = (top - end.z) / (radius * angle.tan());
    let back = end.truncate() - centre;
    let start = (centre + DVec2::from_angle(-turn * total).rotate(back)).extend(top);
    let n = camcraft_geom::steps::bounded_count((total / TAU - 1e-9).ceil().max(1.0));
    let axis = centre.extend(end.z);
    let mut turns: Vec<(glam::DVec3, Arc)> = (1..=n)
        .map(|k| {
            let f = f64::from(k) / f64::from(n);
            let p = if k == n {
                end
            } else {
                (centre + DVec2::from_angle(turn * total * f).rotate(start.truncate() - centre))
                    .extend(top + (end.z - top) * f)
            };
            (
                p,
                Arc {
                    centre: axis,
                    axis: glam::DVec3::Z,
                    sweep: turn * total / f64::from(n),
                },
            )
        })
        .collect();
    turns.push((
        end,
        Arc {
            centre: axis,
            axis: glam::DVec3::Z,
            sweep: turn * TAU,
        },
    ));
    Some((start, turns))
}

/// The material at level `z`: every block whose top stands above it.
fn material(stocks: &[(DVec2, DVec2, f64)], z: f64) -> Result<Region, StrategyError> {
    let rings: Vec<Contour> = stocks
        .iter()
        .filter(|(_, _, top)| *top > z + MATERIAL_THICKNESS)
        .filter_map(|&(lo, hi, _)| Region::rectangle(lo, hi).rings().first().cloned())
        .collect();
    Ok(Region::union_of(&rings)?)
}

/// A disc as a polygon whose corners lie on its circle: within it, so a disc
/// taken as cleared never claims more than was.
fn disc(centre: DVec2, radius: f64, tolerance: f64) -> Option<Contour> {
    if !(radius > 0.0 && radius.is_finite()) {
        return None;
    }
    // Chords that fall short of the circle by at most the tolerance.
    let half = (1.0 - tolerance.min(radius) / radius)
        .clamp(-1.0, 1.0)
        .acos();
    let n = (PI / half.max(1e-3)).ceil().clamp(8.0, 4096.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = n as u32;
    Contour::new(
        (0..n)
            .map(|k| centre + DVec2::from_angle(TAU * f64::from(k) / f64::from(n)) * radius)
            .collect(),
    )
}

/// What a level's clearing is held to, worked out once.
#[derive(Debug, Clone)]
struct LevelParams {
    /// The real tool's radius: what it cuts.
    radius: f64,
    /// The engagement angle asked for.
    target: f64,
    /// Close enough to it.
    within: f64,
    /// The most a pass may take on before it ends.
    limit: f64,
    /// Less than this is cutting air.
    air: f64,
    /// Longest and shortest step.
    longest: f64,
    shortest: f64,
    /// Material narrower than twice this is not worth a pass.
    sliver: f64,
    /// Arc tolerance for the regions derived from the model.
    arc: f64,
    /// Arc tolerance for what the tool has swept: coarser, as the material is
    /// only ever measured against it, and a finer one makes every region after
    /// it slower to work with.
    swept_arc: f64,
    /// Arc tolerance for finding where passes start, which only needs to be
    /// near: the steering is exact.
    coarse: f64,
    /// Which side of the tool the material is on, cutting the main way.
    side: Side,
    both_ways: bool,
    helix_radius: f64,
    stay_down: f64,
}

impl LevelParams {
    fn new(p: &AdaptiveParams, radius: f64) -> Self {
        let angle = |width: f64| (1.0 - (width / radius).min(2.0)).clamp(-1.0, 1.0).acos();
        let target = angle(p.optimal_load);
        let right = match p.direction {
            Direction::Climb | Direction::BothWays => p.climb_right,
            Direction::Conventional => !p.climb_right,
        };
        let side = if right { Side::Right } else { Side::Left };
        Self {
            radius,
            target,
            within: (target * 0.03).max(2e-3),
            limit: angle(2.0 * p.optimal_load).max(target * 1.25),
            air: (target * 0.02).max(1e-3),
            longest: (radius * 0.2).max(p.tolerance),
            shortest: (radius * 0.02).max(p.tolerance * 0.2).min(radius * 0.2),
            sliver: p.tolerance,
            arc: p.tolerance * 0.1,
            swept_arc: p.tolerance * 0.5,
            coarse: (p.tolerance * 0.5).max(radius * 0.005),
            side,
            both_ways: p.direction == Direction::BothWays,
            helix_radius: p.helix_radius,
            stay_down: p.stay_down,
        }
    }
}

/// How a pass is arrived at.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Arrive {
    /// However the linker sees fit: lifting clear, and going down, by a helix
    /// where it starts in material.
    Link,
    /// Straight across at the level, through what is cleared.
    Stay,
    /// Down into material with no way in from the side: a helix about
    /// `centre`, as the tool's centre goes, ending where the pass starts;
    /// straight down for one with no radius.
    Enter {
        /// The helix's axis.
        centre: DVec2,
    },
}

/// One level's passes.
#[derive(Debug, Clone)]
struct LevelCut {
    z: f64,
    passes: Vec<(Arrive, Vec<DVec2>)>,
    /// Where it went down into closed areas, and the radius of each helix.
    helixes: Vec<(DVec2, f64)>,
    left: f64,
    no_room: u64,
    overloads: u64,
}

impl LevelCut {
    /// Each line the tool's centre ran along at this level, in order, to `each`:
    /// its passes, and the links between them made at the level.
    fn swept(
        &self,
        mut each: impl FnMut(&[DVec2]) -> Result<(), StrategyError>,
    ) -> Result<(), StrategyError> {
        let mut at: Option<DVec2> = None;
        for (arrive, points) in &self.passes {
            if let (Arrive::Stay, Some(a), Some(&b)) = (arrive, at, points.first()) {
                each(&[a, b])?;
            }
            each(points)?;
            at = points.last().copied();
        }
        Ok(())
    }
}

/// Which side of the tool the material is kept on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Right,
    Left,
}

impl Side {
    /// Which way turning toward the material rotates, counter-clockwise positive.
    fn toward(self) -> f64 {
        match self {
            Side::Right => -1.0,
            Side::Left => 1.0,
        }
    }

    /// The other side.
    fn opposite(self) -> Self {
        match self {
            Side::Right => Side::Left,
            Side::Left => Side::Right,
        }
    }

    /// Square to `d`, away from the material: a quarter turn the other way
    /// from it.
    fn away(self, d: DVec2) -> DVec2 {
        match self {
            Side::Right => d.perp(),
            Side::Left => -d.perp(),
        }
    }
}

/// Clear one level.
fn clear_level(
    z: f64,
    allowed: &Region,
    uncut: &Region,
    p: &LevelParams,
) -> Result<LevelCut, StrategyError> {
    let mut c = Clearing::new(allowed, uncut, p)?;
    c.run()?;
    Ok(LevelCut {
        z,
        passes: c.passes,
        helixes: c.helixes,
        left: c.left,
        no_room: c.no_room,
        overloads: c.overloads,
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
mod tests;
