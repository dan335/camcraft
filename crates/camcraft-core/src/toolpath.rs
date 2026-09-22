//! Toolpaths: what a strategy produces, and what every later transform operates on.
//!
//! A strategy never emits machine instructions. It emits one of these, and arc
//! fitting, tabs, lead in and out, feed optimisation and cutter compensation are all
//! `Toolpath -> Toolpath` transforms layered on top. Only at the very end does
//! `camcraft-post` turn one into an instruction stream. That ordering is enforced by
//! the crate graph — this crate cannot see `camcraft-post`, so it could not emit
//! G-code if it tried.
//!
//! # Roles carry intent
//!
//! Every move records *why* it exists, not just where it goes. Reconstructing that
//! later would be guesswork, and three things depend on it:
//!
//! - per-role feeds, which every real CAM system has
//! - feed optimisation, which needs to know what is actually cutting
//! - the safety check that **no rapid may touch stock** — the defect Pngcam
//!   documents, and free to test once the role is recorded
//!
//! # Links are holes, deliberately
//!
//! A strategy knows where it wants to cut, not how to travel between cuts. It marks
//! those connections [`MoveRole::Link`], meaning "get here, somehow". The linker
//! replaces them with real retracts, rapids and plunges. A finished program contains
//! no links, and that is a checkable property rather than a convention.

use glam::DVec3;

/// Why a move exists.
///
/// Ordering is not meaningful; this is a tag, not a scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MoveRole {
    /// Non-cutting traverse at maximum speed. Must never touch stock.
    Rapid,
    /// Removing material at cutting feed.
    Cut,
    /// Approach to or departure from a cut, tangential or radial.
    Lead,
    /// Angled descent into material.
    Ramp,
    /// Straight-down descent into material.
    Plunge,
    /// Withdrawal out of material.
    Retract,
    /// An unplanned connection between cuts. The linker must resolve these; none may
    /// survive into a finished program.
    Link,
}

impl MoveRole {
    /// Whether this move is expected to be in contact with material.
    ///
    /// Used by the simulator to decide which moves may legitimately remove stock.
    pub fn cuts(&self) -> bool {
        matches!(self, Self::Cut | Self::Ramp | Self::Plunge | Self::Lead)
    }

    /// Whether the linker still has to resolve this move.
    pub fn is_unplanned(&self) -> bool {
        matches!(self, Self::Link)
    }
}

/// The shape of a move.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MoveKind {
    /// A straight line to the target.
    Linear,
    /// A circular arc to the target about `centre`.
    ///
    /// Reserved: nothing emits arcs in 0.1. It exists so that arc fitting can be
    /// added as a `Toolpath -> Toolpath` transform without changing this type, which
    /// is the whole reason strategies emit toolpaths rather than instructions.
    Arc {
        /// Arc centre.
        centre: DVec3,
        /// Whether the arc turns clockwise seen from +Z.
        clockwise: bool,
    },
}

/// One move of the tool.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Move {
    /// Where the tool ends up. This is the tool *tip*, in setup space.
    pub to: DVec3,
    /// Straight or arc.
    pub kind: MoveKind,
    /// Why this move exists.
    pub role: MoveRole,
    /// Feed rate in mm/min, or `None` for a rapid.
    pub feed: Option<f64>,
}

/// A move that could not be added.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ToolpathError {
    /// A target position was NaN or infinite.
    #[error("move target is not finite: {0:?}")]
    NonFiniteTarget(DVec3),
    /// A feed rate was zero, negative, or not finite.
    #[error("feed rate must be finite and greater than zero, got {0}")]
    BadFeed(f64),
    /// A rapid was given a feed rate, or a feeding move was not.
    #[error("{role:?} moves must {} a feed rate", if *.expected { "have" } else { "not have" })]
    FeedMismatch {
        /// The role in question.
        role: MoveRole,
        /// Whether a feed was expected.
        expected: bool,
    },
}

/// An ordered sequence of moves.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Toolpath {
    moves: Vec<Move>,
}

impl Toolpath {
    /// An empty path.
    pub fn new() -> Self {
        Self::default()
    }

    /// The moves, in order.
    pub fn moves(&self) -> &[Move] {
        &self.moves
    }

    /// How many moves the path has.
    pub fn len(&self) -> usize {
        self.moves.len()
    }

    /// Whether the path has no moves.
    pub fn is_empty(&self) -> bool {
        self.moves.is_empty()
    }

    /// Where the tool ends up, or `None` for an empty path.
    pub fn end(&self) -> Option<DVec3> {
        self.moves.last().map(|m| m.to)
    }

    /// Append a move, validating it.
    ///
    /// Rapids must carry no feed and everything else must carry one, so an
    /// unfeedable move cannot reach a post processor and become a silent rapid into
    /// the work.
    pub fn push(&mut self, m: Move) -> Result<(), ToolpathError> {
        if !m.to.is_finite() {
            return Err(ToolpathError::NonFiniteTarget(m.to));
        }
        if let MoveKind::Arc { centre, .. } = m.kind
            && !centre.is_finite()
        {
            return Err(ToolpathError::NonFiniteTarget(centre));
        }
        let wants_feed = m.role != MoveRole::Rapid;
        match (wants_feed, m.feed) {
            (true, Some(f)) if f.is_finite() && f > 0.0 => {}
            (true, Some(f)) => return Err(ToolpathError::BadFeed(f)),
            (false, None) => {}
            (expected, _) => {
                return Err(ToolpathError::FeedMismatch {
                    role: m.role,
                    expected,
                });
            }
        }
        self.moves.push(m);
        Ok(())
    }

