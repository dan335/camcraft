//! The simulator as oracle: mesh -> strategy -> linker -> simulate -> verify.
//!
//! This is the test that can fail when everything else passes. Every other check in
//! the project reasons about the path; this one reasons about the *material*, by
//! removing it and looking at what is left. It is the only thing that can answer
//! "did a rapid touch stock" against real geometry rather than against a plane.

#![allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]

use camcraft_core::allowance::{Allowance, EffectiveTool};
use camcraft_core::dropcutter::DropCutter;
use camcraft_core::linker::{ClearancePlane, LinkParams, link};
use camcraft_core::parallel::{ParallelParams, parallel_finish};
use camcraft_core::simadapt::sim_moves;
use camcraft_core::tool::{Ball, ToolProfile};
use camcraft_core::toolpath::{MoveRole, Toolpath};
use camcraft_geom::synthetic;
use camcraft_sim::stock::{MoveClass, PartSurface, SimMove, Stock};
use glam::{dvec2, dvec3};

const PITCH: f64 = 0.25;

/// Machine a dome and confirm the result is the dome, not a hole in it.
#[test]
fn a_finished_dome_matches_the_part() {
    let sphere_r = 12.0;
    let mesh = synthetic::uv_sphere(sphere_r, 160, 220);
    let cutter = DropCutter::new(&mesh);
    let tool = EffectiveTool::new(&Ball::new(2.0).unwrap(), Allowance::none()).unwrap();

    let (lo, hi) = (dvec2(-7.0, -7.0), dvec2(7.0, 7.0));
    let (raw, strategy) = parallel_finish(
        &cutter,
        &tool,
        lo,
        hi,
        &ParallelParams {
            stepover: 0.5,
            tolerance: 0.02,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(strategy.is_clean());

    let top = sphere_r + 5.0;
    let (path, _) = link(
        &raw,
        &ClearancePlane::new(top).unwrap(),
        &LinkParams {
            retract_feed: 900.0,
            plunge_feed: 250.0,
            tool_radius: tool.max_radius(),
        },
    )
    .unwrap();

    let mut stock = Stock::block(lo, hi, top, -sphere_r - 1.0, PITCH).unwrap();
    let sim = stock.apply(&sim_moves(&path), &tool);

    // The check the plan singles out, now against real material rather than a plane.
    assert!(
        sim.is_clean(),
        "a rapid touched stock: {} contact(s), deepest {:.3} mm",
        sim.rapid_contacts,
        sim.deepest_rapid_contact_mm
    );
    assert!(sim.removed_mm3 > 0.0, "nothing was cut");

    let part = PartSurface::from_mesh(&mesh, lo, hi, PITCH).unwrap();
    // Allow the chordal tolerance, the tessellation sag and one stock cell: the
    // simulator resolves the surface on a grid, so it cannot be held to better.
    let budget = 0.02 + synthetic::uv_sphere_max_sag(sphere_r, 160, 220) + PITCH;
    let v = stock.verify(&part, budget, 1.0);

    assert!(
        v.compared > 1000,
        "expected real coverage, compared {}",
        v.compared
    );
    assert!(
        v.is_clean(),
        "gouged {} column(s), deepest {:.3} mm against a {:.3} mm budget",
        v.gouged,
        v.deepest_gouge_mm,
        budget
    );
}

/// The oracle has to be able to fail, or it proves nothing. Drive the same tool
/// below the part on purpose and confirm it is caught.
#[test]
fn a_deliberate_gouge_is_detected() {
    let mesh = synthetic::plane(0.0, 14.0);
    let (lo, hi) = (dvec2(-6.0, -6.0), dvec2(6.0, 6.0));
    let part = PartSurface::from_mesh(&mesh, lo, hi, PITCH).unwrap();
    let tool = EffectiveTool::new(&Ball::new(2.0).unwrap(), Allowance::none()).unwrap();

    let mut stock = Stock::block(lo, hi, 5.0, -5.0, PITCH).unwrap();
    let mut moves = Vec::new();
    let mut y = -5.0;
    while y <= 5.0 {
        // One millimetre too deep, everywhere.
        moves.push(SimMove {
            to: dvec3(-5.0, y, -1.0),
            class: MoveClass::MayCut,
        });
        moves.push(SimMove {
            to: dvec3(5.0, y, -1.0),
            class: MoveClass::MayCut,
        });
        y += 0.5;
    }
    stock.apply(&moves, &tool);

    let v = stock.verify(&part, 0.05, 1.0);
    assert!(!v.is_clean(), "cutting 1 mm under the part must be caught");
    assert!(
        (v.deepest_gouge_mm - 1.0).abs() < 0.1,
        "expected about 1 mm, got {}",
        v.deepest_gouge_mm
    );
}

/// And the rapid check has to be able to fail too.
#[test]
fn a_rapid_through_the_part_is_detected() {
    let (lo, hi) = (dvec2(-6.0, -6.0), dvec2(6.0, 6.0));
    let tool = EffectiveTool::new(&Ball::new(2.0).unwrap(), Allowance::none()).unwrap();
    let mut stock = Stock::block(lo, hi, 5.0, -5.0, PITCH).unwrap();

    let mut t = Toolpath::new();
    t.rapid_to(dvec3(-5.0, 0.0, 0.0)).unwrap();
    t.rapid_to(dvec3(5.0, 0.0, 0.0)).unwrap();
    let sim = stock.apply(&sim_moves(&t), &tool);

    assert!(
        !sim.is_clean(),
        "a rapid at Z0 through a block topped at Z5 must be caught"
    );
    assert!(sim.deepest_rapid_contact_mm > 2.0);
}

/// Leaving stock must actually leave it, measured in material rather than inferred
/// from the tool definition.
#[test]
fn stock_to_leave_shows_up_in_the_material() {
    let mesh = synthetic::plane(0.0, 14.0);
    let (lo, hi) = (dvec2(-5.0, -5.0), dvec2(5.0, 5.0));
    let part = PartSurface::from_mesh(&mesh, lo, hi, PITCH).unwrap();
    let cutter = DropCutter::new(&mesh);

    let leave = 0.4;
    let tool = EffectiveTool::new(
        &Ball::new(2.0).unwrap(),
        Allowance::isotropic(leave).unwrap(),
    )
    .unwrap();
    let (raw, _) = parallel_finish(
        &cutter,
        &tool,
        lo,
        hi,
        &ParallelParams {
            stepover: 0.5,
            ..Default::default()
        },
    )
    .unwrap();
    let (path, _) = link(
        &raw,
        &ClearancePlane::new(10.0).unwrap(),
        &LinkParams {
            retract_feed: 900.0,
            plunge_feed: 250.0,
            tool_radius: tool.max_radius(),
        },
    )
    .unwrap();

    let mut stock = Stock::block(lo, hi, 5.0, -5.0, PITCH).unwrap();
    stock.apply(&sim_moves(&path), &tool);

    // Under the middle of the pass, the floor should sit one stock allowance above
    // the part -- not on it.
    let floor = stock.height_at(0.0, 0.0).unwrap();
    let target = part.height_at(0.0, 0.0).unwrap();
    assert!(
        (floor - target - leave).abs() < PITCH,
        "expected {leave} mm left on the part, floor {floor} vs part {target}"
    );

    // And the finishing tolerance must not call that a gouge.
    let v = stock.verify(&part, 0.05, 1.0);
    assert!(v.is_clean(), "leaving stock is not gouging: {v:?}");
}

/// The path the strategy hands over is not machinable, and the simulator says so
/// rather than quietly cutting the traverses nobody planned.
#[test]
fn an_unlinked_path_reports_its_unplanned_traverses() {
    let mesh = synthetic::uv_sphere(10.0, 80, 120);
    let cutter = DropCutter::new(&mesh);
    let tool = EffectiveTool::new(&Ball::new(2.0).unwrap(), Allowance::none()).unwrap();
    let (lo, hi) = (dvec2(-6.0, -6.0), dvec2(6.0, 6.0));

    let (raw, _) = parallel_finish(
        &cutter,
        &tool,
        lo,
        hi,
        &ParallelParams {
            stepover: 1.0,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(raw.has_unplanned_links());
    assert!(raw.moves().iter().any(|m| m.role == MoveRole::Link));

    let mut stock = Stock::block(lo, hi, 15.0, -11.0, PITCH).unwrap();
    let sim = stock.apply(&sim_moves(&raw), &tool);
    assert!(
        !sim.is_clean(),
        "an unlinked path drags the tool between passes and must not look clean"
    );
}
