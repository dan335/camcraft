//! Where a cutter may not be, at a level, straight from the model.
//!
//! The tool tip may not stand at `(x, y, z)` if the cutter there would overlap the
//! surface. That is exactly the drop-cutter's `Zcl(x, y) > z`, so roughing, the
//! finishing strategies and the simulator all mean the same thing by it, and the
//! drop-cutter is the oracle the tests hold this to.
//!
//! # From shadows, never a grid
//!
//! Grids are only for stock. This region is built from the mesh itself: a cutter is
//! a solid of revolution with radius `r(h)` at height `h` above its tip, so it
//! overlaps a surface point `p` exactly when `p` is above the tip and within
//! `r(p.z - z)` of the axis. So the forbidden region is the union, over every
//! height `h`, of the shadow above `z + h` grown by `r(h)`.
//!
//! - A **flat** mill has one radius, and the region is exact: the shadow above `z`
//!   grown by `R`.
//! - A **ball** or **bullnose** (which is what stock-to-leave turns every cutter
//!   into) widens over its corner. The corner is cut into bands across which the
//!   radius grows by at most a band step, half the tolerance, and each band's
//!   shadow is grown by the radius at the **top** of the band. That can only
//!   overstate the region, never understate it, and by at most one band step
//!   ([`LevelReach::overstates`]).
//!   Vertical walls and flats come out exact, because there every band sees the
//!   same shadow and the full radius wins.
//!
//! Exact slicing of the offset surface would remove the band step altogether. It
//! would replace [`LevelReach::forbidden`] and nothing else.
//!
//! # The error budget
//!
//! On the side that could gouge, the only errors are the mesh's own chordal error
//! and rounding to Clipper's 0.1 µm ticks; every offset is padded so its arc
//! approximation errs outward. Everything else errs toward leaving material: the
//! band step and the arc padding. Two tiny allowances go the other way: a triangle
//! seen edge-on is taken as a line, which it misses by at most
//! [`EDGE_ON_WIDTH`](camcraft_geom::shadow::EDGE_ON_WIDTH), and a line within
//! [`ON_AREA`](camcraft_geom::shadow::ON_AREA) of the shadow counts as on it.

use std::sync::Arc;

use camcraft_geom::contour::{Region, RegionError};
use camcraft_geom::mesh::TriMesh;
use camcraft_geom::shadow::{Shadow, Shadows};
use rayon::prelude::*;

use crate::cutter::allowance::EffectiveTool;
use crate::cutter::tool::ToolProfile;
use crate::is_positive;

/// A band limit so that a pathological tool cannot allocate without bound.
const MAX_BANDS: usize = 4096;

/// Why a forbidden region could not be built.
#[derive(Debug, thiserror::Error)]
pub enum ReachError {
    /// The tolerance was not a positive finite number.
    #[error("tolerance must be positive, got {0}")]
    Tolerance(f64),
    /// The cutter undercuts, so its radius is not a function of height alone.
    #[error("an undercutting cutter cannot rough by levels")]
    Undercutting,
    /// The cutter's profile could not be divided into bands.
    #[error("cutter profile could not be divided into bands of {step} mm")]
    Profile {
        /// The band step asked for.
        step: f64,
    },
    /// A polygon operation failed.
    #[error(transparent)]
    Region(#[from] RegionError),
}

/// One height band of the cutter: from `height` above the tip up to the next band,
/// grown by `radius`, the most the cutter reaches anywhere in it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Band {
    height: f64,
    radius: f64,
}

/// The forbidden region of one cutter against one mesh, at any level.
#[derive(Debug)]
pub struct LevelReach<'m> {
    shadows: Shadows<'m>,
    /// Bands from the tip up.
    bands: Vec<Band>,
    /// How far the controlled point sits above the profile's own tip.
    lift: f64,
    max_radius: f64,
    /// How much wider than the real tool the profile is: the stock to leave
    /// beside, or zero.
    leave: f64,
    tolerance: f64,
    band_step: f64,
    arc_tolerance: f64,
}

