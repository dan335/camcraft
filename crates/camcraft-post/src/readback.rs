//! Reading a program back, as a controller would, and checking it against the
//! path it was posted from.
//!
//! The renderer's tests say what it means to write; this says what the text does.
//! [`read`] interprets the words a program holds — units, distance mode, planes,
//! arcs by centre or by radius, helices, drilling cycles, `G28` and `G53`,
//! length offsets, tool changes — into steps in millimetres, and refuses
//! anything it does not understand rather than step over it. [`check`] replays
//! those steps against the toolpath, or the holes, the program was made from:
//! every move reached in order, no cut turned into a rapid, feeds as asked, arcs
//! on the same curve turning the same way. And the program's safety: the tool
//! never goes across from a height it does not know, nor across and down at
//! once; the spindle is stopped for the tool change; the right tool, work
//! offset, length offset, spindle and coolant are in force before it cuts; and
//! after the last cut it only goes up, and across only once it is clear.
//!
//! It is written from the G-code standard (NIST RS274/NGC), not from the
//! renderer, so the two can disagree; where they do, one of them is wrong.
//!
//! Two things are taken as given, as Fusion's posts take them: machine Z0 is the
//! top of Z's travel, so `G53 Z0` raises the tool clear (as on a homed GRBL, and
//! Avid's CNC12, whose posts call it "Move to Z home"); and a tool change
//! leaves the tool no lower than it found it.

use std::f64::consts::TAU;
use std::fmt;

use camcraft_core::job::{Coolant, SpindleDirection};
use camcraft_core::path::toolpath::{Move, MoveKind, Toolpath};
use glam::{DVec2, DVec3, dvec3};

use crate::op::{DrillCycles, ProgramSpec};
use crate::profile::{Profile, ProgramUnits};

/// Millimetres in an inch.
const MM_PER_INCH: f64 = 25.4;

/// How far straight moves may stray from a short arc they are written for, in
/// millimetres, beyond rounding: Fusion's posts write an arc shorter than
/// 0.25 mm across that way, within their `tolerance` of 0.002 mm, as an arc
/// whose ends round together would read as a full circle.
const STRAIGHTENED: f64 = 0.002;

/// Where the machine is in work coordinates, an axis at a time: `None` where
/// that is not known, as after a tool change or a move in machine coordinates.
pub type Position = [Option<f64>; 3];

/// Something wrong with a program: what it says that cannot be read, or what
/// it does that it should not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The program line it is on, from one, where it is on one.
    pub line: Option<usize>,
    /// What is wrong.
    pub what: String,
}

impl Problem {
    fn at(line: usize, what: impl Into<String>) -> Self {
        Self {
            line: Some(line),
            what: what.into(),
        }
    }

    fn whole(what: impl Into<String>) -> Self {
        Self {
            line: None,
            what: what.into(),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "line {line}: {}", self.what),
            None => f.write_str(&self.what),
        }
    }
}

impl std::error::Error for Problem {}

/// One thing a program does, and the line it does it on.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// The program line, from one.
    pub line: usize,
    /// What it does.
    pub action: Action,
}

/// What a program does.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// A move in work coordinates.
    Move(Motion),
    /// Axes sent home (`G28`); where that is in work coordinates is not known.
    Home([bool; 3]),
    /// Axes sent to a place in machine coordinates (`G53`), in millimetres;
    /// where that is in work coordinates is not known.
    Machine([Option<f64>; 3]),
    /// A tool selected (`T`) without a change.
    Select(u32),
    /// A tool change (`M6`) to the tool selected.
    Change(u32),
    /// The program stops until the operator starts it again (`M0`, `M1`).
    Pause,
    /// The spindle started (`M3`, `M4`).
    SpindleOn {
        /// Speed, rev/min.
        rpm: u32,
        /// Which way.
        dir: SpindleDirection,
    },
    /// The spindle stopped (`M5`).
    SpindleOff,
    /// Coolant on (`M7` mist, `M8` flood), or off (`M9`).
    Coolant(Coolant),
    /// A work offset selected, 54 for `G54`.
    WorkOffset(u8),
    /// A tool's length offset taken up (`G43 H`), or let go (`G49`, `None`).
    LengthOffset(Option<u32>),
    /// A pause, in seconds (`G4`).
    Dwell(f64),
    /// The end of the program (`M2`, `M30`).
    End,
}

/// A move in work coordinates, in millimetres.
#[derive(Debug, Clone, PartialEq)]
pub struct Motion {
    /// Where it starts.
    pub from: Position,
    /// Where it ends.
    pub to: Position,
    /// Feed in mm/min, or `None` for a rapid.
    pub feed: Option<f64>,
    /// The arc it turns, or `None` for a straight move.
    pub arc: Option<Turn>,
}

/// An arc or a helix, about an axis square to the plane it turns in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Turn {
    /// Its centre, in the plane, level with its start along `normal`.
    pub centre: DVec3,
    /// The plane's normal: Z for `G17`, Y for `G18`, X for `G19`.
    pub normal: DVec3,
    /// How far it turns, in radians, right-handed about `normal`: positive
    /// for `G3`, negative for `G2`.
    pub sweep: f64,
}

/// `v` less its part along the unit `n`.
fn flat(v: DVec3, n: DVec3) -> DVec3 {
    v - n * v.dot(n)
}

/// `v` turned `angle` about the unit `n`; `v` square to it.
fn turned(v: DVec3, n: DVec3, angle: f64) -> DVec3 {
    let (s, c) = angle.sin_cos();
    v * c + n.cross(v) * s
}

