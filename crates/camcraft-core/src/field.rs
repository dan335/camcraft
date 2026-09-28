//! A grid over an area, for a field worked out across it: scallop's distances
//! along the part, a morphed spiral's blend from the boundary to its centre.
//!
//! The grid only places passes, seen from above (decision 0009): each node is
//! inside the area or beside it, and has a height, the tool's over the part or
//! none, and the field's lines are passes that are then cut on the part exactly.

use camcraft_geom::contour::Region;
use camcraft_geom::mesh::closest_point_on_segment;
use camcraft_geom::trigrid::TriGrid;
use glam::DVec2;
use rayon::prelude::*;

use crate::strategy::StrategyError;

/// A grid over an area: which of its nodes are inside, and how it was sized.
pub(crate) struct AreaGrid {
    /// The grid, a node's height given only inside the area and beside it.
    pub(crate) grid: TriGrid,
    /// Whether each node is inside the area.
    pub(crate) inside: Vec<bool>,
    /// Spacing of the nodes, in millimetres.
    pub(crate) pitch: f64,
    /// Nodes in all.
    pub(crate) nodes: u64,
    /// Whether it was coarsened to stay within its budget.
    pub(crate) coarsened: bool,
}

impl AreaGrid {
    /// A grid over `area`, whose bounds are `lo` to `hi`, `pitch` apart, coarsened
    /// a tenth at a time until it has no more than `max_nodes`, and too large past
    /// `coarsest`. Each node inside the area and beside it has `height` there,
    /// asked row by row in parallel, each row with scratch space of its own.
    pub(crate) fn over(
        area: &Region,
        (lo, hi): (DVec2, DVec2),
        mut pitch: f64,
        coarsest: f64,
        max_nodes: u32,
        height: impl Fn(f64, f64, &mut Vec<u32>) -> Option<f64> + Sync,
    ) -> Result<Self, StrategyError> {
        let size = |pitch: f64| {
            let span = hi - lo + DVec2::splat(4.0 * pitch);
            let nx = count((span.x / pitch).ceil() + 1.0)?;
            let ny = count((span.y / pitch).ceil() + 1.0)?;
            Some((nx, ny, u64::from(nx) * u64::from(ny)))
        };
        // Coarser until it fits; bounded, as the pitch grows past the coarsest.
        let mut coarsened = false;
        let (nx, ny, total) = loop {
            if pitch > coarsest {
                return Err(StrategyError::FieldTooLarge { pitch });
            }
            if let Some(shape) = size(pitch).filter(|&(_, _, n)| n <= u64::from(max_nodes)) {
                break shape;
            }
            pitch *= 1.1;
            coarsened = true;
        };
        let origin = lo - DVec2::splat(2.0 * pitch);
        let (wide, count_nodes) = (
            usize::try_from(nx).unwrap_or(0),
            usize::try_from(total).unwrap_or(0),
        );

        // Inside the area, a row at a time; and the nodes beside them.
        let mut inside = vec![false; count_nodes];
        for iy in 0..ny {
            let y = origin.y + f64::from(iy) * pitch;
            for (a, b) in area.spans(DVec2::X, y) {
                let first = count(((a - origin.x) / pitch).ceil().max(0.0)).unwrap_or(0);
                let last = count(((b - origin.x) / pitch).floor())
                    .unwrap_or(0)
                    .min(nx - 1);
                for ix in first..=last {
                    if let Some(cell) =
                        index(u64::from(iy * nx + ix)).and_then(|i| inside.get_mut(i))
                    {
                        *cell = true;
                    }
                }
            }
        }
        let is_inside = |ix: i64, iy: i64| {
            let (Ok(x), Ok(y)) = (u32::try_from(ix), u32::try_from(iy)) else {
                return false;
            };
            x < nx
                && y < ny
                && index(u64::from(y * nx + x))
                    .and_then(|i| inside.get(i))
                    .copied()
                    .unwrap_or(false)
        };
        let active: Vec<bool> = (0..total)
            .map(|n| {
                let (ix, iy) = (
                    i64::try_from(n % u64::from(nx)).unwrap_or(0),
                    i64::try_from(n / u64::from(nx)).unwrap_or(0),
                );
                (-1..=1).any(|dy| (-1..=1).any(|dx| is_inside(ix + dx, iy + dy)))
            })
            .collect();

        // The height at each, row by row in parallel.
        let mut z = vec![f32::NAN; count_nodes];
        z.par_chunks_mut(wide.max(1))
            .enumerate()
            .for_each_init(Vec::new, |scratch, (iy, row)| {
                let y = origin.y + f64::from(u32::try_from(iy).unwrap_or(0)) * pitch;
                for (ix, cell) in row.iter_mut().enumerate() {
                    if !active.get(iy * wide + ix).copied().unwrap_or(false) {
                        continue;
                    }
                    let x = origin.x + f64::from(u32::try_from(ix).unwrap_or(0)) * pitch;
                    if let Some(v) = height(x, y, scratch) {
                        // Grid storage: f32, as every grid in the workspace.
                        #[allow(clippy::cast_possible_truncation)]
                        let v = v as f32;
                        *cell = v;
                    }
                }
            });
        let grid = TriGrid::new(origin, pitch, nx, ny, z, active)
            .ok_or_else(|| StrategyError::Region("the field's grid could not be made".into()))?;
        Ok(Self {
            grid,
            inside,
            pitch,
            nodes: total,
            coarsened,
        })
    }

