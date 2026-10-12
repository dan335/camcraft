//! Regular height fields.
//!
//! A flat `Vec<f32>` plus dimensions, row-major. The layout is deliberate: it is
//! what a `wgpu` buffer wants, so moving a fill or a dilation onto the GPU later is
//! a kernel swap rather than a data-structure rewrite.
//!
//! # f32 storage, f64 arithmetic
//!
//! Values are computed in `f64` and stored in `f32`. Across a 300 mm part f32 leaves
//! ~0.00003 mm of representation error, far below any tolerance of interest, while
//! halving the memory of a large grid — a 300 mm part at 0.05 mm pitch is 36 million
//! cells. The split is a decision, not an accident.
//!
//! # Interpolated values are not cutter locations
//!
//! Sampling between nodes is safe for *region* questions — which area to clear at
//! which depth — because a small error there only nudges a boundary. It is **not**
//! safe for emitting a path point. Where the underlying surface is convex the
//! interpolant passes below it, and below is a gouge. Path points come from the
//! exact query; the grid is an index, not an answer.

use glam::{DVec2, dvec2};

/// Largest grid this crate will allocate: 2^28 cells, about 1 GB at f32.
///
/// A bound, not a guess — an unbounded allocation driven by a tolerance value is a
/// way to turn a typo into an out-of-memory kill.
pub(crate) const MAX_CELLS: usize = 1 << 28;

/// A regular grid of heights over an axis-aligned XY region.
#[derive(Debug, Clone, PartialEq)]
pub struct HeightMap {
    data: Vec<f32>,
    nx: u32,
    ny: u32,
    origin: DVec2,
    pitch: f64,
}

/// A height field that could not be created.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum HeightMapError {
    /// A grid must have at least one cell on each axis.
    #[error("grid dimensions must be non-zero, got {nx} x {ny}")]
    EmptyGrid {
        /// Requested width.
        nx: u32,
        /// Requested height.
        ny: u32,
    },
    /// Pitch must be finite and positive.
    #[error("grid pitch must be finite and greater than zero, got {0}")]
    BadPitch(f64),
    /// The origin was NaN or infinite.
    #[error("grid origin is not finite: {0:?}")]
    BadOrigin(DVec2),
    /// The grid would have more than 2^28 cells, the most this crate allocates.
    #[error("grid of {nx} x {ny} needs {cells} cells, over the {MAX_CELLS} limit")]
    TooLarge {
        /// Requested width.
        nx: u32,
        /// Requested height.
        ny: u32,
        /// Cells the request implies.
        cells: u128,
    },
}

/// Narrow a computed value to storage width.
///
/// The one place `f64 -> f32` happens, so the decision documented at module level has
/// exactly one home. Public because parallel fills write the buffer directly.
#[allow(clippy::cast_possible_truncation)]
pub fn narrow(v: f64) -> f32 {
    v as f32
}

/// The cell at `v`, a position along an axis of `n` cells counted in cells and
/// already rounded to a whole one: clamped into the axis, and the first cell when
/// `v` is not finite.
pub fn cell_index(v: f64, n: u32) -> u32 {
    if !v.is_finite() {
        return 0;
    }
    // Clamped into the axis, so the narrowing is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let i = v.clamp(0.0, f64::from(n.saturating_sub(1))) as u32;
    i
}

impl HeightMap {
    /// A grid of `nx` by `ny` cells with the given spacing, every cell set to `value`.
    ///
    /// `origin` is the world XY of cell `(0, 0)`; cell `(ix, iy)` sits at
    /// `origin + (ix, iy) * pitch`.
    pub fn filled(
        origin: DVec2,
        pitch: f64,
        nx: u32,
        ny: u32,
        value: f64,
    ) -> Result<Self, HeightMapError> {
        if nx == 0 || ny == 0 {
            return Err(HeightMapError::EmptyGrid { nx, ny });
        }
        if !pitch.is_finite() || pitch <= 0.0 {
            return Err(HeightMapError::BadPitch(pitch));
        }
        if !origin.is_finite() {
            return Err(HeightMapError::BadOrigin(origin));
        }
        let cells = u128::from(nx) * u128::from(ny);
        if cells > MAX_CELLS as u128 {
            return Err(HeightMapError::TooLarge { nx, ny, cells });
        }
        Ok(Self {
            data: vec![narrow(value); nx as usize * ny as usize],
            nx,
            ny,
            origin,
            pitch,
        })
    }

