//! Adaptive clearing: roughing in Z levels with the cutter's load held constant.
//!
//! Z-level roughing ([`crate::pocket_clearing`]) offsets the reachable region inward by
//! the stepover, so the first ring into full material is a full slot, and every
//! inside corner buries the tool. Adaptive clearing, after Fusion's (Freesteel's
//! Adaptive Clearing, 2004, and Stori and Wright's constant-engagement paths,
//! 2000), steers the tool instead: step by step, it turns toward or away from the
//! material until the arc of its edge in material, the engagement angle, is the
//! one the **optimal load** gives. So it can take the whole flute length down at
//! once, at a light radial load, and never slot.
//!
//! # A level at a time, in two regions
//!
//! At each level there are two regions, and every step is judged by both:
//!
//! - **Allowed**: where the tool's centre may be, the complement of
//!   [`LevelReach::forbidden`], from the model. A step that would leave it turns
//!   along its edge instead, which is how walls are left exactly the stock to
//!   leave.
//! - **Uncut**: the material still standing at the level: the stock's block, less
//!   everything the tool has swept at this level. Engagement is measured against
//!   it, exactly ([`camcraft_geom::engagement`]).
//!
//! # Paths from the model, material from exact regions
//!
//! Grids are only for stock. The path is steered by the uncut region, so that
//! region is exact polygons: the stock's box, less the exact sweep of the path
//! itself. Nothing here reads the simulated stock. So an operation run after
//! another cuts air where that one cleared: rest material from earlier operations
//! is still to come.
//!
//! # Getting in, and moving on
//!
//! Where the tool can stand clear of material, beside the block or in what it
//! has cleared, a pass starts at the edge of the material, with the material on
//! its climbing side. Where it cannot, as in a closed pocket, it goes down a helix
//! in the widest place, and spirals out. A pass ends when it runs out of material,
//! or would overload the tool; the next starts at the nearest edge of what is
//! left, reached without lifting where the way is clear.
//!
//! # Levels, and flat areas
//!
//! Levels are a stepdown apart, from the top. Each is cleared on its own: the
//! slab above it was untouched when it is cut. Each flat face of the part adds a
//! level on it (raised by the stock to leave down), cut after the level below it,
//! so it only removes the terrace that level left over the face.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use camcraft_geom::arc::Arc;
use camcraft_geom::contour::{Contour, Region};
use camcraft_geom::engagement::engaged;
use camcraft_geom::mesh::TriMesh;
use glam::{DVec2, dvec3};
use rayon::prelude::*;

use crate::reach::LevelReach;
use crate::strategy::{StrategyError, check, slabs};
use crate::tool::{Cutter, ToolProfile};
use crate::toolpath::{MoveRole, Toolpath};

/// Stock standing less than this above a level is not material to cut there, in
/// millimetres.
const MATERIAL_THICKNESS: f64 = 1.0e-3;

/// The most a pass turns in one step, toward or away from the material, in
/// radians.
const MAX_TURN: f64 = FRAC_PI_4;

/// How many times a step's heading is refined toward the load asked for.
const SEARCH_ITERATIONS: usize = 14;

/// A level gives up after this many passes: bounded by construction, so a
/// pathological region cannot run away.
const MAX_PASSES: usize = 50_000;

/// A level gives up after this many steps, in all its passes together.
const MAX_STEPS: usize = 20_000_000;

/// A level gives up after this many passes in a row that clear nothing.
const MAX_FUTILE: usize = 64;

/// Which way the passes cut.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Direction {
    /// Every pass climbing. Fusion's default.
    #[default]
    Climb,
    /// Every pass conventional.
    Conventional,
    /// Climbing, or conventional where that saves going back: at the end of a
    /// pass, the tool turns round and cuts back rather than linking.
    BothWays,
}

/// How to clear the material.
#[derive(Debug, Clone, PartialEq)]
pub struct AdaptiveParams {
    /// Deepest each level may be, in millimetres.
    pub stepdown: f64,
    /// The radial width of cut the tool is held to, in millimetres: Fusion's
    /// optimal load.
    pub optimal_load: f64,
    /// Cutting feed, in mm/min.
    pub feed: f64,
    /// Highest level to cut at. Usually the top of the stock.
    pub top: f64,
    /// Lowest level to cut at. Usually the bottom of the part.
    pub bottom: f64,
    /// Chordal tolerance, in millimetres.
    pub tolerance: f64,
    /// Whether climbing puts the material on the tool's right, as it does with
    /// a clockwise spindle.
    pub climb_right: bool,
    /// Which way the passes cut.
    pub direction: Direction,
    /// The radius of the helix a closed area is entered down, as the tool's
    /// centre goes, in millimetres; zero to plunge.
    pub helix_radius: f64,
    /// The longest link made without lifting, in millimetres.
    pub stay_down: f64,
    /// How steeply a helix goes down, in radians from level.
    pub ramp_angle: f64,
    /// The feed down a helix, in mm/min.
    pub ramp_feed: f64,
    /// The most a helix goes down in one turn, in millimetres; `None` for no
    /// limit but the angle.
    pub ramp_stepdown: Option<f64>,
    /// How far above the material a helix starts, in millimetres.
    pub safe_distance: f64,
    /// Extra levels, each on a flat face of the part, raised by the stock to
    /// leave down: cut after the level below each, over what it left.
    pub flats: Vec<f64>,
}

impl Default for AdaptiveParams {
    fn default() -> Self {
        Self {
            stepdown: 6.0,
            optimal_load: 0.6,
            feed: 1500.0,
            top: 0.0,
            bottom: -10.0,
            tolerance: 0.01,
            climb_right: true,
            direction: Direction::Climb,
            helix_radius: 2.7,
            stay_down: 30.0,
            ramp_angle: 2f64.to_radians(),
            ramp_feed: 900.0,
            ramp_stepdown: None,
            safe_distance: 2.0,
            flats: Vec::new(),
        }
    }
}

