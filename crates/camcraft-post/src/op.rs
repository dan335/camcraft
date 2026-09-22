//! The machine-independent instruction stream.
//!
//! `camcraft-core` produces a [`Toolpath`]; this crate turns it into a sequence of
//! [`Op`], and a profile renders that into one dialect. The engine never sees this
//! type and never sees G-code — the crate graph makes that structural rather than a
//! matter of discipline.
//!
//! # Structure, not just moves
//!
//! The stream carries section boundaries naming the operation, tool, strategy and
//! work offset. Posts need those to write headers, tool-change blocks and setup
//! sheets; a flat list of coordinates leaves every post guessing.
//!
//! # Reserved variants
//!
//! [`Op::Arc`], [`Op::Cycle`] and [`Op::CutterComp`] are defined and rendered but
//! nothing emits them in 0.1. They exist so arc fitting, drill cycles and cutter
//! compensation can arrive as additions rather than as a change to this type — the
//! same reason strategies emit toolpaths instead of instructions.

use camcraft_core::toolpath::{MoveKind, MoveRole, Toolpath};
use glam::DVec3;

/// Which unit the program is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Units {
    /// Millimetres (`G21`).
    Millimetres,
    /// Inches (`G20`).
    Inches,
}

/// Spindle direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spin {
    /// Clockwise (`M3`).
    Clockwise,
    /// Counter-clockwise (`M4`).
    CounterClockwise,
}

/// Coolant selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coolant {
    /// Flood (`M8`).
    Flood,
    /// Mist (`M7`).
    Mist,
}

/// A canned drilling cycle. Reserved; nothing emits these in 0.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleKind {
    /// Simple drill (`G81`).
    Drill,
    /// Peck drill (`G83`).
    Peck,
    /// Tapping (`G84`).
    Tap,
    /// Boring (`G85`).
    Bore,
}

/// Cutter compensation side. Reserved; nothing emits these in 0.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompSide {
    /// Left of the path (`G41`).
    Left,
    /// Right of the path (`G42`).
    Right,
    /// Cancel (`G40`).
    Off,
}

/// One machine-independent instruction.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Beginning of one operation's output.
    SectionStart {
        /// Human name of the operation.
        op_name: String,
        /// Tool number.
        tool: u32,
        /// Strategy that produced it.
        strategy: String,
        /// Work offset in force, 54 for `G54`.
        wcs: u8,
    },
    /// End of the current operation's output.
    SectionEnd,
    /// A comment.
    Comment(String),
    /// Set the program units.
    Units(Units),
    /// Select a work offset, 54 for `G54`.
    WorkOffset(u8),
    /// Change to a tool.
    ToolChange {
        /// Tool number.
        tool: u32,
        /// Description for the operator.
        comment: String,
    },
    /// Start the spindle.
    SpindleOn {
        /// Speed in rev/min.
        rpm: u32,
        /// Direction.
        dir: Spin,
    },
    /// Stop the spindle.
    SpindleOff,
    /// Turn coolant on.
    CoolantOn(Coolant),
    /// Turn coolant off.
    CoolantOff,
    /// Traverse at maximum speed.
    Rapid {
        /// Target position.
        to: DVec3,
    },
    /// Move at a feed rate.
    Feed {
        /// Target position.
        to: DVec3,
        /// Feed in mm/min.
        feed: f64,
        /// Why this move exists, so a profile can vary its output by role.
        role: MoveRole,
    },
    /// Circular interpolation. Reserved.
    Arc {
        /// Target position.
        to: DVec3,
        /// Arc centre.
        centre: DVec3,
        /// Turning clockwise seen from +Z.
        clockwise: bool,
        /// Feed in mm/min.
        feed: f64,
    },
    /// Canned cycle. Reserved.
    Cycle {
        /// Which cycle.
        kind: CycleKind,
        /// Final depth.
        depth: f64,
        /// Retract height.
        retract: f64,
        /// Peck increment, where the cycle uses one.
        peck: Option<f64>,
        /// Feed in mm/min.
        feed: f64,
    },
    /// Cutter compensation. Reserved.
    CutterComp {
        /// Which side.
        side: CompSide,
        /// Offset register.
        d: u32,
    },
    /// Pause for a number of seconds.
    Dwell(f64),
    /// Text passed through untouched, for manual NC and probe macros.
    Raw(String),
    /// End of program.
    ProgramEnd,
}

