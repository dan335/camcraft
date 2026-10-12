//! Pocket clearing: clearing material in horizontal slices, by offsets. Fusion's
//! 3D Pocket Clearing.
//!
//! At a level `z` the tool may be wherever the cutter would not overlap the part:
//! the complement of [`LevelReach::forbidden`], which comes straight from the
//! model's triangles. What is left, within the operation's extent, is where the
//! tool can go.
//!
//! Two distinct regions, and conflating them is the classic roughing bug:
//!
//! - **Reachable**: where the tool *may* be.
//! - **Worth cutting**: stock still stands above `z`. Where there is anything *to*
//!   remove.
//!
//! Cutting the first without the second produces air passes over finished floors.
//! Cutting the second without the first drives the cutter into the part.
//!
//! # Paths from the model, material from the stock
//!
//! Grids are only for stock. Every ring is an offset of the reachable region, so
//! every point of the path comes from the model's geometry. The stock is a
//! height field, and its outline is a staircase; it only decides *whether* a
//! stretch of a ring cuts anything, by clipping away the stretches that would cut
//! only air. It never shapes a ring.
//!
//! # From the middle out, the ring on the walls last
//!
//! Each piece of the reachable region is cleared by rings a stepover apart in
//! from its edges, as Fusion's are. What is further in than each stepover falls
//! into regions of its own as it narrows, and they nest: each is cut from its
//! middle out, the regions inside it before its own rings, and of those left the
//! nearest next. So the tool finishes one arm of a piece before moving to the
//! next, never goes back to a ring it has left, and the last ring of each region
//! runs along its walls, a light cut that leaves exactly the stock to leave. Its
//! rings stay closed wherever material allows, so it links along them without
//! lifting. The first ring of a region into full material is a slot, which a
//! ramp goes down into; holding the load constant everywhere is adaptive
//! clearing's ([`crate::strategy::adaptive`]).
//!
//! A piece with nothing kept beside it, a level above the part, is cleared the
//! other way, in from the sweep's edge, where the tool starts in air.
//!
//! # Direction
//!
//! Climb by default. With a clockwise spindle a climb cut has the material it is
//! cutting on the tool's right: outward, toward the edges, cutting from the
//! middle out, and inward cutting in from the edges. A spindle turning the other
//! way, or conventional cutting, puts it on the left.
//!
//! # Flat areas
//!
//! A flat face of the part between two levels would be left a terrace as deep
//! as a stepdown. With flat area detection a level is added on each one, cut
//! after the level below it over what that left, as Fusion's is.

use camcraft_geom::contour::{Contour, Region, contour_above, polyline_length};
use camcraft_geom::heightmap::HeightMap;
use glam::DVec2;
use rayon::prelude::*;

use crate::cutter::tool::{Cutter, ToolProfile};
use crate::path::toolpath::Toolpath;
use crate::strategy::nesting::{Cuts, Nested, distance, nearest};
use crate::strategy::parallel::Run;
use crate::strategy::projected::Emit;
use crate::strategy::reach::LevelReach;
use crate::strategy::{MATERIAL_THICKNESS, SAME_LEVEL, StrategyError, check, check_levels, slabs};

/// Offsets bounded: a piece cannot need more rings than its extent divided by the
/// step, and each loop also stops as soon as the rings cover the piece.
const MAX_RINGS: u32 = 10_000;

/// How to clear the material.
#[derive(Debug, Clone, PartialEq)]
pub struct ClearingParams {
    /// Depth of each slice, in millimetres.
    pub stepdown: f64,
    /// Distance between neighbouring offsets, in millimetres.
    pub stepover: f64,
    /// Cutting feed, in mm/min.
    pub feed: f64,
    /// Highest level to cut at. Usually the top of the stock.
    pub top: f64,
    /// Lowest level to cut at. Usually the bottom of the part.
    pub bottom: f64,
    /// Passes shorter than this are dropped rather than emitted.
    ///
    /// A pass a few tool diameters long costs a retract, a traverse and a plunge to
    /// remove a sliver of material. Below some length that is not worth the entry.
    pub min_ring_length: f64,
    /// Whether the material being cut is on the tool's right: climbing with a
    /// clockwise spindle.
    pub kept_on_right: bool,
    /// Extra levels, each on a flat face of the part, raised by the stock to
    /// leave down: cut after the level below each, over what it left.
    pub flats: Vec<f64>,
}

