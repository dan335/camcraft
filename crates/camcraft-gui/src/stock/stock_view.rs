//! The simulated material as an image: one height, one code (its class) and one
//! shade per stock cell.
//!
//! Built on the CPU from the simulator's height field, and uploaded to the GPU as
//! textures the stock shader traces its columns through
//! ([`crate::view::viewport`]), in tiles. A new image is uploaded whole; each step
//! of playback only as the cells the tool touched.
//!
//! Every cell is classified by the verifier's own rule, with the verifier's own
//! lookup of the part and its own thresholds (the job's gouge budget and
//! [`LEFTOVER_LIMIT`]). What is red here is exactly what the verdict counted as
//! gouged, and what is amber is what it counted as left over.
//!
//! The material is drawn plain, [`ColorMode::Material`], unless comparing is
//! asked for, so what reads is the shape being cut, with a gouge still red the
//! moment it happens. Comparing, [`ColorMode::Compare`], colours every class; the
//! colour is chosen on the GPU from each cell's code, so switching redraws
//! nothing.

use camcraft_core::run::LEFTOVER_LIMIT;
use camcraft_geom::heightmap::narrow;
pub use camcraft_sim::stock::Class;
use camcraft_sim::stock::{CellRect, PartSurface, Stock};
use glam::DVec2;

use crate::stock::stock_ao::{self, Occluder};
use crate::ui::theme;

/// Every class, in the order the legend lists them.
pub const CLASSES: [Class; 4] = [Class::Gouge, Class::Leftover, Class::OnPart, Class::OffPart];

/// How the material is coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMode {
    /// One material colour, and red below the part.
    #[default]
    Material,
    /// Every class in its own colour, by the verifier's rule.
    Compare,
}

/// Where the leftover colour stops getting stronger, in millimetres above the part.
const LEFTOVER_SATURATES: f64 = 5.0;

/// Classify one column: `stock` height against the part's, if it has one, as
/// `(its height, how far the material is below the part)`, as the part's lookup
/// gives them ([`PartSurface::lookup`]): the second is measured from the lowest
/// the part comes nearby, so a wall's height is not a gouge beside it.
/// By the verifier's own rule ([`camcraft_sim::stock::classify`]).
pub fn classify(stock: f64, part: Option<(f64, f64)>, budget: f64) -> Class {
    camcraft_sim::stock::classify(stock, part, budget, LEFTOVER_LIMIT)
}

/// A class's name, for the legend.
pub fn class_name(class: Class) -> &'static str {
    match class {
        Class::Gouge => "below the part",
        Class::OnPart => "on the part",
        Class::Leftover => "left above the part",
        Class::OffPart => "outside the part",
    }
}

/// A class's colour for the legend: leftover at its strongest.
pub fn class_color(class: Class) -> [u8; 3] {
    match class {
        Class::Gouge => theme::ERR,
        Class::OnPart => theme::GRAY_8,
        Class::Leftover => theme::AMBER_5,
        Class::OffPart => theme::GRAY_5,
    }
}

/// A cell's colour: its class, with leftover shaded by how much is left; or the
/// material's, unless it is a gouge.
pub fn cell_color(mode: ColorMode, class: Class, delta: f64) -> [u8; 3] {
    if mode == ColorMode::Material {
        return match class {
            Class::Gouge => theme::ERR,
            _ => theme::MATERIAL,
        };
    }
    match class {
        Class::Leftover => {
            let t = leftover_shade(delta);
            let mix = |a: u8, b: u8| {
                let (a, b) = (f64::from(a), f64::from(b));
                // Between two bytes, so within a byte.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let v = (a + (b - a) * t).round() as u8;
                v
            };
            let ([r0, g0, b0], [r1, g1, b1]) = (theme::AMBER_7, theme::AMBER_5);
            [mix(r0, r1), mix(g0, g1), mix(b0, b1)]
        }
        other => class_color(other),
    }
}

