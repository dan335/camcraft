//! Adapting engine types to the simulator's.
//!
//! `camcraft-sim` sits below this crate and cannot name [`Toolpath`] or
//! [`ToolProfile`] without creating a cycle, so it defines the minimum it needs and
//! this module supplies it. The direction is deliberate: the simulator stays a
//! geometry service, and the CAM vocabulary stays here.

use camcraft_sim::stock::{MoveClass, SimMove, SweptTool};

use crate::allowance::EffectiveTool;
use crate::tool::ToolProfile;
#[cfg(test)]
use crate::tool::{Ball, Bullnose, Flat};
use crate::toolpath::{Move, MoveRole, Toolpath};

/// Reduce a toolpath to what the simulator needs.
///
/// A rapid is the only class that must not touch material. Everything else -- cuts,
/// plunges, ramps, leads, retracts -- is legitimately in the work at some point, and
/// an unresolved link is treated as a rapid so that simulating an unlinked path
/// reports the traverses nobody planned rather than quietly cutting them.
pub fn sim_moves(path: &Toolpath) -> Vec<SimMove> {
    sim_moves_of(path.moves())
}

/// [`sim_moves`], for some of a path's moves.
pub fn sim_moves_of(moves: &[Move]) -> Vec<SimMove> {
    moves
        .iter()
        .map(|m| SimMove {
            to: m.to,
            class: match m.role {
                MoveRole::Rapid | MoveRole::Link => MoveClass::Rapid,
                _ => MoveClass::MayCut,
            },
        })
        .collect()
}

#[cfg(test)]
macro_rules! swept {
    ($t:ty) => {
        impl SweptTool for $t {
            fn max_radius(&self) -> f64 {
                ToolProfile::max_radius(self)
            }
            fn height_at(&self, d: f64) -> Option<f64> {
                ToolProfile::height_at(self, d)
            }
        }
    };
}

/// The material is cut by the real tool: the grown one only finds where its tip
/// goes, and the stock it leaves stays in the material.
impl SweptTool for EffectiveTool {
    fn max_radius(&self) -> f64 {
        ToolProfile::max_radius(self.base())
    }
    fn height_at(&self, d: f64) -> Option<f64> {
        ToolProfile::height_at(self.base(), d)
    }
}

// A run only ever sweeps the effective tool; the bare cutters sweep in tests.
#[cfg(test)]
swept!(Flat);
#[cfg(test)]
swept!(Ball);
#[cfg(test)]
swept!(Bullnose);

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests {
    use super::*;
    use glam::dvec3;

    #[test]
    fn roles_map_to_the_right_class() {
        let mut t = Toolpath::new();
        t.rapid_to(dvec3(0.0, 0.0, 10.0)).unwrap();
        t.feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Plunge, 300.0)
            .unwrap();
        t.feed_to(dvec3(5.0, 0.0, 0.0), MoveRole::Cut, 900.0)
            .unwrap();
        t.feed_to(dvec3(5.0, 0.0, 10.0), MoveRole::Retract, 900.0)
            .unwrap();

        let classes: Vec<MoveClass> = sim_moves(&t).iter().map(|m| m.class).collect();
        assert_eq!(
            classes,
            vec![
                MoveClass::Rapid,
                MoveClass::MayCut,
                MoveClass::MayCut,
                MoveClass::MayCut
            ]
        );
    }

    /// An unresolved link is an unplanned traverse. Simulating it as a cut would
    /// hide exactly the thing the linker exists to prevent.
    #[test]
    fn an_unresolved_link_simulates_as_a_rapid() {
        let mut t = Toolpath::new();
        t.feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Link, 100.0)
            .unwrap();
        assert_eq!(sim_moves(&t).first().unwrap().class, MoveClass::Rapid);
    }

    /// The simulator cuts with the tool that was machined with, not the one grown
    /// by its stock to leave, or the stock left on a wall would be swept away.
    #[test]
    fn the_simulator_sweeps_the_real_tool() {
        use crate::allowance::Allowance;
        let bare = Ball::new(3.0).unwrap();
        let with_stock = EffectiveTool::new(&bare, Allowance::isotropic(0.5).unwrap()).unwrap();
        assert_eq!(SweptTool::max_radius(&with_stock), 3.0);
        for d in [0.0, 1.0, 2.5, 3.0] {
            assert_eq!(
                SweptTool::height_at(&with_stock, d),
                SweptTool::height_at(&bare, d)
            );
        }
    }
}
