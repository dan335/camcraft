//! The part a program is meant to leave, sampled on the stock's grid, which the
//! material is judged against.

use camcraft_geom::heightmap::{HeightMap, HeightMapError, cell_index, narrow};
use camcraft_geom::mesh::TriMesh;
use glam::{DVec2, DVec3, dvec2};
use rayon::prelude::*;

use super::{CellRect, StockError};

/// The part's own upper surface, sampled per column.
///
/// Not a cutter-location surface: this is the shape the program is supposed to
/// leave behind, which is what a gouge is measured against.
///
/// # Gouges beside walls
///
/// A column's height alone cannot judge material beside a vertical wall. The
/// meshed part and the cut differ sideways by up to the tolerance (a hole meshed
/// as a polygon inside its circle, a cell straddling a wall), and sideways beside
/// a wall is the wall's whole height down. So a surface given a reach
/// ([`PartSurface::reaching`]) judges material against the lowest the part comes
/// within that distance across: material is gouged only when it is below that by
/// more than the budget, which holds the tolerance in every direction rather than
/// only downwards. A real gouge into a wall, deeper than that distance, is still
/// below the wall's foot and still found. The lowest nearby is looked for only
/// where material is already that far below the column's own height, which is
/// rare, so it is not kept for every column.
#[derive(Debug, Clone)]
pub struct PartSurface {
    /// The part's window of the grid: its heights, unset where there is none.
    field: HeightMap,
    /// The window's first cell, in the grid.
    at: (u32, u32),
    /// The grid it is judged on: its first cell, pitch, columns and rows.
    grid: Grid,
    /// Offsets, in cells, to every cell within the reach across.
    disc: Vec<(i64, i64)>,
}

impl PartSurface {
    /// Rasterise the top surface of a mesh over `[min, max]`.
    pub fn from_mesh(
        mesh: &TriMesh,
        min: DVec2,
        max: DVec2,
        pitch: f64,
    ) -> Result<Self, StockError> {
        // The grid it is judged on, the stock's: as `HeightMap::covering` lays one
        // out, but not allocated, since the part may cover little of it.
        if !pitch.is_finite() || pitch <= 0.0 {
            return Err(HeightMapError::BadPitch(pitch).into());
        }
        let (nx, ny) = HeightMap::covering_size(min, max, pitch);
        let grid = (min, pitch, nx, ny);
        let window = window(mesh, grid);
        let mut field = HeightMap::filled(
            min + dvec2(f64::from(window.x0) * pitch, f64::from(window.y0) * pitch),
            pitch,
            window.width(),
            window.height(),
            f64::NAN,
        )?;
        let rows = rows_of(mesh, grid, window);
        rasterise(&mut field, mesh, &rows, grid, window);
        Ok(Self {
            field,
            at: (window.x0, window.y0),
            grid,
            disc: Vec::new(),
        })
    }

    /// Height of the part at a column, or `None` where the part is absent.
    pub fn height_at(&self, x: f64, y: f64) -> Option<f64> {
        self.field.sample(x, y)
    }

    /// Judge gouges allowing `reach` across as well as down: see the type's docs.
    #[must_use]
    pub fn reaching(mut self, reach: f64) -> Self {
        let cells = if reach.is_finite() && reach > 0.0 {
            reach / self.field.pitch()
        } else {
            0.0
        };
        #[allow(clippy::cast_possible_truncation)]
        let k = cells.floor().min(64.0) as i64;
        self.disc = (-k..=k)
            .flat_map(|dy| (-k..=k).map(move |dx| (dx, dy)))
            .filter(|&(dx, dy)| {
                #[allow(clippy::cast_precision_loss)]
                let d2 = (dx * dx + dy * dy) as f64;
                d2 <= cells * cells + 1.0e-9
            })
            .collect();
        self
    }