/// What adaptive clearing did.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AdaptiveReport {
    /// Levels that produced cutting, flat ones included.
    pub levels: u64,
    /// Of them, levels on flat faces.
    pub flat_levels: u64,
    /// Passes emitted.
    pub passes: u64,
    /// Links between passes made without lifting.
    pub stayed_down: u64,
    /// Closed areas entered down a helix, or a plunge.
    pub entries: u64,
    /// Of them, entered down a helix.
    pub helixes: u64,
    /// Areas with no room to go down into, left uncut.
    pub no_room: u64,
    /// Material the tool could reach but did not clear, in square millimetres,
    /// summed over the levels.
    pub left_mm2: f64,
    /// Passes that ended because the next step would overload the tool.
    pub overloads: u64,
}

/// The heights of the part's flat faces that face up: where Fusion's flat area
/// detection adds a level.
pub fn flat_heights(mesh: &TriMesh, tolerance: f64) -> Vec<f64> {
    // Within half a degree of level.
    let level = (0.5f64).to_radians().cos();
    let mut zs: Vec<f64> = (0..mesh.len())
        .filter_map(|i| mesh.triangle(i))
        .filter_map(|[a, b, c]| {
            let n = (b - a).cross(c - a).try_normalize()?;
            (n.z >= level).then_some((a.z + b.z + c.z) / 3.0)
        })
        .collect();
    zs.sort_by(f64::total_cmp);
    let mut out: Vec<f64> = Vec::new();
    for z in zs {
        match out.last_mut() {
            // One face, tessellated at a rounding error's difference in height,
            // is one level: the highest of it, so the floor is never cut into.
            Some(last) if z - *last <= tolerance * 0.1 => *last = z,
            _ => out.push(z),
        }
    }
    out
}

/// Clear material in Z levels, holding the tool's engagement constant.
///
/// `reach` gives where the tool may not be, from the model. `area` is where its
/// centre may go: past the stock's sides, so passes can start outside it, and
/// within the operation's boundary. `stocks` are the blocks of material, each
/// its footprint's corners and its top. `cutter` is the real tool, whose width
/// at each height says how much of a terrace a level below cleared.
pub fn adaptive_rough(
    reach: &mut LevelReach<'_>,
    area: &Region,
    stocks: &[(DVec2, DVec2, f64)],
    cutter: &Cutter,
    params: &AdaptiveParams,
) -> Result<(Toolpath, AdaptiveReport), StrategyError> {
    check("stepdown", params.stepdown)?;
    check("optimal load", params.optimal_load)?;
    check("feed", params.feed)?;
    check("tolerance", params.tolerance)?;
    if params.top <= params.bottom || !params.top.is_finite() || !params.bottom.is_finite() {
        return Err(StrategyError::BadParameter {
            name: "top",
            value: params.top,
        });
    }
    if !(params.helix_radius.is_finite() && params.helix_radius >= 0.0) {
        return Err(StrategyError::BadParameter {
            name: "helix radius",
            value: params.helix_radius,
        });
    }
    let radius = reach.cutter_radius();
    check("tool radius", radius)?;

    // Levels from the top down, a stepdown apart, each flat level in the slab of
    // the one just below it, cut after it from the bottom up.
    let slabs = slabs(params.top, params.bottom, params.stepdown, &params.flats);
    let rough: Vec<f64> = slabs.iter().map(|&(z, _)| z).collect();

    // Where the tool may be at every level, from the top: the shadows are
    // cheapest asked for in descending order.
    let mut all: Vec<f64> = slabs
        .iter()
        .flat_map(|(z, flats)| std::iter::once(*z).chain(flats.iter().copied()))
        .collect();
    all.sort_by(|a, b| b.total_cmp(a));
    let mut allowed: Vec<(f64, Region)> = Vec::with_capacity(all.len());
    for &z in &all {
        let forbidden = reach.forbidden(z)?;
        allowed.push((z, area.difference(&forbidden)?));
    }
    let allowed_at = |z: f64| {
        allowed
            .iter()
            .find(|(at, _)| (at - z).abs() < 1e-9)
            .map(|(_, r)| r)
    };

    let level = LevelParams::new(params, radius);
    // Slabs depend on nothing but their own regions, so they are worked in
    // parallel and emitted in order: the result is the same on any number of
    // cores.
    let worked: Vec<Result<Vec<LevelCut>, StrategyError>> = slabs
        .par_iter()
        .map(|(z, flats)| {
            let mut cuts = Vec::with_capacity(1 + flats.len());
            let Some(region) = allowed_at(*z) else {
                return Ok(cuts);
            };
            let uncut = material(stocks, *z);
            let first = clear_level(*z, region, &uncut, &level)?;
            // What the levels of this slab have swept, and where, for the flat
            // levels above them to start from.
            let mut below = vec![(first.z, first.swept(), first.helixes.clone())];
            cuts.push(first);
            for &f in flats {
                let Some(region) = allowed_at(f) else {
                    continue;
                };
                let mut uncut = Tiles::new(&material(stocks, f), 16.0 * level.radius)?;
                for (at, lines, discs) in &below {
                    // The real tool's width that far above its tip.
                    let wide = cutter.radius_at(f - at);
                    if wide <= 0.0 {
                        continue;
                    }
                    // A little at a time, as the level itself was cleared: the
                    // whole sweep at once is a very large polygon.
                    for line in lines {
                        for chunk in line.chunks(48).enumerate().map(|(k, c)| {
                            // Each chunk from the last point of the one before.
                            let from = (k * 48).saturating_sub(1);
                            line.get(from..(k * 48 + c.len()).min(line.len()))
                                .unwrap_or(c)
                        }) {
                            uncut.subtract(&Region::around_polylines(
                                &[chunk.to_vec()],
                                wide,
                                level.arc,
                            ))?;
                        }
                    }
                    for &(c, r) in discs {
                        if let Some(d) = disc(c, r + wide, level.arc) {
                            uncut.subtract(&Region::union_of(&[d])?)?;
                        }
                    }
                }
                let uncut = uncut.whole()?;
                let cut = clear_level(f, region, &uncut, &level)?;
                below.push((cut.z, cut.swept(), cut.helixes.clone()));
                cuts.push(cut);
            }
            Ok(cuts)
        })
        .collect();

    let mut path = Toolpath::new();
    let mut report = AdaptiveReport::default();
    for slab in worked {
        for cut in slab? {
            report.left_mm2 += cut.left;
            report.no_room += cut.no_room;
            report.entries += cut.helixes.len() as u64;
            report.overloads += cut.overloads;
            if cut.passes.is_empty() {
                continue;
            }
            report.levels += 1;
            if !rough.iter().any(|z| (z - cut.z).abs() < 1e-9) {
                report.flat_levels += 1;
            }
            // The material stands up to the level above, or the top.
            let above = rough
                .iter()
                .rev()
                .find(|&&z| z > cut.z + 1e-9)
                .copied()
                .unwrap_or(params.top);
            for (arrive, points) in &cut.passes {
                let Some(first) = points.first() else {
                    continue;
                };
                let first = dvec3(first.x, first.y, cut.z);
                match *arrive {
                    Arrive::Link => path.feed_to(first, MoveRole::Link, params.feed)?,
                    Arrive::Stay => {
                        report.stayed_down += 1;
                        path.feed_to(first, MoveRole::Transition, params.feed)?;
                    }
                    Arrive::Enter { centre, .. } => {
                        match helix(
                            centre,
                            first,
                            above + params.safe_distance,
                            params,
                            level.right,
                        ) {
                            Some((start, turns)) => {
                                report.helixes += 1;
                                path.feed_to(start, MoveRole::Link, params.feed)?;
                                for (to, arc) in turns {
                                    path.arc_to(to, arc, MoveRole::Ramp, params.ramp_feed)?;
                                }
                            }
                            // No helix: the linker goes straight down.
                            None => path.feed_to(first, MoveRole::Link, params.feed)?,
                        }
                    }
                }
                for p in points.iter().skip(1) {
                    path.feed_to(dvec3(p.x, p.y, cut.z), MoveRole::Cut, params.feed)?;
                }
                report.passes += 1;
            }
        }
    }
    Ok((path, report))
}

