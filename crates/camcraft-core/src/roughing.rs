//! Z-level roughing: clearing material in horizontal slices.
//!
//! At a level `z` the tool may occupy a column only if the cutter-location surface
//! there is at or below `z`. So the forbidden region is `{ Zcl(x,y) > z }`, and what
//! is left — intersected with the stock that actually has material above `z` — is
//! where the tool can go.
//!
//! Two distinct regions, and conflating them is the classic roughing bug:
//!
//! - **Reachable**: `Zcl <= z`. Where the tool *may* be.
//! - **Worth cutting**: stock still stands above `z`. Where there is anything *to*
//!   remove.
//!
//! Cutting the first without the second produces air passes over finished floors.
//! Cutting the second without the first drives the cutter into the part.
//!
//! # Contour-parallel, from the boundary inward
//!
//! Offsets march inward from the region boundary by the stepover until nothing is
//! left. Inward-from-the-edge keeps full-width engagement off the walls, which is
//! the conservative choice; constant-engagement adaptive clearing is a later
//! strategy and a much larger piece of work.

use camcraft_geom::contour::{Region, RegionError, contour_above};
use camcraft_geom::heightmap::HeightMap;
use glam::{DVec2, DVec3, dvec3};

use crate::clmap::ClMap;
use crate::parallel::StrategyError;
use crate::toolpath::{MoveRole, Toolpath};

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
    /// Rings shorter than this are dropped rather than emitted.
    ///
    /// A ring a few tool diameters long costs a retract, a traverse and a plunge to
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
    /// Closed rings emitted.
    pub rings: u64,
    /// Rings dropped for being shorter than `min_ring_length`.
    pub rings_too_short: u64,
    /// Offset passes that produced no further rings, ending a level early.
    pub offsets_exhausted: u64,
}

