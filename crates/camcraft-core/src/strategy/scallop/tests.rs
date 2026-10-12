use super::*;
use crate::cutter::tool::{Ball, Flat};
use crate::path::toolpath::MoveRole;
use camcraft_geom::contour::Contour;
use camcraft_geom::synthetic;
use glam::{DVec3, dvec2};

/// Each pass in `path`: its points, in order.
fn passes(path: &Toolpath) -> Vec<Vec<DVec3>> {
    let mut out: Vec<Vec<DVec3>> = Vec::new();
    for m in path.moves() {
        match m.role {
            MoveRole::Link => out.push(vec![m.to]),
            _ => {
                if let Some(p) = out.last_mut() {
                    p.push(m.to);
                }
            }
        }
    }
    out
}

fn signed_area(points: &[DVec3]) -> f64 {
    let flat: Vec<DVec2> = points.iter().map(|p| p.truncate()).collect();
    Contour::new(flat).map_or(0.0, |c| c.signed_area())
}

fn disc(r: f64) -> Region {
    let points = (0..360)
        .map(|i| {
            let a = f64::from(i).to_radians();
            dvec2(a.cos(), a.sin()) * r
        })
        .collect();
    Region::new(vec![Contour::new(points).unwrap()])
}

fn square(half: f64) -> Region {
    Region::rectangle(DVec2::splat(-half), DVec2::splat(half))
}

fn params(stepover: f64) -> ScallopParams {
    ScallopParams {
        stepover,
        tolerance: 0.01,
        direction: Direction::Climb,
        ..Default::default()
    }
}

/// A plate, a square boundary: the passes are the square shrunk by whole
/// stepovers, the first the boundary itself, and cut from the boundary in.
#[test]
fn on_a_plate_the_passes_are_offsets_of_the_boundary() {
    let mesh = synthetic::plane(0.0, 20.0);
    let cutter = DropCutter::new(&mesh);
    let tool = Flat::new(1.0).unwrap();
    let s = 1.0;
    let (path, report) = scallop_finish(&cutter, &tool, &square(8.0), &params(s)).unwrap();
    assert!(report.is_clean(), "{report:?}");
    let found = passes(&path);
    assert!(found.len() >= 8, "{}", found.len());
    let pitch = report.pitch;
    let mut levels = Vec::new();
    for pass in &found {
        // Away from the corners' diagonals, where the offsets meet.
        let ins: Vec<f64> = pass
            .iter()
            .filter(|p| (p.x.abs() - p.y.abs()).abs() > 2.0 * pitch)
            .map(|p| 8.0 - p.x.abs().max(p.y.abs()))
            .collect();
        let Some(&first) = ins.first() else { continue };
        let k = (first / s).round();
        for d in &ins {
            assert!((d - k * s).abs() < 1e-3, "{d} is not at {k} stepovers in");
        }
        levels.push(k);
        for p in pass {
            assert!(p.z.abs() < 1e-9);
        }
    }
    assert_eq!(levels.first(), Some(&0.0), "the boundary first");
    for w in levels.windows(2) {
        assert!(w[1] >= w[0], "outside in: {levels:?}");
    }
}

/// On a dome the passes are a stepover apart along the surface, however steep:
/// a ball's tip rides a sphere 12 across, so a pass at `r` from the middle is
/// `12·acos(r/12)` from the equator along it.
#[test]
fn on_a_dome_the_passes_are_a_stepover_apart_along_the_surface() {
    let (lat, lon) = (160, 240);
    let mesh = synthetic::uv_sphere(10.0, lat, lon);
    let sag = synthetic::uv_sphere_max_sag(10.0, lat, lon);
    let cutter = DropCutter::new(&mesh);
    let tool = Ball::new(2.0).unwrap();
    let s = 0.5;
    // A boundary at 7 from the middle, 36° down the dome.
    let edge = 12.0 * (7.0f64 / 12.0).acos();
    let (path, report) = scallop_finish(&cutter, &tool, &disc(7.0), &params(s)).unwrap();
    assert!(report.is_clean(), "{report:?}");
    let along = |p: &DVec3| 12.0 * (p.truncate().length().min(12.0) / 12.0).acos() - edge;
    let mut found = passes(&path);
    // The last is the top-up, halfway from the last level to the crest.
    let top_up = found.pop().unwrap();
    let crest = 12.0 * 7f64.atan2((144.0f64 - 49.0).sqrt());
    let top_level = (crest / s).floor() * s;
    for p in &top_up {
        assert!(
            (along(p) - (top_level + crest) / 2.0).abs() < 0.25 * s,
            "{p:?}"
        );
    }
    let mut means = Vec::new();
    for pass in &found {
        let at: Vec<f64> = pass.iter().map(along).collect();
        let mean = at.iter().sum::<f64>() / at.len() as f64;
        let level = (mean / s).round() * s;
        for a in &at {
            // First order in the grid's pitch, most where the rings close in.
            assert!((a - level).abs() < 0.25 * s + sag, "{a} is not at {level}");
        }
        means.push(mean);
    }
    assert!(means.len() >= 10, "{}", means.len());
    for w in means.windows(2) {
        let gap = w[1] - w[0];
        assert!(
            (gap - s).abs() < 0.1 * s,
            "passes {gap} apart, not {s}: {means:?}"
        );
    }
}

