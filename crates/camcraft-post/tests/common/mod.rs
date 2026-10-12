//! What the post's integration tests share: one path exercising every move
//! role and every kind of arc, the program it is posted as, and three holes.

#![allow(dead_code, clippy::unwrap_used)]

use camcraft_core::job::{Coolant, SpindleDirection};
use camcraft_core::path::toolpath::Arc;
use camcraft_core::path::toolpath::{MoveRole, Toolpath};
use camcraft_core::strategy::drilling::Hole;
use camcraft_post::op::{DrillCycles, ProgramSpec};
use camcraft_post::profile::ProgramUnits;
use glam::{DVec3, dvec2, dvec3};

/// A path exercising every move role the engine currently produces, plus the
/// coordinate cases that formatting gets wrong: negatives, a value that rounds, and
/// a move that changes only one axis.
pub fn fixture() -> Toolpath {
    let mut t = Toolpath::new();
    t.rapid_to(dvec3(0.0, 0.0, 25.0)).unwrap();
    t.rapid_to(dvec3(-12.3456, 7.5, 25.0)).unwrap();
    t.feed_to(dvec3(-12.3456, 7.5, 1.0), MoveRole::Plunge, 250.0)
        .unwrap();
    t.feed_to(dvec3(-12.3456, 7.5, -0.0001), MoveRole::Plunge, 250.0)
        .unwrap();
    t.feed_to(dvec3(10.0, 7.5, -0.0001), MoveRole::Cut, 1200.0)
        .unwrap();
    t.feed_to(dvec3(10.0, 9.5, -0.0001), MoveRole::Cut, 1200.0)
        .unwrap();
    // A quarter turn round a corner, level; a full turn of helix down; and a
    // quarter turn in a vertical plane, as a lead.
    t.arc_to(
        dvec3(12.0, 11.5, -0.0001),
        Arc {
            centre: dvec3(12.0, 9.5, -0.0001),
            axis: DVec3::Z,
            sweep: -std::f64::consts::FRAC_PI_2,
        },
        MoveRole::Cut,
        1200.0,
    )
    .unwrap();
    t.arc_to(
        dvec3(12.0, 11.5, -1.0),
        Arc {
            centre: dvec3(12.0, 9.5, 0.0),
            axis: DVec3::Z,
            sweep: std::f64::consts::TAU,
        },
        MoveRole::Ramp,
        400.0,
    )
    .unwrap();
    t.arc_to(
        dvec3(13.0, 11.5, -2.0),
        Arc {
            centre: dvec3(13.0, 11.5, -1.0),
            axis: DVec3::Y,
            sweep: -std::f64::consts::FRAC_PI_2,
        },
        MoveRole::Lead,
        1200.0,
    )
    .unwrap();
    t.feed_to(dvec3(10.0, 9.5, -2.5), MoveRole::Cut, 800.0)
        .unwrap();
    t.feed_to(dvec3(10.0, 9.5, 25.0), MoveRole::Retract, 900.0)
        .unwrap();
    t
}

pub fn spec() -> ProgramSpec {
    ProgramSpec {
        op_name: "finish".into(),
        strategy: "parallel".into(),
        tool: 3,
        tool_comment: "6mm ball (carbide)".into(),
        wcs: 54,
        rpm: 18000,
        spin: SpindleDirection::Clockwise,
        coolant: Coolant::Flood,
        units: ProgramUnits::Mm,
        drill: None,
    }
}

/// Three holes, drilled as canned cycles on a controller that takes them: across
/// to the first at the clearance height, one line a hole returning there, then
/// G80. The path is ignored when the holes are given.
pub fn drilling(peck: Option<f64>, dwell: Option<f64>) -> ProgramSpec {
    let hole = |x: f64, y: f64| Hole {
        at: dvec2(x, y),
        top: 5.0,
        bottom: -1.5,
        radius: 3.0,
    };
    ProgramSpec {
        op_name: "holes".into(),
        strategy: "drill".into(),
        tool: 4,
        tool_comment: "6mm drill".into(),
        coolant: Coolant::Off,
        drill: Some(DrillCycles {
            holes: vec![hole(10.0, 10.0), hole(30.0, 10.0), hole(-12.345, 40.0)],
            peck,
            dwell,
            feed: 250.0,
            clearance: 20.0,
        }),
        ..spec()
    }
}