/// A helix down onto `end` about `centre`, from `top`, as adaptive clearing
/// goes down into material: no steeper than the ramp angle, nor than the ramp's
/// most in one turn, counter-clockwise when `ccw`, then once round level at the
/// bottom, as Fusion's ends. Where it starts, and its turns. `None` for a helix
/// with no radius, or nowhere to go down.
fn helix(
    centre: DVec2,
    end: glam::DVec3,
    top: f64,
    p: &AdaptiveParams,
    ccw: bool,
) -> Option<(glam::DVec3, Vec<(glam::DVec3, Arc)>)> {
    let radius = end.truncate().distance(centre);
    if radius <= 1e-6 || top <= end.z + 1e-9 || !(p.ramp_angle > 0.0 && p.ramp_angle < FRAC_PI_2) {
        return None;
    }
    let lap = TAU * radius;
    let angle = p
        .ramp_stepdown
        .map_or(p.ramp_angle, |d| p.ramp_angle.min((d / lap).atan()));
    let turn = if ccw { 1.0 } else { -1.0 };
    let total = (top - end.z) / (radius * angle.tan());
    let back = end.truncate() - centre;
    let start = (centre + DVec2::from_angle(-turn * total).rotate(back)).extend(top);
    let n = camcraft_geom::steps::bounded_count((total / TAU - 1e-9).ceil().max(1.0));
    let axis = centre.extend(end.z);
    let mut turns: Vec<(glam::DVec3, Arc)> = (1..=n)
        .map(|k| {
            let f = f64::from(k) / f64::from(n);
            let p = if k == n {
                end
            } else {
                (centre + DVec2::from_angle(turn * total * f).rotate(start.truncate() - centre))
                    .extend(top + (end.z - top) * f)
            };
            (
                p,
                Arc {
                    centre: axis,
                    axis: glam::DVec3::Z,
                    sweep: turn * total / f64::from(n),
                },
            )
        })
        .collect();
    turns.push((
        end,
        Arc {
            centre: axis,
            axis: glam::DVec3::Z,
            sweep: turn * TAU,
        },
    ));
    Some((start, turns))
}

/// The material at level `z`: every block whose top stands above it.
fn material(stocks: &[(DVec2, DVec2, f64)], z: f64) -> Region {
    let rings: Vec<Contour> = stocks
        .iter()
        .filter(|(_, _, top)| *top > z + MATERIAL_THICKNESS)
        .filter_map(|&(lo, hi, _)| Region::rectangle(lo, hi).rings().first().cloned())
        .collect();
    Region::union_of(&rings).unwrap_or_default()
}

/// A disc as a polygon whose corners lie on its circle: within it, so a disc
/// taken as cleared never claims more than was.
fn disc(centre: DVec2, radius: f64, tolerance: f64) -> Option<Contour> {
    if !(radius > 0.0 && radius.is_finite()) {
        return None;
    }
    // Chords that fall short of the circle by at most the tolerance.
    let half = (1.0 - tolerance.min(radius) / radius)
        .clamp(-1.0, 1.0)
        .acos();
    let n = (PI / half.max(1e-3)).ceil().clamp(8.0, 4096.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = n as u32;
    Contour::new(
        (0..n)
            .map(|k| centre + DVec2::from_angle(TAU * f64::from(k) / f64::from(n)) * radius)
            .collect(),
    )
}

/// What a level's clearing is held to, worked out once.
#[derive(Debug, Clone)]
struct LevelParams {
    /// The real tool's radius: what it cuts.
    radius: f64,
    /// The engagement angle asked for.
    target: f64,
    /// Close enough to it.
    within: f64,
    /// The most a pass may take on before it ends.
    limit: f64,
    /// Less than this is cutting air.
    air: f64,
    /// Longest and shortest step.
    longest: f64,
    shortest: f64,
    /// Material narrower than twice this is not worth a pass.
    sliver: f64,
    /// Arc tolerance for the regions derived from the model.
    arc: f64,
    /// Arc tolerance for what the tool has swept: coarser, as the material is
    /// only ever measured against it, and a finer one makes every region after
    /// it slower to work with.
    swept_arc: f64,
    /// Arc tolerance for finding where passes start, which only needs to be
    /// near: the steering is exact.
    coarse: f64,
    /// Whether the material is on the tool's right, cutting the main way.
    right: bool,
    both_ways: bool,
    helix_radius: f64,
    stay_down: f64,
}

impl LevelParams {
    fn new(p: &AdaptiveParams, radius: f64) -> Self {
        let angle = |width: f64| (1.0 - (width / radius).min(2.0)).clamp(-1.0, 1.0).acos();
        let target = angle(p.optimal_load);
        let right = match p.direction {
            Direction::Climb | Direction::BothWays => p.climb_right,
            Direction::Conventional => !p.climb_right,
        };
        Self {
            radius,
            target,
            within: (target * 0.03).max(2e-3),
            limit: angle(2.0 * p.optimal_load).max(target * 1.25),
            air: (target * 0.02).max(1e-3),
            longest: (radius * 0.2).max(p.tolerance),
            shortest: (radius * 0.02).max(p.tolerance * 0.2).min(radius * 0.2),
            sliver: p.tolerance,
            arc: p.tolerance * 0.1,
            swept_arc: p.tolerance * 0.5,
            coarse: (p.tolerance * 0.5).max(radius * 0.005),
            right,
            both_ways: p.direction == Direction::BothWays,
            helix_radius: p.helix_radius,
            stay_down: p.stay_down,
        }
    }
}

/// How a pass is arrived at.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Arrive {
    /// However the linker sees fit: lifting clear, and going down, by a helix
    /// where it starts in material.
    Link,
    /// Straight across at the level, through what is cleared.
    Stay,
    /// Down into material with no way in from the side: a helix of `radius`
    /// about `centre`, as the tool's centre goes, ending where the pass
    /// starts; straight down for none.
    Enter {
        /// The helix's axis.
        centre: DVec2,
        /// Its radius.
        radius: f64,
    },
}

