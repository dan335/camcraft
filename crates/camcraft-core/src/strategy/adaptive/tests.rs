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
        least_helix_radius: 0.675,
        stay_down: 30.0,
        stay_down_level: 1.0,
        stay_down_clearance: 0.0,
        lift_height: 0.0,
        slot_width: None,
        ramp_angle: 2f64.to_radians(),
        ramp_feed: 500.0,
        ramp_stepdown: None,
        safe_distance: 2.0,
        flats: Vec::new(),
        fine_stepdown: None,
        min_flat_stepdown: 0.0,
        min_axial_engagement: 0.0,
        shallow: None,
        machine_cavities: true,
        min_cutting_radius: 0.0,
        by_area: false,
        before: Vec::new(),
        ignore_less_than: 0.0,
        watch: crate::control::Watch::default(),
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
        if let (Arrive::Stay { via }, Some(a), Some(&b)) = (arrive, at, points.first()) {
            // A link at the level only grazes the material: nowhere along it does
            // the tool take more than a pass would.
            let way: Vec<DVec2> = std::iter::once(a)
                .chain(via.iter().copied())
                .chain(std::iter::once(b))
                .collect();
            for leg in way.windows(2) {
                let (a, b) = (leg[0], leg[1]);
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

/// A closed slot too narrow for the least helix is ramped into along its
/// middle, never a helix smaller than the least; a hole too small even to
/// ramp along is left, and said.
#[test]
fn a_slot_too_narrow_for_a_helix_is_ramped_into_along_it() {
    let p = params();
    let level = LevelParams::new(&p, R);
    let block = rect(-10.0, -10.0, 60.0, 50.0);
    // A slot whose centre line is too narrow for the least helix, 0.675.
    let allowed = rect(10.0, 10.0, 30.0, 10.4);
    let cut = clear_level(-6.0, &allowed, &block, &level).unwrap();
    assert!(cut.helixes.is_empty(), "{:?}", cut.helixes);
    assert_eq!(cut.no_room, 0);
    let Some((Arrive::Ramp { way }, _)) = cut.passes.first() else {
        panic!("{:?}", cut.passes.first());
    };
    // Along the slot, as long as the largest helix goes round, inside it.
    let length = camcraft_geom::contour::polyline_length(way);
    assert!((length - TAU * 2.7).abs() < 1e-6, "{length}");
    assert!(
        way.windows(2)
            .all(|w| allowed.offset(1e-6, 1e-4).contains_segment(w[0], w[1]))
    );
    assert!(cut.left < 1.0, "left {}", cut.left);

    // Down it no steeper than the ramp angle, then along it at the level,
    // ending where it started.
    let moves = ramped(way, -6.0, 2.0, &p);
    assert_eq!(moves.last(), Some(&way[0].extend(-6.0)));
    assert!((moves[0].z - 2.0).abs() < 1e-9);
    for w in moves.windows(2) {
        let (a, b) = (w[0], w[1]);
        let run = a.truncate().distance(b.truncate());
        assert!(b.z <= a.z + 1e-9, "{a} {b}");
        assert!(
            a.z - b.z <= run * p.ramp_angle.tan() + 1e-9,
            "{a} to {b} is steeper than the ramp"
        );
    }
    let level_run: f64 = moves
        .windows(2)
        .filter(|w| (w[0].z + 6.0).abs() < 1e-9 && (w[1].z + 6.0).abs() < 1e-9)
        .map(|w| w[0].distance(w[1]))
        .sum();
    assert!((level_run - length).abs() < 1e-6, "{level_run}");

    // Too small for a ramp no shorter than the least helix is across.
    let cut = clear_level(-6.0, &rect(10.0, 10.0, 10.4, 10.4), &block, &level).unwrap();
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

/// A slope rising across the stock, roughed 6 mm a level: with a fine
/// stepdown, levels between cut the terraces the levels leave on it, and
/// nothing where the levels below have cleared already; without, none.
#[test]
fn a_fine_stepdown_cuts_the_terraces_on_a_slope() {
    use crate::cutter::tool::Flat;
    use camcraft_geom::synthetic;
    // z = x / 2, from -10 at x = -20 to 10 at x = 20.
    let mesh = synthetic::inclined_plane(0.5, 0.0, 0.0, 20.0);
    let flat = Flat::new(R).unwrap();
    let rough = |fine: Option<f64>| {
        let tool = crate::cutter::allowance::EffectiveTool::new(
            &flat,
            crate::cutter::allowance::Allowance::new(0.0, 0.0).unwrap(),
        )
        .unwrap();
        let mut reach = LevelReach::new(&mesh, &tool, 0.01).unwrap();
        let mut p = params();
        p.top = 10.0;
        p.bottom = -10.0;
        p.stepdown = 6.0;
        p.fine_stepdown = fine;
        let area = rect(-30.0, -30.0, 30.0, 30.0);
        let stocks = [(dvec2(-20.0, -20.0), dvec2(20.0, 20.0), 10.0)];
        adaptive_rough(&mut reach, &area, &stocks, tool.base(), &p).unwrap()
    };
    // 20 deep at no more than 6 a level: levels at 5, 0, -5 and -10, evenly;
    // fine ones 2 mm up from each.
    let at = |path: &Toolpath, z: f64| -> Vec<f64> {
        path.moves()
            .iter()
            .filter(|m| m.role == MoveRole::Cut && (m.to.z - z).abs() < 1e-9)
            .map(|m| m.to.x)
            .collect()
    };
    let (plain, report) = rough(None);
    assert_eq!(report.fine_levels, 0);
    // At -10 the slope leaves nothing to cut.
    assert_eq!(cut_levels(&plain), [5.0, 0.0, -5.0]);
    assert!(at(&plain, 2.0).is_empty());

    let (fine, report) = rough(Some(2.0));
    assert!(report.fine_levels > 0, "{report:?}");
    let xs = at(&fine, 2.0);
    assert!(!xs.is_empty(), "the terrace between 0 and 2 is cut");
    // At 2 mm the slope stands at x = 4, and the level at 0 cleared up to
    // where it stands at 0, x = 0, less the tool: only the terrace between,
    // and the tool beside it, is left to cut.
    assert!(
        xs.iter().all(|&x| x > -2.0 * R - 0.5 && x < 4.0),
        "{:?}",
        xs.iter()
            .fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)))
    );
}

