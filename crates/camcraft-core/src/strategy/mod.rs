//! The strategies, each a pure function from the part and stock to a
//! [`Toolpath`](crate::path::toolpath::Toolpath), and what they share: the error
//! one fails with, the tests their parameters must pass, the levels they cut at,
//! and how far in a region goes.

// Finishing.
pub mod flat;
pub mod morph;
pub mod parallel;
pub mod patterns;
pub mod projected;
pub mod scallop;
pub mod waterline;

// Clearing.
pub mod adaptive;
pub mod face;
pub mod planar;
pub mod pocket_clearing;

// Holes.
pub mod drilling;

// What they share.
mod field;
pub(crate) mod nesting;
pub mod reach;
pub(crate) mod slope;

use camcraft_geom::contour::Region;
use glam::DVec2;

use crate::is_positive;
use crate::path::toolpath::ToolpathError;

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
    /// Its run was stopped while it worked ([`crate::control`]).
    #[error(transparent)]
    Stopped(#[from] crate::control::Stopped),
}

impl From<camcraft_geom::contour::RegionError> for StrategyError {
    fn from(e: camcraft_geom::contour::RegionError) -> Self {
        StrategyError::Region(e.to_string())
    }
}

impl From<crate::strategy::reach::ReachError> for StrategyError {
    fn from(e: crate::strategy::reach::ReachError) -> Self {
        StrategyError::Region(e.to_string())
    }
}

/// Levels closer than this are one, in millimetres.
pub(crate) const SAME_LEVEL: f64 = 1.0e-6;

/// `levels` in order, highest first when `down`, those within [`SAME_LEVEL`]
/// of the one before left out.
pub(crate) fn sort_levels(levels: &mut Vec<f64>, down: bool) {
    if down {
        levels.sort_by(|a, b| b.total_cmp(a));
    } else {
        levels.sort_by(f64::total_cmp);
    }
    levels.dedup_by(|a, b| (*a - *b).abs() < SAME_LEVEL);
}

/// `heights`, lowest first, each run of them no more than `within` above the
/// one before made one: the highest of the run, so what is at it is never cut
/// into.
pub(crate) fn merged_highest(mut heights: Vec<f64>, within: f64) -> Vec<f64> {
    heights.sort_by(f64::total_cmp);
    let mut out: Vec<f64> = Vec::new();
    for z in heights {
        match out.last_mut() {
            Some(last) if z - *last <= within => *last = z,
            _ => out.push(z),
        }
    }
    out
}

/// Stock standing less than this above a level is not material to cut there, in
/// millimetres. Far below any tolerance, far above the grid's f32 storage error.
pub(crate) const MATERIAL_THICKNESS: f64 = 1.0e-3;

/// `area` with its edge counted in it: grown by `tolerance`, which an area is
/// drawn to, its arcs as chords that far inside them. A pass that runs along
/// the edge, as a ring round a wall at the boundary does, or a first pass on
/// the edge itself, is then in all along, not cut into bits wherever the
/// edge's chords fall either side of it.
pub(crate) fn with_edge(area: &Region, tolerance: f64) -> Region {
    area.offset(tolerance, 0.1 * tolerance)
}

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
        let mut z = (top - f64::from(i) * stepdown).max(bottom);
        // A rounding error above the bottom is the bottom: a stepdown that
        // divides the depth exactly in millimetres need not in binary.
        if z - bottom < SAME_LEVEL {
            z = bottom;
        }
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
        sort_levels(flats, false);
    }
    slabs
}