    /// The part as an operation that cuts `radial` past its walls and `axial`
    /// below its floors means to leave it, in the cells of the grid it is judged
    /// on that `cut` says it cut: each the lowest the part comes within `radial`
    /// of it, less `axial`, and no part at all where that reaches past the
    /// part's edge. Negative stock to leave, as asked, is then no gouge, and
    /// only where the operation that asked for it cut.
    #[must_use]
    pub fn cut_past(mut self, radial: f64, axial: f64, cut: impl Fn(u32, u32) -> bool) -> Self {
        let cells = radial.max(0.0) / self.field.pitch();
        #[allow(clippy::cast_possible_truncation)]
        let k = cells.ceil().min(64.0) as i64;
        let disc: Vec<(i64, i64)> = (-k..=k)
            .flat_map(|dy| (-k..=k).map(move |dx| (dx, dy)))
            .filter(|&(dx, dy)| {
                #[allow(clippy::cast_precision_loss)]
                let d2 = (dx * dx + dy * dy) as f64;
                d2 <= cells * cells + 1.0e-9
            })
            .collect();
        let (nx, ny) = (self.field.nx(), self.field.ny());
        let mut out = self.field.data().to_vec();
        for wy in 0..ny {
            for wx in 0..nx {
                let (gx, gy) = (wx + self.at.0, wy + self.at.1);
                if self.field.get(wx, wy).is_none() || !cut(gx, gy) {
                    continue;
                }
                let low = disc.iter().try_fold(f64::INFINITY, |low, &(dx, dy)| {
                    self.at_cell(i64::from(gx) + dx, i64::from(gy) + dy)
                        .map(|v| low.min(v))
                });
                if let Some(slot) = out.get_mut(wy as usize * nx as usize + wx as usize) {
                    *slot = low.map_or(f32::NAN, |l| narrow(l - axial.max(0.0)));
                }
            }
        }
        self.field.data_mut().copy_from_slice(&out);
        self
    }

    /// The part's height at cell `(ix, iy)` of the grid it is judged on, if it
    /// has one there.
    pub(super) fn at_cell(&self, ix: i64, iy: i64) -> Option<f64> {
        let (x, y) = (ix - i64::from(self.at.0), iy - i64::from(self.at.1));
        self.field
            .get(u32::try_from(x).ok()?, u32::try_from(y).ok()?)
    }

    /// The lowest the part comes within the reach of cell `(ix, iy)`, no higher
    /// than `own`: minus infinity beside a column with no part, or past the grid's
    /// edge, where there is no part either.
    fn floor_at(&self, ix: i64, iy: i64, own: f64) -> f64 {
        let mut low = own;
        for &(dx, dy) in &self.disc {
            match self.at_cell(ix + dx, iy + dy) {
                Some(v) => low = low.min(v),
                None => return f64::NEG_INFINITY,
            }
        }
        low
    }

    /// The grid's cell nearest `(x, y)`.
    fn nearest(&self, x: f64, y: f64) -> Option<(i64, i64)> {
        let (o, pitch, _, _) = self.grid;
        let (fx, fy) = (((x - o.x) / pitch).round(), ((y - o.y) / pitch).round());
        if !(fx.is_finite() && fy.is_finite()) || fx.abs() > 1.0e15 || fy.abs() > 1.0e15 {
            return None;
        }
        #[allow(clippy::cast_possible_truncation)]
        let cell = (fx as i64, fy as i64);
        Some(cell)
    }

    /// The lowest the part comes near `(x, y)`, within the reach it was given
    /// ([`PartSurface::reaching`]), or its own height if it was given none. Minus
    /// infinity beside a column with no part; `None` where there is no part.
    #[cfg(test)]
    pub(crate) fn floor_near(&self, x: f64, y: f64) -> Option<f64> {
        let own = self.height_at(x, y)?;
        Some(match self.nearest(x, y) {
            Some((ix, iy)) => self.floor_at(ix, iy, own),
            None => own,
        })
    }