impl Turn {
    /// The point a fraction `u` of the way round, from `from` to `to`. Ends at
    /// different distances from the centre, as rounding leaves them, are
    /// joined by a spiral, as controllers join them.
    fn point_at(&self, from: DVec3, to: DVec3, u: f64) -> DVec3 {
        let n = self.normal;
        let (r0, r1) = (flat(from - self.centre, n), flat(to - self.centre, n));
        let (l0, l1) = (r0.length(), r1.length());
        let radius = l0 + (l1 - l0) * u;
        let along = from.dot(n) + (to.dot(n) - from.dot(n)) * u;
        flat(self.centre, n) + turned(r0 / l0, n, self.sweep * u) * radius + n * along
    }

    fn length(&self, from: DVec3, to: DVec3) -> f64 {
        let n = self.normal;
        let (l0, l1) = (
            flat(from - self.centre, n).length(),
            flat(to - self.centre, n).length(),
        );
        (0.5 * (l0 + l1) * self.sweep.abs()).hypot((to - from).dot(n))
    }
}

/// Every axis of `p`, where every one is known.
fn known(p: &Position) -> Option<DVec3> {
    match *p {
        [Some(x), Some(y), Some(z)] => Some(dvec3(x, y, z)),
        _ => None,
    }
}

/// Whether axis `i` moves from `a` to `b`: from unknown to known, or between
/// two values.
fn moves(a: &Position, b: &Position, i: usize) -> bool {
    match (a.get(i).copied().flatten(), b.get(i).copied().flatten()) {
        (None, Some(_)) => true,
        (Some(p), Some(q)) => (p - q).abs() > 0.0,
        _ => false,
    }
}

impl Motion {
    /// Where it starts and ends, where both are known.
    fn ends(&self) -> Option<(DVec3, DVec3)> {
        Some((known(&self.from)?, known(&self.to)?))
    }

    fn length(&self) -> f64 {
        match (self.ends(), self.arc) {
            (Some((a, b)), Some(t)) => t.length(a, b),
            (Some((a, b)), None) => a.distance(b),
            (None, _) => 0.0,
        }
    }

    fn point_at(&self, u: f64) -> Option<DVec3> {
        let (a, b) = self.ends()?;
        Some(match self.arc {
            Some(t) => t.point_at(a, b, u),
            None => a.lerp(b, u),
        })
    }
}

/// What a program does, step by step, in millimetres: read as a controller
/// reads it. Refused, with the line, where it says something this cannot
/// read, or something no controller could do, such as a move before its units
/// are set.
pub fn read(text: &str) -> Result<Vec<Step>, Problem> {
    let mut r = Reader::default();
    for (i, line) in text.lines().enumerate() {
        r.line = i + 1;
        let words = words(line).map_err(|what| Problem::at(r.line, what))?;
        r.block(&words).map_err(|what| Problem::at(r.line, what))?;
    }
    Ok(r.steps)
}

/// The words of a line, comments and line numbers left out.
fn words(line: &str) -> Result<Vec<(char, f64)>, String> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '(' => loop {
                match chars.next() {
                    Some(')') => break,
                    Some(_) => {}
                    None => return Err("a comment is never closed".into()),
                }
            },
            ';' => break,
            '%' => {}
            c if c.is_whitespace() => {}
            c if c.is_ascii_alphabetic() => {
                let mut number = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() || matches!(d, '.' | '-' | '+') {
                        number.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let value: f64 = number
                    .parse()
                    .map_err(|_| format!("{c}{number} is not a word"))?;
                out.push((c.to_ascii_uppercase(), value));
            }
            c => return Err(format!("{c:?} is not part of any word")),
        }
    }
    Ok(out)
}

/// A whole number from a word, such as a tool or a speed.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn whole(letter: char, v: f64) -> Result<u32, String> {
    if v.fract() != 0.0 || !(0.0..=f64::from(u32::MAX)).contains(&v) {
        return Err(format!("{letter}{v} is not a whole number"));
    }
    Ok(v as u32)
}

/// The words of one block, gathered.
#[derive(Default)]
struct Block {
    g: Vec<u32>,
    m: Vec<u32>,
    axes: [Option<f64>; 3],
    ijk: [Option<f64>; 3],
    r: Option<f64>,
    q: Option<f64>,
    p: Option<f64>,
    f: Option<f64>,
    s: Option<f64>,
    t: Option<f64>,
    h: Option<f64>,
}

impl Block {
    fn gather(words: &[(char, f64)]) -> Result<Self, String> {
        let mut b = Self::default();
        for &(letter, v) in words {
            let slot = match letter {
                'N' => continue,
                'G' => {
                    b.g.push(whole('G', v)?);
                    continue;
                }
                'M' => {
                    b.m.push(whole('M', v)?);
                    continue;
                }
                'X' => &mut b.axes[0],
                'Y' => &mut b.axes[1],
                'Z' => &mut b.axes[2],
                'I' => &mut b.ijk[0],
                'J' => &mut b.ijk[1],
                'K' => &mut b.ijk[2],
                'R' => &mut b.r,
                'Q' => &mut b.q,
                'P' => &mut b.p,
                'F' => &mut b.f,
                'S' => &mut b.s,
                'T' => &mut b.t,
                'H' => &mut b.h,
                other => return Err(format!("{other} words are not understood")),
            };
            if slot.is_some() {
                return Err(format!("{letter} appears twice"));
            }
            *slot = Some(v);
        }
        Ok(b)
    }

    fn has_g(&self, code: u32) -> bool {
        self.g.contains(&code)
    }

    fn has_m(&self, code: u32) -> bool {
        self.m.contains(&code)
    }
}

