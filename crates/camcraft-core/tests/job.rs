//! A two-operation job: rough, then finish, with stock threaded between them.
//!
//! This is what 0.1-c is actually for. Roughing reads the block and writes back what
//! it removed; finishing reads *that*, so it never cuts air the roughing pass
//! already cleared. Rest machining is not implemented anywhere — it is what happens
//! when the second operation reads the first one's stock.

#![allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]

use camcraft_core::allowance::{Allowance, EffectiveTool};
use camcraft_core::dropcutter::DropCutter;
use camcraft_core::linker::{ClearanceField, LinkParams, NoPart, Unchanging, link};
use camcraft_core::ordering::order_passes;
use camcraft_core::parallel::{ParallelParams, parallel_finish};
use camcraft_core::reach::LevelReach;
use camcraft_core::roughing::{RoughingParams, zlevel_rough};
use camcraft_core::simadapt::sim_moves;
use camcraft_core::tool::{Ball, Flat, ToolProfile};
use camcraft_geom::contour::Region;
use camcraft_geom::synthetic;
use camcraft_sim::stock::{PartSurface, Stock};
use glam::dvec2;

const PITCH: f64 = 0.4;

#[test]
fn rough_then_finish_threading_stock_between_them() {
    // A dome standing proud of a flat base, so roughing has real material to clear
    // and finishing has a curved surface to follow.
    let sphere_r = 10.0;
    let mesh = synthetic::uv_sphere(sphere_r, 140, 200);
    let cutter = DropCutter::new(&mesh);

    let (lo, hi) = (dvec2(-11.0, -11.0), dvec2(11.0, 11.0));
    let block_top = sphere_r + 2.0;
    let block_floor = -sphere_r - 1.0;

    // One block of material, carried through both operations.
    let mut stock = Stock::block(lo, hi, block_top, block_floor, PITCH).unwrap();
    let started_with = stock.volume();

    // ---- operation 1: rough with a big flat mill, leaving stock ----------------
    let rough_tool =
        EffectiveTool::new(&Flat::new(3.0).unwrap(), Allowance::isotropic(0.6).unwrap()).unwrap();
    let mut rough_reach = LevelReach::new(&mesh, &rough_tool, 0.05).unwrap();

    let (rough_raw, rough_report) = zlevel_rough(
        &mut rough_reach,
        &Region::rectangle(lo, hi),
        stock.field(),
        &RoughingParams {
            stepdown: 2.0,
            stepover: 2.0,
            top: block_top,
            bottom: 0.0,
            feed: 1500.0,
            min_ring_length: 3.0,
        },
    )
    .unwrap();
    assert!(
        rough_report.rings > 0,
        "roughing found nothing: {rough_report:?}"
    );

    let (rough_ordered, order) = order_passes(&rough_raw, true).unwrap();
    assert!(
        order.travel_after_mm <= order.travel_before_mm,
        "ordering must not make travel worse"
    );

    // Clearance from the material itself rather than a fixed plane.
    let clearance = ClearanceField::new(stock.field().clone(), 4.0, block_top + 4.0).unwrap();
    let linking = LinkParams::plain(1500.0, 300.0, rough_tool.max_radius());
    let mut before = Unchanging(stock.field().clone());
    let (rough_path, _) = link(&rough_ordered, &clearance, &mut before, &NoPart, &linking).unwrap();
    assert!(!rough_path.has_unplanned_links());

    let rough_sim = stock.apply(&sim_moves(&rough_path), &rough_tool);
    assert!(
        rough_sim.is_clean(),
        "roughing rapid touched stock: {} contact(s)",
        rough_sim.rapid_contacts
    );
    assert!(rough_sim.removed_mm3 > 0.0, "roughing removed nothing");

    let after_roughing = stock.volume();
    assert!(
        after_roughing < started_with,
        "the block should be smaller after roughing"
    );

    // Roughing left its allowance: the dome is not yet at size.
    let part = PartSurface::from_mesh(&mesh, lo, hi, PITCH).unwrap();
    let apex_stock = stock.height_at(0.0, 0.0).unwrap();
    let apex_part = part.height_at(0.0, 0.0).unwrap();
    assert!(
        apex_stock > apex_part,
        "roughing must leave material on the part: stock {apex_stock}, part {apex_part}"
    );

    // ---- operation 2: finish, reading the stock roughing left ------------------
    let finish_tool = EffectiveTool::new(&Ball::new(2.0).unwrap(), Allowance::none()).unwrap();
    let (finish_raw, finish_report) = parallel_finish(
        &cutter,
        &finish_tool,
        dvec2(-7.0, -7.0),
        dvec2(7.0, 7.0),
        &ParallelParams {
            stepover: 0.6,
            tolerance: 0.02,
            feed: 1800.0,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(finish_report.is_clean());

    let (finish_ordered, _) = order_passes(&finish_raw, false).unwrap();
    let finish_clearance =
        ClearanceField::new(stock.field().clone(), 4.0, block_top + 4.0).unwrap();
    let (finish_path, _) = link(
        &finish_ordered,
        &finish_clearance,
        &mut Unchanging(stock.field().clone()),
        &NoPart,
        &LinkParams::plain(1800.0, 300.0, finish_tool.max_radius()),
    )
    .unwrap();

    let finish_sim = stock.apply(&sim_moves(&finish_path), &finish_tool);
    assert!(
        finish_sim.is_clean(),
        "finishing rapid touched stock: {} contact(s), deepest {:.3} mm",
        finish_sim.rapid_contacts,
        finish_sim.deepest_rapid_contact_mm
    );

    // ---- the result is the part -------------------------------------------------
    let budget = 0.02 + synthetic::uv_sphere_max_sag(sphere_r, 140, 200) + PITCH;
    let v = stock.verify(&part, budget, 1.0);
    assert!(
        v.compared > 1000,
        "expected coverage, compared {}",
        v.compared
    );
    assert!(
        v.is_clean(),
        "gouged {} column(s), deepest {:.3} mm against {:.3} mm",
        v.gouged,
        v.deepest_gouge_mm,
        budget
    );

    // And finishing actually brought the apex down onto the part.
    let final_apex = stock.height_at(0.0, 0.0).unwrap();
    assert!(
        (final_apex - apex_part).abs() < budget,
        "finishing should reach the part: stock {final_apex}, part {apex_part}"
    );
}

/// Threading stock is only worth anything if the second operation can see what the
/// first removed.
///
/// "Nothing the second time" would be the wrong claim. Roughing leaves scallops
/// between its rings and the stock grid has a finite pitch, so real material does
/// remain — and the second pass correctly goes after it rather than redoing the
/// first pass. That reduction *is* rest machining, and it is what this measures.
#[test]
fn a_second_roughing_pass_only_targets_the_leftovers() {
    let mesh = synthetic::plane(0.0, 24.0);
    let (lo, hi) = (dvec2(-10.0, -10.0), dvec2(10.0, 10.0));

    let tool = EffectiveTool::new(&Flat::new(3.0).unwrap(), Allowance::none()).unwrap();
    let mut reach = LevelReach::new(&mesh, &tool, 0.05).unwrap();
    let params = RoughingParams {
        stepdown: 2.0,
        stepover: 2.5,
        top: 8.0,
        bottom: 0.0,
        feed: 1500.0,
        min_ring_length: 3.0,
    };

    let mut stock = Stock::block(lo, hi, 8.0, -2.0, PITCH).unwrap();
    let (first, first_report) = zlevel_rough(
        &mut reach,
        &Region::rectangle(lo, hi),
        stock.field(),
        &params,
    )
    .unwrap();
    assert!(first_report.rings > 0, "the first pass must find material");

    let clearance = ClearanceField::new(stock.field().clone(), 4.0, 12.0).unwrap();
    let (linked, _) = link(
        &first,
        &clearance,
        &mut Unchanging(stock.field().clone()),
        &NoPart,
        &LinkParams::plain(1500.0, 300.0, tool.max_radius()),
    )
    .unwrap();
    stock.apply(&sim_moves(&linked), &tool);

    let (_, second_report) = zlevel_rough(
        &mut reach,
        &Region::rectangle(lo, hi),
        stock.field(),
        &params,
    )
    .unwrap();
    assert!(
        second_report.rings * 2 < first_report.rings,
        "a second pass should target only the leftovers, not redo the first: \
         first {} rings, second {}",
        first_report.rings,
        second_report.rings
    );
    assert!(
        second_report.empty_levels > 0,
        "levels the first pass finished should now be empty"
    );
}