/// A frame 60 by 20 with a divider down its middle, standing on a floor: two
/// pockets, 24 by 12 and 8 deep, side by side, the stock its outline.
fn two_pockets() -> TriMesh {
    use camcraft_geom::synthetic::cuboid;
    let parts = [
        cuboid(dvec3(0.0, 0.0, -10.0), dvec3(60.0, 20.0, -8.0)),
        cuboid(dvec3(0.0, 0.0, -8.0), dvec3(4.0, 20.0, 0.0)),
        cuboid(dvec3(56.0, 0.0, -8.0), dvec3(60.0, 20.0, 0.0)),
        cuboid(dvec3(4.0, 0.0, -8.0), dvec3(56.0, 4.0, 0.0)),
        cuboid(dvec3(4.0, 16.0, -8.0), dvec3(56.0, 20.0, 0.0)),
        cuboid(dvec3(28.0, 4.0, -8.0), dvec3(32.0, 16.0, 0.0)),
    ];
    let refs: Vec<&TriMesh> = parts.iter().collect();
    TriMesh::merged(&refs).unwrap()
}

/// The two pockets roughed 3 mm a level with a 6 mm flat, as `p` asks.
fn rough_pockets(p: &AdaptiveParams) -> (Toolpath, AdaptiveReport) {
    use crate::cutter::allowance::{Allowance, EffectiveTool};
    use crate::cutter::tool::Flat;
    let mesh = two_pockets();
    let flat = Flat::new(R).unwrap();
    let tool = EffectiveTool::new(&flat, Allowance::new(0.0, 0.0).unwrap()).unwrap();
    let mut reach = LevelReach::new(&mesh, &tool, 0.01).unwrap();
    let mut p = p.clone();
    p.top = 0.0;
    p.bottom = -8.0;
    p.stepdown = 3.0;
    let area = rect(-10.0, -10.0, 70.0, 30.0);
    let stocks = [(dvec2(0.0, 0.0), dvec2(60.0, 20.0), 0.0)];
    adaptive_rough(&mut reach, &area, &stocks, tool.base(), &p).unwrap()
}

/// Which pocket each cutting move is in, in order, each change once: 0 for the
/// first, 1 for the second.
fn pockets_in_order(path: &Toolpath) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for m in path.moves().iter().filter(|m| m.role == MoveRole::Cut) {
        let k = u8::from(m.to.x > 30.0);
        if out.last() != Some(&k) {
            out.push(k);
        }
    }
    out
}

/// By depth, each level goes round both pockets before the next down; by
/// area, one pocket is cleared all the way down before the other. The 8 mm
/// is not a whole number of 3 mm stepdowns: the levels share it evenly.
#[test]
fn by_area_each_pocket_is_cleared_all_the_way_down_first() {
    let (by_depth, report) = rough_pockets(&params());
    assert!(report.levels >= 3 && report.helixes >= 6, "{report:?}");
    assert!(
        pockets_in_order(&by_depth).len() > 2,
        "by depth went between them at every level"
    );
    let mut p = params();
    p.by_area = true;
    let (by_area, area_report) = rough_pockets(&p);
    assert_eq!(pockets_in_order(&by_area).len(), 2, "one, then the other");
    // The same passes, only in another order.
    assert_eq!(area_report.passes, report.passes);
    assert_eq!(area_report.helixes, report.helixes);
    // Each pocket from the top down.
    for k in [0u8, 1] {
        let zs: Vec<f64> = by_area
            .moves()
            .iter()
            .filter(|m| m.role == MoveRole::Cut && u8::from(m.to.x > 30.0) == k)
            .map(|m| m.to.z)
            .collect();
        assert!(zs.windows(2).all(|w| w[1] <= w[0] + 1e-9), "pocket {k}");
    }
    let third = 8.0 / 3.0;
    let even = [-third, -2.0 * third, -8.0];
    let zs = cut_levels(&by_area);
    assert_eq!(zs.len(), 3, "{zs:?}");
    assert!(
        zs.iter().zip(even).all(|(a, b)| (a - b).abs() < 1e-9),
        "{zs:?}"
    );
}

/// A surface over `lo`-`hi`, `n` cells a side, at the height `h` gives.
fn surface(lo: DVec2, hi: DVec2, n: u32, h: impl Fn(DVec2) -> f64) -> TriMesh {
    let mut v = Vec::new();
    for j in 0..=n {
        for i in 0..=n {
            let t = dvec2(f64::from(i), f64::from(j)) / f64::from(n);
            let p = lo + (hi - lo) * t;
            v.push(p.extend(h(p)));
        }
    }
    let k = |i: u32, j: u32| j * (n + 1) + i;
    let mut t = Vec::new();
    for j in 0..n {
        for i in 0..n {
            t.push([k(i, j), k(i + 1, j), k(i + 1, j + 1)]);
            t.push([k(i, j), k(i + 1, j + 1), k(i, j + 1)]);
        }
    }
    TriMesh::new(v, t).unwrap()
}

