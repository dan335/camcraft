//! Flat finishing: each flat face of the part cleared at its own height, in rings
//! a stepover apart in from its edges, and nothing else. Fusion's Flat.
//!
//! # Where the tool rests on a flat
//!
//! The flat faces' heights are found from the part ([`crate::adaptive::flat_heights`]).
//! At a flat's height `z` the tool's tip may not be inside
//! [`LevelReach::forbidden`]`(z)`: that keeps it off every wall standing on the
//! flat, since a face lying exactly at the height does not stand in the way. A
//! little lower it may not be where the flat itself is under it. What lies
//! between the two is where the tool rests on the flat and touches nothing else,
//! with the stock to leave beside and below included in the tool.
//!
//! Each piece of that is cleared as a pocket clearing level is
//! ([`crate::pocket_clearing`]): rings a stepover apart in from its edges, so the
//! ring along the walls is exact. From the outside in, or from the middle out;
//! the highest flat first.

use camcraft_geom::contour::Region;
use glam::DVec2;

use crate::nesting::{Order, distance, nearest};
use crate::planar::pieces;
use crate::pocket_clearing::{Concentric, Cut};
use crate::reach::LevelReach;
use crate::strategy::{StrategyError, check};
use crate::toolpath::Toolpath;

/// How to lay out the passes.
#[derive(Debug, Clone, PartialEq)]
pub struct FlatParams {
    /// Distance between rings, in millimetres.
    pub stepover: f64,
    /// Chordal tolerance, in millimetres.
    pub tolerance: f64,
    /// Cutting feed, in mm/min.
    pub feed: f64,
    /// Heights of the flat faces, with the stock to leave down on them.
    pub flats: Vec<f64>,
    /// Whether the material cut is on the tool's right: climbing with a
    /// clockwise spindle.
    pub kept_on_right: bool,
    /// Which way round the rings are cut.
    pub order: Order,
    /// Stretches shorter than this are not worth the entry.
    pub min_ring_length: f64,
}

impl Default for FlatParams {
    fn default() -> Self {
        Self {
            stepover: 1.0,
            tolerance: 0.01,
            feed: 1000.0,
            flats: Vec::new(),
            kept_on_right: true,
            order: Order::OutsideIn,
            min_ring_length: 0.0,
        }
    }
}

/// What happened while laying out the passes.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FlatReport {
    /// Flat faces found at a height of their own.
    pub flats: u64,
    /// Of those, the ones with room for the tool.
    pub cleared: u64,
    /// Pieces of flat cleared.
    pub pieces: u64,
    /// Rings cut.
    pub passes: u64,
    /// Rings too short to be worth the entry.
    pub too_short: u64,
}

