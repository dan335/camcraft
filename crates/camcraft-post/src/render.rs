//! Rendering an instruction stream into one dialect.
//!
//! The renderer holds the only mutable machine state in the crate: what motion mode,
//! position and feed the machine has actually been told. Modal suppression is
//! decided against *that*, not against what the caller meant, because a word omitted
//! as "unchanged" is only safe if the machine already has the value.

use std::fmt::Write as _;

use camcraft_core::toolpath::MoveRole;
use glam::{DVec2, DVec3};

use crate::op::{CompSide, Coolant, CycleKind, Op, Spin, Units};
use crate::profile::{ArcStyle, Profile, ToolChangeStyle};

/// What rendering produced besides the text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderReport {
    /// Lines written.
    pub lines: u64,
    /// Motion blocks written.
    pub motion_blocks: u64,
    /// Axis words omitted because the machine already had the value.
    pub suppressed_words: u64,
    /// Reserved instructions encountered.
    ///
    /// Non-zero means the stream used something 0.1 does not emit, so the output
    /// should be read before it is trusted.
    pub reserved_used: u64,
}

/// Render a stream into G-code.
pub fn render(ops: &[Op], profile: &Profile, program_name: &str) -> (String, RenderReport) {
    let mut w = Writer::new(profile);
    for line in &profile.header {
        w.raw(&line.replace("{name}", program_name));
    }
    for op in ops {
        w.op(op);
    }
    for line in &profile.footer {
        w.raw(&line.replace("{name}", program_name));
    }
    (w.out, w.report)
}

struct Writer<'a> {
    p: &'a Profile,
    out: String,
    report: RenderReport,
    line_no: u32,
    // What the machine has actually been told. `None` means "never written".
    motion: Option<&'static str>,
    at: Option<DVec3>,
    feed: Option<String>,
}

impl<'a> Writer<'a> {
    fn new(p: &'a Profile) -> Self {
        Self {
            p,
            out: String::new(),
            report: RenderReport::default(),
            line_no: p.numbering.start,
            motion: None,
            at: None,
            feed: None,
        }
    }

    fn raw(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.p.numbering.enabled {
            let _ = write!(self.out, "N{} ", self.line_no);
            self.line_no = self.line_no.saturating_add(self.p.numbering.increment);
        }
        self.out.push_str(text);
        self.out.push('\n');
        self.report.lines += 1;
    }

    fn comment(&mut self, text: &str) {
        let c = self.p.comment(text);
        self.raw(&c);
    }

    fn op(&mut self, op: &Op) {
        match op {
            Op::SectionStart {
                op_name,
                tool,
                strategy,
                wcs,
            } => {
                self.comment(&format!("{op_name} - {strategy} - T{tool} G{wcs}"));
            }
            Op::SectionEnd => {}
            Op::Comment(t) => self.comment(t),
            Op::Units(u) => {
                self.raw(match u {
                    Units::Millimetres => "G21",
                    Units::Inches => "G20",
                });
                // Changing units invalidates every remembered coordinate.
                self.at = None;
                self.feed = None;
            }
            Op::WorkOffset(n) => {
                let s = format!("G{n}");
                self.raw(&s);
            }
            Op::ToolChange { tool, comment } => match self.p.tool_change {
                ToolChangeStyle::M6 => {
                    if !comment.is_empty() {
                        self.comment(comment);
                    }
                    let s = format!("T{tool} M6");
                    self.raw(&s);
                }
                ToolChangeStyle::Pause => {
                    let msg = if comment.is_empty() {
                        format!("fit tool {tool}")
                    } else {
                        format!("fit tool {tool}: {comment}")
                    };
                    self.comment(&msg);
                    let s = format!("T{tool}");
                    self.raw(&s);
                    self.raw("M0");
                }
            },
            Op::SpindleOn { rpm, dir } => {
                let m = match dir {
                    Spin::Clockwise => "M3",
                    Spin::CounterClockwise => "M4",
                };
                let s = format!("S{rpm} {m}");
                self.raw(&s);
            }
            Op::SpindleOff => self.raw("M5"),
            Op::CoolantOn(c) => self.raw(match c {
                Coolant::Flood => "M8",
                Coolant::Mist => "M7",
            }),
            Op::CoolantOff => self.raw("M9"),
            Op::Rapid { to } => self.motion_block("G0", *to, None),
            Op::Feed { to, feed, .. } => {
                let f = self.p.feed(*feed);
                self.motion_block("G1", *to, Some(f));
            }
            Op::Arc {
                to,
                centre,
                clockwise,
                feed,
            } => {
                self.report.reserved_used += 1;
                self.arc(*to, *centre, *clockwise, *feed);
            }
            Op::Cycle {
                kind,
                at,
                depth,
                retract,
                peck,
                dwell,
                feed,
            } => {
                self.cycle(*kind, *at, *depth, *retract, *peck, *dwell, *feed);
            }
            Op::CycleEnd => self.raw("G80"),
            Op::CutterComp { side, d } => {
                self.report.reserved_used += 1;
                let g = match side {
                    CompSide::Left => "G41",
                    CompSide::Right => "G42",
                    CompSide::Off => "G40",
                };
                let s = match side {
                    CompSide::Off => g.to_string(),
                    _ => format!("{g} D{d}"),
                };
                self.raw(&s);
                self.motion = None;
            }
            Op::Dwell(secs) => {
                let s = format!("G4 P{}", self.p.feed(*secs));
                self.raw(&s);
            }
            Op::Raw(text) => {
                // Passed through untouched, by definition. Modal state is unknown
                // afterwards, so nothing may be suppressed against it.
                self.raw(text);
                self.motion = None;
                self.at = None;
                self.feed = None;
            }
            Op::ProgramEnd => {}
        }
    }

