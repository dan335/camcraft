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
//! itself. Nothing here reads the simulated stock. With rest machining, the
//! operations before it are swept exactly too, level by level, each with the
//! tool it was cut with ([`rest`]); without, an operation run after another
//! cuts air where that one cleared.
//!
//! # Getting in, and moving on
//!
//! Where the tool can stand clear of material, beside the block or in what it
//! has cleared, a pass starts at the edge of the material, with the material on
//! its climbing side. Where it cannot, as in a closed pocket, it goes down a helix
//! in the widest place, and spirals out, unless it is not to machine cavities;
//! with slot clearing, it first cuts a slot along the pocket's longest way. A
//! pass ends when it runs out of material, or would overload the tool; the next
//! starts at the nearest edge of what is left, reached without lifting where the
//! way is clear, straight or round what stands between through what is cleared,
//! as far round as the stay-down level allows, kept the clearance off the
//! material, and lifted off the level if asked.
//!
//! # Corners, and order
//!
//! A minimum cutting radius rounds the inside corners of where the tool may be,
//! so it never turns tighter there, and leaves the material in them. Levels are
//! cut by depth, each all round before the next down, or by area: each area
//! a level splits into cleared all the way down before the next.
//!
//! # Levels, and flat areas
//!
//! Levels are a stepdown apart, from the top. Each is cleared on its own: the
//! slab above it was untouched when it is cut. Each flat face and peak of the
//! part adds a level on it (raised by the stock to leave down), cut after the
//! level below it, so it only removes the terrace that level left over the face.
//! A fine stepdown adds levels up each slab, and shallow areas add them where
//! the terraces are wide, cutting only there; those that would cut
//! too little depth are left out.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use camcraft_geom::arc::Arc;
use camcraft_geom::contour::{Contour, Indexed, Region};
use camcraft_geom::engagement::engaged;
use camcraft_geom::mesh::TriMesh;
use glam::{DVec2, dvec3};
use rayon::prelude::*;

use crate::control::{Watch, share};
use crate::cutter::tool::{Cutter, ToolProfile};
use crate::path::toolpath::{MoveRole, Toolpath};
use crate::strategy::reach::LevelReach;
use crate::strategy::{
    MATERIAL_THICKNESS, SAME_LEVEL, StrategyError, check, check_levels, merged_highest, slabs,
    sort_levels, with_fine_levels,
};

mod clearing;
mod levels;
pub mod rest;
mod steer;
mod tiles;