/// `slabs`, as [`slabs`] makes them from the top down to `top`, with Fusion's
/// fine stepdown in each: levels `fine` apart up from the slab's level and from
/// each flat in it, to below the level above, or `top`. Each is cut, as a flat
/// level is, after the slab's level and the ones below it in the slab, over what
/// they left: on a slope, the terraces they leave cut down to steps `fine`
/// tall. Bounded, as [`slabs`] is.
pub(crate) fn with_fine_levels(
    mut slabs: Vec<(f64, Vec<f64>)>,
    top: f64,
    fine: f64,
) -> Vec<(f64, Vec<f64>)> {
    if !is_positive(fine) {
        return slabs;
    }
    let mut above = top;
    for (z, fills) in &mut slabs {
        let bases: Vec<f64> = std::iter::once(*z).chain(fills.iter().copied()).collect();
        for base in bases {
            for k in 1..=10_000_u32 {
                let f = base + f64::from(k) * fine;
                if f >= above - SAME_LEVEL {
                    break;
                }
                fills.push(f);
            }
        }
        sort_levels(fills, false);
        above = *z;
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

/// `value` if it is usable as a length that may be nothing: finite and not
/// negative.
pub(crate) fn at_least_zero(name: &'static str, value: f64) -> Result<(), StrategyError> {
    (value.is_finite() && value >= 0.0)
        .then_some(())
        .ok_or(StrategyError::BadParameter { name, value })
}

/// The levels a strategy cuts between, if usable: both finite, `top` above
/// `bottom`.
pub(crate) fn check_levels(top: f64, bottom: f64) -> Result<(), StrategyError> {
    if top <= bottom || !top.is_finite() || !bottom.is_finite() {
        return Err(StrategyError::BadParameter {
            name: "top",
            value: top,
        });
    }
    Ok(())
}

/// What is left of `region` offset in as far as leaves anything of it, between
/// `inner` and `outer`, and how far in that is: found by halving, until the two
/// are closer than `within`. `None` if every offset tried leaves nothing.
pub(crate) fn innermost(
    region: &Region,
    (mut inner, mut outer): (f64, f64),
    arc: f64,
    within: f64,
) -> Option<(Region, f64)> {
    let mut best = None;
    for _ in 0..24 {
        let mid = (inner + outer) * 0.5;
        let shrunk = region.offset(-mid, arc);
        if shrunk.is_empty() {
            outer = mid;
        } else {
            inner = mid;
            best = Some((shrunk, mid));
        }
        if outer - inner < within {
            break;
        }
    }
    best
}

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Fine levels go up from each level and from each flat in its slab, a fine
    /// stepdown apart, to below the level above, or the top; one that falls on
    /// a flat is that flat. Each slab's are lowest first, to be cut in order.
    #[test]
    fn fine_levels_fill_each_slab_up_to_the_level_above() {
        let s = with_fine_levels(slabs(10.0, 0.0, 4.0, &[5.0]), 10.0, 1.5);
        assert_eq!(
            s,
            [
                (6.0, vec![7.5, 9.0]),
                (2.0, vec![3.5, 5.0]),
                (0.0, vec![1.5]),
            ]
        );
        // Not a number, or no step at all: nothing added.
        let plain = slabs(10.0, 0.0, 4.0, &[5.0]);
        for bad in [0.0, -1.0, f64::NAN] {
            assert_eq!(with_fine_levels(plain.clone(), 10.0, bad), plain);
        }
    }

    /// 3/4" stock at 1/8" and at 1/4" a level: the depth is a whole number of
    /// steps in millimetres, but not in binary, where the last step falls a
    /// rounding error short of the bottom. It is the bottom, cut once.
    #[test]
    fn a_step_that_divides_the_depth_ends_on_the_bottom_once() {
        for (step, count) in [(3.175, 6), (6.35, 3)] {
            let zs: Vec<f64> = slabs(19.05, 0.0, step, &[]).iter().map(|s| s.0).collect();
            assert_eq!(zs.len(), count, "{step}: {zs:?}");
            assert_eq!(zs.last(), Some(&0.0), "{step}: {zs:?}");
            for pair in zs.windows(2) {
                let [a, b] = pair else { continue };
                assert!(a - b > step * 0.5, "{step}: {zs:?}");
            }
        }
    }

    #[test]
    fn levels_end_at_the_bottom_and_take_in_the_flats() {
        let s = slabs(10.0, 0.5, 4.0, &[5.5, 2.0, 7.0, 6.0]);
        let zs: Vec<f64> = s.iter().map(|s| s.0).collect();
        assert_eq!(zs, vec![6.0, 2.0, 0.5]);
        // Each flat in the slab of the level just below it; one on a level is
        // that level.
        assert_eq!(s[0].1, vec![7.0]);
        assert_eq!(s[1].1, vec![5.5]);
        assert!(s[2].1.is_empty());
    }
}
