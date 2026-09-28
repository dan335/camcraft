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
//! clearing's ([`crate::adaptive`]).
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

use camcraft_geom::contour::{Contour, Region, RegionError, contour_above};
use camcraft_geom::heightmap::HeightMap;
use glam::{DVec2, dvec3};
use rayon::prelude::*;

use crate::planar::pieces;
use crate::reach::{LevelReach, ReachError};
use crate::strategy::{StrategyError, check};
use crate::tool::{Cutter, ToolProfile};
use crate::toolpath::{MoveRole, Toolpath};

/// Stock standing less than this above a level is not material to cut there, in
/// millimetres. Far below any tolerance, far above the grid's f32 storage error.
const MATERIAL_THICKNESS: f64 = 1.0e-3;

/// Levels closer than this are one, in millimetres.
const SAME_LEVEL: f64 = 1.0e-6;

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
    /// Levels whose every piece was offset until the rings covered it.
    pub offsets_exhausted: u64,
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
/// The path contains [`MoveRole::Cut`] passes separated by [`MoveRole::Link`] moves,
/// exactly as a finishing pass does, in the order they are to be cut. The linker
/// resolves them.
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
    if params.top <= params.bottom || !params.top.is_finite() || !params.bottom.is_finite() {
        return Err(StrategyError::BadParameter {
            name: "top",
            value: params.top,
        });
    }
    let area = match limit {
        Some(l) => sweep.intersection(l).map_err(region_err)?,
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

    // Where the tool may be at every level, from the top: the shadows are
    // cheapest asked for in descending order.
    let mut all: Vec<f64> = levels.iter().map(|&(z, _)| z).collect();
    all.sort_by(|a, b| b.total_cmp(a));
    let mut regions: Vec<(f64, Region, Option<Region>)> = Vec::with_capacity(all.len());
    for &z in &all {
        let forbidden = reach.forbidden(z).map_err(reach_err)?;
        let reachable = area.difference(&forbidden).map_err(region_err)?;
        // The part and the stock left on it: where the tool's centre may not go,
        // less the real tool's reach, and a cell for the grid.
        let kept = (leave > 0.0).then(|| forbidden.offset(-(reach.cutter_radius() - slop), arc));
        regions.push((z, reachable, kept));
    }
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
                    material = material
                        .difference(&cleared.offset(wide, arc))
                        .map_err(region_err)?;
                }
            }
            let material = match kept {
                // Less what is kept, the slivers the two outlines leave between
                // them taken back off with a cell's width, and given back after.
                Some(kept) => material
                    .difference(kept)
                    .map_err(region_err)?
                    .offset(-slop, arc)
                    .grow_covering(touch + slop),
                None => material.grow_covering(touch),
            };
            if reachable.is_empty() || material.is_empty() {
                return Ok(None);
            }
            clear_level(sweep, reachable, &material, arc, params).map(Some)
        })
        .collect();

    let mut path = Toolpath::new();
    let mut report = ClearingReport::default();
    let mut tool_at: Option<DVec2> = None;
    for (level, &(z, below)) in worked.into_iter().zip(&levels) {
        let Some(level) = level? else {
            report.empty_levels += 1;
            continue;
        };
        report.rings_too_short += level.too_short;
        report.offsets_exhausted += u64::from(level.exhausted);
        if level.pieces.is_empty() {
            report.empty_levels += 1;
            continue;
        }
        report.levels += 1;
        report.flat_levels += u64::from(below.is_some());
        // The nearest piece next, by where it is first cut.
        let mut left = level.pieces;
        while !left.is_empty() {
            let pick = nearest(&left, tool_at, |tree| distance(tree.anchor(), tool_at));
            let mut cut = Cut {
                z,
                feed: params.feed,
                path: &mut path,
                at: tool_at,
                passes: 0,
            };
            cut.tree(left.remove(pick))?;
            tool_at = cut.at;
            report.rings += cut.passes;
        }
    }

    Ok((path, report))
}

