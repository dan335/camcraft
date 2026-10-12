use super::*;
use glam::dvec2;

/// A 6 mm tool at a tenth of its width, climbing with a clockwise spindle.
fn params() -> AdaptiveParams {
    AdaptiveParams {
        stepdown: 6.0,
        optimal_load: 0.6,
        feed: 1000.0,
        top: 0.0,
        bottom: -6.0,
        tolerance: 0.01,
        climb_right: true,
        direction: Direction::Climb,
        helix_radius: 2.7,
        stay_down: 30.0,
        ramp_angle: 2f64.to_radians(),
        ramp_feed: 500.0,
        ramp_stepdown: None,
        safe_distance: 2.0,
        flats: Vec::new(),
    }
}

const R: f64 = 3.0;

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Region {
    Region::rectangle(dvec2(x0, y0), dvec2(x1, y1))
}

/// The engagement of every step of a level's passes, measured on a raster of
/// the material, apart from the regions: each cell is cut when the tool's
/// centre passes within its radius, and a step's engagement is the share of
/// the circle round where it ends that lands on cells still standing. Steps
/// within `entry` of where a pass went down are left out, as a helix's start
/// is loaded as it may be.
struct Raster {
    lo: DVec2,
    cell: f64,
    nx: usize,
    ny: usize,
    standing: Vec<bool>,
}

impl Raster {
    fn new(material: &Region, cell: f64) -> Self {
        let (lo, hi) = material.bounds().unwrap();
        let nx = ((hi.x - lo.x) / cell).ceil() as usize + 1;
        let ny = ((hi.y - lo.y) / cell).ceil() as usize + 1;
        let mut standing = vec![false; nx * ny];
        for j in 0..ny {
            for i in 0..nx {
                standing[j * nx + i] = material.contains(Self::centre_of(lo, cell, i, j));
            }
        }
        Self {
            lo,
            cell,
            nx,
            ny,
            standing,
        }
    }

    fn centre_of(lo: DVec2, cell: f64, i: usize, j: usize) -> DVec2 {
        lo + dvec2((i as f64 + 0.5) * cell, (j as f64 + 0.5) * cell)
    }

    fn at(&self, p: DVec2) -> bool {
        let q = (p - self.lo) / self.cell;
        if q.x < 0.0 || q.y < 0.0 {
            return false;
        }
        let (i, j) = (q.x as usize, q.y as usize);
        i < self.nx && j < self.ny && self.standing[j * self.nx + i]
    }

    /// Cut along the segment `a`–`b`.
    fn sweep(&mut self, a: DVec2, b: DVec2, r: f64) {
        let lo = ((a.min(b) - DVec2::splat(r) - self.lo) / self.cell).floor();
        let hi = ((a.max(b) + DVec2::splat(r) - self.lo) / self.cell).ceil();
        let (i0, j0) = (lo.x.max(0.0) as usize, lo.y.max(0.0) as usize);
        let (i1, j1) = (
            (hi.x.max(0.0) as usize).min(self.nx),
            (hi.y.max(0.0) as usize).min(self.ny),
        );
        for j in j0..j1 {
            for i in i0..i1 {
                let c = Self::centre_of(self.lo, self.cell, i, j);
                if camcraft_geom::engagement::segment_distance2(c, a, b) <= r * r {
                    self.standing[j * self.nx + i] = false;
                }
            }
        }
    }

    /// The angle of the circle about `c` on standing cells.
    fn engagement(&self, c: DVec2, r: f64) -> f64 {
        self.engagement_past(c, r, c)
    }

    /// The angle of the circle about `c` on standing cells and further than
    /// `r` from the segment `from`–`c`: what a step from `from` cuts ahead.
    fn engagement_past(&self, c: DVec2, r: f64, from: DVec2) -> f64 {
        let n = 720;
        let inside = (r * (1.0 - 1e-3)).powi(2);
        let hits = (0..n)
            .filter(|&k| {
                let p = c + DVec2::from_angle(TAU * (k as f64 + 0.5) / n as f64) * r;
                self.at(p) && camcraft_geom::engagement::segment_distance2(p, from, c) > inside
            })
            .count();
        TAU * hits as f64 / n as f64
    }

    fn left(&self) -> usize {
        self.standing.iter().filter(|s| **s).count()
    }
}