/// The height at `r` from a centre, along straight lines between `knots`,
/// each a distance out and a height, the last's beyond it.
fn profile(r: f64, knots: &[(f64, f64)]) -> f64 {
    for w in knots.windows(2) {
        let ((r0, z0), (r1, z1)) = (w[0], w[1]);
        if r <= r1 {
            return z0 + (z1 - z0) * ((r - r0) / (r1 - r0)).clamp(0.0, 1.0);
        }
    }
    knots.last().unwrap().1
}

/// `mesh` roughed with a 6 mm flat from 0 down to -8, 4 mm a level and
/// fine levels 1 mm apart, inside the stock `lo`-`hi`, as `p` asks.
fn rough_fine(
    mesh: &TriMesh,
    lo: DVec2,
    hi: DVec2,
    p: &AdaptiveParams,
) -> (Toolpath, AdaptiveReport) {
    use crate::cutter::allowance::{Allowance, EffectiveTool};
    use crate::cutter::tool::Flat;
    let flat = Flat::new(R).unwrap();
    let tool = EffectiveTool::new(&flat, Allowance::new(0.0, 0.0).unwrap()).unwrap();
    let mut reach = LevelReach::new(mesh, &tool, 0.01).unwrap();
    let mut p = p.clone();
    p.top = 0.0;
    p.bottom = -8.0;
    p.stepdown = 4.0;
    p.fine_stepdown = Some(1.0);
    let area = Region::rectangle(lo - DVec2::splat(10.0), hi + DVec2::splat(10.0));
    let stocks = [(lo, hi, 0.0)];
    adaptive_rough(&mut reach, &area, &stocks, tool.base(), &p).unwrap()
}

/// Which area each cutting move is in, by `area_of`, each change once, and
/// the levels each area's moves are at, in the order they are cut, each
/// change once.
fn areas_in_order(path: &Toolpath, area_of: impl Fn(DVec2) -> u8) -> (Vec<u8>, Vec<Vec<f64>>) {
    let mut order: Vec<u8> = Vec::new();
    let mut levels: Vec<Vec<f64>> = vec![Vec::new(); 2];
    for m in path.moves().iter().filter(|m| m.role == MoveRole::Cut) {
        let k = area_of(m.to.truncate());
        if order.last() != Some(&k) {
            order.push(k);
        }
        let zs = &mut levels[usize::from(k)];
        if zs.last().is_none_or(|z| (z - m.to.z).abs() > 1e-9) {
            zs.push(m.to.z);
        }
    }
    (order, levels)
}

/// Each slab's level and then its fine levels bottom up, the slabs from the
/// top: -4 and the three above it, then -8 and the three above it.
const SLAB_THEN_FINE: [f64; 8] = [-4.0, -3.0, -2.0, -1.0, -8.0, -7.0, -6.0, -5.0];

/// Two round pockets with drafted walls side by side, 8 deep, and a fine
/// stepdown: by area, each pocket is cut whole before the other, each slab's
/// level and then its fine levels up the wall from the bottom, then the next
/// slab down. The fine levels cut terraces outside where the tool may be at
/// their slab's level, and are still the pocket's.
#[test]
fn by_area_each_drafted_pocket_is_cut_slab_by_slab_with_its_fine_levels() {
    let centres = [dvec2(16.0, 16.0), dvec2(44.0, 16.0)];
    let knots = [(0.0, -8.0), (7.0, -8.0), (11.0, 0.0)];
    let mesh = surface(dvec2(0.0, 0.0), dvec2(60.0, 32.0), 120, |p| {
        centres
            .iter()
            .map(|c| profile(p.distance(*c), &knots))
            .fold(f64::INFINITY, f64::min)
    });
    let (lo, hi) = (dvec2(0.0, 0.0), dvec2(60.0, 32.0));
    let pocket = |p: DVec2| u8::from(p.x > 30.0);
    let (by_depth, report) = rough_fine(&mesh, lo, hi, &params());
    assert!(report.fine_levels >= 6, "{report:?}");
    assert!(areas_in_order(&by_depth, pocket).0.len() > 2);
    let mut p = params();
    p.by_area = true;
    let (path, area_report) = rough_fine(&mesh, lo, hi, &p);
    assert_eq!(area_report.passes, report.passes);
    let (order, levels) = areas_in_order(&path, pocket);
    assert_eq!(order.len(), 2, "one, then the other: {order:?}");
    for zs in &levels {
        assert_eq!(zs.len(), SLAB_THEN_FINE.len(), "{zs:?}");
        assert!(
            zs.iter()
                .zip(SLAB_THEN_FINE)
                .all(|(a, b)| (a - b).abs() < 1e-9),
            "{zs:?}"
        );
    }
}

/// A round pocket 8 deep in an island, and a channel as deep round the
/// island, their walls drafted: the channel's box holds the pocket whole.
fn island_in_a_channel() -> TriMesh {
    let centre = dvec2(32.0, 32.0);
    let knots = [
        (0.0, -8.0),
        (5.0, -8.0),
        (9.0, 0.0),
        (11.0, 0.0),
        (15.0, -8.0),
        (25.0, -8.0),
        (29.0, 0.0),
    ];
    surface(dvec2(0.0, 0.0), dvec2(64.0, 64.0), 128, |p| {
        profile(p.distance(centre), &knots)
    })
}

