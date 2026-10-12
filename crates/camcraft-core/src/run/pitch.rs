//! How finely a setup's material is simulated.

use glam::DVec2;

use crate::is_positive;

/// Cells a small stock is given, at least: as fine as that allows, so a small
/// part is simulated, verified and shown in fine detail whatever its tools.
pub(super) const SIM_CELLS: f64 = 2.0e6;

/// Most cells any stock is given, to bound memory: each copy of the material is
/// four bytes a cell, and a run and its playback keep several. A full 5 by 12
/// foot sheet reaches this at about 0.3 mm.
pub(super) const MAX_SIM_CELLS: f64 = 64.0e6;

/// Finest simulation pitch, in millimetres, however small the stock.
pub(super) const FINEST_PITCH: f64 = 0.02;

/// Cells across the smallest tool: its slot is this many cells wide.
pub(super) const CELLS_PER_TOOL: f64 = 8.0;

/// The grid pitch for the material over a stock `size` across, at `tolerance`,
/// for a setup whose narrowest tool is `narrowest` across.
///
/// Fine enough for that tool to cut a slot [`CELLS_PER_TOOL`] cells wide, and
/// never coarser than eight times the tolerance (at most 1 mm); finer still when
/// the stock is small enough to spend [`SIM_CELLS`] on it; but never so fine that
/// it takes more than [`MAX_SIM_CELLS`], and never finer than [`FINEST_PITCH`].
pub(super) fn sim_pitch(tolerance: f64, size: DVec2, narrowest: Option<f64>) -> f64 {
    let mut wanted = (tolerance * 8.0).clamp(0.05, 1.0);
    if let Some(d) = narrowest.filter(|d| is_positive(*d)) {
        wanted = wanted.min(d / CELLS_PER_TOOL);
    }
    let area = (size.x * size.y).abs();
    if !area.is_finite() {
        return wanted.max(FINEST_PITCH);
    }
    let small = (area / SIM_CELLS).sqrt();
    let most = (area / MAX_SIM_CELLS).sqrt();
    wanted.min(small).max(most).max(FINEST_PITCH)
}