/// Clear material in Z levels.
///
/// `cl` is the cutter-location map for the roughing tool, and `stock` is the
/// material as it currently stands — so running this after another operation
/// automatically rest-machines, because the stock field already records what that
/// operation removed.
///
/// The path contains [`MoveRole::Cut`] rings separated by [`MoveRole::Link`] moves,
/// exactly as a finishing pass does. The linker resolves them.
pub fn zlevel_rough(
    cl: &ClMap,
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

    // Levels from the top down. Bounded by construction, and the count is clamped so
    // a tiny stepdown against a tall part cannot run away.
    let span = params.top - params.bottom;
    let count = ((span / params.stepdown).ceil()).clamp(1.0, 10_000.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = count as u32;

    for i in 1..=count {
        let z = (params.top - f64::from(i) * params.stepdown).max(params.bottom);

        // Where the tool may be: the complement of the forbidden region, within the
        // area the CL map covers.
        let forbidden = contour_above(cl.grid(), z);
        let extent = grid_extent(cl.grid());
        let reachable = extent.difference(&forbidden).map_err(region_err)?;
        if reachable.is_empty() {
            report.empty_levels += 1;
            continue;
        }

        // Where there is material to remove: stock still standing above this level.
        let has_material = contour_above(stock, z);
        let target = reachable.intersection(&has_material).map_err(region_err)?;
        if target.is_empty() {
            report.empty_levels += 1;
            continue;
        }

        let before = report.rings;
        clear_region(&target, z, params, &mut path, &mut report)?;
        if report.rings > before {
            report.levels += 1;
        } else {
            report.empty_levels += 1;
        }

        if z <= params.bottom {
            break;
        }
    }

    Ok((path, report))
}

/// Emit contour-parallel offsets inward until the region is exhausted.
fn clear_region(
    target: &Region,
    z: f64,
    params: &RoughingParams,
    path: &mut Toolpath,
    report: &mut RoughingReport,
) -> Result<(), StrategyError> {
    // The first offset steps in by half a stepover so the cutter's edge lands on the
    // region boundary rather than its centre; after that, full steps.
    let mut inset = params.stepover * 0.5;
    // Bounded: a region cannot need more passes than its extent divided by the step,
    // and the loop also stops as soon as an offset yields nothing.
    let max_passes = 10_000u32;

    for _ in 0..max_passes {
        let ring_set = target.offset(-inset);
        if ring_set.is_empty() {
            report.offsets_exhausted += 1;
            return Ok(());
        }
        for ring in ring_set.rings() {
            if ring.perimeter() < params.min_ring_length {
                report.rings_too_short += 1;
                continue;
            }
            emit_ring(ring.points(), z, params.feed, path)?;
            report.rings += 1;
        }
        inset += params.stepover;
    }
    Ok(())
}

/// Emit one closed ring as a link to its start followed by cutting moves.
fn emit_ring(
    points: &[DVec2],
    z: f64,
    feed: f64,
    path: &mut Toolpath,
) -> Result<(), StrategyError> {
    let Some(first) = points.first() else {
        return Ok(());
    };
    // The strategy does not plan how to arrive; the linker does.
    path.feed_to(dvec3(first.x, first.y, z), MoveRole::Link, feed)?;
    for p in points.iter().skip(1) {
        path.feed_to(dvec3(p.x, p.y, z), MoveRole::Cut, feed)?;
    }
    // Close the ring.
    path.feed_to(dvec3(first.x, first.y, z), MoveRole::Cut, feed)?;
    Ok(())
}

fn grid_extent(g: &HeightMap) -> Region {
    let o = g.origin();
    let far = DVec2::new(
        o.x + f64::from(g.nx().saturating_sub(1)) * g.pitch(),
        o.y + f64::from(g.ny().saturating_sub(1)) * g.pitch(),
    );
    Region::rectangle(o, far)
}

fn region_err(e: RegionError) -> StrategyError {
    StrategyError::Region(e.to_string())
}

fn check(name: &'static str, value: f64) -> Result<(), StrategyError> {
    (value.is_finite() && value > 0.0)
        .then_some(())
        .ok_or(StrategyError::BadParameter { name, value })
}

/// Height of the tool tip, for a ring lying at `z`.
///
/// Trivial today, but it is where a per-ring lift would go if roughing ever needed
/// to ride over something rather than cut through it.
pub fn ring_height(z: f64) -> DVec3 {
    dvec3(0.0, 0.0, z)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests {
    use super::*;
    use crate::clmap::ClMap;
    use crate::dropcutter::DropCutter;
    use crate::tool::Flat;
    use camcraft_geom::synthetic;

    const LO: DVec2 = DVec2::new(-12.0, -12.0);
    const HI: DVec2 = DVec2::new(12.0, 12.0);

    fn cl_for(mesh: &camcraft_geom::mesh::TriMesh, radius: f64) -> ClMap {
        let dc = DropCutter::new(mesh);
        let tool = Flat::new(radius).unwrap();
        ClMap::compute(&dc, &tool, LO, HI, 0.5).unwrap()
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
        let cl = cl_for(&mesh, 3.0);
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
            assert!(zlevel_rough(&cl, &stock, &bad).is_err());
        }
    }

    /// Clearing above a flat plate should produce full-area passes at each level,
    /// and stop once the levels reach the plate.
    #[test]
    fn clears_a_block_down_to_a_plate() {
        let mesh = synthetic::plane(0.0, 20.0);
        let cl = cl_for(&mesh, 3.0);
        let stock = full_stock(10.0);
        let (path, report) = zlevel_rough(&cl, &stock, &params(10.0, 0.0)).unwrap();

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
        let cl = cl_for(&mesh, 3.0);

        let full = zlevel_rough(&cl, &full_stock(10.0), &params(10.0, 0.0))
            .unwrap()
            .1;
        // Same job, but the stock is already down to the plate.
        let done = zlevel_rough(&cl, &full_stock(0.0), &params(10.0, 0.0))
            .unwrap()
            .1;

        assert!(full.rings > 0, "a full block must need clearing");
        assert_eq!(
            done.rings, 0,
            "stock already at the finished height needs no roughing"
        );
        assert!(done.empty_levels > 0);
    }

    /// The forbidden region is what keeps the cutter out of the part.
    #[test]
    fn a_tall_boss_is_cut_around_not_through() {
        // A step: plateau at 8 on one side, floor at 0 on the other.
        let mesh = synthetic::step(8.0, 20.0);
        let cl = cl_for(&mesh, 2.0);
        let stock = full_stock(12.0);
        let (path, report) = zlevel_rough(&cl, &stock, &params(12.0, 0.0)).unwrap();
        assert!(report.rings > 0);

        // Below the plateau top, nothing may be cut over the plateau side (x < 0).
        let dc = DropCutter::new(&mesh);
        let tool = Flat::new(2.0).unwrap();
        for m in path.moves() {
            if m.to.z > 7.9 {
                continue;
            }
            if let Some(limit) = dc.drop_at(&tool, m.to.x, m.to.y) {
                assert!(
                    m.to.z >= limit - 0.6,
                    "cut at {:?} is below the CL surface {limit}",
                    m.to
                );
            }
        }
    }

    /// Slivers cost a retract, a traverse and a plunge. Past some point that is not
    /// worth the entry, and the parameter must actually govern it.
    #[test]
    fn short_rings_are_dropped() {
        let mesh = synthetic::plane(0.0, 20.0);
        let cl = cl_for(&mesh, 3.0);
        let stock = full_stock(6.0);

        let keep = RoughingParams {
            min_ring_length: 0.0,
            ..params(6.0, 0.0)
        };
        let drop = RoughingParams {
            min_ring_length: 1.0e6,
            ..params(6.0, 0.0)
        };

        let a = zlevel_rough(&cl, &stock, &keep).unwrap().1;
        let b = zlevel_rough(&cl, &stock, &drop).unwrap().1;
        assert!(a.rings > 0);
        assert_eq!(b.rings, 0, "an enormous minimum should drop everything");
        assert!(b.rings_too_short > 0);
    }

    /// Every ring must close, or the cutter leaves a step where it stopped.
    #[test]
    fn rings_close() {
        let mesh = synthetic::plane(0.0, 20.0);
        let cl = cl_for(&mesh, 3.0);
        let (path, _) = zlevel_rough(&cl, &full_stock(6.0), &params(6.0, 0.0)).unwrap();

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
        let cl = cl_for(&mesh, 3.0);
        let stock = full_stock(10.0);
        let p = RoughingParams {
            stepdown: 3.0,
            ..params(10.0, -5.0)
        };
        let (path, _) = zlevel_rough(&cl, &stock, &p).unwrap();
        for m in path.moves() {
            assert!(m.to.z >= -5.0 - 1e-9, "cut below the bottom: {:?}", m.to);
        }
    }
}