/// The pocket's fine levels cut terraces outside where the tool may be at
/// its slab's level, nearest the pocket but inside the channel's box: they
/// are the pocket's. By area, one is cut whole, slab by slab with its fine
/// levels, and then the other. On one core and on eight the program is the
/// same.
#[test]
fn by_area_a_pocket_inside_a_channel_is_cut_whole_apart_from_it() {
    let mesh = island_in_a_channel();
    let (lo, hi) = (dvec2(0.0, 0.0), dvec2(64.0, 64.0));
    let mut p = params();
    p.by_area = true;
    let (path, report) = rough_fine(&mesh, lo, hi, &p);
    assert!(report.fine_levels >= 6, "{report:?}");
    let area = |q: DVec2| u8::from(q.distance(dvec2(32.0, 32.0)) > 10.0);
    let (order, levels) = areas_in_order(&path, area);
    assert_eq!(
        order.len(),
        2,
        "the pocket and the channel, each whole: {order:?} {levels:?}"
    );
    for zs in &levels {
        assert_eq!(zs.len(), SLAB_THEN_FINE.len(), "{zs:?}");
        assert!(
            zs.iter()
                .zip(SLAB_THEN_FINE)
                .all(|(a, b)| (a - b).abs() < 1e-9),
            "{zs:?}"
        );
    }
    // The same on any number of cores.
    let on = |threads: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| rough_fine(&mesh, lo, hi, &p).0)
    };
    let (one, eight) = (on(1), on(8));
    assert_eq!(one.moves(), eight.moves());
    assert_eq!(one.moves(), path.moves());
}

/// The most a pass's heading turns within the tool's radius along it, over
/// every stretch: a full turn or more is going round on the spot.
fn tightest_turn(points: &[DVec2]) -> f64 {
    let mut steps: Vec<(f64, f64)> = Vec::new();
    let (mut gone, mut turned) = (0.0, 0.0);
    for w in points.windows(3) {
        let (a, b, c) = (w[0], w[1], w[2]);
        let (u, v) = (b - a, c - b);
        turned += u.perp_dot(v).atan2(u.dot(v));
        gone += b.distance(c);
        steps.push((gone, turned));
    }
    let mut most: f64 = 0.0;
    for (i, &(g, t)) in steps.iter().enumerate() {
        for &(h, u) in &steps[i..] {
            if h - g > R {
                break;
            }
            most = most.max((u - t).abs());
        }
    }
    most
}

/// A ridge narrower than the optimal load, standing free where the tool may
/// go either side of it: no heading takes the load from it, so a pass turned
/// hard toward it at every step would go round on the spot. It is taken in a
/// bounded number of steps, and no pass turns round on the spot.
#[test]
fn a_ridge_narrower_than_the_load_is_taken_along_it_not_orbited() {
    let p = params();
    let level = LevelParams::new(&p, R);
    let ring = |r: f64| disc(DVec2::ZERO, r, 1e-3).unwrap();
    let ridge = Region::new(vec![ring(10.4), {
        let mut hole = ring(10.0).points().to_vec();
        hole.reverse();
        Contour::new(hole).unwrap()
    }]);
    assert!((ridge.area() - PI * (10.4f64.powi(2) - 100.0)).abs() < 0.1);
    let allowed = rect(-30.0, -30.0, 30.0, 30.0);
    let cut = clear_level(-6.0, &allowed, &ridge, &level).unwrap();
    assert!(cut.left < 0.5, "left {} mm2", cut.left);
    let points: usize = cut.passes.iter().map(|(_, pts)| pts.len()).sum();
    assert!(points < 20_000, "{points} points");
    for (_, pts) in &cut.passes {
        let t = tightest_turn(pts);
        assert!(t < TAU, "a pass turned {t} rad within the tool's radius");
    }
}