/// The levels, in the order they are cut, each with the level below it if it is
/// a flat one: from the top down a stepdown apart, and each flat level after the
/// one just below it, from the bottom up.
fn levels(params: &ClearingParams) -> Vec<(f64, Option<f64>)> {
    // Bounded by construction, and the count is clamped so a tiny stepdown
    // against a tall part cannot run away.
    let span = params.top - params.bottom;
    let count = ((span / params.stepdown).ceil()).clamp(1.0, 10_000.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = count as u32;
    let mut rough = Vec::new();
    for i in 1..=count {
        let z = (params.top - f64::from(i) * params.stepdown).max(params.bottom);
        rough.push(z);
        if z <= params.bottom {
            break;
        }
    }
    let mut slabs: Vec<(f64, Vec<f64>)> = rough.iter().map(|&z| (z, Vec::new())).collect();
    for &f in &params.flats {
        if !(f > params.bottom && f < params.top)
            || rough.iter().any(|z| (z - f).abs() < SAME_LEVEL)
        {
            continue;
        }
        if let Some(slab) = slabs.iter_mut().find(|(z, _)| *z < f) {
            slab.1.push(f);
        }
    }
    slabs
        .into_iter()
        .flat_map(|(z, mut flats)| {
            flats.sort_by(f64::total_cmp);
            flats.dedup_by(|a, b| (*a - *b).abs() < SAME_LEVEL);
            std::iter::once((z, None)).chain(flats.into_iter().map(move |f| (f, Some(z))))
        })
        .collect()
}

/// One level's passes, a tree of them for each piece; and what was dropped on
/// the way.
struct Level {
    pieces: Vec<Tree>,
    too_short: u64,
    exhausted: bool,
}

/// A piece's rings, as they nest. The piece is its root; what is further in
/// than each stepover falls into regions of its own as it narrows, each a node
/// holding the stretches of its boundary that cut, with the regions a stepover
/// further in, inside it, as its children.
struct Tree {
    nodes: Vec<Node>,
    /// Whether a node's children are cut before it: from the middle out, the
    /// ring on the edges last. Otherwise in from the edges.
    children_first: bool,
}

struct Node {
    passes: Vec<Pass>,
    children: Vec<usize>,
    /// A point where the node's subtree cuts, to go to the nearest by.
    anchor: Option<DVec2>,
}

impl Tree {
    fn anchor(&self) -> Option<DVec2> {
        self.nodes.first().and_then(|n| n.anchor)
    }
}

/// Each piece of `reachable` cleared by rings a stepover apart in from its
/// edges, keeping only what meets `material`.
fn clear_level(
    sweep: &Region,
    reachable: &Region,
    material: &Region,
    arc: f64,
    params: &ClearingParams,
) -> Result<Level, StrategyError> {
    let mut level = Level {
        pieces: Vec::new(),
        too_short: 0,
        exhausted: true,
    };
    for piece in pieces(reachable) {
        // What a ring may cut: material, in the piece. Grown a little, so the ring
        // on the walls, which is the piece's own boundary, is inside it.
        let clip = piece
            .offset(arc, arc)
            .intersection(material)
            .map_err(region_err)?;
        if clip.is_empty() {
            continue;
        }
        // With anything kept beside it, from the middle out; with nothing, in
        // from the sweep's edge, where there is only air.
        let children_first = !walls_of(&piece, sweep, params.stepover.max(1.0))?.is_empty();
        let mut tree = Tree {
            nodes: Vec::new(),
            children_first,
        };
        // The regions a stepover nearer the edges than the ones being found,
        // each with its node.
        let mut last: Vec<(usize, Region)> = Vec::new();
        let mut covered = false;
        for k in 0..MAX_RINGS {
            // What is further than `k` stepovers in from the piece's edges.
            let left = piece.offset(-f64::from(k) * params.stepover, arc);
            if left.is_empty() {
                covered = true;
                break;
            }
            let mut next = Vec::new();
            for region in pieces(&left) {
                let id = tree.nodes.len();
                tree.nodes.push(Node {
                    passes: passes(
                        region.rings(),
                        &clip,
                        params,
                        !children_first,
                        &mut level.too_short,
                    )?,
                    children: Vec::new(),
                    anchor: None,
                });
                if let Some(parent) = parent_of(&region, &last)?
                    && let Some(node) = tree.nodes.get_mut(parent)
                {
                    node.children.push(id);
                }
                next.push((id, region));
            }
            last = next;
        }
        level.exhausted &= covered;
        // Children come after their parents, so each anchor is known before
        // its parent's.
        for i in (0..tree.nodes.len()).rev() {
            let Some(node) = tree.nodes.get(i) else {
                continue;
            };
            let anchor = node
                .passes
                .first()
                .and_then(|p| p.points.first().copied())
                .or_else(|| {
                    node.children
                        .iter()
                        .find_map(|&c| tree.nodes.get(c).and_then(|n| n.anchor))
                });
            if let Some(node) = tree.nodes.get_mut(i) {
                node.anchor = anchor;
            }
        }
        if tree.anchor().is_some() {
            level.pieces.push(tree);
        }
    }
    Ok(level)
}

/// Which of `last`, the regions a stepover nearer the edges, `region` is in. It
/// is in one of them whole, as it is what is left of that one a stepover in.
fn parent_of(region: &Region, last: &[(usize, Region)]) -> Result<Option<usize>, StrategyError> {
    let Some((lo, hi)) = region.bounds() else {
        return Ok(None);
    };
    let slack = DVec2::splat(1e-6);
    let around: Vec<&(usize, Region)> = last
        .iter()
        .filter(|(_, r)| {
            r.bounds().is_some_and(|(rlo, rhi)| {
                (rlo - slack).cmple(lo).all() && (rhi + slack).cmpge(hi).all()
            })
        })
        .collect();
    if let [(id, _)] = around.as_slice() {
        return Ok(Some(*id));
    }
    for (id, r) in around {
        if r.intersection(region).map_err(region_err)?.area() > 0.0 {
            return Ok(Some(*id));
        }
    }
    Ok(None)
}

/// Cutting a level's passes into the path, in order, from where the tool is.
struct Cut<'a> {
    z: f64,
    feed: f64,
    path: &'a mut Toolpath,
    at: Option<DVec2>,
    passes: u64,
}

