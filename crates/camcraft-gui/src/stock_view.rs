//! The simulated material as an image: one height and one colour per stock cell.
//!
//! Built on the CPU from the simulator's height field, and uploaded to the GPU as
//! two textures that a static grid is displaced by. So showing a different stage is
//! a texture write, and later, during playback, so is each step: only the cells the
//! tool touched.
//!
//! Every cell is classified by the verifier's own rule, with the verifier's own
//! lookup of the part and its own thresholds (the job's gouge budget and
//! [`LEFTOVER_LIMIT`]). What is red here is exactly what the verdict counted as
//! gouged, and what is amber is what it counted as left over.
//!
//! During playback the material is plain [`ColorMode::Material`] instead, so what
//! reads is the shape being cut, with a gouge still red the moment it happens.

use camcraft_core::run::LEFTOVER_LIMIT;
use camcraft_geom::heightmap::narrow;
use camcraft_sim::stock::{CellRect, PartSurface, Stock};
use glam::DVec2;

use crate::stock_ao::{self, Occluder};
use crate::theme;

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
pub fn classify(stock: f64, part: Option<(f64, f64)>, budget: f64) -> Class {
    let Some((part, under)) = part else {
        return Class::OffPart;
    };
    let delta = stock - part;
    if under > budget {
        Class::Gouge
    } else if delta > LEFTOVER_LIMIT {
        Class::Leftover
    } else {
        Class::OnPart
    }
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
    /// How much sky each cell sees, row by row: see [`crate::stock_ao`].
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
    /// shade it.
    pub fn build(stock: &Stock, part: &PartSurface, budget: f64) -> Self {
        Self::build_shaded(stock, part, budget).0
    }

    /// As [`StockImage::build`], with what shaded it, to keep it shaded as it
    /// changes.
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

    /// Classify again the cells of `rect`, which is all that changed in `stock`
    /// since this image was made from it, keeping the counts.
    #[cfg(test)]
    pub fn patch(&mut self, stock: &Stock, part: &PartSurface, budget: f64, rect: CellRect) {
        self.classify_at(stock, part, budget, rect, (0, 0), false);
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
    pub fn count(&self, class: Class) -> usize {
        CLASSES
            .iter()
            .position(|c| *c == class)
            .and_then(|i| self.counts.get(i))
            .copied()
            .unwrap_or(0)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::test_jobs::run_setup_zero;
    use camcraft_core::job::Job;

    #[test]
    fn thresholds_sit_where_the_verifier_puts_them() {
        let budget = 0.45;
        assert_eq!(classify(10.0, None, budget), Class::OffPart);
        assert_eq!(
            classify(10.0 - 0.45, Some((10.0, 0.45)), budget),
            Class::OnPart
        );
        assert_eq!(
            classify(10.0 - 0.46, Some((10.0, 0.46)), budget),
            Class::Gouge
        );
        assert_eq!(
            classify(10.0 + LEFTOVER_LIMIT, Some((10.0, -LEFTOVER_LIMIT)), budget),
            Class::OnPart
        );
        assert_eq!(
            classify(
                10.0 + LEFTOVER_LIMIT + 0.01,
                Some((10.0, -LEFTOVER_LIMIT - 0.01)),
                budget
            ),
            Class::Leftover
        );
    }

    /// Beside a wall, what counts is how far below the lowest nearby part the
    /// material is, not how far below the column's own height.
    #[test]
    fn beside_a_wall_the_floor_decides() {
        assert_eq!(classify(0.0, Some((10.0, 0.0)), 0.45), Class::OnPart);
        assert_eq!(classify(-0.5, Some((10.0, 0.5)), 0.45), Class::Gouge);
    }

    #[test]
    fn leftover_deepens_with_what_is_left() {
        let faint = cell_color(ColorMode::Compare, Class::Leftover, LEFTOVER_LIMIT + 0.01);
        let strong = cell_color(ColorMode::Compare, Class::Leftover, 50.0);
        assert_eq!(
            strong,
            theme::AMBER_5,
            "saturates at the design system's amber"
        );
        let brightness = |c: [u8; 3]| c.iter().map(|v| u32::from(*v)).sum::<u32>();
        assert!(brightness(faint) < brightness(strong));
    }

    /// The picture agrees with the verdict: on a real run, the cells drawn red and
    /// amber are exactly the columns the verifier counted as gouged and left over.
    #[test]
    fn the_picture_counts_what_the_verdict_counts() {
        let job: Job = toml::from_str(
            r#"
version = 1
tolerance = 0.1
[tools.flat]
kind = "flat"
diameter = 6.0
[[model]]
name = "hinge"
path = "fixtures/step/hinge_jig.step"
[[stock]]
name = "block"
kind = "around-models"
margin = [2.0, 2.0, 2.0]
[[setup]]
name = "top"
models = ["hinge"]
stocks = ["block"]
[[setup.operation]]
name = "rough"
strategy = "zlevel-rough"
tool = "flat"
stepdown = 4.0
passes = { stepover = 3.0, feed = 1800.0, stock_to_leave = { radial = 0.3, axial = 0.3 } }
"#,
        )
        .unwrap();
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let result = run_setup_zero(&job, &repo).result;

        let finished = result.keyframes.last().unwrap().unpack().unwrap();
        let image = StockImage::build(&finished, &result.part, result.verify_budget);
        assert_eq!(image.count(Class::Gouge) as u64, result.verify.gouged);
        assert_eq!(image.count(Class::Leftover) as u64, result.verify.leftover);
        assert!(
            result.verify.leftover > 0,
            "a roughing pass that leaves stock should leave some over the limit"
        );
        let compared =
            image.count(Class::Gouge) + image.count(Class::Leftover) + image.count(Class::OnPart);
        assert_eq!(compared as u64, result.verify.compared);
        assert_eq!(image.heights.len(), (image.nx * image.ny) as usize);
        assert_eq!(image.codes.len(), image.heights.len());
    }

    /// A code keeps its class, and a leftover's shade, and nothing else collides
    /// with the code for no cell.
    #[test]
    fn a_code_is_a_class_and_a_shade() {
        for class in CLASSES {
            for delta in [0.0, 0.5, 2.0, 50.0] {
                let c = code(class, delta);
                assert_eq!(class_of(c), Some(class), "{class:?} {delta}");
                assert_ne!(c, NO_CELL);
            }
        }
        assert!(code(Class::Leftover, 50.0) > code(Class::Leftover, LEFTOVER_LIMIT + 0.1));
        assert_eq!(class_of(NO_CELL), None);
    }

    #[test]
    fn material_is_one_colour_but_a_gouge_is_red() {
        for class in CLASSES {
            let want = if class == Class::Gouge {
                theme::ERR
            } else {
                theme::MATERIAL
            };
            assert_eq!(cell_color(ColorMode::Material, class, 3.0), want);
        }
    }

    /// Patching only the cells a sweep changed gives the image built afresh: the
    /// same heights, codes and counts.
    #[test]
    fn a_patch_is_a_rebuild_of_what_changed() {
        use camcraft_geom::synthetic;
        use camcraft_sim::stock::{MoveClass, SimMove, SweptTool};
        use glam::{dvec2, dvec3};

        struct Flat(f64);
        impl SweptTool for Flat {
            fn max_radius(&self) -> f64 {
                self.0
            }
            fn height_at(&self, d: f64) -> Option<f64> {
                (d <= self.0).then_some(0.0)
            }
        }
        let (lo, hi) = (dvec2(-10.0, -10.0), dvec2(10.0, 10.0));
        let part = PartSurface::from_mesh(&synthetic::plane(0.0, 12.0), lo, hi, 0.5).unwrap();
        {
            let mut stock = Stock::block(lo, hi, 5.0, -5.0, 0.5).unwrap();
            let mut image = StockImage::build(&stock, &part, 0.1);
            for (a, b, z) in [
                ((-8.0, -2.0), (8.0, -2.0), 1.0),
                ((0.0, -8.0), (0.0, 3.0), -1.0),
            ] {
                let moves = [
                    SimMove {
                        to: dvec3(a.0, a.1, z),
                        class: MoveClass::MayCut,
                    },
                    SimMove {
                        to: dvec3(b.0, b.1, z),
                        class: MoveClass::MayCut,
                    },
                ];
                let rect = stock.sweep(&moves, &Flat(1.5)).touched.unwrap();
                // Once as one rectangle, once as tiles in parallel.
                let mut tiled = image.clone();
                image.patch(&stock, &part, 0.1, rect);
                let mut tiles = crate::tiles::Tiles::new(image.nx, image.ny);
                tiles.mark(rect);
                tiled.patch_many(&stock, &part, 0.1, &tiles.rects());
                let fresh = StockImage::build(&stock, &part, 0.1);
                for got in [&image, &tiled] {
                    assert_eq!(got.heights, fresh.heights);
                    assert_eq!(got.codes, fresh.codes);
                    assert_eq!(got.counts, fresh.counts);
                }
            }
            assert!(
                image.count(Class::Gouge) > 0,
                "the second cut goes under the part"
            );
        }
    }
}
