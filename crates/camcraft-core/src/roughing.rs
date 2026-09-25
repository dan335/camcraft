//! Z-level roughing: clearing material in horizontal slices.
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
//! every point of the path comes from the model's geometry. The stock is a height
//! field, and its outline is a staircase; it only decides *whether* a stretch of a
//! ring cuts anything, by clipping away the stretches that would cut only air. It
//! never shapes a ring.
//!
//! # Contour-parallel, from the boundary inward
//!
//! The first ring runs on the boundary of the reachable region itself, so walls
//! are left exactly the stock to leave. Further rings step inward by the stepover
//! until nothing is left. At the outside of the block, the rings run from the
//! operation's extent inward, so no ring engages more than a stepover of material
//! there. Constant-engagement adaptive clearing is a later strategy and a much
//! larger piece of work.

use camcraft_geom::contour::{Contour, Region, RegionError, contour_above};
use camcraft_geom::heightmap::HeightMap;
use glam::{DVec2, dvec3};
use rayon::prelude::*;

use crate::reach::{LevelReach, ReachError};
use crate::strategy::{StrategyError, check};
use crate::toolpath::{MoveRole, Toolpath};

/// Stock standing less than this above a level is not material to cut there, in
/// millimetres. Far below any tolerance, far above the grid's f32 storage error.
const MATERIAL_THICKNESS: f64 = 1.0e-3;

/// How to clear the material.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoughingParams {
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
}

impl Default for RoughingParams {
    fn default() -> Self {
        Self {
            stepdown: 2.0,
            stepover: 4.0,
            feed: 1500.0,
            top: 0.0,
            bottom: -10.0,
            min_ring_length: 2.0,
        }
    }
}

/// What roughing did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RoughingReport {
    /// Z levels that produced cutting.
    pub levels: u64,
    /// Levels skipped because nothing there could be reached or was worth cutting.
    pub empty_levels: u64,
    /// Passes emitted: whole rings, and the stretches of rings that meet material.
    pub rings: u64,
    /// Passes dropped for being shorter than `min_ring_length`.
    pub rings_too_short: u64,
    /// Offset passes that produced no further rings, ending a level early.
    pub offsets_exhausted: u64,
}