impl Cut<'_> {
    /// A piece's rings: each region's children and its own rings, in the tree's
    /// order, and of the children the nearest next. Walked with a stack of its
    /// own, as a tree is as deep as the piece is wide over the stepover.
    fn tree(&mut self, mut tree: Tree) -> Result<(), StrategyError> {
        let children = |tree: &Tree, n: usize| {
            tree.nodes
                .get(n)
                .map(|node| node.children.clone())
                .unwrap_or_default()
        };
        // Each node on the way down: the children left to cut, and whether its
        // own rings are cut.
        let mut stack: Vec<(usize, Vec<usize>, bool)> = vec![(0, children(&tree, 0), false)];
        while let Some((node, left, cut)) = stack.last_mut() {
            let node = *node;
            if !*cut && (!tree.children_first || left.is_empty()) {
                *cut = true;
                let passes = tree
                    .nodes
                    .get_mut(node)
                    .map(|n| std::mem::take(&mut n.passes))
                    .unwrap_or_default();
                self.passes(passes)?;
                continue;
            }
            if left.is_empty() {
                stack.pop();
                continue;
            }
            let at = self.at;
            let k = nearest(left, at, |&c| {
                distance(tree.nodes.get(c).and_then(|n| n.anchor), at)
            });
            let child = left.remove(k);
            stack.push((child, children(&tree, child), false));
        }
        Ok(())
    }

    /// Passes the same distance from the walls, the nearest next.
    fn passes(&mut self, mut passes: Vec<Pass>) -> Result<(), StrategyError> {
        while !passes.is_empty() {
            let (k, _) = nearest_in(&passes, self.at);
            let mut pass = passes.remove(k);
            if let Some(p) = self.at {
                pass.start_near(p);
            }
            emit(&pass, self.z, self.feed, self.path)?;
            self.passes += 1;
            self.at = pass.end();
        }
        Ok(())
    }
}

/// How far `to` is from `at`: nowhere is as far as can be.
fn distance(to: Option<DVec2>, at: Option<DVec2>) -> f64 {
    match (to, at) {
        (Some(to), Some(at)) => to.distance(at),
        _ => f64::MAX,
    }
}

/// What is kept beside a piece: everything beside it within the sweep, the part
/// and what the machining boundary keeps the tool out of. Past the sweep's edge
/// is only air, so no wall. Only what is within `margin` of the piece is taken.
fn walls_of(piece: &Region, sweep: &Region, margin: f64) -> Result<Region, StrategyError> {
    let Some((lo, hi)) = piece.bounds() else {
        return Ok(Region::default());
    };
    Region::rectangle(lo - DVec2::splat(margin), hi + DVec2::splat(margin))
        .intersection(sweep)
        .and_then(|near| near.difference(piece))
        .map_err(region_err)
}

/// The stretches of `rings` that cut `clip`, long enough to be worth the entry,
/// each the way it cuts. `inward` says the material they cut is inside the
/// region they bound, as it is cleared in from the sweep's edge; otherwise it is
/// outside, toward the edges, as it is cleared from the middle out.
fn passes(
    rings: &[Contour],
    clip: &Region,
    params: &ClearingParams,
    inward: bool,
    too_short: &mut u64,
) -> Result<Vec<Pass>, StrategyError> {
    let mut out = Vec::new();
    for mut pass in cutting_stretches(rings, clip)? {
        if length(&pass.points) < params.min_ring_length {
            *too_short += 1;
            continue;
        }
        // A ring has its region on its left: the material it cuts is on its left
        // cleared inward, and on its right cleared outward.
        if params.kept_on_right == inward {
            pass.points.reverse();
        }
        out.push(pass);
    }
    Ok(out)
}

/// A stretch of a ring to cut: the whole ring, closed, or an open piece of it.
#[derive(Debug, Clone)]
struct Pass {
    points: Vec<DVec2>,
    closed: bool,
}

impl Pass {
    /// Where the tool is when it has cut it.
    fn end(&self) -> Option<DVec2> {
        if self.closed {
            self.points.first().copied()
        } else {
            self.points.last().copied()
        }
    }

