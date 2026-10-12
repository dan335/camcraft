use super::*;
use crate::cutter::allowance::{Allowance, EffectiveTool};
use crate::cutter::dropcutter::DropCutter;
use crate::cutter::tool::Flat;
use crate::path::toolpath::MoveRole;
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
