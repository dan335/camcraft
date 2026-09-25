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

use crate::features::Chain;
use crate::strategy::{StrategyError, check};
use crate::toolpath::{MoveRole, Toolpath};

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
/// highest rim to its lowest, or to `bottom` when only one rim of it was picked.
///
/// `circle` gives each chain's circle, centre and radius, in the setup's frame;
/// `tol` is how close two rims' axes must be to be one hole's.
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
            let bottom = if zs.len() > 1 { low } else { bottom };
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
        let mut best = 0;
        let mut best_d = f64::MAX;
        for (i, h) in left.iter().enumerate() {
            let d = h.at.distance_squared(at);
            if d < best_d {
                best = i;
                best_d = d;
            }
        }
        let h = left.remove(best);
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
    for (name, v) in [("feed", params.feed), ("retract_feed", params.retract_feed)] {
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
mod tests {
    use super::*;
    use glam::dvec2;

    fn rim(edge: u32, centre: DVec3, radius: f64) -> Chain {
        let points = (0..16)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / 16.0;
                centre + dvec3(radius * a.cos(), radius * a.sin(), 0.0)
            })
            .collect();
        Chain {
            model: "plate".into(),
            edge,
            points,
            closed: true,
            ends: None,
        }
    }

    fn fit(c: &Chain) -> Option<(DVec3, f64)> {
        let n = c.points.len() as f64;
        let centre = c.points.iter().copied().sum::<DVec3>() / n;
        Some((centre, c.points[0].distance(centre)))
    }

    #[test]
    fn a_rim_alone_is_drilled_to_the_bottom_asked_for() {
        let got = holes(&[rim(1, dvec3(5.0, 5.0, 10.0), 3.0)], fit, -1.0, 0.01).unwrap();
        assert_eq!(got.len(), 1);
        assert!(got[0].at.distance(dvec2(5.0, 5.0)) < 1e-9);
        assert_eq!((got[0].top, got[0].bottom), (10.0, -1.0));
        assert!((got[0].radius - 3.0).abs() < 1e-9);
    }

    /// Both rims of one hole make one hole, from the top one to the bottom one.
    #[test]
    fn both_rims_of_a_hole_set_its_depth() {
        let got = holes(
            &[
                rim(1, dvec3(0.0, 0.0, 4.0), 2.0),
                rim(2, dvec3(20.0, 0.0, 10.0), 2.0),
                rim(3, dvec3(0.0, 0.0, 10.0), 2.0),
            ],
            fit,
            -5.0,
            0.01,
        )
        .unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!((got[0].top, got[0].bottom), (10.0, 4.0));
        assert_eq!((got[1].top, got[1].bottom), (10.0, -5.0));
    }

    #[test]
    fn what_is_not_a_hole_is_said() {
        assert_eq!(holes(&[], fit, 0.0, 0.01), Err(HoleError::NothingPicked));
        let square = rim(4, dvec3(0.0, 0.0, 5.0), 2.0);
        assert!(matches!(
            holes(std::slice::from_ref(&square), |_| None, 0.0, 0.01),
            Err(HoleError::NotRound { edge: 4, .. })
        ));
        let mut tilted = square;
        tilted.points[3].z += 1.0;
        assert!(matches!(
            holes(&[tilted], fit, 0.0, 0.01),
            Err(HoleError::NotVertical { .. })
        ));
        assert!(matches!(
            holes(&[rim(5, dvec3(0.0, 0.0, 5.0), 2.0)], fit, 6.0, 0.01),
            Err(HoleError::NoDepth { .. })
        ));
    }

    /// Pecks go down by their depth each time, back out to the R plane between,
    /// and come back to just above where the last one stopped.
    #[test]
    fn pecks_back_out_between() {
        let hole = Hole {
            at: dvec2(1.0, 2.0),
            top: 10.0,
            bottom: 4.0,
            radius: 3.0,
        };
        let params = DrillParams {
            peck: Some(2.5),
            feed: 200.0,
            retract_feed: 1000.0,
        };
        let path = drill(&[hole], &params).unwrap();
        let downs: Vec<f64> = path
            .moves()
            .windows(2)
            .filter(|w| w[1].role == MoveRole::Retract)
            .map(|w| w[0].to.z)
            .collect();
        assert_eq!(downs, [7.5, 5.0, 4.0]);
        let lowest = path.moves().iter().map(|m| m.to.z).fold(f64::MAX, f64::min);
        assert_eq!(lowest, 4.0);
        assert_eq!(path.end().unwrap().z, 12.0, "ends at the R plane");
        assert!(path.moves().iter().all(|m| m.role != MoveRole::Cut));

        let once = drill(
            &[hole],
            &DrillParams {
                peck: None,
                ..params
            },
        )
        .unwrap();
        assert_eq!(once.len(), 3, "link, plunge, retract");
    }

    #[test]
    fn holes_are_drilled_nearest_first() {
        let at = |x: f64| Hole {
            at: dvec2(x, 0.0),
            top: 1.0,
            bottom: 0.0,
            radius: 1.0,
        };
        let order: Vec<f64> = nearest_first(&[at(0.0), at(10.0), at(3.0), at(4.0)])
            .iter()
            .map(|h| h.at.x)
            .collect();
        assert_eq!(order, [0.0, 3.0, 4.0, 10.0]);
        assert!(nearest_first(&[]).is_empty());
    }
}