/// Flat passes over `area`, where the tool's centre may go.
///
/// The path holds [`crate::toolpath::MoveRole::Cut`] rings separated by
/// [`crate::toolpath::MoveRole::Link`] moves, the highest flat first. The linker
/// resolves them.
pub fn flat_finish(
    reach: &mut LevelReach<'_>,
    area: &Region,
    params: &FlatParams,
) -> Result<(Toolpath, FlatReport), StrategyError> {
    check("stepover", params.stepover)?;
    check("tolerance", params.tolerance)?;
    check("feed", params.feed)?;
    let mut report = FlatReport::default();
    // Below a flat by this much, the flat is under the tool: more than the
    // heights of one face's triangles differ by, and a small part of the
    // tolerance.
    let under = 0.5 * params.tolerance;
    let mut flats: Vec<f64> = params
        .flats
        .iter()
        .copied()
        .filter(|z| z.is_finite())
        .collect();
    flats.sort_by(|a, b| b.total_cmp(a));
    flats.dedup_by(|lower, higher| *higher - *lower < 2.0 * under);
    report.flats = flats.len() as u64;
    if area.is_empty() {
        return Ok((Toolpath::new(), report));
    }

    let rings = Concentric {
        stepover: params.stepover,
        // Drawn as finely as the regions are, so the ring along the walls is
        // the boundary itself.
        arc: reach.arc_tolerance(),
        min_length: params.min_ring_length,
        kept_on_right: params.kept_on_right,
        children_first: params.order == Order::InsideOut,
    };
    let mut path = Toolpath::new();
    let mut tool_at: Option<DVec2> = None;
    // Each flat's height and just under it, from the top down, all at once: the
    // shadows are built once.
    let heights: Vec<f64> = flats.iter().flat_map(|&z| [z, z - under]).collect();
    let regions = reach.forbidden_all(&heights)?;
    // Shrunk by what it may overstate, so a ring along an edge of the flat with
    // nothing beyond it still rests on it.
    let overstated = reach.band_step() + 2.0 * reach.arc_tolerance();
    for (z, pair) in flats.into_iter().zip(regions.chunks(2)) {
        let [clear, on] = pair else { continue };
        let on = on.offset(-overstated, reach.arc_tolerance());
        let rest = on.difference(clear)?.intersection(area)?;
        let mut trees = Vec::new();
        for piece in pieces(&rest) {
            let (tree, _) = rings.clear(&piece, None, &mut report.too_short)?;
            if tree.anchor(0).is_some() {
                trees.push(tree);
            }
        }
        if trees.is_empty() {
            continue;
        }
        report.cleared += 1;
        report.pieces += trees.len() as u64;
        // The nearest piece next, by where it is first cut.
        while !trees.is_empty() {
            let pick = nearest(&trees, |tree| distance(tree.anchor(0), tool_at));
            let mut cut = Cut {
                z,
                feed: params.feed,
                path: &mut path,
                at: tool_at,
                passes: 0,
            };
            trees.remove(pick).cut(0, &mut cut)?;
            tool_at = cut.at;
            report.passes += cut.passes;
        }
    }
    Ok((path, report))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::adaptive::flat_heights;
    use crate::allowance::{Allowance, EffectiveTool};
    use crate::dropcutter::DropCutter;
    use crate::tool::Flat;
    use crate::toolpath::MoveRole;
    use camcraft_geom::mesh::TriMesh;
    use camcraft_geom::synthetic;
    use glam::{DVec3, dvec2};

    const TOLERANCE: f64 = 0.02;

    /// A 40 mm square plate 2 mm thick with a 10 mm square boss 5 mm tall on it:
    /// two flats, the plate's top round the boss and the boss's top.
    fn stepped() -> TriMesh {
        let plate = synthetic::cuboid(DVec3::new(-20.0, -20.0, 0.0), DVec3::new(20.0, 20.0, 2.0));
        let boss = synthetic::cuboid(DVec3::new(-5.0, -5.0, 2.0), DVec3::new(5.0, 5.0, 7.0));
        TriMesh::merged(&[&plate, &boss]).unwrap()
    }

    fn flat(r: f64, radial: f64) -> EffectiveTool {
        EffectiveTool::new(&Flat::new(r).unwrap(), Allowance::new(radial, 0.0).unwrap()).unwrap()
    }

    fn run(mesh: &TriMesh, tool: &EffectiveTool, params: &FlatParams) -> (Toolpath, FlatReport) {
        let mut reach = LevelReach::new(mesh, tool, TOLERANCE).unwrap();
        let area = Region::rectangle(dvec2(-20.0, -20.0), dvec2(20.0, 20.0));
        flat_finish(&mut reach, &area, params).unwrap()
    }

    fn params(mesh: &TriMesh) -> FlatParams {
        FlatParams {
            stepover: 2.0,
            tolerance: TOLERANCE,
            flats: flat_heights(mesh, TOLERANCE),
            ..Default::default()
        }
    }

    #[test]
    fn each_flat_is_cleared_at_its_height_and_nothing_else() {
        let mesh = stepped();
        let tool = flat(2.0, 0.0);
        let (path, report) = run(&mesh, &tool, &params(&mesh));
        assert_eq!(report.flats, 2, "{report:?}");
        assert_eq!(report.cleared, 2, "{report:?}");
        let cutter = DropCutter::new(&mesh);
        let mut heights: Vec<f64> = Vec::new();
        for m in path.moves() {
            // Every point where the tool rests on a flat, at its height.
            let h = cutter.drop_at(&tool, m.to.x, m.to.y).unwrap();
            assert!((h - m.to.z).abs() < 1e-6, "{} is {h} under the tool", m.to);
            if !heights.iter().any(|z| (z - m.to.z).abs() < 1e-9) {
                heights.push(m.to.z);
            }
        }
        heights.sort_by(f64::total_cmp);
        assert_eq!(heights, vec![2.0, 7.0]);
    }

    #[test]
    fn the_rings_keep_the_stock_to_leave_off_the_walls() {
        let mesh = stepped();
        let tool = flat(2.0, 0.5);
        let (path, _) = run(&mesh, &tool, &params(&mesh));
        for m in path.moves().iter().filter(|m| m.to.z < 3.0) {
            // Round the boss: the tool's side at least 0.5 mm off it.
            let off = (dvec2(m.to.x.abs(), m.to.y.abs()) - dvec2(5.0, 5.0)).max(DVec2::ZERO);
            let d = if off.x > 0.0 && off.y > 0.0 {
                off.length()
            } else {
                off.x.max(off.y)
            };
            assert!(d >= 2.5 - TOLERANCE, "{} is {d} off the boss", m.to);
        }
    }

    #[test]
    fn outside_in_ends_on_the_wall_and_inside_out_starts_there() {
        let mesh = stepped();
        let tool = flat(2.0, 0.0);
        let first_ring_on_top = |order: Order| {
            let (path, _) = run(
                &mesh,
                &tool,
                &FlatParams {
                    order,
                    ..params(&mesh)
                },
            );
            // The first ring cut on the boss's top: its distance from the middle.
            path.moves()
                .iter()
                .find(|m| m.role == MoveRole::Link && m.to.z > 6.0)
                .map(|m| m.to.truncate().abs().max_element())
                .unwrap()
        };
        let outside_in = first_ring_on_top(Order::OutsideIn);
        let inside_out = first_ring_on_top(Order::InsideOut);
        assert!(outside_in > inside_out, "{outside_in} {inside_out}");
    }

    #[test]
    fn a_part_with_no_flat_has_nothing_to_cut() {
        let mesh = synthetic::uv_sphere(10.0, 30, 45);
        let tool = flat(2.0, 0.0);
        let (path, report) = run(&mesh, &tool, &params(&mesh));
        assert!(path.moves().is_empty(), "{report:?}");
    }
}