    /// Each node within a cell's diagonal of `area`'s boundary, with a height:
    /// how far it is from the boundary, seen from above, and the nearest point of
    /// the boundary to it. `None` for the rest.
    pub(crate) fn boundary_feet(&self, area: &Region) -> Vec<Option<(f64, DVec2)>> {
        let grid = &self.grid;
        let pitch = self.pitch;
        let (nx, ny) = (grid.nx(), grid.ny());
        let origin = grid.origin();
        let reach = std::f64::consts::SQRT_2 * pitch + 1e-9;
        let segments: Vec<(DVec2, DVec2)> = area
            .rings()
            .iter()
            .flat_map(|r| {
                let p = r.points();
                p.iter()
                    .copied()
                    .zip(p.iter().copied().cycle().skip(1))
                    .take(p.len())
            })
            .collect();
        let near: Vec<Vec<(u32, f64, DVec2)>> = segments
            .par_iter()
            .map(|&(a, b)| {
                let mut out = Vec::new();
                let (min, max) = (
                    a.min(b) - DVec2::splat(1.5 * pitch),
                    a.max(b) + DVec2::splat(1.5 * pitch),
                );
                let x0 = count(((min.x - origin.x) / pitch).floor().max(0.0)).unwrap_or(0);
                let y0 = count(((min.y - origin.y) / pitch).floor().max(0.0)).unwrap_or(0);
                let x1 = count(((max.x - origin.x) / pitch).ceil())
                    .unwrap_or(0)
                    .min(nx - 1);
                let y1 = count(((max.y - origin.y) / pitch).ceil())
                    .unwrap_or(0)
                    .min(ny - 1);
                for iy in y0..=y1 {
                    for ix in x0..=x1 {
                        let Some(n) = grid.node(ix, iy) else { continue };
                        let Some(p) = grid.xy(n).filter(|_| grid.defined(n)) else {
                            continue;
                        };
                        let foot =
                            closest_point_on_segment(p.extend(0.0), a.extend(0.0), b.extend(0.0))
                                .truncate();
                        let flat = p.distance(foot);
                        if flat <= reach {
                            out.push((n, flat, foot));
                        }
                    }
                }
                out
            })
            .collect();
        // Each node's nearest point of the boundary.
        let mut feet: Vec<Option<(f64, DVec2)>> = vec![None; self.inside.len()];
        for (n, flat, foot) in near.into_iter().flatten() {
            if let Some(slot) = index(u64::from(n)).and_then(|i| feet.get_mut(i))
                && slot.is_none_or(|(best, _)| flat < best)
            {
                *slot = Some((flat, foot));
            }
        }
        feet
    }

    /// Nodes inside the area, with a height, that `d` leaves unknown.
    pub(crate) fn unreached(&self, d: &[f64]) -> u64 {
        d.iter()
            .zip(&self.inside)
            .enumerate()
            .filter(|&(n, (d, &inside))| {
                inside && !d.is_finite() && u32::try_from(n).is_ok_and(|n| self.grid.defined(n))
            })
            .count() as u64
    }
}

/// A whole, non-negative count that fits a `u32`.
pub(crate) fn count(v: f64) -> Option<u32> {
    // Checked in range first, so the narrowing is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (v.is_finite() && (0.0..=f64::from(u32::MAX)).contains(&v)).then_some(v as u32)
}

/// `n` as an index.
pub(crate) fn index(n: u64) -> Option<usize> {
    usize::try_from(n).ok()
}
