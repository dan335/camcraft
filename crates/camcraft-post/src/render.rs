//! Rendering an instruction stream into one dialect.
//!
//! The renderer holds the only mutable machine state in the crate: what motion mode,
//! position and feed the machine has actually been told. Modal suppression is
//! decided against *that*, not against what the caller meant, because a word omitted
//! as "unchanged" is only safe if the machine already has the value.

use std::fmt::Write as _;

use std::f64::consts::{FRAC_PI_2, PI};

use camcraft_core::job::{Coolant, SpindleDirection};
use camcraft_core::toolpath::Arc;
use glam::{DVec2, DVec3};

use crate::op::{CycleKind, Op, Units};
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
}

/// Render a stream into G-code.
pub fn render(ops: &[Op], profile: &Profile, program_name: &str) -> (String, RenderReport) {
    let mut w = Writer::new(profile);
    // The name goes in the header's comments: made safe to, or a name with a
    // delimiter in it would end one early.
    let name = profile.escaped(program_name);
    for line in &profile.header {
        w.raw(&line.replace("{name}", &name));
    }
    for op in ops {
        w.op(op);
    }
    for line in &profile.footer {
        w.raw(&line.replace("{name}", &name));
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
    // Whether the tool is known to be as high as the machine takes it, where it
    // is otherwise unknown: after a safe retract, until it next moves.
    raised: bool,
    // The plane arcs turn in (`G17`, `G18`, `G19`), once one has been written.
    plane: Option<&'static str>,
    // Whether lengths and feeds are written in inches: after a `G20`.
    inches: bool,
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
            raised: false,
            plane: None,
            inches: false,
        }
    }

    /// Select `plane` for arcs, unless it is in force.
    fn plane(&mut self, plane: &'static str) -> &'static str {
        if self.plane == Some(plane) {
            ""
        } else {
            self.plane = Some(plane);
            plane
        }
    }

    /// Back to the XY plane, if an arc left another in force: a canned cycle
    /// drills along the selected plane's normal, and the next program expects
    /// `G17`.
    fn level_plane(&mut self) {
        if matches!(self.plane, Some(p) if p != "G17") {
            self.raw("G17");
            self.plane = Some("G17");
        }
    }

    /// Forget where the machine is: something moved it, or may have.
    fn lost(&mut self) {
        self.motion = None;
        self.at = None;
        self.feed = None;
        self.raised = false;
    }

    /// The profile's lines for a step, after which where the machine is is
    /// unknown.
    fn lines(&mut self, lines: &[String]) {
        for line in lines {
            self.raw(line);
        }
        if !lines.is_empty() {
            self.lost();
        }
    }

    fn raw(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        // Blocks are numbered, comments not, as Fusion's are.
        if self.p.numbering.enabled && !text.starts_with(self.p.comment.0.as_str()) {
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
                self.inches = *u == Units::Inches;
                // Changing units invalidates every remembered coordinate.
                self.at = None;
                self.feed = None;
            }
            Op::WorkOffset(n) => {
                let s = format!("G{n}");
                self.raw(&s);
                // Another offset puts the same point at other coordinates.
                self.at = None;
            }
            Op::SafeRetract => {
                let p = self.p;
                self.lines(&p.safe_retract);
                self.raised = !p.safe_retract.is_empty();
            }
            Op::Home => {
                let p = self.p;
                self.lines(&p.end_home);
            }
            Op::ToolChange { tool, comment } => match self.p.tool_change {
                ToolChangeStyle::M6 => {
                    if !comment.is_empty() {
                        self.comment(comment);
                    }
                    let s = format!("T{tool} M6");
                    self.raw(&s);
                    // A changer moves the machine; the tool is where the change
                    // left it. Raised by a safe retract, it is still raised.
                    self.at = None;
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
                    // The operator may have jogged anywhere, down included.
                    self.lost();
                }
            },
            Op::SpindleOn { rpm, dir } => {
                let m = match dir {
                    SpindleDirection::Clockwise => "M3",
                    SpindleDirection::CounterClockwise => "M4",
                };
                let s = format!("S{rpm} {m}");
                self.raw(&s);
            }
            Op::SpindleOff => {
                self.level_plane();
                self.raw("M5");
            }
            Op::CoolantOn(c) => self.raw(match c {
                Coolant::Flood => "M8",
                Coolant::Mist => "M7",
                // Not emitted; asked for anyway, off is off.
                Coolant::Off => "M9",
            }),
            Op::CoolantOff => self.raw("M9"),
            Op::Rapid { to } => self.motion_block("G0", *to, None),
            Op::Feed { to, feed, .. } => {
                let f = self.p.feed_in(*feed, self.inches);
                self.motion_block("G1", *to, Some(f));
            }
            Op::Arc { to, arc, feed, .. } => self.arc(*to, arc, *feed),
            Op::Cycle {
                kind,
                at,
                depth,
                retract,
                peck,
                dwell,
                feed,
            } => {
                self.level_plane();
                self.cycle(*kind, *at, *depth, *retract, *peck, *dwell, *feed);
            }
            Op::CycleEnd => self.raw("G80"),
            Op::Raw(text) => {
                // Passed through untouched, by definition. Modal state is unknown
                // afterwards, so nothing may be suppressed against it.
                self.raw(text);
                self.lost();
            }
            Op::ProgramEnd => {}
        }
    }

    /// Emit one motion block, suppressing only what the machine already has.
    ///
    /// A rapid from nowhere known goes across and down one at a time, never
    /// diagonally: across first when the tool is known to be raised, as high as
    /// it can be, and otherwise down (or up) to its height first, which is
    /// clear of everything since it is the height the path crosses at.
    fn motion_block(&mut self, g: &'static str, to: DVec3, feed: Option<String>) {
        if g == "G0" && self.at.is_none() {
            let (x, y, z) = (
                self.p.length(to.x, self.inches),
                self.p.length(to.y, self.inches),
                self.p.length(to.z, self.inches),
            );
            let again = if self.p.modal.suppress_motion {
                ""
            } else {
                "G0 "
            };
            let (first, second) = if self.raised {
                (format!("G0 X{x} Y{y}"), format!("{again}Z{z}"))
            } else {
                (format!("G0 Z{z}"), format!("{again}X{x} Y{y}"))
            };
            self.raw(&first);
            self.raw(&second);
            self.report.motion_blocks += 2;
            self.motion = Some(g);
            self.at = Some(to);
            self.raised = false;
            return;
        }
        self.raised = false;
        // Round first, then compare. Comparing unrounded values while emitting
        // rounded ones can suppress a word the machine would have seen change.
        let (x, y, z) = (
            self.p.length(to.x, self.inches),
            self.p.length(to.y, self.inches),
            self.p.length(to.z, self.inches),
        );
        let prev = self.at.map(|p| {
            (
                self.p.length(p.x, self.inches),
                self.p.length(p.y, self.inches),
                self.p.length(p.z, self.inches),
            )
        });

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

    /// An arc, or a helix, from where the tool is, in the plane it turns in,
    /// written in pieces no longer than the profile's arcs may be: a half turn
    /// with centre offsets, which cannot be misread even as a full circle, and a
    /// quarter with a radius, whose sign would otherwise have to choose the long
    /// way round.
    fn arc(&mut self, to: DVec3, arc: &Arc, feed: f64) {
        let Some(from) = self.at else {
            // Nothing is known to start it from: straight to its end, rather than
            // round a centre measured from nowhere. The engine never does this.
            let f = self.p.feed_in(feed, self.inches);
            self.motion_block("G1", to, Some(f));
            return;
        };
        let most = match self.p.arc_style {
            ArcStyle::Ij => PI,
            ArcStyle::R => FRAC_PI_2,
        };
        let pieces = (arc.sweep.abs() / most - 1e-9).ceil().max(1.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let n = pieces.min(8.0) as u32;
        let mut at = from;
        for k in 1..=n {
            let end = if k == n {
                to
            } else {
                arc.point_at(from, to, f64::from(k) / f64::from(n))
            };
            let piece = Arc {
                sweep: arc.sweep / f64::from(n),
                ..*arc
            };
            self.arc_block(at, end, &piece, feed);
            at = end;
        }
    }

    /// One block of an arc no more than a half turn long.
    fn arc_block(&mut self, from: DVec3, to: DVec3, arc: &Arc, feed: f64) {
        // Which plane, its two axes, the axis along its normal, and whether the
        // arc turns counter-clockwise seen from that axis's positive end.
        let a = arc.axis;
        let (plane, axes, normal, ccw) = if a.z.abs() >= a.x.abs().max(a.y.abs()) {
            (
                "G17",
                [('X', 'I', 0), ('Y', 'J', 1)],
                ('Z', 2),
                arc.sweep * a.z > 0.0,
            )
        } else if a.y.abs() >= a.x.abs() {
            (
                "G18",
                [('X', 'I', 0), ('Z', 'K', 2)],
                ('Y', 1),
                arc.sweep * a.y > 0.0,
            )
        } else {
            (
                "G19",
                [('Y', 'J', 1), ('Z', 'K', 2)],
                ('X', 0),
                arc.sweep * a.x > 0.0,
            )
        };
        let g = if ccw { "G3" } else { "G2" };
        let centre = arc.centre + a * (from - arc.centre).dot(a);
        let mut block = String::new();
        let plane = self.plane(plane);
        if !plane.is_empty() {
            block.push_str(plane);
            block.push(' ');
        }
        block.push_str(g);
        // Both of the plane's axes, always, as Fusion writes them; the third only
        // where it moves, which makes a helix.
        for (axis, _, i) in axes {
            let _ = write!(block, " {axis}{}", self.p.length(to[i], self.inches));
        }
        let (axis, i) = normal;
        if self.p.length(to[i], self.inches) != self.p.length(from[i], self.inches) {
            let _ = write!(block, " {axis}{}", self.p.length(to[i], self.inches));
        }
        match self.p.arc_style {
            ArcStyle::Ij => {
                for (_, word, i) in axes {
                    let _ = write!(
                        block,
                        " {word}{}",
                        self.p.length(centre[i] - from[i], self.inches)
                    );
                }
            }
            ArcStyle::R => {
                let _ = write!(block, " R{}", self.p.length(arc.radius(from), self.inches));
            }
        }
        let f = self.p.feed_in(feed, self.inches);
        if self.feed.as_deref() != Some(f.as_str()) || !self.p.modal.suppress_feed {
            let _ = write!(block, " F{f}");
        }
        self.raw(&block);
        self.report.motion_blocks += 1;
        self.motion = Some(g);
        self.at = Some(to);
        self.feed = Some(f);
        self.raised = false;
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
            self.p.length(at.x, self.inches),
            self.p.length(at.y, self.inches),
            self.p.length(depth, self.inches),
            self.p.length(retract, self.inches),
            self.p.feed_in(feed, self.inches)
        );
        if let Some(q) = peck {
            let _ = write!(block, " Q{}", self.p.length(q, self.inches));
        }
        if let Some(p) = dwell {
            // Seconds, whatever the units.
            let _ = write!(block, " P{}", self.p.feed(p));
        }
        self.raw(&block);
        // A cycle leaves the machine in its own modal group and moves the tool.
        self.lost();
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    /// A program named with a comment's delimiters in it cannot end the
    /// header's comment early, nor start a block with a newline.
    #[test]
    fn a_program_name_stays_inside_its_comment() {
        let p = Profile::builtin("masso").unwrap().unwrap();
        let (text, _) = render(&[], &p, "bracket (rev B)\nM3");
        let first = text.lines().find(|l| l.contains("bracket")).unwrap();
        assert!(first.contains("bracket [rev B] M3"), "{first}");
        assert!(!text.lines().any(|l| l.trim() == "M3"), "{text}");
    }
    use crate::op::ProgramSpec;
    use camcraft_core::toolpath::{MoveRole, Toolpath};
    use glam::dvec3;

    /// The Masso profile written plainly: every digit, no line numbers, so the
    /// tests read the renderer's decisions rather than its dialect.
    fn masso() -> Profile {
        let mut p = Profile::builtin("masso").unwrap().unwrap();
        p.format.strip_trailing_zeros = false;
        p.numbering.enabled = false;
        p
    }

    /// The Masso profile as it is shipped writes as Fusion's Masso post does:
    /// blocks numbered in fives from N10, comments not, and numbers without
    /// their trailing zeros but with their point.
    #[test]
    fn masso_writes_as_fusions_post_does() {
        let p = Profile::builtin("masso").unwrap().unwrap();
        let g = program(&p);
        let lines: Vec<&str> = g.lines().collect();
        assert_eq!(lines[0], "(test)", "{g}");
        assert_eq!(lines[2], "N10 G90 G94 G17", "{g}");
        assert_eq!(lines[3], "N15 G21", "{g}");
        assert!(g.contains("G0 X0. Y0."), "{g}");
        assert!(g.contains(" Z10.\n"), "{g}");
        assert!(g.contains("X10. F1200."), "{g}");
        assert!(
            lines
                .iter()
                .filter(|l| l.starts_with('('))
                .all(|l| !l.contains('N')),
            "{g}"
        );
    }

    /// In inches, every length and feed is converted, and the program says G20.
    #[test]
    fn inches_convert_every_length_and_feed() {
        let p = Profile::builtin("masso-inch").unwrap().unwrap();
        let spec = ProgramSpec {
            units: crate::op::Units::Inches,
            ..ProgramSpec::default()
        };
        let mut t = Toolpath::new();
        t.rapid_to(dvec3(25.4, 50.8, 12.7)).unwrap();
        t.feed_to(dvec3(25.4, 50.8, 0.0), MoveRole::Plunge, 254.0)
            .unwrap();
        t.arc_to(
            dvec3(25.4 + 12.7, 50.8 + 12.7, 0.0),
            Arc {
                centre: dvec3(25.4, 50.8 + 12.7, 0.0),
                axis: DVec3::Z,
                sweep: FRAC_PI_2,
            },
            MoveRole::Cut,
            3810.0,
        )
        .unwrap();
        let ops = crate::op::lower(&t, &spec).unwrap();
        let (g, _) = render(&ops, &p, "t");
        assert!(g.contains(" G20\n"), "{g}");
        assert!(g.contains("G0 X1. Y2."), "{g}");
        assert!(g.contains("G1 Z0. F10."), "{g}");
        assert!(g.contains("G3 X1.5 Y2.5 I0. J0.5 F150."), "{g}");
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

    /// The tool goes up clear before the tool change, and the first move after
    /// it goes across at that height and then down, as Fusion's does: never
    /// diagonally from wherever the change left it. It states every word, as the
    /// machine has no remembered position to suppress against.
    #[test]
    fn the_first_move_goes_across_then_down() {
        let g = program(&masso());
        let lines: Vec<&str> = g.lines().collect();
        let at = |text: &str| lines.iter().position(|l| *l == text);
        let (Some(up), Some(change)) = (at("G28 G91 Z0"), at("T1 M6")) else {
            panic!("no safe retract before the tool change:\n{g}");
        };
        assert!(up < change, "{g}");
        let first = lines
            .iter()
            .position(|l| l.starts_with("G0"))
            .expect("a rapid should be present");
        assert_eq!(
            lines.get(first..first + 2),
            Some(&["G0 X0.000 Y0.000", "Z10.000"][..]),
            "{g}"
        );
    }

    /// Without a safe retract the height is unknown, so the first move rises (or
    /// falls) to the height the path crosses at before it goes across.
    #[test]
    fn unraised_the_first_move_goes_to_its_height_first() {
        let mut p = masso();
        p.safe_retract.clear();
        let g = program(&p);
        let lines: Vec<&str> = g.lines().collect();
        let first = lines.iter().position(|l| l.starts_with("G0")).unwrap();
        assert_eq!(
            lines.get(first..first + 2),
            Some(&["G0 Z10.000", "X0.000 Y0.000"][..]),
            "{g}"
        );
    }

    /// The program ends with the tool raised and the machine home across, after
    /// the spindle stops.
    #[test]
    fn the_program_ends_raised_and_home() {
        let g = program(&masso());
        let tail: Vec<&str> = g.lines().rev().take(6).collect();
        assert_eq!(
            tail,
            ["M30", "G90", "G28 G91 X0 Y0", "G90", "G28 G91 Z0", "M5"],
            "{g}"
        );
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

    /// Canned cycles render every word.
    #[test]
    fn cycles_render() {
        let ops = vec![
            Op::Units(Units::Millimetres),
            Op::Rapid {
                to: dvec3(0.0, 0.0, 0.0),
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
        let (g, _) = render(&ops, &masso(), "t");
        assert!(g.contains("G83"), "peck cycle missing:\n{g}");
        assert!(g.contains("Q1.000"));
        assert!(
            g.contains("X1.000 Y2.000"),
            "the hole's place missing:\n{g}"
        );
    }

    /// After a rapid to `from`, each arc, or a straight feed where there is none.
    fn arc_program(p: &Profile, from: DVec3, arcs: &[(DVec3, Option<Arc>)]) -> String {
        let role = camcraft_core::toolpath::MoveRole::Cut;
        let mut ops = vec![Op::Rapid { to: from }];
        for (to, arc) in arcs {
            ops.push(match arc {
                Some(arc) => Op::Arc {
                    to: *to,
                    arc: *arc,
                    feed: 600.0,
                    role,
                },
                None => Op::Feed {
                    to: *to,
                    feed: 600.0,
                    role,
                },
            });
        }
        ops.push(Op::SpindleOff);
        let (g, _) = render(&ops, p, "t");
        g.lines()
            .skip_while(|l| !l.starts_with("G0"))
            .skip(2)
            .map(str::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Fusion's lead into its lego contour, in inches as Fusion wrote it and
    /// written here in the same words: down a quarter turn in the YZ plane,
    /// then along the XY plane, which is selected again.
    #[test]
    fn a_vertical_lead_is_written_as_fusion_writes_it() {
        let down = Arc {
            centre: dvec3(4.86, 0.825, 0.065),
            axis: DVec3::X,
            sweep: FRAC_PI_2,
        };
        let on = Arc {
            centre: dvec3(4.835, 0.85, 0.04),
            axis: DVec3::Z,
            sweep: FRAC_PI_2,
        };
        let g = arc_program(
            &masso(),
            dvec3(4.86, 0.8, 0.065),
            &[
                (dvec3(4.86, 0.825, 0.04), Some(down)),
                (dvec3(4.86, 0.85, 0.04), None),
                (dvec3(4.835, 0.875, 0.04), Some(on)),
            ],
        );
        let lines: Vec<&str> = g.lines().collect();
        assert_eq!(
            lines[..3],
            [
                "G19 G3 Y0.825 Z0.040 J0.025 K0.000 F600.0",
                "G1 Y0.850",
                "G17 G3 X4.835 Y0.875 I-0.025 J0.000",
            ],
            "{g}"
        );
    }

    /// A helix is an arc in the XY plane that moves along Z; a full turn is
    /// written as two halves, which cannot be misread as no turn at all.
    #[test]
    fn a_helix_is_written_in_halves() {
        let turn = Arc {
            centre: dvec3(0.0, 0.0, 5.0),
            axis: DVec3::Z,
            sweep: -std::f64::consts::TAU,
        };
        let g = arc_program(
            &masso(),
            dvec3(2.0, 0.0, 5.0),
            &[(dvec3(2.0, 0.0, 4.0), Some(turn))],
        );
        let lines: Vec<&str> = g.lines().collect();
        assert_eq!(
            lines[..2],
            [
                "G17 G2 X-2.000 Y0.000 Z4.500 I-2.000 J0.000 F600.0",
                "G2 X2.000 Y0.000 Z4.000 I2.000 J0.000",
            ],
            "{g}"
        );
    }

    /// An arc about Y is in the XZ plane, and one that left another plane in
    /// force is put back to XY before the spindle stops.
    #[test]
    fn the_xz_plane_and_back() {
        // Clockwise about +Y from above the centre to its +X side: +Z turns to
        // +X under a positive turn about Y, so this is counter-clockwise, G3.
        let a = Arc {
            centre: DVec3::ZERO,
            axis: DVec3::Y,
            sweep: FRAC_PI_2,
        };
        let g = arc_program(
            &masso(),
            dvec3(0.0, 0.0, 1.0),
            &[(dvec3(1.0, 0.0, 0.0), Some(a))],
        );
        let lines: Vec<&str> = g.lines().collect();
        assert_eq!(
            lines[0], "G18 G3 X1.000 Z0.000 I0.000 K-1.000 F600.0",
            "{g}"
        );
        assert_eq!(lines[1..3], ["G17", "M5"], "{g}");
    }

    /// With radii, no piece is over a quarter turn, whose radius would need a
    /// sign to say which way round.
    #[test]
    fn radius_arcs_are_quarter_turns_at_most() {
        let mut p = masso();
        p.arc_style = ArcStyle::R;
        let half = Arc {
            centre: DVec3::ZERO,
            axis: DVec3::Z,
            sweep: PI,
        };
        let g = arc_program(
            &p,
            dvec3(3.0, 0.0, 0.0),
            &[(dvec3(-3.0, 0.0, 0.0), Some(half))],
        );
        let arcs: Vec<&str> = g.lines().filter(|l| l.contains("G3")).collect();
        assert_eq!(arcs.len(), 2, "{g}");
        assert!(
            arcs[0].contains("X0.000 Y3.000") && arcs[0].ends_with("R3.000 F600.0"),
            "{g}"
        );
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
        let after: String = g
            .lines()
            .skip_while(|l| *l != "G53 G0 Z0")
            .skip(1)
            .collect();
        for axis in ['X', 'Y', 'Z'] {
            assert!(after.contains(axis), "{axis} not restated:\n{g}");
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
        // The first, from nowhere known, is two blocks: down, then across.
        assert_eq!(report.motion_blocks, 2, "the second move is a no-op:\n{g}");
    }
}