impl Default for ClearingParams {
    fn default() -> Self {
        Self {
            stepdown: 2.0,
            stepover: 4.0,
            feed: 1500.0,
            top: 0.0,
            bottom: -10.0,
            min_ring_length: 2.0,
            kept_on_right: true,
            flats: Vec::new(),
        }
    }
}

/// What pocket clearing did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClearingReport {
    /// Z levels that produced cutting, flat ones included.
    pub levels: u64,
    /// Of them, levels on flat faces.
    pub flat_levels: u64,
    /// Levels skipped because nothing there could be reached or was worth cutting.
    pub empty_levels: u64,
    /// Passes emitted: whole rings, and the stretches of rings that meet material.
    pub rings: u64,
    /// Passes dropped for being shorter than `min_ring_length`.
    pub rings_too_short: u64,
}

/// Clear material in Z levels.
///
/// `reach` gives where the tool may not be, from the model. `sweep` is where the
/// tool's centre may go at all: the stock's footprint and the tool past it, with
/// only air beyond its edge. `limit`, if any, is the machining boundary the
/// centre is kept to within it. `stock` is the material as it currently stands —
/// so running this after another operation automatically rest-machines, because
/// the stock field already records what that operation removed. `cutter` is the
/// real tool, whose width at each height says how much the level below a flat
/// one cleared.
///
/// The path contains [`MoveRole::Cut`](crate::path::toolpath::MoveRole::Cut) passes
/// separated by [`MoveRole::Link`](crate::path::toolpath::MoveRole::Link) moves, exactly
/// as a finishing pass does, in the order they are to be cut. The linker resolves
/// them.
pub fn pocket_clearing(
    reach: &mut LevelReach<'_>,
    sweep: &Region,
    limit: Option<&Region>,
    stock: &HeightMap,
    cutter: &Cutter,
    params: &ClearingParams,
) -> Result<(Toolpath, ClearingReport), StrategyError> {
    check("stepdown", params.stepdown)?;
    check("stepover", params.stepover)?;
    check("feed", params.feed)?;
    check_levels(params.top, params.bottom)?;
    let area = match limit {
        Some(l) => sweep.intersection(l)?,
        None => sweep.clone(),
    };

    // Material counts for a ring wherever the cutter would touch it: within the
    // real tool's radius, plus a cell's diagonal because the outline is only
    // known to the grid.
    let slop = stock.pitch() * std::f64::consts::SQRT_2;
    let touch = reach.cutter_radius() + slop;
    // The stock to leave beside: what an operation before this one that left it
    // left standing on the walls, which no ring here may cut.
    let leave = reach.max_radius() - reach.cutter_radius();
    // Rings off the walls are drawn to half the chordal tolerance: their arcs err
    // away from the walls, so never toward the part, and a finer arc on a
    // clearing ring buys nothing but points. The ring on the walls is their
    // boundary itself, drawn as finely as the forbidden region was.
    let arc = reach.tolerance() * 0.5;

    let levels = levels(params);

    // Where the tool may not be at every level, from the top: the shadows are
    // cheapest asked for in descending order. What is left of the area, and what
    // is kept on the part, each level's own, in parallel.
    let mut all: Vec<f64> = levels.iter().map(|&(z, _)| z).collect();
    all.sort_by(|a, b| b.total_cmp(a));
    let mut forbidden = Vec::with_capacity(all.len());
    for &z in &all {
        forbidden.push((z, reach.forbidden(z)?));
    }
    let radius = reach.cutter_radius();
    let regions: Vec<(f64, Region, Option<Region>)> = forbidden
        .into_par_iter()
        .map(|(z, forbidden)| {
            let reachable = area.difference(&forbidden)?;
            // The part and the stock left on it: where the tool's centre may not
            // go, less the real tool's reach, and a cell for the grid.
            let kept = (leave > 0.0).then(|| forbidden.offset(-(radius - slop), arc));
            Ok((z, reachable, kept))
        })
        .collect::<Result<_, StrategyError>>()?;
    let regions_at = |z: f64| {
        regions
            .iter()
            .find(|(at, ..)| (at - z).abs() < SAME_LEVEL)
            .map(|(_, reachable, kept)| (reachable, kept.as_ref()))
    };

    // Each level's rings depend on nothing but its own regions, so the levels are
    // worked in parallel and emitted in order: the result is the same on any number
    // of cores.
    let worked: Vec<Result<Option<Level>, StrategyError>> = levels
        .par_iter()
        .map(|&(z, below)| {
            let Some((reachable, kept)) = regions_at(z) else {
                return Ok(None);
            };
            // Where there is material to remove: stock still standing above this
            // level. Standing *on* it is nothing to cut; a cut to `z` leaves the
            // stock at `z`, give or take the grid's f32 storage. What is kept on
            // the part is not.
            let mut material = contour_above(stock, z + MATERIAL_THICKNESS);
            // A flat level is cut after the level below it, over what that left:
            // the tool there went everywhere it could, as wide as it is this far
            // above its tip.
            if let Some(b) = below
                && let Some((cleared, _)) = regions_at(b)
            {
                let wide = cutter.radius_at(z - b);
                if wide > 0.0 {
                    material = material.difference(&cleared.offset(wide, arc))?;
                }
            }
            let material = match kept {
                // Less what is kept, the slivers the two outlines leave between
                // them taken back off with a cell's width, and given back after.
                Some(kept) => material
                    .difference(kept)?
                    .offset(-slop, arc)
                    .grow_covering(touch + slop),
                None => material.grow_covering(touch),
            };
            if reachable.is_empty() || material.is_empty() {
                return Ok(None);
            }
            clear_level(sweep, reachable, &material, z, arc, params).map(Some)
        })
        .collect();

    let mut report = ClearingReport::default();
    let mut cut = Cut {
        emit: Emit::new(params.feed),
    };
    for (level, &(_, below)) in worked.into_iter().zip(&levels) {
        let Some(level) = level? else {
            report.empty_levels += 1;
            continue;
        };
        report.rings_too_short += level.too_short;
        if level.pieces.is_empty() {
            report.empty_levels += 1;
            continue;
        }
        report.levels += 1;
        report.flat_levels += u64::from(below.is_some());
        cut.pieces(level.pieces)?;
    }
    report.rings = cut.emit.passes;
    Ok((cut.emit.path, report))
}