    /// How far it is from `p` to where this pass would start: anywhere on a
    /// closed ring, which can be started anywhere, and an open one's start.
    fn distance(&self, p: DVec2) -> f64 {
        if self.closed {
            self.points
                .iter()
                .map(|q| q.distance(p))
                .fold(f64::MAX, f64::min)
        } else {
            self.points.first().map_or(f64::MAX, |q| q.distance(p))
        }
    }

    /// A closed ring started at its point nearest `p`, going the same way.
    fn start_near(&mut self, p: DVec2) {
        if !self.closed {
            return;
        }
        let nearest = self
            .points
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.distance(p).total_cmp(&b.1.distance(p)))
            .map(|(i, _)| i);
        if let Some(i) = nearest {
            self.points.rotate_left(i);
        }
    }
}

/// Which of `passes` to cut next from `at`, and how far away it is: the
/// nearest, ties to the earlier; the first when the tool is nowhere yet.
fn nearest_in(passes: &[Pass], at: Option<DVec2>) -> (usize, f64) {
    let Some(at) = at else {
        return (0, 0.0);
    };
    passes
        .iter()
        .enumerate()
        .fold((0, f64::MAX), |best, (i, pass)| {
            let d = pass.distance(at);
            if d < best.1 { (i, d) } else { best }
        })
}

/// Which of `items` is nearest, by `distance`, ties to the earlier; the first
/// when the tool is nowhere yet.
fn nearest<T>(items: &[T], at: Option<DVec2>, distance: impl Fn(&T) -> f64) -> usize {
    if at.is_none() {
        return 0;
    }
    items
        .iter()
        .enumerate()
        .fold((0, f64::MAX), |best, (i, item)| {
            let d = distance(item);
            if d < best.1 { (i, d) } else { best }
        })
        .0
}

/// The parts of `rings` that meet material, as passes: a ring wholly in material
/// stays one closed pass, and the rest is cut into the open stretches in it.
fn cutting_stretches(rings: &[Contour], material: &Region) -> Result<Vec<Pass>, StrategyError> {
    let lines: Vec<(Vec<DVec2>, bool)> =
        rings.iter().map(|r| (r.points().to_vec(), true)).collect();
    Ok(material
        .clip_lines(&lines)
        .map_err(region_err)?
        .into_iter()
        .map(|(points, closed)| Pass { points, closed })
        .collect())
}

fn length(points: &[DVec2]) -> f64 {
    points
        .iter()
        .zip(points.iter().skip(1))
        .map(|(a, b)| a.distance(*b))
        .sum()
}

/// Emit one pass as a link to its start followed by cutting moves.
fn emit(pass: &Pass, z: f64, feed: f64, path: &mut Toolpath) -> Result<(), StrategyError> {
    let Some(first) = pass.points.first() else {
        return Ok(());
    };
    // The strategy does not plan how to arrive; the linker does.
    path.feed_to(dvec3(first.x, first.y, z), MoveRole::Link, feed)?;
    for p in pass.points.iter().skip(1) {
        path.feed_to(dvec3(p.x, p.y, z), MoveRole::Cut, feed)?;
    }
    if pass.closed {
        path.feed_to(dvec3(first.x, first.y, z), MoveRole::Cut, feed)?;
    }
    Ok(())
}

fn region_err(e: RegionError) -> StrategyError {
    StrategyError::Region(e.to_string())
}

