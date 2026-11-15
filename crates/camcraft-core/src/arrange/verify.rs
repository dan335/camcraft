//! The exact check of a layout, with the outlines themselves.
//!
//! Nothing of the grid is used here. Each part placed is its outline where
//! its stand puts it; grown by the spacing, it must meet no other part's
//! outline, and grown by the border, it must lie on the sheet. So a fault in
//! how the grid covers a part cannot pass by being reasoned the same way
//! twice.

use camcraft_geom::contour::{Region, RegionError};
use glam::DVec2;
use rayon::prelude::*;

use super::{ArrangeError, Outcome, Part, Sheet, TICK_ERROR, Violation, check};

/// How far a round join's chords may fall inside its arc as the check grows
/// an outline, in millimetres: half a micrometre. The clipper grows by this
/// much more, so a grown outline holds the exact growth, but for the
/// clipper's rounding to its tick.
const ARC: f64 = 5.0e-4;

/// How far past the exact growth an outline the check grows can reach: the
/// arc's padding and the clipper's rounding, 0.64 µm. A layout whose parts
/// keep the border and the spacing by less than this more may fail the
/// check; the grid keeps them by several times it ([`super::grid`]).
pub(super) const ERROR: f64 = ARC + TICK_ERROR;

/// Whether `outcomes`, one for each of `parts` in order, keep the sheet's
/// rules: every part placed stands at a turn it may take, at least the
/// border inside the sheet's edge, and at least the spacing from every other
/// part placed, to within the clipper's rounding of a fraction of a
/// micrometre.
pub fn verify(
    sheet: &Sheet,
    shapes: &[Region],
    parts: &[Part],
    outcomes: &[Outcome],
) -> Result<(), ArrangeError> {
    check(sheet, shapes, parts)?;
    if outcomes.len() != parts.len() {
        return Err(ArrangeError::Unverified(Violation::Outcomes {
            parts: parts.len(),
            outcomes: outcomes.len(),
        }));
    }
    let mut placed: Vec<(usize, Region)> = Vec::new();
    for (k, (part, outcome)) in parts.iter().zip(outcomes).enumerate() {
        let Outcome::Placed(stand) = outcome else {
            continue;
        };
        if !part.turns.contains(&stand.turn) {
            return Err(ArrangeError::Unverified(Violation::Turn { part: k }));
        }
        if let Some(shape) = shapes.get(part.shape) {
            placed.push((k, stand.region(shape)));
        }
    }

    let sheet_area = Region::rectangle(sheet.corner, sheet.corner + sheet.size);
    let off_sheet: Vec<Result<bool, RegionError>> = placed
        .par_iter()
        .map(|(_, outline)| {
            let grown = outline.offset(sheet.border, ARC);
            Ok(!grown.difference(&sheet_area)?.is_empty())
        })
        .collect();
    for ((k, _), off) in placed.iter().zip(off_sheet) {
        if off? {
            return Err(ArrangeError::Unverified(Violation::Border { part: *k }));
        }
    }

    let grown: Vec<Region> = placed
        .par_iter()
        .map(|(_, outline)| outline.offset(sheet.spacing, ARC))
        .collect();
    let boxes: Vec<(DVec2, DVec2)> = placed
        .iter()
        .map(|(_, outline)| outline.bounds().unwrap_or_default())
        .collect();
    let pairs = near(&boxes, sheet.spacing);
    let met: Vec<Result<bool, RegionError>> = pairs
        .par_iter()
        .map(|&(a, b)| {
            let (Some(grown), Some((_, other))) = (grown.get(a), placed.get(b)) else {
                return Ok(false);
            };
            Ok(!grown.intersection(other)?.is_empty())
        })
        .collect();
    for (&(a, b), met) in pairs.iter().zip(met) {
        if met? {
            let part = |i: usize| placed.get(i).map_or(i, |(k, _)| *k);
            return Err(ArrangeError::Unverified(Violation::Spacing {
                a: part(a),
                b: part(b),
            }));
        }
    }
    Ok(())
}

/// Every two of `boxes` no further apart than `spacing` along X and along Y,
/// each pair the earlier first, in order: the only ones whose outlines can
/// be nearer each other than that.
fn near(boxes: &[(DVec2, DVec2)], spacing: f64) -> Vec<(usize, usize)> {
    let mut by_left: Vec<usize> = (0..boxes.len()).collect();
    let left = |k: usize| boxes.get(k).map_or(0.0, |b| b.0.x);
    by_left.sort_by(|&a, &b| left(a).total_cmp(&left(b)));
    let mut pairs = Vec::new();
    for (n, &a) in by_left.iter().enumerate() {
        let Some(&(lo_a, hi_a)) = boxes.get(a) else {
            continue;
        };
        for &b in by_left.iter().skip(n + 1) {
            let Some(&(lo_b, hi_b)) = boxes.get(b) else {
                continue;
            };
            // Sorted by their left sides: every box after this one is further.
            if lo_b.x - hi_a.x > spacing {
                break;
            }
            if lo_b.y - hi_a.y <= spacing && lo_a.y - hi_b.y <= spacing {
                pairs.push((a.min(b), a.max(b)));
            }
        }
    }
    pairs.sort_unstable();
    pairs
}
