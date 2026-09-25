//! Ambient occlusion for the material, worked out on its heights.
//!
//! The material is a height per cell with solid below, so how much of the sky a
//! cell sees can be found on the grid itself rather than on the screen: from each
//! cell, look out along a few directions for the highest horizon, as GTAO does
//! with a depth buffer, and turn the horizons into the share of cosine-weighted
//! sky left open. Found this way it belongs to the material, not the view, so it
//! does not change as the view turns, has no thickness to guess (everything under
//! a height is solid), and costs nothing to draw. It is worked out when cells
//! change, over the cells within [`Occluder::reach`] of them.
//!
//! Looking far is cheap: the heights are kept as a pyramid of maxima, and a look
//! out at a distance reads the highest cell of a square about as wide as the gap
//! to the next look, so a thin wall between two looks is still seen.

use camcraft_sim::stock::CellRect;
use rayon::prelude::*;

use crate::stock::pyramid::{Level, Pyramid};
use crate::stock::stock_view::StockImage;
use crate::view::viewport::WALL_CELLS;

/// Directions looked along from each cell, each both ways.
const SLICES: usize = 8;

/// Each look is this much further out than the last, and at least a cell.
const STEP: f32 = 1.35;

/// How far a cell looks for what shades it: this share of the stock's longer
/// side, within [`REACH_MM`]. A pocket on a brick and a pocket on a sheet are both
/// shaded, though they are two orders apart.
const REACH_SHARE: f64 = 0.04;
const REACH_MM: (f64, f64) = (2.0, 40.0);

/// The most cells a look reaches, however fine the pitch.
const MAX_REACH: f64 = 256.0;

/// The last share of the reach, over which what is found counts less and less,
/// so the edge of the reach does not show as a line.
const FALLOFF: f32 = 0.6;

/// Fully open sky, as stored.
pub const OPEN: u8 = 255;

/// Samples across the reach: the occlusion is worked out on a grid this much
/// coarser than the cells, where it is that fine, since it changes slowly over
/// the reach. Each cell then takes it from the samples round it.
const SAMPLES_ACROSS_REACH: u32 = 24;

/// The heights, kept for looking far cheaply; how far to look; and the
/// occlusion on its coarse grid.
pub struct Occluder {
    pyramid: Pyramid,
    reach: u32,
    /// Distances looked out to, in cells, and the pyramid level each reads.
    looks: Vec<(f32, usize)>,
    /// Cells a sample of the coarse grid stands for, each way.
    stride: u32,
    /// The coarse grid's size, and its samples, row by row.
    cw: u32,
    ch: u32,
    samples: Vec<u8>,
}

impl Occluder {
    /// For `image`, as it stands, with every sample worked out.
    pub fn new(image: &StockImage) -> Self {
        let (w, h) = (image.nx.next_power_of_two(), image.ny.next_power_of_two());
        let reach = reach(image);
        let stride = reach.div_ceil(SAMPLES_ACROSS_REACH).max(1);
        let (cw, ch) = (image.nx.div_ceil(stride), image.ny.div_ceil(stride));
        let mut occluder = Self {
            pyramid: Pyramid::new(w, h, base(image)),
            reach,
            looks: looks(reach),
            stride,
            cw,
            ch,
            samples: vec![OPEN; cw as usize * ch as usize],
        };
        if cw > 0 && ch > 0 {
            occluder.resample(
                image,
                CellRect {
                    x0: 0,
                    y0: 0,
                    x1: cw - 1,
                    y1: ch - 1,
                },
            );
        }
        occluder
    }

    /// How far, in cells, a change in height changes the occlusion around it.
    pub fn reach(&self) -> u32 {
        self.reach
    }

    /// The heights of `rects` of `image` changed.
    pub fn update(&mut self, image: &StockImage, rects: &[CellRect]) {
        for r in rects {
            self.pyramid.update(*r, base(image));
        }
    }

    /// The occlusion of the cells of `rect` of `image`, row by row, from the
    /// samples as they stand: [`OPEN`] where all the sky is seen.
    pub fn shade(&self, image: &StockImage, rect: CellRect) -> Vec<u8> {
        let w = rect.width() as usize;
        let mut out = vec![OPEN; w * rect.height() as usize];
        if w == 0 {
            return out;
        }
        out.par_chunks_mut(w)
            .zip(rect.y0..rect.y1 + 1)
            .for_each(|(row, y)| {
                for (v, x) in row.iter_mut().zip(rect.x0..=rect.x1) {
                    *v = self.between(image, x, y);
                }
            });
        out
    }