    /// Emit one motion block, suppressing only what the machine already has.
    fn motion_block(&mut self, g: &'static str, to: DVec3, feed: Option<String>) {
        // Round first, then compare. Comparing unrounded values while emitting
        // rounded ones can suppress a word the machine would have seen change.
        let (x, y, z) = (self.p.axis(to.x), self.p.axis(to.y), self.p.axis(to.z));
        let prev = self
            .at
            .map(|p| (self.p.axis(p.x), self.p.axis(p.y), self.p.axis(p.z)));

        let mut block = String::new();
        let motion_changed = self.motion != Some(g);
        if motion_changed || !self.p.modal.suppress_motion {
            block.push_str(g);
        }

        let push = |axis: char, val: &str, same: bool, block: &mut String| -> bool {
            if same && self.p.modal.suppress_axes {
                return true;
            }
            if !block.is_empty() {
                block.push(' ');
            }
            let _ = write!(block, "{axis}{val}");
            false
        };

        let same = |a: Option<&String>, b: &str| a.map(|s| s.as_str()) == Some(b);
        let mut dropped = 0u64;
        if push('X', &x, same(prev.as_ref().map(|p| &p.0), &x), &mut block) {
            dropped += 1;
        }
        if push('Y', &y, same(prev.as_ref().map(|p| &p.1), &y), &mut block) {
            dropped += 1;
        }
        if push('Z', &z, same(prev.as_ref().map(|p| &p.2), &z), &mut block) {
            dropped += 1;
        }

        if let Some(f) = &feed {
            let unchanged = self.feed.as_deref() == Some(f.as_str());
            if !unchanged || !self.p.modal.suppress_feed {
                if !block.is_empty() {
                    block.push(' ');
                }
                let _ = write!(block, "F{f}");
            }
            self.feed = Some(f.clone());
        }

        // Every word suppressed means the machine is already where it is being told
        // to go. Emitting a bare motion word would be meaningless, so skip it.
        if block.is_empty() {
            self.at = Some(to);
            return;
        }

        self.raw(&block);
        self.report.motion_blocks += 1;
        self.report.suppressed_words += dropped;
        self.motion = Some(g);
        self.at = Some(to);
    }