/// The levels, in the order they are cut, each with the level below it if it is
/// a flat one: from the top down a stepdown apart, and each flat level after the
/// one just below it, from the bottom up.
fn levels(params: &ClearingParams) -> Vec<(f64, Option<f64>)> {
    slabs(params.top, params.bottom, params.stepdown, &params.flats)
        .into_iter()
        .flat_map(|(z, flats)| {
            std::iter::once((z, None)).chain(flats.into_iter().map(move |f| (f, Some(z))))
        })
        .collect()
}

/// One level's passes, a tree of them for each piece; and what was dropped on
/// the way.
///
/// A piece's rings nest: the piece is the root, and what is further in than each
/// stepover falls into regions of its own as it narrows, each holding the
/// stretches of its boundary that cut.
struct Level {
    pieces: Vec<Nested<Run>>,
    too_short: u64,
}

/// Each piece of `reachable` cleared by rings a stepover apart in from its
/// edges, at level `z`, keeping only what meets `material`.
fn clear_level(
    sweep: &Region,
    reachable: &Region,
    material: &Region,
    z: f64,
    arc: f64,
    params: &ClearingParams,
) -> Result<Level, StrategyError> {
    let mut level = Level {
        pieces: Vec::new(),
        too_short: 0,
    };
    for piece in reachable.pieces() {
        // What a ring may cut: material, in the piece. Grown a little, so the ring
        // on the walls, which is the piece's own boundary, is inside it.
        let clip = piece.offset(arc, arc).intersection(material)?;
        if clip.is_empty() {
            continue;
        }
        // In material all over, no ring needs clipping.
        let clip = (!piece.difference(material)?.is_empty()).then_some(&clip);
        // With anything kept beside it, from the middle out; with nothing, in
        // from the sweep's edge, where there is only air.
        let children_first = has_walls(&piece, sweep, params.stepover.max(1.0))?;
        let rings = Concentric {
            stepover: params.stepover,
            arc,
            min_length: params.min_ring_length,
            kept_on_right: params.kept_on_right,
            children_first,
        };
        let tree = rings.clear(&piece, clip, z, &mut level.too_short)?;
        if tree.anchor(0).is_some() {
            level.pieces.push(tree);
        }
    }
    Ok(level)
}