    /// A grid covering `[min, max]` in XY at the given pitch, every cell set to
    /// `value`.
    ///
    /// The grid is grown outward to whole cells, so it always covers the region asked
    /// for rather than clipping it.
    pub fn covering(
        min: DVec2,
        max: DVec2,
        pitch: f64,
        value: f64,
    ) -> Result<Self, HeightMapError> {
        if !pitch.is_finite() || pitch <= 0.0 {
            return Err(HeightMapError::BadPitch(pitch));
        }
        if !min.is_finite() || !max.is_finite() {
            return Err(HeightMapError::BadOrigin(if min.is_finite() {
                max
            } else {
                min
            }));
        }
        let (nx, ny) = Self::covering_size(min, max, pitch);
        Self::filled(min, pitch, nx, ny, value)
    }

    /// The columns and rows [`HeightMap::covering`] lays over `[min, max]` at
    /// `pitch`, which must be finite and greater than zero: for a grid that is
    /// laid out the same way but not allocated.
    pub fn covering_size(min: DVec2, max: DVec2, pitch: f64) -> (u32, u32) {
        let span = (max - min).max(DVec2::ZERO);
        let cells = |s: f64| -> u32 {
            let n = (s / pitch).ceil() + 1.0;
            if n.is_finite() && n >= 1.0 {
                // Clamped into range and finite, so the narrowing is exact.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let n = n.min(f64::from(u32::MAX)) as u32;
                n
            } else {
                1
            }
        };
        (cells(span.x), cells(span.y))
    }

    /// Grid width in cells.
    pub fn nx(&self) -> u32 {
        self.nx
    }

    /// Grid height in cells.
    pub fn ny(&self) -> u32 {
        self.ny
    }

    /// Spacing between cell centres, in millimetres.
    pub fn pitch(&self) -> f64 {
        self.pitch
    }

    /// World XY of cell `(0, 0)`.
    pub fn origin(&self) -> DVec2 {
        self.origin
    }

    /// The raw row-major buffer.
    pub fn data(&self) -> &[f32] {
        &self.data
    }

    /// The raw row-major buffer, mutably — for parallel fills.
    pub fn data_mut(&mut self) -> &mut [f32] {
        &mut self.data
    }

    /// World XY of cell `(ix, iy)`, or `None` if out of range.
    pub fn position(&self, ix: u32, iy: u32) -> Option<DVec2> {
        (ix < self.nx && iy < self.ny)
            .then(|| self.origin + dvec2(f64::from(ix) * self.pitch, f64::from(iy) * self.pitch))
    }

    /// Height at cell `(ix, iy)`, or `None` if out of range or unset.
    ///
    /// Unset cells are stored as NaN, so "no geometry here" and "height here" are
    /// distinguishable rather than being conflated with zero.
    pub fn get(&self, ix: u32, iy: u32) -> Option<f64> {
        if ix >= self.nx || iy >= self.ny {
            return None;
        }
        let v = f64::from(
            *self
                .data
                .get(iy as usize * self.nx as usize + ix as usize)?,
        );
        v.is_finite().then_some(v)
    }

    /// Set cell `(ix, iy)`. Returns false if out of range.
    pub fn set(&mut self, ix: u32, iy: u32, value: f64) -> bool {
        if ix >= self.nx || iy >= self.ny {
            return false;
        }
        let nx = self.nx as usize;
        match self.data.get_mut(iy as usize * nx + ix as usize) {
            Some(slot) => {
                *slot = narrow(value);
                true
            }
            None => false,
        }
    }

