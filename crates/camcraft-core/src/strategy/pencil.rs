//! Pencil finishing: the tool run along where it touches the part in two
//! places at once, the inside corners and valleys a larger tool before it
//! could not reach into.
//!
//! # Where the tool touches twice
//!
//! Where a tool dropped onto the part comes to rest, its tip's height over each
//! point seen from above (its cutter location) is the highest of what each
//! face, edge and corner of the part would hold it at: smooth sheets, one for
//! each place it can touch. Where two meet, the tool touches both at once, and
//! the height has a crease, always a valley, never a ridge, since it is the
//! higher of the two either side. Where one sheet stands above the other the
//! height jumps instead: the tool's side touches a wall while its end rests on
//! what is below. The angle between the sheets either side, between where the
//! tool touches each, or at a wall's foot between what is below and the
//! wall's face, is the bitangency angle: a crease or a foot sharper than
//! it is a pencil line, one gentler is not ([decision 0028]). A ball whose
//! side meets a wall where a fillet it fits rises into it touches the two in
//! one place: the fillet faces the wall's way there, and it is no line.
//!
//! # Finding them
//!
//! - The tool is dropped on a grid over the area, and each node is given the
//!   sheet it is on: its height, and which way that sheet faces, from drops a
//!   ten-thousandth of a millimetre either side, moved off a crease that
//!   passes between them. The grid only finds where the lines run, near
//!   enough to look for them (decision 0009): it is fine enough that the
//!   sheet turns less than a quarter of the bitangency angle from node to
//!   node, and no coarser than ten tolerances.
//! - Two neighbouring nodes on different sheets have a line between them. The
//!   lines are where those pairs are, drawn from cell to cell: each cell to
//!   the cells beside it across such a pair. Where three or more meet, or one
//!   ends, a line ends.
//! - Each point of a line is then found exactly: looked for across the line,
//!   by halving, to an eighth of the tolerance, between two drops on different
//!   sheets, and judged once more there, where they meet. It is put on the
//!   lower of the two, where the tool's end rests at the foot of a wall, as
//!   far again from the wall, and given the height the tool comes to rest at
//!   there. One found behind the one before, as where a line stands upright
//!   seen from above, between two upright walls, is left out; where more than
//!   two cells running are not found again, the line is broken.
//! - Thinned, then each chord's middle looked at again: where the line, found
//!   again across it, or the part under the tool, is further than a quarter
//!   of the tolerance from it, a point is put there and each half looked at
//!   the same way.
//!
//! Every point is where the tool comes to rest dropped onto the part, so on
//! it, never in it; and no chord goes into the part by more than a quarter of
//! the tolerance at its middle, as every finishing pass's chords are kept.
//!
//! Where a line falls or climbs further than a cut step as steeply as a
//! plunge, down a corner between two upright walls, that stretch is left to
//! the linker, as a parallel pass's is.
//!
//! # Overthickness
//!
//! The lines may be found for a tool larger than the one that cuts, by the
//! overthickness: the cutting tool is then run along where the larger
//! one would touch twice, at the height it rests at there itself.
//!
//! # Passes beside them
//!
//! With stepovers, passes run either side of the
//! lines a stepover apart along the surface, as scallop's run in from a
//! boundary: the lines of equal distance from them across the tool's height
//! over the part, worked out on the grid ([`camcraft_geom::geodesic`]), and cut
//! on the part as a scallop pass is. They are cut from the outermost in, the
//! pencil lines last.
//!
//! # Order and direction
//!
//! The nearest next. Both ways starts an open line at its nearer end. One way
//! has the higher side, the wall or the dome, on the tool's right with a
//! clockwise spindle, climbing; the other way, the other.
//!
//! [decision 0028]: ../../../../docs/decisions/0028-pencil.md

use camcraft_geom::contour::{Region, polyline_length, simplify};
use camcraft_geom::geodesic::{Seed, march};
use camcraft_geom::mesh::closest_point_on_segment;
use camcraft_geom::trigrid::TriGrid;
use glam::{DVec2, DVec3};
use rayon::prelude::*;

use crate::control::{Watch, share};
use crate::cutter::surfaces::Seat;
use crate::cutter::tool::ToolProfile;
use crate::path::toolpath::Toolpath;
use crate::strategy::adaptive::Direction;
use crate::strategy::field::{AreaGrid, level_lines};
use crate::strategy::parallel::{
    ParallelParams, ParallelReport, Run, broken_steep, sweep_polyline,
};
use crate::strategy::projected::Emit;
use crate::strategy::{StrategyError, check, with_edge};