/// One level's passes.
#[derive(Debug, Clone)]
struct LevelCut {
    z: f64,
    passes: Vec<(Arrive, Vec<DVec2>)>,
    /// Where it went down into closed areas, and the radius of each helix.
    helixes: Vec<(DVec2, f64)>,
    left: f64,
    no_room: u64,
    overloads: u64,
}

impl LevelCut {
    /// Every line the tool's centre ran along at this level.
    fn swept(&self) -> Vec<Vec<DVec2>> {
        let mut lines: Vec<Vec<DVec2>> = Vec::with_capacity(self.passes.len() * 2);
        let mut at: Option<DVec2> = None;
        for (arrive, points) in &self.passes {
            if let (Arrive::Stay, Some(a), Some(&b)) = (arrive, at, points.first()) {
                lines.push(vec![a, b]);
            }
            lines.push(points.clone());
            at = points.last().copied();
        }
        lines
    }
}

/// Which side of the tool the material is kept on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Right,
    Left,
}

impl Side {
    /// Which way turning toward the material rotates, counter-clockwise positive.
    fn toward(self) -> f64 {
        match self {
            Side::Right => -1.0,
            Side::Left => 1.0,
        }
    }
}

/// Clear one level.
fn clear_level(
    z: f64,
    allowed: &Region,
    uncut: &Region,
    p: &LevelParams,
) -> Result<LevelCut, StrategyError> {
    let mut c = Clearing::new(allowed, uncut, p)?;
    c.run()?;
    Ok(LevelCut {
        z,
        passes: c.passes,
        helixes: c.helixes,
        left: c.left,
        no_room: c.no_room,
        overloads: c.overloads,
    })
}

/// A window of the regions around the tool, which every step reads.
#[derive(Debug, Clone, Default)]
struct Window {
    lo: DVec2,
    hi: DVec2,
    uncut: Region,
    allowed: Region,
}

/// A region cut into square tiles, so what is near a point is found, and
/// changed, without touching the rest: a level of a whole sheet has a great
/// deal of rest.
#[derive(Debug, Clone)]
struct Tiles {
    lo: DVec2,
    size: f64,
    nx: usize,
    ny: usize,
    cells: Vec<Region>,
}

impl Tiles {
    /// `region` in tiles of `size`.
    // Tile counts are clamped to 4096 a side, so the conversions are exact.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn new(region: &Region, size: f64) -> Result<Self, StrategyError> {
        let Some((lo, hi)) = region.bounds() else {
            return Ok(Self {
                lo: DVec2::ZERO,
                size,
                nx: 0,
                ny: 0,
                cells: Vec::new(),
            });
        };
        // Bounded: a tile is at least a tool wide, and the count is clamped.
        let count = |extent: f64| ((extent / size).ceil().clamp(1.0, 4096.0)) as usize;
        let (nx, ny) = (count(hi.x - lo.x), count(hi.y - lo.y));
        let mut cells = Vec::with_capacity(nx * ny);
        for j in 0..ny {
            // A row at a time, so each tile is cut from a row, not the whole.
            let (y0, y1) = (lo.y + size * j as f64, lo.y + size * (j + 1) as f64);
            let row = region.intersection(&Region::rectangle(
                DVec2::new(lo.x - 1.0, y0),
                DVec2::new(hi.x + 1.0, y1),
            ))?;
            for i in 0..nx {
                let (x0, x1) = (lo.x + size * i as f64, lo.x + size * (i + 1) as f64);
                cells.push(
                    row.intersection(&Region::rectangle(DVec2::new(x0, y0), DVec2::new(x1, y1)))?,
                );
            }
        }
        Ok(Self {
            lo,
            size,
            nx,
            ny,
            cells,
        })
    }

    /// The tiles overlapping the box from `lo` to `hi`, as column and row ranges.
    fn range(&self, lo: DVec2, hi: DVec2) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
        if self.nx == 0 || self.ny == 0 {
            return (0..0, 0..0);
        }
        // Clamped into the tiles, so the narrowing is exact.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let cell = |v: f64, n: usize| (v / self.size).floor().clamp(0.0, (n - 1) as f64) as usize;
        let (a, b) = (lo - self.lo, hi - self.lo);
        (
            cell(a.x, self.nx)..cell(b.x, self.nx) + 1,
            cell(a.y, self.ny)..cell(b.y, self.ny) + 1,
        )
    }

    /// What of the region lies in the box from `lo` to `hi`.
    fn window(&self, lo: DVec2, hi: DVec2) -> Result<Region, StrategyError> {
        let (xs, ys) = self.range(lo, hi);
        let mut pieces = Vec::new();
        for j in ys {
            for i in xs.clone() {
                if let Some(c) = self.cells.get(j * self.nx + i).filter(|c| !c.is_empty()) {
                    pieces.push(c.clone());
                }
            }
        }
        Region::union_all(&pieces)
            .and_then(|r| r.intersection(&Region::rectangle(lo, hi)))
            .map_err(StrategyError::from)
    }

    /// The whole region.
    fn whole(&self) -> Result<Region, StrategyError> {
        Region::union_all(&self.cells).map_err(StrategyError::from)
    }

    /// Take `cut` out of every tile it touches.
    fn subtract(&mut self, cut: &Region) -> Result<(), StrategyError> {
        let Some((lo, hi)) = cut.bounds() else {
            return Ok(());
        };
        let (xs, ys) = self.range(lo, hi);
        for j in ys {
            for i in xs.clone() {
                if let Some(c) = self.cells.get_mut(j * self.nx + i)
                    && !c.is_empty()
                {
                    *c = c.difference(cut)?;
                }
            }
        }
        Ok(())
    }
}

