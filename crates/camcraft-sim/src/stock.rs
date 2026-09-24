//! Material removal simulation: the stock model and the gouge oracle.
//!
//! These are deliberately the same code. Simulating a toolpath *is* how stock is
//! updated, so rest machining is not a feature — it is what happens when operation
//! two runs after operation one. And because the simulator already knows where the
//! material went, it can answer the questions no geometric check can:
//!
//! - did the tool end up **below the part**, and by how much
//! - did a **rapid touch material**, which is the defect Pngcam ships with
//!
//! # Removal is an erosion, the dual of the cutter-location dilation
//!
//! A tool tip at `(x, y, z)` occupies, at radial distance `d` from its axis, every
//! height at or above `z + h(d)`. So removing it from the stock field is
//!
//! ```text
//! Zstock(px, py) <- min( Zstock(px, py), z + h(d) )
//! ```
//!
//! over the tool footprint — exactly the mirror of how the CL surface is built.
//!
//! # Why this crate defines its own traits
//!
//! `camcraft-core` depends on this crate, not the other way round, so nothing here
//! can name `Toolpath` or `ToolProfile` without creating a cycle. [`SweptTool`] and
//! [`SimMove`] are the minimum the simulator actually needs — a footprint, a profile
//! and whether a move is allowed to touch material — and core implements them for
//! its own types. The restriction is useful rather than merely tolerated: it keeps
//! the simulator a geometry service that cannot quietly grow CAM opinions.

use camcraft_geom::heightmap::{HeightMap, HeightMapError, narrow};
use camcraft_geom::mesh::TriMesh;
use glam::{DVec2, DVec3, dvec2};
use rayon::prelude::*;

/// How far a move may climb and still be level, or wander and still be straight
/// up or down, in millimetres: both are then swept exactly.
const STRAIGHT: f64 = 1.0e-9;

/// Cells a level move's footprint must span before its rows are cut in parallel.
const PARALLEL_CELLS: u64 = 100_000;

/// Distance from `p` to the segment `a`-`b`.
fn distance_to_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 > 0.0 {
        ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p - (a + ab * t)).length()
}

/// Where the row at `y` crosses the points within `r` of the segment `a`-`b`, as
/// `(lowest x, highest x)`. The region is convex, so its crossing is one span:
/// the widest of the span across either end's disc and across the band between.
fn capsule_row(a: DVec2, b: DVec2, r: f64, y: f64) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for end in [a, b] {
        let dy = y - end.y;
        if dy.abs() <= r {
            let w = (r * r - dy * dy).max(0.0).sqrt();
            lo = lo.min(end.x - w);
            hi = hi.max(end.x + w);
        }
    }
    let ab = b - a;
    let len = ab.length();
    if len > 0.0 {
        let u = ab / len;
        let n = dvec2(-u.y, u.x);
        // Along the row, x moves the point `coef` per unit across the band and
        // along the segment; each must stay in its range.
        let mut span = (f64::NEG_INFINITY, f64::INFINITY);
        for (coef, at, min, max) in [
            (n.x, (y - a.y) * n.y, -r, r),
            (u.x, (y - a.y) * u.y, 0.0, len),
        ] {
            if coef.abs() > 1.0e-12 {
                let (p, q) = ((min - at) / coef, (max - at) / coef);
                span = (span.0.max(p.min(q)), span.1.min(p.max(q)));
            } else if !(min..=max).contains(&at) {
                span = (f64::INFINITY, f64::NEG_INFINITY);
            }
        }
        if span.0 <= span.1 {
            lo = lo.min(a.x + span.0);
            hi = hi.max(a.x + span.1);
        }
    }
    (lo <= hi).then_some((lo, hi))
}

/// Upper bound on samples along one swept move.
///
/// A bound rather than a hope: a pathological coordinate must not turn a single
/// move into an unbounded loop.
const MAX_SWEEP_STEPS: u32 = 1_000_000;

/// The minimum a simulator needs to know about a cutter.
pub trait SweptTool: Sync {
    /// Largest radius anywhere on the cutting portion.
    fn max_radius(&self) -> f64;

    /// Height of the tool surface at radial distance `d` above the tip, or `None`
    /// beyond the tool.
    fn height_at(&self, d: f64) -> Option<f64>;
}

/// Whether a move is permitted to be in material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveClass {
    /// Cutting, plunging, ramping or leading. May remove material.
    MayCut,
    /// A traverse at full speed. Must never touch material; if it does, that is a
    /// defect worth stopping for.
    Rapid,
}

/// One move, reduced to what the simulator needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimMove {
    /// Where the tool tip ends up.
    pub to: DVec3,
    /// Whether it may be in material.
    pub class: MoveClass,
}

/// A block of material, as a height field.
#[derive(Debug, Clone, PartialEq)]
pub struct Stock {
    field: HeightMap,
    floor: f64,
}

/// Stock that could not be created.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum StockError {
    /// The grid was unusable.
    #[error("cannot build the stock grid: {0}")]
    Grid(#[from] HeightMapError),
    /// There were no blocks to make stock from.
    #[error("there is no stock")]
    NoBlocks,
    /// The block had no height.
    #[error("stock top {top} is not above its floor {floor}")]
    Inverted {
        /// Requested top.
        top: f64,
        /// Requested floor.
        floor: f64,
    },
}

/// What simulating a path did.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SimReport {
    /// Approximate volume removed, in cubic millimetres.
    pub removed_mm3: f64,
    /// Rapid moves that passed through material.
    ///
    /// Any non-zero value is a defect: a rapid is a full-speed traverse and is not
    /// supposed to be cutting anything.
    pub rapid_contacts: u64,
    /// Deepest a rapid went into material, in millimetres.
    pub deepest_rapid_contact_mm: f64,
}

impl SimReport {
    /// Whether anything here should stop a program being run.
    pub fn is_clean(&self) -> bool {
        self.rapid_contacts == 0
    }
}

/// A rectangle of stock cells, inclusive on every side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellRect {
    /// First column.
    pub x0: u32,
    /// First row.
    pub y0: u32,
    /// Last column.
    pub x1: u32,
    /// Last row.
    pub y1: u32,
}

impl CellRect {
    /// The one cell `(ix, iy)`.
    pub fn cell(ix: u32, iy: u32) -> Self {
        Self {
            x0: ix,
            y0: iy,
            x1: ix,
            y1: iy,
        }
    }

    /// The smallest rectangle holding both.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    /// Either, or the rectangle holding both.
    pub fn join(a: Option<Self>, b: Option<Self>) -> Option<Self> {
        match (a, b) {
            (Some(a), Some(b)) => Some(a.union(b)),
            (a, b) => a.or(b),
        }
    }

    /// Columns across.
    pub fn width(&self) -> u32 {
        self.x1 - self.x0 + 1
    }

    /// Rows down.
    pub fn height(&self) -> u32 {
        self.y1 - self.y0 + 1
    }

    /// Whether `(ix, iy)` is inside.
    pub fn contains(&self, ix: u32, iy: u32) -> bool {
        (self.x0..=self.x1).contains(&ix) && (self.y0..=self.y1).contains(&iy)
    }
}

/// What a sweep did, short of the volume it removed.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Swept {
    /// Rapid moves that passed through material.
    pub rapid_contacts: u64,
    /// Deepest a rapid went into material, in millimetres.
    pub deepest_rapid_contact_mm: f64,
    /// The cells it lowered, or `None` when it removed nothing.
    pub touched: Option<CellRect>,
}

/// A stock kept small: its heights run-length encoded, since machined material
/// is mostly flat (the untouched top, cleared floors, terraces), or kept as they
/// are when that would be larger. Bit for bit the stock it was made from.
#[derive(Debug, Clone, PartialEq)]
pub struct PackedStock {
    origin: DVec2,
    pitch: f64,
    nx: u32,
    ny: u32,
    floor: f64,
    cells: Packed,
}

/// A stock's heights, packed.
#[derive(Debug, Clone, PartialEq)]
enum Packed {
    /// Runs of equal heights, as the bits of the height and how many.
    Runs(Vec<(u32, u32)>),
    /// Every height, when runs would take more.
    Raw(Vec<u32>),
}