fn reach_err(e: ReachError) -> StrategyError {
    StrategyError::Region(e.to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests {
    use super::*;
    use crate::allowance::{Allowance, EffectiveTool};
    use crate::dropcutter::DropCutter;
    use crate::tool::Flat;
    use camcraft_geom::mesh::TriMesh;
    use camcraft_geom::synthetic;
    use glam::DVec3;

    const LO: DVec2 = DVec2::new(-12.0, -12.0);
    const HI: DVec2 = DVec2::new(12.0, 12.0);

    const TOLERANCE: f64 = 0.05;

    fn flat(radius: f64) -> EffectiveTool {
        EffectiveTool::new(&Flat::new(radius).unwrap(), Allowance::none()).unwrap()
    }

    fn rough(
        mesh: &TriMesh,
        radius: f64,
        stock: &HeightMap,
        params: &ClearingParams,
    ) -> Result<(Toolpath, ClearingReport), StrategyError> {
        let tool = flat(radius);
        let mut reach = LevelReach::new(mesh, &tool, TOLERANCE).unwrap();
        let sweep = Region::rectangle(LO, HI);
        pocket_clearing(&mut reach, &sweep, None, stock, tool.base(), params)
    }

    fn full_stock(top: f64) -> HeightMap {
        HeightMap::covering(LO, HI, 0.5, top).unwrap()
    }

    fn params(top: f64, bottom: f64) -> ClearingParams {
        ClearingParams {
            stepdown: 2.0,
            stepover: 3.0,
            top,
            bottom,
            ..Default::default()
        }
    }

    #[test]
    fn rejects_unusable_parameters() {
        let mesh = synthetic::plane(0.0, 20.0);
        let radius = 3.0;
        let stock = full_stock(10.0);
        for bad in [
            ClearingParams {
                stepdown: 0.0,
                ..params(10.0, 0.0)
            },
            ClearingParams {
                stepover: -1.0,
                ..params(10.0, 0.0)
            },
            ClearingParams {
                feed: f64::NAN,
                ..params(10.0, 0.0)
            },
            params(0.0, 10.0),
        ] {
            assert!(rough(&mesh, radius, &stock, &bad).is_err());
        }
    }

    /// Clearing above a flat plate should produce full-area passes at each level,
    /// and stop once the levels reach the plate.
    #[test]
    fn clears_a_block_down_to_a_plate() {
        let mesh = synthetic::plane(0.0, 20.0);
        let radius = 3.0;
        let stock = full_stock(10.0);
        let (path, report) = rough(&mesh, radius, &stock, &params(10.0, 0.0)).unwrap();

        assert!(
            report.levels >= 4,
            "expected several levels, got {report:?}"
        );
        assert!(report.rings > 0);
        assert!(
            path.has_unplanned_links(),
            "roughing does not plan its own links"
        );
        for m in path.moves() {
            assert!(
                m.to.z >= -1e-9,
                "nothing should cut below the plate: {:?}",
                m.to
            );
        }
    }

    /// The point of threading stock: material already removed is not cut again.
    #[test]
    fn stock_already_cleared_is_not_cut_again() {
        let mesh = synthetic::plane(0.0, 20.0);
        let radius = 3.0;

        let full = rough(&mesh, radius, &full_stock(10.0), &params(10.0, 0.0))
            .unwrap()
            .1;
        // Same job, but the stock is already down to the plate.
        let done = rough(&mesh, radius, &full_stock(0.0), &params(10.0, 0.0))
            .unwrap()
            .1;

        assert!(full.rings > 0, "a full block must need clearing");
        assert_eq!(
            done.rings, 0,
            "stock already at the finished height needs no roughing"
        );
        assert!(done.empty_levels > 0);
    }

    /// The forbidden region is what keeps the cutter out of the part, and it comes
    /// from the model, so nothing may cut below the CL surface at all: not at a
    /// point, and not between points.
    #[test]
    fn a_tall_boss_is_cut_around_not_through() {
        // A step: plateau at 8 on one side, floor at 0 on the other.
        let mesh = synthetic::step(8.0, 20.0);
        let radius = 2.0;
        let stock = full_stock(12.0);
        let (path, report) = rough(&mesh, radius, &stock, &params(12.0, 0.0)).unwrap();
        assert!(report.rings > 0);
        no_cut_below_the_cl_surface(&mesh, &flat(radius), &path);
    }

    fn no_cut_below_the_cl_surface(mesh: &TriMesh, tool: &EffectiveTool, path: &Toolpath) {
        let dc = DropCutter::new(mesh);
        let mut prev: Option<DVec3> = None;
        for m in path.moves() {
            if m.role == MoveRole::Cut
                && let Some(a) = prev
            {
                for s in [0.0, 0.25, 0.5, 0.75, 1.0] {
                    let p = a.lerp(m.to, s);
                    if let Some(limit) = dc.drop_at(tool, p.x, p.y) {
                        assert!(
                            p.z >= limit - 1e-6,
                            "cut at {p:?} is below the CL surface {limit}"
                        );
                    }
                }
            }
            prev = Some(m.to);
        }
    }

    /// The complaint that started this: a round part must give round rings, not a
    /// staircase. A flat mill against a sphere at level `z` must stay out of the
    /// disc of radius `sqrt(r^2 - z^2) + R`, and the ring against it must follow
    /// that circle to within the tessellation's own error and the tolerance.
    ///
    /// The tessellated sphere lies inside the true one, so staying out of the true
    /// disc less the sag is already staying out of the mesh's; no drop-cutter check
    /// is needed on top, and on a mesh this dense it would dominate the run.
    #[test]
    fn a_round_part_gives_round_rings() {
        let r = 9.0;
        let (lat, lon) = (120, 180);
        let mesh = synthetic::uv_sphere(r, lat, lon);
        let sag = synthetic::uv_sphere_max_sag(r, lat, lon);
        let radius = 2.0;
        let stock = full_stock(r + 2.0);
        let p = ClearingParams {
            stepdown: 2.5,
            stepover: 1.5,
            ..params(r + 2.0, 0.0)
        };
        let (path, _) = rough(&mesh, radius, &stock, &p).unwrap();

        let mut hugging = 0;
        let mut prev: Option<DVec3> = None;
        for m in path.moves() {
            let circle = (r * r - m.to.z * m.to.z).max(0.0).sqrt() + radius;
            if m.role == MoveRole::Cut
                && let Some(a) = prev
                && m.to.z < r
            {
                for s in [0.0, 0.5, 1.0] {
                    let q = a.lerp(m.to, s);
                    let d = q.truncate().length();
                    if d < circle + p.stepover * 0.5 {
                        hugging += 1;
                        assert!(
                            d >= circle - sag - 1e-6,
                            "{q:?} is {:.4} inside the circle",
                            circle - d
                        );
                        assert!(
                            d <= circle + TOLERANCE,
                            "{q:?} is {:.4} outside the circle",
                            d - circle
                        );
                    }
                }
            }
            prev = Some(m.to);
        }
        assert!(
            hugging > 100,
            "too few points on the ring against the part: {hugging}"
        );
    }

    /// After material is gone, rings are clipped to what is left rather than
    /// reshaped by it: what remains of each ring lies in material, and every point
    /// still comes from the model.
    #[test]
    fn rest_roughing_clips_rings_to_the_material_left() {
        let mesh = synthetic::plane(0.0, 20.0);
        let radius = 2.0;
        // Half the block already cut down to the plate: x > 0 is clear.
        let mut stock = full_stock(6.0);
        for iy in 0..stock.ny() {
            for ix in 0..stock.nx() {
                if stock.position(ix, iy).unwrap().x > 0.0 {
                    stock.set(ix, iy, 0.0);
                }
            }
        }
        let (path, report) = rough(&mesh, radius, &stock, &params(6.0, 0.0)).unwrap();
        assert!(report.rings > 0);
        // The stock outline runs half a cell beyond the last full cell.
        let touch = radius + stock.pitch() * (0.5 + std::f64::consts::SQRT_2) + 0.01;
        for m in path.moves().iter().filter(|m| m.role == MoveRole::Cut) {
            assert!(
                m.to.x <= touch,
                "{:?} cuts where the cutter cannot reach any material",
                m.to
            );
        }
        let full = rough(&mesh, radius, &full_stock(6.0), &params(6.0, 0.0))
            .unwrap()
            .0;
        let cut = |p: &Toolpath| p.moves().iter().filter(|m| m.role == MoveRole::Cut).count();
        assert!(cut(&path) < cut(&full), "clipping removed nothing");
    }

    /// Slivers cost a retract, a traverse and a plunge. Past some point that is not
    /// worth the entry, and the parameter must actually govern it.
    #[test]
    fn short_rings_are_dropped() {
        let mesh = synthetic::plane(0.0, 20.0);
        let radius = 3.0;
        let stock = full_stock(6.0);

        let keep = ClearingParams {
            min_ring_length: 0.0,
            ..params(6.0, 0.0)
        };
        let drop = ClearingParams {
            min_ring_length: 1.0e6,
            ..params(6.0, 0.0)
        };

        let a = rough(&mesh, radius, &stock, &keep).unwrap().1;
        let b = rough(&mesh, radius, &stock, &drop).unwrap().1;
        assert!(a.rings > 0);
        assert_eq!(b.rings, 0, "an enormous minimum should drop everything");
        assert!(b.rings_too_short > 0);
    }

    /// Every ring must close, or the cutter leaves a step where it stopped.
    #[test]
    fn rings_close() {
        let mesh = synthetic::plane(0.0, 20.0);
        let radius = 3.0;
        let (path, _) = rough(&mesh, radius, &full_stock(6.0), &params(6.0, 0.0)).unwrap();

        let mut start: Option<DVec3> = None;
        let mut closed = 0;
        let mut prev: Option<DVec3> = None;
        for m in path.moves() {
            if m.role == MoveRole::Link {
                if let (Some(s), Some(p)) = (start, prev) {
                    assert!(
                        (s - p).length() < 1e-6,
                        "ring did not close: {s:?} vs {p:?}"
                    );
                    closed += 1;
                }
                start = Some(m.to);
            }
            prev = Some(m.to);
        }
        if let (Some(s), Some(p)) = (start, prev) {
            assert!((s - p).length() < 1e-6, "last ring did not close");
            closed += 1;
        }
        assert!(closed > 0, "expected closed rings");
    }

    #[test]
    fn levels_never_go_below_the_bottom() {
        let mesh = synthetic::plane(-20.0, 20.0);
        let radius = 3.0;
        let stock = full_stock(10.0);
        let p = ClearingParams {
            stepdown: 3.0,
            ..params(10.0, -5.0)
        };
        let (path, _) = rough(&mesh, radius, &stock, &p).unwrap();
        for m in path.moves() {
            assert!(m.to.z >= -5.0 - 1e-9, "cut below the bottom: {:?}", m.to);
        }
    }

    /// Each run of cutting in `path`, with the level it is at.
    fn passes_of(path: &Toolpath) -> Vec<(f64, Vec<DVec2>)> {
        let mut out: Vec<(f64, Vec<DVec2>)> = Vec::new();
        for m in path.moves() {
            match m.role {
                MoveRole::Link => out.push((m.to.z, vec![m.to.truncate()])),
                _ => {
                    if let Some((_, pts)) = out.last_mut() {
                        pts.push(m.to.truncate());
                    }
                }
            }
        }
        out
    }

    fn signed_area(points: &[DVec2]) -> f64 {
        Contour::new(points.to_vec()).map_or(0.0, |c| c.signed_area())
    }

    /// A pocket, walled in by the machining boundary: a square 16 wide, in a
    /// block with nothing to keep, one level down.
    fn pocket(kept_on_right: bool) -> Toolpath {
        let mesh = synthetic::plane(0.0, 20.0);
        let tool = flat(2.0);
        let mut reach = LevelReach::new(&mesh, &tool, TOLERANCE).unwrap();
        let limit = Region::rectangle(DVec2::splat(-8.0), DVec2::splat(8.0));
        let p = ClearingParams {
            kept_on_right,
            ..params(2.0, 1.0)
        };
        let sweep = Region::rectangle(LO, HI);
        let (path, report) = pocket_clearing(
            &mut reach,
            &sweep,
            Some(&limit),
            &full_stock(2.0),
            tool.base(),
            &p,
        )
        .unwrap();
        assert_eq!(report.levels, 1, "{report:?}");
        path
    }

    /// How far in from the pocket's walls a point is.
    fn in_from_the_walls(p: DVec2) -> f64 {
        8.0 - p.x.abs().max(p.y.abs())
    }

    /// Fusion's pocket: from the middle out, the ring along the walls last, so
    /// the tool goes in where there is room and never cuts a full width beside
    /// the wall.
    #[test]
    fn a_closed_pocket_is_cleared_from_its_middle_out() {
        let passes = passes_of(&pocket(true));
        assert!(passes.len() >= 3, "{}", passes.len());
        let depth = |pts: &[DVec2]| {
            pts.iter()
                .map(|&p| in_from_the_walls(p))
                .fold(f64::MAX, f64::min)
        };
        let depths: Vec<f64> = passes.iter().map(|(_, pts)| depth(pts)).collect();
        for pair in depths.windows(2) {
            let [a, b] = pair else { continue };
            assert!(b <= &(a + 1e-6), "not middle out: {depths:?}");
        }
        let last = depths.last().copied().unwrap();
        assert!(
            last.abs() < 1e-3,
            "the last ring is not on the walls: {depths:?}"
        );
        assert!(
            depths.first().is_some_and(|&d| d > 5.0),
            "the first ring is not in the middle: {depths:?}"
        );
    }

    /// Climbing with a clockwise spindle has the walls on the tool's right,
    /// anticlockwise round the inside of a pocket; conventional goes the other
    /// way.
    #[test]
    fn climbing_keeps_the_walls_on_the_right() {
        for (climb, sign) in [(true, 1.0), (false, -1.0)] {
            let passes = passes_of(&pocket(climb));
            let rings: Vec<f64> = passes
                .iter()
                .map(|(_, pts)| signed_area(pts))
                .filter(|a| a.abs() > 1.0)
                .collect();
            assert!(!rings.is_empty());
            for a in rings {
                assert!(a * sign > 0.0, "climb {climb}: a ring winds {a}");
            }
        }
    }

    /// A boss, 6 wide and 10 tall, in the middle of the block.
    fn boss() -> TriMesh {
        synthetic::cuboid(DVec3::new(-3.0, -3.0, 0.0), DVec3::new(3.0, 3.0, 10.0))
    }

    /// How far a point is from where a tool `radius` wide may be beside the
    /// boss.
    fn off_the_boss(q: DVec2, radius: f64) -> f64 {
        (q.abs() - DVec2::splat(3.0)).max(DVec2::ZERO).length() - radius
    }

    /// How far a point is in from the sweep's edge.
    fn in_from_the_edge(q: DVec2) -> f64 {
        12.0 - q.x.abs().max(q.y.abs())
    }

    /// Round a boss, as Fusion's pocket: the rings a stepover apart in from
    /// the boss and the sweep's edge alike, cut from the middle out, the ring
    /// beside the boss last of its level.
    #[test]
    fn a_boss_is_cleared_from_the_middle_out() {
        let mesh = boss();
        let radius = 2.0;
        let stock = full_stock(10.0);
        let p = params(10.0, 6.0);
        let (path, report) = rough(&mesh, radius, &stock, &p).unwrap();
        assert_eq!(report.levels, 2, "{report:?}");
        // Round the boss's corners the ring on it is the forbidden region's own
        // chords, to the clipper's tick: a micron narrower is clear of it.
        no_cut_below_the_cl_surface(&mesh, &flat(radius - 1e-3), &path);

        let edge = |q: DVec2| off_the_boss(q, radius).min(in_from_the_edge(q));
        for level in [8.0, 6.0] {
            let depths: Vec<f64> = passes_of(&path)
                .into_iter()
                .filter(|(z, _)| (z - level).abs() < 1e-9)
                .map(|(_, pts)| pts.iter().map(|&q| edge(q)).fold(f64::MAX, f64::min))
                .collect();
            assert!(depths.len() >= 3, "{depths:?}");
            for pair in depths.windows(2) {
                assert!(
                    pair.get(1) <= pair.first().map(|a| a + 0.05).as_ref(),
                    "not from the middle out: {depths:?}"
                );
            }
            assert!(
                depths.first().is_some_and(|&d| d > 2.0),
                "the first ring is not in the middle: {depths:?}"
            );
            let last = passes_of(&path)
                .into_iter()
                .rfind(|(z, _)| (z - level).abs() < 1e-9)
                .map(|(_, pts)| pts)
                .unwrap_or_default();
            assert!(
                last.iter().all(|&q| off_the_boss(q, radius).abs() < 0.05)
                    || last.iter().all(|&q| in_from_the_edge(q).abs() < 0.05),
                "the last ring is on neither the boss nor the edge: {last:?}"
            );
        }
    }

    /// Climbing, the material cut is on the tool's right: clockwise round the
    /// boss, which is kept, and anticlockwise along the stock's edge, where the
    /// material is outward.
    #[test]
    fn climbing_round_a_boss_keeps_it_on_the_right() {
        let (path, _) = rough(&boss(), 2.0, &full_stock(10.0), &params(10.0, 8.0)).unwrap();
        let (mut round, mut along) = (0, 0);
        for (_, pts) in passes_of(&path) {
            let a = signed_area(&pts);
            if a.abs() < 1.0 {
                continue;
            }
            let nearer_the_boss = pts
                .iter()
                .all(|&q| off_the_boss(q, 2.0) < in_from_the_edge(q) + 1e-6);
            if nearer_the_boss {
                round += 1;
                assert!(a < 0.0, "a ring round the boss winds {a}");
            } else {
                along += 1;
                assert!(a > 0.0, "a ring along the edge winds {a}");
            }
        }
        assert!(round > 0 && along > 0, "{round} round, {along} along");
    }

    /// A flat face between two levels gets a level of its own, cut after the
    /// level below it, over only the face: the rest the level below cleared.
    #[test]
    fn a_flat_face_between_levels_gets_a_level_of_its_own() {
        // A plateau at 5 for x < 0, a shelf at 0 beyond.
        let mesh = synthetic::step(5.0, 20.0);
        let radius = 2.0;
        let tool = EffectiveTool::new(
            &Flat::new(radius).unwrap(),
            Allowance::new(0.5, 0.5).unwrap(),
        )
        .unwrap();
        let stock = full_stock(10.0);
        let run = |flats: Vec<f64>| {
            let mut reach = LevelReach::new(&mesh, &tool, TOLERANCE).unwrap();
            let p = ClearingParams {
                stepdown: 4.0,
                flats,
                ..params(10.0, 0.5)
            };
            let sweep = Region::rectangle(LO, HI);
            pocket_clearing(&mut reach, &sweep, None, &stock, tool.base(), &p).unwrap()
        };
        let (path, report) = run(vec![5.5]);
        assert_eq!(report.flat_levels, 1, "{report:?}");
        let zs: Vec<f64> = passes_of(&path).iter().map(|(z, _)| *z).collect();
        let first = |z: f64| zs.iter().position(|&at| (at - z).abs() < 1e-9).unwrap();
        // Levels at 6, 2 and 0.5; the flat one after 2, over the plateau. The
        // level at 2 cleared to half a millimetre past the plateau's edge, and
        // the tool reaches that and a grid cell beyond.
        assert!(first(6.0) < first(2.0) && first(2.0) < first(5.5) && first(5.5) < first(0.5));
        let reach = 0.5 + radius + stock.pitch() * 2.0;
        for (z, pts) in passes_of(&path) {
            if (z - 5.5).abs() < 1e-9 {
                assert!(pts.iter().all(|q| q.x < reach), "{pts:?}");
            }
        }
        no_cut_below_the_cl_surface(&mesh, &tool, &path);

        let (without, report) = run(Vec::new());
        assert_eq!(report.flat_levels, 0);
        assert!(
            passes_of(&without)
                .iter()
                .all(|(z, _)| (z - 5.5).abs() > 1e-9)
        );
    }
}