/// How far either side of a point the drops that say which way its sheet faces
/// are, in millimetres: far below any tolerance.
const ETA: f64 = 1.0e-4;

/// Where a point is moved to, in [`ETA`]s, when a crease passes between the
/// drops either side of it: off it, a different way each time.
const NUDGES: [DVec2; 4] = [
    DVec2::ZERO,
    DVec2::new(3.0, 1.0),
    DVec2::new(-2.0, 3.0),
    DVec2::new(1.0, -3.0),
];

/// The most the two sides of a point may disagree on which way its sheet
/// faces, in radians, for it to be on one: a degree.
const SAME_SHEET: f64 = 0.017_453_292_519_943_295;

/// How many times a chord is halved at most, to keep it on the line and the
/// part.
const MAX_SPLITS: u32 = 16;

/// The most points a line is looked at back along itself for at once when it
/// is thinned: far past any line's straight stretch at a usable pitch.
const MAX_THIN: usize = 4096;

/// How to find and cut the lines.
#[derive(Debug, Clone, PartialEq)]
pub struct PencilParams {
    /// Chordal tolerance, in millimetres.
    pub tolerance: f64,
    /// Cutting feed, in mm/min.
    pub feed: f64,
    /// The least angle between the two places the tool touches, in radians,
    /// for it to be a pencil line.
    pub bitangency: f64,
    /// Which way each line cuts.
    pub direction: Direction,
    /// Whether climbing puts the higher side on the tool's right, as it does
    /// with a clockwise spindle.
    pub climb_right: bool,
    /// Passes either side of each line, a stepover apart along the surface.
    pub stepovers: u32,
    /// The distance between them, in millimetres.
    pub stepover: f64,
    /// Largest Z drop, in millimetres, that a pass beside the lines may cut
    /// rather than leave to the linker, as a scallop pass.
    pub max_cut_step: f64,
    /// The most grid nodes; past it the grid is coarsened.
    pub max_nodes: u32,
    /// What it tells how far it has got, and asks whether to stop.
    pub watch: Watch,
}

impl Default for PencilParams {
    fn default() -> Self {
        Self {
            tolerance: 0.01,
            feed: 1000.0,
            bitangency: 20f64.to_radians(),
            direction: Direction::BothWays,
            climb_right: true,
            stepovers: 0,
            stepover: 0.5,
            max_cut_step: 0.5,
            max_nodes: 1 << 22,
            watch: Watch::default(),
        }
    }
}

/// What happened while finding and cutting the lines.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PencilReport {
    /// Passes cut: the lines, and those beside them.
    pub passes: u64,
    /// Pencil lines found and cut.
    pub lines: u64,
    /// Passes beside them.
    pub beside: u64,
    /// Points of the lines found exactly.
    pub located: u64,
    /// Points the grid drew that were not found again across the line, or
    /// were found behind the last, left out.
    pub unlocated: u64,
    /// Stretches too short to be worth cutting.
    pub fragments: u64,
    /// Grid nodes the tool was dropped at.
    pub nodes: u64,
    /// Their spacing, in millimetres.
    pub pitch: f64,
    /// Whether the grid was coarsened to stay within its budget.
    pub coarsened: bool,
    /// What sampling the passes beside the lines on the part found, as for
    /// parallel finishing.
    pub sweep: ParallelReport,
}

impl PencilReport {
    /// Whether anything needs the caller's attention.
    pub fn is_clean(&self) -> bool {
        self.sweep.is_clean()
    }
}

