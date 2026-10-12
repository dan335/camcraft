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

pub use camcraft_geom::arc::Arc;
use glam::DVec3;

use crate::is_positive;

/// How far, in millimetres, an arc's end may be from where its shape says it
/// ends: rounding, and nothing more.
const ARC_END: f64 = 1.0e-6;

/// Whether a three-axis machine cuts an arc about `axis` as one: it turns only
/// about X, Y or Z (`G19`, `G18`, `G17`).
pub fn principal(axis: DVec3) -> bool {
    let a = axis.abs();
    a.max_element() > 1.0 - 1.0e-9
}

/// Whether an arc turns about Z, so runs level, or as a helix about a vertical
/// axis.
pub(crate) fn turns_about_z(arc: &Arc) -> bool {
    arc.axis.z.abs() > 1.0 - 1.0e-9
}

/// Descent-to-advance ratio past which a cutting move is falling rather than
/// cutting along.
///
/// Two conditions make a plunge, not one. Steepness alone is safe: a ball riding
/// down a near-vertical but *smooth* face descends faster than it advances and is
/// fully supported, for a short way. Depth alone is safe too: a long gentle descent
/// is just a cut. The hazard is both together — dropping a long way while barely
/// advancing, which engages the end of the cutter rather than its side. So a plunge
/// is a continuous descent steeper than this that falls further than the
/// operation's `max_cut_step`. Ten is about 84 degrees; real surfaces measured on
/// the router jig top out near 3.
pub const PLUNGE_RATIO: f64 = 10.0;

/// Whether falling `drop` over `flat` of travel seen from above is steeper than
/// `ratio`, fall per unit of travel: at [`PLUNGE_RATIO`], a plunge.
pub(crate) fn steep(drop: f64, flat: f64, ratio: f64) -> bool {
    drop > 0.0 && drop > flat * ratio
}

/// One continuous steep descent: moves `first..=last`, falling `depth` in all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Descent {
    /// Index of its first move.
    pub first: usize,
    /// Index of its last move.
    pub last: usize,
    /// How far it falls, in millimetres.
    pub depth: f64,
}

/// Continuous steep descents, followed a step at a time: a run of steps each
/// falling steeply is one descent, measured in full, and any other step ends it.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SteepRun {
    /// The descent in progress, if one is.
    current: Option<Descent>,
}

impl SteepRun {
    /// Step `i`, falling `drop`, steeply or not. Returns the descent it ends, if
    /// it ends one.
    pub(crate) fn step(&mut self, i: usize, drop: f64, steep: bool) -> Option<Descent> {
        if !steep {
            return self.current.take();
        }
        let d = self.current.get_or_insert(Descent {
            first: i,
            last: i,
            depth: 0.0,
        });
        d.last = i;
        d.depth += drop;
        None
    }

    /// The descent in progress at the end, if one is.
    pub(crate) fn finish(self) -> Option<Descent> {
        self.current
    }
}

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
    /// A connection between two cuts made without lifting clear: at the cutting
    /// feed, over the part, never into it.
    Transition,
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
    /// A circular arc, or a helix, to the target, from where the move before it
    /// ended. Only a feeding move follows one: a rapid, plunge or retract is
    /// straight by definition.
    Arc(Arc),
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

impl Move {
    /// The point a fraction `t` of the way along it, from `from`.
    pub fn point_at(&self, from: DVec3, t: f64) -> DVec3 {
        match self.kind {
            MoveKind::Linear => from.lerp(self.to, t),
            MoveKind::Arc(a) => a.point_at(from, self.to, t),
        }
    }

    /// The direction it runs a fraction `t` of the way along, from `from`, or
    /// `None` for a move that goes nowhere.
    pub fn tangent_at(&self, from: DVec3, t: f64) -> Option<DVec3> {
        match self.kind {
            MoveKind::Linear => (self.to - from).try_normalize(),
            MoveKind::Arc(a) => Some(a.tangent_at(from, self.to, t)),
        }
    }

    /// Its length, from `from`.
    pub fn length(&self, from: DVec3) -> f64 {
        match self.kind {
            MoveKind::Linear => from.distance(self.to),
            MoveKind::Arc(a) => a.length(from, self.to),
        }
    }

    /// How far it runs seen from above, from `from`: an arc that turns about Z
    /// exactly, any other arc along its chords.
    pub fn flat_length(&self, from: DVec3) -> f64 {
        match self.kind {
            MoveKind::Linear => from.truncate().distance(self.to.truncate()),
            MoveKind::Arc(a) if turns_about_z(&a) => a.radius(from) * a.sweep.abs(),
            MoveKind::Arc(_) => {
                let mut at = from;
                let mut length = 0.0;
                for p in self.chords(from, 1e-3) {
                    length += at.truncate().distance(p.truncate());
                    at = p;
                }
                length
            }
        }
    }

    /// Points along it within `tolerance` of it, from `from`, the start left out
    /// and its end last: a straight move's is its end alone.
    pub fn chords(&self, from: DVec3, tolerance: f64) -> Vec<DVec3> {
        match self.kind {
            MoveKind::Linear => vec![self.to],
            MoveKind::Arc(a) => a.chords(from, self.to, tolerance),
        }
    }