/// Rings a stepover apart in from a region's edges, each the way it cuts: how a
/// pocket clearing level and a flat are cleared.
pub(crate) struct Concentric {
    /// Distance between rings, in millimetres.
    pub(crate) stepover: f64,
    /// Arc tolerance for the offsets.
    pub(crate) arc: f64,
    /// Stretches shorter than this are not worth the entry.
    pub(crate) min_length: f64,
    /// Whether the material cut is on the tool's right.
    pub(crate) kept_on_right: bool,
    /// Whether they are cut from the middle out, each region's rings after the
    /// ones further in; otherwise in from the edges.
    pub(crate) children_first: bool,
}

impl Concentric {
    /// `piece`'s rings at level `z`, as the regions they bound nest, settled to
    /// be cut; only what meets `clip`, if given. Stretches too short are counted
    /// in `too_short`.
    pub(crate) fn clear(
        &self,
        piece: &Region,
        clip: Option<&Region>,
        z: f64,
        too_short: &mut u64,
    ) -> Result<Nested<Run>, StrategyError> {
        let mut tree = Nested::new(self.children_first);
        // The regions a stepover nearer the edges than the ones being found,
        // each with its node and bounds.
        let mut last: Vec<(usize, Region, Bounds)> = Vec::new();
        for k in 0..MAX_RINGS {
            // What is further than `k` stepovers in from the piece's edges.
            let left = piece.offset(-f64::from(k) * self.stepover, self.arc);
            if left.is_empty() {
                break;
            }
            let mut next = Vec::new();
            for region in left.pieces() {
                let rings = passes(
                    region.rings(),
                    clip,
                    z,
                    self.min_length,
                    self.kept_on_right,
                    !self.children_first,
                    too_short,
                )?;
                let id = tree.push(rings);
                if let Some(parent) = parent_of(&region, &last) {
                    tree.nest(parent, id);
                }
                let Some(bounds) = region.bounds() else {
                    continue;
                };
                next.push((id, region, bounds));
            }
            last = next;
        }
        tree.settle(|r| r.points.first().map(|p| p.truncate()));
        Ok(tree)
    }
}

/// A region's lower and upper corners.
type Bounds = (DVec2, DVec2);