impl PackedStock {
    /// `stock`, packed.
    pub fn pack(stock: &Stock) -> Self {
        let data = stock.field.data();
        let mut runs: Vec<(u32, u32)> = Vec::new();
        let mut raw = false;
        for v in data {
            let bits = v.to_bits();
            match runs.last_mut() {
                Some((b, n)) if *b == bits && *n < u32::MAX => *n += 1,
                _ => runs.push((bits, 1)),
            }
            // Past half as many runs as cells, runs are larger than the cells.
            if runs.len() > data.len() / 2 + 1 {
                raw = true;
                break;
            }
        }
        let cells = if raw {
            Packed::Raw(data.iter().map(|v| v.to_bits()).collect())
        } else {
            runs.shrink_to_fit();
            Packed::Runs(runs)
        };
        let f = &stock.field;
        Self {
            origin: f.origin(),
            pitch: f.pitch(),
            nx: f.nx(),
            ny: f.ny(),
            floor: stock.floor,
            cells,
        }
    }

    /// Bytes it holds.
    pub fn bytes(&self) -> usize {
        match &self.cells {
            Packed::Runs(r) => r.len() * 8,
            Packed::Raw(r) => r.len() * 4,
        }
    }

    /// Every height, in order.
    fn heights(&self) -> impl Iterator<Item = f32> + '_ {
        let (runs, raw): (&[(u32, u32)], &[u32]) = match &self.cells {
            Packed::Runs(r) => (r, &[]),
            Packed::Raw(r) => (&[], r),
        };
        runs.iter()
            .flat_map(|&(bits, n)| std::iter::repeat_n(f32::from_bits(bits), n as usize))
            .chain(raw.iter().map(|b| f32::from_bits(*b)))
    }

    /// The stock, unpacked.
    pub fn unpack(&self) -> Result<Stock, StockError> {
        let mut field = HeightMap::filled(self.origin, self.pitch, self.nx, self.ny, 0.0)?;
        for (slot, v) in field.data_mut().iter_mut().zip(self.heights()) {
            *slot = v;
        }
        Ok(Stock {
            field,
            floor: self.floor,
        })
    }

    /// Make `stock`, on the same grid, this one again, in place. Returns the
    /// cells that differed, or `None` when the grids are not the same.
    pub fn restore(&self, stock: &mut Stock) -> Option<Option<CellRect>> {
        let b = &stock.field;
        if self.nx != b.nx() || self.ny != b.ny() || self.pitch.to_bits() != b.pitch().to_bits() {
            return None;
        }
        stock.floor = self.floor;
        let nx = stock.field.nx() as usize;
        let mut changed: Option<CellRect> = None;
        for (i, (slot, v)) in stock
            .field
            .data_mut()
            .iter_mut()
            .zip(self.heights())
            .enumerate()
        {
            if slot.to_bits() != v.to_bits() {
                *slot = v;
                let (ix, iy) = (u32::try_from(i % nx).ok()?, u32::try_from(i / nx).ok()?);
                let cell = CellRect::cell(ix, iy);
                changed = Some(changed.map_or(cell, |c| c.union(cell)));
            }
        }
        Some(changed)
    }
}

impl Stock {
    /// Make this stock `other` again, in place, on the same grid. Returns the
    /// cells that differed, or `None` when the grids are not the same.
    pub fn restore_from(&mut self, other: &Stock) -> Option<Option<CellRect>> {
        let (a, b) = (&other.field, &self.field);
        if a.nx() != b.nx() || a.ny() != b.ny() || a.pitch().to_bits() != b.pitch().to_bits() {
            return None;
        }
        self.floor = other.floor;
        let nx = a.nx() as usize;
        let mut changed: Option<CellRect> = None;
        for (iy, (dst, src)) in (0u32..).zip(
            self.field
                .data_mut()
                .chunks_mut(nx)
                .zip(other.field.data().chunks(nx)),
        ) {
            let same = |(x, y): (&f32, &f32)| x.to_bits() == y.to_bits();
            let Some(first) = dst.iter().zip(src.iter()).position(|p| !same(p)) else {
                continue;
            };
            let last = dst
                .iter()
                .zip(src.iter())
                .rposition(|p| !same(p))
                .unwrap_or(first);
            dst.copy_from_slice(src);
            let row = CellRect {
                x0: u32::try_from(first).ok()?,
                y0: iy,
                x1: u32::try_from(last).ok()?,
                y1: iy,
            };
            changed = Some(changed.map_or(row, |c| c.union(row)));
        }
        Some(changed)
    }
}

/// How the simulated result compares with the part.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VerifyReport {
    /// Columns compared.
    pub compared: u64,
    /// Columns where the simulated surface is below the part.
    pub gouged: u64,
    /// Deepest such excursion, in millimetres.
    pub deepest_gouge_mm: f64,
    /// Columns where material remains well above the part.
    pub leftover: u64,
    /// Most material left anywhere, in millimetres.
    pub tallest_leftover_mm: f64,
}

impl VerifyReport {
    /// Whether the result is within tolerance of the part.
    ///
    /// Leftover does not count: an unfinished region is a planning question, while a
    /// gouge is a ruined part.
    pub fn is_clean(&self) -> bool {
        self.gouged == 0
    }
}

impl Stock {
    /// A rectangular block covering `[min, max]`, its top at `top`.
    pub fn block(
        min: DVec2,
        max: DVec2,
        top: f64,
        floor: f64,
        pitch: f64,
    ) -> Result<Self, StockError> {
        if top <= floor || !top.is_finite() || !floor.is_finite() {
            return Err(StockError::Inverted { top, floor });
        }
        Ok(Self {
            field: HeightMap::covering(min, max, pitch, top)?,
            floor,
        })
    }

    /// Several rectangular blocks as one stock, each `(min, max, top)`.
    ///
    /// The field covers all of them. A cell takes the highest top of the blocks
    /// over it, and a cell no block covers holds `floor`: no material. One block is
    /// exactly [`Stock::block`]. Blocks stacked at the same XY cannot be told apart,
    /// since the field holds one height per column.
    pub fn from_boxes(
        boxes: &[(DVec2, DVec2, f64)],
        floor: f64,
        pitch: f64,
    ) -> Result<Self, StockError> {
        let Some(&(min0, max0, _)) = boxes.first() else {
            return Err(StockError::NoBlocks);
        };
        let (mut min, mut max) = (min0, max0);
        for &(lo, hi, top) in boxes {
            if top <= floor || !top.is_finite() || !floor.is_finite() {
                return Err(StockError::Inverted { top, floor });
            }
            min = min.min(lo);
            max = max.max(hi);
        }
        let mut field = HeightMap::covering(min, max, pitch, floor)?;
        let (nx, ny, origin) = (field.nx(), field.ny(), field.origin());
        // Cells from the one at or below a block's low edge to the one at or
        // beyond its high edge: a lone block covers the whole grid, as `block`.
        let span = |lo: f64, hi: f64, o: f64, n: u32| -> (u32, u32) {
            let last = f64::from(n.saturating_sub(1));
            let a = ((lo - o) / pitch + 1.0e-9).floor().clamp(0.0, last);
            let b = ((hi - o) / pitch - 1.0e-9).ceil().clamp(0.0, last);
            // Clamped into the grid, so the narrowing is exact.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let r = (a as u32, b as u32);
            r
        };
        for &(lo, hi, top) in boxes {
            let (x0, x1) = span(lo.x, hi.x, origin.x, nx);
            let (y0, y1) = span(lo.y, hi.y, origin.y, ny);
            for iy in y0..=y1 {
                for ix in x0..=x1 {
                    if field.get(ix, iy).is_none_or(|h| h < top) {
                        field.set(ix, iy, top);
                    }
                }
            }
        }
        Ok(Self { field, floor })
    }

    /// The material height field.
    pub fn field(&self) -> &HeightMap {
        &self.field
    }

    /// The bottom of the block.
    pub fn floor(&self) -> f64 {
        self.floor
    }

    /// Height of material at a column, or `None` outside the block.
    pub fn height_at(&self, x: f64, y: f64) -> Option<f64> {
        self.field.sample(x, y)
    }