/// Pencil passes over `area`, where the tool's centre may go: the tool held
/// as `seat` cuts them, along where the one held as `finder` touches the part
/// twice. They are the same tool unless the lines are found for a larger one,
/// by the overthickness.
///
/// The path holds [`MoveRole::Cut`](crate::path::toolpath::MoveRole::Cut)
/// passes separated by
/// [`MoveRole::Link`](crate::path::toolpath::MoveRole::Link) moves, in the
/// order they are cut. The linker resolves them.
pub fn pencil(
    seat: &Seat<'_>,
    finder: &Seat<'_>,
    area: &Region,
    params: &PencilParams,
) -> Result<(Toolpath, PencilReport), StrategyError> {
    check("tolerance", params.tolerance)?;
    check("feed", params.feed)?;
    check("bitangency", params.bitangency)?;
    check("max_cut_step", params.max_cut_step)?;
    if params.stepovers > 0 {
        check("stepover", params.stepover)?;
    }
    let mut report = PencilReport::default();
    let Some((lo, hi)) = area.bounds() else {
        return Ok((Toolpath::new(), report));
    };
    let theta = params.bitangency.min(std::f64::consts::PI);

    // The grid: fine enough that a sheet turns less than a quarter of the
    // angle from node to node, where it turns fastest, round an edge of the
    // part at the tool's radius; and no coarser than ten tolerances. Set off
    // from the area's corner by an odd share of a pitch, so a line along a
    // face of the part square to the axes does not run through its nodes.
    let radius = finder.tool().max_radius();
    let pitch = (radius * theta / 4.0)
        .min(10.0 * params.tolerance)
        .max(params.tolerance);
    let on = AreaGrid::over(
        area,
        (lo - DVec2::new(0.382, 0.618) * pitch, hi),
        pitch,
        radius / 2.0,
        params.max_nodes,
        &params.watch.part(0.0, 0.2),
        |x, y, scratch| finder.drop_at_in(x, y, scratch),
    )?;
    let grid = &on.grid;
    let pitch = grid.pitch();
    report.pitch = pitch;
    report.nodes = u64::from(grid.nodes());
    report.coarsened = on.coarsened;

    // Each node's sheet.
    let sheets: Vec<Option<Sheet>> = (0..grid.nodes())
        .into_par_iter()
        .map_init(
            || Probe::new(finder),
            |probe, n| {
                if !grid.defined(n) {
                    return None;
                }
                grid.xy(n).and_then(|p| probe.sheet(p))
            },
        )
        .collect();
    params.watch.check()?;
    params.watch.report(0.4);

    // The lines, from cell to cell.
    let cells = Cells::of(grid, &on.inside, &sheets, theta);
    let chains = cells.chains();

    // Each point found exactly, across the line.
    let finding = Finding {
        seat,
        finder,
        theta,
        eps: params.tolerance / 8.0,
    };
    let width = 1.5 * pitch;
    let wanted: Vec<(usize, usize)> = chains
        .iter()
        .enumerate()
        .flat_map(|(c, chain)| (0..chain.cells.len()).map(move |i| (c, i)))
        .collect();
    let each = params.watch.part(0.4, 0.75);
    let all = wanted.len().max(1);
    let found: Vec<Option<Found>> = wanted
        .par_iter()
        .map_init(
            || finding.locator(),
            |at, &(c, i)| {
                if params.watch.stopped() {
                    return None;
                }
                each.add(share(1, all));
                let chain = chains.get(c)?;
                let here = cells.centre(*chain.cells.get(i)?)?;
                let along = chain.along(&cells, i)?;
                at.locate(here, along, width)
            },
        )
        .collect();
    params.watch.check()?;
    let kept = with_edge(area, params.tolerance);
    let mut lines: Vec<(Vec<Found>, bool)> = Vec::new();
    let mut found = found.into_iter();
    for chain in &chains {
        // Broken where more than two cells running are not found again: the
        // grid's cells may run on past where the sheets meet as sharply as
        // asked, as along the top of a fillet a ball fits, where its side
        // meets the wall as the fillet does; a line is never drawn across.
        let mut pieces: Vec<Vec<Found>> = vec![Vec::new()];
        let mut missed = 0;
        for _ in 0..chain.cells.len() {
            let piece = pieces.last_mut().filter(|_| missed <= 2);
            let f = found.next().flatten();
            match (f, piece) {
                // Each a step on along the line from the last: where it turns
                // on itself, as round where a line stands upright seen from
                // above, between two upright walls, a point found behind the
                // last is left out, so the tool never goes back and forth.
                (Some(f), Some(piece))
                    if piece
                        .last()
                        .is_none_or(|last| (f.at - last.at).truncate().dot(f.along) > 0.0) =>
                {
                    report.located += 1;
                    missed = 0;
                    piece.push(f);
                }
                (Some(f), None) => {
                    report.located += 1;
                    missed = 0;
                    pieces.push(vec![f]);
                }
                _ => {
                    report.unlocated += 1;
                    missed += 1;
                }
            }
        }
        pieces.retain(|p| !p.is_empty());
        let closed = chain.closed && pieces.len() == 1;
        for points in pieces {
            // Kept to the area: broken where a point is outside it.
            if points.iter().all(|f| kept.contains(f.at.truncate())) {
                lines.push((points, closed));
            } else {
                for piece in points.split(|f| !kept.contains(f.at.truncate())) {
                    if !piece.is_empty() {
                        lines.push((piece.to_vec(), false));
                    }
                }
            }
        }
    }

    // Thinned, put right at each chord, each the way it cuts.
    let refined: Vec<Option<(Run, f64)>> = lines
        .par_iter()
        .map_init(
            || finding.locator(),
            |at, (points, closed)| {
                let lean: f64 = points.iter().map(|f| f.lean).sum();
                let at_points: Vec<DVec3> = points.iter().map(|f| f.at).collect();
                let thin = thinned(&at_points, *closed, 0.25 * params.tolerance);
                let run = at.refined(&thin, *closed, params.tolerance, width);
                let length = polyline_length(&run.iter().map(|p| p.truncate()).collect::<Vec<_>>());
                let long = run.len() >= 2 && length >= 2.0 * pitch;
                long.then_some((
                    Run {
                        points: run,
                        closed: *closed,
                    },
                    lean,
                ))
            },
        )
        .collect();
    params.watch.check()?;
    let steep = ParallelParams {
        max_cut_step: params.max_cut_step,
        ..Default::default()
    };
    let mut runs: Vec<Run> = Vec::new();
    for r in refined {
        match r {
            Some((mut run, lean)) => {
                // Left higher where the lean is above nothing: climbing with a
                // clockwise spindle wants it on the right.
                let right_higher = (params.direction == Direction::Climb) == params.climb_right;
                if params.direction != Direction::BothWays && (lean > 0.0) == right_higher {
                    run.points.reverse();
                }
                // Where a line falls down a corner as steep as a plunge, as
                // between two upright walls, that stretch is left to the
                // linker, which lifts at its top and plunges at the plunge
                // feed to its foot, as it does a parallel pass's.
                runs.extend(broken_steep(run, &steep, &mut report.sweep));
            }
            None => report.fragments += 1,
        }
    }
    report.lines = runs.len() as u64;
    params.watch.report(0.85);

    let both_ways = params.direction == Direction::BothWays;
    let mut emit = Emit::new(params.feed);
    if params.stepovers > 0 {
        let beside = beside(seat, (&on, &kept), &runs, params, &mut report)?;
        for level in beside.into_iter().rev() {
            emit.nearest_first(level, both_ways)?;
        }
    }
    if params.watch.showing() {
        params.watch.show(runs.iter().map(Run::shown));
    }
    emit.nearest_first(runs, both_ways)?;
    report.passes = emit.passes;
    params.watch.report(1.0);
    Ok((emit.path, report))
}