/// The settings that last only through one run of drilling cycles.
#[derive(Default)]
struct Cycle {
    /// The height the tool was at when the run began, for `G98`.
    initial: Option<f64>,
    z: Option<f64>,
    r: Option<f64>,
    q: Option<f64>,
    p: Option<f64>,
}

/// A controller's state as the program leaves it.
struct Reader {
    steps: Vec<Step>,
    line: usize,
    /// `G20` (true) or `G21`, once set.
    inches: Option<bool>,
    /// `G90` (true) or `G91`, once set.
    absolute: Option<bool>,
    /// 17, 18 or 19.
    plane: u32,
    /// 0, 1, 2, 3, 81, 82 or 83, once set; none after `G80`.
    motion: Option<u32>,
    /// mm/min.
    feed: Option<f64>,
    speed: Option<f64>,
    at: Position,
    selected: Option<u32>,
    wcs: Option<u8>,
    /// `G98` (true) or `G99`.
    to_initial: bool,
    cycle: Cycle,
}

impl Default for Reader {
    fn default() -> Self {
        Self {
            steps: Vec::new(),
            line: 0,
            inches: None,
            absolute: None,
            plane: 17,
            motion: None,
            feed: None,
            speed: None,
            at: [None; 3],
            selected: None,
            wcs: None,
            to_initial: true,
            cycle: Cycle::default(),
        }
    }
}

impl Reader {
    fn push(&mut self, action: Action) {
        self.steps.push(Step {
            line: self.line,
            action,
        });
    }

    /// Millimetres in one of the program's units.
    fn scale(&self) -> Result<f64, String> {
        match self.inches {
            Some(true) => Ok(MM_PER_INCH),
            Some(false) => Ok(1.0),
            None => Err("a length before the units are set (G20 or G21)".into()),
        }
    }

    fn block(&mut self, words: &[(char, f64)]) -> Result<(), String> {
        let b = Block::gather(words)?;
        let motions =
            b.g.iter()
                .filter(|g| matches!(g, 0 | 1 | 2 | 3 | 80 | 81 | 82 | 83))
                .count();
        if motions > 1 {
            return Err("two motions in one block".into());
        }
        for &g in &b.g {
            match g {
                0 | 1 | 2 | 3 | 81 | 82 | 83 => self.motion = Some(g),
                80 => {
                    self.motion = None;
                    self.cycle = Cycle::default();
                }
                17..=19 => self.plane = g,
                20 => self.inches = Some(true),
                21 => self.inches = Some(false),
                90 => self.absolute = Some(true),
                91 => self.absolute = Some(false),
                98 => self.to_initial = true,
                99 => self.to_initial = false,
                4 | 28 | 40 | 43 | 49 | 53 | 94 => {}
                54..=59 => {}
                other => return Err(format!("G{other} is not understood")),
            }
        }
        if !matches!(self.motion, Some(81..=83)) {
            self.cycle = Cycle::default();
        }
        if let Some(f) = b.f {
            self.feed = Some(f * self.scale()?);
        }
        if let Some(s) = b.s {
            self.speed = Some(s);
        }
        if let Some(t) = b.t {
            let t = whole('T', t)?;
            self.selected = Some(t);
            if !b.has_m(6) {
                self.push(Action::Select(t));
            }
        }
        for &m in &b.m {
            match m {
                6 => {
                    let t = self.selected.ok_or("M6 with no tool selected")?;
                    self.push(Action::Change(t));
                    // A changer moves the machine.
                    self.at = [None; 3];
                }
                3 | 4 => {
                    let s = self.speed.ok_or("the spindle started with no speed")?;
                    let rpm = whole('S', s.round())?;
                    let dir = if m == 3 {
                        SpindleDirection::Clockwise
                    } else {
                        SpindleDirection::CounterClockwise
                    };
                    self.push(Action::SpindleOn { rpm, dir });
                }
                5 => self.push(Action::SpindleOff),
                7 => self.push(Action::Coolant(Coolant::Mist)),
                8 => self.push(Action::Coolant(Coolant::Flood)),
                9 => self.push(Action::Coolant(Coolant::Off)),
                0 | 1 | 2 | 30 => {}
                other => return Err(format!("M{other} is not understood")),
            }
        }
        if b.has_g(4) {
            let p = b.p.ok_or("G4 with no time")?;
            self.push(Action::Dwell(p));
        }
        if b.has_g(49) {
            self.push(Action::LengthOffset(None));
            // The same machine position is now at another Z.
            self.at[2] = None;
        }
        if b.has_g(43) {
            let h = whole('H', b.h.ok_or("G43 with no H")?)?;
            self.push(Action::LengthOffset(Some(h)));
            self.at[2] = None;
        }
        if let Some(&g) = b.g.iter().find(|g| (54..=59).contains(*g)) {
            let n = u8::try_from(g).map_err(|_| "a work offset out of range")?;
            if self.wcs != Some(n) {
                self.at = [None; 3];
            }
            self.wcs = Some(n);
            self.push(Action::WorkOffset(n));
        }

        let has_axes = b.axes.iter().any(Option::is_some);
        if b.has_g(28) {
            self.home(&b)?;
        } else if b.has_g(53) {
            self.machine(&b)?;
        } else if has_axes || (b.r.is_some() && matches!(self.motion, Some(81..=83))) {
            match self.motion {
                Some(0) => self.line_to(&b, None)?,
                Some(1) => {
                    let f = self.feed.ok_or("a feed move with no feed")?;
                    self.line_to(&b, Some(f))?;
                }
                Some(g @ (2 | 3)) => self.arc_to(&b, g == 3)?,
                Some(g @ 81..=83) => self.drill(&b, g)?,
                _ => return Err("axis words with no motion in force".into()),
            }
        } else if b.ijk.iter().any(Option::is_some) || b.r.is_some() {
            return Err("arc words with nowhere to go".into());
        }

        for &m in &b.m {
            match m {
                0 | 1 => {
                    self.push(Action::Pause);
                    // The operator may jog anywhere while it waits.
                    self.at = [None; 3];
                }
                2 | 30 => self.push(Action::End),
                _ => {}
            }
        }
        Ok(())
    }