/// A rim on a wall, its top nearly level, a hair above where a fine level
/// counted up from the slab would fall: the rim has one level, its flat
/// level, and no other near it; the fine levels below it are spaced evenly
/// up to it. No pass at it goes round on the spot, and it takes a bounded
/// number of moves.
#[test]
fn a_nearly_level_rim_just_above_a_fine_level_is_one_level() {
    use crate::cutter::allowance::{Allowance, EffectiveTool};
    use crate::cutter::tool::Flat;
    let centre = dvec2(24.0, 24.0);
    // The rim rises a thousandth over its 2 mm: nearly level, one flat face,
    // whose highest is its level.
    let knots = [
        (0.0, -10.0),
        (6.0, -10.0),
        (8.0, -2.491),
        (10.0, -2.49),
        (12.0, -10.0),
    ];
    let mesh = surface(dvec2(0.0, 0.0), dvec2(48.0, 48.0), 96, |p| {
        profile(p.distance(centre), &knots)
    });
    let flat = Flat::new(R).unwrap();
    let tool = EffectiveTool::new(&flat, Allowance::new(0.0, 0.0).unwrap()).unwrap();
    let mut reach = LevelReach::new(&mesh, &tool, 0.01).unwrap();
    let mut p = params();
    p.top = 0.0;
    p.bottom = -10.0;
    p.stepdown = 10.0;
    p.fine_stepdown = Some(2.5);
    p.flats = flat_heights(&mesh, 0.01)
        .into_iter()
        .filter(|z| *z > -10.0 + 1e-6)
        .collect();
    assert_eq!(p.flats.len(), 1, "{:?}", p.flats);
    assert!((p.flats[0] + 2.49).abs() < 1e-3, "{:?}", p.flats);
    let area = rect(-10.0, -10.0, 58.0, 58.0);
    let stocks = [(dvec2(0.0, 0.0), dvec2(48.0, 48.0), 0.0)];
    let (path, report) = adaptive_rough(&mut reach, &area, &stocks, tool.base(), &p).unwrap();
    assert_eq!(report.flat_levels, 1, "{report:?}");
    let zs = cut_levels(&path);
    let rim = p.flats[0];
    assert!(
        zs.iter()
            .all(|z| (z - rim).abs() < 1e-9 || (z - rim).abs() > 1.0),
        "{zs:?}"
    );
    // Evenly up to the rim from the slab's level: 7.51 in four steps.
    let step = (rim + 10.0) / 4.0;
    for k in 1..4 {
        let z = -10.0 + step * f64::from(k);
        assert!(zs.iter().any(|l| (l - z).abs() < 1e-9), "{z}: {zs:?}");
    }
    let at_rim: Vec<DVec2> = path
        .moves()
        .iter()
        .filter(|m| (m.to.z - rim).abs() < 1e-9)
        .map(|m| m.to.truncate())
        .collect();
    assert!(!at_rim.is_empty());
    assert!(at_rim.len() < 20_000, "{} moves at the rim", at_rim.len());
    let mut pass: Vec<DVec2> = Vec::new();
    for m in path.moves() {
        if m.role == MoveRole::Cut && (m.to.z - rim).abs() < 1e-9 {
            pass.push(m.to.truncate());
        } else if !pass.is_empty() {
            assert!(tightest_turn(&pass) < TAU, "a pass at the rim orbits");
            pass.clear();
        }
    }
}

/// Not to machine cavities, pockets with no way in from the side are left,
/// and said to be.
#[test]
fn without_cavities_closed_pockets_are_left() {
    let mut p = params();
    p.machine_cavities = false;
    let (path, report) = rough_pockets(&p);
    assert_eq!(report.passes, 0, "{report:?}");
    assert_eq!(report.entries, 0, "{report:?}");
    assert!(report.cavities >= 3, "{report:?}");
    assert!(report.cavity_mm2 > 100.0, "{report:?}");
    assert!(
        report.left_mm2 < 1.0,
        "left as asked, not missed: {report:?}"
    );
    assert!(path.moves().iter().all(|m| m.role != MoveRole::Cut));

    // Open on every side, a block is still cleared.
    let level = LevelParams::new(&p, R);
    let block = rect(0.0, 0.0, 40.0, 25.0);
    let cut = clear_level(-6.0, &rect(-8.0, -8.0, 48.0, 33.0), &block, &level).unwrap();
    assert!(!cut.passes.is_empty() && !cut.cavities && cut.left < 1.0);
}

/// A minimum cutting radius rounds the corners of where the tool may be: it
/// never comes nearer an inside corner than the blend lets it.
#[test]
fn a_minimum_cutting_radius_keeps_out_of_the_corners() {
    let square = rect(0.0, 0.0, 20.0, 10.0);
    let round = blended(&square, 2.0, 1e-3).unwrap();
    // Nearest the corner: the blend's arc, 2(sqrt 2 - 1) from it.
    assert!(!round.contains(dvec2(0.3, 0.3)));
    assert!(round.contains(dvec2(0.9, 0.9)));
    assert!(round.contains(dvec2(10.0, 0.01)), "the sides are kept");
    assert!((square.area() - round.area() - 4.0 * (4.0 - PI)).abs() < 0.05);
    // Too narrow for it, nothing is left.
    assert!(
        blended(&rect(0.0, 0.0, 20.0, 3.0), 2.0, 1e-3)
            .unwrap()
            .is_empty()
    );

    let mut p = params();
    p.min_cutting_radius = 2.0;
    let (path, report) = rough_pockets(&p);
    assert!(report.passes > 0, "{report:?}");
    // Where the tool's centre may be in the first pocket: 7 to 25 by 7 to 13.
    let corners = [
        dvec2(7.0, 7.0),
        dvec2(25.0, 7.0),
        dvec2(7.0, 13.0),
        dvec2(25.0, 13.0),
    ];
    let nearest = path
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Cut)
        .flat_map(|m| corners.map(|c| m.to.truncate().distance(c)))
        .fold(f64::INFINITY, f64::min);
    assert!(
        nearest > 2.0 * (2f64.sqrt() - 1.0) - 0.02,
        "came within {nearest} of a corner"
    );
}

/// Where the way straight between two passes runs through what stands, a
/// stay-down link goes round it, by the corners of what is clear, as short as
/// it can and no longer than the stay-down distance.
#[test]
fn a_stay_down_link_goes_round_what_stands_between() {
    let p = LevelParams::new(&params(), R);
    // Clear of the material: all of it but a block in the way, grown by the
    // tool.
    let block = rect(10.0, -5.0, 14.0, 5.0);
    let free = rect(-20.0, -20.0, 40.0, 20.0)
        .difference(&block.offset(R, 1e-3))
        .unwrap();
    let clear = |a: DVec2, b: DVec2| steer::clear_of(&block, a, b, 0.9 * R);
    let (from, to) = (dvec2(0.0, 0.0), dvec2(24.0, 0.0));
    assert!(!clear(from, to));
    let via = clearing::route(&free, from, to, 60.0, &p, |_| true, clear).unwrap();
    assert!(!via.is_empty());
    let way: Vec<DVec2> = std::iter::once(from)
        .chain(via.iter().copied())
        .chain(std::iter::once(to))
        .collect();
    let length: f64 = way.windows(2).map(|w| w[0].distance(w[1])).sum();
    assert!(way.windows(2).all(|w| clear(w[0], w[1])));
    // Round the block's end, 5 + 3 from the line: about 2 x sqrt(12^2 + 8^2).
    assert!(length < 31.0, "{length}");
    // Too far round for the distance allowed, it lifts.
    assert!(clearing::route(&free, from, to, 26.0, &p, |_| true, clear).is_none());
}