    /// Sweep a path through the block, removing material.
    ///
    /// Level moves, and moves straight up or down, are swept exactly. A move that
    /// climbs or descends as it goes is sampled at no more than half a cell, so
    /// consecutive stamps overlap heavily; its residual error is bounded by how
    /// much `z + h(d)` can change over half a cell, which is well under a cell for
    /// any real cutter.
    pub fn apply<T: SweptTool>(&mut self, moves: &[SimMove], tool: &T) -> SimReport {
        let before = self.volume();
        let swept = self.sweep(moves, tool);
        SimReport {
            removed_mm3: (before - self.volume()).max(0.0),
            rapid_contacts: swept.rapid_contacts,
            deepest_rapid_contact_mm: swept.deepest_rapid_contact_mm,
        }
    }

    /// Sweep a path through the block exactly as [`Stock::apply`] does, without
    /// measuring the volume removed, and say which cells it lowered.
    ///
    /// For playback, which sweeps a few moves at a time: two passes over the whole
    /// grid for the volume would cost more than the sweep.
    pub fn sweep<T: SweptTool>(&mut self, moves: &[SimMove], tool: &T) -> Swept {
        let mut report = Swept::default();
        let pitch = self.field.pitch();
        let step = (pitch * 0.5).max(1.0e-6);

        let mut at: Option<DVec3> = None;
        let mut first = true;
        for m in moves {
            let from = match at {
                Some(p) => p,
                None => {
                    at = Some(m.to);
                    continue;
                }
            };
            let rise = (m.to.z - from.z).abs();
            let run = (m.to - from).truncate().length();
            let (worst, touched) = if rise <= STRAIGHT {
                // Level: exact, however long the move.
                self.stamp_level(from, m.to, tool)
            } else if run <= STRAIGHT {
                // Straight up or down: the tool is lowest at one end, and that end
                // cuts everything the rest of the move could.
                self.stamp(if from.z < m.to.z { from } else { m.to }, tool)
            } else {
                self.stamp_along(from, m.to, tool, step, first)
            };
            first = false;
            report.touched = CellRect::join(report.touched, touched);
            if m.class == MoveClass::Rapid && worst > pitch * 0.01 {
                report.rapid_contacts += 1;
                report.deepest_rapid_contact_mm = report.deepest_rapid_contact_mm.max(worst);
            }
            at = Some(m.to);
        }
        report
    }

    /// Sweep a move that climbs or descends as it goes, sampled at no more than
    /// `step` along it.
    fn stamp_along<T: SweptTool>(
        &mut self,
        from: DVec3,
        to: DVec3,
        tool: &T,
        step: f64,
        first: bool,
    ) -> (f64, Option<CellRect>) {
        let span = (to - from).length();
        // Bounded and clamped, so every later conversion is exact: a pathological
        // coordinate cannot turn one move into an unbounded loop.
        let steps = {
            let n = (span / step).ceil();
            if !n.is_finite() || n < 1.0 {
                1u32
            } else {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let n = n.min(f64::from(MAX_SWEEP_STEPS)) as u32;
                n
            }
        };
        let mut worst = 0.0f64;
        let mut touched = None;
        // The start was stamped as the end of the move before, unless this is the
        // first move swept; stamping lowers to a minimum, so doing it again would
        // change nothing, and leaving it out changes nothing either.
        for k in u32::from(!first)..=steps {
            let t = f64::from(k) / f64::from(steps);
            let (cut, cells) = self.stamp(from + (to - from) * t, tool);
            worst = worst.max(cut);
            touched = CellRect::join(touched, cells);
        }
        (worst, touched)
    }

    /// Sweep a level move exactly: every cell within the tool's reach of the
    /// move's line is lowered to the tool's underside at its distance from the
    /// line, since a tool's profile only rises away from its axis.
    fn stamp_level<T: SweptTool>(
        &mut self,
        from: DVec3,
        to: DVec3,
        tool: &T,
    ) -> (f64, Option<CellRect>) {
        let r = tool.max_radius();
        let Some(rect) = CellRect::join(self.footprint(from, r), self.footprint(to, r)) else {
            return (0.0, None);
        };
        let (a, b) = (from.truncate(), to.truncate());
        let z = from.z.min(to.z);
        let pitch = self.field.pitch();
        let origin = self.field.origin();
        let floor = self.floor;
        let nx = self.field.nx() as usize;
        // One row: how deep it cut, and the first and last columns it lowered.
        let row = |iy: u32, cells: &mut [f32]| -> Option<(f64, u32, u32)> {
            let y = origin.y + f64::from(iy) * pitch;
            let (lo, hi) = capsule_row(a, b, r, y)?;
            // A cell either side of the span, so rounding cannot drop the rim; the
            // distance test below decides.
            let first = ((lo - origin.x) / pitch).floor() - 1.0;
            let last = ((hi - origin.x) / pitch).ceil() + 1.0;
            let (x0, x1) = (f64::from(rect.x0), f64::from(rect.x1));
            if !first.is_finite() || !last.is_finite() || last < x0 || first > x1 {
                return None;
            }
            // Clamped into the rectangle, so the narrowing is exact.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let (i0, i1) = (first.max(x0) as u32, last.min(x1) as u32);
            let mut cut: Option<(f64, u32, u32)> = None;
            for ix in i0..=i1 {
                let p = origin + dvec2(f64::from(ix) * pitch, f64::from(iy) * pitch);
                let Some(h) = tool.height_at(distance_to_segment(p, a, b)) else {
                    continue;
                };
                let under = (z + h).max(floor);
                let Some(slot) = cells.get_mut(ix as usize) else {
                    continue;
                };
                let cur = f64::from(*slot);
                if cur.is_finite() && under < cur {
                    *slot = narrow(under);
                    let (d, lo, _) = cut.unwrap_or((0.0, ix, ix));
                    cut = Some((d.max(cur - under), lo, ix));
                }
            }
            cut
        };
        let rows = self
            .field
            .data_mut()
            .get_mut(rect.y0 as usize * nx..(rect.y1 as usize + 1) * nx)
            .unwrap_or_default();
        // Rows are independent, and each cell is worked out alone, so a move over
        // many cells (a long pass on a sheet) is cut in parallel; a short one is
        // not worth handing out.
        let cells = u64::from(rect.width()) * u64::from(rect.height());
        let cuts: Vec<Option<(f64, u32, u32)>> = if cells >= PARALLEL_CELLS {
            rows.par_chunks_mut(nx)
                .enumerate()
                .map(|(i, cells)| row(rect.y0 + u32::try_from(i).unwrap_or(0), cells))
                .collect()
        } else {
            rows.chunks_mut(nx)
                .zip(rect.y0..=rect.y1)
                .map(|(cells, iy)| row(iy, cells))
                .collect()
        };
        let mut deepest = 0.0f64;
        let mut touched: Option<CellRect> = None;
        for (iy, cut) in (rect.y0..).zip(cuts) {
            if let Some((d, lo, hi)) = cut {
                deepest = deepest.max(d);
                let r = CellRect {
                    x0: lo,
                    y0: iy,
                    x1: hi,
                    y1: iy,
                };
                touched = Some(touched.map_or(r, |t| t.union(r)));
            }
        }
        (deepest, touched)
    }

    /// The heights of the cells in `rect`, row by row, exactly as stored.
    pub fn read_cells(&self, rect: CellRect) -> Vec<f32> {
        let nx = self.field.nx() as usize;
        let data = self.field.data();
        let mut out = Vec::with_capacity(rect.width() as usize * rect.height() as usize);
        for iy in rect.y0..=rect.y1 {
            let row = iy as usize * nx;
            if let Some(cells) = data.get(row + rect.x0 as usize..=row + rect.x1 as usize) {
                out.extend_from_slice(cells);
            }
        }
        out
    }

    /// Put back cells read by [`Stock::read_cells`] from the same `rect`.
    pub fn write_cells(&mut self, rect: CellRect, cells: &[f32]) {
        let nx = self.field.nx() as usize;
        let w = rect.width() as usize;
        let data = self.field.data_mut();
        for (iy, src) in (rect.y0..=rect.y1).zip(cells.chunks_exact(w)) {
            let row = iy as usize * nx;
            if let Some(dst) = data.get_mut(row + rect.x0 as usize..=row + rect.x1 as usize) {
                dst.copy_from_slice(src);
            }
        }
    }