    /// Where a block's axis words send the tool, in work coordinates.
    fn target(&self, axes: &[Option<f64>; 3]) -> Result<Position, String> {
        let scale = self.scale()?;
        let absolute = self
            .absolute
            .ok_or("a move before the distance mode is set (G90 or G91)")?;
        let mut to = self.at;
        for ((slot, word), now) in to.iter_mut().zip(axes).zip(self.at) {
            let Some(v) = word else { continue };
            *slot = if absolute {
                Some(v * scale)
            } else {
                match now {
                    Some(now) => Some(now + v * scale),
                    None if *v == 0.0 => None,
                    None => return Err("an incremental move from where is not known".into()),
                }
            };
        }
        Ok(to)
    }

    fn go(&mut self, to: Position, feed: Option<f64>, arc: Option<Turn>) {
        self.push(Action::Move(Motion {
            from: self.at,
            to,
            feed,
            arc,
        }));
        self.at = to;
    }

    /// `G28`: across to the point the axis words give, then those axes home.
    fn home(&mut self, b: &Block) -> Result<(), String> {
        let named = b.axes.map(|a| a.is_some());
        if !named.iter().any(|n| *n) {
            return Err("G28 with no axes to send home".into());
        }
        let via = self.target(&b.axes)?;
        if via != self.at {
            self.go(via, None, None);
        }
        self.push(Action::Home(named));
        for (at, n) in self.at.iter_mut().zip(named) {
            if n {
                *at = None;
            }
        }
        Ok(())
    }

    /// `G53`: a straight move in machine coordinates.
    fn machine(&mut self, b: &Block) -> Result<(), String> {
        if !matches!(self.motion, Some(0 | 1)) {
            return Err("G53 without G0 or G1".into());
        }
        if self.absolute != Some(true) {
            return Err("G53 in incremental mode".into());
        }
        let scale = self.scale()?;
        let to = b.axes.map(|a| a.map(|v| v * scale));
        self.push(Action::Machine(to));
        for (at, a) in self.at.iter_mut().zip(to) {
            if a.is_some() {
                *at = None;
            }
        }
        Ok(())
    }

    fn line_to(&mut self, b: &Block, feed: Option<f64>) -> Result<(), String> {
        let to = self.target(&b.axes)?;
        self.go(to, feed, None);
        Ok(())
    }

    /// The plane's normal and its two axes with their centre-offset words, as
    /// indices: X0 Y1 Z2, I0 J1 K2.
    fn plane_axes(&self) -> (DVec3, [usize; 2]) {
        match self.plane {
            18 => (DVec3::Y, [2, 0]),
            19 => (DVec3::X, [1, 2]),
            _ => (DVec3::Z, [0, 1]),
        }
    }

    fn arc_to(&mut self, b: &Block, ccw: bool) -> Result<(), String> {
        let feed = self.feed.ok_or("an arc with no feed")?;
        let from = known(&self.at).ok_or("an arc from where is not known")?;
        let to = self.target(&b.axes)?;
        let to_p = known(&to).ok_or("an arc to where is not known")?;
        let (n, axes) = self.plane_axes();
        let scale = self.scale()?;
        let centre = if let Some(r) = b.r {
            if b.ijk.iter().any(Option::is_some) {
                return Err("an arc given both a radius and a centre".into());
            }
            let r = r * scale;
            let chord = flat(to_p - from, n);
            let half = chord.length() / 2.0;
            if half == 0.0 {
                return Err("a radius arc whose ends are the same point".into());
            }
            let h2 = r * r - half * half;
            if h2 < -1.0e-6 * r * r {
                return Err("a radius too small to reach between the arc's ends".into());
            }
            // Left of the chord, seen from the normal's tip, for the short way
            // round counter-clockwise; the long way (a negative radius) or
            // clockwise puts it on the right.
            let side = if (r > 0.0) == ccw { 1.0 } else { -1.0 };
            from + chord / 2.0 + n.cross(chord / chord.length()) * h2.max(0.0).sqrt() * side
        } else {
            let mut c = from;
            for (i, offset) in b.ijk.iter().enumerate() {
                let Some(offset) = offset else { continue };
                if !axes.contains(&i) {
                    return Err(format!(
                        "{} is not a centre offset in G{}",
                        ['I', 'J', 'K'].get(i).copied().unwrap_or('?'),
                        self.plane
                    ));
                }
                c[i] += offset * scale;
            }
            c
        };
        let (r0, r1) = (flat(from - centre, n), flat(to_p - centre, n));
        if r0.length() == 0.0 || r1.length() == 0.0 {
            return Err("an arc with an end at its centre".into());
        }
        let angle = n.dot(r0.cross(r1)).atan2(r0.dot(r1));
        let sweep = match (ccw, angle) {
            (true, a) if a <= 0.0 => a + TAU,
            (false, a) if a >= 0.0 => a - TAU,
            (_, a) => a,
        };
        let turn = Turn {
            centre: flat(centre, n) + n * from.dot(n),
            normal: n,
            sweep,
        };
        self.go(to, Some(feed), Some(turn));
        Ok(())
    }