    /// Work the samples over `rects` of `image` out again, and shade their
    /// cells in place. The rectangles must not overlap, and must hold every cell
    /// whose occlusion the change of height reaches.
    pub fn shade_into(&mut self, image: &mut StockImage, rects: &[CellRect]) {
        let s = self.stride;
        for r in rects {
            // The samples the cells take from: those over them, and one more
            // each way.
            let coarse = CellRect {
                x0: (r.x0 / s).saturating_sub(1),
                y0: (r.y0 / s).saturating_sub(1),
                x1: (r.x1 / s + 1).min(self.cw.saturating_sub(1)),
                y1: (r.y1 / s + 1).min(self.ch.saturating_sub(1)),
            };
            self.resample(image, coarse);
        }
        let shaded: Vec<(CellRect, Vec<u8>)> =
            rects.iter().map(|&r| (r, self.shade(image, r))).collect();
        let nx = image.nx as usize;
        for (r, values) in shaded {
            let w = r.width() as usize;
            for (y, row) in (r.y0..=r.y1).zip(values.chunks_exact(w)) {
                let at = y as usize * nx + r.x0 as usize;
                if let Some(dst) = image.ao.get_mut(at..at + w) {
                    dst.copy_from_slice(row);
                }
            }
        }
    }

    /// Work out the samples of `coarse` (the coarse grid's cells) again.
    fn resample(&mut self, image: &StockImage, coarse: CellRect) {
        let w = coarse.width() as usize;
        let mut fresh = vec![OPEN; w * coarse.height() as usize];
        fresh
            .par_chunks_mut(w)
            .zip(coarse.y0..coarse.y1 + 1)
            .for_each(|(row, j)| {
                for (v, i) in row.iter_mut().zip(coarse.x0..=coarse.x1) {
                    let (x, y) = self.centre(image, i, j);
                    *v = self.cell(image, x, y);
                }
            });
        let cw = self.cw as usize;
        for (j, row) in (coarse.y0..=coarse.y1).zip(fresh.chunks_exact(w)) {
            let at = j as usize * cw + coarse.x0 as usize;
            if let Some(dst) = self.samples.get_mut(at..at + w) {
                dst.copy_from_slice(row);
            }
        }
    }

    /// The cell a sample of the coarse grid is worked out at: the middle of the
    /// cells it stands for.
    fn centre(&self, image: &StockImage, i: u32, j: u32) -> (u32, u32) {
        let s = self.stride;
        (
            (i * s + s / 2).min(image.nx.saturating_sub(1)),
            (j * s + s / 2).min(image.ny.saturating_sub(1)),
        )
    }

    /// Cell `(x, y)`'s occlusion, between the four samples round it; worked
    /// out for the cell alone when any of them is across a wall from it. So
    /// every cell within a sample of a wall is worked out alone: the shade is
    /// sharpest there, and a cell in a notch of the staircase a round wall
    /// makes would otherwise take it from samples further off than its
    /// neighbours do, and stripe the wall's foot.
    fn between(&self, image: &StockImage, x: u32, y: u32) -> u8 {
        let s = self.stride;
        if s == 1 {
            return self
                .samples
                .get(y as usize * self.cw as usize + x as usize)
                .copied()
                .unwrap_or(OPEN);
        }
        let own = height(image, i64::from(x), i64::from(y));
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        let wall = WALL_CELLS * image.pitch as f32 * s as f32;
        // Where the cell's middle is on the coarse grid, whose samples sit at
        // the middles of theirs.
        #[allow(clippy::cast_precision_loss)]
        let (u, v) = (
            (x as f32 + 0.5) / s as f32 - 0.5,
            (y as f32 + 0.5) / s as f32 - 0.5,
        );
        let (fu, fv) = (u.floor(), v.floor());
        let (tu, tv) = (u - fu, v - fv);
        let (mut sum, mut weight) = (0.0_f32, 0.0_f32);
        for (dj, wv) in [(0_i64, 1.0 - tv), (1, tv)] {
            for (di, wu) in [(0_i64, 1.0 - tu), (1, tu)] {
                #[allow(clippy::cast_possible_truncation)]
                let (i, j) = (fu as i64 + di, fv as i64 + dj);
                let (Ok(i), Ok(j)) = (u32::try_from(i), u32::try_from(j)) else {
                    continue;
                };
                if i >= self.cw || j >= self.ch {
                    continue;
                }
                let (cx, cy) = self.centre(image, i, j);
                if (height(image, i64::from(cx), i64::from(cy)) - own).abs() >= wall {
                    return self.cell(image, x, y);
                }
                let Some(&sample) = self.samples.get(j as usize * self.cw as usize + i as usize)
                else {
                    continue;
                };
                let w = wu * wv;
                sum += w * f32::from(sample);
                weight += w;
            }
        }
        if weight < 1e-3 {
            return self.cell(image, x, y);
        }
        // Within 0..=255, as a weighted mean of such.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let mean = (sum / weight).round() as u8;
        mean
    }

