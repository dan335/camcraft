//! Which parts of the material changed, in tiles.
//!
//! A playback step sweeps moves all over the stock: a roughing ring's bounding
//! rectangle is most of the part, though the ring touches only a band of it. So
//! what changed is kept as square tiles of cells, and only those are recoloured,
//! sent, uploaded and taken up the height pyramid.

use camcraft_sim::stock::CellRect;

/// Cells along a tile's side.
pub const TILE: u32 = 32;

/// A set of tiles over a stock `nx` by `ny` cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirtyTiles {
    nx: u32,
    ny: u32,
    cols: u32,
    rows: u32,
    marked: Vec<bool>,
}

impl DirtyTiles {
    /// None marked.
    pub fn new(nx: u32, ny: u32) -> Self {
        let (cols, rows) = (nx.div_ceil(TILE), ny.div_ceil(TILE));
        Self {
            nx,
            ny,
            cols,
            rows,
            marked: vec![false; cols as usize * rows as usize],
        }
    }

    /// Mark every tile `rect` reaches.
    pub fn mark(&mut self, rect: CellRect) {
        self.set(rect, true);
    }

    /// Unmark every tile `rect` reaches.
    pub fn unmark(&mut self, rect: CellRect) {
        self.set(rect, false);
    }

    /// Mark or unmark every tile `rect` reaches.
    fn set(&mut self, rect: CellRect, marked: bool) {
        if self.cols == 0 || self.rows == 0 {
            return;
        }
        let (c0, c1) = (
            (rect.x0 / TILE).min(self.cols - 1),
            (rect.x1 / TILE).min(self.cols - 1),
        );
        let (r0, r1) = (
            (rect.y0 / TILE).min(self.rows - 1),
            (rect.y1 / TILE).min(self.rows - 1),
        );
        for r in r0..=r1 {
            let row = r as usize * self.cols as usize;
            if let Some(span) = self.marked.get_mut(row + c0 as usize..=row + c1 as usize) {
                span.fill(marked);
            }
        }
    }

    /// Whether it covers no cells: those of a stock that has none.
    pub fn is_blank(&self) -> bool {
        self.marked.is_empty()
    }

    /// The first of the marked rectangles ([`DirtyTiles::rects`]) that together hold
    /// no more than `cells`, or the first alone if it holds more.
    pub fn first(&self, cells: u64) -> Vec<CellRect> {
        let mut out = Vec::new();
        let mut held = 0u64;
        for r in self.rects() {
            let n = u64::from(r.width()) * u64::from(r.height());
            if !out.is_empty() && held + n > cells {
                break;
            }
            held += n;
            out.push(r);
        }
        out
    }

    /// Whether none is marked.
    pub fn is_empty(&self) -> bool {
        !self.marked.contains(&true)
    }

    /// Unmark them all.
    pub fn clear(&mut self) {
        self.marked.fill(false);
    }

    /// The marked cells, as few rectangles as runs of tiles along each row of
    /// tiles make, held to the stock.
    pub fn rects(&self) -> Vec<CellRect> {
        let mut out = Vec::new();
        for (r, row) in (0..self.rows).zip(self.marked.chunks(self.cols.max(1) as usize)) {
            let mut c = 0;
            while c < self.cols {
                if !row.get(c as usize).copied().unwrap_or(false) {
                    c += 1;
                    continue;
                }
                let start = c;
                while row.get(c as usize).copied().unwrap_or(false) {
                    c += 1;
                }
                out.push(CellRect {
                    x0: start * TILE,
                    y0: r * TILE,
                    x1: (c * TILE).min(self.nx) - 1,
                    y1: ((r + 1) * TILE).min(self.ny) - 1,
                });
            }
        }
        out
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn covered(rects: &[CellRect], x: u32, y: u32) -> bool {
        rects.iter().any(|r| r.contains(x, y))
    }

    /// The rectangles cover every marked cell and no tile that was not marked,
    /// stay inside the stock, and do not overlap.
    #[test]
    fn tiles_cover_what_was_marked_and_little_else() {
        let (nx, ny) = (100, 70);
        let mut t = DirtyTiles::new(nx, ny);
        assert!(t.is_empty() && t.rects().is_empty());
        let a = CellRect {
            x0: 5,
            y0: 3,
            x1: 40,
            y1: 10,
        };
        let b = CellRect {
            x0: 90,
            y0: 60,
            x1: 99,
            y1: 69,
        };
        t.mark(a);
        t.mark(b);
        let rects = t.rects();
        for r in [a, b] {
            for y in r.y0..=r.y1 {
                for x in r.x0..=r.x1 {
                    assert!(covered(&rects, x, y), "({x}, {y})");
                }
            }
        }
        assert!(!covered(&rects, 50, 40), "an unmarked tile");
        for r in &rects {
            assert!(r.x1 < nx && r.y1 < ny, "{r:?}");
        }
        let cells: u32 = rects.iter().map(|r| r.width() * r.height()).sum();
        let distinct = (0..ny)
            .flat_map(|y| (0..nx).map(move |x| (x, y)))
            .filter(|&(x, y)| covered(&rects, x, y))
            .count();
        assert_eq!(cells as usize, distinct, "rectangles overlap");
        // A run along a row of tiles is one rectangle.
        assert_eq!(
            rects.first().unwrap(),
            &CellRect {
                x0: 0,
                y0: 0,
                x1: 63,
                y1: 31
            }
        );

        // A batch no larger than asked, and unmarking it leaves the rest.
        let first = t.first(64 * 32);
        assert_eq!(first, vec![rects[0]]);
        t.unmark(first[0]);
        assert_eq!(t.rects(), rects[1..].to_vec());
        t.clear();
        assert!(t.is_empty());
        assert!(!t.is_blank() && DirtyTiles::new(0, 5).is_blank());
    }
}