    /// The cells a tool of `radius` at `tip` could reach, clamped to the grid,
    /// inclusive: the rim's own cell is in it.
    pub fn footprint(&self, tip: DVec3, radius: f64) -> Option<CellRect> {
        if !tip.is_finite() || !radius.is_finite() || radius <= 0.0 {
            return None;
        }
        let pitch = self.field.pitch();
        let origin = self.field.origin();
        let (nx, ny) = (self.field.nx(), self.field.ny());
        let idx = |v: f64, n: u32| -> u32 {
            let last = n.saturating_sub(1);
            if !v.is_finite() || v <= 0.0 {
                0
            } else if v >= f64::from(last) {
                last
            } else {
                // Non-negative and below `last`, so the narrowing is exact.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let v = v as u32;
                v
            }
        };
        Some(CellRect {
            x0: idx(((tip.x - radius - origin.x) / pitch).floor(), nx),
            x1: idx(((tip.x + radius - origin.x) / pitch).ceil(), nx),
            y0: idx(((tip.y - radius - origin.y) / pitch).floor(), ny),
            y1: idx(((tip.y + radius - origin.y) / pitch).ceil(), ny),
        })
    }

    /// Lower the field to the underside of a tool at `tip`, returning how far the
    /// deepest cell was cut and which cells were lowered.
    fn stamp<T: SweptTool>(&mut self, tip: DVec3, tool: &T) -> (f64, Option<CellRect>) {
        let r = tool.max_radius();
        if !tip.is_finite() || !r.is_finite() || r <= 0.0 {
            return (0.0, None);
        }
        // The footprint's bounds are inclusive. An exclusive upper bound leaves
        // the cell exactly at the cutter's rim unstamped, which under-removes
        // material -- and under-removal is the dangerous direction, because it can
        // hide a gouge.
        let Some(CellRect { x0, y0, x1, y1 }) = self.footprint(tip, r) else {
            return (0.0, None);
        };

        let floor = self.floor;
        let (origin, pitch, nx) = (self.field.origin(), self.field.pitch(), self.field.nx());
        // A hair over the radius squared: a cell this rejects, the tool's own
        // profile would have too.
        let reach2 = r * r * (1.0 + 1.0e-9);
        let data = self.field.data_mut();
        let mut deepest = 0.0f64;
        let mut touched: Option<CellRect> = None;
        for iy in y0..=y1 {
            let py = origin.y + f64::from(iy) * pitch;
            let dy = py - tip.y;
            let across = reach2 - dy * dy;
            if across < 0.0 {
                continue;
            }
            // Only the columns the tool's circle spans on this row, and one more
            // each side for rounding; the profile decides.
            let w = across.sqrt();
            let first = (((tip.x - w - origin.x) / pitch).floor() - 1.0).max(f64::from(x0));
            let last = (((tip.x + w - origin.x) / pitch).ceil() + 1.0).min(f64::from(x1));
            if first.is_nan() || last.is_nan() || first > last {
                continue;
            }
            // Within the footprint, so the narrowing is exact.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let (first, last) = (first as u32, last as u32);
            let row = iy as usize * nx as usize;
            for ix in first..=last {
                let px = origin.x + f64::from(ix) * pitch;
                let d = (dvec2(px, py) - dvec2(tip.x, tip.y)).length();
                let Some(h) = tool.height_at(d) else { continue };
                let under = (tip.z + h).max(floor);
                let Some(slot) = data.get_mut(row + ix as usize) else {
                    continue;
                };
                let cur = f64::from(*slot);
                if cur.is_finite() && under < cur {
                    deepest = deepest.max(cur - under);
                    *slot = narrow(under);
                    let cell = CellRect::cell(ix, iy);
                    touched = Some(touched.map_or(cell, |t| t.union(cell)));
                }
            }
        }
        (deepest, touched)
    }

    /// Volume of material remaining, in cubic millimetres.
    pub fn volume(&self) -> f64 {
        let cell = self.field.pitch() * self.field.pitch();
        let floor = self.floor;
        self.field
            .data()
            .par_iter()
            .map(|v| {
                let h = f64::from(*v);
                if h.is_finite() {
                    (h - floor).max(0.0)
                } else {
                    0.0
                }
            })
            .sum::<f64>()
            * cell
    }

    /// Compare the simulated result against the part it was meant to produce.
    ///
    /// This is the oracle. Below the part by more than `tolerance` is a gouge, which
    /// is a ruined part; well above it is leftover, which is only unfinished. Below
    /// is measured from the lowest the part comes nearby when it was given a reach
    /// ([`PartSurface::reaching`]), so a wall's height is not a gouge beside it.
    pub fn verify(&self, part: &PartSurface, tolerance: f64, leftover_limit: f64) -> VerifyReport {
        let mut r = VerifyReport::default();
        let part_at = part.lookup(&self.field);
        for iy in 0..self.field.ny() {
            for ix in 0..self.field.nx() {
                let (Some(p), Some(stock)) = (self.field.position(ix, iy), self.field.get(ix, iy))
                else {
                    continue;
                };
                let Some((target, under)) = part_at(ix, iy, p, stock, tolerance) else {
                    continue;
                };
                r.compared += 1;
                let delta = stock - target;
                if under > tolerance {
                    r.gouged += 1;
                    r.deepest_gouge_mm = r.deepest_gouge_mm.max(under);
                } else if delta > leftover_limit {
                    r.leftover += 1;
                    r.tallest_leftover_mm = r.tallest_leftover_mm.max(delta);
                }
            }
        }
        r
    }

    /// Write the field as a 16-bit grayscale PGM, for eyeballing.
    ///
    /// Cheap to produce and worth a great deal when a number looks wrong and the
    /// question is *where*.
    pub fn to_pgm(&self) -> String {
        let (nx, ny) = (self.field.nx(), self.field.ny());
        let mut lo = f64::MAX;
        let mut hi = f64::MIN;
        for v in self.field.data() {
            let h = f64::from(*v);
            if h.is_finite() {
                lo = lo.min(h);
                hi = hi.max(h);
            }
        }
        let span = if hi > lo { hi - lo } else { 1.0 };
        let mut s = format!("P2\n{nx} {ny}\n65535\n");
        for iy in (0..ny).rev() {
            for ix in 0..nx {
                let v = self.field.get(ix, iy).unwrap_or(lo);
                let t = ((v - lo) / span * 65535.0).clamp(0.0, 65535.0);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let t = t as u32;
                s.push_str(&t.to_string());
                s.push(' ');
            }
            s.push('\n');
        }
        s
    }
}

/// The part's own upper surface, sampled per column.
///
/// Not a cutter-location surface: this is the shape the program is supposed to
/// leave behind, which is what a gouge is measured against.
///
/// # Gouges beside walls
///
/// A column's height alone cannot judge material beside a vertical wall. The
/// meshed part and the cut differ sideways by up to the tolerance (a hole meshed
/// as a polygon inside its circle, a cell straddling a wall), and sideways beside
/// a wall is the wall's whole height down. So a surface given a reach
/// ([`PartSurface::reaching`]) judges material against the lowest the part comes
/// within that distance across: material is gouged only when it is below that by
/// more than the budget, which holds the tolerance in every direction rather than
/// only downwards. A real gouge into a wall, deeper than that distance, is still
/// below the wall's foot and still found. The lowest nearby is looked for only
/// where material is already that far below the column's own height, which is
/// rare, so it is not kept for every column.
#[derive(Debug, Clone)]
pub struct PartSurface {
    /// The part's window of the grid: its heights, unset where there is none.
    field: HeightMap,
    /// The window's first cell, in the grid.
    at: (u32, u32),
    /// The grid it is judged on: its first cell, pitch, columns and rows.
    grid: (DVec2, f64, u32, u32),
    /// Offsets, in cells, to every cell within the reach across.
    disc: Vec<(i64, i64)>,
}