    /// The highest material in the square of pyramid `level` over cell `(x, y)`,
    /// the floor past the stock.
    fn highest(&self, image: &StockImage, level: usize, x: i64, y: i64) -> f32 {
        let floor = floor(image);
        if x < 0 || y < 0 {
            return floor;
        }
        let (cx, cy) = (x >> level, y >> level);
        if level == 0 {
            return height(image, cx, cy);
        }
        let Some(l) = self.pyramid.levels.get(level - 1) else {
            return floor;
        };
        at(l, cx, cy).unwrap_or(floor)
    }

    /// How open cell `(x, y)` is to the sky, as GTAO works it out: for each
    /// slice, the horizons either way, held to the hemisphere over the surface's
    /// normal, and the cosine-weighted arc of sky between them.
    fn cell(&self, image: &StockImage, x: u32, y: u32) -> u8 {
        let (xi, yi) = (i64::from(x), i64::from(y));
        let own = height(image, xi, yi);
        let normal = smooth_normal(image, xi, yi, own);
        #[allow(clippy::cast_possible_truncation)]
        let pitch = image.pitch as f32;
        #[allow(clippy::cast_precision_loss)]
        let reach = self.reach as f32;
        let fade_from = reach * (1.0 - FALLOFF);
        // Cells across a stock are far fewer than an f32 counts exactly.
        #[allow(clippy::cast_precision_loss)]
        let centre = (x as f32 + 0.5, y as f32 + 0.5);
        let mut open = 0.0;
        for s in 0..SLICES {
            #[allow(clippy::cast_precision_loss)]
            let phi = s as f32 * std::f32::consts::PI / SLICES as f32;
            let d = (phi.cos(), phi.sin());
            // The normal seen in the slice: its length there, and its angle from
            // straight up, positive leaning along `d`.
            let across = [-d.1, d.0, 0.0];
            let off = dot(normal, across);
            let proj = [
                normal[0] - across[0] * off,
                normal[1] - across[1] * off,
                normal[2] - across[2] * off,
            ];
            let len = dot(proj, proj).sqrt();
            if len < 1e-6 {
                continue;
            }
            let cos_n = (proj[2] / len).clamp(-1.0, 1.0);
            let n = (proj[0] * d.0 + proj[1] * d.1).signum() * cos_n.acos();
            let half = std::f32::consts::FRAC_PI_2;

            // The highest horizon along `sign * d`, as a cosine from straight
            // up, starting from the lowest the hemisphere over the normal allows.
            let horizon = |sign: f32, low: f32| {
                let mut best = low;
                for &(dist, level) in &self.looks {
                    let px = centre.0 + sign * d.0 * dist;
                    let py = centre.1 + sign * d.1 * dist;
                    #[allow(clippy::cast_possible_truncation)]
                    let h = self.highest(image, level, px.floor() as i64, py.floor() as i64);
                    let dz = h - own;
                    let run = dist * pitch;
                    let c = dz / (run * run + dz * dz).sqrt();
                    let weight = if dist <= fade_from {
                        1.0
                    } else {
                        ((reach - dist) / (reach - fade_from)).clamp(0.0, 1.0)
                    };
                    best = best.max(low + (c - low) * weight);
                }
                best.clamp(-1.0, 1.0).acos()
            };
            let h1 = n + (horizon(1.0, (n + half).cos()) - n).min(half);
            let h0 = n + (-horizon(-1.0, (n - half).cos()) - n).max(-half);
            let arc = |h: f32| (cos_n + 2.0 * h * n.sin() - (2.0 * h - n).cos()) / 4.0;
            open += len * (arc(h0) + arc(h1));
        }
        #[allow(clippy::cast_precision_loss)]
        let open = (open / SLICES as f32).clamp(0.0, 1.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let stored = (open * f32::from(OPEN)).round() as u8;
        stored
    }
}

/// How far, in millimetres, a point looks for what shades it, on a stock or
/// part whose longer side is `side`: the part's shade reaches as far as the
/// material's, so the two read alike.
pub fn reach_mm(side: f64) -> f64 {
    (side * REACH_SHARE).clamp(REACH_MM.0, REACH_MM.1)
}

/// How far, in cells, a cell of `image` looks for what shades it.
pub fn reach(image: &StockImage) -> u32 {
    let side = f64::from(image.nx.max(image.ny)) * image.pitch;
    let mm = reach_mm(side);
    // A small positive whole number of cells, by the clamp.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let cells = (mm / image.pitch.max(1e-9)).clamp(2.0, MAX_REACH).ceil() as u32;
    cells
}

/// `r` grown by `by` cells each way, held to a stock `nx` by `ny`.
pub fn grow(r: CellRect, by: u32, nx: u32, ny: u32) -> CellRect {
    CellRect {
        x0: r.x0.saturating_sub(by),
        y0: r.y0.saturating_sub(by),
        x1: r.x1.saturating_add(by).min(nx.saturating_sub(1)),
        y1: r.y1.saturating_add(by).min(ny.saturating_sub(1)),
    }
}

/// The distances looked out to, out to `reach` cells, each with the pyramid
/// level whose squares are about half the gap to the next look: wide enough to
/// catch a thin wall between looks, narrow enough not to reach back to the cell.
fn looks(reach: u32) -> Vec<(f32, usize)> {
    #[allow(clippy::cast_precision_loss)]
    let reach = reach as f32;
    let mut out = Vec::new();
    let mut d = 1.0_f32;
    while d <= reach {
        let next = (d * STEP).max(d + 1.0);
        let square = ((next - d) * 0.5).max(1.0);
        // A small whole number: the log of at most a few hundred.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let level = square.log2().floor() as usize;
        out.push((d, level));
        d = next;
    }
    out
}

fn floor(image: &StockImage) -> f32 {
    #[allow(clippy::cast_possible_truncation)]
    let f = image.floor as f32;
    f
}

/// A cell's height; the floor past the stock.
fn height(image: &StockImage, x: i64, y: i64) -> f32 {
    let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
        return floor(image);
    };
    if x >= image.nx || y >= image.ny {
        return floor(image);
    }
    image
        .heights
        .get(y as usize * image.nx as usize + x as usize)
        .copied()
        .unwrap_or_else(|| floor(image))
}

