use super::*;
use crate::cutter::allowance::Allowance;
use crate::cutter::dropcutter::DropCutter;
use crate::cutter::tool::{Ball, Bullnose, Flat};
use camcraft_geom::synthetic;
use glam::dvec2;

#[test]
fn a_flat_mill_is_one_band() {
    let b = bands(&Flat::new(3.0).unwrap(), 0.025).unwrap();
    assert_eq!(
        b,
        vec![Band {
            height: 0.0,
            radius: 3.0
        }]
    );
}

/// Every band's radius is at least the cutter's anywhere in it, and at most a
/// step more than the least.
#[test]
fn bands_bound_the_profile_from_above_within_a_step() {
    let step = 0.05;
    let tools: Vec<Box<dyn ToolProfile>> = vec![
        Box::new(Ball::new(3.0).unwrap()),
        Box::new(Bullnose::new(3.5, 0.5).unwrap()),
        Box::new(Bullnose::new(4.0, 1.7).unwrap()),
    ];
    for tool in &tools {
        let b = bands(tool.as_ref(), step).unwrap();
        for (k, band) in b.iter().enumerate() {
            let top = b.get(k + 1).map_or(band.height + 10.0, |n| n.height);
            for i in 0..=200 {
                let h = band.height + (top - band.height) * f64::from(i) / 200.0;
                // The top of each band belongs to the next.
                if i == 200 && k + 1 < b.len() {
                    continue;
                }
                let r = tool.radius_at(h);
                assert!(
                    r <= band.radius + 1e-12,
                    "band {k} at {h}: {r} > {}",
                    band.radius
                );
                assert!(
                    band.radius - r <= step + 1e-9,
                    "band {k} at {h}: {r} vs {}",
                    band.radius
                );
            }
        }
    }
}

/// The oracle: the forbidden region is where the drop-cutter's answer is above
/// the level, to within the band step and the arc padding, and never short of it.
fn agrees_with_drop_cutter(mesh: &TriMesh, tool: &EffectiveTool, levels: &[f64], half: f64) {
    let tolerance = 0.05;
    let mut reach = LevelReach::new(mesh, tool, tolerance).unwrap();
    let dc = DropCutter::new(mesh);
    let slack = reach.overstates() + 0.002;
    let n = 61;
    for &z in levels {
        let f = reach.forbidden(z).unwrap();
        let core = f.offset(-slack, reach.arc_tolerance());
        let mut forbidden_seen = 0;
        for i in 0..n {
            for j in 0..n {
                // Off the lattice the region is aligned with, deliberately.
                let x = -half + 2.0 * half * (f64::from(i) + 0.37) / f64::from(n);
                let y = -half + 2.0 * half * (f64::from(j) + 0.61) / f64::from(n);
                let p = dvec2(x, y);
                let zcl = dc.drop_at(tool, x, y);
                if zcl.is_some_and(|h| h > z + 1e-6) {
                    forbidden_seen += 1;
                    assert!(
                        f.contains(p),
                        "z {z}: ({x:.3}, {y:.3}) has Zcl {zcl:?} but is allowed"
                    );
                }
                if core.contains(p) {
                    assert!(
                        zcl.is_some_and(|h| h > z - 1e-6),
                        "z {z}: ({x:.3}, {y:.3}) is forbidden by more than the \
                             slack but Zcl is {zcl:?}"
                    );
                }
            }
        }
        assert!(forbidden_seen > 0, "z {z}: the test saw nothing forbidden");
    }
}

#[test]
fn a_flat_mill_on_a_sphere_agrees_with_the_drop_cutter() {
    let mesh = synthetic::uv_sphere(8.0, 60, 90);
    let tool = EffectiveTool::new(&Flat::new(2.0).unwrap(), Allowance::none()).unwrap();
    agrees_with_drop_cutter(&mesh, &tool, &[6.0, 2.0, -1.0], 12.0);
}

#[test]
fn a_ball_on_a_sphere_agrees_with_the_drop_cutter() {
    let mesh = synthetic::uv_sphere(8.0, 60, 90);
    let tool = EffectiveTool::new(&Ball::new(2.0).unwrap(), Allowance::none()).unwrap();
    agrees_with_drop_cutter(&mesh, &tool, &[6.0, 2.0, -3.0], 12.0);
}

/// Stock to leave makes a bullnose of a flat mill and lifts the tip; both must
/// be read the way the drop-cutter reads them.
#[test]
fn stock_to_leave_agrees_with_the_drop_cutter() {
    let mesh = synthetic::uv_sphere(8.0, 60, 90);
    let tool =
        EffectiveTool::new(&Flat::new(2.0).unwrap(), Allowance::new(0.6, 0.3).unwrap()).unwrap();
    agrees_with_drop_cutter(&mesh, &tool, &[6.0, 2.0, -3.0], 12.0);
}

#[test]
fn a_step_agrees_with_the_drop_cutter() {
    let mesh = synthetic::step(8.0, 10.0);
    for tool in [
        EffectiveTool::new(&Flat::new(2.0).unwrap(), Allowance::none()).unwrap(),
        EffectiveTool::new(
            &Bullnose::new(2.0, 0.5).unwrap(),
            Allowance::isotropic(0.5).unwrap(),
        )
        .unwrap(),
    ] {
        agrees_with_drop_cutter(&mesh, &tool, &[7.0, 3.0], 9.0);
    }
}

#[test]
fn all_levels_at_once_are_each_level_alone() {
    let mesh = synthetic::uv_sphere(10.0, 40, 60);
    let tool = EffectiveTool::new(&Ball::new(3.0).unwrap(), Allowance::none()).unwrap();
    // A step smaller than the ball's corner, so level by level the shadows
    // would start again.
    let levels = [9.0, 8.0, 7.0, 7.0, 4.5, 1.0];
    let all = LevelReach::new(&mesh, &tool, 0.05)
        .unwrap()
        .forbidden_all(&levels)
        .unwrap();
    let mut one = LevelReach::new(&mesh, &tool, 0.05).unwrap();
    for (&z, region) in levels.iter().zip(&all) {
        let alone = one.forbidden(z).unwrap();
        let apart =
            alone.difference(region).unwrap().area() + region.difference(&alone).unwrap().area();
        assert!(apart < 1e-6, "level {z}: {apart} mm² apart");
    }
}

#[test]
fn nonsense_tolerances_are_refused() {
    let mesh = synthetic::plane(0.0, 5.0);
    let tool = EffectiveTool::new(&Flat::new(2.0).unwrap(), Allowance::none()).unwrap();
    for bad in [0.0, -1.0, f64::NAN] {
        assert!(LevelReach::new(&mesh, &tool, bad).is_err());
    }
}