/// A level being cleared.
struct Clearing<'a> {
    p: &'a LevelParams,
    allowed: &'a Region,
    /// Where the tool may be, in tiles.
    room: Tiles,
    /// A little inside it, off its edges, in tiles: where the edge of the free
    /// region is material's, not a wall's.
    inner: Tiles,
    /// Material still standing, less what `trail` has swept.
    uncut: Tiles,
    /// Swept since the material was last brought up to date, the last point
    /// where the tool is.
    trail: Vec<DVec2>,
    trail_length: f64,
    window: Option<Window>,
    passes: Vec<(Arrive, Vec<DVec2>)>,
    /// Which side of each pass its material is on.
    sides: Vec<Side>,
    helixes: Vec<(DVec2, f64)>,
    /// Where passes have started that cut nothing.
    dead: Vec<DVec2>,
    steps: usize,
    left: f64,
    no_room: u64,
    overloads: u64,
}

/// What a part of a level looks like, between passes.
struct Situation {
    /// The box it covers, and within which it is exact: what is further in than
    /// the tool and a margin from its edges.
    lo: DVec2,
    hi: DVec2,
    exact: f64,
    /// Where the tool may be.
    allowed: Region,
    /// A little inside it.
    inner: Region,
    /// Material still worth a pass.
    remaining: Region,
    /// Where the tool stands clear of it.
    free: Region,
}

impl Situation {
    /// Whether `p` is far enough inside for what is said of it to be so.
    fn knows(&self, p: DVec2) -> bool {
        p.cmpge(self.lo + DVec2::splat(self.exact)).all()
            && p.cmple(self.hi - DVec2::splat(self.exact)).all()
    }
}

impl<'a> Clearing<'a> {
    fn new(allowed: &'a Region, uncut: &Region, p: &'a LevelParams) -> Result<Self, StrategyError> {
        // What the tool cannot reach from where it may be, it never touches.
        let reach = allowed.offset(p.radius, p.arc);
        let uncut = uncut.intersection(&reach)?;
        let size = 16.0 * p.radius;
        Ok(Self {
            p,
            allowed,
            room: Tiles::new(allowed, size)?,
            inner: Tiles::new(&allowed.offset(-2.0 * p.sliver, p.coarse), size)?,
            uncut: Tiles::new(&uncut, size)?,
            trail: Vec::new(),
            trail_length: 0.0,
            window: None,
            passes: Vec::new(),
            sides: Vec::new(),
            helixes: Vec::new(),
            dead: Vec::new(),
            steps: 0,
            left: 0.0,
            no_room: 0,
            overloads: 0,
        })
    }

    fn run(&mut self) -> Result<(), StrategyError> {
        let mut at: Option<DVec2> = None;
        let mut futile = 0;
        for _ in 0..MAX_PASSES {
            if self.steps >= MAX_STEPS || futile >= MAX_FUTILE {
                break;
            }
            // Near where the tool is, first, further, and only where nothing is
            // near, everywhere.
            let mut near = None;
            if let Some(from) = at {
                for half in [8.0 * self.p.radius, 32.0 * self.p.radius] {
                    let s = self.situation_near(from, half)?;
                    if let Some(found) = self.next_start(&s, Some(from))? {
                        near = Some((s, found));
                        break;
                    }
                }
            }
            let (s, found) = match near {
                Some((s, found)) => (s, Some(found)),
                None => {
                    let s = self.situation_everywhere()?;
                    if s.remaining.is_empty() {
                        break;
                    }
                    let found = self.next_start(&s, at)?;
                    (s, found)
                }
            };
            let (start, dir, side, arrive, entered) = match found {
                Some((start, dir, side)) => {
                    let arrive = match at {
                        Some(from) if self.stays_down(&s, from, start) => Arrive::Stay,
                        _ => Arrive::Link,
                    };
                    (start, dir, side, arrive, false)
                }
                None => match self.enter(&s)? {
                    Some((start, dir, side, arrive)) => (start, dir, side, arrive, true),
                    None => {
                        self.no_room += 1;
                        break;
                    }
                },
            };
            let points = self.pass(start, dir, side, entered)?;
            if points.len() < 2 {
                futile += 1;
                self.dead.push(start);
                continue;
            }
            futile = 0;
            at = points.last().copied();
            self.passes.push((arrive, points));
            self.sides.push(side);
        }
        self.left = self.situation_everywhere()?.remaining.area();
        self.stand_off();
        Ok(())
    }

    /// Where the tool lifts to link to a pass, start it a little off the
    /// material, and where it lifts after one, end it so: the linker comes down
    /// at rapid, and goes up, only where the tool is clear of material, and a
    /// pass starts and ends against it. A third of the tool is clear of any
    /// cell the simulator has, whose pitch is at most a quarter of it.
    fn stand_off(&mut self) {
        let off = self.p.radius / 3.0;
        let n = self.passes.len();
        for k in 0..n {
            let side = self.sides.get(k).copied().unwrap_or(Side::Right);
            // Away from the material is a quarter turn the other way from it.
            let away = |d: DVec2| match side {
                Side::Right => d.perp(),
                Side::Left => -d.perp(),
            };
            let lifts_after = self
                .passes
                .get(k + 1)
                .is_none_or(|(a, _)| *a != Arrive::Stay);
            let Some((arrive, points)) = self.passes.get_mut(k) else {
                continue;
            };
            if *arrive == Arrive::Link
                && let (Some(&a), Some(&b)) = (points.first(), points.get(1))
                && let Some(d) = (b - a).try_normalize()
            {
                let before = a + away(d) * off;
                if self.allowed.contains_segment(before, a) {
                    points.insert(0, before);
                }
            }
            if lifts_after
                && let (Some(&b), Some(&a)) = (
                    points.last(),
                    points.len().checked_sub(2).and_then(|i| points.get(i)),
                )
                && let Some(d) = (b - a).try_normalize()
            {
                let after = b + away(d) * off;
                if self.allowed.contains_segment(b, after) {
                    points.push(after);
                }
            }
        }
    }