/// The largest engagement any step of `cut` takes, away from its entries, and
/// the mean over the steps that cut: measured by the raster, from `material`.
fn measured(cut: &LevelCut, material: &Region, target: f64) -> (f64, f64, Raster) {
    let mut raster = Raster::new(material, 0.02);
    for &(c, r) in &cut.helixes {
        // What the helix cut going down.
        let n = 256;
        for k in 0..n {
            let a = c + DVec2::from_angle(TAU * k as f64 / n as f64) * r;
            let b = c + DVec2::from_angle(TAU * (k + 1) as f64 / n as f64) * r;
            raster.sweep(a, b, R);
        }
    }
    let mut most: f64 = 0.0;
    let mut sum = 0.0;
    let mut cutting = 0;
    let mut at: Option<DVec2> = None;
    for (arrive, points) in &cut.passes {
        if let (Arrive::Stay, Some(a), Some(&b)) = (arrive, at, points.first()) {
            // A link at the level only grazes the material: nowhere along it does
            // the tool take more than a pass would.
            let n = (a.distance(b) / 0.1).ceil().max(1.0) as usize;
            for k in 0..=n {
                let c = a.lerp(b, k as f64 / n as f64);
                let e = raster.engagement(c, R);
                assert!(
                    e <= target + 0.12,
                    "a stay-down link from {a} to {b} takes {e} rad of material at {c}"
                );
            }
            raster.sweep(a, b, R);
        }
        let entered = cut.helixes.iter().any(|(c, r)| {
            points
                .first()
                .is_some_and(|p| (p.distance(*c) - r).abs() < 1e-6)
        });
        let mut run = 0.0;
        for w in points.windows(2) {
            let (a, b) = (w[0], w[1]);
            // Ahead of the step only: the circle's back half is its own sweep.
            let e = raster.engagement_past(b, R, a);
            run += a.distance(b);
            raster.sweep(a, b, R);
            if entered && run < 2.0 * R {
                continue;
            }
            most = most.max(e);
            if e > 0.05 {
                sum += e;
                cutting += 1;
            }
        }
        at = points.last().copied();
    }
    (most, sum / cutting.max(1) as f64, raster)
}

#[test]
fn an_open_block_is_cleared_at_the_load_asked_for() {
    let p = params();
    let level = LevelParams::new(&p, R);
    let block = rect(0.0, 0.0, 40.0, 25.0);
    // Room round it for the tool to start outside.
    let allowed = rect(-8.0, -8.0, 48.0, 33.0);
    let cut = clear_level(-6.0, &allowed, &block, &level).unwrap();
    assert!(!cut.passes.is_empty());
    assert!(cut.helixes.is_empty(), "went down into an open block");
    assert!(cut.left < 1.0, "left {} mm2", cut.left);
    let (most, mean, raster) = measured(&cut, &block, level.target);
    assert!(
        most <= level.limit + 0.12,
        "a step took {most} rad, over the limit {}",
        level.limit
    );
    assert!(
        (mean - level.target).abs() < 0.2 * level.target,
        "mean engagement {mean} rad against {} asked for",
        level.target
    );
    // Everything the raster still has standing is a sliver.
    assert!(raster.left() * 4 < 400 * 10, "{} cells left", raster.left());
}

#[test]
fn a_closed_pocket_is_entered_down_a_helix_and_cleared() {
    let p = params();
    let level = LevelParams::new(&p, R);
    let block = rect(-10.0, -10.0, 60.0, 50.0);
    // The tool kept to a pocket well inside the block.
    let allowed = rect(3.0, 3.0, 37.0, 27.0);
    let cut = clear_level(-6.0, &allowed, &block, &level).unwrap();
    assert_eq!(cut.helixes.len(), 1, "{:?}", cut.helixes);
    let (c, r) = cut.helixes[0];
    assert!((r - 2.7).abs() < 1e-9);
    assert!(allowed.offset(-r, 1e-3).contains(c));
    let reachable = block.intersection(&allowed.offset(R, 1e-3)).unwrap();
    assert!(
        cut.left < 1.0,
        "left {} of {} mm2",
        cut.left,
        reachable.area()
    );
    let (most, mean, _) = measured(&cut, &reachable, level.target);
    assert!(most <= level.limit + 0.12, "a step took {most} rad");
    assert!(mean > 0.6 * level.target, "mean engagement {mean}");
    // Every pass kept the tool's centre where it may be.
    for (_, points) in &cut.passes {
        for w in points.windows(2) {
            assert!(allowed.offset(1e-6, 1e-4).contains_segment(w[0], w[1]));
        }
    }
}

#[test]
fn an_island_is_cut_round_and_never_into() {
    let p = params();
    let level = LevelParams::new(&p, R);
    let block = rect(0.0, 0.0, 50.0, 40.0);
    let island = rect(20.0, 15.0, 30.0, 25.0);
    let allowed = rect(-8.0, -8.0, 58.0, 48.0)
        .difference(&island.offset(R, 1e-3))
        .unwrap();
    let cut = clear_level(-6.0, &allowed, &block.difference(&island).unwrap(), &level).unwrap();
    assert!(cut.left < 1.0, "left {}", cut.left);
    // Never nearer the island than the tool's radius, to Clipper's tick.
    let (lo, hi) = (dvec2(20.0, 15.0), dvec2(30.0, 25.0));
    for (_, points) in &cut.passes {
        for q in points {
            let d = (*q - q.clamp(lo, hi)).length();
            assert!(d >= R - 1e-4, "{q} is {d} from the island");
        }
    }
}

