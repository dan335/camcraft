//! Rendering an instruction stream into one dialect.
//!
//! The renderer holds the only mutable machine state in the crate: what motion mode,
//! position and feed the machine has actually been told. Modal suppression is
//! decided against *that*, not against what the caller meant, because a word omitted
//! as "unchanged" is only safe if the machine already has the value.

use std::fmt::Write as _;

use std::f64::consts::{FRAC_PI_2, PI};

use camcraft_core::job::{Coolant, SpindleDirection};
use camcraft_core::path::toolpath::Arc;
use glam::{DVec2, DVec3};

use crate::op::{CycleKind, Op};
use crate::profile::{ArcStyle, Profile, ProgramUnits, ToolChangeStyle};

/// Arcs shorter than this across their chord, in millimetres, are written as
/// straight moves, as Fusion's posts write them (`minimumChordLength`): an arc
/// whose ends round to the same point reads as a full circle.
const SHORTEST_CHORD: f64 = 0.25;

/// How far the straight moves written for such an arc may stray from it, in
/// millimetres: Fusion's posts' `tolerance`.
const STRAIGHTENED: f64 = 0.002;

/// What rendering produced besides the text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderReport {
    /// Lines written.
    pub lines: u64,
    /// Motion blocks written.
    pub motion_blocks: u64,
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
    // The tool whose length offset the next Z word takes up, after a tool
    // change, where the profile says to.
    offset: Option<u32>,
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
            offset: None,
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
            Op::Units(u) => {
                self.raw(match u {
                    ProgramUnits::Mm => "G21",
                    ProgramUnits::Inch => "G20",
                });
                self.inches = *u == ProgramUnits::Inch;
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
            Op::ToolChange { tool, comment } => {
                if self.p.length_offset {
                    self.offset = Some(*tool);
                }
                self.tool_change(*tool, comment);
            }
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
            Op::CycleStart => {
                self.raw("G98");
                // The cycles move the machine: nothing is suppressed against
                // where it was.
                self.lost();
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

    /// Change to `tool`, by changer or by hand, as the profile says.
    fn tool_change(&mut self, tool: u32, comment: &str) {
        match self.p.tool_change {
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
            let z = self.z_word(&z);
            let (first, second) = if self.raised {
                (format!("G0 X{x} Y{y}"), format!("{again}{z}"))
            } else {
                (format!("G0 {z}"), format!("{again}X{x} Y{y}"))
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

        let push = |axis: char, val: &str, same: bool, block: &mut String| {
            if same && self.p.modal.suppress_axes {
                return;
            }
            if !block.is_empty() {
                block.push(' ');
            }
            let _ = write!(block, "{axis}{val}");
        };

        let same = |a: Option<&String>, b: &str| a.map(|s| s.as_str()) == Some(b);
        let z_same = same(prev.as_ref().map(|p| &p.2), &z);
        // A length offset waits for a Z word; after a tool change, the first
        // block states every axis, so it never waits long.
        let offset = if z_same && self.p.modal.suppress_axes {
            None
        } else {
            self.offset.take()
        };
        if offset.is_some() {
            if !block.is_empty() {
                block.push(' ');
            }
            block.push_str("G43");
        }
        push('X', &x, same(prev.as_ref().map(|p| &p.0), &x), &mut block);
        push('Y', &y, same(prev.as_ref().map(|p| &p.1), &y), &mut block);
        push('Z', &z, z_same, &mut block);
        if let Some(h) = offset {
            let _ = write!(block, " H{h}");
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
        self.motion = Some(g);
        self.at = Some(to);
    }

    /// A Z word, taking up the tool's length offset with it when one is
    /// waiting: `G43 Z… H<tool>`, as Fusion writes it.
    fn z_word(&mut self, z: &str) -> String {
        match self.offset.take() {
            Some(h) => format!("G43 Z{z} H{h}"),
            None => format!("Z{z}"),
        }
    }

    /// An arc, or a helix, from where the tool is, in the plane it turns in,
    /// written in pieces no longer than the profile's arcs may be: a half turn
    /// with centre offsets, which cannot be misread even as a full circle, and a
    /// quarter with a radius, whose sign would otherwise have to choose the long
    /// way round. A piece too short across to be written as an arc is written
    /// as straight moves along it.
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
            let across = (end - at) - arc.axis * (end - at).dot(arc.axis);
            if across.length() < SHORTEST_CHORD {
                let f = self.p.feed_in(feed, self.inches);
                for p in piece.chords(at, end, STRAIGHTENED) {
                    self.motion_block("G1", p, Some(f.clone()));
                }
            } else {
                self.arc_block(at, end, &piece, feed);
            }
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
        };
        // Every word every time: a cycle is modal, but a program read by a person
        // should say where each hole is and how deep, whatever came before. In
        // Fusion's order: where, then a peck or a dwell, then the feed.
        let mut block = format!(
            "{g} X{} Y{} Z{} R{}",
            self.p.length(at.x, self.inches),
            self.p.length(at.y, self.inches),
            self.p.length(depth, self.inches),
            self.p.length(retract, self.inches),
        );
        if let Some(q) = peck {
            let _ = write!(block, " Q{}", self.p.length(q, self.inches));
        }
        if let Some(p) = dwell {
            // Seconds, whatever the units.
            let _ = write!(block, " P{}", self.p.seconds(p));
        }
        let _ = write!(block, " F{}", self.p.feed_in(feed, self.inches));
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
mod tests;