    /// One hole of a run of drilling cycles, in the XY plane: the moves the
    /// standard gives them. `G83` pecks back to R between pecks, and rapids
    /// back down to the depth already drilled, the deepest any controller
    /// goes.
    fn drill(&mut self, b: &Block, g: u32) -> Result<(), String> {
        if self.absolute != Some(true) {
            return Err("drilling cycles in incremental mode are not understood".into());
        }
        if self.plane != 17 {
            return Err("drilling cycles out of the XY plane are not understood".into());
        }
        let scale = self.scale()?;
        let feed = self.feed.ok_or("a drilling cycle with no feed")?;
        let [word_x, word_y, word_z] = b.axes;
        let c = &mut self.cycle;
        if let Some(z) = word_z {
            c.z = Some(z * scale);
        }
        if let Some(r) = b.r {
            c.r = Some(r * scale);
        }
        if let Some(q) = b.q {
            c.q = Some(q * scale);
        }
        if let Some(p) = b.p {
            c.p = Some(p);
        }
        let bottom = c.z.ok_or("a drilling cycle with no depth")?;
        let r = c.r.ok_or("a drilling cycle with no R")?;
        let peck = c.q;
        let dwell = c.p;
        let now = self.at[2].ok_or("a drilling cycle from a height not known")?;
        let initial = *c.initial.get_or_insert(now);
        let x = match word_x {
            Some(x) => Some(x * scale),
            None => self.at[0],
        };
        let y = match word_y {
            Some(y) => Some(y * scale),
            None => self.at[1],
        };
        if x.is_none() || y.is_none() {
            return Err("a drilling cycle at a place not known".into());
        }
        if now < r {
            self.go([self.at[0], self.at[1], Some(r)], None, None);
        }
        let z = self.at[2];
        self.go([x, y, z], None, None);
        self.go([x, y, Some(r)], None, None);
        match g {
            83 => {
                let q = peck.filter(|q| *q > 0.0).ok_or("G83 with no peck")?;
                let mut depth = r;
                while depth > bottom {
                    if depth < r {
                        self.go([x, y, Some(depth)], None, None);
                    }
                    let next = (depth - q).max(bottom);
                    self.go([x, y, Some(next)], Some(feed), None);
                    depth = next;
                    if depth > bottom {
                        self.go([x, y, Some(r)], None, None);
                    }
                }
            }
            _ => {
                self.go([x, y, Some(bottom)], Some(feed), None);
                if g == 82 {
                    self.push(Action::Dwell(dwell.unwrap_or(0.0)));
                }
            }
        }
        let clear = if self.to_initial { initial.max(r) } else { r };
        self.go([x, y, Some(clear)], None, None);
        Ok(())
    }
}

/// How many points to compare along a move: the end and the middle of a
/// straight one, and eight a quarter turn along an arc.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn samples(sweep: Option<f64>) -> u32 {
    let quarters = sweep.map_or(0.0, |s| (s / std::f64::consts::FRAC_PI_2).ceil().min(8.0));
    2 + 8 * quarters as u32
}

/// How near the read-back must come: the rounding of the profile's words.
#[derive(Debug, Clone, Copy)]
struct Tolerance {
    /// Millimetres, for positions.
    at: f64,
    /// mm/min, for feeds.
    feed: f64,
}

impl Tolerance {
    /// An axis word is out by up to half its last place; a point inside an
    /// arc, whose centre is a rounded start plus a rounded offset, by a few of
    /// those. Six is ample for that and far short of any real fault.
    fn of(profile: &Profile, units: ProgramUnits) -> Self {
        let (places, scale) = match units {
            ProgramUnits::Mm => (profile.format.axis_decimals, 1.0),
            ProgramUnits::Inch => (profile.format.inch_axis_decimals, MM_PER_INCH),
        };
        let half = |places: u8| 0.5 * 10f64.powi(-i32::from(places)) * scale;
        Self {
            at: 6.0 * half(places) + 1.0e-9,
            feed: half(profile.format.feed_decimals) * 1.0001 + 1.0e-9,
        }
    }
}

/// Check that `text`, read back, does what posting `path` with `spec` for
/// `profile` should: as the module says. The first thing found wrong is
/// returned, with the program line it is on.
pub fn check(
    text: &str,
    path: &Toolpath,
    spec: &ProgramSpec,
    profile: &Profile,
) -> Result<(), Problem> {
    let steps = read(text)?;
    let mut c = Checker::new(&steps, spec, Tolerance::of(profile, spec.units));
    if let Some(d) = &spec.drill {
        c.holes(d)?;
    } else if let Some(first) = path.moves().first() {
        c.arrive(first.to, first.feed.is_none())?;
        c.phase = Phase::Cutting;
        for (k, pair) in path.moves().windows(2).enumerate() {
            if let [before, m] = pair {
                c.follow(before.to, m, k + 1)?;
            }
        }
    }
    c.phase = Phase::After;
    c.leave()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Before,
    Cutting,
    After,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Spindle {
    Unknown,
    Off,
    On(u32, SpindleDirection),
}

struct Checker<'a> {
    steps: &'a [Step],
    next: usize,
    spec: &'a ProgramSpec,
    tol: Tolerance,
    phase: Phase,
    at: Position,
    /// Whether Z is known to be at the top of its travel.
    raised: bool,
    /// The highest the path goes.
    high: f64,
    tool: Option<u32>,
    selected: Option<u32>,
    wcs: Option<u8>,
    spindle: Spindle,
    coolant: Option<Coolant>,
    cut: bool,
    dwells: Vec<f64>,
    ended: bool,
}