    /// Bilinear sample at world `(x, y)`.
    ///
    /// `None` outside the grid, or when any of the four surrounding cells is unset —
    /// a partial neighbourhood would otherwise silently invent geometry.
    ///
    /// Not a cutter location. See the module docs.
    pub fn sample(&self, x: f64, y: f64) -> Option<f64> {
        let fx = (x - self.origin.x) / self.pitch;
        let fy = (y - self.origin.y) / self.pitch;
        if !fx.is_finite() || !fy.is_finite() || fx < 0.0 || fy < 0.0 {
            return None;
        }
        let (ix, iy) = (fx.floor(), fy.floor());
        // Past the grid there is nothing to sample; and a cell's neighbour is
        // then always one more that fits.
        if ix >= f64::from(self.nx) || iy >= f64::from(self.ny) {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (ix, iy) = (ix as u32, iy as u32);
        let (tx, ty) = (fx - f64::from(ix), fy - f64::from(iy));

        // Clamp the upper corner so a sample exactly on the far edge still reads.
        let jx = if ix + 1 < self.nx { ix + 1 } else { ix };
        let jy = if iy + 1 < self.ny { iy + 1 } else { iy };

        let v00 = self.get(ix, iy)?;
        let v10 = self.get(jx, iy)?;
        let v01 = self.get(ix, jy)?;
        let v11 = self.get(jx, jy)?;

        let a = v00 + (v10 - v00) * tx;
        let b = v01 + (v11 - v01) * tx;
        Some(a + (b - a) * ty)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unusable_parameters() {
        let o = dvec2(0.0, 0.0);
        assert_eq!(
            HeightMap::filled(o, 1.0, 0, 4, 0.0),
            Err(HeightMapError::EmptyGrid { nx: 0, ny: 4 })
        );
        assert_eq!(
            HeightMap::filled(o, 0.0, 4, 4, 0.0),
            Err(HeightMapError::BadPitch(0.0))
        );
        assert!(matches!(
            HeightMap::filled(o, f64::NAN, 4, 4, 0.0),
            Err(HeightMapError::BadPitch(_))
        ));
        assert!(matches!(
            HeightMap::filled(dvec2(f64::NAN, 0.0), 1.0, 4, 4, 0.0),
            Err(HeightMapError::BadOrigin(_))
        ));
    }

    /// An unbounded allocation driven by a tolerance typo is an OOM kill, so the
    /// limit must actually be enforced.
    #[test]
    fn refuses_an_absurd_grid() {
        let err = HeightMap::filled(dvec2(0.0, 0.0), 0.001, 100_000, 100_000, 0.0);
        assert!(matches!(err, Err(HeightMapError::TooLarge { .. })));
    }

    #[test]
    fn maps_cells_to_world_positions() {
        let g = HeightMap::filled(dvec2(-5.0, 10.0), 0.5, 4, 3, 0.0).unwrap();
        assert_eq!(g.position(0, 0), Some(dvec2(-5.0, 10.0)));
        assert_eq!(g.position(3, 2), Some(dvec2(-3.5, 11.0)));
        assert_eq!(g.position(4, 0), None);
        assert_eq!(g.position(0, 3), None);
    }

    #[test]
    fn unset_cells_read_as_none_not_zero() {
        let mut g = HeightMap::filled(dvec2(0.0, 0.0), 1.0, 2, 2, f64::NAN).unwrap();
        assert_eq!(g.get(0, 0), None);
        assert!(g.set(0, 0, 3.5));
        assert_eq!(g.get(0, 0), Some(3.5));
        assert!(!g.set(9, 9, 1.0));
        assert_eq!(g.get(9, 9), None);
    }

    #[test]
    fn covering_encloses_the_requested_region() {
        let g = HeightMap::covering(dvec2(-1.0, -2.0), dvec2(1.0, 2.0), 0.3, 0.0).unwrap();
        let last = g.position(g.nx() - 1, g.ny() - 1).unwrap();
        assert!(last.x >= 1.0, "grid must reach past x max, got {}", last.x);
        assert!(last.y >= 2.0, "grid must reach past y max, got {}", last.y);
        assert_eq!(g.position(0, 0), Some(dvec2(-1.0, -2.0)));
    }

    /// Bilinear reproduces a plane exactly; that is the baseline any interpolation
    /// error must be measured against.
    #[test]
    fn samples_a_linear_field_exactly() {
        let mut g = HeightMap::filled(dvec2(0.0, 0.0), 1.0, 8, 8, 0.0).unwrap();
        let f = |x: f64, y: f64| 2.0 + 0.5 * x - 0.25 * y;
        for iy in 0..g.ny() {
            for ix in 0..g.nx() {
                let p = g.position(ix, iy).unwrap();
                g.set(ix, iy, f(p.x, p.y));
            }
        }
        for (x, y) in [(0.0, 0.0), (3.7, 2.2), (6.9, 6.9), (7.0, 7.0)] {
            let got = g.sample(x, y).unwrap();
            assert!((got - f(x, y)).abs() < 1e-9, "at ({x},{y}) got {got}");
        }
    }

    #[test]
    fn sampling_outside_the_grid_is_none() {
        let g = HeightMap::filled(dvec2(0.0, 0.0), 1.0, 4, 4, 1.0).unwrap();
        assert_eq!(g.sample(-0.001, 0.0), None);
        assert_eq!(g.sample(0.0, -0.001), None);
        assert_eq!(g.sample(100.0, 0.0), None);
        assert_eq!(g.sample(f64::NAN, 0.0), None);
    }

    #[test]
    fn a_hole_in_the_neighbourhood_reads_as_none() {
        let mut g = HeightMap::filled(dvec2(0.0, 0.0), 1.0, 4, 4, 1.0).unwrap();
        g.set(1, 1, f64::NAN);
        assert_eq!(g.sample(0.5, 0.5), None, "must not invent geometry");
        assert_eq!(g.sample(2.5, 2.5), Some(1.0));
    }
}