    /// The same move run backwards, from its end to `from`, with the same role
    /// and feed.
    pub fn reversed(&self, from: DVec3) -> Self {
        Self {
            to: from,
            kind: match self.kind {
                MoveKind::Linear => MoveKind::Linear,
                MoveKind::Arc(a) => MoveKind::Arc(a.reversed()),
            },
            ..*self
        }
    }
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
    /// An arc was the first move, so had nowhere to start from.
    #[error("an arc cannot be a path's first move")]
    ArcWithoutStart,
    /// An arc's shape was unusable, or it does not end where the move does.
    #[error("the arc to {0:?} is not a usable arc from where the path is")]
    BadArc(DVec3),
    /// A move that must be straight was given an arc.
    #[error("{0:?} moves are straight")]
    CurvedMove(MoveRole),
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
        if let MoveKind::Arc(a) = m.kind {
            if matches!(
                m.role,
                MoveRole::Rapid | MoveRole::Plunge | MoveRole::Retract | MoveRole::Link
            ) {
                return Err(ToolpathError::CurvedMove(m.role));
            }
            let from = self.end().ok_or(ToolpathError::ArcWithoutStart)?;
            if !a.is_usable(from, ARC_END) || !a.ends_at(from, m.to, ARC_END) {
                return Err(ToolpathError::BadArc(m.to));
            }
        }
        let wants_feed = m.role != MoveRole::Rapid;
        match (wants_feed, m.feed) {
            (true, Some(f)) if is_positive(f) => {}
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

    /// Append an arc, or a helix, from where the path is.
    pub fn arc_to(
        &mut self,
        to: DVec3,
        arc: Arc,
        role: MoveRole,
        feed: f64,
    ) -> Result<(), ToolpathError> {
        self.push(Move {
            to,
            kind: MoveKind::Arc(arc),
            role,
            feed: Some(feed),
        })
    }

    /// Append an arc, as one move where a three-axis machine can cut it as one
    /// ([`principal`]), and otherwise as its chords within `tolerance`, so the
    /// path is what the program will be.
    pub fn arc_or_chords_to(
        &mut self,
        to: DVec3,
        arc: Arc,
        role: MoveRole,
        feed: f64,
        tolerance: f64,
    ) -> Result<(), ToolpathError> {
        if principal(arc.axis) {
            return self.arc_to(to, arc, role, feed);
        }
        let from = self.end().ok_or(ToolpathError::ArcWithoutStart)?;
        for p in arc.chords(from, to, tolerance) {
            self.feed_to(p, role, feed)?;
        }
        Ok(())
    }

    /// Whether any move still needs the linker's attention.
    ///
    /// A finished program must answer `false`. This is the checkable form of "links
    /// are holes": nothing convention-based, just a property.
    pub fn has_unplanned_links(&self) -> bool {
        self.moves.iter().any(|m| m.role.is_unplanned())
    }

    /// The path in passes: each from a [`MoveRole::Link`] up to the next, and
    /// first whatever comes before the first link, if anything does.
    pub fn passes(&self) -> impl Iterator<Item = &[Move]> {
        self.moves.chunk_by(|_, m| m.role != MoveRole::Link)
    }

    /// Every continuous steep descent: a run of consecutive cutting moves, each
    /// falling more steeply than `max_ratio` (fall per unit of horizontal travel).
    ///
    /// A cutting move that drops a long way while barely advancing is a full-depth
    /// plunge wearing a `Cut` label, and at cutting feed that is how end mills get
    /// broken. It arises naturally wherever the CL surface is discontinuous — a
    /// parallel pass crossing a vertical wall samples the top on one side and the
    /// floor on the other, and both samples are correct.
    ///
    /// A descent is measured over the whole run, not move by move: a plunge split
    /// into short moves is still the same plunge, and judging each piece alone
    /// would let it through.
    ///
    /// Detecting it is not optional politeness: every other check in this crate
    /// passes on such a path, because every individual point really is on the
    /// surface. Only the *connection* between them is unmachinable.
    pub fn steep_descents(&self, from: DVec3, max_ratio: f64) -> Vec<Descent> {
        let mut out = Vec::new();
        let mut at = from;
        let mut run = SteepRun::default();
        for (i, m) in self.moves.iter().enumerate() {
            let drop = at.z - m.to.z;
            // A transition runs at the cutting feed too, so a steep one is the
            // same plunge.
            let cutting = matches!(m.role, MoveRole::Cut | MoveRole::Transition);
            let falling = cutting && steep(drop, m.flat_length(at), max_ratio);
            out.extend(run.step(i, drop, falling));
            at = m.to;
        }
        out.extend(run.finish());
        out
    }

    /// How many continuous steep descents there are (see
    /// [`steep_descents`](Self::steep_descents)), and the deepest, in millimetres.
    pub fn steep_cut_descents(&self, from: DVec3, max_ratio: f64) -> (u64, f64) {
        let all = self.steep_descents(from, max_ratio);
        let deepest = all.iter().map(|d| d.depth).fold(0.0, f64::max);
        (all.len() as u64, deepest)
    }

    /// Total length of the moves at a feed that cuts, in millimetres: cuts,
    /// ramps, plunges, leads and transitions.
    #[cfg(test)]
    pub(crate) fn cutting_length(&self, from: DVec3) -> f64 {
        let mut at = from;
        let mut total = 0.0;
        for m in &self.moves {
            if matches!(
                m.role,
                MoveRole::Cut
                    | MoveRole::Ramp
                    | MoveRole::Plunge
                    | MoveRole::Lead
                    | MoveRole::Transition
            ) {
                total += m.length(at);
            }
            at = m.to;
        }
        total
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests;
