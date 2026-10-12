//! Stock kept small, for the keyframes and checkpoints a replay restores.

use camcraft_geom::heightmap::HeightMap;
use glam::DVec2;

use super::{CellRect, Stock, StockError};

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
    ///
    /// Row by row, and a row run by run: only the stretch of each that differs
    /// is written, and the cells changed are joined a row at a time.
    pub fn restore(&self, stock: &mut Stock) -> Option<Option<CellRect>> {
        let f = &stock.field;
        if self.nx != f.nx()
            || self.ny != f.ny()
            || self.pitch.to_bits() != f.pitch().to_bits()
            || self.origin != f.origin()
        {
            return None;
        }
        stock.floor = self.floor;
        let nx = self.nx as usize;
        let rows = stock.field.data_mut().chunks_exact_mut(nx).zip(0u32..);
        let mut changed: Option<CellRect> = None;
        let mut mark = |iy: u32, (x0, x1): (usize, usize)| {
            // Within a row of a grid `nx` wide, so both fit.
            #[allow(clippy::cast_possible_truncation)]
            let r = CellRect {
                x0: x0 as u32,
                y0: iy,
                x1: x1 as u32,
                y1: iy,
            };
            changed = CellRect::join(changed, Some(r));
        };
        match &self.cells {
            Packed::Raw(raw) => {
                for ((row, iy), packed) in rows.zip(raw.chunks_exact(nx)) {
                    if let Some(span) = put_row(row, packed) {
                        mark(iy, span);
                    }
                }
            }
            Packed::Runs(runs) => {
                let mut runs = runs.iter().copied();
                let (mut run, mut left) = (0u32, 0usize);
                for (row, iy) in rows {
                    let mut span: Option<(usize, usize)> = None;
                    let mut at = 0;
                    while at < nx {
                        if left == 0 {
                            let Some((b, n)) = runs.next() else { break };
                            (run, left) = (b, n as usize);
                            continue;
                        }
                        let n = left.min(nx - at);
                        if let Some(cells) = row.get_mut(at..at + n)
                            && let Some((x0, x1)) = put_run(cells, run)
                        {
                            let (x0, x1) = (at + x0, at + x1);
                            span = Some(span.map_or((x0, x1), |(lo, _)| (lo, x1)));
                        }
                        at += n;
                        left -= n;
                    }
                    if let Some(span) = span {
                        mark(iy, span);
                    }
                }
            }
        }
        Some(changed)
    }
}

/// Set `cells` to the height whose bits are `run`, from the first that differs
/// to the last: which cells those were, or `None` when none differ.
fn put_run(cells: &mut [f32], run: u32) -> Option<(usize, usize)> {
    let first = cells.iter().position(|v| v.to_bits() != run)?;
    let last = cells
        .iter()
        .rposition(|v| v.to_bits() != run)
        .unwrap_or(first);
    if let Some(changed) = cells.get_mut(first..=last) {
        changed.fill(f32::from_bits(run));
    }
    Some((first, last))
}

/// Set `cells` to the heights whose bits are `packed`, from the first that
/// differs to the last: which cells those were, or `None` when none differ.
fn put_row(cells: &mut [f32], packed: &[u32]) -> Option<(usize, usize)> {
    let differs = |(v, b): (&f32, &u32)| v.to_bits() != *b;
    let first = cells.iter().zip(packed).position(differs)?;
    let last = cells.iter().zip(packed).rposition(differs).unwrap_or(first);
    if let (Some(changed), Some(packed)) = (cells.get_mut(first..=last), packed.get(first..=last)) {
        for (v, b) in changed.iter_mut().zip(packed) {
            *v = f32::from_bits(*b);
        }
    }
    Some((first, last))
}