/// Rest machining's material: what an earlier pass swept is gone at its level
/// and every level above, the tool's width at each; the levels below it keep
/// it; and what is left thinner than the stock to ignore is let be.
#[test]
fn rest_material_is_what_the_operations_before_left() {
    use crate::cutter::tool::{Bullnose, Cutter};
    let stocks = [(dvec2(0.0, 0.0), dvec2(40.0, 20.0), 0.0)];
    // A 6 mm flat once along the middle at -6.
    let mut flat_path = Toolpath::new();
    flat_path.rapid_to(dvec3(-5.0, 10.0, 5.0)).unwrap();
    flat_path
        .feed_to(dvec3(-5.0, 10.0, -6.0), MoveRole::Plunge, 300.0)
        .unwrap();
    flat_path
        .feed_to(dvec3(45.0, 10.0, -6.0), MoveRole::Cut, 1000.0)
        .unwrap();
    let flat = Earlier {
        path: std::sync::Arc::new(flat_path),
        cutter: Cutter::Bullnose(Bullnose::new(R, 0.0).unwrap()),
    };
    let watch = crate::control::Watch::default();
    let area_at = |left: &[(f64, Region)], z: f64| {
        left.iter()
            .find(|(at, _)| (*at - z).abs() < 1e-9)
            .unwrap()
            .1
            .area()
    };
    let left = rest::material_at(
        &stocks,
        &[-8.0, -6.0, -3.0],
        std::slice::from_ref(&flat),
        0.0,
        0.01,
        &watch,
    )
    .unwrap();
    assert!(
        (area_at(&left, -8.0) - 800.0).abs() < 0.5,
        "below it, untouched"
    );
    assert!((area_at(&left, -6.0) - 560.0).abs() < 0.5);
    assert!((area_at(&left, -3.0) - 560.0).abs() < 0.5);

    // A 6 mm ball the same way: at its tip it cuts nothing, and wider up it.
    let ball = Earlier {
        cutter: Cutter::Bullnose(Bullnose::new(R, R).unwrap()),
        ..flat.clone()
    };
    let left = rest::material_at(&stocks, &[-6.0, -5.0, -3.0], &[ball], 0.0, 0.01, &watch).unwrap();
    let w5 = (2.0 * R - 1.0f64).sqrt();
    assert!((area_at(&left, -6.0) - 800.0).abs() < 0.5);
    let cut5 = 800.0 - area_at(&left, -5.0);
    assert!(cut5 > 0.0 && cut5 <= 40.0 * 2.0 * w5 + 0.5, "{cut5}");
    assert!((area_at(&left, -3.0) - 560.0).abs() < 0.5);

    // A second pass 5.8 mm over leaves a strip 0.2 mm wide: ignored at 0.5.
    let mut twice = flat.clone();
    for x in [45.0, -5.0] {
        std::sync::Arc::make_mut(&mut twice.path)
            .feed_to(dvec3(x, 7.0 - 0.2 - R, -6.0), MoveRole::Cut, 1000.0)
            .unwrap();
    }
    let kept = rest::material_at(
        &stocks,
        &[-6.0],
        std::slice::from_ref(&twice),
        0.0,
        0.01,
        &watch,
    )
    .unwrap();
    let let_be = rest::material_at(&stocks, &[-6.0], &[twice], 0.5, 0.01, &watch).unwrap();
    let strip = area_at(&kept, -6.0) - area_at(&let_be, -6.0);
    assert!((strip - 40.0 * 0.2).abs() < 0.5, "{strip}");
}

/// Peaks are where nothing within the tool's width stands higher, on a face
/// that faces up: a sphere's top, a block's top, a slope's top edge; never
/// its bottom.
#[test]
fn peaks_are_where_nothing_near_stands_higher() {
    use camcraft_geom::synthetic;
    let sphere = synthetic::uv_sphere(10.0, 16, 32);
    assert_eq!(peak_heights(&sphere, 0.01, R), vec![10.0]);
    let block = synthetic::cuboid(dvec3(0.0, 0.0, -5.0), dvec3(10.0, 10.0, 2.0));
    assert_eq!(peak_heights(&block, 0.01, R), vec![2.0]);
    let slope = synthetic::inclined_plane(0.5, 0.0, 0.0, 20.0);
    assert_eq!(peak_heights(&slope, 0.01, R), vec![10.0]);
}