/// Everything a program needs that a toolpath does not carry.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgramSpec {
    /// Program name, for the header comment.
    pub name: String,
    /// Operation name.
    pub op_name: String,
    /// Strategy that produced the path.
    pub strategy: String,
    /// Tool number.
    pub tool: u32,
    /// Tool description for the operator.
    pub tool_comment: String,
    /// Work offset, 54 for `G54`.
    pub wcs: u8,
    /// Spindle speed.
    pub rpm: u32,
    /// Spindle direction.
    pub spin: Spin,
    /// Coolant, or `None` to leave it off.
    pub coolant: Option<Coolant>,
    /// Program units.
    pub units: Units,
}

impl Default for ProgramSpec {
    fn default() -> Self {
        Self {
            name: "camcraft".into(),
            op_name: "operation".into(),
            strategy: "parallel".into(),
            tool: 1,
            tool_comment: String::new(),
            wcs: 54,
            rpm: 18000,
            spin: Spin::Clockwise,
            coolant: None,
            units: Units::Millimetres,
        }
    }
}

/// A toolpath that cannot be turned into a program.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum LowerError {
    /// The path still contains moves the linker was supposed to resolve.
    ///
    /// Refused rather than rendered: a link is "get here somehow", and a post that
    /// guessed would be inventing a traverse nobody planned.
    #[error("toolpath still contains {0} unresolved link(s); run the linker first")]
    UnresolvedLinks(usize),
    /// A work offset outside the usual range.
    #[error("work offset {0} is not in the range 54..=59")]
    BadWorkOffset(u8),
}