    /// The level as it stands everywhere.
    fn situation_everywhere(&mut self) -> Result<Situation, StrategyError> {
        self.flush()?;
        let uncut = self.uncut.whole()?;
        let (lo, hi) = self
            .allowed
            .bounds()
            .map_or((DVec2::ZERO, DVec2::ZERO), |(lo, hi)| {
                let grow = DVec2::splat(self.p.radius + 1.0);
                (lo - grow, hi + grow)
            });
        let inner = self.inner.whole()?;
        self.situation(lo, hi, self.allowed.clone(), inner, &uncut, 0.0)
    }

    /// The level as it stands in a box `half` each way of `at`.
    fn situation_near(&mut self, at: DVec2, half: f64) -> Result<Situation, StrategyError> {
        self.flush()?;
        let (lo, hi) = (at - DVec2::splat(half), at + DVec2::splat(half));
        let uncut = self.uncut.window(lo, hi)?;
        let allowed = self.room.window(lo, hi)?;
        let inner = self.inner.window(lo, hi)?;
        // What lies within the tool and a margin of a point is all that is said
        // of it.
        let exact = self.p.radius + 4.0 * self.p.sliver + self.p.coarse;
        self.situation(lo, hi, allowed, inner, &uncut, exact)
    }

    fn situation(
        &self,
        lo: DVec2,
        hi: DVec2,
        allowed: Region,
        inner: Region,
        uncut: &Region,
        exact: f64,
    ) -> Result<Situation, StrategyError> {
        // Shrunk, so slivers a rounding error wide are not passes.
        let remaining = uncut.offset(-self.p.sliver, self.p.coarse);
        // A radius clear of the material, grown back by what it was shrunk.
        let free =
            allowed.difference(&remaining.offset(self.p.radius + self.p.sliver, self.p.coarse))?;
        Ok(Situation {
            lo,
            hi,
            exact,
            allowed,
            inner,
            remaining,
            free,
        })
    }

    /// Where the next pass starts, which way, and with the material on which
    /// side: the nearest place the tool stands clear of the material and
    /// touching it, where a stretch of the material's edge begins, heading along
    /// it with the material on the cutting side. `None` where there is nowhere
    /// so in what `s` knows of, and nearer `from` than it knows all round.
    ///
    /// Beginnings, not just anywhere along an edge: a pass started part way along
    /// runs into whatever ends the edge behind it, a wall or the part, and has
    /// nowhere to go.
    fn next_start(
        &self,
        s: &Situation,
        from: Option<DVec2>,
    ) -> Result<Option<(DVec2, DVec2, Side)>, StrategyError> {
        // The free region's edge is where the tool may go no further, or where
        // it touches material: off the edge of where it may be, it is the
        // material's.
        let touching = &s.inner;
        // Each ring as a closed polyline, from a corner off the material where
        // it has one, so no stretch along it is split where it starts.
        let loops: Vec<Vec<DVec2>> = s
            .free
            .rings()
            .iter()
            .map(|ring| {
                let pts = ring.points();
                let k = pts
                    .iter()
                    .take(64)
                    .position(|p| !touching.contains(*p))
                    .unwrap_or(0);
                let mut l: Vec<DVec2> = pts
                    .iter()
                    .skip(k)
                    .chain(pts.iter().take(k))
                    .copied()
                    .collect();
                if let Some(&first) = l.first() {
                    l.push(first);
                }
                l
            })
            .collect();
        let runs = touching.clip_polylines(&loops)?;
        // Nearer than the edge of what it knows, the nearest it knows is the
        // nearest there is.
        let within = match from {
            Some(f) if s.exact > 0.0 => {
                let edge = (f - s.lo).min(s.hi - f).min_element();
                Some(edge - s.exact)
            }
            _ => None,
        };
        let from = from.or_else(|| self.allowed.bounds().map(|b| b.0));
        let main = if self.p.right {
            Side::Right
        } else {
            Side::Left
        };
        let other = if main == Side::Right {
            Side::Left
        } else {
            Side::Right
        };
        let mut best: Option<(f64, DVec2, DVec2, Side)> = None;
        let mut consider = |p: DVec2, towards: DVec2, side: Side| {
            if !s.knows(p)
                || self
                    .dead
                    .iter()
                    .any(|d| d.distance(p) < self.p.radius * 0.25)
            {
                return;
            }
            let Some(dir) = (towards - p).try_normalize() else {
                return;
            };
            let d = from.map_or(0.0, |f| f.distance(p));
            if within.is_some_and(|w| d > w) {
                return;
            }
            if best.as_ref().is_none_or(|b| d < b.0 - 1e-9) {
                best = Some((d, p, dir, side));
            }
        };
        // Free is on a ring's left, so the material is on the right going its
        // way: a run begins at its first point going its way, and, cutting the
        // other way, at its last going back.
        for run in &runs {
            let (Some(&a), Some(&b)) = (run.first(), run.get(1)) else {
                continue;
            };
            let (Some(&z), Some(&y)) = (
                run.last(),
                run.len().checked_sub(2).and_then(|k| run.get(k)),
            ) else {
                continue;
            };
            let (fore, back) = if main == Side::Right {
                ((a, b), (z, y))
            } else {
                ((z, y), (a, b))
            };
            consider(fore.0, fore.1, main);
            if self.p.both_ways {
                consider(back.0, back.1, other);
            }
        }
        Ok(best.map(|(_, p, dir, side)| (p, dir, side)))
    }