/// How far into the leftover colour a cell is, from 0 (just over the limit) to 1
/// (at [`LEFTOVER_SATURATES`] or more).
fn leftover_shade(delta: f64) -> f64 {
    ((delta - LEFTOVER_LIMIT) / (LEFTOVER_SATURATES - LEFTOVER_LIMIT)).clamp(0.0, 1.0)
}

/// Shades of leftover a code can carry.
const SHADES: u8 = 64;

/// A cell with no height: counted in no class, drawn as material off the part.
pub const NO_CELL: u8 = u8::MAX;

/// A cell's code, one byte: its class's place in [`CLASSES`] times 64, plus how
/// deep into the leftover colour it is. The colour is chosen on the GPU from the
/// code and the colour mode, so changing the mode redraws nothing.
pub fn code(class: Class, delta: f64) -> u8 {
    let slot = CLASSES.iter().position(|c| *c == class).unwrap_or(3);
    let shade = if class == Class::Leftover {
        // Between 0 and 63, so within a byte.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let s = (leftover_shade(delta) * f64::from(SHADES - 1)).round() as u8;
        s
    } else {
        0
    };
    u8::try_from(slot).unwrap_or(3) * SHADES + shade
}

/// The place in [`CLASSES`] of the class a code stands for, if any.
fn class_index(code: u8) -> Option<usize> {
    (code != NO_CELL).then_some(usize::from(code / SHADES).min(CLASSES.len() - 1))
}

/// The class a code stands for, if any.
#[cfg(test)]
pub fn class_of(code: u8) -> Option<Class> {
    CLASSES.get(class_index(code)?).copied()
}

/// A block of an image's cells to classify: rows `w` cells wide, whose cell
/// (0, 0) is the stock's cell `at`; the cells of `rect` in it; every one, or
/// only those whose height has changed.
struct Block {
    w: u32,
    rect: CellRect,
    at: (u32, u32),
    all: bool,
}

impl Block {
    /// Classify the block's cells of `stock` into `held` heights and `codes`,
    /// its rows. Returns what each class gained, less what it lost.
    fn classify(
        &self,
        stock: &Stock,
        part: &PartSurface,
        budget: f64,
        held: &mut [f32],
        codes: &mut [u8],
    ) -> [isize; 4] {
        let field = stock.field();
        // The verifier's own lookup, so the counts are its counts.
        let part_at = part.lookup(field);
        let (w, at) = (self.w as usize, self.at);
        let rows = u32::try_from(held.len() / w.max(1)).unwrap_or(0);
        let mut gained = [0isize; 4];
        if w == 0 || rows == 0 {
            return gained;
        }
        let stride = field.nx() as usize;
        let (x0, x1) = (self.rect.x0.min(self.w - 1), self.rect.x1.min(self.w - 1));
        let (origin, pitch) = (field.origin(), field.pitch());
        let floor = narrow(stock.floor());
        for ly in self.rect.y0..=self.rect.y1.min(rows - 1) {
            let iy = at.1 + ly;
            let mine = ly as usize * w;
            let theirs = iy as usize * stride + at.0 as usize;
            let (Some(now), Some(held), Some(codes)) = (
                field
                    .data()
                    .get(theirs + x0 as usize..=theirs + x1 as usize),
                held.get_mut(mine + x0 as usize..=mine + x1 as usize),
                codes.get_mut(mine + x0 as usize..=mine + x1 as usize),
            ) else {
                continue;
            };
            let y = origin.y + f64::from(iy) * pitch;
            for ((ix, now), (held, old)) in (at.0 + x0..).zip(now).zip(held.iter_mut().zip(codes)) {
                if !self.all && now.to_bits() == held.to_bits() {
                    continue;
                }
                let h = f64::from(*now);
                let new = if h.is_finite() {
                    let p = DVec2::new(origin.x + f64::from(ix) * pitch, y);
                    let near = part_at(ix, iy, p, h, budget);
                    let class = classify(h, near, budget);
                    *held = *now;
                    code(class, near.map_or(0.0, |(t, _)| h - t))
                } else {
                    *held = floor;
                    NO_CELL
                };
                if let Some(n) = class_index(*old).and_then(|k| gained.get_mut(k)) {
                    *n -= 1;
                }
                if let Some(n) = class_index(new).and_then(|k| gained.get_mut(k)) {
                    *n += 1;
                }
                *old = new;
            }
        }
        gained
    }
}

