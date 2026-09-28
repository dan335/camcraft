//! What the strategies share: the error one fails with, and the test its
//! parameters must pass.

use glam::DVec2;

use crate::is_positive;
use crate::toolpath::ToolpathError;

/// A strategy that could not produce a path.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum StrategyError {
    /// A parameter was outside its usable range.
    #[error("{name} must be finite and greater than zero, got {value}")]
    BadParameter {
        /// Which parameter.
        name: &'static str,
        /// What it was.
        value: f64,
    },
    /// The region was empty or inverted.
    #[error("region is empty or inverted: {min:?} to {max:?}")]
    EmptyRegion {
        /// Lower corner.
        min: DVec2,
        /// Upper corner.
        max: DVec2,
    },
    /// A move could not be added.
    #[error(transparent)]
    Toolpath(#[from] ToolpathError),
    /// The area is too large for the field a strategy works out over it: its grid
    /// would have to be so coarse it no longer places the passes.
    #[error(
        "the area is too large to finish at this stepover: its grid would need a pitch of \
         {pitch:.3} mm; keep it to a boundary, or take a larger stepover"
    )]
    FieldTooLarge {
        /// The pitch the grid would need.
        pitch: f64,
    },
    /// A polygon operation failed.
    ///
    /// Propagated rather than swallowed: an empty region reads as "nothing to cut",
    /// which is indistinguishable from a correct answer.
    #[error("region operation failed: {0}")]
    Region(String),
}

impl From<camcraft_geom::contour::RegionError> for StrategyError {
    fn from(e: camcraft_geom::contour::RegionError) -> Self {
        StrategyError::Region(e.to_string())
    }
}

impl From<crate::reach::ReachError> for StrategyError {
    fn from(e: crate::reach::ReachError) -> Self {
        StrategyError::Region(e.to_string())
    }
}

/// Levels closer than this are one, in millimetres.
const SAME_LEVEL: f64 = 1.0e-6;

/// Z levels from `top` down to `bottom`, `stepdown` apart, the last at `bottom`,
/// each with the flat levels among `flats` that lie between it and the one above,
/// lowest first: each is cut after the level just below it. Bounded, so a tiny
/// stepdown against a tall part cannot run away.
pub(crate) fn slabs(top: f64, bottom: f64, stepdown: f64, flats: &[f64]) -> Vec<(f64, Vec<f64>)> {
    let count = ((top - bottom) / stepdown).ceil().clamp(1.0, 10_000.0);
    // Clamped to a small whole number, so the narrowing is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = count as u32;
    let mut slabs: Vec<(f64, Vec<f64>)> = Vec::new();
    for i in 1..=count {
        let z = (top - f64::from(i) * stepdown).max(bottom);
        slabs.push((z, Vec::new()));
        if z <= bottom {
            break;
        }
    }
    for &f in flats {
        if !(f > bottom && f < top) || slabs.iter().any(|(z, _)| (z - f).abs() < SAME_LEVEL) {
            continue;
        }
        if let Some(slab) = slabs.iter_mut().find(|(z, _)| *z < f) {
            slab.1.push(f);
        }
    }
    for (_, flats) in &mut slabs {
        flats.sort_by(f64::total_cmp);
        flats.dedup_by(|a, b| (*a - *b).abs() < SAME_LEVEL);
    }
    slabs
}

/// `value`, for the parameter called `name`, if it is usable: finite and greater
/// than zero.
pub(crate) fn check(name: &'static str, value: f64) -> Result<(), StrategyError> {
    is_positive(value)
        .then_some(())
        .ok_or(StrategyError::BadParameter { name, value })
}