    /// Whether the way from `from` to `to` may be made at the level: short
    /// enough, clear of the part, and through what is cleared.
    fn stays_down(&self, s: &Situation, from: DVec2, to: DVec2) -> bool {
        if from.distance(to) > self.p.stay_down || !s.knows(from) || !s.knows(to) {
            return false;
        }
        // Where the passes end and start the tool only grazes the material, so
        // it may come a little into the tool's reach.
        s.allowed.contains_segment(from, to)
            && clear_of(&s.remaining, from, to, self.p.radius * 0.9)
    }

    /// Go down into material there is no way into from the side: a helix, or a
    /// plunge, at the widest place there is room for one, cleared as it goes
    /// down. `None` where there is room nowhere.
    fn enter(
        &mut self,
        s: &Situation,
    ) -> Result<Option<(DVec2, DVec2, Side, Arrive)>, StrategyError> {
        let sizes: &[f64] = if self.p.helix_radius > 0.0 {
            // As the linker tries them.
            &[1.0, 0.5, 0.25]
        } else {
            &[0.0]
        };
        let side = if self.p.right {
            Side::Right
        } else {
            Side::Left
        };
        let dead: Vec<Contour> = self
            .dead
            .iter()
            .filter_map(|&d| disc(d, self.p.radius * 0.25, self.p.arc))
            .collect();
        let dead = Region::union_of(&dead)?;
        for &k in sizes {
            let r = self.p.helix_radius * k;
            let room = s
                .allowed
                .offset(-r, self.p.arc)
                .intersection(&s.remaining)
                .and_then(|room| room.difference(&dead))?;
            let Some(centre) = deepest(&room, self.p.arc) else {
                continue;
            };
            // The helix ends where the pass begins, and turns the way it climbs,
            // with the tool's centre on its circle: counter-clockwise for material
            // on the right.
            let heading = DVec2::X;
            let aside = match side {
                Side::Right => heading.perp(),
                Side::Left => -heading.perp(),
            };
            let start = centre - aside * r;
            if let Some(d) = disc(centre, r + self.p.radius, self.p.arc) {
                self.uncut.subtract(&Region::union_of(&[d])?)?;
                self.window = None;
            }
            self.helixes.push((centre, r));
            return Ok(Some((
                start,
                heading,
                side,
                Arrive::Enter { centre, radius: r },
            )));
        }
        Ok(None)
    }

    /// Take what the trail has swept out of the material.
    fn flush(&mut self) -> Result<(), StrategyError> {
        if self.trail.len() >= 2 {
            let last = self.trail.last().copied();
            let lines = vec![std::mem::take(&mut self.trail)];
            let swept = Region::around_polylines(&lines, self.p.radius, self.p.swept_arc);
            if let Some(w) = &mut self.window {
                w.uncut = w.uncut.difference(&swept)?;
            }
            self.uncut.subtract(&swept)?;
            self.trail = last.into_iter().collect();
        }
        self.trail_length = 0.0;
        Ok(())
    }

    /// Make sure the window holds everything a step from `at` reads.
    fn window_at(&mut self, at: DVec2) -> Result<(), StrategyError> {
        let margin = self.p.radius + 3.0 * self.p.longest;
        let fits = self.window.as_ref().is_some_and(|w| {
            at.cmpge(w.lo + DVec2::splat(margin)).all()
                && at.cmple(w.hi - DVec2::splat(margin)).all()
        });
        if fits {
            return Ok(());
        }
        self.flush()?;
        let half = DVec2::splat(margin + 4.0 * self.p.radius);
        let (lo, hi) = (at - half, at + half);
        self.window = Some(Window {
            lo,
            hi,
            uncut: self.uncut.window(lo, hi)?,
            allowed: self.room.window(lo, hi)?,
        });
        Ok(())
    }

    /// Cut one pass, from `start` heading along `dir`, with the material on
    /// `side`: step after step, each turned to hold the engagement, until the
    /// material runs out or the next step would overload the tool. `entered`
    /// for a pass that begins where the tool went down, which may start
    /// heavily loaded. Empty where it cut nothing.
    fn pass(
        &mut self,
        start: DVec2,
        dir: DVec2,
        side: Side,
        entered: bool,
    ) -> Result<Vec<DVec2>, StrategyError> {
        self.flush()?;
        self.trail = vec![start];
        self.trail_length = 0.0;
        let mut points = vec![start];
        let mut at = start;
        let mut heading = dir;
        let mut step = self.p.shortest;
        let mut air = 0.0;
        let mut last_cut = 0;
        let mut lenient = if entered { 2.0 * self.p.radius } else { 0.0 };
        while self.steps < MAX_STEPS {
            self.steps += 1;
            self.window_at(at)?;
            let Some(w) = self.window.as_ref() else {
                break;
            };
            let Some(chosen) = choose(
                w,
                &self.trail,
                at,
                heading,
                side,
                step,
                lenient > 0.0,
                self.p,
            ) else {
                self.overloads += 1;
                break;
            };
            at = chosen.at;
            heading = chosen.heading;
            points.push(at);
            self.trail.push(at);
            self.trail_length += step;
            lenient -= step;
            if chosen.engagement < self.p.air {
                air += step;
                if air > self.p.radius {
                    break;
                }
            } else {
                air = 0.0;
                last_cut = points.len() - 1;
            }
            // Short steps where it turns, long where it runs straight.
            step = if chosen.turn.abs() > 0.3 {
                (step * 0.5).max(self.p.shortest)
            } else if chosen.turn.abs() < 0.1 {
                (step * 1.5).min(self.p.longest)
            } else {
                step
            };
            if self.trail_length > self.p.radius || self.trail.len() > 48 {
                self.flush()?;
            }
        }
        self.flush()?;
        points.truncate(last_cut + 1);
        Ok(if points.len() >= 2 {
            points
        } else {
            Vec::new()
        })
    }
}