    fn arc(&mut self, to: DVec3, centre: DVec3, clockwise: bool, feed: f64) {
        let g = if clockwise { "G2" } else { "G3" };
        let from = self.at.unwrap_or(DVec3::ZERO);
        let mut block = format!(
            "{g} X{} Y{} Z{}",
            self.p.axis(to.x),
            self.p.axis(to.y),
            self.p.axis(to.z)
        );
        match self.p.arc_style {
            ArcStyle::Ij => {
                let _ = write!(
                    block,
                    " I{} J{}",
                    self.p.axis(centre.x - from.x),
                    self.p.axis(centre.y - from.y)
                );
            }
            ArcStyle::R => {
                let r = (centre.truncate() - from.truncate()).length();
                let _ = write!(block, " R{}", self.p.axis(r));
            }
        }
        let f = self.p.feed(feed);
        let _ = write!(block, " F{f}");
        self.raw(&block);
        self.report.motion_blocks += 1;
        self.motion = Some(g);
        self.at = Some(to);
        self.feed = Some(f);
    }

    #[allow(clippy::too_many_arguments)]
    fn cycle(
        &mut self,
        kind: CycleKind,
        at: DVec2,
        depth: f64,
        retract: f64,
        peck: Option<f64>,
        dwell: Option<f64>,
        feed: f64,
    ) {
        let g = match kind {
            CycleKind::Drill => "G81",
            CycleKind::DrillDwell => "G82",
            CycleKind::Peck => "G83",
            CycleKind::Tap => "G84",
            CycleKind::Bore => "G85",
        };
        // Every word every time: a cycle is modal, but a program read by a person
        // should say where each hole is and how deep, whatever came before.
        let mut block = format!(
            "{g} X{} Y{} Z{} R{} F{}",
            self.p.axis(at.x),
            self.p.axis(at.y),
            self.p.axis(depth),
            self.p.axis(retract),
            self.p.feed(feed)
        );
        if let Some(q) = peck {
            let _ = write!(block, " Q{}", self.p.axis(q));
        }
        if let Some(p) = dwell {
            let _ = write!(block, " P{}", self.p.feed(p));
        }
        self.raw(&block);
        // A cycle leaves the machine in its own modal group and moves the tool.
        self.motion = None;
        self.at = None;
        self.feed = None;
    }
}

/// Whether a role is one a profile might want to treat specially later.
///
/// Unused in 0.1 — every role renders the same way — but the role reaches the
/// renderer so per-role feeds and lead handling can be added without changing the
/// IR or the engine.
pub fn role_is_cutting(role: MoveRole) -> bool {
    role.cuts()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::float_cmp,
    clippy::panic
)]
mod tests {
    use super::*;
    use crate::op::ProgramSpec;
    use camcraft_core::toolpath::Toolpath;
    use glam::dvec3;

    fn masso() -> Profile {
        Profile::builtin("masso").unwrap().unwrap()
    }