impl PartSurface {
    /// Rasterise the top surface of a mesh over `[min, max]`.
    pub fn from_mesh(
        mesh: &TriMesh,
        min: DVec2,
        max: DVec2,
        pitch: f64,
    ) -> Result<Self, StockError> {
        // The grid it is judged on, the stock's: as `HeightMap::covering` lays one
        // out, but not allocated, since the part may cover little of it.
        if !pitch.is_finite() || pitch <= 0.0 {
            return Err(HeightMapError::BadPitch(pitch).into());
        }
        let origin = min;
        let span = (max - min).max(DVec2::ZERO);
        let cells = |s: f64| -> u32 {
            let n = (s / pitch).ceil() + 1.0;
            if n.is_finite() && n >= 1.0 {
                // Clamped into range and finite, so the narrowing is exact.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let n = n.min(f64::from(u32::MAX)) as u32;
                n
            } else {
                1
            }
        };
        let (nx, ny) = (cells(span.x), cells(span.y));

        // Only the cells the part's footprint reaches are kept: its window, a
        // hair wider than the triangles so a column its edge passes through is
        // not missed. Past the window there is no part.
        const SLACK: f64 = 1.0e-6;
        let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for i in 0..mesh.len() {
            if let Some(t) = mesh.triangle(i) {
                for p in t {
                    lo = lo.min(p.truncate());
                    hi = hi.max(p.truncate());
                }
            }
        }
        // A column of the grid for an offset `v` from its first cell.
        let index = |v: f64, n: u32, round: fn(f64) -> f64| -> u32 {
            let i = round(v / pitch);
            // Clamped into the grid, so the narrowing is exact.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let i = if i.is_finite() {
                i.clamp(0.0, f64::from(n - 1)) as u32
            } else {
                0
            };
            i
        };
        let (x0, x1, y0, y1) = if lo.x <= hi.x && lo.y <= hi.y {
            (
                index(lo.x - SLACK - origin.x, nx, f64::floor),
                index(hi.x + SLACK - origin.x, nx, f64::ceil),
                index(lo.y - SLACK - origin.y, ny, f64::floor),
                index(hi.y + SLACK - origin.y, ny, f64::ceil),
            )
        } else {
            (0, 0, 0, 0)
        };
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        let mut field = HeightMap::filled(
            origin + dvec2(f64::from(x0) * pitch, f64::from(y0) * pitch),
            pitch,
            w,
            h,
            f64::NAN,
        )?;

        // Each of the window's rows' triangles: those whose extent in Y reaches it.
        let mut rows: Vec<Vec<usize>> = vec![Vec::new(); h as usize];
        for i in 0..mesh.len() {
            let Some(t) = mesh.triangle(i) else { continue };
            let lo = t[0].y.min(t[1].y).min(t[2].y) - SLACK;
            let hi = t[0].y.max(t[1].y).max(t[2].y) + SLACK;
            let first = ((lo - origin.y) / pitch).ceil().max(f64::from(y0));
            let last = ((hi - origin.y) / pitch).floor().min(f64::from(y1));
            if !(first.is_finite() && last.is_finite()) || first > last {
                continue;
            }
            // Clamped into the window, so the narrowing is exact.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let (first, last) = ((first as u32 - y0) as usize, (last as u32 - y0) as usize);
            for row in rows.get_mut(first..=last).into_iter().flatten() {
                row.push(i);
            }
        }

        // Per column, the highest of the triangles covering it, each column at
        // the stock grid's own position for it. Parallel over rows, each writing
        // only its own slice, so the result does not depend on thread count; and
        // the maximum does not depend on the order it is taken.
        field
            .data_mut()
            .par_chunks_mut(w as usize)
            .zip(rows.par_iter())
            .zip(y0..y1 + 1)
            .for_each(|((row, tris), iy)| {
                let y = origin.y + f64::from(iy) * pitch;
                for &i in tris {
                    let Some(t) = mesh.triangle(i) else { continue };
                    let lo = t[0].x.min(t[1].x).min(t[2].x) - SLACK;
                    let hi = t[0].x.max(t[1].x).max(t[2].x) + SLACK;
                    let first = ((lo - origin.x) / pitch).ceil().max(f64::from(x0));
                    let last = ((hi - origin.x) / pitch).floor().min(f64::from(x1));
                    if !(first.is_finite() && last.is_finite()) || first > last {
                        continue;
                    }
                    // Clamped into the window, so the narrowing is exact.
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let (first, last) = (first as u32, last as u32);
                    for ix in first..=last {
                        let x = origin.x + f64::from(ix) * pitch;
                        let (Some(z), Some(cell)) =
                            (triangle_height(&t, x, y), row.get_mut((ix - x0) as usize))
                        else {
                            continue;
                        };
                        let z = narrow(z);
                        if cell.is_nan() || z > *cell {
                            *cell = z;
                        }
                    }
                }
            });
        Ok(Self {
            field,
            at: (x0, y0),
            grid: (origin, pitch, nx, ny),
            disc: Vec::new(),
        })
    }

    /// Height of the part at a column, or `None` where the part is absent.
    pub fn height_at(&self, x: f64, y: f64) -> Option<f64> {
        self.field.sample(x, y)
    }

    /// Judge gouges allowing `reach` across as well as down: see the type's docs.
    #[must_use]
    pub fn reaching(mut self, reach: f64) -> Self {
        let cells = if reach.is_finite() && reach > 0.0 {
            reach / self.field.pitch()
        } else {
            0.0
        };
        #[allow(clippy::cast_possible_truncation)]
        let k = cells.floor().min(64.0) as i64;
        self.disc = (-k..=k)
            .flat_map(|dy| (-k..=k).map(move |dx| (dx, dy)))
            .filter(|&(dx, dy)| {
                #[allow(clippy::cast_precision_loss)]
                let d2 = (dx * dx + dy * dy) as f64;
                d2 <= cells * cells + 1.0e-9
            })
            .collect();
        self
    }

    /// The part's height at cell `(ix, iy)` of the grid it is judged on, if it
    /// has one there.
    fn at_cell(&self, ix: i64, iy: i64) -> Option<f64> {
        let (x, y) = (ix - i64::from(self.at.0), iy - i64::from(self.at.1));
        self.field
            .get(u32::try_from(x).ok()?, u32::try_from(y).ok()?)
    }

    /// The lowest the part comes within the reach of cell `(ix, iy)`, no higher
    /// than `own`: minus infinity beside a column with no part, or past the grid's
    /// edge, where there is no part either.
    fn floor_at(&self, ix: i64, iy: i64, own: f64) -> f64 {
        let mut low = own;
        for &(dx, dy) in &self.disc {
            match self.at_cell(ix + dx, iy + dy) {
                Some(v) => low = low.min(v),
                None => return f64::NEG_INFINITY,
            }
        }
        low
    }

    /// The grid's cell nearest `(x, y)`.
    fn nearest(&self, x: f64, y: f64) -> Option<(i64, i64)> {
        let (o, pitch, _, _) = self.grid;
        let (fx, fy) = (((x - o.x) / pitch).round(), ((y - o.y) / pitch).round());
        if !(fx.is_finite() && fy.is_finite()) || fx.abs() > 1.0e15 || fy.abs() > 1.0e15 {
            return None;
        }
        #[allow(clippy::cast_possible_truncation)]
        let cell = (fx as i64, fy as i64);
        Some(cell)
    }

    /// The lowest the part comes near `(x, y)`, within the reach it was given
    /// ([`PartSurface::reaching`]), or its own height if it was given none. Minus
    /// infinity beside a column with no part; `None` where there is no part.
    pub fn floor_near(&self, x: f64, y: f64) -> Option<f64> {
        let own = self.height_at(x, y)?;
        Some(match self.nearest(x, y) {
            Some((ix, iy)) => self.floor_at(ix, iy, own),
            None => own,
        })
    }

    /// The part's window of the grid, and where its first cell is in the grid.
    pub fn window(&self) -> (&HeightMap, (u32, u32)) {
        (&self.field, self.at)
    }

    /// The position of cell `(ix, iy)` of the grid the part is judged on, worked
    /// out as the grid works it out.
    pub fn position(&self, ix: u32, iy: u32) -> DVec2 {
        let (o, pitch, _, _) = self.grid;
        o + dvec2(f64::from(ix) * pitch, f64::from(iy) * pitch)
    }

    /// Whether this surface is judged on exactly the cells of `field`, so a
    /// column can be read by its index rather than by position.
    pub fn on_grid_of(&self, field: &HeightMap) -> bool {
        let (o, pitch, nx, ny) = self.grid;
        nx == field.nx()
            && ny == field.ny()
            && pitch.to_bits() == field.pitch().to_bits()
            && o == field.origin()
    }

    /// How to judge material standing on a cell of `field` against the part: a
    /// function of the cell, its position and the material's height there, and
    /// the budget, giving the part's height and how far the material is below the
    /// part (from the lowest the part comes nearby, where that could matter), or
    /// `None` where there is no part. Everything that judges material finds the
    /// part this way, so they agree cell for cell.
    pub fn lookup<'a>(
        &'a self,
        field: &HeightMap,
    ) -> impl Fn(u32, u32, DVec2, f64, f64) -> Option<(f64, f64)> + 'a {
        let same = self.on_grid_of(field);
        move |ix, iy, p, stock, budget| {
            let (own, cell) = if same {
                let cell = (i64::from(ix), i64::from(iy));
                (self.at_cell(cell.0, cell.1)?, Some(cell))
            } else {
                (self.height_at(p.x, p.y)?, self.nearest(p.x, p.y))
            };
            let under = own - stock;
            // The lowest nearby is no higher than the column itself, so it only
            // matters where the material is already too deep below the column.
            if under <= budget {
                return Some((own, under));
            }
            let floor = cell.map_or(own, |(x, y)| self.floor_at(x, y, own));
            Some((own, floor - stock))
        }
    }
}