/// The sheet the tool rests on at a point: where, seen from above, the height
/// it rests at, and which way the sheet faces, up.
#[derive(Debug, Clone, Copy)]
struct Sheet {
    at: DVec2,
    z: f64,
    n: DVec3,
}

/// Whether `a` and `b` are on different sheets that meet at more than
/// `theta`: turned that far from each other; or one standing above the other,
/// further than either's slope takes it between them, the tool's side against
/// a wall and its end on the lower, where the lower faces at least that far
/// from the wall's face, which faces it. A flat end resting on an edge above
/// the floor beside it is the one; a ball whose side meets the wall where a
/// fillet below it rises into the wall, the lower facing the wall's way, is
/// not.
fn apart(a: &Sheet, b: &Sheet, theta: f64) -> bool {
    if a.n.angle_between(b.n) >= theta {
        return true;
    }
    if !cliff(a, b) {
        return false;
    }
    let (low, high) = if a.z < b.z { (a, b) } else { (b, a) };
    (low.at - high.at)
        .try_normalize()
        .is_some_and(|wall| low.n.angle_between(wall.extend(0.0)) >= theta)
}

/// Whether one of `a` and `b` stands above the other further than either's
/// slope takes it between them: a wall between, the tool's side against it.
fn cliff(a: &Sheet, b: &Sheet) -> bool {
    let slope = |s: &Sheet| s.n.truncate().length() / s.n.z.max(1e-9);
    (a.z - b.z).abs() > a.at.distance(b.at) * (2.0 + slope(a) + slope(b))
}

/// Drops of one tool, with the scratch space they share.
struct Probe<'a, 'm> {
    seat: &'a Seat<'m>,
    scratch: Vec<u32>,
}

impl<'a, 'm> Probe<'a, 'm> {
    fn new(seat: &'a Seat<'m>) -> Self {
        Self {
            seat,
            scratch: Vec::new(),
        }
    }

    /// Where the tool's tip rests at `p`, if anything is under it.
    fn z(&mut self, p: DVec2) -> Option<f64> {
        self.seat.drop_at_in(p.x, p.y, &mut self.scratch)
    }