    fn path() -> Toolpath {
        let mut t = Toolpath::new();
        t.rapid_to(dvec3(0.0, 0.0, 10.0)).unwrap();
        t.feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Plunge, 300.0)
            .unwrap();
        t.feed_to(dvec3(10.0, 0.0, 0.0), MoveRole::Cut, 1200.0)
            .unwrap();
        t.feed_to(dvec3(10.0, 5.0, 0.0), MoveRole::Cut, 1200.0)
            .unwrap();
        t
    }

    fn program(p: &Profile) -> String {
        let ops = crate::op::lower(&path(), &ProgramSpec::default()).unwrap();
        render(&ops, p, "test").0
    }

    #[test]
    fn writes_a_recognisable_program() {
        let g = program(&masso());
        assert!(g.contains("G21"), "units missing:\n{g}");
        assert!(g.contains("G54"));
        assert!(g.contains("M3"));
        assert!(g.contains("M5"));
        assert!(g.contains("M30"));
        assert!(g.contains("G0"));
        assert!(g.contains("G1"));
    }

    /// The first motion block must carry every word: nothing has been written yet,
    /// so the machine has no remembered position to suppress against.
    #[test]
    fn the_first_move_states_everything() {
        let g = program(&masso());
        let first = g
            .lines()
            .find(|l| l.starts_with("G0"))
            .expect("a rapid should be present");
        for axis in ['X', 'Y', 'Z'] {
            assert!(first.contains(axis), "first move omits {axis}: {first}");
        }
    }

    /// Suppression is the point of modal output, so it must actually happen.
    #[test]
    fn unchanged_axes_are_suppressed() {
        let g = program(&masso());
        // Cutting from (0,0,0) to (10,0,0) changes only X.
        let cut = g
            .lines()
            .find(|l| l.contains("X10.000"))
            .expect("the cut should be present");
        assert!(
            !cut.contains('Y'),
            "Y was unchanged and should be gone: {cut}"
        );
        assert!(
            !cut.contains('Z'),
            "Z was unchanged and should be gone: {cut}"
        );
    }

    /// The feed is stated once and then inherited.
    #[test]
    fn repeated_feeds_are_suppressed() {
        let g = program(&masso());
        assert_eq!(
            g.matches("F1200.0").count(),
            1,
            "the cutting feed should be stated once:\n{g}"
        );
    }

    /// A motion word is only omitted when the mode genuinely carries over.
    #[test]
    fn motion_mode_is_restated_when_it_changes() {
        let g = program(&masso());
        let motion: Vec<&str> = g
            .lines()
            .filter(|l| l.starts_with("G0") || l.starts_with("G1"))
            .collect();
        assert!(motion.len() >= 2, "expected both a rapid and a feed");
        assert!(motion.iter().any(|l| l.starts_with("G0")));
        assert!(motion.iter().any(|l| l.starts_with("G1")));
    }

    /// The two profiles must actually differ, or the abstraction is decoration.
    #[test]
    fn the_two_dialects_differ() {
        let m = program(&masso());
        let g = program(&Profile::builtin("grbl").unwrap().unwrap());
        assert_ne!(m, g, "masso and grbl produced identical output");
        assert!(m.contains("M6"), "masso uses a tool changer");
        assert!(g.contains("M0"), "grbl pauses for a manual change");
        assert!(g.contains("X10 "), "grbl strips trailing zeros:\n{g}");
    }

    /// Reserved instructions still render, and say so, so that using one before its
    /// feature exists is visible rather than silent.
    #[test]
    fn reserved_instructions_render_and_are_counted() {
        let ops = vec![
            Op::Units(Units::Millimetres),
            Op::Rapid {
                to: dvec3(0.0, 0.0, 0.0),
            },
            Op::Arc {
                to: dvec3(10.0, 0.0, 0.0),
                centre: dvec3(5.0, 0.0, 0.0),
                clockwise: true,
                feed: 600.0,
            },
            Op::Cycle {
                kind: CycleKind::Peck,
                at: glam::dvec2(1.0, 2.0),
                depth: -5.0,
                retract: 2.0,
                peck: Some(1.0),
                dwell: None,
                feed: 200.0,
            },
        ];
        let (g, report) = render(&ops, &masso(), "t");
        assert!(g.contains("G2 "), "arc missing:\n{g}");
        assert!(g.contains("I5.000"), "IJ centre missing:\n{g}");
        assert!(g.contains("G83"), "peck cycle missing:\n{g}");
        assert!(g.contains("Q1.000"));
        assert!(
            g.contains("X1.000 Y2.000"),
            "the hole's place missing:\n{g}"
        );
        // A cycle is emitted now, by drilling; only the arc is still reserved.
        assert_eq!(report.reserved_used, 1);
    }

    /// Raw text is a hole in the renderer's knowledge; suppressing against stale
    /// state afterwards could omit a word the machine now needs.
    #[test]
    fn raw_text_resets_modal_state() {
        let ops = vec![
            Op::Rapid {
                to: dvec3(1.0, 2.0, 3.0),
            },
            Op::Raw("G53 G0 Z0".into()),
            Op::Rapid {
                to: dvec3(1.0, 2.0, 3.0),
            },
        ];
        let (g, _) = render(&ops, &masso(), "t");
        let rapids: Vec<&str> = g.lines().filter(|l| l.starts_with("G0 ")).collect();
        assert_eq!(rapids.len(), 2, "both rapids should be present:\n{g}");
        for r in rapids {
            assert!(r.contains('X') && r.contains('Y') && r.contains('Z'), "{r}");
        }
    }

    /// A move to where the machine already is produces nothing at all.
    #[test]
    fn a_move_to_the_current_position_emits_nothing() {
        let ops = vec![
            Op::Rapid {
                to: dvec3(1.0, 2.0, 3.0),
            },
            Op::Rapid {
                to: dvec3(1.0, 2.0, 3.0),
            },
        ];
        let (g, report) = render(&ops, &masso(), "t");
        assert_eq!(report.motion_blocks, 1, "the second move is a no-op:\n{g}");
    }
}
