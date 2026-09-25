//! The material's heights as a pyramid of maxima, which the stock shader's rays
//! walk down.
//!
//! Level 0 is the heights themselves, over a grid a power of two each way (what
//! lies past the material reads as its floor), so every level is exactly half the
//! one below, down to one cell, and a level's cell covers exactly its children.
//! Each level above holds the highest of the four cells under it. A ray that
//! passes over a cell of some level above its height passes over everything it
//! covers, so it skips that whole square in one step: the cost of drawing the
//! material grows with the number of levels, not the number of cells.
//!
//! These are the height texture's mip levels. Level 0 is not kept here: it is read
//! from the image it comes from, so a sheet's heights are not held twice. A
//! playback step changes a rectangle of level 0; [`Pyramid::update`] redoes only
//! the cells above it and says which, so only those are uploaded.

use camcraft_sim::stock::CellRect;

/// One level: its size and its heights, row by row.
#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    /// Cells across.
    pub w: u32,
    /// Cells down.
    pub h: u32,
    /// Heights, row by row.
    pub data: Vec<f32>,
}

impl Level {
    fn get(&self, x: u32, y: u32) -> f32 {
        if x >= self.w || y >= self.h {
            return f32::NEG_INFINITY;
        }
        self.data
            .get(y as usize * self.w as usize + x as usize)
            .copied()
            .unwrap_or(f32::NEG_INFINITY)
    }

    /// The whole level, as a rectangle.
    pub fn all(&self) -> CellRect {
        CellRect::all(self.w, self.h)
    }
}

/// Every level above level 0, finest first.
#[derive(Debug, Clone, PartialEq)]
pub struct Pyramid {
    /// Level 0's size: powers of two.
    pub w: u32,
    /// Level 0's size: powers of two.
    pub h: u32,
    /// Levels 1 and up.
    pub levels: Vec<Level>,
    /// Level 0's own value, when it is a single cell.
    only: f32,
}

impl Pyramid {
    /// The pyramid over a level 0 `w` by `h` (each a power of two) whose cell
    /// `(x, y)` is `base(x, y)`.
    pub fn new(w: u32, h: u32, base: impl Fn(u32, u32) -> f32) -> Self {
        let (w, h) = (w.max(1), h.max(1));
        let mut levels: Vec<Level> = Vec::new();
        let (mut bw, mut bh) = (w, h);
        while bw > 1 || bh > 1 {
            let (lw, lh) = ((bw / 2).max(1), (bh / 2).max(1));
            let mut level = Level {
                w: lw,
                h: lh,
                data: vec![0.0; lw as usize * lh as usize],
            };
            let all = level.all();
            match levels.last() {
                Some(below) => fill(&mut level, |x, y| below.get(x, y), all),
                None => fill(&mut level, |x, y| edge(&base, w, h, x, y), all),
            }
            levels.push(level);
            (bw, bh) = (lw, lh);
        }
        Self {
            w,
            h,
            levels,
            only: base(0, 0),
        }
    }

    /// Level 0 changed in `rect`: redo every level above it. Returns the
    /// rectangle changed on each level from 1 up.
    pub fn update(&mut self, rect: CellRect, base: impl Fn(u32, u32) -> f32) -> Vec<CellRect> {
        let (w, h) = (self.w, self.h);
        let mut r = CellRect {
            x0: rect.x0.min(w - 1),
            y0: rect.y0.min(h - 1),
            x1: rect.x1.min(w - 1),
            y1: rect.y1.min(h - 1),
        };
        self.only = base(0, 0);
        let mut changed = Vec::with_capacity(self.levels.len());
        for i in 0..self.levels.len() {
            let (lower, upper) = self.levels.split_at_mut(i);
            let Some(level) = upper.first_mut() else {
                break;
            };
            r = CellRect {
                x0: (r.x0 / 2).min(level.w - 1),
                y0: (r.y0 / 2).min(level.h - 1),
                x1: (r.x1 / 2).min(level.w - 1),
                y1: (r.y1 / 2).min(level.h - 1),
            };
            match lower.last() {
                Some(below) => fill(level, |x, y| below.get(x, y), r),
                None => fill(level, |x, y| edge(&base, w, h, x, y), r),
            }
            changed.push(r);
        }
        changed
    }

    /// The highest material anywhere.
    pub fn top(&self) -> f32 {
        self.levels
            .last()
            .and_then(|l| l.data.first())
            .copied()
            .unwrap_or(self.only)
    }
}