    /// The sheet at `p`: which way it faces from drops [`ETA`] either side,
    /// those ahead and those behind agreeing; moved off a crease that passes
    /// between them, to where they do. Where none near agrees, as on a sheet
    /// that turns sharply by a wall's top, both together at the last place
    /// tried. `None` where nothing is under the tool.
    fn sheet(&mut self, p: DVec2) -> Option<Sheet> {
        let mut last = None;
        for nudge in NUDGES {
            let q = p + nudge * ETA;
            let z = self.z(q)?;
            let mut facing = |s: f64| -> Option<DVec3> {
                let zx = self.z(q + DVec2::X * (s * ETA))?;
                let zy = self.z(q + DVec2::Y * (s * ETA))?;
                DVec3::new(-(zx - z) / (s * ETA), -(zy - z) / (s * ETA), 1.0).try_normalize()
            };
            let ahead = facing(1.0);
            let behind = facing(-1.0);
            match (ahead, behind) {
                (Some(a), Some(b)) if a.angle_between(b) <= SAME_SHEET => {
                    return (a + b).try_normalize().map(|n| Sheet { at: q, z, n });
                }
                (Some(n), None) | (None, Some(n)) => return Some(Sheet { at: q, z, n }),
                (Some(a), Some(b)) => {
                    last = (a + b).try_normalize().map(|n| Sheet { at: q, z, n });
                }
                (None, None) => {}
            }
        }
        last
    }
}

/// The grid's cells, each joined to those beside it across two nodes on
/// different sheets.
struct Cells {
    origin: DVec2,
    pitch: f64,
    /// Cells across and up.
    wide: u32,
    high: u32,
    /// Which sides of each cell are crossed: below, above, left, right.
    sides: Vec<u8>,
}

/// The sides of a cell, each a bit, and the step to the cell beyond it.
const SIDES: [(u8, i64, i64); 4] = [(1, 0, -1), (2, 0, 1), (4, -1, 0), (8, 1, 0)];

/// A line from cell to cell.
struct Chain {
    cells: Vec<u32>,
    closed: bool,
}

impl Chain {
    /// Which way it runs at its `i`th cell, from two cells back to two on.
    fn along(&self, cells: &Cells, i: usize) -> Option<DVec2> {
        let n = self.cells.len();
        let (back, on) = if self.closed {
            ((i + 2 * n - 2) % n, (i + 2) % n)
        } else {
            (i.saturating_sub(2), (i + 2).min(n.saturating_sub(1)))
        };
        let a = cells.centre(*self.cells.get(back)?)?;
        let b = cells.centre(*self.cells.get(on)?)?;
        (b - a).try_normalize()
    }
}

impl Cells {
    /// The cells of `grid`, joined across each pair of neighbouring nodes on
    /// different sheets, one of them inside the area.
    fn of(grid: &TriGrid, inside: &[bool], sheets: &[Option<Sheet>], theta: f64) -> Self {
        let (nx, ny) = (grid.nx(), grid.ny());
        let pitch = grid.pitch();
        let at = |ix: u32, iy: u32| -> Option<(Sheet, bool)> {
            let n = grid.node(ix, iy)?;
            let i = usize::try_from(n).ok()?;
            Some(((*sheets.get(i)?)?, inside.get(i).copied().unwrap_or(false)))
        };
        let crossed = |a: Option<(Sheet, bool)>, b: Option<(Sheet, bool)>| match (a, b) {
            (Some((a, ia)), Some((b, ib))) => (ia || ib) && apart(&a, &b, theta),
            _ => false,
        };
        let (wide, high) = (nx.saturating_sub(1), ny.saturating_sub(1));
        let sides: Vec<u8> = (0..high)
            .into_par_iter()
            .flat_map_iter(|cy| {
                (0..wide).map(move |cx| {
                    let corner = |dx: u32, dy: u32| at(cx + dx, cy + dy);
                    let mut s = 0u8;
                    if cy > 0 && crossed(corner(0, 0), corner(1, 0)) {
                        s |= 1;
                    }
                    if cy + 1 < high && crossed(corner(0, 1), corner(1, 1)) {
                        s |= 2;
                    }
                    if cx > 0 && crossed(corner(0, 0), corner(0, 1)) {
                        s |= 4;
                    }
                    if cx + 1 < wide && crossed(corner(1, 0), corner(1, 1)) {
                        s |= 8;
                    }
                    s
                })
            })
            .collect();
        Self {
            origin: grid.origin(),
            pitch,
            wide,
            high,
            sides,
        }
    }

    fn sides(&self, c: u32) -> u8 {
        usize::try_from(c)
            .ok()
            .and_then(|i| self.sides.get(i))
            .copied()
            .unwrap_or(0)
    }