/// Level 0 of the pyramid: the heights, and the floor past the stock.
fn base(image: &StockImage) -> impl Fn(u32, u32) -> f32 + '_ {
    move |x, y| height(image, i64::from(x), i64::from(y))
}

fn at(level: &Level, x: i64, y: i64) -> Option<f32> {
    let (x, y) = (u32::try_from(x).ok()?, u32::try_from(y).ok()?);
    if x >= level.w || y >= level.h {
        return None;
    }
    level
        .data
        .get(y as usize * level.w as usize + x as usize)
        .copied()
}

/// The surface's slope at a cell, as the stock shader finds it: from the
/// neighbours that are not across a wall.
fn smooth_normal(image: &StockImage, x: i64, y: i64, own: f32) -> [f32; 3] {
    #[allow(clippy::cast_possible_truncation)]
    let pitch = image.pitch as f32;
    let wall = WALL_CELLS * pitch;
    let joined = |x: i64, y: i64| {
        let h = height(image, x, y);
        if (h - own).abs() < wall { h } else { own }
    };
    let dx = (joined(x + 1, y) - joined(x - 1, y)) / (2.0 * pitch);
    let dy = (joined(x, y + 1) - joined(x, y - 1)) / (2.0 * pitch);
    let len = (dx * dx + dy * dy + 1.0).sqrt();
    [-dx / len, -dy / len, 1.0 / len]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)]
mod tests {
    use super::*;
    use glam::DVec2;