/// Level 0 at `(x, y)`, or nothing past its edge.
fn edge(base: &impl Fn(u32, u32) -> f32, w: u32, h: u32, x: u32, y: u32) -> f32 {
    if x < w && y < h {
        base(x, y)
    } else {
        f32::NEG_INFINITY
    }
}

/// Set `rect` of `level` to the highest of the cells under each, in `below`.
fn fill(level: &mut Level, below: impl Fn(u32, u32) -> f32, rect: CellRect) {
    let w = level.w as usize;
    for y in rect.y0..=rect.y1 {
        for x in rect.x0..=rect.x1 {
            let v = below(2 * x, 2 * y)
                .max(below(2 * x + 1, 2 * y))
                .max(below(2 * x, 2 * y + 1))
                .max(below(2 * x + 1, 2 * y + 1));
            if let Some(slot) = level.data.get_mut(y as usize * w + x as usize) {
                *slot = v;
            }
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)]
mod tests {
    use super::*;

    fn heights(nx: u32, ny: u32, seed: u32) -> Vec<f32> {
        (0..nx * ny)
            .map(|i| {
                let v = ((i.wrapping_mul(2_654_435_761).wrapping_add(seed)) % 1000) as f32;
                v / 10.0
            })
            .collect()
    }

    /// Level 0 of `h`, `nx` by `ny`, the floor past it.
    fn base(h: &[f32], nx: u32, ny: u32, floor: f32) -> impl Fn(u32, u32) -> f32 + '_ {
        move |x, y| {
            if x < nx && y < ny {
                h[(y * nx + x) as usize]
            } else {
                floor
            }
        }
    }

    /// Every cell of every level is the highest of the heights it covers, and the
    /// padding is the floor.
    #[test]
    fn each_level_holds_the_highest_under_it() {
        let (nx, ny, floor) = (37, 11, -3.0);
        let h = heights(nx, ny, 7);
        let (w, hh) = (nx.next_power_of_two(), ny.next_power_of_two());
        let p = Pyramid::new(w, hh, base(&h, nx, ny, floor));
        assert_eq!((p.w, p.h), (64, 16));
        assert_eq!(p.levels.last().map(|l| (l.w, l.h)), Some((1, 1)));
        for (i, level) in p.levels.iter().enumerate() {
            let s = 2u32 << i;
            for y in 0..level.h {
                for x in 0..level.w {
                    let mut want = f32::NEG_INFINITY;
                    // A level clamped to one cell covers all the rest.
                    let (xs, ys) = (
                        if level.w == 1 {
                            0..w
                        } else {
                            x * s..(x + 1) * s
                        },
                        if level.h == 1 {
                            0..hh
                        } else {
                            y * s..(y + 1) * s
                        },
                    );
                    for by in ys {
                        for bx in xs.clone() {
                            want = want.max(base(&h, nx, ny, floor)(bx, by));
                        }
                    }
                    assert_eq!(level.get(x, y), want, "level {} ({x}, {y})", i + 1);
                }
            }
        }
        assert_eq!(p.top(), h.iter().copied().fold(floor, f32::max));
        // One cell is its own top.
        assert_eq!(Pyramid::new(1, 1, |_, _| 4.0).top(), 4.0);
    }

    /// Updating a rectangle gives the pyramid built afresh, and says which cells of
    /// each level it touched.
    #[test]
    fn an_update_is_a_rebuild_of_what_changed() {
        let (nx, ny) = (50, 30);
        let mut h = heights(nx, ny, 1);
        let (w, hh) = (64, 32);
        let mut p = Pyramid::new(w, hh, base(&h, nx, ny, 0.0));
        let rect = CellRect {
            x0: 13,
            y0: 4,
            x1: 21,
            y1: 9,
        };
        for y in rect.y0..=rect.y1 {
            for x in rect.x0..=rect.x1 {
                h[(y * nx + x) as usize] = -1.0 - (x + y) as f32;
            }
        }
        let changed = p.update(rect, base(&h, nx, ny, 0.0));
        assert_eq!(p, Pyramid::new(w, hh, base(&h, nx, ny, 0.0)));
        assert_eq!(changed.len(), p.levels.len());
        assert_eq!(
            changed[0],
            CellRect {
                x0: 6,
                y0: 2,
                x1: 10,
                y1: 4
            }
        );
    }
}
