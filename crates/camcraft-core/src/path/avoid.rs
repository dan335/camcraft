//! A finishing path kept to the faces its operation machines, as Fusion's
//! keeps to its machine and avoid groups: what of each pass rests only on
//! faces avoided is cut out, and what is left of it, in pieces, is passes of
//! their own, for the linker to join round.
//!
//! The strategies lay their passes on the part as the operation meets it
//! ([`PartSurfaces`](crate::cutter::surfaces::PartSurfaces)), every group of
//! faces under the tool grown by its offsets, so a pass already keeps a face
//! avoided's clearance; here it is cut where that is all it rests on. Cut, not
//! lifted over: Fusion's programs lift between the pieces, its rapids doubled
//! where a face is avoided.

use super::held::cut_out;
use super::toolpath::{Toolpath, ToolpathError};
use super::trim::Trimmed;
use crate::cutter::surfaces::Seat;
use crate::cutter::tool::ToolProfile;

/// `path`, a strategy's passes each a [`MoveRole::Link`] to its start and then
/// its cuts, laid on the part as `seat` holds the tool over it, less where the
/// tool would come within `tolerance` of faces it avoids and of none it
/// machines ([`Seat::avoids`]); each move looked at along its chords, at least
/// every half the tool's radius, and cut where that changes to within the
/// tolerance.
///
/// A piece left that touches no face it machines anywhere ([`Seat::machines`])
/// is left out too: it would only go through the air towards what it avoids,
/// as the way down along a ring at a level does, from above, to a ring cut
/// out. A pass dropped onto the part rests on it at every point it was laid
/// at, so only passes that go round the part at levels have such pieces.
///
/// [`MoveRole::Link`]: super::toolpath::MoveRole::Link
pub fn machined_only(
    path: &Toolpath,
    seat: &Seat<'_>,
    tolerance: f64,
) -> Result<(Toolpath, Trimmed), ToolpathError> {
    // As finely as a pass is sampled where it is flat: nothing narrower than
    // a quarter of the tool is passed over.
    let step = (seat.tool().max_radius() * 0.5).max(tolerance);
    let (pieces, report) = cut_out(path, |p| seat.avoids(p, tolerance), step, tolerance)?;
    let mut kept = Toolpath::new();
    let mut after = 0;
    for pass in pieces
        .passes()
        .filter(|pass| pass.iter().any(|m| seat.machines(m.to, tolerance)))
    {
        kept.append(pass)?;
        after += 1;
    }
    Ok((
        kept,
        Trimmed {
            before: report.before,
            after,
        },
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests;
