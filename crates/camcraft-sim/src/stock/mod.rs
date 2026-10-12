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
//! Simulating the emitted program and comparing the result with the part is the
//! primary regression test for the whole system: it finds real defects without
//! anyone reading G-code.
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

mod packed;
mod part;
mod sweep;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests;

use camcraft_geom::arc::Arc;
use camcraft_geom::heightmap::{HeightMap, HeightMapError, cell_index};
use glam::{DVec2, DVec3};
use rayon::prelude::*;

pub use packed::PackedStock;
pub use part::PartSurface;

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
    /// The arc it follows there from where the move before it ended, or `None`
    /// for a straight move.
    pub arc: Option<Arc>,
}

impl SimMove {
    /// The first `t` of it, from `from`, as a move of its own.
    pub fn part(&self, from: DVec3, t: f64) -> Self {
        match self.arc {
            None => Self {
                to: from.lerp(self.to, t),
                ..*self
            },
            Some(a) => {
                let (first, to, _) = a.split(from, self.to, t);
                Self {
                    to,
                    arc: Some(first),
                    ..*self
                }
            }
        }
    }

    /// Points along it within `tolerance` of it, from `from`, the start left out
    /// and its end last.
    pub fn chords(&self, from: DVec3, tolerance: f64) -> Vec<DVec3> {
        match self.arc {
            None => vec![self.to],
            Some(a) => a.chords(from, self.to, tolerance),
        }
    }
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

    /// Every cell of a grid `nx` columns across and `ny` rows down.
    pub fn all(nx: u32, ny: u32) -> Self {
        Self {
            x0: 0,
            y0: 0,
            x1: nx.saturating_sub(1),
            y1: ny.saturating_sub(1),
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

/// How a column of material compares with the part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Below the part by more than the budget: a ruined part.
    Gouge,
    /// On the part, within the budget and the leftover limit.
    OnPart,
    /// Further above the part than the leftover limit: where the cutter could not
    /// reach, or has not yet.
    Leftover,
    /// Material the part does not cover.
    OffPart,
}

/// Classify a column of material `stock` high against the part there, as
/// [`PartSurface::lookup`] gives it: `(its height, how far the material is below
/// it)`, or `None` where there is no part. Below by more than `budget` is a
/// gouge; otherwise more than `leftover_limit` above is leftover.
///
/// The rule [`Stock::verify`] counts by, so whatever shows material by it agrees
/// with the verdict cell for cell.
pub fn classify(stock: f64, part: Option<(f64, f64)>, budget: f64, leftover_limit: f64) -> Class {
    let Some((part, under)) = part else {
        return Class::OffPart;
    };
    if under > budget {
        Class::Gouge
    } else if stock - part > leftover_limit {
        Class::Leftover
    } else {
        Class::OnPart
    }
}

/// Cells added up as one part of a volume: fixed, so the parts, and the order
/// they are added in, do not depend on how many threads there are.
const VOLUME_CELLS: usize = 1 << 14;

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
        let span = |lo: f64, hi: f64, o: f64, n: u32| {
            (
                cell_index(((lo - o) / pitch + 1.0e-9).floor(), n),
                cell_index(((hi - o) / pitch - 1.0e-9).ceil(), n),
            )
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

    /// Volume of material remaining, in cubic millimetres.
    ///
    /// Added up in parts of a fixed size in parallel, then the parts in order, so
    /// it comes out the same to the bit on any number of threads.
    pub fn volume(&self) -> f64 {
        let cell = self.field.pitch() * self.field.pitch();
        let floor = self.floor;
        let parts: Vec<f64> = self
            .field
            .data()
            .par_chunks(VOLUME_CELLS)
            .map(|cells| {
                cells
                    .iter()
                    .map(|v| {
                        let h = f64::from(*v);
                        if h.is_finite() {
                            (h - floor).max(0.0)
                        } else {
                            0.0
                        }
                    })
                    .sum::<f64>()
            })
            .collect();
        parts.iter().sum::<f64>() * cell
    }

    /// Compare the simulated result against the part it was meant to produce.
    ///
    /// This is the oracle. Below the part by more than `tolerance` is a gouge, which
    /// is a ruined part; well above it is leftover, which is only unfinished. Below
    /// is measured from the lowest the part comes nearby when it was given a reach
    /// ([`PartSurface::reaching`]), so a wall's height is not a gouge beside it.
    /// Each column is judged by [`classify`].
    ///
    /// Rows are judged in parallel. Their counts add and their extremes are
    /// maxima, so the report does not depend on the order they are combined in.
    pub fn verify(&self, part: &PartSurface, tolerance: f64, leftover_limit: f64) -> VerifyReport {
        let part_at = part.lookup(&self.field);
        let row = |iy: u32| {
            let mut r = VerifyReport::default();
            for ix in 0..self.field.nx() {
                let (Some(p), Some(stock)) = (self.field.position(ix, iy), self.field.get(ix, iy))
                else {
                    continue;
                };
                let near = part_at(ix, iy, p, stock, tolerance);
                let Some((target, under)) = near else {
                    continue;
                };
                r.compared += 1;
                match classify(stock, near, tolerance, leftover_limit) {
                    Class::Gouge => {
                        r.gouged += 1;
                        r.deepest_gouge_mm = r.deepest_gouge_mm.max(under);
                    }
                    Class::Leftover => {
                        r.leftover += 1;
                        r.tallest_leftover_mm = r.tallest_leftover_mm.max(stock - target);
                    }
                    Class::OnPart | Class::OffPart => {}
                }
            }
            r
        };
        (0..self.field.ny())
            .into_par_iter()
            .map(row)
            .reduce(VerifyReport::default, |a, b| VerifyReport {
                compared: a.compared + b.compared,
                gouged: a.gouged + b.gouged,
                deepest_gouge_mm: a.deepest_gouge_mm.max(b.deepest_gouge_mm),
                leftover: a.leftover + b.leftover,
                tallest_leftover_mm: a.tallest_leftover_mm.max(b.tallest_leftover_mm),
            })
    }

    /// Write the field as a 16-bit grayscale PGM, for eyeballing.
    ///
    /// Cheap to produce and worth a great deal when a number looks wrong and the
    /// question is *where*.
    #[cfg(test)]
    pub(crate) fn to_pgm(&self) -> String {
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
