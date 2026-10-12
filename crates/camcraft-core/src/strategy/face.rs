//! Facing: the stock's top cut flat, straight passes across it side by side,
//! level by level down to the bottom. Fusion's Face.
//!
//! # From the stock, not the model
//!
//! What is faced is the stock's footprint, grown by the stock offset, and nothing
//! of the model: every pass is a straight line across it, so nothing here needs
//! the part's geometry. Its bottom is the model's top unless another is asked
//! for; one asked for lower cuts into the part, and the verifier says so.
//!
//! # Coverage
//!
//! The outermost passes put the tool's centre on the faced area's edges, so the
//! tool reaches a radius past them, and every pass runs on until the tool is clear
//! of the area, then on by the pass extension: nothing of the area is left under
//! the tool's reach, and the tool enters and leaves off the stock. The passes are
//! spaced evenly, no more than the stepover apart.
//!
//! # Direction
//!
//! Both ways by default, as Fusion's: back and forth, stepping over off the stock
//! at each end without lifting. Climb or conventional cut every pass the same way,
//! and the linker brings the tool back between them. Each level starts from the
//! side the one above finished on.

use crate::job::FaceDirection;
use crate::path::toolpath::{MoveRole, Toolpath};
use crate::strategy::planar::{PlanarReport, levels};
use crate::strategy::{StrategyError, check};
use glam::{DVec2, DVec3, dvec2, dvec3};

/// How to face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceParams {
    /// The area faced, lower and upper corner: the stock's footprint and its
    /// offset.
    pub area: (DVec2, DVec2),
    /// Pass direction in radians; zero runs along +X.
    pub angle: f64,
    /// Distance between passes, in millimetres.
    pub stepover: f64,
    /// How far each pass runs on once the tool is clear of the area.
    pub extension: f64,
    /// The tool's radius.
    pub radius: f64,
    /// The levels cut, from the top down; the last is the bottom.
    pub levels: LevelsAsked,
    /// Which way the passes cut.
    pub direction: FaceDirection,
    /// Whether climbing has the material on the tool's right: a clockwise
    /// spindle.
    pub clockwise: bool,
    /// Cutting feed, in mm/min.
    pub feed: f64,
}

/// The levels facing cuts at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelsAsked {
    /// Highest level cut from.
    pub top: f64,
    /// The faced level.
    pub bottom: f64,
    /// Deepest a level may be below the one above: Fusion's multiple depths.
    /// `None` cuts the bottom alone.
    pub stepdown: Option<f64>,
}

/// The passes facing `params.area`, at each level.
pub fn face(params: &FaceParams) -> Result<(Toolpath, PlanarReport), StrategyError> {
    check("stepover", params.stepover)?;
    check("feed", params.feed)?;
    check("tool radius", params.radius)?;
    if let Some(d) = params.levels.stepdown {
        check("stepdown", d)?;
    }
    // Each finite, and no less than the least it may be.
    for (name, value, least) in [
        ("pass extension", params.extension, 0.0),
        ("pass angle", params.angle, f64::NEG_INFINITY),
        ("top", params.levels.top, f64::NEG_INFINITY),
    ] {
        if !(value.is_finite() && value >= least) {
            return Err(StrategyError::BadParameter { name, value });
        }
    }

    let mut report = PlanarReport::default();
    let asked = params.levels;
    let zs = levels(asked.top, asked.bottom, asked.stepdown);
    let lines = passes(params);
    if zs.is_empty() {
        report.too_shallow = 1;
        return Ok((Toolpath::new(), report));
    }
    if lines.is_empty() {
        return Ok((Toolpath::new(), report));
    }
    report.areas = 1;
    report.rings = lines.len() as u64;
    report.levels = zs.len() as u64;

    let mut path = Toolpath::new();
    // Both ways turns at the end of every pass, from one level into the next
    // too, so each level starts where the one above finished.
    let mut turn = 0usize;
    for (k, &z) in zs.iter().enumerate() {
        let ascending = k % 2 == 0;
        let order: Vec<usize> = if ascending {
            (0..lines.len()).collect()
        } else {
            (0..lines.len()).rev().collect()
        };
        for (n, &i) in order.iter().enumerate() {
            let Some(&(a, b)) = lines.get(i) else {
                continue;
            };
            // Each pass is `a` to `b` along +u. Going +u, the tool's right is -v;
            // the side not yet cut is the one the next passes are toward, +v
            // while they ascend. Climbing with a clockwise spindle has it on the
            // right.
            let forward = match params.direction {
                FaceDirection::BothWays => turn.is_multiple_of(2),
                FaceDirection::Climb => params.clockwise != ascending,
                FaceDirection::Conventional => params.clockwise == ascending,
            };
            turn += 1;
            let (from, to) = if forward { (a, b) } else { (b, a) };
            // Both ways steps over at the end without lifting, off the stock;
            // otherwise, and onto each level, the linker brings the tool.
            let role = if params.direction == FaceDirection::BothWays && n > 0 {
                MoveRole::Cut
            } else {
                MoveRole::Link
            };
            path.feed_to(at(from, z), role, params.feed)?;
            path.feed_to(at(to, z), MoveRole::Cut, params.feed)?;
            report.passes += 1;
        }
    }
    Ok((path, report))
}