impl<'a> Checker<'a> {
    fn new(steps: &'a [Step], spec: &'a ProgramSpec, tol: Tolerance) -> Self {
        Self {
            steps,
            next: 0,
            spec,
            tol,
            phase: Phase::Before,
            at: [None; 3],
            raised: false,
            high: f64::NEG_INFINITY,
            tool: None,
            selected: None,
            wcs: None,
            spindle: Spindle::Unknown,
            coolant: None,
            cut: false,
            dwells: Vec::new(),
            ended: false,
        }
    }

    /// The next move, after taking in whatever comes before it; `None` at the
    /// end of the program.
    fn next_move(&mut self) -> Result<Option<(usize, &'a Motion)>, Problem> {
        while let Some(step) = self.steps.get(self.next) {
            self.next += 1;
            if self.ended {
                return Err(Problem::at(step.line, "something after the program's end"));
            }
            match &step.action {
                Action::Move(m) => {
                    if self.wcs != Some(self.spec.wcs) || self.tool != Some(self.spec.tool) {
                        return Err(Problem::at(
                            step.line,
                            format!(
                                "moves before tool {} is in and G{} is selected",
                                self.spec.tool, self.spec.wcs
                            ),
                        ));
                    }
                    return Ok(Some((step.line, m)));
                }
                other => self.take(step.line, other)?,
            }
        }
        Ok(None)
    }

    fn take(&mut self, line: usize, action: &Action) -> Result<(), Problem> {
        let cutting = self.phase == Phase::Cutting;
        let refuse = |what: &str| {
            Err(Problem::at(
                line,
                format!("{what} in the middle of the path"),
            ))
        };
        match action {
            Action::Move(_) => {}
            Action::Home(axes) => {
                if cutting {
                    return refuse("goes home");
                }
                self.machine_move(line, [axes[0], axes[1]], axes[2])?;
                for (at, a) in self.at.iter_mut().zip(axes) {
                    if *a {
                        *at = None;
                    }
                }
            }
            Action::Machine(to) => {
                if cutting {
                    return refuse("moves in machine coordinates");
                }
                let xy = [to[0].is_some(), to[1].is_some()];
                self.machine_move(line, xy, to[2].is_some_and(|z| z == 0.0))?;
                if to[2].is_some_and(|z| z != 0.0) {
                    self.raised = false;
                }
                for (at, a) in self.at.iter_mut().zip(to) {
                    if a.is_some() {
                        *at = None;
                    }
                }
            }
            Action::Select(t) => {
                if cutting {
                    return refuse("selects a tool");
                }
                self.selected = Some(*t);
            }
            Action::Change(t) => {
                if cutting {
                    return refuse("changes the tool");
                }
                if self.spindle != Spindle::Off {
                    return Err(Problem::at(
                        line,
                        "changes the tool without stopping the spindle",
                    ));
                }
                self.tool = Some(*t);
                self.at = [None; 3];
            }
            Action::Pause => {
                if cutting {
                    return refuse("stops");
                }
                if self.selected.is_some() && self.spindle != Spindle::Off {
                    return Err(Problem::at(
                        line,
                        "stops for a tool with the spindle running",
                    ));
                }
                if self.selected.is_some() {
                    self.tool = self.selected;
                }
                self.at = [None; 3];
                self.raised = false;
            }
            Action::SpindleOn { rpm, dir } => {
                if cutting {
                    return refuse("starts the spindle");
                }
                self.spindle = Spindle::On(*rpm, *dir);
            }
            Action::SpindleOff => {
                if cutting {
                    return refuse("stops the spindle");
                }
                self.spindle = Spindle::Off;
            }
            Action::Coolant(c) => {
                if cutting {
                    return refuse("turns the coolant");
                }
                self.coolant = Some(*c);
            }
            Action::WorkOffset(n) => {
                if cutting {
                    return refuse("changes the work offset");
                }
                self.wcs = Some(*n);
                self.at = [None; 3];
            }
            Action::LengthOffset(h) => {
                if cutting {
                    return refuse("changes the length offset");
                }
                if let Some(h) = h
                    && *h != self.spec.tool
                {
                    return Err(Problem::at(
                        line,
                        format!("takes up tool {h}'s length with tool {} in", self.spec.tool),
                    ));
                }
                self.at[2] = None;
            }
            Action::Dwell(s) => self.dwells.push(*s),
            Action::End => {
                if self.phase != Phase::After {
                    return Err(Problem::at(line, "the program ends before the path does"));
                }
                if self.spindle != Spindle::Off {
                    return Err(Problem::at(line, "the program ends with the spindle on"));
                }
                if self.coolant.is_some_and(|c| c != Coolant::Off) {
                    return Err(Problem::at(line, "the program ends with the coolant on"));
                }
                self.ended = true;
            }
        }
        Ok(())
    }

    /// A move in machine coordinates: across only once the tool is clear.
    fn machine_move(&mut self, line: usize, xy: [bool; 2], raises: bool) -> Result<(), Problem> {
        if raises {
            self.raised = true;
        }
        if xy.iter().any(|a| *a) && !self.clear() {
            return Err(Problem::at(
                line,
                "goes across in machine coordinates before the tool is clear",
            ));
        }
        Ok(())
    }

    /// Whether the tool is high enough to go across: at the top of its travel,
    /// or as high as the path goes.
    fn clear(&self) -> bool {
        self.raised || self.at[2].is_some_and(|z| z >= self.high - self.tol.at)
    }

