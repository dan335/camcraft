//! The material a level has left, held in tiles, so a pass asks only the
//! tiles near it.

use super::*;

/// A window of the regions around the tool, which every step reads.
#[derive(Debug, Clone, Default)]
pub(super) struct Window {
    pub(super) lo: DVec2,
    pub(super) hi: DVec2,
    pub(super) uncut: Region,
    /// `uncut`'s edges indexed, so a step reads only those near it: made again
    /// whenever `uncut` changes, every few dozen steps, and read by each try
    /// of each.
    pub(super) uncut_index: Indexed,
    /// Where the tool may be, indexed: it does not change.
    pub(super) allowed_index: Indexed,
}

/// How wide a tile is, in tool radii: wide enough that a pass's step reads
/// few, narrow enough that each holds little.
pub(super) const TILE_RADII: f64 = 16.0;

/// Points of a line swept at once: a whole path's sweep is one very large
/// polygon, and slow to take away.
pub(super) const SWEPT_AT_ONCE: usize = 48;

/// A region cut into square tiles, so what is near a point is found, and
/// changed, without touching the rest: a level of a whole sheet has a great
/// deal of rest.
#[derive(Debug, Clone)]
pub(super) struct Tiles {
    pub(super) lo: DVec2,
    pub(super) size: f64,
    pub(super) nx: usize,
    pub(super) ny: usize,
    pub(super) cells: Vec<Region>,
}

impl Tiles {
    /// Take what a tool `radius` wide sweeps along `line` away, a little at a
    /// time: [`SWEPT_AT_ONCE`] points, each piece from the last point of the
    /// one before.
    pub(super) fn subtract_swept(
        &mut self,
        line: &[DVec2],
        radius: f64,
        arc: f64,
    ) -> Result<(), StrategyError> {
        let mut start = 0;
        while start + 1 < line.len() {
            let end = (start + SWEPT_AT_ONCE).min(line.len());
            if let Some(piece) = line.get(start..end) {
                self.subtract(&Region::around_polylines(&[piece.to_vec()], radius, arc))?;
            }
            start = end - 1;
        }
        Ok(())
    }

    /// `region` in tiles of `size`.
    // Tile counts are clamped to 4096 a side, so the conversions are exact.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub(super) fn new(region: &Region, size: f64) -> Result<Self, StrategyError> {
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
        // A row at a time, so each tile is cut from a row, not the whole; the
        // rows in parallel, in order.
        let rows = (0..ny)
            .into_par_iter()
            .map(|j| {
                let (y0, y1) = (lo.y + size * j as f64, lo.y + size * (j + 1) as f64);
                let row = region.intersection(&Region::rectangle(
                    DVec2::new(lo.x - 1.0, y0),
                    DVec2::new(hi.x + 1.0, y1),
                ))?;
                (0..nx)
                    .map(|i| {
                        let (x0, x1) = (lo.x + size * i as f64, lo.x + size * (i + 1) as f64);
                        row.intersection(&Region::rectangle(DVec2::new(x0, y0), DVec2::new(x1, y1)))
                    })
                    .collect::<Result<Vec<Region>, _>>()
            })
            .collect::<Result<Vec<Vec<Region>>, _>>()?;
        Ok(Self {
            lo,
            size,
            nx,
            ny,
            cells: rows.into_iter().flatten().collect(),
        })
    }

    /// The tiles overlapping the box from `lo` to `hi`, as column and row ranges.
    pub(super) fn range(
        &self,
        lo: DVec2,
        hi: DVec2,
    ) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
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
    pub(super) fn window(&self, lo: DVec2, hi: DVec2) -> Result<Region, StrategyError> {
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
    pub(super) fn whole(&self) -> Result<Region, StrategyError> {
        Region::union_all(&self.cells).map_err(StrategyError::from)
    }

    /// The square of tile `k`, its lower and upper corner.
    #[allow(clippy::cast_precision_loss)]
    pub(super) fn square(&self, k: usize) -> (DVec2, DVec2) {
        let (i, j) = ((k % self.nx.max(1)) as f64, (k / self.nx.max(1)) as f64);
        let lo = self.lo + DVec2::new(i, j) * self.size;
        (lo, lo + DVec2::splat(self.size))
    }

    /// The square of tile `k`, and beyond the tiles by `beyond` where it is on
    /// their edge: what of a region grown by up to `beyond` is the tile's own,
    /// none of it lost past the outermost tiles.
    pub(super) fn square_beyond(&self, k: usize, beyond: f64) -> (DVec2, DVec2) {
        let (mut lo, mut hi) = self.square(k);
        let (i, j) = (k % self.nx.max(1), k / self.nx.max(1));
        if i == 0 {
            lo.x -= beyond;
        }
        if j == 0 {
            lo.y -= beyond;
        }
        if i + 1 >= self.nx {
            hi.x += beyond;
        }
        if j + 1 >= self.ny {
            hi.y += beyond;
        }
        (lo, hi)
    }

    /// Each tile's place within the box from `lo` to `hi`, in order.
    pub(super) fn within(&self, lo: DVec2, hi: DVec2) -> Vec<usize> {
        let (xs, ys) = self.range(lo, hi);
        ys.flat_map(|j| xs.clone().map(move |i| j * self.nx + i))
            .collect()
    }

    /// Take `cut` out of every tile it touches.
    pub(super) fn subtract(&mut self, cut: &Region) -> Result<(), StrategyError> {
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