/// The material, ready to upload: a grid of heights and one code a cell.
#[derive(Debug, Clone)]
pub struct StockImage {
    /// Columns.
    pub nx: u32,
    /// Rows.
    pub ny: u32,
    /// Position of cell (0, 0).
    pub origin: DVec2,
    /// Cell size.
    pub pitch: f64,
    /// The bottom of the block.
    pub floor: f64,
    /// Height per cell, row by row.
    pub heights: Vec<f32>,
    /// Class and shade per cell, row by row: see [`code`].
    pub codes: Vec<u8>,
    /// How much sky each cell sees, row by row: see [`crate::stock::stock_ao`].
    pub ao: Vec<u8>,
    /// Cells per class, in [`CLASSES`] order.
    pub counts: [usize; 4],
}

impl StockImage {
    /// No material: what the playback image holds before it is first filled.
    pub fn empty() -> Self {
        Self {
            nx: 0,
            ny: 0,
            origin: DVec2::ZERO,
            pitch: 1.0,
            floor: 0.0,
            heights: Vec::new(),
            codes: Vec::new(),
            ao: Vec::new(),
            counts: [0; 4],
        }
    }

    /// Classify `stock` against `part`, with the job's gouge `budget`, and
    /// shade it; with what shaded it, to keep it shaded as it changes.
    pub fn build_shaded(stock: &Stock, part: &PartSurface, budget: f64) -> (Self, Occluder) {
        let field = stock.field();
        let (nx, ny) = (field.nx(), field.ny());
        let cells = nx as usize * ny as usize;
        let mut image = Self {
            nx,
            ny,
            origin: field.origin(),
            pitch: field.pitch(),
            floor: stock.floor(),
            heights: vec![0.0; cells],
            codes: vec![NO_CELL; cells],
            ao: vec![stock_ao::OPEN; cells],
            counts: [0; 4],
        };
        if nx > 0 && ny > 0 {
            use rayon::prelude::*;
            // Bands of rows, each its own slices of the image, in parallel.
            const BAND: u32 = 64;
            let n = nx as usize * BAND as usize;
            let gained = image
                .heights
                .par_chunks_mut(n)
                .zip(image.codes.par_chunks_mut(n))
                .enumerate()
                .map(|(b, (heights, codes))| {
                    let y0 = u32::try_from(b).unwrap_or(0) * BAND;
                    let rows = u32::try_from(heights.len() / nx as usize).unwrap_or(0);
                    let block = Block {
                        w: nx,
                        rect: CellRect {
                            x0: 0,
                            y0: 0,
                            x1: nx - 1,
                            y1: rows.saturating_sub(1),
                        },
                        at: (0, y0),
                        all: true,
                    };
                    block.classify(stock, part, budget, heights, codes)
                })
                .reduce(
                    || [0; 4],
                    |a, b| [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]],
                );
            for (count, g) in image.counts.iter_mut().zip(gained) {
                *count = count.wrapping_add_signed(g);
            }
            let all = CellRect::all(nx, ny);
            let occluder = Occluder::new(&image);
            image.ao = occluder.shade(&image, all);
            return (image, occluder);
        }
        let occluder = Occluder::new(&image);
        (image, occluder)
    }

    /// Classify the cells of `rect` whose height is not the one this image holds
    /// (a cell's class follows from its height), or all of them, for an image
    /// whose cell (0, 0) is the stock's cell `at`: the whole image, or a piece. Its counts change by what
    /// the cells gain and lose, wrapping, so a piece's hold the difference.
    fn classify_at(
        &mut self,
        stock: &Stock,
        part: &PartSurface,
        budget: f64,
        rect: CellRect,
        at: (u32, u32),
        all: bool,
    ) {
        let field = stock.field();
        let (nx, ny) = (self.nx, self.ny);
        if nx == 0 || ny == 0 || at.0 + nx > field.nx() || at.1 + ny > field.ny() {
            return;
        }
        let block = Block {
            w: nx,
            rect,
            at,
            all,
        };
        let gained = block.classify(stock, part, budget, &mut self.heights, &mut self.codes);
        for (count, g) in self.counts.iter_mut().zip(gained) {
            *count = count.wrapping_add_signed(g);
        }
    }

    /// Classify again the cells of each of `rects`, which are all that changed in
    /// `stock` since this image was made from it and must not overlap, in
    /// parallel, keeping the counts.
    pub fn patch_many(
        &mut self,
        stock: &Stock,
        part: &PartSurface,
        budget: f64,
        rects: &[CellRect],
    ) {
        use rayon::prelude::*;
        // Each rectangle worked out on its own image of it, from this one's
        // heights; then written back.
        let pieces: Vec<(CellRect, Self)> = rects
            .par_iter()
            .map(|&r| {
                let mut piece = self.piece(r);
                let local = CellRect::all(r.width(), r.height());
                piece.classify_at(stock, part, budget, local, (r.x0, r.y0), false);
                (r, piece)
            })
            .collect();
        for (r, piece) in pieces {
            self.put(r, &piece);
        }
    }

    /// A copy of the cells of `rect`, as an image of its own.
    fn piece(&self, rect: CellRect) -> Self {
        let (w, h) = (rect.width() as usize, rect.height() as usize);
        let mut heights = Vec::with_capacity(w * h);
        let mut codes = Vec::with_capacity(w * h);
        for iy in rect.y0..=rect.y1 {
            let row = iy as usize * self.nx as usize;
            let span = row + rect.x0 as usize..=row + rect.x1 as usize;
            if let (Some(hs), Some(cs)) = (self.heights.get(span.clone()), self.codes.get(span)) {
                heights.extend_from_slice(hs);
                codes.extend_from_slice(cs);
            }
        }
        Self {
            nx: rect.width(),
            ny: rect.height(),
            heights,
            codes,
            ao: Vec::new(),
            counts: [0; 4],
            ..*self
        }
    }

    /// Write a piece made by [`StockImage::piece`] back where it came from.
    fn put(&mut self, rect: CellRect, piece: &Self) {
        let w = rect.width() as usize;
        for ((iy, hs), cs) in (rect.y0..=rect.y1)
            .zip(piece.heights.chunks_exact(w))
            .zip(piece.codes.chunks_exact(w))
        {
            let row = iy as usize * self.nx as usize;
            let span = row + rect.x0 as usize..=row + rect.x1 as usize;
            if let Some(dst) = self.heights.get_mut(span.clone()) {
                dst.copy_from_slice(hs);
            }
            if let Some(dst) = self.codes.get_mut(span) {
                dst.copy_from_slice(cs);
            }
        }
        // A piece's counts are what it gained and lost, wrapped.
        for (count, g) in self.counts.iter_mut().zip(piece.counts) {
            *count = count.wrapping_add(g);
        }
    }

    /// Cells of `class`.
    #[cfg(test)]
    pub fn count(&self, class: Class) -> usize {
        count_of(&self.counts, class)
    }
}

/// Cells of `class`, of `counts` kept per class in [`CLASSES`] order.
pub fn count_of(counts: &[usize; 4], class: Class) -> usize {
    CLASSES
        .iter()
        .position(|c| *c == class)
        .and_then(|i| counts.get(i))
        .copied()
        .unwrap_or(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::float_cmp)]
mod tests;