    /// The middle of cell `c`, seen from above.
    fn centre(&self, c: u32) -> Option<DVec2> {
        (c < self.wide * self.high).then(|| {
            let (cx, cy) = (c % self.wide, c / self.wide);
            self.origin + (DVec2::new(f64::from(cx), f64::from(cy)) + 0.5) * self.pitch
        })
    }

    /// The cell beyond side `bit` of `c`.
    fn beyond(&self, c: u32, bit: u8) -> Option<u32> {
        let &(_, dx, dy) = SIDES.iter().find(|s| s.0 == bit)?;
        let cx = i64::from(c % self.wide) + dx;
        let cy = i64::from(c / self.wide) + dy;
        let (cx, cy) = (u32::try_from(cx).ok()?, u32::try_from(cy).ok()?);
        (cx < self.wide && cy < self.high).then_some(cy * self.wide + cx)
    }

    /// The side of a cell its neighbour across `bit` meets it by.
    fn opposite(bit: u8) -> u8 {
        match bit {
            1 => 2,
            2 => 1,
            4 => 8,
            _ => 4,
        }
    }

    /// The lines, cell to cell: from each cell where one ends or more than two
    /// meet, along each side of it, through cells joined on two sides, to the
    /// next such cell; then the rings, every cell of them joined on two.
    fn chains(&self) -> Vec<Chain> {
        let count = self.wide * self.high;
        let mut used = vec![0u8; self.sides.len()];
        let mut out = Vec::new();
        let degree = |c: u32| self.sides(c).count_ones();
        let walk = |start: u32, bit: u8, used: &mut Vec<u8>| -> Chain {
            let mut cells = vec![start];
            let (mut at, mut bit) = (start, bit);
            let closed = loop {
                if let Some(u) = usize::try_from(at).ok().and_then(|i| used.get_mut(i)) {
                    *u |= bit;
                }
                let Some(next) = self.beyond(at, bit) else {
                    break false;
                };
                if let Some(u) = usize::try_from(next).ok().and_then(|i| used.get_mut(i)) {
                    *u |= Self::opposite(bit);
                }
                if next == start {
                    break true;
                }
                cells.push(next);
                if degree(next) != 2 {
                    break false;
                }
                let free = self.sides(next) & !Self::opposite(bit);
                at = next;
                bit = free.isolate_lowest_one();
                if bit == 0 {
                    break false;
                }
            };
            Chain { cells, closed }
        };
        for c in 0..count {
            if matches!(degree(c), 0 | 2) {
                continue;
            }
            for &(bit, ..) in &SIDES {
                let done = usize::try_from(c)
                    .ok()
                    .and_then(|i| used.get(i))
                    .is_some_and(|u| u & bit != 0);
                if self.sides(c) & bit != 0 && !done {
                    out.push(walk(c, bit, &mut used));
                }
            }
        }
        for c in 0..count {
            if degree(c) != 2 {
                continue;
            }
            let free = self.sides(c)
                & !usize::try_from(c)
                    .ok()
                    .and_then(|i| used.get(i))
                    .copied()
                    .unwrap_or(0);
            if free != 0 {
                out.push(walk(c, free.isolate_lowest_one(), &mut used));
            }
        }
        out
    }
}

/// A point of a line found exactly: where the tool cuts, and how much higher
/// the part is on the line's left than on its right, as it was found.
#[derive(Debug, Clone, Copy)]
struct Found {
    at: DVec3,
    lean: f64,
    /// Which way the line was taken to run where it was found.
    along: DVec2,
}

/// What finding the lines' points takes.
struct Finding<'a, 'm> {
    seat: &'a Seat<'m>,
    finder: &'a Seat<'m>,
    theta: f64,
    eps: f64,
}

impl<'a, 'm> Finding<'a, 'm> {
    fn locator(&self) -> Locator<'a, 'm> {
        Locator {
            finder: Probe::new(self.finder),
            tool: Probe::new(self.seat),
            theta: self.theta,
            eps: self.eps,
        }
    }
}

/// Points of the lines found, one thread's drops.
struct Locator<'a, 'm> {
    finder: Probe<'a, 'm>,
    tool: Probe<'a, 'm>,
    theta: f64,
    eps: f64,
}