    /// The part's window of the grid, and where its first cell is in the grid.
    pub fn window(&self) -> (&HeightMap, (u32, u32)) {
        (&self.field, self.at)
    }

    /// The position of cell `(ix, iy)` of the grid the part is judged on, worked
    /// out as the grid works it out.
    pub fn position(&self, ix: u32, iy: u32) -> DVec2 {
        let (o, pitch, _, _) = self.grid;
        o + dvec2(f64::from(ix) * pitch, f64::from(iy) * pitch)
    }

    /// Whether this surface is judged on exactly the cells of `field`, so a
    /// column can be read by its index rather than by position.
    pub fn on_grid_of(&self, field: &HeightMap) -> bool {
        let (o, pitch, nx, ny) = self.grid;
        nx == field.nx()
            && ny == field.ny()
            && pitch.to_bits() == field.pitch().to_bits()
            && o == field.origin()
    }

    /// How to judge material standing on a cell of `field` against the part: a
    /// function of the cell, its position and the material's height there, and
    /// the budget, giving the part's height and how far the material is below the
    /// part (from the lowest the part comes nearby, where that could matter), or
    /// `None` where there is no part. Everything that judges material finds the
    /// part this way, so they agree cell for cell.
    pub fn lookup<'a>(
        &'a self,
        field: &HeightMap,
    ) -> impl Fn(u32, u32, DVec2, f64, f64) -> Option<(f64, f64)> + 'a {
        let same = self.on_grid_of(field);
        move |ix, iy, p, stock, budget| {
            let (own, cell) = if same {
                let cell = (i64::from(ix), i64::from(iy));
                (self.at_cell(cell.0, cell.1)?, Some(cell))
            } else {
                (self.height_at(p.x, p.y)?, self.nearest(p.x, p.y))
            };
            let under = own - stock;
            // The lowest nearby is no higher than the column itself, so it only
            // matters where the material is already too deep below the column.
            if under <= budget {
                return Some((own, under));
            }
            let floor = cell.map_or(own, |(x, y)| self.floor_at(x, y, own));
            Some((own, floor - stock))
        }
    }
}

/// How much wider than a triangle the part is looked for: a hair, so a column
/// its edge passes through is not missed.
const SLACK: f64 = 1.0e-6;

/// The grid a part is judged on: its first cell, pitch, columns and rows.
type Grid = (DVec2, f64, u32, u32);

/// The cells of `grid` the part's footprint reaches: its window, a hair wider than
/// the triangles. Past the window there is no part.
fn window(mesh: &TriMesh, (origin, pitch, nx, ny): Grid) -> CellRect {
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for i in 0..mesh.len() {
        if let Some(t) = mesh.triangle(i) {
            for p in t {
                lo = lo.min(p.truncate());
                hi = hi.max(p.truncate());
            }
        }
    }
    if !(lo.x <= hi.x && lo.y <= hi.y) {
        return CellRect::cell(0, 0);
    }
    // A column of the grid for an offset `v` from its first cell.
    let index = |v: f64, n: u32, round: fn(f64) -> f64| cell_index(round(v / pitch), n);
    CellRect {
        x0: index(lo.x - SLACK - origin.x, nx, f64::floor),
        x1: index(hi.x + SLACK - origin.x, nx, f64::ceil),
        y0: index(lo.y - SLACK - origin.y, ny, f64::floor),
        y1: index(hi.y + SLACK - origin.y, ny, f64::ceil),
    }
}

