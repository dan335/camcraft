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
    pub fn restore(&self, stock: &mut Stock) -> Option<Option<CellRect>> {
        let b = &stock.field;
        if self.nx != b.nx() || self.ny != b.ny() || self.pitch.to_bits() != b.pitch().to_bits() {
            return None;
        }
        stock.floor = self.floor;
        let nx = stock.field.nx() as usize;
        let mut changed: Option<CellRect> = None;
        for (i, (slot, v)) in stock
            .field
            .data_mut()
            .iter_mut()
            .zip(self.heights())
            .enumerate()
        {
            if slot.to_bits() != v.to_bits() {
                *slot = v;
                let (ix, iy) = (u32::try_from(i % nx).ok()?, u32::try_from(i / nx).ok()?);
                changed = CellRect::join(changed, Some(CellRect::cell(ix, iy)));
            }
        }
        Some(changed)
    }
}