/// Highest point of the mesh directly above `(x, y)`, by looking at every
/// triangle: what [`PartSurface::from_mesh`] must agree with.
#[cfg(test)]
fn column_height(mesh: &TriMesh, x: f64, y: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    for i in 0..mesh.len() {
        let Some(t) = mesh.triangle(i) else { continue };
        if let Some(z) = triangle_height(&t, x, y) {
            best = Some(match best {
                Some(b) if b >= z => b,
                _ => z,
            });
        }
    }
    best
}

/// Height of a triangle above `(x, y)`, or `None` if it does not cover the column.
fn triangle_height(t: &[DVec3; 3], x: f64, y: f64) -> Option<f64> {
    let (a, b, c) = (t[0], t[1], t[2]);
    let (ab, ac) = (b - a, c - a);
    let det = ab.x * ac.y - ab.y * ac.x;
    if det.abs() < 1.0e-18 {
        // Vertical or degenerate: contributes no column height of its own, and its
        // edges belong to the neighbouring faces.
        return None;
    }
    let (px, py) = (x - a.x, y - a.y);
    let u = (px * ac.y - py * ac.x) / det;
    let v = (ab.x * py - ab.y * px) / det;
    const E: f64 = 1.0e-9;
    if u >= -E && v >= -E && u + v <= 1.0 + E {
        Some(a.z + ab.z * u + ac.z * v)
    } else {
        None
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests {
    use super::*;
    use camcraft_geom::synthetic;
    use glam::dvec3;

    /// A ball of radius r, standing in for the real cutter.
    struct Ball(f64);
    impl SweptTool for Ball {
        fn max_radius(&self) -> f64 {
            self.0
        }
        fn height_at(&self, d: f64) -> Option<f64> {
            (d <= self.0).then(|| self.0 - (self.0 * self.0 - d * d).max(0.0).sqrt())
        }
    }

    /// A flat mill of radius r.
    struct Flat(f64);
    impl SweptTool for Flat {
        fn max_radius(&self) -> f64 {
            self.0
        }
        fn height_at(&self, d: f64) -> Option<f64> {
            (d <= self.0).then_some(0.0)
        }
    }

    fn block() -> Stock {
        Stock::block(dvec2(-10.0, -10.0), dvec2(10.0, 10.0), 5.0, -5.0, 0.5).unwrap()
    }

    /// One block through `from_boxes` is the same stock as `block`, cell for cell.
    #[test]
    fn one_box_is_a_block() {
        for (min, max, pitch) in [
            (dvec2(-10.0, -10.0), dvec2(10.0, 10.0), 0.5),
            (dvec2(-3.3, 1.7), dvec2(12.1, 9.05), 0.37),
        ] {
            let a = Stock::block(min, max, 5.0, -5.0, pitch).unwrap();
            let b = Stock::from_boxes(&[(min, max, 5.0)], -5.0, pitch).unwrap();
            assert_eq!(a, b);
        }
    }

    /// Two blocks apart: material over each at its own top, none between, and the
    /// volumes add up.
    #[test]
    fn two_boxes_are_their_union() {
        let a = (dvec2(0.0, 0.0), dvec2(10.0, 10.0), 5.0);
        let b = (dvec2(20.0, 0.0), dvec2(30.0, 10.0), 8.0);
        let s = Stock::from_boxes(&[a, b], 0.0, 0.5).unwrap();
        assert_eq!(s.height_at(5.0, 5.0), Some(5.0));
        assert_eq!(s.height_at(25.0, 5.0), Some(8.0));
        assert_eq!(s.height_at(15.0, 5.0), Some(0.0), "the gap is empty");
        let alone = |(lo, hi, top)| Stock::block(lo, hi, top, 0.0, 0.5).unwrap().volume();
        assert!((s.volume() - (alone(a) + alone(b))).abs() < 1e-6);
        // Overlapping, the taller one wins where they meet.
        let c = (dvec2(5.0, 0.0), dvec2(15.0, 10.0), 9.0);
        let s = Stock::from_boxes(&[a, c], 0.0, 0.5).unwrap();
        assert_eq!(s.height_at(7.0, 5.0), Some(9.0));
        assert_eq!(s.height_at(2.0, 5.0), Some(5.0));
        assert!(matches!(
            Stock::from_boxes(&[], 0.0, 0.5),
            Err(StockError::NoBlocks)
        ));
    }

    #[test]
    fn rejects_an_inverted_block() {
        let e = Stock::block(dvec2(0.0, 0.0), dvec2(1.0, 1.0), 0.0, 5.0, 0.5);
        assert!(matches!(e, Err(StockError::Inverted { .. })));
    }

    #[test]
    fn a_fresh_block_is_full() {
        let s = block();
        assert_eq!(s.height_at(0.0, 0.0), Some(5.0));
        // 20 x 20 x 10, within a cell of rounding on the grown grid.
        assert!((s.volume() - 4000.0).abs() < 400.0, "got {}", s.volume());
    }

    /// The defining behaviour: cutting removes material down to the tool underside.
    #[test]
    fn a_flat_cut_leaves_a_flat_floor() {
        let mut s = block();
        let moves = vec![
            SimMove {
                to: dvec3(-8.0, 0.0, 1.0),
                class: MoveClass::MayCut,
            },
            SimMove {
                to: dvec3(8.0, 0.0, 1.0),
                class: MoveClass::MayCut,
            },
        ];
        let r = s.apply(&moves, &Flat(2.0));
        assert!(r.is_clean());
        assert!(r.removed_mm3 > 0.0);
        assert_eq!(
            s.height_at(0.0, 0.0),
            Some(1.0),
            "floor should be at the tip"
        );
        assert_eq!(
            s.height_at(0.0, 9.0),
            Some(5.0),
            "away from the cut, untouched"
        );
    }

    /// A ball leaves its own radius behind, not a flat floor.
    #[test]
    fn a_ball_leaves_a_round_bottom() {
        let mut s = Stock::block(dvec2(-6.0, -6.0), dvec2(6.0, 6.0), 5.0, -5.0, 0.1).unwrap();
        let moves = vec![
            SimMove {
                to: dvec3(0.0, 0.0, 0.0),
                class: MoveClass::MayCut,
            },
            SimMove {
                to: dvec3(0.0, 0.0, 0.0),
                class: MoveClass::MayCut,
            },
        ];
        s.apply(&moves, &Ball(3.0));
        assert!(
            (s.height_at(0.0, 0.0).unwrap() - 0.0).abs() < 0.05,
            "tip touches bottom"
        );
        // One radius out, the ball surface is a full radius up.
        let side = s.height_at(3.0, 0.0).unwrap();
        assert!(
            (side - 3.0).abs() < 0.2,
            "expected ~3.0 at the equator, got {side}"
        );
    }

    /// The check the plan singles out, and the defect Pngcam documents.
    #[test]
    fn a_rapid_through_material_is_caught() {
        let mut s = block();
        let moves = vec![
            SimMove {
                to: dvec3(-8.0, 0.0, 1.0),
                class: MoveClass::Rapid,
            },
            SimMove {
                to: dvec3(8.0, 0.0, 1.0),
                class: MoveClass::Rapid,
            },
        ];
        let r = s.apply(&moves, &Flat(2.0));
        assert!(
            !r.is_clean(),
            "a rapid ploughing through stock must not pass"
        );
        assert_eq!(r.rapid_contacts, 1);
        assert!(
            r.deepest_rapid_contact_mm > 3.0,
            "got {}",
            r.deepest_rapid_contact_mm
        );
    }

    /// And the same move above the block is fine, or the check would be useless.
    #[test]
    fn a_rapid_above_the_stock_is_fine() {
        let mut s = block();
        let moves = vec![
            SimMove {
                to: dvec3(-8.0, 0.0, 20.0),
                class: MoveClass::Rapid,
            },
            SimMove {
                to: dvec3(8.0, 0.0, 20.0),
                class: MoveClass::Rapid,
            },
        ];
        let r = s.apply(&moves, &Flat(2.0));
        assert!(r.is_clean());
        assert_eq!(r.removed_mm3, 0.0);
    }

    #[test]
    fn part_surface_reads_a_plane() {
        let mesh = synthetic::plane(2.0, 10.0);
        let p = PartSurface::from_mesh(&mesh, dvec2(-5.0, -5.0), dvec2(5.0, 5.0), 0.5).unwrap();
        assert!((p.height_at(0.0, 0.0).unwrap() - 2.0).abs() < 1e-9);
        assert!((p.height_at(3.3, -2.1).unwrap() - 2.0).abs() < 1e-9);
    }

    /// Rasterising triangle by triangle gives every column exactly the height
    /// that looking at every triangle does, NaN where there is none.
    #[test]
    fn part_surface_is_the_highest_triangle_over_each_column() {
        for (mesh, pitch) in [
            (synthetic::uv_sphere(8.0, 40, 60), 0.37),
            (synthetic::step(4.0, 10.0), 0.25),
        ] {
            let (lo, hi) = (dvec2(-9.0, -9.0), dvec2(9.0, 9.0));
            let p = PartSurface::from_mesh(&mesh, lo, hi, pitch).unwrap();
            let grid = HeightMap::covering(lo, hi, pitch, 0.0).unwrap();
            let (mut hit, mut miss) = (0, 0);
            for iy in 0..grid.ny() {
                for ix in 0..grid.nx() {
                    let at = grid.position(ix, iy).unwrap();
                    assert_eq!(p.position(ix, iy), at);
                    let want = column_height(&mesh, at.x, at.y).map(narrow);
                    let got = p
                        .at_cell(i64::from(ix), i64::from(iy))
                        .map_or(f32::NAN, narrow);
                    match want {
                        Some(w) => {
                            assert_eq!(got.to_bits(), w.to_bits(), "({ix}, {iy})");
                            hit += 1;
                        }
                        None => {
                            assert!(got.is_nan(), "({ix}, {iy}) has {got}");
                            miss += 1;
                        }
                    }
                }
            }
            assert!(hit > 100, "{hit} {miss}");
        }
    }

    #[test]
    fn part_surface_reads_a_step() {
        let mesh = synthetic::step(4.0, 10.0);
        let p = PartSurface::from_mesh(&mesh, dvec2(-5.0, -5.0), dvec2(5.0, 5.0), 0.25).unwrap();
        assert!(
            (p.height_at(-3.0, 0.0).unwrap() - 4.0).abs() < 1e-9,
            "plateau"
        );
        assert!(p.height_at(3.0, 0.0).unwrap().abs() < 1e-9, "shelf");
    }

    /// Cutting too deep must be detected, and cutting to the right depth must not.
    #[test]
    fn verify_distinguishes_a_gouge_from_a_good_cut() {
        let mesh = synthetic::plane(0.0, 12.0);
        let part = PartSurface::from_mesh(&mesh, dvec2(-6.0, -6.0), dvec2(6.0, 6.0), 0.5).unwrap();

        let sweep = |z: f64| {
            let mut s = Stock::block(dvec2(-6.0, -6.0), dvec2(6.0, 6.0), 5.0, -5.0, 0.5).unwrap();
            let mut moves = Vec::new();
            let mut y = -5.0;
            while y <= 5.0 {
                moves.push(SimMove {
                    to: dvec3(-5.0, y, z),
                    class: MoveClass::MayCut,
                });
                moves.push(SimMove {
                    to: dvec3(5.0, y, z),
                    class: MoveClass::MayCut,
                });
                y += 1.0;
            }
            s.apply(&moves, &Flat(1.5));
            s
        };

        let good = sweep(0.0).verify(&part, 0.01, 0.5);
        assert!(
            good.is_clean(),
            "cutting to the surface is not a gouge: {good:?}"
        );
        assert!(good.compared > 100);

        let bad = sweep(-1.0).verify(&part, 0.01, 0.5);
        assert!(!bad.is_clean(), "cutting 1 mm under the part is a gouge");
        assert!(
            (bad.deepest_gouge_mm - 1.0).abs() < 0.05,
            "got {}",
            bad.deepest_gouge_mm
        );
    }

    /// Beside a vertical wall, material a hair past the wall is not a gouge the
    /// wall's height deep once the part is given a reach across; a cut well into
    /// the wall still is.
    #[test]
    fn a_wall_is_judged_across_as_well_as_down() {
        // Plateau at 4 for x < 0, shelf at 0 beyond: a 4 mm wall at x = 0.
        let mesh = synthetic::step(4.0, 10.0);
        let (lo, hi, pitch) = (dvec2(-5.0, -5.0), dvec2(5.0, 5.0), 0.1);
        let column = PartSurface::from_mesh(&mesh, lo, hi, pitch).unwrap();
        let across = column.clone().reaching(0.25);
        // Clear the shelf with a flat whose edge runs `into` past the wall.
        let cut = |into: f64| {
            let mut s = Stock::block(lo, hi, 5.0, -5.0, pitch).unwrap();
            let mut moves = Vec::new();
            for y in [-4.0, -2.0, 0.0, 2.0, 4.0] {
                for x in [1.0 - into, 4.0] {
                    moves.push(SimMove {
                        to: dvec3(x, y, 0.0),
                        class: MoveClass::MayCut,
                    });
                }
            }
            s.sweep(&moves, &Flat(1.0));
            s
        };
        let shaved = cut(0.15);
        assert!(
            shaved.verify(&column, 0.1, 50.0).gouged > 0,
            "by columns alone"
        );
        let v = shaved.verify(&across, 0.1, 50.0);
        assert_eq!(v.gouged, 0, "{v:?}");
        let v = cut(1.0).verify(&across, 0.1, 50.0);
        assert!(v.gouged > 0 && v.deepest_gouge_mm > 3.9, "{v:?}");
        // Where nothing is near a wall, the floor is the column's own height.
        assert_eq!(across.floor_near(-3.0, 0.0), column.height_at(-3.0, 0.0));
        // Past the grid's edge there is no part: a column at the edge, where a
        // part's edge meets a stock with no margin, may be cut down past it.
        assert_eq!(across.floor_near(-5.0, 0.0), Some(f64::NEG_INFINITY));
    }

    /// Uncut material is reported as leftover, not as an error.
    #[test]
    fn untouched_stock_is_leftover_not_a_gouge() {
        let mesh = synthetic::plane(0.0, 12.0);
        let part = PartSurface::from_mesh(&mesh, dvec2(-6.0, -6.0), dvec2(6.0, 6.0), 0.5).unwrap();
        let s = Stock::block(dvec2(-6.0, -6.0), dvec2(6.0, 6.0), 5.0, -5.0, 0.5).unwrap();
        let r = s.verify(&part, 0.01, 0.5);
        assert!(r.is_clean(), "leftover is unfinished, not ruined");
        assert!(r.leftover > 100);
        assert!((r.tallest_leftover_mm - 5.0).abs() < 1e-6);
    }

    #[test]
    fn writes_a_readable_pgm() {
        let s = block();
        let p = s.to_pgm();
        assert!(p.starts_with("P2\n"));
        assert!(p.lines().count() > 3);
    }

    /// A zigzag with a rapid and a ramp in it, over the whole block.
    fn zigzag() -> Vec<SimMove> {
        let mut moves = vec![SimMove {
            to: dvec3(-9.0, -9.0, 8.0),
            class: MoveClass::Rapid,
        }];
        let (mut y, mut z, mut sign) = (-7.0, 3.0, 1.0);
        while y <= 7.0 {
            for x in [-7.0, 6.5] {
                moves.push(SimMove {
                    to: dvec3(x * sign, y, z),
                    class: MoveClass::MayCut,
                });
            }
            y += 2.3;
            z -= 0.4;
            sign = -sign;
        }
        moves
    }

    /// Every cell a sweep lowers is inside the rectangle it reports, and it lowers
    /// exactly what `apply` does.
    #[test]
    fn a_sweep_says_where_it_cut() {
        let moves = zigzag();
        for cut in [1..4, 3..9, 0..moves.len(), 7..8] {
            let slice = moves.get(cut).unwrap();
            let mut applied = block();
            applied.apply(slice, &Ball(1.7));
            let mut swept = block();
            let before = swept.clone();
            let touched = swept.sweep(slice, &Ball(1.7)).touched;
            assert_eq!(swept, applied, "sweep and apply must leave the same stock");
            let f = swept.field();
            let mut changed = 0;
            for iy in 0..f.ny() {
                for ix in 0..f.nx() {
                    if f.get(ix, iy) != before.field().get(ix, iy) {
                        changed += 1;
                        assert!(
                            touched.is_some_and(|r| r.contains(ix, iy)),
                            "cell ({ix}, {iy}) changed outside {touched:?}"
                        );
                    }
                }
            }
            assert_eq!(touched.is_some(), changed > 0);
        }
    }

    /// A level move is swept exactly: every cell ends at the tool's underside at
    /// its distance from the move's line, checked cell by cell against a brute
    /// force over the whole grid, at every angle; and sampling finely along the
    /// move never cuts deeper.
    #[test]
    fn a_level_move_is_swept_exactly() {
        let ball = Ball(1.7);
        for (a, b) in [
            ((-8.0, -3.0), (7.5, 2.2)),
            ((0.3, -9.0), (0.3, 9.0)),
            ((-9.0, 4.1), (9.0, 4.1)),
            ((5.0, 5.0), (-6.0, -7.0)),
            ((1.1, 1.3), (1.1, 1.3)),
        ] {
            let (from, to) = (dvec3(a.0, a.1, 2.0), dvec3(b.0, b.1, 2.0));
            let moves = [
                SimMove {
                    to: from,
                    class: MoveClass::MayCut,
                },
                SimMove {
                    to,
                    class: MoveClass::MayCut,
                },
            ];
            let mut exact = block();
            exact.sweep(&moves, &ball);
            let mut sampled = block();
            sampled.stamp_along(from, to, &ball, 0.01, true);
            let f = exact.field();
            for iy in 0..f.ny() {
                for ix in 0..f.nx() {
                    let p = f.position(ix, iy).unwrap();
                    let d = distance_to_segment(p, from.truncate(), to.truncate());
                    let want = ball.height_at(d).map_or(5.0, |h| (2.0 + h).min(5.0));
                    let got = f.get(ix, iy).unwrap();
                    assert!((got - want).abs() < 1e-5, "({ix}, {iy}): {got} for {want}");
                    let s = sampled.field().get(ix, iy).unwrap();
                    assert!(s + 1e-6 >= got, "sampling cut deeper at ({ix}, {iy})");
                    assert!(s - got < 0.01, "sampling left {} at ({ix}, {iy})", s - got);
                }
            }
        }
    }

    /// Straight down is the tool at the bottom, whatever the pitch.
    #[test]
    fn a_plunge_is_the_tool_at_its_lowest() {
        let mut plunged = block();
        let down = [
            SimMove {
                to: dvec3(1.0, 1.0, 8.0),
                class: MoveClass::MayCut,
            },
            SimMove {
                to: dvec3(1.0, 1.0, 0.5),
                class: MoveClass::MayCut,
            },
        ];
        plunged.sweep(&down, &Ball(2.0));
        let mut stamped = block();
        stamped.stamp(dvec3(1.0, 1.0, 0.5), &Ball(2.0));
        assert_eq!(plunged, stamped);
    }

    /// A packed stock is the stock it came from, bit for bit, small when the
    /// stock is flat; and restoring it, or another stock, in place says which
    /// cells changed.
    #[test]
    fn a_packed_stock_is_the_stock_bit_for_bit() {
        let fresh = block();
        let mut cut = block();
        cut.sweep(&zigzag(), &Ball(1.7));
        for s in [&fresh, &cut] {
            let p = PackedStock::pack(s);
            assert_eq!(&p.unpack().unwrap(), s);
        }
        let flat = PackedStock::pack(&fresh);
        assert!(
            flat.bytes() < 64,
            "a flat block is one run: {}",
            flat.bytes()
        );
        // Restoring the fresh block over the cut one changes exactly the cut cells.
        let mut back = cut.clone();
        let touched = block().sweep(&zigzag(), &Ball(1.7)).touched;
        assert_eq!(flat.restore(&mut back), Some(touched));
        assert_eq!(back, fresh);
        let mut again = fresh.clone();
        assert_eq!(again.restore_from(&cut), Some(touched));
        assert_eq!(again, cut);
        assert_eq!(
            again.restore_from(&cut),
            Some(None),
            "nothing left to change"
        );
    }

    /// Cells read out and written back are the same bits, and only those move.
    #[test]
    fn cells_read_and_written_back_are_exact() {
        let mut s = block();
        let rect = CellRect {
            x0: 3,
            y0: 5,
            x1: 17,
            y1: 9,
        };
        let saved = s.read_cells(rect);
        assert_eq!(saved.len(), (rect.width() * rect.height()) as usize);
        let before = s.clone();
        s.sweep(&zigzag(), &Flat(2.0));
        let after = s.clone();
        s.write_cells(rect, &saved);
        let f = s.field();
        for iy in 0..f.ny() {
            for ix in 0..f.nx() {
                let want = if rect.contains(ix, iy) {
                    &before
                } else {
                    &after
                };
                assert_eq!(f.get(ix, iy), want.field().get(ix, iy));
            }
        }
    }

    #[test]
    fn a_footprint_covers_the_rim_and_stays_in_the_grid() {
        let s = block();
        // Pitch 0.5 from -10: a radius-1 tool at the origin reaches cells 18..=22.
        let r = s.footprint(dvec3(0.0, 0.0, 0.0), 1.0).unwrap();
        assert_eq!((r.x0, r.x1, r.y0, r.y1), (18, 22, 18, 22));
        let edge = s.footprint(dvec3(-10.0, 30.0, 0.0), 1.0).unwrap();
        assert_eq!((edge.x0, edge.y1), (0, s.field().ny() - 1));
        assert!(s.footprint(dvec3(f64::NAN, 0.0, 0.0), 1.0).is_none());
    }

    #[test]
    fn survives_non_finite_positions_without_panicking() {
        let mut s = block();
        let moves = vec![
            SimMove {
                to: dvec3(0.0, 0.0, 0.0),
                class: MoveClass::MayCut,
            },
            SimMove {
                to: dvec3(f64::NAN, 0.0, 0.0),
                class: MoveClass::MayCut,
            },
        ];
        let _ = s.apply(&moves, &Flat(1.0));
    }
}