/// Whether every point of the segment `a`–`b` is further than `distance` from
/// `region`, and outside it.
fn clear_of(region: &Region, a: DVec2, b: DVec2, distance: f64) -> bool {
    use camcraft_geom::engagement::segment_distance2;
    if region.contains(a) || region.contains(b) {
        return false;
    }
    let d2 = distance * distance;
    let cross = |o: DVec2, p: DVec2, q: DVec2| (p - o).perp_dot(q - o);
    for ring in region.rings() {
        let pts = ring.points();
        let n = pts.len();
        for i in 0..n {
            let (Some(&c), Some(&d)) = (pts.get(i), pts.get((i + 1) % n)) else {
                continue;
            };
            // Crossing, or nearer than the distance at an end of either.
            if (cross(a, b, c) > 0.0) != (cross(a, b, d) > 0.0)
                && (cross(c, d, a) > 0.0) != (cross(c, d, b) > 0.0)
            {
                return false;
            }
            if segment_distance2(c, a, b) < d2
                || segment_distance2(d, a, b) < d2
                || segment_distance2(a, c, d) < d2
                || segment_distance2(b, c, d) < d2
            {
                return false;
            }
        }
    }
    true
}

/// A step decided on.
#[derive(Debug, Clone, Copy)]
struct Step {
    at: DVec2,
    heading: DVec2,
    turn: f64,
    engagement: f64,
}

/// The step from `at`, heading along `heading`, `length` long, turned toward or
/// away from the material on `side` until its engagement is as asked; `None`
/// where every way would overload the tool or leave where it may be.
#[allow(clippy::too_many_arguments)]
fn choose(
    w: &Window,
    trail: &[DVec2],
    at: DVec2,
    heading: DVec2,
    side: Side,
    length: f64,
    lenient: bool,
    p: &LevelParams,
) -> Option<Step> {
    let mut path: Vec<DVec2> = Vec::with_capacity(trail.len() + 1);
    path.extend_from_slice(trail);
    path.push(at);
    // Turned `t` toward the material: the step, and its engagement, or `None`
    // where it would leave where the tool may be, or cut mostly the wrong way.
    let mut eval = |t: f64| -> Option<Step> {
        let dir = DVec2::from_angle(side.toward() * t).rotate(heading);
        let next = at + dir * length;
        if !w.allowed.contains_segment(at, next) {
            return None;
        }
        path.push(next);
        let e = engaged(next, p.radius, &w.uncut, &path, p.radius);
        path.pop();
        let total = e.angle();
        let (right, left) = e.sides(dir);
        let (with, against) = match side {
            Side::Right => (right, left),
            Side::Left => (left, right),
        };
        // Cutting mostly the wrong way is no way to go on, but a full slot's
        // halves are even.
        if against > with + 0.1 && against > p.air && !lenient {
            return None;
        }
        Some(Step {
            at: next,
            heading: dir,
            turn: t,
            engagement: total,
        })
    };
    let load = |s: &Option<Step>| s.as_ref().map_or(f64::INFINITY, |s| s.engagement);
    let target = p.target;
    let straight = eval(0.0);
    let g0 = load(&straight);
    if (g0 - target).abs() <= p.within {
        return straight;
    }
    // The bracket: `lo` under the target, `hi` over it.
    let (lo, hi) = if g0 < target {
        let most = eval(MAX_TURN);
        if load(&most) <= target + p.within {
            // Not enough even turned hard in: take what there is.
            return most;
        }
        ((0.0, straight), (MAX_TURN, most))
    } else {
        let away = eval(-MAX_TURN);
        let ga = load(&away);
        if ga >= target - p.within {
            // Too much even turned hard away: further, back the way it came if
            // need be, to anything the tool can take.
            if ga <= p.limit || (lenient && ga.is_finite()) {
                return away;
            }
            for t in [-1.5 * MAX_TURN, -2.0 * MAX_TURN, -3.0 * MAX_TURN] {
                let s = eval(t);
                let g = load(&s);
                if g < target - p.within {
                    return bisect((t, s), (-MAX_TURN, away), &mut eval, target, p);
                }
                if g <= p.limit || (lenient && g.is_finite()) {
                    return s;
                }
            }
            return None;
        }
        ((-MAX_TURN, away), (0.0, straight))
    };
    lo.1.as_ref()?;
    bisect(lo, hi, &mut eval, target, p)
}

/// Refine between `lo`, under the target, and `hi`, over it (or not a way at
/// all), to a step whose engagement is within reach of the target: the last one
/// found under it if none is.
fn bisect(
    lo: (f64, Option<Step>),
    hi: (f64, Option<Step>),
    eval: &mut impl FnMut(f64) -> Option<Step>,
    target: f64,
    p: &LevelParams,
) -> Option<Step> {
    let load = |s: &Option<Step>| s.as_ref().map_or(f64::INFINITY, |s| s.engagement);
    let (mut lo, mut hi) = (lo, hi);
    for _ in 0..SEARCH_ITERATIONS {
        let (gl, gh) = (load(&lo.1), load(&hi.1));
        // Regula falsi where both ends are ways, kept off the ends; halving
        // where one is not.
        let f = if gh.is_finite() && gh > gl {
            ((target - gl) / (gh - gl)).clamp(0.2, 0.8)
        } else {
            0.5
        };
        let t = lo.0 + (hi.0 - lo.0) * f;
        let s = eval(t);
        let g = load(&s);
        if (g - target).abs() <= p.within {
            return s;
        }
        if g < target {
            lo = (t, s);
        } else {
            hi = (t, s);
        }
    }
    lo.1
}

/// Somewhere deep inside `region`: the middle of what is left of it shrunk as
/// far as it can be. `None` for an empty region.
fn deepest(region: &Region, arc: f64) -> Option<DVec2> {
    let (lo, hi) = region.bounds()?;
    let mut inner = 0.0;
    let mut outer = (hi - lo).length() * 0.5 + arc;
    let mut best = region.clone();
    for _ in 0..24 {
        let mid = (inner + outer) * 0.5;
        let shrunk = region.offset(-mid, arc);
        if shrunk.is_empty() {
            outer = mid;
        } else {
            inner = mid;
            best = shrunk;
        }
        if outer - inner < arc {
            break;
        }
    }
    // The middle of the largest ring left, if it is inside; else a corner of it.
    let ring = best
        .rings()
        .iter()
        .max_by(|a, b| a.signed_area().total_cmp(&b.signed_area()))?;
    let pts = ring.points();
    #[allow(clippy::cast_precision_loss)]
    let middle = pts.iter().copied().sum::<DVec2>() / pts.len() as f64;
    if best.contains(middle) {
        Some(middle)
    } else {
        pts.first().copied()
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
mod tests;