/// Turn a linked toolpath into an instruction stream.
///
/// Refuses a path containing unresolved links. That check lives here rather than in
/// a post because every post would otherwise have to repeat it, and one that forgot
/// would emit a program with a hole in it.
pub fn lower(path: &Toolpath, spec: &ProgramSpec) -> Result<Vec<Op>, LowerError> {
    let unresolved = path
        .moves()
        .iter()
        .filter(|m| m.role.is_unplanned())
        .count();
    if unresolved > 0 {
        return Err(LowerError::UnresolvedLinks(unresolved));
    }
    if !(54..=59).contains(&spec.wcs) {
        return Err(LowerError::BadWorkOffset(spec.wcs));
    }

    // The profile header owns the program name via `{name}`; repeating it here
    // would print it twice in every file.
    let mut out = Vec::with_capacity(path.len() + 16);
    out.push(Op::Units(spec.units));
    out.push(Op::SectionStart {
        op_name: spec.op_name.clone(),
        tool: spec.tool,
        strategy: spec.strategy.clone(),
        wcs: spec.wcs,
    });
    out.push(Op::ToolChange {
        tool: spec.tool,
        comment: spec.tool_comment.clone(),
    });
    out.push(Op::WorkOffset(spec.wcs));
    out.push(Op::SpindleOn {
        rpm: spec.rpm,
        dir: spec.spin,
    });
    if let Some(c) = spec.coolant {
        out.push(Op::CoolantOn(c));
    }

    for m in path.moves() {
        match (m.kind, m.role, m.feed) {
            (MoveKind::Linear, MoveRole::Rapid, _) => out.push(Op::Rapid { to: m.to }),
            (MoveKind::Linear, role, Some(feed)) => out.push(Op::Feed {
                to: m.to,
                feed,
                role,
            }),
            (MoveKind::Arc { centre, clockwise }, _, Some(feed)) => out.push(Op::Arc {
                to: m.to,
                centre,
                clockwise,
                feed,
            }),
            // A feeding move with no feed cannot be constructed: Toolpath::push
            // rejects it. Skipping is unreachable, and silently emitting a rapid
            // here would be the dangerous choice.
            _ => {}
        }
    }

    if spec.coolant.is_some() {
        out.push(Op::CoolantOff);
    }
    out.push(Op::SpindleOff);
    out.push(Op::SectionEnd);
    out.push(Op::ProgramEnd);
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests {
    use super::*;
    use glam::dvec3;

    fn linked_path() -> Toolpath {
        let mut t = Toolpath::new();
        t.rapid_to(dvec3(0.0, 0.0, 10.0)).unwrap();
        t.feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Plunge, 300.0)
            .unwrap();
        t.feed_to(dvec3(10.0, 0.0, 0.0), MoveRole::Cut, 1200.0)
            .unwrap();
        t
    }

    #[test]
    fn wraps_the_path_in_a_program() {
        let ops = lower(&linked_path(), &ProgramSpec::default()).unwrap();
        assert_eq!(ops.first(), Some(&Op::Units(Units::Millimetres)));
        assert!(
            !ops.iter()
                .any(|o| matches!(o, Op::Comment(c) if *c == "camcraft")),
            "the profile header owns the program name; lowering must not repeat it"
        );
        assert!(ops.contains(&Op::WorkOffset(54)));
        assert!(ops.contains(&Op::SpindleOff));
        assert_eq!(ops.last(), Some(&Op::ProgramEnd));
        assert!(matches!(
            ops.iter().find(|o| matches!(o, Op::SectionStart { .. })),
            Some(Op::SectionStart {
                tool: 1,
                wcs: 54,
                ..
            })
        ));
    }

    /// A link is "get here somehow". A post that rendered one would be inventing a
    /// traverse nobody planned, so lowering refuses instead.
    #[test]
    fn refuses_a_path_with_unresolved_links() {
        let mut t = linked_path();
        t.feed_to(dvec3(5.0, 5.0, 0.0), MoveRole::Link, 100.0)
            .unwrap();
        assert_eq!(
            lower(&t, &ProgramSpec::default()),
            Err(LowerError::UnresolvedLinks(1))
        );
    }

    #[test]
    fn rejects_an_impossible_work_offset() {
        let spec = ProgramSpec {
            wcs: 99,
            ..Default::default()
        };
        assert_eq!(
            lower(&linked_path(), &spec),
            Err(LowerError::BadWorkOffset(99))
        );
    }

    #[test]
    fn moves_keep_their_role_and_feed() {
        let ops = lower(&linked_path(), &ProgramSpec::default()).unwrap();
        let feeds: Vec<_> = ops
            .iter()
            .filter_map(|o| match o {
                Op::Feed { role, feed, .. } => Some((*role, *feed)),
                _ => None,
            })
            .collect();
        assert_eq!(
            feeds,
            vec![(MoveRole::Plunge, 300.0), (MoveRole::Cut, 1200.0)]
        );
        assert_eq!(
            ops.iter().filter(|o| matches!(o, Op::Rapid { .. })).count(),
            1
        );
    }

    #[test]
    fn coolant_is_paired() {
        let spec = ProgramSpec {
            coolant: Some(Coolant::Flood),
            ..Default::default()
        };
        let ops = lower(&linked_path(), &spec).unwrap();
        assert!(ops.contains(&Op::CoolantOn(Coolant::Flood)));
        assert!(ops.contains(&Op::CoolantOff));

        let dry = lower(&linked_path(), &ProgramSpec::default()).unwrap();
        assert!(!dry.iter().any(|o| matches!(o, Op::CoolantOn(_))));
        assert!(!dry.contains(&Op::CoolantOff));
    }
}