/// Where the surface is outside the slope range, nothing is cut.
#[test]
fn slope_limits_are_kept_to() {
    let mesh = synthetic::uv_sphere(10.0, 120, 180);
    let cutter = DropCutter::new(&mesh);
    let tool = Ball::new(2.0).unwrap();
    let p = ScallopParams {
        slope_from: 20f64.to_radians(),
        slope_to: 50f64.to_radians(),
        ..params(0.5)
    };
    let (path, report) = scallop_finish(&cutter, &tool, &disc(11.0), &p).unwrap();
    assert!(report.outside_slope > 0, "{report:?}");
    let (lo, hi) = (
        12.0 * 20f64.to_radians().sin(),
        12.0 * 50f64.to_radians().sin(),
    );
    let slack = 2.0 * report.pitch;
    let found = passes(&path);
    assert!(!found.is_empty());
    for pass in found {
        for q in pass {
            let r = q.truncate().length();
            assert!(r > lo - slack && r < hi + slack, "{r} outside {lo}..{hi}");
        }
    }
}

/// Climbing with a clockwise spindle from the boundary in has the side still
/// to finish, inward, on the right: clockwise. Conventional, or inside out,
/// the other way.
#[test]
fn direction_follows_climb_and_order() {
    let mesh = synthetic::plane(0.0, 20.0);
    let cutter = DropCutter::new(&mesh);
    let tool = Flat::new(1.0).unwrap();
    for (direction, order, clockwise) in [
        (Direction::Climb, Order::OutsideIn, true),
        (Direction::Conventional, Order::OutsideIn, false),
        (Direction::Climb, Order::InsideOut, false),
    ] {
        let p = ScallopParams {
            direction,
            order,
            ..params(1.0)
        };
        let (path, _) = scallop_finish(&cutter, &tool, &square(6.0), &p).unwrap();
        let rings: Vec<f64> = passes(&path)
            .iter()
            .map(|pass| signed_area(pass))
            .filter(|a| a.abs() > 1.0)
            .collect();
        assert!(!rings.is_empty());
        for a in rings {
            assert_eq!(a < 0.0, clockwise, "{direction:?} {order:?}: {a}");
        }
    }
}

/// Inside out, the innermost passes come first and the boundary last.
#[test]
fn inside_out_ends_on_the_boundary() {
    let mesh = synthetic::plane(0.0, 20.0);
    let cutter = DropCutter::new(&mesh);
    let tool = Flat::new(1.0).unwrap();
    let p = ScallopParams {
        order: Order::InsideOut,
        ..params(1.0)
    };
    let (path, _) = scallop_finish(&cutter, &tool, &square(6.0), &p).unwrap();
    let depth = |pass: &Vec<DVec3>| {
        pass.iter()
            .map(|q| 6.0 - q.x.abs().max(q.y.abs()))
            .fold(f64::MAX, f64::min)
    };
    let depths: Vec<f64> = passes(&path).iter().map(depth).collect();
    for w in depths.windows(2) {
        assert!(w[1] <= w[0] + 1e-6, "{depths:?}");
    }
    assert!(depths.last().unwrap().abs() < 1e-6);
}