fn at(p: DVec2, z: f64) -> DVec3 {
    dvec3(p.x, p.y, z)
}

/// The pass direction and the one across it, as unit vectors.
fn frame(angle: f64) -> (DVec2, DVec2) {
    let u = dvec2(angle.cos(), angle.sin());
    (u, u.perp())
}

/// Each pass's two ends, in the setup's frame, ordered across from -v to +v and
/// each running +u: spaced evenly from one edge of the area to the other, and
/// running across the area grown by the tool's radius, then the extension.
fn passes(params: &FaceParams) -> Vec<(DVec2, DVec2)> {
    let (lo, hi) = params.area;
    if !(hi.x > lo.x && hi.y > lo.y) {
        return Vec::new();
    }
    let (u, v) = frame(params.angle);
    let corners = |lo: DVec2, hi: DVec2| {
        [
            dvec2(lo.x, lo.y),
            dvec2(hi.x, lo.y),
            dvec2(hi.x, hi.y),
            dvec2(lo.x, hi.y),
        ]
    };
    let across: Vec<f64> = corners(lo, hi).iter().map(|c| c.dot(v)).collect();
    let (v0, v1) = across
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &c| {
            (a.min(c), b.max(c))
        });
    let span = v1 - v0;
    // Bounded, so a tiny stepover cannot run away.
    let steps = (span / params.stepover - 1e-9).ceil().clamp(0.0, 100_000.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let steps = steps as u32;
    let r = params.radius;
    let grown = corners(lo - DVec2::splat(r), hi + DVec2::splat(r));
    (0..=steps)
        .filter_map(|i| {
            let c = if steps == 0 {
                v0 + span / 2.0
            } else {
                v0 + span * f64::from(i) / f64::from(steps)
            };
            let (a, b) = crossing(&grown, u, v, c)?;
            let e = params.extension;
            Some((u * (a - e) + v * c, u * (b + e) + v * c))
        })
        .collect()
}

/// Where the line `p·v = c` crosses the convex polygon `ring`, as the least and
/// greatest `p·u` on it.
fn crossing(ring: &[DVec2], u: DVec2, v: DVec2, c: f64) -> Option<(f64, f64)> {
    let mut hits = Vec::new();
    for (i, &p) in ring.iter().enumerate() {
        let q = *ring.get((i + 1) % ring.len())?;
        let (dp, dq) = (p.dot(v) - c, q.dot(v) - c);
        if (dp - dq).abs() < 1e-12 {
            // Along the line, or beside it: its ends are all it can add.
            if dp.abs() < 1e-12 {
                hits.extend([p.dot(u), q.dot(u)]);
            }
        } else if (dp <= 0.0) != (dq <= 0.0) || dp.abs() < 1e-12 || dq.abs() < 1e-12 {
            hits.push((p + (q - p) * (dp / (dp - dq))).dot(u));
        }
    }
    let lo = hits.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = hits.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    (lo <= hi).then_some((lo, hi))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::indexing_slicing)]
mod tests;