/// Clear material in Z levels.
///
/// `reach` gives where the roughing tool may not be, from the model. `area` is
/// where the tool's centre may go: the stock's footprint, or less where the
/// operation is kept to picked faces. `stock` is the material as it currently
/// stands — so
/// running this after another operation automatically rest-machines, because the
/// stock field already records what that operation removed.
///
/// The path contains [`MoveRole::Cut`] passes separated by [`MoveRole::Link`] moves,
/// exactly as a finishing pass does. The linker resolves them.
pub fn zlevel_rough(
    reach: &mut LevelReach<'_>,
    area: &Region,
    stock: &HeightMap,
    params: &RoughingParams,
) -> Result<(Toolpath, RoughingReport), StrategyError> {
    check("stepdown", params.stepdown)?;
    check("stepover", params.stepover)?;
    check("feed", params.feed)?;
    if params.top <= params.bottom || !params.top.is_finite() || !params.bottom.is_finite() {
        return Err(StrategyError::BadParameter {
            name: "top",
            value: params.top,
        });
    }

    let mut path = Toolpath::new();
    let mut report = RoughingReport::default();
    // Material counts for a ring wherever the cutter would touch it: within the
    // tool's radius, plus a cell's diagonal because the outline is only known to
    // the grid.
    let touch = reach.max_radius() + stock.pitch() * std::f64::consts::SQRT_2;
    // Rings inside the first are drawn to half the chordal tolerance: their arcs
    // err inward, so never toward the part, and a finer arc on a clearing ring
    // buys nothing but points. The first ring is the reachable boundary itself,
    // drawn as finely as the forbidden region was.
    let arc = reach.tolerance() * 0.5;

    // Levels from the top down. Bounded by construction, and the count is clamped so
    // a tiny stepdown against a tall part cannot run away.
    let span = params.top - params.bottom;
    let count = ((span / params.stepdown).ceil()).clamp(1.0, 10_000.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = count as u32;
    let mut levels = Vec::new();
    for i in 1..=count {
        let z = (params.top - f64::from(i) * params.stepdown).max(params.bottom);
        levels.push(z);
        if z <= params.bottom {
            break;
        }
    }

    // Where the tool may be, level by level from the top: the shadows are cheapest
    // asked for in descending order.
    let mut reachable = Vec::with_capacity(levels.len());
    for &z in &levels {
        let forbidden = reach.forbidden(z).map_err(reach_err)?;
        reachable.push(area.difference(&forbidden).map_err(region_err)?);
    }

    // Each level's rings depend on nothing but its own regions, so the levels are
    // worked in parallel and emitted in order: the result is the same on any number
    // of cores.
    let worked: Vec<Result<Option<Level>, StrategyError>> = levels
        .par_iter()
        .zip(reachable.par_iter())
        .map(|(&z, reachable)| {
            // Where there is material to remove: stock still standing above this
            // level. Standing *on* it is nothing to cut; a cut to `z` leaves the
            // stock at `z`, give or take the grid's f32 storage.
            let material = contour_above(stock, z + MATERIAL_THICKNESS).grow_covering(touch);
            if reachable.is_empty() || material.is_empty() {
                return Ok(None);
            }
            clear_region(reachable, &material, arc, params).map(Some)
        })
        .collect();

    for (level, &z) in worked.into_iter().zip(&levels) {
        let Some(level) = level? else {
            report.empty_levels += 1;
            continue;
        };
        report.rings_too_short += level.too_short;
        report.offsets_exhausted += u64::from(level.exhausted);
        if level.passes.is_empty() {
            report.empty_levels += 1;
            continue;
        }
        report.levels += 1;
        for pass in &level.passes {
            emit(pass, z, params.feed, &mut path)?;
            report.rings += 1;
        }
    }

    Ok((path, report))
}

/// One level's passes, and what was dropped on the way.
struct Level {
    passes: Vec<Pass>,
    too_short: u64,
    exhausted: bool,
}

/// Contour-parallel offsets inward until the region is exhausted, keeping only what
/// meets material.
fn clear_region(
    reachable: &Region,
    material: &Region,
    arc: f64,
    params: &RoughingParams,
) -> Result<Level, StrategyError> {
    let mut level = Level {
        passes: Vec::new(),
        too_short: 0,
        exhausted: false,
    };
    // The first ring is the boundary itself: that is where the tool may go, so a
    // wall is left exactly the stock to leave.
    let mut inset = 0.0;
    // Bounded: a region cannot need more passes than its extent divided by the step,
    // and the loop also stops as soon as an offset yields nothing.
    let max_passes = 10_000u32;

    for _ in 0..max_passes {
        let ring_set = reachable.offset(-inset, arc);
        if ring_set.is_empty() {
            level.exhausted = true;
            return Ok(level);
        }
        for pass in cutting_stretches(ring_set.rings(), material)? {
            if length(&pass.points) < params.min_ring_length {
                level.too_short += 1;
                continue;
            }
            level.passes.push(pass);
        }
        inset += params.stepover;
    }
    Ok(level)
}

/// A stretch of a ring to cut: the whole ring, closed, or an open piece of it.
struct Pass {
    points: Vec<DVec2>,
    closed: bool,
}

/// The parts of `rings` that meet material, as passes.
///
/// One clip for the whole set, since the material region can be large. A ring
/// wholly in material stays one closed pass. Otherwise it is cut into the open
/// stretches that lie in material; the stretch running through the ring's starting
/// point comes back from the clipper as two pieces, and is rejoined.
fn cutting_stretches(rings: &[Contour], material: &Region) -> Result<Vec<Pass>, StrategyError> {
    let loops: Vec<Vec<DVec2>> = rings
        .iter()
        .filter_map(|r| {
            let first = *r.points().first()?;
            let mut l = r.points().to_vec();
            l.push(first);
            Some(l)
        })
        .collect();
    let mut pieces: Vec<Option<Vec<DVec2>>> = material
        .clip_polylines(&loops)
        .map_err(region_err)?
        .into_iter()
        .filter(|p| p.len() >= 2)
        .map(Some)
        .collect();

    // Ring starts, to the clipper's tick: rings come from the clipper, so their
    // points are on its grid and a start survives the clip exactly.
    let key = |p: DVec2| -> (i64, i64) {
        // Rounded well inside range, so the narrowing is exact.
        #[allow(clippy::cast_possible_truncation)]
        let k = ((p.x * 1.0e3).round() as i64, (p.y * 1.0e3).round() as i64);
        k
    };
    let starts: std::collections::HashMap<(i64, i64), usize> = rings
        .iter()
        .enumerate()
        .filter_map(|(i, r)| Some((key(*r.points().first()?), i)))
        .collect();
    let ring_of = |p: Option<&DVec2>| p.and_then(|&q| starts.get(&key(q)).copied());

    let mut passes = Vec::new();
    for i in 0..rings.len() {
        let Some(ring) = rings.get(i) else { continue };
        let starting = pieces
            .iter()
            .position(|p| p.as_ref().is_some_and(|p| ring_of(p.first()) == Some(i)));
        let ending = pieces
            .iter()
            .position(|p| p.as_ref().is_some_and(|p| ring_of(p.last()) == Some(i)));
        match (starting, ending) {
            // Wholly inside: the clipper hands back the ring itself.
            (Some(a), Some(b))
                if a == b
                    && pieces
                        .get(a)
                        .and_then(Option::as_ref)
                        .is_some_and(|p| (length(p) - ring.perimeter()).abs() < 1e-6) =>
            {
                pieces.get_mut(a).map(Option::take);
                passes.push(Pass {
                    points: ring.points().to_vec(),
                    closed: true,
                });
            }
            (Some(a), Some(b)) if a != b => {
                let tail = pieces.get_mut(a).and_then(Option::take).unwrap_or_default();
                if let Some(Some(head)) = pieces.get_mut(b) {
                    head.extend(tail.into_iter().skip(1));
                }
            }
            _ => {}
        }
    }
    passes.extend(pieces.into_iter().flatten().map(|points| Pass {
        points,
        closed: false,
    }));
    Ok(passes)
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
        params: &RoughingParams,
    ) -> Result<(Toolpath, RoughingReport), StrategyError> {
        let tool = flat(radius);
        let mut reach = LevelReach::new(mesh, &tool, TOLERANCE).unwrap();
        zlevel_rough(&mut reach, &Region::rectangle(LO, HI), stock, params)
    }

    fn full_stock(top: f64) -> HeightMap {
        HeightMap::covering(LO, HI, 0.5, top).unwrap()
    }

    fn params(top: f64, bottom: f64) -> RoughingParams {
        RoughingParams {
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
            RoughingParams {
                stepdown: 0.0,
                ..params(10.0, 0.0)
            },
            RoughingParams {
                stepover: -1.0,
                ..params(10.0, 0.0)
            },
            RoughingParams {
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
        let p = RoughingParams {
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

        let keep = RoughingParams {
            min_ring_length: 0.0,
            ..params(6.0, 0.0)
        };
        let drop = RoughingParams {
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
        let p = RoughingParams {
            stepdown: 3.0,
            ..params(10.0, -5.0)
        };
        let (path, _) = rough(&mesh, radius, &stock, &p).unwrap();
        for m in path.moves() {
            assert!(m.to.z >= -5.0 - 1e-9, "cut below the bottom: {:?}", m.to);
        }
    }
}