    /// A block `n` cells square at `pitch`, all at height `top`, with `cut` taken
    /// down to `depth`.
    fn block(n: u32, pitch: f64, top: f32, cut: Option<(CellRect, f32)>) -> StockImage {
        let mut heights = vec![top; (n * n) as usize];
        if let Some((r, depth)) = cut {
            for y in r.y0..=r.y1 {
                for x in r.x0..=r.x1 {
                    heights[(y * n + x) as usize] = depth;
                }
            }
        }
        let mut image = StockImage::empty();
        image.nx = n;
        image.ny = n;
        image.origin = DVec2::ZERO;
        image.pitch = pitch;
        image.floor = -20.0;
        image.heights = heights;
        image.codes = vec![0; (n * n) as usize];
        image.ao = vec![OPEN; (n * n) as usize];
        image
    }

    fn all(n: u32) -> CellRect {
        CellRect {
            x0: 0,
            y0: 0,
            x1: n - 1,
            y1: n - 1,
        }
    }

    /// A flat top sees all the sky.
    #[test]
    fn a_flat_top_is_open() {
        let image = block(64, 0.5, 0.0, None);
        let ao = Occluder::new(&image).shade(&image, all(64));
        assert!(ao.iter().all(|&v| v >= OPEN - 1), "{:?}", &ao[..8]);
    }

    /// A pocket's floor is darker at its walls than at its middle, and darkest
    /// in its corners; the top around it stays open.
    #[test]
    fn a_pocket_darkens_its_floor_at_the_walls() {
        let n = 96;
        let pocket = CellRect {
            x0: 24,
            y0: 24,
            x1: 71,
            y1: 71,
        };
        let image = block(n, 0.25, 0.0, Some((pocket, -3.0)));
        let ao = Occluder::new(&image).shade(&image, all(n));
        let at = |x: u32, y: u32| ao[(y * n + x) as usize];
        let (middle, wall, corner) = (at(48, 48), at(24, 48), at(24, 24));
        assert!(wall < middle, "wall {wall} middle {middle}");
        assert!(corner < wall, "corner {corner} wall {wall}");
        assert!(
            at(4, 4) >= OPEN - 1,
            "the top far off is open: {}",
            at(4, 4)
        );
    }

    /// At a fine pitch the occlusion comes from a coarser grid, and each cell's
    /// is close to what working it out for that cell alone gives, walls and all.
    #[test]
    fn the_coarse_grid_gives_nearly_each_cells_own() {
        let n = 240;
        let pocket = CellRect {
            x0: 60,
            y0: 70,
            x1: 180,
            y1: 150,
        };
        // 0.02 mm cells: a 2 mm reach is 100 of them.
        let image = block(n, 0.02, 0.0, Some((pocket, -1.0)));
        let occluder = Occluder::new(&image);
        assert!(occluder.stride > 1, "stride {}", occluder.stride);
        let ao = occluder.shade(&image, all(n));
        let (mut worst, mut sum, mut count) = (0_i32, 0_i64, 0_i64);
        for y in (0..n).step_by(3) {
            for x in (0..n).step_by(3) {
                let own = i32::from(occluder.cell(&image, x, y));
                let got = i32::from(ao[(y * n + x) as usize]);
                worst = worst.max((own - got).abs());
                sum += i64::from((own - got).abs());
                count += 1;
            }
        }
        let mean = sum as f64 / count as f64;
        assert!(mean < 3.0, "mean error {mean:.2} of 255");
        assert!(worst < 40, "worst error {worst} of 255");
    }

    /// Shading a part of the image gives what shading all of it gives there.
    #[test]
    fn a_part_shades_as_the_whole_does() {
        let n = 80;
        let cut = CellRect {
            x0: 30,
            y0: 10,
            x1: 50,
            y1: 60,
        };
        let image = block(n, 0.5, 0.0, Some((cut, -4.0)));
        let occluder = Occluder::new(&image);
        let whole = occluder.shade(&image, all(n));
        let part = CellRect {
            x0: 20,
            y0: 5,
            x1: 45,
            y1: 30,
        };
        let piece = occluder.shade(&image, part);
        for (i, y) in (part.y0..=part.y1).enumerate() {
            for (j, x) in (part.x0..=part.x1).enumerate() {
                let w = part.width() as usize;
                assert_eq!(piece[i * w + j], whole[(y * n + x) as usize]);
            }
        }
    }
}