/// Each of the window's rows' triangles: those whose extent in Y reaches it.
fn rows_of(mesh: &TriMesh, (origin, pitch, _, _): Grid, window: CellRect) -> Vec<Vec<usize>> {
    let (y0, y1) = (window.y0, window.y1);
    let mut rows: Vec<Vec<usize>> = vec![Vec::new(); window.height() as usize];
    for i in 0..mesh.len() {
        let Some(t) = mesh.triangle(i) else { continue };
        let lo = t[0].y.min(t[1].y).min(t[2].y) - SLACK;
        let hi = t[0].y.max(t[1].y).max(t[2].y) + SLACK;
        let first = ((lo - origin.y) / pitch).ceil().max(f64::from(y0));
        let last = ((hi - origin.y) / pitch).floor().min(f64::from(y1));
        if !(first.is_finite() && last.is_finite()) || first > last {
            continue;
        }
        // Clamped into the window, so the narrowing is exact.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (first, last) = ((first as u32 - y0) as usize, (last as u32 - y0) as usize);
        for row in rows.get_mut(first..=last).into_iter().flatten() {
            row.push(i);
        }
    }
    rows
}

/// Set each column of `field`, the window of the grid, to the highest of the
/// triangles covering it, `rows` being each row's triangles, each column at the
/// grid's own position for it.
///
/// Parallel over rows, each writing only its own slice, so the result does not
/// depend on thread count; and the maximum does not depend on the order it is
/// taken.
fn rasterise(
    field: &mut HeightMap,
    mesh: &TriMesh,
    rows: &[Vec<usize>],
    (origin, pitch, _, _): Grid,
    window: CellRect,
) {
    let CellRect { x0, y0, x1, y1 } = window;
    field
        .data_mut()
        .par_chunks_mut(window.width() as usize)
        .zip(rows.par_iter())
        .zip(y0..y1 + 1)
        .for_each(|((row, tris), iy)| {
            let y = origin.y + f64::from(iy) * pitch;
            for &i in tris {
                let Some(t) = mesh.triangle(i) else { continue };
                let lo = t[0].x.min(t[1].x).min(t[2].x) - SLACK;
                let hi = t[0].x.max(t[1].x).max(t[2].x) + SLACK;
                let first = ((lo - origin.x) / pitch).ceil().max(f64::from(x0));
                let last = ((hi - origin.x) / pitch).floor().min(f64::from(x1));
                if !(first.is_finite() && last.is_finite()) || first > last {
                    continue;
                }
                // Clamped into the window, so the narrowing is exact.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let (first, last) = (first as u32, last as u32);
                for ix in first..=last {
                    let x = origin.x + f64::from(ix) * pitch;
                    let (Some(z), Some(cell)) =
                        (triangle_height(&t, x, y), row.get_mut((ix - x0) as usize))
                    else {
                        continue;
                    };
                    let z = narrow(z);
                    if cell.is_nan() || z > *cell {
                        *cell = z;
                    }
                }
            }
        });
}

/// Highest point of the mesh directly above `(x, y)`, by looking at every
/// triangle: what [`PartSurface::from_mesh`] must agree with.
#[cfg(test)]
pub(super) fn column_height(mesh: &TriMesh, x: f64, y: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    for i in 0..mesh.len() {
        let Some(t) = mesh.triangle(i) else { continue };
        if let Some(z) = triangle_height(&t, x, y) {
            best = Some(match best {
                Some(b) if b >= z => b,
                _ => z,
            });
        }
    }
    best
}

/// Height of a triangle above `(x, y)`, or `None` if it does not cover the column.
fn triangle_height(t: &[DVec3; 3], x: f64, y: f64) -> Option<f64> {
    let (a, b, c) = (t[0], t[1], t[2]);
    let (ab, ac) = (b - a, c - a);
    let det = ab.x * ac.y - ab.y * ac.x;
    if det.abs() < 1.0e-18 {
        // Vertical or degenerate: contributes no column height of its own, and its
        // edges belong to the neighbouring faces.
        return None;
    }
    let (px, py) = (x - a.x, y - a.y);
    let u = (px * ac.y - py * ac.x) / det;
    let v = (ab.x * py - ab.y * px) / det;
    const E: f64 = 1.0e-9;
    if u >= -E && v >= -E && u + v <= 1.0 + E {
        Some(a.z + ab.z * u + ac.z * v)
    } else {
        None
    }
}