/// A step's wall is left for a waterline pass, and every point is on the
/// part and every cut between points within tolerance of it.
#[test]
fn a_wall_is_left_and_nothing_cuts_below_the_surface() {
    let mesh = synthetic::step(6.0, 20.0);
    let cutter = DropCutter::new(&mesh);
    let tool = Ball::new(1.5).unwrap();
    let (path, report) = scallop_finish(&cutter, &tool, &square(8.0), &params(0.8)).unwrap();
    assert!(report.too_steep > 0, "{report:?}");
    for m in path.moves() {
        let exact = cutter.drop_at(&tool, m.to.x, m.to.y).unwrap();
        assert!(
            (m.to.z - exact).abs() < 1e-9,
            "{:?} is not on the part",
            m.to
        );
    }
    let mut prev: Option<DVec3> = None;
    for m in path.moves() {
        if m.role == MoveRole::Cut
            && let Some(a) = prev
        {
            for t in [0.25, 0.5, 0.75] {
                let q = a.lerp(m.to, t);
                let floor = cutter.drop_at(&tool, q.x, q.y).unwrap();
                assert!(q.z >= floor - 0.01 - 1e-9, "{q:?} below {floor}");
            }
        }
        prev = Some(m.to);
    }
}

/// A strip whose middle rises more than half a stepover past its last pass
/// gets one more, near its middle.
#[test]
fn a_crest_gets_a_top_up() {
    let mesh = synthetic::plane(0.0, 60.0);
    let cutter = DropCutter::new(&mesh);
    let tool = Flat::new(0.5).unwrap();
    let strip = Region::rectangle(dvec2(-1.7, -20.0), dvec2(1.7, 20.0));
    let (path, report) = scallop_finish(&cutter, &tool, &strip, &params(1.0)).unwrap();
    assert_eq!(report.top_ups, 1, "{report:?}");
    let nearest = passes(&path)
        .iter()
        .flatten()
        .filter(|q| q.y.abs() < 15.0)
        .map(|q| q.x.abs())
        .fold(f64::MAX, f64::min);
    assert!(
        nearest < 0.5,
        "nothing within half a stepover of the middle: {nearest}"
    );
}

#[test]
fn a_field_too_large_is_coarsened_then_refused() {
    let mesh = synthetic::plane(0.0, 20.0);
    let cutter = DropCutter::new(&mesh);
    let tool = Flat::new(1.0).unwrap();
    let coarse = ScallopParams {
        max_nodes: 2_000,
        ..params(1.0)
    };
    let (_, report) = scallop_finish(&cutter, &tool, &square(8.0), &coarse).unwrap();
    assert!(report.coarsened && report.nodes <= 2_000, "{report:?}");
    let tiny = ScallopParams {
        max_nodes: 50,
        ..params(1.0)
    };
    assert!(matches!(
        scallop_finish(&cutter, &tool, &square(8.0), &tiny),
        Err(StrategyError::FieldTooLarge { .. })
    ));
}

#[test]
fn rejects_unusable_parameters() {
    let mesh = synthetic::plane(0.0, 20.0);
    let cutter = DropCutter::new(&mesh);
    let tool = Flat::new(1.0).unwrap();
    for bad in [
        params(0.0),
        ScallopParams {
            tolerance: f64::NAN,
            ..params(1.0)
        },
        ScallopParams {
            slope_from: 1.0,
            slope_to: 0.5,
            ..params(1.0)
        },
        ScallopParams {
            slope_to: 2.0,
            ..params(1.0)
        },
    ] {
        assert!(scallop_finish(&cutter, &tool, &square(5.0), &bad).is_err());
    }
}

/// The same passes on one thread or four.
#[test]
fn is_deterministic() {
    let mesh = synthetic::uv_sphere(10.0, 60, 90);
    let cutter = DropCutter::new(&mesh);
    let tool = Ball::new(2.0).unwrap();
    let run = |threads: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                scallop_finish(&cutter, &tool, &disc(9.0), &params(0.8))
                    .unwrap()
                    .0
            })
    };
    assert_eq!(run(1).moves(), run(4).moves());
}

/// Only where the tool touches the part.
#[test]
fn contact_is_the_part_grown_by_the_tool() {
    let mesh = synthetic::plane(0.0, 5.0);
    let shadow = camcraft_geom::shadow::shadow_above(&mesh, -1.0).unwrap();
    let contact = contact_region(&shadow, 2.0, 0.01).unwrap();
    let (lo, hi) = contact.bounds().unwrap();
    assert!(
        (lo.x + 7.0).abs() < 0.02 && (hi.y - 7.0).abs() < 0.02,
        "{lo:?} {hi:?}"
    );
    assert!(lo.x > -7.0 && hi.y < 7.0);
}
