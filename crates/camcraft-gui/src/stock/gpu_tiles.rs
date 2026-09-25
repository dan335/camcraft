//! The material cut into tiles for the GPU.
//!
//! A GPU holds textures only so large (eframe asks for 8192 across), and a full
//! sheet at a fine pitch is far larger. So the stock is drawn as tiles, each its
//! own box traced by the stock shader, with its own textures and height pyramid.
//! Each tile's textures also hold an apron of its neighbours' cells, [`APRON`]
//! wide, so shading at a tile's edge sees the cells past it and the seams do not
//! show. With the apron a tile's texture is exactly [`TEXTURE`] across, a power of
//! two as its pyramid needs.

use camcraft_sim::stock::CellRect;

/// Cells of neighbouring tiles each tile's textures hold around its own, as far
/// as the stock shader looks to light a cell.
pub const APRON: u32 = 4;

/// A tile's texture size, apron and all.
pub const TEXTURE: u32 = 2048;

/// A tile's own cells across, at most.
pub const CORE: u32 = TEXTURE - 2 * APRON;

/// One tile: its own cells, and the cells its textures hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tile {
    /// Its own cells, in the stock's grid.
    pub core: CellRect,
    /// The stock cell its textures' cell (0, 0) stands for: its core's first
    /// cell less the apron, so possibly before the stock begins.
    pub x0: i64,
    /// As `x0`, down.
    pub y0: i64,
    /// Cells across its textures hold: core and apron.
    pub w: u32,
    /// Cells down its textures hold: core and apron.
    pub h: u32,
    /// Its height texture's size: powers of two, for the pyramid.
    pub tex_w: u32,
    /// As `tex_w`, down.
    pub tex_h: u32,
}

impl Tile {
    /// The part of `rect` (stock cells) its textures hold, in its own cells.
    pub fn local(&self, rect: CellRect) -> Option<CellRect> {
        let (x1, y1) = (
            self.x0 + i64::from(self.w) - 1,
            self.y0 + i64::from(self.h) - 1,
        );
        let lo_x = i64::from(rect.x0).max(self.x0);
        let lo_y = i64::from(rect.y0).max(self.y0);
        let hi_x = i64::from(rect.x1).min(x1);
        let hi_y = i64::from(rect.y1).min(y1);
        if lo_x > hi_x || lo_y > hi_y {
            return None;
        }
        Some(CellRect {
            x0: u32::try_from(lo_x - self.x0).ok()?,
            y0: u32::try_from(lo_y - self.y0).ok()?,
            x1: u32::try_from(hi_x - self.x0).ok()?,
            y1: u32::try_from(hi_y - self.y0).ok()?,
        })
    }

    /// Its own cells `local`, as stock cells, when every one is on a stock `nx` by
    /// `ny` cells.
    pub fn stock_rect(&self, local: CellRect, nx: u32, ny: u32) -> Option<CellRect> {
        let x0 = u32::try_from(self.x0 + i64::from(local.x0)).ok()?;
        let y0 = u32::try_from(self.y0 + i64::from(local.y0)).ok()?;
        let x1 = u32::try_from(self.x0 + i64::from(local.x1)).ok()?;
        let y1 = u32::try_from(self.y0 + i64::from(local.y1)).ok()?;
        (x1 < nx && y1 < ny).then_some(CellRect { x0, y0, x1, y1 })
    }

    /// The stock cell of its own cell `(x, y)`, if it is on the stock.
    pub fn global(&self, x: u32, y: u32, nx: u32, ny: u32) -> Option<(u32, u32)> {
        let gx = u32::try_from(self.x0 + i64::from(x)).ok()?;
        let gy = u32::try_from(self.y0 + i64::from(y)).ok()?;
        (gx < nx && gy < ny).then_some((gx, gy))
    }
}

/// The tiles over a stock `nx` by `ny` cells, row by row.
pub fn tiles(nx: u32, ny: u32) -> Vec<Tile> {
    let mut out = Vec::new();
    let mut y = 0;
    while y < ny {
        let ch = CORE.min(ny - y);
        let mut x = 0;
        while x < nx {
            let cw = CORE.min(nx - x);
            let (w, h) = (cw + 2 * APRON, ch + 2 * APRON);
            out.push(Tile {
                core: CellRect {
                    x0: x,
                    y0: y,
                    x1: x + cw - 1,
                    y1: y + ch - 1,
                },
                x0: i64::from(x) - i64::from(APRON),
                y0: i64::from(y) - i64::from(APRON),
                w,
                h,
                tex_w: w.next_power_of_two(),
                tex_h: h.next_power_of_two(),
            });
            x += cw;
        }
        y += ch;
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Every cell is some one tile's own, every texture fits the GPU and is a
    /// power of two, and each tile holds its neighbours' cells an apron wide.
    #[test]
    fn tiles_cover_the_stock_once_and_fit_the_gpu() {
        // A full sheet at 0.3 mm, and the lego brick.
        for (nx, ny) in [(5080, 12193), (1974, 1016), (1, 1)] {
            let t = tiles(nx, ny);
            let mut owned = 0u64;
            for tile in &t {
                owned += u64::from(tile.core.width()) * u64::from(tile.core.height());
                assert!(tile.tex_w <= TEXTURE && tile.tex_h <= TEXTURE, "{tile:?}");
                assert!(tile.tex_w.is_power_of_two() && tile.tex_h.is_power_of_two());
                assert!(tile.w <= tile.tex_w && tile.h <= tile.tex_h);
                // Its own cells sit an apron in.
                let core = tile.local(tile.core).unwrap();
                assert_eq!((core.x0, core.y0), (APRON, APRON));
            }
            assert_eq!(owned, u64::from(nx) * u64::from(ny));
        }
        assert_eq!(tiles(5080, 12193).len(), 3 * 6);
        assert_eq!(tiles(1974, 1016).len(), 1, "the brick is one tile");
    }

    #[test]
    fn a_rectangle_lands_in_every_tile_that_holds_it() {
        let t = tiles(5000, 100);
        // Across the seam between the first two tiles.
        let rect = CellRect {
            x0: CORE - 2,
            y0: 10,
            x1: CORE + 1,
            y1: 12,
        };
        let holding: Vec<_> = t.iter().filter_map(|tile| tile.local(rect)).collect();
        assert_eq!(holding.len(), 2);
        assert_eq!(
            holding[0],
            CellRect {
                x0: CORE - 2 + APRON,
                y0: 10 + APRON,
                x1: CORE + 1 + APRON,
                y1: 12 + APRON
            },
            "the first holds it all, the last cells in its apron"
        );
        assert_eq!(holding[1].x0, APRON - 2);
        // A stock rectangle comes back to itself; one reaching past the stock's
        // edge is not all on the stock.
        let local = t[1].local(rect).unwrap();
        assert_eq!(t[1].stock_rect(local, 5000, 100), Some(rect));
        let edge = t[0]
            .local(CellRect {
                x0: 0,
                y0: 0,
                x1: 3,
                y1: 3,
            })
            .unwrap();
        assert_eq!(t[0].stock_rect(edge, 5000, 100).unwrap().x0, 0);
        assert!(
            t[0].stock_rect(
                CellRect {
                    x0: 0,
                    y0: 0,
                    x1: 5,
                    y1: 5
                },
                5000,
                100
            )
            .is_none()
        );
        // Past the stock's edge there is no stock cell.
        assert_eq!(t[0].global(0, 0, 5000, 100), None);
        assert_eq!(t[0].global(APRON, APRON, 5000, 100), Some((0, 0)));
    }
}