use clearing::Clearing;
pub use levels::{Shallow, peak_heights};
use rest::Earlier;
use tiles::{SWEPT_AT_ONCE, TILE_RADII, Tiles};

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
    /// How far round what stands between a link stays down, from none, 0,
    /// straight only, to 1, as far as `stay_down` allows: Fusion's Stay-Down
    /// Level. A way round may be longer than straight by this much of what
    /// `stay_down` leaves.
    pub stay_down_level: f64,
    /// How far the tool stays off the material along a link it stays down
    /// for, in millimetres, but where it leaves a pass and comes to the next:
    /// Fusion's Minimum Stay-Down Clearance.
    pub stay_down_clearance: f64,
    /// How far the tool rises off the level for a link it stays down for, in
    /// millimetres: Fusion's Lift Height. No higher than the level above.
    pub lift_height: f64,
    /// Fusion's Slot Clearing: a closed area gone down into is first cut a
    /// slot this wide along its middle, in millimetres, no narrower than the
    /// tool; none when `None`.
    pub slot_width: Option<f64>,
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
    /// Fusion's fine stepdown, in millimetres: levels this far apart up from
    /// each level and flat, cut as flats are ([`with_fine_levels`]); none
    /// when `None`.
    pub fine_stepdown: Option<f64>,
    /// Flat levels, `flats`, less than this below the level above their own,
    /// in millimetres, are left out: Fusion's flat area Minimum Stepdown.
    pub min_flat_stepdown: f64,
    /// Fine and shallow levels less than this below the level above their
    /// own, in millimetres, are left out: Fusion's Minimum Axial Engagement.
    /// Every level between two a stepdown apart cuts into material standing
    /// up to the upper one.
    pub min_axial_engagement: f64,
    /// Fusion's Machine Shallow Areas: levels added between two where the
    /// terrace between them is too wide. None when `None`.
    pub shallow: Option<Shallow>,
    /// Whether to go down into material there is no way into from the side:
    /// Fusion's Machine Cavities. Off, such areas are left.
    pub machine_cavities: bool,
    /// The tightest the tool's centre may turn in an inside corner, in
    /// millimetres; none when zero.
    pub min_cutting_radius: f64,
    /// Each area cleared all the way down before the next, rather than level
    /// by level.
    pub by_area: bool,
    /// The operations before it, for rest machining: what they swept is not
    /// material. None for the stock as it starts.
    pub before: Vec<Earlier>,
    /// With rest machining, material they left thinner than this, in
    /// millimetres, is let be.
    pub ignore_less_than: f64,
    /// What it tells how far it has got, and asks whether to stop.
    pub watch: Watch,
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
            stay_down_level: 1.0,
            stay_down_clearance: 0.0,
            lift_height: 0.0,
            slot_width: None,
            ramp_angle: 2f64.to_radians(),
            ramp_feed: 900.0,
            ramp_stepdown: None,
            safe_distance: 2.0,
            flats: Vec::new(),
            fine_stepdown: None,
            min_flat_stepdown: 0.0,
            min_axial_engagement: 0.0,
            shallow: None,
            machine_cavities: true,
            min_cutting_radius: 0.0,
            by_area: false,
            before: Vec::new(),
            ignore_less_than: 0.0,
            watch: Watch::default(),
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
    /// Of them, levels of the fine stepdown.
    pub fine_levels: u64,
    /// Of them, levels added in shallow areas.
    pub shallow_levels: u64,
    /// Passes emitted.
    pub passes: u64,
    /// Links between passes made without lifting.
    pub stayed_down: u64,
    /// Closed areas entered down a helix, or a plunge.
    pub entries: u64,
    /// Of them, entered down a helix.
    pub helixes: u64,
    /// Of them, first cut a slot along their middle.
    pub slots: u64,
    /// Areas with no room to go down into, left uncut.
    pub no_room: u64,
    /// Levels where closed areas were left, as it was not to machine
    /// cavities.
    pub cavities: u64,
    /// Material left in them, in square millimetres, summed over the levels.
    pub cavity_mm2: f64,
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
    let zs: Vec<f64> = (0..mesh.len())
        .filter_map(|i| mesh.triangle(i))
        .filter_map(|[a, b, c]| {
            let n = (b - a).cross(c - a).try_normalize()?;
            (n.z >= level).then_some((a.z + b.z + c.z) / 3.0)
        })
        .collect();
    // One face, tessellated at a rounding error's difference in height, is
    // one level: the highest of it, so the floor is never cut into.
    merged_highest(zs, tolerance * 0.1)
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
    // the one just below it, cut after it from the bottom up: those too near
    // the level above to be worth one left out.
    let mut slabs = slabs(params.top, params.bottom, params.stepdown, &params.flats);
    levels::without_thin(&mut slabs, params.top, params.min_flat_stepdown, |_| true);
    let mut slabs = match params.fine_stepdown {
        Some(fine) => with_fine_levels(slabs, params.top, fine),
        None => slabs,
    };
    let is_flat = |z: f64| params.flats.iter().any(|f| (f - z).abs() < SAME_LEVEL);
    let levels_of = |slabs: &[(f64, Vec<f64>)]| -> Vec<f64> {
        let mut all: Vec<f64> = slabs
            .iter()
            .flat_map(|(z, flats)| std::iter::once(*z).chain(flats.iter().copied()))
            .collect();
        all.sort_by(|a, b| b.total_cmp(a));
        all
    };

    // Where the tool may not be at every level, all at once: each height
    // asked once, from the top, and the levels put together in parallel.
    let zs = levels_of(&slabs);
    let mut forbidden: Vec<(f64, Region)> =
        zs.iter().copied().zip(reach.forbidden_all(&zs)?).collect();
    // Levels added where the terraces are wide, each to cut only there.
    let arc = params.tolerance * 0.1;
    let mut zones: Vec<(f64, Region)> = Vec::new();
    if let Some(shallow) = params.shallow {
        let added =
            levels::shallow_levels(&slabs, params.top, shallow, reach, &forbidden, stocks, arc)?;
        for a in added {
            if let Some(slab) = slabs.iter_mut().find(|(z, _)| *z < a.z) {
                slab.1.push(a.z);
            }
            forbidden.push((a.z, a.forbidden));
            zones.push((a.z, a.zone));
        }
        for (_, fills) in &mut slabs {
            sort_levels(fills, false);
        }
    }
    let zone_of = |z: f64| {
        zones
            .iter()
            .find(|(at, _)| (at - z).abs() < SAME_LEVEL)
            .map(|(_, r)| r)
    };
    // Levels between that would cut too little depth left out.
    levels::without_thin(&mut slabs, params.top, params.min_axial_engagement, |f| {
        !is_flat(f)
    });
    let all = levels_of(&slabs);
    forbidden.retain(|(z, _)| all.iter().any(|a| (a - z).abs() < SAME_LEVEL));

    // What that leaves of the area, each level's own, in parallel: a shallow
    // level's only where its terraces are wide, and as far again as the
    // tool's radius to come at them from. Inside corners rounded to the
    // minimum cutting radius: an opening, kept within where the tool may be,
    // so it never turns tighter, and nothing narrower than the tool and twice
    // the radius is cut.
    let blend = params.min_cutting_radius;
    let allowed: Vec<(f64, Region)> = forbidden
        .into_par_iter()
        .map(|(z, forbidden)| {
            let mut room = area.difference(&forbidden)?;
            if let Some(zone) = zone_of(z) {
                room = room.intersection(&zone.offset(radius + params.tolerance, arc))?;
            }
            Ok((z, blended(&room, blend, arc)?))
        })
        .collect::<Result<_, StrategyError>>()?;
    let allowed_at = |z: f64| {
        allowed
            .iter()
            .find(|(at, _)| (at - z).abs() < 1e-9)
            .map(|(_, r)| r)
    };
    // The material at each level: the blocks, less, with rest machining, what
    // the operations before swept there.
    let rested = if params.before.is_empty() {
        Vec::new()
    } else {
        rest::material_at(
            stocks,
            &all,
            &params.before,
            params.ignore_less_than,
            params.tolerance,
            &params.watch,
        )?
    };
    let standing = |z: f64| -> Result<Region, StrategyError> {
        match rested.iter().find(|(at, _)| (at - z).abs() < 1e-9) {
            Some((_, r)) => Ok(r.clone()),
            None => material(stocks, z),
        }
    };

    let level = LevelParams::new(params, radius);
    // Each level an equal share of the work, told as it is done.
    let share = share(1, all.len());
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
            cuts.push(clear_level(*z, region, &standing(*z)?, &level)?);
            show_level(&params.watch, cuts.last());
            params.watch.add(share);
            // Each flat level over what the levels of the slab below it left.
            for &f in flats {
                let Some(region) = allowed_at(f) else {
                    continue;
                };
                let cut = clear_flat(f, region, &standing(f)?, &cuts, cutter, &level)?;
                show_level(&params.watch, Some(&cut));
                cuts.push(cut);
                params.watch.add(share);
            }
            Ok(cuts)
        })
        .collect();
    let worked: Vec<Vec<LevelCut>> = worked.into_iter().collect::<Result<_, _>>()?;

    let mut report = AdaptiveReport::default();
    for (k, cut) in worked.iter().flat_map(|slab| slab.iter().enumerate()) {
        if cut.cavities {
            report.cavities += 1;
            report.cavity_mm2 += cut.left;
        } else {
            report.left_mm2 += cut.left;
        }
        report.no_room += cut.no_room;
        report.entries += cut.helixes.len() as u64;
        report.slots += cut.slots;
        report.overloads += cut.overloads;
        if cut.passes.is_empty() {
            continue;
        }
        report.levels += 1;
        // A slab's first level is its own; the rest are on flats.
        if k > 0 {
            if is_flat(cut.z) {
                report.flat_levels += 1;
            } else if zone_of(cut.z).is_some() {
                report.shallow_levels += 1;
            } else {
                report.fine_levels += 1;
            }
        }
    }

    // Every pass, by its slab, its level in the slab and its place there, in
    // the order they are cut.
    let mut order: Vec<Place> = worked
        .iter()
        .enumerate()
        .flat_map(|(s, slab)| {
            slab.iter().enumerate().flat_map(move |(k, cut)| {
                (0..cut.passes.len()).map(move |i| Place {
                    slab: s,
                    level: k,
                    pass: i,
                })
            })
        })
        .collect();
    if params.by_area {
        let bases: Vec<Option<&Region>> = slabs.iter().map(|(z, _)| allowed_at(*z)).collect();
        order = by_area(&order, &worked, &bases);
    }

    // The material stands up to the level above, or the top.
    let above: Vec<f64> = std::iter::once(params.top)
        .chain(slabs.iter().map(|(z, _)| *z))
        .collect();
    let mut path = Toolpath::new();
    let mut last: Option<Place> = None;
    for place in order {
        let Some(cut) = worked.get(place.slab).and_then(|s| s.get(place.level)) else {
            continue;
        };
        let Some((arrive, points)) = cut.passes.get(place.pass) else {
            continue;
        };
        let Some(first) = points.first() else {
            continue;
        };
        let first = dvec3(first.x, first.y, cut.z);
        // Staying down only from the pass it was found after.
        let follows = last.is_some_and(|l| {
            l.slab == place.slab && l.level == place.level && l.pass + 1 == place.pass
        });
        match arrive {
            Arrive::Stay { via } if follows => {
                report.stayed_down += 1;
                // Lifted no higher than the material the level stands under.
                let top = above.get(place.slab).copied().unwrap_or(params.top);
                let lift = params.lift_height.min(top - cut.z).max(0.0);
                let from = path.end().map_or(first.truncate(), |e| e.truncate());
                let way: Vec<DVec2> = std::iter::once(from)
                    .chain(via.iter().copied())
                    .chain(std::iter::once(first.truncate()))
                    .collect();
                for p in lifted(&way, cut.z, lift) {
                    path.feed_to(p, MoveRole::Transition, params.feed)?;
                }
            }
            Arrive::Link | Arrive::Stay { .. } => {
                path.feed_to(first, MoveRole::Link, params.feed)?;
            }
            Arrive::Enter { centre } => {
                let top = above.get(place.slab).copied().unwrap_or(params.top);
                match helix(
                    *centre,
                    first,
                    top + params.safe_distance,
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
        last = Some(place);
    }
    Ok((path, report))
}

/// The way along `way`, from its first point, at level `z`, lifted `lift` off
/// it between: up and down again each as steeply as a link may be without
/// being taken for a plunge ([`crate::path::toolpath::PLUNGE_RATIO`]), half
/// that, and lower where the way is too short to rise so far and come down.
fn lifted(way: &[DVec2], z: f64, lift: f64) -> Vec<glam::DVec3> {
    use crate::path::toolpath::PLUNGE_RATIO;
    let length: f64 = way
        .windows(2)
        .map(|w| match w {
            [a, b] => a.distance(*b),
            _ => 0.0,
        })
        .sum();
    let ramp_per_mm = 2.0 / PLUNGE_RATIO;
    let lift = lift.min(length / (2.0 * ramp_per_mm)).max(0.0);
    let Some(&last) = way.last() else {
        return Vec::new();
    };
    if lift <= MATERIAL_THICKNESS {
        return way.iter().skip(1).map(|p| p.extend(z)).collect();
    }
    let ramp = lift * ramp_per_mm;
    // Where it is up: past the rise, and short of the fall.
    let (up, down) = (ramp, length - ramp);
    let mut out = Vec::new();
    let mut gone = 0.0;
    for w in way.windows(2) {
        let [a, b] = w else {
            continue;
        };
        let leg = a.distance(*b);
        for at in [up, down]
            .into_iter()
            .take(if down - up > 1e-9 { 2 } else { 1 })
        {
            if at > gone + 1e-9 && at < gone + leg - 1e-9 {
                out.push(a.lerp(*b, (at - gone) / leg).extend(z + lift));
            }
        }
        gone += leg;
        if *b != last {
            let h = if gone <= up || gone >= down {
                z + lift * (gone.min(length - gone) / ramp).min(1.0)
            } else {
                z + lift
            };
            out.push(b.extend(h));
        }
    }
    out.push(last.extend(z));
    out
}

/// Where the tool may be, `allowed`, with its inside corners rounded to
/// `radius` as its centre goes: an opening, kept within it, so the tool never
/// turns tighter there and leaves the material in them; and nothing narrower
/// than twice `radius` is left of it. As it is for none.
fn blended(allowed: &Region, radius: f64, arc: f64) -> Result<Region, StrategyError> {
    if radius <= 0.0 {
        return Ok(allowed.clone());
    }
    Ok(allowed
        .offset(-radius, arc)
        .offset(radius, arc)
        .intersection(allowed)?)
}

/// Where a pass is among a run's: its slab, its level in the slab (the slab's
/// own first, then its flat and fine levels) and its place among the level's
/// passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Place {
    slab: usize,
    level: usize,
    pass: usize,
}

/// `order`, by depth, put in order by area: each area a slab's own level
/// splits into, its piece of where the tool may be (`bases`), cleared all the
/// way down before the next, the areas it splits into after it in turn. A
/// level's passes keep their order, and a slab's flat and fine levels follow
/// its own in each area, as they must: each cuts what that one left.
fn by_area(order: &[Place], worked: &[Vec<LevelCut>], bases: &[Option<&Region>]) -> Vec<Place> {
    let pieces: Vec<Vec<Region>> = bases
        .iter()
        .map(|r| r.map(Region::pieces).unwrap_or_default())
        .collect();
    // Which piece of slab `s`'s level `p` is in: the one it is inside, or the
    // nearest.
    let piece_of = |s: usize, p: DVec2| -> usize {
        let Some(ps) = pieces.get(s) else {
            return 0;
        };
        if let Some(k) = ps.iter().position(|r| r.contains(p)) {
            return k;
        }
        ps.iter()
            .enumerate()
            .filter_map(|(k, r)| {
                let (lo, hi) = r.bounds()?;
                Some((k, p.distance(p.clamp(lo, hi))))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(0, |(k, _)| k)
    };
    // Each pass's area, as the pieces of every slab down to its own that its
    // start is in: an area's below it are within it, as where the tool may be
    // only shrinks going down.
    let path_of = |place: &Place| -> Vec<usize> {
        let start = worked
            .get(place.slab)
            .and_then(|s| s.get(place.level))
            .and_then(|c| c.passes.get(place.pass))
            .and_then(|(_, pts)| pts.first().copied())
            .unwrap_or(DVec2::ZERO);
        (0..=place.slab).map(|s| piece_of(s, start)).collect()
    };
    // Areas side by side are cut in the order they are first come to.
    let mut first_seen: std::collections::HashMap<(usize, usize), usize> =
        std::collections::HashMap::new();
    let mut keyed: Vec<(Vec<usize>, usize, Place)> = Vec::with_capacity(order.len());
    for (n, place) in order.iter().enumerate() {
        let pieces = path_of(place);
        for (s, &k) in pieces.iter().enumerate() {
            first_seen.entry((s, k)).or_insert(n);
        }
        keyed.push((pieces, n, *place));
    }
    let mut ranked: Vec<(Vec<usize>, usize, Place)> = keyed
        .into_iter()
        .map(|(pieces, n, place)| {
            let ranks = pieces
                .iter()
                .enumerate()
                .map(|(s, &k)| first_seen.get(&(s, k)).copied().unwrap_or(n))
                .collect();
            (ranks, n, place)
        })
        .collect();
    // An area before the areas below it, which are before the next beside it;
    // within one, by depth as before.
    ranked.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    ranked.into_iter().map(|(_, _, place)| place).collect()
}

/// Clear flat level `f` of a slab over what the levels of the slab `below` it
/// left of `standing`, the material at it: the real tool, `cutter`, as wide at
/// each as it is that far above its tip.
fn clear_flat(
    f: f64,
    allowed: &Region,
    standing: &Region,
    below: &[LevelCut],
    cutter: &Cutter,
    p: &LevelParams,
) -> Result<LevelCut, StrategyError> {
    let mut uncut = Tiles::new(standing, TILE_RADII * p.radius)?;
    for cut in below {
        // The real tool's width that far above its tip.
        let wide = cutter.radius_at(f - cut.z);
        if wide <= 0.0 {
            continue;
        }
        // A little at a time, as the level itself was cleared: the whole sweep
        // at once is a very large polygon.
        cut.swept(|line| uncut.subtract_swept(line, wide, p.arc))?;
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

/// Show `cut`'s passes, at its level, to `watch` ([`Watch::show`]).
fn show_level(watch: &Watch, cut: Option<&LevelCut>) {
    let Some(cut) = cut.filter(|_| watch.showing()) else {
        return;
    };
    watch.show(
        cut.passes
            .iter()
            .map(|(_, points)| points.iter().map(|p| dvec3(p.x, p.y, cut.z)).collect()),
    );
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
    /// Material narrower than twice this is not worth a pass: half the
    /// tolerance, so a wall is never left more than the tolerance past its
    /// stock to leave, at any level. Left thicker, a level below, where
    /// there is material in front of it, runs the wall exactly and the shank
    /// above the flutes rubs what the level above left.
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
    /// How much of what the stay-down distance leaves past going straight a
    /// way round may take.
    stay_down_level: f64,
    /// How far a link keeps off the material between the passes it joins.
    stay_down_clearance: f64,
    /// How wide a slot a closed area is first cut along its middle; none
    /// when `None`.
    slot_width: Option<f64>,
    /// Whether to go down into what has no way in from the side.
    machine_cavities: bool,
    /// Asked whether to stop before each pass.
    watch: Watch,
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
            sliver: p.tolerance * 0.5,
            arc: p.tolerance * 0.1,
            swept_arc: p.tolerance * 0.5,
            coarse: (p.tolerance * 0.5).max(radius * 0.005),
            side,
            both_ways: p.direction == Direction::BothWays,
            helix_radius: p.helix_radius,
            stay_down: p.stay_down,
            stay_down_level: p.stay_down_level.clamp(0.0, 1.0),
            stay_down_clearance: p.stay_down_clearance.max(0.0),
            slot_width: p.slot_width.map(|w| w.max(2.0 * radius)),
            machine_cavities: p.machine_cavities,
            watch: p.watch.clone(),
        }
    }
}

/// How a pass is arrived at.
#[derive(Debug, Clone, PartialEq)]
enum Arrive {
    /// However the linker sees fit: lifting clear, and going down, by a helix
    /// where it starts in material.
    Link,
    /// Across at the level, through what is cleared: straight, or by `via`,
    /// the corners it goes round on the way.
    Stay {
        /// Where it turns between where it was and where the pass starts.
        via: Vec<DVec2>,
    },
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
    /// Whether closed areas were left, as it was not to machine cavities.
    cavities: bool,
    overloads: u64,
    /// Closed areas first cut a slot along their middle.
    slots: u64,
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
            if let (Arrive::Stay { via }, Some(a), Some(&b)) = (arrive, at, points.first()) {
                let way: Vec<DVec2> = std::iter::once(a)
                    .chain(via.iter().copied())
                    .chain(std::iter::once(b))
                    .collect();
                each(&way)?;
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
        cavities: c.cavities,
        overloads: c.overloads,
        slots: c.slots,
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
