//! End to end: mesh -> strategy -> linker -> machinable path.
//!
//! The unit tests check each stage against its own contract. This checks that the
//! stages actually compose, through the public API only, which is the thing a caller
//! depends on and the thing a refactor is most likely to break quietly.

#![allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]

use camcraft_core::cutter::allowance::{Allowance, EffectiveTool};
use camcraft_core::cutter::dropcutter::DropCutter;
use camcraft_core::cutter::tool::{Ball, ToolProfile};
use camcraft_core::path::linker::{Air, ClearancePlane, LinkParams, NoPart, link};
use camcraft_core::path::toolpath::MoveRole;
use camcraft_core::strategy::parallel::{ParallelParams, parallel_finish};
use camcraft_geom::synthetic;
use glam::dvec2;

const SPHERE_R: f64 = 10.0;
const LAT: u32 = 160;
const LON: u32 = 220;
const CLEARANCE: f64 = 25.0;

#[test]
fn a_sphere_becomes_a_machinable_path() {
    let mesh = synthetic::uv_sphere(SPHERE_R, LAT, LON);
    let cutter = DropCutter::new(&mesh);

    // A finishing pass that still leaves a little stock, so the effective-tool seam
    // is exercised rather than bypassed.
    let stock = Allowance::isotropic(0.2).unwrap();
    let tool = EffectiveTool::new(&Ball::new(2.0).unwrap(), stock).unwrap();

    let sweep = ParallelParams {
        stepover: 0.8,
        tolerance: 0.01,
        feed: 1200.0,
        ..Default::default()
    };
    let (raw, strategy) =
        parallel_finish(&cutter, &tool, dvec2(-6.0, -6.0), dvec2(6.0, 6.0), &sweep).unwrap();

    assert!(strategy.is_clean(), "refinement was limited: {strategy:?}");
    assert!(strategy.passes > 5, "expected several passes");
    assert!(
        raw.has_unplanned_links(),
        "a strategy must hand over an unlinked path"
    );

    let plane = ClearancePlane::new(CLEARANCE).unwrap();
    let linking = LinkParams::plain(900.0, 250.0, tool.max_radius());
    let (path, links) = link(&raw, &plane, &mut Air, &NoPart, &linking).unwrap();

    // The property that makes it machinable at all.
    assert!(!path.has_unplanned_links());
    assert_eq!(links.links_resolved, strategy.passes);
    assert_eq!(
        links.suspicious_plunges, 0,
        "clearance well above the part should give clean plunges"
    );

    // Every cutting point still sits on the exact surface the tool was asked for,
    // stock included. Linking must not have nudged the geometry.
    let mut cuts = 0;
    for m in path.moves().iter().filter(|m| m.role == MoveRole::Cut) {
        let exact = cutter.drop_at(&tool, m.to.x, m.to.y).unwrap();
        assert!(
            (m.to.z - exact).abs() < 1e-9,
            "cut point {:?} drifted from the exact surface {exact}",
            m.to
        );
        cuts += 1;
    }
    assert!(cuts > 100, "expected real cutting coverage, got {cuts}");

    // And the stock really is left behind: the tool sits above the bare part.
    let bare = Ball::new(2.0).unwrap();
    let apex_with_stock = cutter.drop_at(&tool, 0.0, 0.0).unwrap();
    let apex_bare = cutter.drop_at(&bare, 0.0, 0.0).unwrap();
    assert!(
        (apex_with_stock - apex_bare - 0.2).abs() < 1e-6,
        "expected 0.2 mm of stock at the apex, got {}",
        apex_with_stock - apex_bare
    );
}

/// The safety property the whole linking stage exists for: nothing traverses
/// through the part.
#[test]
fn no_traverse_passes_below_clearance() {
    let mesh = synthetic::uv_sphere(SPHERE_R, 80, 120);
    let cutter = DropCutter::new(&mesh);
    let tool = EffectiveTool::new(&Ball::new(2.0).unwrap(), Allowance::none()).unwrap();

    let (raw, _) = parallel_finish(
        &cutter,
        &tool,
        dvec2(-7.0, -7.0),
        dvec2(7.0, 7.0),
        &ParallelParams {
            stepover: 1.2,
            ..Default::default()
        },
    )
    .unwrap();

    let plane = ClearancePlane::new(CLEARANCE).unwrap();
    let (path, _) = link(
        &raw,
        &plane,
        &mut Air,
        &NoPart,
        &LinkParams::plain(900.0, 250.0, tool.max_radius()),
    )
    .unwrap();

    // Across only at clearance; below it, a rapid only goes straight down to the
    // safe distance above the next cut.
    let mut rapids = 0;
    let mut at = path.moves().first().map_or(glam::DVec3::ZERO, |m| m.to);
    for m in path.moves() {
        if m.role == MoveRole::Rapid {
            let across = (m.to.truncate() - at.truncate()).length() > 1e-9;
            if across {
                assert!(
                    m.to.z >= CLEARANCE - 1e-9 && (rapids == 0 || at.z >= CLEARANCE - 1e-9),
                    "rapid across from {at:?} to {:?} is below clearance {CLEARANCE}",
                    m.to
                );
            }
            rapids += 1;
        }
        at = m.to;
    }
    assert!(rapids > 0, "expected traverses between passes");

    // The part never reaches the clearance plane, so no rapid can be inside it.
    let peak = mesh.vertices().iter().fold(f64::MIN, |hi, v| hi.max(v.z));
    assert!(
        peak < CLEARANCE,
        "fixture must keep the part below clearance"
    );
}

/// A flat plate should need no refinement and produce a path whose every point is
/// the same height — the degenerate case that catches sign errors and drift.
#[test]
fn a_flat_plate_gives_a_flat_path() {
    let mesh = synthetic::plane(3.0, 30.0);
    let cutter = DropCutter::new(&mesh);
    let tool =
        EffectiveTool::new(&Ball::new(3.0).unwrap(), Allowance::new(0.0, 0.5).unwrap()).unwrap();

    let (raw, report) = parallel_finish(
        &cutter,
        &tool,
        dvec2(-10.0, -10.0),
        dvec2(10.0, 10.0),
        &ParallelParams {
            stepover: 2.0,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(report.is_clean());

    let plane = ClearancePlane::new(CLEARANCE).unwrap();
    let (path, _) = link(
        &raw,
        &plane,
        &mut Air,
        &NoPart,
        &LinkParams::plain(900.0, 250.0, tool.max_radius()),
    )
    .unwrap();

    for m in path.moves().iter().filter(|m| m.role == MoveRole::Cut) {
        assert!(
            (m.to.z - 3.5).abs() < 1e-9,
            "cut at {:?} should sit 0.5 mm above the plate",
            m.to
        );
    }
}
