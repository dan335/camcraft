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

/// `value`, for the parameter called `name`, if it is usable: finite and greater
/// than zero.
pub(crate) fn check(name: &'static str, value: f64) -> Result<(), StrategyError> {
    is_positive(value)
        .then_some(())
        .ok_or(StrategyError::BadParameter { name, value })
}
