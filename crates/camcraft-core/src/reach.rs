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
//!   radius grows by at most [`band_step`](LevelReach::band_step), and each band's
//!   shadow is grown by the radius at the **top** of the band. That can only
//!   overstate the region, never understate it, and by at most one band step.
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

use camcraft_geom::contour::{Region, RegionError};
use camcraft_geom::mesh::TriMesh;
use camcraft_geom::shadow::Shadows;

use rayon::prelude::*;

use crate::allowance::EffectiveTool;
use crate::is_positive;
use crate::tool::ToolProfile;

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
        Self::with_profile(mesh, tool, tool.tip_offset(), tolerance)
    }

    /// Prepare for any monotonic `profile` whose controlled point sits `lift` above
    /// its own tip.
    pub fn with_profile(
        mesh: &'m TriMesh,
        profile: &(impl ToolProfile + ?Sized),
        lift: f64,
        tolerance: f64,
    ) -> Result<Self, ReachError> {
        if !is_positive(tolerance) {
            return Err(ReachError::Tolerance(tolerance));
        }
        if !profile.is_monotonic() {
            return Err(ReachError::Undercutting);
        }
        // Half the tolerance for the bands, a tenth for the arcs: both only ever
        // leave material, and together they stay well inside what was asked.
        let band_step = tolerance * 0.5;
        let arc_tolerance = tolerance * 0.1;
        let bands = bands(profile, band_step)?;
        Ok(Self {
            shadows: Shadows::new(mesh),
            bands,
            lift,
            max_radius: profile.max_radius(),
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

    /// The cutter's largest radius.
    pub fn max_radius(&self) -> f64 {
        self.max_radius
    }

    /// The chordal tolerance this was built to.
    pub fn tolerance(&self) -> f64 {
        self.tolerance
    }

    /// The most the region may overstate the true one by, from the banding alone.
    pub fn band_step(&self) -> f64 {
        self.band_step
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
mod tests {
    use super::*;
    use crate::allowance::Allowance;
    use crate::dropcutter::DropCutter;
    use crate::tool::{Ball, Bullnose, Flat};
    use camcraft_geom::synthetic;
    use glam::dvec2;

    #[test]
    fn a_flat_mill_is_one_band() {
        let b = bands(&Flat::new(3.0).unwrap(), 0.025).unwrap();
        assert_eq!(
            b,
            vec![Band {
                height: 0.0,
                radius: 3.0
            }]
        );
    }

    /// Every band's radius is at least the cutter's anywhere in it, and at most a
    /// step more than the least.
    #[test]
    fn bands_bound_the_profile_from_above_within_a_step() {
        let step = 0.05;
        let tools: Vec<Box<dyn ToolProfile>> = vec![
            Box::new(Ball::new(3.0).unwrap()),
            Box::new(Bullnose::new(3.5, 0.5).unwrap()),
            Box::new(Bullnose::new(4.0, 1.7).unwrap()),
        ];
        for tool in &tools {
            let b = bands(tool.as_ref(), step).unwrap();
            for (k, band) in b.iter().enumerate() {
                let top = b.get(k + 1).map_or(band.height + 10.0, |n| n.height);
                for i in 0..=200 {
                    let h = band.height + (top - band.height) * f64::from(i) / 200.0;
                    // The top of each band belongs to the next.
                    if i == 200 && k + 1 < b.len() {
                        continue;
                    }
                    let r = tool.radius_at(h);
                    assert!(
                        r <= band.radius + 1e-12,
                        "band {k} at {h}: {r} > {}",
                        band.radius
                    );
                    assert!(
                        band.radius - r <= step + 1e-9,
                        "band {k} at {h}: {r} vs {}",
                        band.radius
                    );
                }
            }
        }
    }

    /// The oracle: the forbidden region is where the drop-cutter's answer is above
    /// the level, to within the band step and the arc padding, and never short of it.
    fn agrees_with_drop_cutter(mesh: &TriMesh, tool: &EffectiveTool, levels: &[f64], half: f64) {
        let tolerance = 0.05;
        let mut reach = LevelReach::new(mesh, tool, tolerance).unwrap();
        let dc = DropCutter::new(mesh);
        let slack = reach.band_step() + 2.0 * reach.arc_tolerance() + 0.002;
        let n = 61;
        for &z in levels {
            let f = reach.forbidden(z).unwrap();
            let core = f.offset(-slack, reach.arc_tolerance());
            let mut forbidden_seen = 0;
            for i in 0..n {
                for j in 0..n {
                    // Off the lattice the region is aligned with, deliberately.
                    let x = -half + 2.0 * half * (f64::from(i) + 0.37) / f64::from(n);
                    let y = -half + 2.0 * half * (f64::from(j) + 0.61) / f64::from(n);
                    let p = dvec2(x, y);
                    let zcl = dc.drop_at(tool, x, y);
                    if zcl.is_some_and(|h| h > z + 1e-6) {
                        forbidden_seen += 1;
                        assert!(
                            f.contains(p),
                            "z {z}: ({x:.3}, {y:.3}) has Zcl {zcl:?} but is allowed"
                        );
                    }
                    if core.contains(p) {
                        assert!(
                            zcl.is_some_and(|h| h > z - 1e-6),
                            "z {z}: ({x:.3}, {y:.3}) is forbidden by more than the \
                             slack but Zcl is {zcl:?}"
                        );
                    }
                }
            }
            assert!(forbidden_seen > 0, "z {z}: the test saw nothing forbidden");
        }
    }

    #[test]
    fn a_flat_mill_on_a_sphere_agrees_with_the_drop_cutter() {
        let mesh = synthetic::uv_sphere(8.0, 60, 90);
        let tool = EffectiveTool::new(&Flat::new(2.0).unwrap(), Allowance::none()).unwrap();
        agrees_with_drop_cutter(&mesh, &tool, &[6.0, 2.0, -1.0], 12.0);
    }

    #[test]
    fn a_ball_on_a_sphere_agrees_with_the_drop_cutter() {
        let mesh = synthetic::uv_sphere(8.0, 60, 90);
        let tool = EffectiveTool::new(&Ball::new(2.0).unwrap(), Allowance::none()).unwrap();
        agrees_with_drop_cutter(&mesh, &tool, &[6.0, 2.0, -3.0], 12.0);
    }

    /// Stock to leave makes a bullnose of a flat mill and lifts the tip; both must
    /// be read the way the drop-cutter reads them.
    #[test]
    fn stock_to_leave_agrees_with_the_drop_cutter() {
        let mesh = synthetic::uv_sphere(8.0, 60, 90);
        let tool = EffectiveTool::new(&Flat::new(2.0).unwrap(), Allowance::new(0.6, 0.3).unwrap())
            .unwrap();
        agrees_with_drop_cutter(&mesh, &tool, &[6.0, 2.0, -3.0], 12.0);
    }

    #[test]
    fn a_step_agrees_with_the_drop_cutter() {
        let mesh = synthetic::step(8.0, 10.0);
        for tool in [
            EffectiveTool::new(&Flat::new(2.0).unwrap(), Allowance::none()).unwrap(),
            EffectiveTool::new(
                &Bullnose::new(2.0, 0.5).unwrap(),
                Allowance::isotropic(0.5).unwrap(),
            )
            .unwrap(),
        ] {
            agrees_with_drop_cutter(&mesh, &tool, &[7.0, 3.0], 9.0);
        }
    }

    #[test]
    fn nonsense_tolerances_are_refused() {
        let mesh = synthetic::plane(0.0, 5.0);
        let tool = EffectiveTool::new(&Flat::new(2.0).unwrap(), Allowance::none()).unwrap();
        for bad in [0.0, -1.0, f64::NAN] {
            assert!(LevelReach::new(&mesh, &tool, bad).is_err());
        }
    }
}