/// The slope `z = x / 2`, from -10 to 10, roughed 6 mm a level with a 6 mm
/// flat as `p` asks.
fn rough_slope(p: &AdaptiveParams) -> (Toolpath, AdaptiveReport) {
    use crate::cutter::allowance::{Allowance, EffectiveTool};
    use crate::cutter::tool::Flat;
    use camcraft_geom::synthetic;
    let mesh = synthetic::inclined_plane(0.5, 0.0, 0.0, 20.0);
    let flat = Flat::new(R).unwrap();
    let tool = EffectiveTool::new(&flat, Allowance::new(0.0, 0.0).unwrap()).unwrap();
    let mut reach = LevelReach::new(&mesh, &tool, 0.01).unwrap();
    let mut p = p.clone();
    p.top = 10.0;
    p.bottom = -10.0;
    p.stepdown = 6.0;
    let area = rect(-30.0, -30.0, 30.0, 30.0);
    let stocks = [(dvec2(-20.0, -20.0), dvec2(20.0, 20.0), 10.0)];
    adaptive_rough(&mut reach, &area, &stocks, tool.base(), &p).unwrap()
}

/// The heights of the levels a path cuts at, highest first.
fn cut_levels(path: &Toolpath) -> Vec<f64> {
    let mut zs: Vec<f64> = path
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Cut)
        .map(|m| m.to.z)
        .collect();
    zs.sort_by(|a, b| b.total_cmp(a));
    zs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    zs
}

/// On a slope rising 1 in 2, a level 6 mm down leaves terraces 12 mm wide:
/// shallow areas add levels halfway, and halfway again, until they are no
/// wider than the most allowed, or the levels would come closer than the
/// least; each cutting only on the slope.
#[test]
fn shallow_areas_add_levels_where_the_terraces_are_wide() {
    let (plain, report) = rough_slope(&params());
    assert_eq!(report.shallow_levels, 0);
    let mut p = params();
    p.shallow = Some(Shallow {
        min_stepdown: 0.5,
        max_stepover: 2.0,
    });
    let (path, report) = rough_slope(&p);
    assert!(report.shallow_levels > 0, "{report:?}");
    let zs = cut_levels(&path);
    assert!(zs.len() > cut_levels(&plain).len(), "{zs:?}");
    // 5 mm halved three times leaves terraces 1.25 mm wide, 0.625 mm apart;
    // once more would leave levels nearer than 0.5.
    let gaps: Vec<f64> = zs.windows(2).map(|w| w[0] - w[1]).collect();
    assert!(
        gaps.iter().all(|g| *g > 0.5 - 1e-9 && *g < 1.0 + 1e-6),
        "{zs:?}"
    );
    // Each level added cuts only on the slope: where the terrace it was
    // added for is, and the tool's width beside it.
    let own = |z: f64| [5.0, 0.0, -5.0, -10.0].iter().any(|l| (l - z).abs() < 1e-9);
    for m in path
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Cut && !own(m.to.z))
    {
        // The slope stands at x = 2z; at a level halfway between two 5 mm
        // apart, the terrace runs 5 mm either side of it.
        assert!(
            (m.to.x - 2.0 * m.to.z).abs() < 12.0 + 2.0 * R + 0.5,
            "{m:?}"
        );
    }

    // Walls and a flat top with a level of its own leave no terraces.
    use crate::cutter::allowance::{Allowance, EffectiveTool};
    use crate::cutter::tool::Flat;
    use camcraft_geom::synthetic;
    let mesh = synthetic::cuboid(dvec3(0.0, 0.0, 0.0), dvec3(40.0, 20.0, 10.0));
    let flat = Flat::new(R).unwrap();
    let tool = EffectiveTool::new(&flat, Allowance::new(0.5, 0.5).unwrap()).unwrap();
    let block = |flats: Vec<f64>| {
        let mut reach = LevelReach::new(&mesh, &tool, 0.01).unwrap();
        let mut p = p.clone();
        p.top = 14.0;
        p.bottom = 0.0;
        p.flats = flats;
        let area = rect(-20.0, -20.0, 60.0, 40.0);
        let stocks = [(dvec2(-10.0, -10.0), dvec2(50.0, 30.0), 14.0)];
        adaptive_rough(&mut reach, &area, &stocks, tool.base(), &p)
            .unwrap()
            .1
    };
    assert_eq!(block(vec![10.5]).shallow_levels, 0);
    // Without, its top between two levels is a terrace too.
    assert!(block(Vec::new()).shallow_levels > 0);
}

/// A flat face too near the level above for a level of its own to be worth
/// cutting is left out, as is a fine level cutting too little depth.
#[test]
fn levels_too_thin_are_left_out() {
    let mut slabs = vec![(8.0, vec![9.0, 13.5]), (2.0, vec![5.0, 7.8])];
    levels::without_thin(&mut slabs, 14.0, 1.0, |z| z != 5.0);
    assert_eq!(slabs, vec![(8.0, vec![9.0]), (2.0, vec![5.0])]);

    let mut p = params();
    p.fine_stepdown = Some(2.0);
    let (_, all) = rough_slope(&p);
    p.min_axial_engagement = 3.0;
    let (path, fewer) = rough_slope(&p);
    assert!(fewer.fine_levels < all.fine_levels, "{fewer:?} {all:?}");
    // Levels at 5, 0, -5 and -10; none between less than 3 below one.
    for z in cut_levels(&path) {
        let above = [10.0, 5.0, 0.0, -5.0]
            .into_iter()
            .filter(|a| *a > z + 1e-9)
            .fold(f64::INFINITY, f64::min);
        let own = [5.0, 0.0, -5.0, -10.0].iter().any(|l| (l - z).abs() < 1e-9);
        assert!(own || above - z >= 3.0 - 1e-9, "{z}");
    }
}