    /// Append a straight rapid.
    pub fn rapid_to(&mut self, to: DVec3) -> Result<(), ToolpathError> {
        self.push(Move {
            to,
            kind: MoveKind::Linear,
            role: MoveRole::Rapid,
            feed: None,
        })
    }

    /// Append a straight feeding move.
    pub fn feed_to(&mut self, to: DVec3, role: MoveRole, feed: f64) -> Result<(), ToolpathError> {
        self.push(Move {
            to,
            kind: MoveKind::Linear,
            role,
            feed: Some(feed),
        })
    }

    /// Whether any move still needs the linker's attention.
    ///
    /// A finished program must answer `false`. This is the checkable form of "links
    /// are holes": nothing convention-based, just a property.
    pub fn has_unplanned_links(&self) -> bool {
        self.moves.iter().any(|m| m.role.is_unplanned())
    }

    /// Total length of moves whose role cuts, in millimetres.
    ///
    /// Arcs are measured by their chord in 0.1, since nothing emits them yet.
    pub fn cutting_length(&self, from: DVec3) -> f64 {
        let mut at = from;
        let mut total = 0.0;
        for m in &self.moves {
            if m.role.cuts() {
                total += (m.to - at).length();
            }
            at = m.to;
        }
        total
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests {
    use super::*;
    use glam::dvec3;

    #[test]
    fn rejects_non_finite_targets() {
        let mut t = Toolpath::new();
        assert!(matches!(
            t.rapid_to(dvec3(f64::NAN, 0.0, 0.0)),
            Err(ToolpathError::NonFiniteTarget(_))
        ));
        assert!(t.is_empty(), "a rejected move must not be stored");
    }

    /// A feeding move without a feed would reach the post and become a rapid into
    /// the work. The type refuses to hold one.
    #[test]
    fn feeding_moves_must_carry_a_feed() {
        let mut t = Toolpath::new();
        let bad = Move {
            to: dvec3(1.0, 0.0, 0.0),
            kind: MoveKind::Linear,
            role: MoveRole::Cut,
            feed: None,
        };
        assert_eq!(
            t.push(bad),
            Err(ToolpathError::FeedMismatch {
                role: MoveRole::Cut,
                expected: true
            })
        );
        assert!(matches!(
            t.feed_to(dvec3(1.0, 0.0, 0.0), MoveRole::Cut, 0.0),
            Err(ToolpathError::BadFeed(_))
        ));
        assert!(
            t.feed_to(dvec3(1.0, 0.0, 0.0), MoveRole::Cut, 500.0)
                .is_ok()
        );
    }

    #[test]
    fn rapids_must_not_carry_a_feed() {
        let mut t = Toolpath::new();
        let bad = Move {
            to: dvec3(1.0, 0.0, 0.0),
            kind: MoveKind::Linear,
            role: MoveRole::Rapid,
            feed: Some(500.0),
        };
        assert_eq!(
            t.push(bad),
            Err(ToolpathError::FeedMismatch {
                role: MoveRole::Rapid,
                expected: false
            })
        );
    }

    #[test]
    fn roles_classify_cutting_and_planning() {
        for r in [
            MoveRole::Cut,
            MoveRole::Ramp,
            MoveRole::Plunge,
            MoveRole::Lead,
        ] {
            assert!(r.cuts(), "{r:?} should count as cutting");
        }
        for r in [MoveRole::Rapid, MoveRole::Retract, MoveRole::Link] {
            assert!(!r.cuts(), "{r:?} should not count as cutting");
        }
        assert!(MoveRole::Link.is_unplanned());
        assert!(!MoveRole::Rapid.is_unplanned());
    }

    #[test]
    fn unplanned_links_are_detectable() {
        let mut t = Toolpath::new();
        t.rapid_to(dvec3(0.0, 0.0, 5.0)).unwrap();
        assert!(!t.has_unplanned_links());
        t.feed_to(dvec3(1.0, 0.0, 0.0), MoveRole::Link, 100.0)
            .unwrap();
        assert!(
            t.has_unplanned_links(),
            "a finished program must be able to prove it has none"
        );
    }

    #[test]
    fn measures_only_cutting_moves() {
        let mut t = Toolpath::new();
        t.rapid_to(dvec3(0.0, 0.0, 0.0)).unwrap();
        t.feed_to(dvec3(3.0, 0.0, 0.0), MoveRole::Cut, 500.0)
            .unwrap();
        t.rapid_to(dvec3(3.0, 10.0, 0.0)).unwrap();
        t.feed_to(dvec3(3.0, 14.0, 0.0), MoveRole::Cut, 500.0)
            .unwrap();
        assert_eq!(t.cutting_length(dvec3(0.0, 0.0, 0.0)), 7.0);
        assert_eq!(t.end(), Some(dvec3(3.0, 14.0, 0.0)));
        assert_eq!(t.len(), 4);
    }

    /// Arcs are reserved but must already be representable, or arc fitting would
    /// have nowhere to put its output and this type would need changing.
    #[test]
    fn arcs_are_representable_though_unused() {
        let mut t = Toolpath::new();
        let arc = Move {
            to: dvec3(1.0, 1.0, 0.0),
            kind: MoveKind::Arc {
                centre: dvec3(1.0, 0.0, 0.0),
                clockwise: true,
            },
            role: MoveRole::Cut,
            feed: Some(400.0),
        };
        assert!(t.push(arc).is_ok());

        let broken = Move {
            kind: MoveKind::Arc {
                centre: dvec3(f64::INFINITY, 0.0, 0.0),
                clockwise: false,
            },
            ..arc
        };
        assert!(matches!(
            t.push(broken),
            Err(ToolpathError::NonFiniteTarget(_))
        ));
    }
}