    /// Before the first cut, the spindle and coolant must be as asked.
    fn cutting(&mut self, line: usize) -> Result<(), Problem> {
        if self.cut {
            return Ok(());
        }
        if self.spindle != Spindle::On(self.spec.rpm, self.spec.spin) {
            return Err(Problem::at(
                line,
                format!(
                    "cuts before the spindle turns {:?} at {} rev/min",
                    self.spec.spin, self.spec.rpm
                ),
            ));
        }
        if self.coolant.unwrap_or(Coolant::Off) != self.spec.coolant {
            return Err(Problem::at(
                line,
                format!("cuts with the coolant not {:?}", self.spec.coolant),
            ));
        }
        self.cut = true;
        Ok(())
    }

    /// From wherever the tool is to where the path starts, one way at a time
    /// while the tool's place is not known, and across only once it is clear.
    fn arrive(&mut self, to: DVec3, rapid: bool) -> Result<(), Problem> {
        loop {
            let (line, m) = self.next_move()?.ok_or_else(|| {
                Problem::whole(format!(
                    "the program ends before reaching the path's start, {to}"
                ))
            })?;
            if m.feed.is_none() != rapid {
                return Err(Problem::at(
                    line,
                    "a rapid where the path feeds, or a feed where it rapids",
                ));
            }
            if !rapid {
                self.cutting(line)?;
            }
            let across = moves(&m.from, &m.to, 0) || moves(&m.from, &m.to, 1);
            let down = moves(&m.from, &m.to, 2);
            if m.from.iter().any(Option::is_none) && across && down {
                return Err(Problem::at(
                    line,
                    "goes across and down at once from a place not known",
                ));
            }
            if across && m.from[2].is_none() && !self.raised {
                return Err(Problem::at(line, "goes across at a height not known"));
            }
            if down {
                self.raised = false;
            }
            self.at = m.to;
            if let Some(at) = known(&self.at) {
                if at.distance(to) > self.tol.at {
                    return Err(Problem::at(
                        line,
                        format!("goes to {at} on its way to the path's start, {to}"),
                    ));
                }
                self.high = to.z;
                return Ok(());
            }
        }
    }