impl<'m> LevelReach<'m> {
    /// Prepare for `tool`, as the drop-cutter contacts with it, against `mesh`.
    ///
    /// The tool is the effective one, dilated by its stock to leave, and levels are
    /// the real tool tip's, as they are everywhere else.
    pub fn new(
        mesh: &'m TriMesh,
        tool: &EffectiveTool,
        tolerance: f64,
    ) -> Result<Self, ReachError> {
        if !is_positive(tolerance) {
            return Err(ReachError::Tolerance(tolerance));
        }
        if !tool.is_monotonic() {
            return Err(ReachError::Undercutting);
        }
        // Half the tolerance for the bands, a tenth for the arcs: both only ever
        // leave material, and together they stay well inside what was asked.
        let band_step = tolerance * 0.5;
        let arc_tolerance = tolerance * 0.1;
        let bands = bands(tool, band_step)?;
        Ok(Self {
            shadows: Shadows::new(mesh),
            bands,
            lift: tool.tip_offset(),
            max_radius: tool.max_radius(),
            leave: tool.max_radius() - tool.cutter_radius(),
            tolerance,
            band_step,
            arc_tolerance,
        })
    }

    /// Where the tool tip may not be at level `z`.
    ///
    /// Asking for levels from the top down is cheapest; see [`Shadows`].
    pub fn forbidden(&mut self, z: f64) -> Result<Region, ReachError> {
        let tip = z - self.lift;
        // Highest band first, so the shadows are asked for in descending order.
        let mut shadows = Vec::with_capacity(self.bands.len());
        for band in self.bands.iter().rev() {
            let shadow = self.shadows.above(tip + band.height)?;
            if !shadow.is_empty() {
                shadows.push((shadow, band.radius));
            }
        }
        // Growing each band is independent, so it is done in parallel; the order is
        // kept, so the union is the same on any number of cores.
        let grown = shadows
            .par_iter()
            .map(|(shadow, radius)| shadow.grown(*radius, self.arc_tolerance))
            .collect::<Result<Vec<Region>, RegionError>>()?;
        Ok(Region::union_all(&grown)?)
    }

    /// Where the tool tip may not be at each of `levels`, in their order: each
    /// the same region [`forbidden`](Self::forbidden) gives, if not always the
    /// same points round it.
    ///
    /// A level asks for the shadows above each of its bands, which reach up the
    /// tool's corner. Asked level by level, the next level down starts above
    /// where the last one ended whenever the corner is taller than the step
    /// between them, and the shadows start again from the top. Here every
    /// height of every level is asked once, highest first, so the shadows are
    /// built once all the way down; a level's region is put together as soon as
    /// its lowest band is in.
    pub fn forbidden_all(&mut self, levels: &[f64]) -> Result<Vec<Region>, ReachError> {
        // Every band of every level: the height asked, whose it is, and its
        // radius; highest first, and, as `forbidden` has them, each level's
        // bands from the top of the tool down.
        let mut asks: Vec<(f64, usize, f64)> = levels
            .iter()
            .enumerate()
            .flat_map(|(i, &z)| {
                let tip = z - self.lift;
                self.bands
                    .iter()
                    .rev()
                    .map(move |band| (tip + band.height, i, band.radius))
            })
            .collect();
        asks.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut left: Vec<usize> = vec![self.bands.len(); levels.len()];
        let mut grown: Vec<Vec<Region>> = vec![Vec::new(); levels.len()];
        let mut out: Vec<Option<Region>> = vec![None; levels.len()];
        // Grown in parallel a batch at a time, each level's in its order, so the
        // unions are the same on any number of cores.
        let mut batch: Vec<(usize, Arc<Shadow>, f64)> = Vec::new();
        let mut last: Option<(f64, Arc<Shadow>)> = None;
        // Each level's shadow for the band above: a band whose shadow is the same
        // is grown less, inside what that one was, and adds nothing. Where the
        // part is walls and flats, most bands are.
        let mut above: Vec<Option<Arc<Shadow>>> = vec![None; levels.len()];
        let flush = |batch: &mut Vec<(usize, Arc<Shadow>, f64)>,
                     grown: &mut Vec<Vec<Region>>,
                     arc: f64|
         -> Result<(), ReachError> {
            let regions = batch
                .par_iter()
                .map(|(_, shadow, radius)| shadow.grown(*radius, arc))
                .collect::<Result<Vec<Region>, RegionError>>()?;
            for ((i, ..), region) in batch.drain(..).zip(regions) {
                if let Some(list) = grown.get_mut(i) {
                    list.push(region);
                }
            }
            Ok(())
        };
        // Levels complete, each unioned on its own, a batch at a time in parallel.
        let mut ready: Vec<(usize, Vec<Region>)> = Vec::new();
        let union = |ready: &mut Vec<(usize, Vec<Region>)>,
                     out: &mut Vec<Option<Region>>|
         -> Result<(), ReachError> {
            let regions = ready
                .par_iter()
                .map(|(_, mine)| Region::union_all(mine))
                .collect::<Result<Vec<Region>, RegionError>>()?;
            for ((i, _), region) in ready.drain(..).zip(regions) {
                if let Some(slot) = out.get_mut(i) {
                    *slot = Some(region);
                }
            }
            Ok(())
        };
        for (t, i, radius) in asks {
            let shadow = match &last {
                // The very same height, another level's: asked once.
                Some((at, shadow)) if at.to_bits() == t.to_bits() => Arc::clone(shadow),
                _ => {
                    let shadow = Arc::new(self.shadows.above(t)?);
                    last = Some((t, Arc::clone(&shadow)));
                    shadow
                }
            };
            let covered = above
                .get(i)
                .and_then(Option::as_ref)
                .is_some_and(|a| *a == shadow);
            if !shadow.is_empty() && !covered {
                if let Some(slot) = above.get_mut(i) {
                    *slot = Some(Arc::clone(&shadow));
                }
                batch.push((i, shadow, radius));
            }
            let Some(count) = left.get_mut(i) else {
                continue;
            };
            *count -= 1;
            if *count == 0 {
                // The level is complete: its bands grown, then unioned.
                flush(&mut batch, &mut grown, self.arc_tolerance)?;
                let mine = grown.get_mut(i).map(std::mem::take).unwrap_or_default();
                ready.push((i, mine));
                if ready.len() >= rayon::current_num_threads() {
                    union(&mut ready, &mut out)?;
                }
            } else if batch.len() >= 64 {
                flush(&mut batch, &mut grown, self.arc_tolerance)?;
            }
        }
        union(&mut ready, &mut out)?;
        Ok(out.into_iter().map(Option::unwrap_or_default).collect())
    }