impl Locator<'_, '_> {
    /// The line nearest `c`, across `along`, looked for `w` either side of it,
    /// then twice as far: found by halving between two points on different
    /// sheets to [`Finding::eps`], the lower kept.
    fn locate(&mut self, c: DVec2, along: DVec2, w: f64) -> Option<Found> {
        let left = along.perp();
        for width in [w, 2.0 * w] {
            let (Some(l), Some(r)) = (
                self.finder.sheet(c + left * width),
                self.finder.sheet(c - left * width),
            ) else {
                continue;
            };
            if !apart(&l, &r, self.theta) {
                continue;
            }
            let (mut lo, mut hi) = ((-width, r), (width, l));
            while hi.0 - lo.0 > self.eps {
                let mid = 0.5 * (lo.0 + hi.0);
                let m = self.finder.sheet(c + left * mid)?;
                let with_lo = !apart(&m, &lo.1, self.theta);
                let with_hi = !apart(&m, &hi.1, self.theta);
                let lower = match (with_lo, with_hi) {
                    (true, false) => true,
                    (false, true) => false,
                    // Turned less than the angle from both: whichever it is
                    // nearer.
                    (true, true) => m.n.angle_between(lo.1.n) <= m.n.angle_between(hi.1.n),
                    // A third sheet between: the line nearer `c`.
                    (false, false) => mid < 0.0,
                };
                if lower {
                    lo = (mid, m);
                } else {
                    hi = (mid, m);
                }
            }
            // Two sheets that meet as sharply as asked, judged where they
            // meet: at a wall's foot the lower faces there as it does at the
            // foot, not as a tolerance or two out.
            if !apart(&lo.1, &hi.1, self.theta) {
                return None;
            }
            // The lower; at a wall's foot, on the floor beside it, and as far
            // again from the wall as it is found to, so it is never on the
            // wall's top however the drops either side of it fall.
            let (low, high) = if lo.1.z <= hi.1.z {
                (lo.1, hi.1)
            } else {
                (hi.1, lo.1)
            };
            let p = if cliff(&low, &high) {
                low.at + (low.at - high.at).normalize_or_zero() * self.eps
            } else {
                low.at
            };
            let z = self.tool.z(p)?;
            return Some(Found {
                at: p.extend(z),
                lean: l.z - r.z,
                along,
            });
        }
        None
    }

    /// `points`, a line thinned, with points put back where a chord strays
    /// more than a quarter of `tolerance` from the line, found again across
    /// it as far as `width`, or from the part, into it or off it.
    fn refined(
        &mut self,
        points: &[DVec3],
        closed: bool,
        tolerance: f64,
        width: f64,
    ) -> Vec<DVec3> {
        let mut out = Vec::with_capacity(points.len());
        let ends = points.iter().zip(
            points
                .iter()
                .skip(1)
                .chain(points.first().filter(|_| closed)),
        );
        for (&a, &b) in ends {
            out.push(a);
            self.between(a, b, (tolerance, width), 0, &mut out);
        }
        if !closed && let Some(&last) = points.last() {
            out.push(last);
        }
        out
    }

    /// Points between `a` and `b`, both on the line and the part: the chord's
    /// middle found again on the line across it, or dropped onto the part,
    /// where it strays more than a quarter of the tolerance, and each half
    /// looked at the same way.
    fn between(
        &mut self,
        a: DVec3,
        b: DVec3,
        (tolerance, width): (f64, f64),
        depth: u32,
        out: &mut Vec<DVec3>,
    ) {
        let flat = a.truncate().distance(b.truncate());
        if depth > MAX_SPLITS || flat < 0.5 * tolerance {
            return;
        }
        let m = a.lerp(b, 0.5);
        let Some(along) = (b - a).truncate().try_normalize() else {
            return;
        };
        let near = (0.5 * flat).clamp(4.0 * self.eps, width);
        // The line found again across the chord, where it is between its ends.
        let line = self
            .locate(m.truncate(), along, near)
            .map(|f| f.at)
            .filter(|p| {
                let t = (p.truncate() - a.truncate()).dot(along) / flat;
                t > 0.0 && t < 1.0
            });
        let put = line
            .filter(|p| p.distance(m) > 0.25 * tolerance)
            .or_else(|| {
                // Near enough the line, but into the part: the chord cuts the
                // inside of a curve at a wall's foot, up the wall. The line's
                // own point there, on the floor beside it, or failing that
                // where the tool rests.
                let z = self.tool.z(m.truncate())?;
                (z > m.z + 0.25 * tolerance).then(|| line.unwrap_or(m.truncate().extend(z)))
            });
        if let Some(p) = put {
            self.between(a, p, (tolerance, width), depth + 1, out);
            out.push(p);
            self.between(p, b, (tolerance, width), depth + 1, out);
        }
    }
}