/// Lifted, a link it stays down for rises off the level and comes down again
/// no more steeply than a link may, so it is never taken for a plunge.
#[test]
fn a_lift_height_lifts_the_links_it_stays_down_for() {
    use crate::path::toolpath::PLUNGE_RATIO;
    let way = [dvec2(0.0, 0.0), dvec2(10.0, 0.0), dvec2(10.0, 10.0)];
    let up = lifted(&way, -3.0, 1.0);
    assert_eq!(up.last(), Some(&dvec3(10.0, 10.0, -3.0)));
    assert!(up.iter().any(|q| (q.z + 2.0).abs() < 1e-9), "{up:?}");
    // Too short to rise so far and come down, lower.
    let short = lifted(&[dvec2(0.0, 0.0), dvec2(0.2, 0.0)], 0.0, 1.0);
    let top = short.iter().map(|q| q.z).fold(0.0, f64::max);
    assert!(top > 0.0 && top < 1.0, "{short:?}");

    let (flat, report) = rough_pockets(&params());
    let mut p = params();
    p.lift_height = 1.0;
    let (path, lifted_report) = rough_pockets(&p);
    assert_eq!(lifted_report.stayed_down, report.stayed_down);
    assert!(report.stayed_down > 0, "{report:?}");
    let highest = |path: &Toolpath| {
        path.moves()
            .iter()
            .filter(|m| m.role == MoveRole::Transition)
            .map(|m| m.to.z)
            .fold(f64::MIN, f64::max)
    };
    assert!(highest(&path) > highest(&flat) + 0.5);
    let start = path.moves()[0].to;
    let (_, deepest) = path.steep_cut_descents(start, PLUNGE_RATIO);
    let (_, before) = flat.steep_cut_descents(flat.moves()[0].to, PLUNGE_RATIO);
    assert!(deepest <= before + 1e-9, "{deepest} against {before}");
}

/// At the least stay-down level, links stay down only straight; and a way
/// round keeps the clearance asked for off the material.
#[test]
fn the_stay_down_level_and_clearance_say_how_links_stay_down() {
    let routed = |level: f64| {
        let mut p = params();
        p.stay_down_level = level;
        let level = LevelParams::new(&p, R);
        let block = rect(0.0, 0.0, 50.0, 40.0);
        let island = rect(20.0, 15.0, 30.0, 25.0);
        let allowed = rect(-8.0, -8.0, 58.0, 48.0)
            .difference(&island.offset(R, 1e-3))
            .unwrap();
        let cut = clear_level(-6.0, &allowed, &block.difference(&island).unwrap(), &level).unwrap();
        cut.passes
            .iter()
            .filter(|(a, _)| matches!(a, Arrive::Stay { via } if !via.is_empty()))
            .count()
    };
    assert!(routed(1.0) > 0);
    assert_eq!(routed(0.0), 0);

    let mut p = params();
    p.stay_down_clearance = 1.0;
    let p = LevelParams::new(&p, R);
    let block = rect(10.0, -5.0, 14.0, 5.0);
    let free = rect(-20.0, -20.0, 40.0, 20.0)
        .difference(&block.offset(R, 1e-3))
        .unwrap();
    let clear = |a: DVec2, b: DVec2| steer::clear_of(&block, a, b, R + 1.0);
    let (from, to) = (dvec2(0.0, 0.0), dvec2(24.0, 0.0));
    let via = clearing::route(&free, from, to, 60.0, &p, |_| true, clear).unwrap();
    assert!(!via.is_empty());
    let (lo, hi) = (dvec2(10.0, -5.0), dvec2(14.0, 5.0));
    for v in &via {
        let d = (*v - v.clamp(lo, hi)).length();
        assert!(d >= R + 1.0, "{v} is {d} from the block");
    }
}

/// With slot clearing, a closed pocket gone down into is first cut a slot
/// along its long way, through the middle, then out to the width asked for;
/// it is all cleared still, and the tool kept where it may be.
#[test]
fn slot_clearing_cuts_a_slot_along_the_middle_first() {
    let block = rect(-10.0, -10.0, 60.0, 50.0);
    let allowed = rect(3.0, 3.0, 37.0, 27.0);
    for width in [6.0, 12.0] {
        let mut p = params();
        p.slot_width = Some(width);
        let level = LevelParams::new(&p, R);
        let cut = clear_level(-6.0, &allowed, &block, &level).unwrap();
        assert_eq!(cut.slots, 1, "{width}");
        let (arrive, points) = &cut.passes[0];
        assert!(matches!(arrive, Arrive::Enter { .. }));
        // Along the pocket's long way, the whole length of where the tool
        // may be, less the width beyond the tool's.
        let out = width / 2.0 - R;
        let (a, b) = (points[1], points[2]);
        assert!((a.y - b.y).abs() < 1e-6, "{a} {b}");
        assert!((a.x.min(b.x) - (3.0 + out)).abs() < 5e-3, "{a} {b}");
        assert!((a.x.max(b.x) - (37.0 - out)).abs() < 5e-3, "{a} {b}");
        // Rings out to the width, if wider.
        let widest = points[1..]
            .iter()
            .map(|q| (q.y - a.y).abs())
            .fold(0.0, f64::max);
        assert!((widest - out).abs() < 5e-3, "{width}: {widest}");
        assert!(cut.left < 1.0, "left {}", cut.left);
        for (_, points) in &cut.passes {
            for w in points.windows(2) {
                assert!(allowed.offset(1e-3, 1e-4).contains_segment(w[0], w[1]));
            }
        }
    }
}