/// Which of `last`, the regions a stepover nearer the edges, `region` is in. It
/// is in one of them whole, a stepover in from its edges, as it is what is left
/// of that one: so whichever holds any point of it.
fn parent_of(region: &Region, last: &[(usize, Region, Bounds)]) -> Option<usize> {
    let (lo, hi) = region.bounds()?;
    let slack = DVec2::splat(1e-6);
    let mut around = last.iter().filter(|(_, _, (rlo, rhi))| {
        (*rlo - slack).cmple(lo).all() && (*rhi + slack).cmpge(hi).all()
    });
    let first = around.next()?;
    let Some(second) = around.next() else {
        return Some(first.0);
    };
    let p = *region.rings().first()?.points().first()?;
    std::iter::once(first)
        .chain(std::iter::once(second))
        .chain(around)
        .find(|(_, r, _)| r.contains(p))
        .map(|(id, ..)| *id)
}

/// Cutting levels' rings into the path, in order, from where the tool is.
pub(crate) struct Cut {
    pub(crate) emit: Emit,
}

impl Cut {
    /// Each of `pieces`' rings, the nearest piece next, by where it is first cut.
    pub(crate) fn pieces(&mut self, mut pieces: Vec<Nested<Run>>) -> Result<(), StrategyError> {
        while !pieces.is_empty() {
            let pick = nearest(&pieces, |tree| distance(tree.anchor(0), self.emit.at));
            pieces.remove(pick).cut(0, self)?;
        }
        Ok(())
    }
}

impl Cuts<Run> for Cut {
    type Error = StrategyError;

    fn at(&self) -> Option<DVec2> {
        self.emit.at
    }

    /// Rings the same distance from the walls, the nearest next, each started
    /// where it comes nearest.
    fn cut(&mut self, runs: Vec<Run>) -> Result<(), StrategyError> {
        self.emit.nearest_first(runs, false)
    }
}

/// Whether anything is kept beside a piece, within `margin` of it: the part, or
/// what the machining boundary keeps the tool out of. Past the sweep's edge is
/// only air, so no wall. The piece lies within the sweep, so anything of the
/// sweep near it and not in it is a wall.
fn has_walls(piece: &Region, sweep: &Region, margin: f64) -> Result<bool, StrategyError> {
    let Some((lo, hi)) = piece.bounds() else {
        return Ok(false);
    };
    let near = Region::rectangle(lo - DVec2::splat(margin), hi + DVec2::splat(margin))
        .intersection(sweep)?;
    Ok(near.area() - piece.area() > 1e-6)
}

/// The stretches of `rings` that cut `clip`, long enough to be worth the entry,
/// each at level `z` the way it cuts: a whole ring stays closed. `inward` says
/// the material they cut is inside the region they bound, as it is cleared in
/// from the sweep's edge; otherwise it is outside, toward the edges, as it is
/// cleared from the middle out.
fn passes(
    rings: &[Contour],
    clip: Option<&Region>,
    z: f64,
    min_length: f64,
    kept_on_right: bool,
    inward: bool,
    too_short: &mut u64,
) -> Result<Vec<Run>, StrategyError> {
    let mut out = Vec::new();
    for (mut points, closed) in cutting_stretches(rings, clip)? {
        if polyline_length(&points) < min_length {
            *too_short += 1;
            continue;
        }
        // A ring has its region on its left: the material it cuts is on its left
        // cleared inward, and on its right cleared outward.
        if kept_on_right == inward {
            points.reverse();
        }
        out.push(Run {
            points: points.into_iter().map(|p| p.extend(z)).collect(),
            closed,
        });
    }
    Ok(out)
}

/// The parts of `rings` that meet material: a ring wholly in material stays
/// whole, closed, and the rest is cut into the open stretches in it. With no
/// material to clip to, every ring is in it.
fn cutting_stretches(
    rings: &[Contour],
    material: Option<&Region>,
) -> Result<Vec<(Vec<DVec2>, bool)>, StrategyError> {
    let lines: Vec<(Vec<DVec2>, bool)> =
        rings.iter().map(|r| (r.points().to_vec(), true)).collect();
    Ok(match material {
        Some(material) => material.clip_lines(&lines)?,
        None => lines,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests;
