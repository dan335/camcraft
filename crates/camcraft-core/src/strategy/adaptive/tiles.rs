//! The material a level has left, held in tiles, so a pass asks only the
//! tiles near it.

use super::*;

/// A window of the regions around the tool, which every step reads.
#[derive(Debug, Clone, Default)]
pub(super) struct Window {
    pub(super) lo: DVec2,
    pub(super) hi: DVec2,
    pub(super) uncut: Region,
    pub(super) allowed: Region,
}

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