#[test]
fn a_slot_too_narrow_for_a_helix_is_left_and_said() {
    let p = params();
    let level = LevelParams::new(&p, R);
    let block = rect(-10.0, -10.0, 60.0, 50.0);
    // A slot whose centre line is too narrow for even a quarter helix.
    let allowed = rect(10.0, 10.0, 30.0, 10.4);
    let cut = clear_level(-6.0, &allowed, &block, &level).unwrap();
    assert!(cut.passes.is_empty());
    assert_eq!(cut.no_room, 1);
}

#[test]
fn cutting_both_ways_cuts_back_conventional_and_holds_the_load() {
    let mut p = params();
    p.direction = Direction::BothWays;
    let level = LevelParams::new(&p, R);
    let block = rect(0.0, 0.0, 40.0, 25.0);
    let allowed = rect(-8.0, -8.0, 48.0, 33.0);
    let both = clear_level(-6.0, &allowed, &block, &level).unwrap();
    assert!(both.left < 1.0, "left {}", both.left);
    let (most, _, _) = measured(&both, &block, level.target);
    assert!(most <= level.limit + 0.12, "a step took {most} rad");

    let climb = clear_level(-6.0, &allowed, &block, &LevelParams::new(&params(), R)).unwrap();
    let links = |c: &LevelCut| c.passes.iter().filter(|(a, _)| *a == Arrive::Link).count();
    assert!(
        links(&both) <= links(&climb),
        "both ways linked {} times, climbing {}",
        links(&both),
        links(&climb)
    );
}

#[test]
fn flat_faces_are_found_once_each() {
    use camcraft_geom::synthetic;
    let mesh = synthetic::cuboid(dvec3(0.0, 0.0, -5.0), dvec3(10.0, 10.0, 0.0));
    let flats = flat_heights(&mesh, 0.01);
    assert_eq!(flats, vec![0.0]);
}

#[test]
fn clearing_is_the_same_every_time() {
    let p = params();
    let level = LevelParams::new(&p, R);
    let block = rect(0.0, 0.0, 30.0, 20.0);
    let allowed = rect(-8.0, -8.0, 38.0, 28.0)
        .difference(&rect(12.0, 8.0, 18.0, 12.0).offset(R, 1e-3))
        .unwrap();
    let a = clear_level(-6.0, &allowed, &block, &level).unwrap();
    let b = clear_level(-6.0, &allowed, &block, &level).unwrap();
    assert_eq!(a.passes, b.passes);
}

/// A block standing in a bigger one, its top a flat face between two levels:
/// the flat level over it clears down to the stock to leave above it.
#[test]
fn a_flat_face_between_levels_gets_a_level_of_its_own() {
    use crate::cutter::allowance::{Allowance, EffectiveTool};
    use crate::cutter::tool::Flat;
    use camcraft_geom::synthetic;
    let mesh = synthetic::cuboid(dvec3(0.0, 0.0, 0.0), dvec3(40.0, 20.0, 10.0));
    let flat = Flat::new(R).unwrap();
    let tool = EffectiveTool::new(&flat, Allowance::new(0.5, 0.5).unwrap()).unwrap();
    let mut reach = LevelReach::new(&mesh, &tool, 0.01).unwrap();
    let mut p = params();
    p.top = 14.0;
    p.bottom = 0.0;
    p.stepdown = 6.0;
    p.flats = flat_heights(&mesh, 0.01)
        .into_iter()
        .map(|z| z + 0.5)
        .collect();
    assert_eq!(p.flats, vec![10.5]);
    let area = rect(-20.0, -20.0, 60.0, 40.0);
    let stocks = [(dvec2(-10.0, -10.0), dvec2(50.0, 30.0), 14.0)];
    let (path, report) = adaptive_rough(&mut reach, &area, &stocks, tool.base(), &p).unwrap();
    assert_eq!(report.flat_levels, 1, "{report:?}");
    // Something cut at the flat level, over the top of the block.
    let over = path
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Cut && (m.to.z - 10.5).abs() < 1e-9)
        .filter(|m| (5.0..35.0).contains(&m.to.x) && (5.0..15.0).contains(&m.to.y))
        .count();
    assert!(over > 0, "{report:?}");
}
