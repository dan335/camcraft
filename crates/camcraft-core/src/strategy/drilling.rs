//! Drilling: holes from picked round edges, and a plunge down each.
//!
//! A hole is picked by its rim, the round edge where it meets the top of the part.
//! Pick its bottom rim as well and it is drilled to there; otherwise it is drilled
//! to the operation's bottom height. Each hole is a pass of its own: a link to just
//! above it, plunges down (in pecks, when asked, backing out to clear chips between
//! them), and a retract. So the linker, simulator and checks treat a drilled hole
//! as they treat anything else, and a post that knows canned cycles can write each
//! hole as one line instead ([`Hole`] carries what it needs).

use glam::{DVec2, DVec3, dvec3};

use crate::path::toolpath::{MoveRole, Toolpath};
use crate::setup::features::Chain;
use crate::strategy::nesting::nearest;
use crate::strategy::{StrategyError, check};

/// How far above a hole's top a drill starts feeding, and backs out to between
/// pecks: the R plane of a canned cycle.
pub const RETRACT_GAP: f64 = 2.0;

/// One hole to drill, in the setup's frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hole {
    /// Where its axis is.
    pub at: DVec2,
    /// The height it starts at: its rim.
    pub top: f64,
    /// The height the drill's tip goes down to.
    pub bottom: f64,
    /// Its radius, from the picked rim.
    pub radius: f64,
}

impl Hole {
    /// The height a drill feeds down from and backs out to: the R plane.
    pub fn retract(&self) -> f64 {
        self.top + RETRACT_GAP
    }
}

/// How each hole is drilled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrillParams {
    /// Depth of each peck; the whole depth at once when `None`.
    pub peck: Option<f64>,
    /// Feed down into the hole, in mm/min.
    pub feed: f64,
    /// Feed back out, in mm/min.
    pub retract_feed: f64,
}

/// Why picked edges are not holes.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum HoleError {
    /// Nothing was picked.
    #[error("pick the round edge at the top of each hole to drill")]
    NothingPicked,
    /// A picked edge is not round.
    #[error("edge {edge} of '{model}' is not round, so it is not a hole's rim")]
    NotRound {
        /// The model.
        model: String,
        /// Its number there.
        edge: u32,
    },
    /// A picked rim is not level, so its hole does not run straight down.
    #[error("the hole at edge {edge} of '{model}' is not vertical in this setup")]
    NotVertical {
        /// The model.
        model: String,
        /// Its number there.
        edge: u32,
    },
    /// A hole would be drilled upward, or not at all.
    #[error("the hole at ({x:.3}, {y:.3}) has its bottom at or above its top")]
    NoDepth {
        /// Where.
        x: f64,
        /// Where.
        y: f64,
    },
}

/// The holes picked rims make, in the order first picked: one per axis, from its
/// highest rim to its lowest, or to `bottom` when the rims picked of it are all
/// at one height, as both edges of a counterbore's floor are.
///
/// `circle` gives each chain's circle, centre and radius, in the setup's frame;
/// `tol` is how close two rims' axes must be to be one hole's, and two rims'
/// heights to be one.
pub fn holes(
    chains: &[Chain],
    circle: impl Fn(&Chain) -> Option<(DVec3, f64)>,
    bottom: f64,
    tol: f64,
) -> Result<Vec<Hole>, HoleError> {
    if chains.is_empty() {
        return Err(HoleError::NothingPicked);
    }
    // Each axis, with the heights of its rims and the widest radius.
    let mut axes: Vec<(DVec2, Vec<f64>, f64)> = Vec::new();
    for chain in chains {
        let Some((centre, radius)) = circle(chain) else {
            return Err(HoleError::NotRound {
                model: chain.model.clone(),
                edge: chain.edge,
            });
        };
        let level = chain
            .points
            .iter()
            .all(|p| (p.z - centre.z).abs() <= tol.max(1e-6));
        if !level {
            return Err(HoleError::NotVertical {
                model: chain.model.clone(),
                edge: chain.edge,
            });
        }
        let at = centre.truncate();
        match axes.iter_mut().find(|(a, _, _)| a.distance(at) <= tol) {
            Some((_, zs, r)) => {
                zs.push(centre.z);
                *r = r.max(radius);
            }
            None => axes.push((at, vec![centre.z], radius)),
        }
    }
    axes.into_iter()
        .map(|(at, zs, radius)| {
            let top = zs.iter().copied().fold(f64::MIN, f64::max);
            let low = zs.iter().copied().fold(f64::MAX, f64::min);
            let bottom = if top - low > tol.max(1e-6) {
                low
            } else {
                bottom
            };
            if bottom >= top {
                return Err(HoleError::NoDepth { x: at.x, y: at.y });
            }
            Ok(Hole {
                at,
                top,
                bottom,
                radius,
            })
        })
        .collect()
}

/// `holes` in drilling order: the first, then always the nearest not yet drilled.
/// Ties go to the earlier, so the order does not depend on anything but the holes.
pub fn nearest_first(holes: &[Hole]) -> Vec<Hole> {
    let mut left: Vec<Hole> = holes.to_vec();
    let mut out = Vec::with_capacity(left.len());
    let mut at = match left.first() {
        Some(h) => h.at,
        None => return out,
    };
    while !left.is_empty() {
        let h = left.remove(nearest(&left, |h| h.at.distance_squared(at)));
        at = h.at;
        out.push(h);
    }
    out
}

/// What a drilling operation did, for a post that writes canned cycles.
#[derive(Debug, Clone, PartialEq)]
pub struct DrillReport {
    /// The holes, in the order drilled.
    pub holes: Vec<Hole>,
    /// Depth of each peck, if it pecked.
    pub peck: Option<f64>,
    /// Seconds to pause at the bottom, if asked.
    pub dwell: Option<f64>,
    /// Feed down into each hole, in mm/min.
    pub feed: f64,
}

/// A plunge down each hole, in order, each a pass the linker joins.
pub fn drill(holes: &[Hole], params: &DrillParams) -> Result<Toolpath, StrategyError> {
    for (name, v) in [("feed", params.feed), ("retract feed", params.retract_feed)] {
        check(name, v)?;
    }
    if let Some(p) = params.peck {
        check("peck", p)?;
    }
    let mut path = Toolpath::new();
    for h in holes {
        let (x, y) = (h.at.x, h.at.y);
        let r = h.retract();
        path.feed_to(dvec3(x, y, r), MoveRole::Link, params.feed)?;
        let mut depth = h.top;
        loop {
            depth = match params.peck {
                Some(p) => (depth - p).max(h.bottom),
                None => h.bottom,
            };
            path.feed_to(dvec3(x, y, depth), MoveRole::Plunge, params.feed)?;
            path.feed_to(dvec3(x, y, r), MoveRole::Retract, params.retract_feed)?;
            if depth <= h.bottom {
                break;
            }
            // Back down to just above the last peck, and on from there.
            path.feed_to(
                dvec3(x, y, (depth + 0.5).min(r)),
                MoveRole::Plunge,
                params.feed,
            )?;
        }
    }
    Ok(path)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)]
mod tests;