    /// Move `k` of the path, from `from`: reached, by the same kind of move at
    /// the same feed, along the same curve. An arc may be written as straight
    /// moves along it.
    fn follow(&mut self, from: DVec3, m: &Move, k: usize) -> Result<(), Problem> {
        let source = match m.kind {
            MoveKind::Arc(a) => Some(a),
            MoveKind::Linear => None,
        };
        let sweep = source.map(|a| a.sweep.abs());
        let tol = self.tol.at;
        let mut group: Vec<(usize, &'a Motion)> = Vec::new();
        loop {
            let Some(at) = known(&self.at) else {
                return Err(Problem::whole(format!(
                    "loses its place before move {k} of the path"
                )));
            };
            let reached = at.distance(m.to) <= tol;
            // How far round the path's arc the moves so far have gone.
            let swept: f64 = group
                .iter()
                .map(|(_, g)| match (g.arc, source, g.ends()) {
                    (Some(t), _, _) => t.sweep.abs(),
                    (None, Some(a), Some((p, q))) => {
                        let (ra, rb) = (flat(p - a.centre, a.axis), flat(q - a.centre, a.axis));
                        a.axis.dot(ra.cross(rb)).atan2(ra.dot(rb)).abs()
                    }
                    (None, _, _) => 0.0,
                })
                .sum();
            // An arc's ends may coincide, so reaching its end is not enough,
            // unless it is too short to be written at all.
            let begun = !group.is_empty() || m.length(from) <= tol;
            match sweep {
                None if reached => break,
                Some(s) if reached && begun && swept > s - 0.5 => break,
                _ => {}
            }
            let limit = if sweep.is_some() { 64 } else { 1 };
            if group.len() >= limit {
                let line = group.last().map_or(0, |(l, _)| *l);
                return Err(Problem::at(
                    line,
                    format!("goes to {at} where move {k} of the path goes to {}", m.to),
                ));
            }
            let (line, g) = self.next_move()?.ok_or_else(|| {
                Problem::whole(format!(
                    "the program ends before move {k} of the path, to {}",
                    m.to
                ))
            })?;
            let Some((_, end)) = g.ends() else {
                return Err(Problem::at(
                    line,
                    "loses its place in the middle of the path",
                ));
            };
            self.at = g.to;
            // A straight move that stays where the tool is, as rounding can
            // leave one, is nothing.
            if group.is_empty() && g.arc.is_none() && end.distance(from) <= tol {
                continue;
            }
            group.push((line, g));
        }
        let mut length = 0.0;
        for &(line, g) in &group {
            match (g.feed, m.feed) {
                (None, Some(_)) => return Err(Problem::at(line, "a rapid where the path cuts")),
                (Some(_), None) => return Err(Problem::at(line, "a feed where the path rapids")),
                (Some(a), Some(b)) => {
                    self.cutting(line)?;
                    if (a - b).abs() > self.tol.feed {
                        return Err(Problem::at(
                            line,
                            format!("feeds at {a:.3} mm/min where the path feeds at {b:.3}"),
                        ));
                    }
                }
                (None, None) => {}
            }
            if g.arc.is_some() && sweep.is_none() {
                return Err(Problem::at(line, "an arc where the path goes straight"));
            }
            length += g.length();
        }
        // The same curve, at the same pace along it.
        let straightened = sweep.is_some() && group.iter().any(|(_, g)| g.arc.is_none());
        let tol = if straightened {
            tol + STRAIGHTENED
        } else {
            tol
        };
        let samples = samples(sweep);
        for i in 1..=samples {
            let t = f64::from(i) / f64::from(samples);
            let want = m.point_at(from, t);
            let Some((line, got)) = Self::along(&group, length * t) else {
                continue;
            };
            if got.distance(want) > tol {
                return Err(Problem::at(
                    line,
                    format!(
                        "strays {:.4} mm from move {k} of the path, at {got} where it is at {want}",
                        got.distance(want)
                    ),
                ));
            }
        }
        self.high = self.high.max(m.to.z);
        Ok(())
    }

    /// The point `s` along a run of moves, and the line it is on.
    fn along(group: &[(usize, &Motion)], s: f64) -> Option<(usize, DVec3)> {
        let mut left = s;
        let last = group.len().checked_sub(1)?;
        for (i, &(line, g)) in group.iter().enumerate() {
            let l = g.length();
            if left <= l || i == last {
                let u = if l > 0.0 {
                    (left / l).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                return Some((line, g.point_at(u)?));
            }
            left -= l;
        }
        None
    }

    /// A drilling operation's holes, in order: across between them only at the
    /// clearance height, each drilled straight down to its bottom at its feed,
    /// with no rapid into what is not yet drilled, and back up clear.
    fn holes(&mut self, d: &DrillCycles) -> Result<(), Problem> {
        let Some(first) = d.holes.first() else {
            return Ok(());
        };
        let tol = self.tol.at;
        self.arrive(dvec3(first.at.x, first.at.y, d.clearance), true)?;
        self.phase = Phase::Cutting;
        self.high = d.clearance;
        for (n, hole) in d.holes.iter().enumerate() {
            let n = n + 1;
            let over = |p: DVec3| DVec2::new(p.x, p.y).distance(hole.at) <= tol;
            if n > 1 {
                let (line, m) = self
                    .next_move()?
                    .ok_or_else(|| Problem::whole(format!("the program ends before hole {n}")))?;
                let Some((a, b)) = m.ends() else {
                    return Err(Problem::at(line, "loses its place between holes"));
                };
                if m.feed.is_some() || a.z < d.clearance - tol || b.z < d.clearance - tol {
                    return Err(Problem::at(
                        line,
                        format!("goes to hole {n} below the clearance height"),
                    ));
                }
                if !over(b) {
                    return Err(Problem::at(
                        line,
                        format!("goes to {b} where hole {n} is at {}", hole.at),
                    ));
                }
                self.at = m.to;
            }
            let dwells = self.dwells.len();
            let mut floor = hole.top;
            let mut deepest: Option<f64> = None;
            loop {
                let (line, m) = self
                    .next_move()?
                    .ok_or_else(|| Problem::whole(format!("the program ends in hole {n}")))?;
                let Some((_, b)) = m.ends() else {
                    return Err(Problem::at(line, format!("loses its place in hole {n}")));
                };
                if !over(b) {
                    let drilled = deepest.is_some_and(|z| z <= hole.bottom + tol);
                    return Err(Problem::at(
                        line,
                        if drilled {
                            format!("goes across from hole {n} below the clearance height")
                        } else {
                            format!("moves across before hole {n} is drilled to its bottom")
                        },
                    ));
                }
                self.at = m.to;
                match m.feed {
                    None => {
                        if b.z < floor - tol {
                            return Err(Problem::at(
                                line,
                                format!(
                                    "rapids to Z{:.3} in hole {n}, drilled only to Z{floor:.3}",
                                    b.z
                                ),
                            ));
                        }
                        let done = deepest.is_some_and(|z| z <= hole.bottom + tol);
                        if done && b.z >= d.clearance - tol {
                            break;
                        }
                    }
                    Some(f) => {
                        self.cutting(line)?;
                        if (f - d.feed).abs() > self.tol.feed {
                            return Err(Problem::at(
                                line,
                                format!(
                                    "drills at {f:.3} mm/min where the hole is drilled at {:.3}",
                                    d.feed
                                ),
                            ));
                        }
                        if b.z < hole.bottom - tol {
                            return Err(Problem::at(
                                line,
                                format!("drills hole {n} past its bottom"),
                            ));
                        }
                        if let (Some(q), Some(a)) = (d.peck, known(&m.from))
                            && a.z - b.z > q + tol
                        {
                            return Err(Problem::at(
                                line,
                                format!("pecks deeper than {q:.3} in hole {n}"),
                            ));
                        }
                        floor = floor.min(b.z);
                        deepest = Some(deepest.map_or(b.z, |z: f64| z.min(b.z)));
                    }
                }
            }
            if d.peck.is_none()
                && let Some(want) = d.dwell
                && !self
                    .dwells
                    .get(dwells..)
                    .unwrap_or_default()
                    .iter()
                    .any(|s| (s - want).abs() <= 5.0e-4)
            {
                return Err(Problem::whole(format!(
                    "hole {n} has no pause of {want} s at its bottom"
                )));
            }
        }
        Ok(())
    }

    /// After the last cut: no more cutting, never down, and across only once
    /// clear; and an end.
    fn leave(&mut self) -> Result<(), Problem> {
        while let Some((line, m)) = self.next_move()? {
            if m.feed.is_some() {
                return Err(Problem::at(line, "cuts after the path has ended"));
            }
            if let (Some(a), Some(b)) = (m.from[2], m.to[2])
                && b < a - self.tol.at
            {
                return Err(Problem::at(line, "goes down after the path has ended"));
            }
            if moves(&m.from, &m.to, 2) {
                self.at[2] = m.to[2];
                self.raised = false;
            }
            if (moves(&m.from, &m.to, 0) || moves(&m.from, &m.to, 1)) && !self.clear() {
                return Err(Problem::at(line, "goes across before the tool is clear"));
            }
            self.at = m.to;
        }
        if !self.ended {
            return Err(Problem::whole("the program has no end (M2 or M30)"));
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests;