    /// The cutter's largest radius.
    pub fn max_radius(&self) -> f64 {
        self.max_radius
    }

    /// The real tool's largest radius, without the stock to leave: how far
    /// round its centre it cuts.
    pub fn cutter_radius(&self) -> f64 {
        self.max_radius - self.leave
    }

    /// The chordal tolerance this was built to.
    pub fn tolerance(&self) -> f64 {
        self.tolerance
    }

    /// The most the region may overstate the true one by: its bands, and the
    /// arcs it is drawn to, either side.
    pub fn overstates(&self) -> f64 {
        self.band_step + 2.0 * self.arc_tolerance
    }

    /// The arc tolerance every offset is drawn to, and padded by.
    pub fn arc_tolerance(&self) -> f64 {
        self.arc_tolerance
    }
}

/// Divide a profile into bands across which the radius grows by at most `step`.
///
/// Band `k` runs from `h_k` to `h_{k+1}`, where `h_k` is the height at which the
/// radius first reaches `r(0) + k * step`. Within it the radius is below
/// `r(0) + (k + 1) * step`, which is what the band is grown by, and at least
/// `r(0) + k * step`, so the band overstates it by at most `step`.
fn bands(profile: &(impl ToolProfile + ?Sized), step: f64) -> Result<Vec<Band>, ReachError> {
    let full = profile.max_radius();
    let mut out = Vec::new();
    let mut height = 0.0;
    let mut reach = profile.radius_at(0.0) + step;
    loop {
        if reach >= full {
            out.push(Band {
                height,
                radius: full,
            });
            return Ok(out);
        }
        out.push(Band {
            height,
            radius: reach,
        });
        if out.len() >= MAX_BANDS {
            return Err(ReachError::Profile { step });
        }
        height = profile
            .height_at(reach)
            .filter(|h| h.is_finite() && *h >= height)
            .ok_or(ReachError::Profile { step })?;
        reach += step;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests;