/// `points`, with each dropped that the chord past it keeps within `within`
/// of, and of every point dropped before it since the last kept.
fn thinned(points: &[DVec3], closed: bool, within: f64) -> Vec<DVec3> {
    let mut line = points.to_vec();
    if closed && let Some(&first) = points.first() {
        line.push(first);
    }
    let n = line.len();
    if n < 3 {
        return points.to_vec();
    }
    let mut out = Vec::with_capacity(n);
    let mut i = 0;
    while let Some(&a) = line.get(i) {
        out.push(a);
        if i + 1 >= n {
            break;
        }
        let mut j = i + 1;
        while j + 1 < n && j - i < MAX_THIN {
            let Some(&b) = line.get(j + 1) else { break };
            let fits = line
                .get(i + 1..=j)
                .unwrap_or_default()
                .iter()
                .all(|&p| p.distance(closest_point_on_segment(p, a, b)) <= within);
            if !fits {
                break;
            }
            j += 1;
        }
        i = j;
    }
    if closed {
        out.pop();
    }
    out
}

/// One level's passes beside the lines, what sampling them found, and how many
/// stretches were too short to cut.
type Level = (Vec<Run>, ParallelReport, u64);

/// The passes beside the lines `runs`, a stepover apart along the surface out
/// to `params.stepovers` of them: lines of equal distance from them over the
/// grid, kept to the area, cut on the part as scallop's are. Each level's
/// passes, the nearest the lines first.
fn beside(
    seat: &Seat<'_>,
    (on, kept): (&AreaGrid, &Region),
    runs: &[Run],
    params: &PencilParams,
    report: &mut PencilReport,
) -> Result<Vec<Vec<Run>>, StrategyError> {
    let grid = &on.grid;
    let pitch = grid.pitch();
    let reach = 1.5 * pitch;
    // The seeds: each node within a pitch and a half of a line, at its
    // distance from it.
    let segments: Vec<(DVec3, DVec3)> = runs
        .iter()
        .flat_map(|r| {
            let p = &r.points;
            let last = if r.closed { p.first() } else { None };
            p.iter()
                .copied()
                .zip(p.iter().skip(1).chain(last).copied())
                .collect::<Vec<_>>()
        })
        .collect();
    let mut near: Vec<Seed> = segments
        .par_iter()
        .flat_map_iter(|&(a, b)| {
            let (min, max) = (
                a.truncate().min(b.truncate()) - DVec2::splat(reach),
                a.truncate().max(b.truncate()) + DVec2::splat(reach),
            );
            on.nodes_near(min, max).filter_map(move |n| {
                let p = grid.point(n)?;
                let d = p.distance(closest_point_on_segment(p, a, b));
                let flat = p
                    .truncate()
                    .distance(closest_point_on_segment(p, a, b).truncate());
                (flat <= reach).then_some(Seed {
                    node: n,
                    distance: d,
                })
            })
        })
        .collect();
    near.par_sort_by(|a, b| a.node.cmp(&b.node).then(a.distance.total_cmp(&b.distance)));
    near.dedup_by_key(|s| s.node);
    params.watch.check()?;
    let d = march(grid, &near, |n| grid.point(n));
    let lines = level_lines(grid, &d, params.stepover, params.stepovers);
    let sweep = ParallelParams {
        stepover: params.stepover,
        tolerance: params.tolerance,
        feed: params.feed,
        max_cut_step: params.max_cut_step,
        ..Default::default()
    };
    let thin = (0.25 * params.tolerance).min(0.05 * pitch);
    let levels: Vec<Result<Level, StrategyError>> = lines
        .into_par_iter()
        .skip(1)
        .map(|level| {
            let mut found = ParallelReport::default();
            let mut fragments = 0;
            let stretches: Vec<(Vec<DVec2>, bool)> =
                level.into_iter().map(|l| (l.points, l.closed)).collect();
            let mut out = Vec::new();
            for (points, closed) in kept.clip_lines(&stretches)? {
                if !closed && polyline_length(&points) < 2.0 * pitch {
                    fragments += 1;
                    continue;
                }
                let points = simplify(&points, closed, thin);
                out.extend(sweep_polyline(seat, &points, closed, &sweep, &mut found));
            }
            Ok((out, found, fragments))
        })
        .collect();
    let mut out = Vec::with_capacity(levels.len());
    for level in levels {
        let (runs, found, fragments) = level?;
        report.sweep += found;
        report.fragments += fragments;
        report.beside += runs.len() as u64;
        out.push(runs);
    }
    Ok(out)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests;
