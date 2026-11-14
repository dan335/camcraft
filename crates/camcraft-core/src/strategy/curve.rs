//! Curves picked on the part, as the strategies that run along them take them:
//! Fusion's Slot and Trace.
//!
//! # Pieces
//!
//! A [`Curve`] is where it starts and its pieces, each a straight line or a
//! level arc, the way a picked edge is drawn: a round edge lying level is an arc,
//! anything else the line through its points. Arcs stay arcs all the way to the
//! program, as `G2` and `G3`.
//!
//! # Joined
//!
//! Picked edges are joined end to end where exactly two of them meet at a corner
//! of the model ([`join`]): a corner where one ends is an end of the curve, as is
//! one where three or more meet, so nothing is guessed at a fork.
//!
//! # Offset
//!
//! A curve is offset to its left or right, seen from above, by moving each piece
//! square to itself ([`offset`]): a line along its normal, an arc to a smaller or
//! larger radius about the same centre. Where two pieces meet at a corner, the
//! outside of the corner is rounded with an arc about it, and the inside trimmed
//! where the two offset pieces cross. A curve that turns tighter on the inside
//! than the offset is refused, rather than cut through the part.

use std::f64::consts::{PI, TAU};

use glam::{DVec2, DVec3};

use crate::path::toolpath::{Arc, MoveRole, Toolpath, ToolpathError};

/// One piece of a curve, from where the piece before it ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Piece {
    /// Straight to `to`.
    Line {
        /// Where it ends.
        to: DVec3,
    },
    /// Round a level arc to `to`, about `centre` seen from above, turning
    /// `sweep` radians, anticlockwise when positive: at most a whole turn. Its
    /// ends are at one height.
    Arc {
        /// Where it ends.
        to: DVec3,
        /// The arc's centre, seen from above.
        centre: DVec2,
        /// How far round it turns, in radians.
        sweep: f64,
    },
}

impl Piece {
    /// Where it ends.
    pub fn to(&self) -> DVec3 {
        match *self {
            Piece::Line { to } | Piece::Arc { to, .. } => to,
        }
    }

    /// The same piece ending at `to` instead.
    fn ending_at(&self, to: DVec3) -> Piece {
        match *self {
            Piece::Line { .. } => Piece::Line { to },
            Piece::Arc { centre, sweep, .. } => Piece::Arc { to, centre, sweep },
        }
    }

    /// The way it runs, seen from above, where it leaves `from` (`at_end`
    /// false) or arrives at its end: a unit vector, or zero for a piece that
    /// goes straight up or down, or nowhere.
    fn heading(&self, from: DVec3, at_end: bool) -> DVec2 {
        match *self {
            Piece::Line { to } => (to - from).truncate().normalize_or_zero(),
            Piece::Arc { to, centre, sweep } => {
                let p = if at_end { to } else { from };
                let radial = p.truncate() - centre;
                (radial.perp() * sweep.signum()).normalize_or_zero()
            }
        }
    }

    /// Its length seen from above, from `from`.
    pub fn flat_length(&self, from: DVec3) -> f64 {
        match *self {
            Piece::Line { to } => from.truncate().distance(to.truncate()),
            Piece::Arc { centre, sweep, .. } => from.truncate().distance(centre) * sweep.abs(),
        }
    }

    /// Its length, from `from`.
    pub fn length(&self, from: DVec3) -> f64 {
        match *self {
            Piece::Line { to } => from.distance(to),
            Piece::Arc { .. } => self.flat_length(from),
        }
    }

    /// The point a fraction `t` of the way along it, from `from`.
    pub fn point_at(&self, from: DVec3, t: f64) -> DVec3 {
        match *self {
            Piece::Line { to } => from.lerp(to, t),
            Piece::Arc { centre, sweep, .. } => {
                let p = centre + DVec2::from_angle(sweep * t).rotate(from.truncate() - centre);
                p.extend(from.z)
            }
        }
    }

    /// The same piece run backwards, from its end to `from`.
    fn reversed(&self, from: DVec3) -> Piece {
        match *self {
            Piece::Line { .. } => Piece::Line { to: from },
            Piece::Arc { centre, sweep, .. } => Piece::Arc {
                to: from,
                centre,
                sweep: -sweep,
            },
        }
    }

    /// Its first fraction `t`, from `from`.
    fn head(&self, from: DVec3, t: f64) -> Piece {
        let to = self.point_at(from, t);
        match *self {
            Piece::Line { .. } => Piece::Line { to },
            Piece::Arc { centre, sweep, .. } => Piece::Arc {
                to,
                centre,
                sweep: sweep * t,
            },
        }
    }
}

/// A curve: where it starts, and its pieces.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    /// Where it starts.
    pub start: DVec3,
    /// Its pieces, each from where the one before ends.
    pub pieces: Vec<Piece>,
}

impl Curve {
    /// Where it ends: where it starts, with no pieces.
    pub fn end(&self) -> DVec3 {
        self.pieces.last().map_or(self.start, Piece::to)
    }

    /// Whether it ends where it starts, within `tol`.
    pub fn is_closed(&self, tol: f64) -> bool {
        !self.pieces.is_empty() && self.end().distance(self.start) <= tol
    }

    /// Each piece, with where it starts.
    pub fn steps(&self) -> impl Iterator<Item = (DVec3, &Piece)> {
        self.pieces.iter().scan(self.start, |at, p| {
            let from = *at;
            *at = p.to();
            Some((from, p))
        })
    }

    /// Its length seen from above.
    pub fn flat_length(&self) -> f64 {
        self.steps().map(|(from, p)| p.flat_length(from)).sum()
    }

    /// Its longest piece seen from above, with where it starts: the last of
    /// them where several are as long.
    pub fn longest(&self) -> Option<(DVec3, &Piece)> {
        self.steps()
            .max_by(|a, b| a.1.flat_length(a.0).total_cmp(&b.1.flat_length(b.0)))
    }

    /// The way it runs where it starts (`at_end` false) or ends, seen from
    /// above: a unit vector, or zero for a curve that goes nowhere.
    pub fn heading(&self, at_end: bool) -> DVec2 {
        let step = if at_end {
            self.steps().last()
        } else {
            self.steps().next()
        };
        step.map_or(DVec2::ZERO, |(from, p)| p.heading(from, at_end))
    }

    /// The same curve run anticlockwise seen from above, or clockwise where
    /// not `anticlockwise`: reversed where it runs the other way.
    pub fn wound(&self, anticlockwise: bool) -> Curve {
        let area = self.signed_area();
        if (anticlockwise && area < 0.0) || (!anticlockwise && area > 0.0) {
            self.reversed()
        } else {
            self.clone()
        }
    }

    /// The same curve run the other way.
    pub fn reversed(&self) -> Curve {
        let steps: Vec<(DVec3, Piece)> = self.steps().map(|(f, p)| (f, *p)).collect();
        Curve {
            start: self.end(),
            pieces: steps.iter().rev().map(|(f, p)| p.reversed(*f)).collect(),
        }
    }

    /// The same curve moved `dz` up.
    pub fn raised(&self, dz: f64) -> Curve {
        let up = DVec3::Z * dz;
        Curve {
            start: self.start + up,
            pieces: self
                .pieces
                .iter()
                .map(|p| p.ending_at(p.to() + up))
                .collect(),
        }
    }

    /// Its first `run` millimetres, seen from above, run backwards from its
    /// end: the way back along it, at most all of it.
    pub fn back_from_end(&self, run: f64) -> Curve {
        let back = self.reversed();
        let mut left = run;
        let mut pieces = Vec::new();
        for (from, p) in back.steps() {
            let l = p.flat_length(from);
            if l >= left {
                if left > 1.0e-12 && l > 1.0e-12 {
                    pieces.push(p.head(from, left / l));
                }
                break;
            }
            left -= l;
            pieces.push(*p);
        }
        Curve {
            start: back.start,
            pieces,
        }
    }

    /// The area it closes, seen from above: positive going anticlockwise. Its
    /// ends are taken as joined.
    pub fn signed_area(&self) -> f64 {
        let mut twice = 0.0;
        for (from, p) in self.steps() {
            let (a, b) = (from.truncate(), p.to().truncate());
            twice += a.perp_dot(b);
            // An arc adds the segment between it and its chord.
            if let Piece::Arc { centre, sweep, .. } = *p {
                let r2 = a.distance_squared(centre);
                twice += r2 * (sweep - sweep.sin());
            }
        }
        let (a, b) = (self.end().truncate(), self.start.truncate());
        twice += a.perp_dot(b);
        twice / 2.0
    }

    /// Points along it no further than `tol` from it, seen from above, from
    /// its start to its end, both included.
    pub fn sample(&self, tol: f64) -> Vec<DVec3> {
        let mut out = vec![self.start];
        for (from, p) in self.steps() {
            if let Piece::Arc { centre, sweep, .. } = *p {
                let r = from.truncate().distance(centre);
                let n = arc_steps(r, sweep, tol);
                for k in 1..n {
                    out.push(p.point_at(from, f64::from(k) / f64::from(n)));
                }
            }
            out.push(p.to());
        }
        out
    }

    /// How far `q` is from it, seen from above.
    pub fn distance_to(&self, q: DVec2) -> f64 {
        let mut best = self.start.truncate().distance(q);
        for (from, p) in self.steps() {
            let d = match *p {
                Piece::Line { to } => segment_distance(q, from.truncate(), to.truncate()),
                Piece::Arc { to, centre, sweep } => {
                    arc_distance(q, (from.truncate(), to.truncate()), centre, sweep)
                }
            };
            best = best.min(d);
        }
        best
    }

    /// Its arcs cut into pieces of at most half a turn, so no move ends where it
    /// starts, seen from above: one that does reads to a ramp as straight down.
    pub fn in_half_turns(&self) -> Curve {
        let mut pieces = Vec::with_capacity(self.pieces.len());
        for (from, p) in self.steps() {
            match *p {
                Piece::Arc { to, centre, sweep } if sweep.abs() > PI + 1.0e-9 => {
                    let n = camcraft_geom::steps::bounded_count((sweep.abs() / PI - 1.0e-9).ceil());
                    for k in 1..=n {
                        let end = if k == n {
                            to
                        } else {
                            p.point_at(from, f64::from(k) / f64::from(n))
                        };
                        pieces.push(Piece::Arc {
                            to: end,
                            centre,
                            sweep: sweep / f64::from(n),
                        });
                    }
                }
                _ => pieces.push(*p),
            }
        }
        Curve {
            start: self.start,
            pieces,
        }
    }

    /// Append it to `path`, which is where it starts, as `role` at `feed`: each
    /// arc as moves of at most half a turn, each centre put where both its ends
    /// are as far from it, so the arc ends where it should to the micron.
    pub fn cut_into(
        &self,
        path: &mut Toolpath,
        role: MoveRole,
        feed: f64,
    ) -> Result<(), ToolpathError> {
        self.cut_with(path, |_, z| z, role, feed)
    }

    /// Append it to `path` as [`Curve::cut_into`] does, but falling evenly
    /// along it, seen from above, from `high` where it starts to `low` where
    /// it ends, whatever its own heights: a ramp along it. Its arcs are
    /// helices.
    pub fn cut_falling(
        &self,
        path: &mut Toolpath,
        (high, low): (f64, f64),
        role: MoveRole,
        feed: f64,
    ) -> Result<(), ToolpathError> {
        let total = self.flat_length();
        let fall = |run: f64, _| {
            if total > 1.0e-12 {
                high + (low - high) * (run / total).min(1.0)
            } else {
                low
            }
        };
        self.cut_with(path, fall, role, feed)
    }

    /// Append it to `path`, each piece ending at the height `z` gives for how
    /// far along it is seen from above and the piece's own height there.
    fn cut_with(
        &self,
        path: &mut Toolpath,
        z: impl Fn(f64, f64) -> f64,
        role: MoveRole,
        feed: f64,
    ) -> Result<(), ToolpathError> {
        let halves = self.in_half_turns();
        let mut run = 0.0;
        for (own_from, p) in halves.steps() {
            run += p.flat_length(own_from);
            let end = p.to();
            let to = end.truncate().extend(z(run, end.z));
            // Where the path is: its own start, raised or lowered as asked.
            let from = path.end().unwrap_or(own_from);
            match *p {
                Piece::Line { .. } => {
                    if to.distance(from) > 1.0e-9 {
                        path.feed_to(to, role, feed)?;
                    }
                }
                Piece::Arc { centre, sweep, .. } => {
                    let c = equidistant(from.truncate(), to.truncate(), centre);
                    let r = from.truncate().distance(c);
                    let turned = sweep_from(from.truncate(), to.truncate(), c, sweep);
                    // Too small to be an arc: less than a micron round.
                    if r <= 1.0e-6 || turned.abs() * r <= 1.0e-6 {
                        if to.distance(from) > 1.0e-9 {
                            path.feed_to(to, role, feed)?;
                        }
                        continue;
                    }
                    path.arc_to(
                        to,
                        Arc {
                            centre: c.extend(to.z),
                            axis: DVec3::Z,
                            sweep: turned,
                        },
                        role,
                        feed,
                    )?;
                }
            }
        }
        Ok(())
    }
}

/// How many pieces of at most a quarter turn, none further than `tol` from the
/// arc of radius `r`, span `sweep`: at least one.
fn arc_steps(r: f64, sweep: f64, tol: f64) -> u32 {
    let widest = if tol < r {
        (2.0 * (1.0 - tol / r).acos()).clamp(1.0e-3, PI / 2.0)
    } else {
        PI / 2.0
    };
    camcraft_geom::steps::bounded_count((sweep.abs() / widest).ceil())
}

/// `centre` moved square to the chord from `a` to `b` onto the line halfway
/// between them, so the two are as far from it, keeping its side of the chord.
/// As it is for ends that coincide.
fn equidistant(a: DVec2, b: DVec2, centre: DVec2) -> DVec2 {
    let chord = b - a;
    let len = chord.length();
    if len <= 1.0e-9 {
        return centre;
    }
    let mid = (a + b) / 2.0;
    let n = (chord / len).perp();
    mid + n * (centre - mid).dot(n)
}

/// The sweep from `a` to `b` about `c`, turning the way `like` does: a whole
/// turn where they meet and `like` is more than half of one, nothing where it
/// is less.
fn sweep_from(a: DVec2, b: DVec2, c: DVec2, like: f64) -> f64 {
    let (u, v) = (a - c, b - c);
    if a.distance(b) <= 1.0e-9 * (1.0 + u.length()) {
        return if like.abs() > PI {
            TAU.copysign(like)
        } else {
            0.0
        };
    }
    let mut angle = u.perp_dot(v).atan2(u.dot(v));
    if like > 0.0 && angle < 0.0 {
        angle += TAU;
    } else if like < 0.0 && angle > 0.0 {
        angle -= TAU;
    }
    angle
}

/// How far `q` is from the segment from `a` to `b`.
fn segment_distance(q: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let l2 = ab.length_squared();
    if l2 <= 1.0e-18 {
        return q.distance(a);
    }
    let t = ((q - a).dot(ab) / l2).clamp(0.0, 1.0);
    q.distance(a + ab * t)
}

/// How far `q` is from the arc from `a` to `b` about `c`, turning `sweep`.
fn arc_distance(q: DVec2, (a, b): (DVec2, DVec2), c: DVec2, sweep: f64) -> f64 {
    let r = a.distance(c);
    let ends = q.distance(a).min(q.distance(b));
    let v = q - c;
    if v.length() <= 1.0e-12 {
        return r.min(ends);
    }
    // How far round from `a` the direction to `q` is, the way the arc turns.
    let u = a - c;
    let mut at = u.perp_dot(v).atan2(u.dot(v)) * sweep.signum();
    if at < 0.0 {
        at += TAU;
    }
    if at <= sweep.abs() + 1.0e-12 {
        (v.length() - r).abs().min(ends)
    } else {
        ends
    }
}

/// A picked edge as a piece of curve: which model's corners it runs between,
/// and its shape.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// The model it is on.
    pub model: String,
    /// The corners it ends at, in its model's numbering; `None` for an edge
    /// that closes on itself.
    pub ends: Option<[u32; 2]>,
    /// Its shape, from its first corner to its second.
    pub curve: Curve,
}

/// `edges` joined end to end where exactly two of them meet at a corner, each
/// curve as far as it goes both ways, or round to where it started. In the
/// order of their first edge as given; each runs the way that edge does.
pub fn join(edges: &[Edge]) -> Vec<Curve> {
    // How many of the edges' ends are at a corner of `model`.
    let meeting = |model: &str, v: u32| {
        edges
            .iter()
            .filter(|e| e.model == model)
            .filter_map(|e| e.ends)
            .map(|[a, b]| usize::from(a == v) + usize::from(b == v))
            .sum::<usize>()
    };
    let mut used = vec![false; edges.len()];
    let mut out = Vec::new();
    for (i, first) in edges.iter().enumerate() {
        if used.get(i).copied().unwrap_or(true) {
            continue;
        }
        if let Some(u) = used.get_mut(i) {
            *u = true;
        }
        let Some([back, front]) = first.ends else {
            out.push(first.curve.clone());
            continue;
        };
        // The other edge at `corner`, where only two ends meet: its place, and
        // whether it starts there.
        let next = |corner: u32, used: &[bool]| {
            if meeting(&first.model, corner) != 2 {
                return None;
            }
            edges.iter().enumerate().find_map(|(j, e)| {
                if used.get(j).copied().unwrap_or(true) || e.model != first.model {
                    return None;
                }
                match e.ends {
                    Some([a, _]) if a == corner => Some((j, true)),
                    Some([_, b]) if b == corner => Some((j, false)),
                    _ => None,
                }
            })
        };
        let mut curve = first.curve.clone();
        // Forward from its end, then back from its start; each edge once.
        let mut at = front;
        let mut closed = at == back;
        for _ in 0..edges.len() {
            if closed {
                break;
            }
            let Some((j, starts)) = next(at, &used) else {
                break;
            };
            let Some(e) = edges.get(j) else { break };
            if let Some(u) = used.get_mut(j) {
                *u = true;
            }
            let (piece, far) = match (starts, e.ends) {
                (true, Some([_, b])) => (e.curve.clone(), b),
                (false, Some([a, _])) => (e.curve.reversed(), a),
                _ => break,
            };
            curve.pieces.extend(piece.pieces);
            at = far;
            closed = at == back;
        }
        let mut at = back;
        for _ in 0..edges.len() {
            if closed {
                break;
            }
            let Some((j, starts)) = next(at, &used) else {
                break;
            };
            let Some(e) = edges.get(j) else { break };
            if let Some(u) = used.get_mut(j) {
                *u = true;
            }
            // It comes before: run so it ends where the curve starts.
            let (piece, far) = match (starts, e.ends) {
                (true, Some([_, b])) => (e.curve.reversed(), b),
                (false, Some([a, _])) => (e.curve.clone(), a),
                _ => break,
            };
            let mut pieces = piece.pieces;
            pieces.append(&mut curve.pieces);
            curve = Curve {
                start: piece.start,
                pieces,
            };
            at = far;
        }
        out.push(curve);
    }
    out
}

/// Why a curve cannot be offset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TooTight {
    /// Where it turns too tightly, seen from above.
    pub at: DVec2,
}

/// A piece of a curve being offset: its place on the curve, where it starts,
/// the piece, and the way it runs at each end.
#[derive(Debug, Clone, Copy)]
struct Moved {
    place: usize,
    start: DVec3,
    piece: Piece,
    heads: (DVec2, DVec2),
}

/// `curve` moved `d` to its left, seen from above as it runs, or to its right
/// where `d` is negative: each piece moved square to itself, the outside of each
/// corner rounded about it and the inside trimmed where the two offset pieces
/// cross. A closed curve stays closed. An arc turning more tightly on the inside
/// than `d` is refused, as is a corner whose offset pieces do not cross; one
/// turning exactly as tightly shrinks to its centre and is left out. `tol` is
/// how close two ends must be to be one.
pub fn offset(curve: &Curve, d: f64, tol: f64) -> Result<Curve, TooTight> {
    if d.abs() <= 1.0e-12 {
        return Ok(curve.clone());
    }
    let closed = curve.is_closed(tol);
    let steps: Vec<(DVec3, Piece)> = curve.steps().map(|(f, p)| (f, *p)).collect();
    // Each piece's way across, seen from above, at each end: where a piece
    // goes straight up or down, the way of the one before it, or after.
    let mut heads: Vec<(DVec2, DVec2)> = steps
        .iter()
        .map(|(f, p)| (p.heading(*f, false), p.heading(*f, true)))
        .collect();
    fill_headings(&mut heads);
    // Each piece moved; an arc shrunk to its centre is left out.
    let mut moved: Vec<Moved> = Vec::with_capacity(steps.len());
    for (i, ((from, p), &(h0, h1))) in steps.iter().zip(&heads).enumerate() {
        let (start, piece) = match *p {
            Piece::Line { to } => {
                let (n0, n1) = (h0.perp() * d, h1.perp() * d);
                (
                    *from + n0.extend(0.0),
                    Piece::Line {
                        to: to + n1.extend(0.0),
                    },
                )
            }
            Piece::Arc { to, centre, sweep } => {
                let r = from.truncate().distance(centre);
                // The centre is on the left going anticlockwise.
                let shrunk = r - d * sweep.signum();
                if shrunk < -tol || r <= 1.0e-12 {
                    return Err(TooTight { at: centre });
                }
                if shrunk <= tol {
                    continue;
                }
                let k = shrunk / r;
                let scale = |q: DVec3| (centre + (q.truncate() - centre) * k).extend(q.z);
                (
                    scale(*from),
                    Piece::Arc {
                        to: scale(to),
                        centre,
                        sweep,
                    },
                )
            }
        };
        moved.push(Moved {
            place: i,
            start,
            piece,
            heads: (h0, h1),
        });
    }
    let count = moved.len();
    if count == 0 {
        return Err(TooTight {
            at: curve.start.truncate(),
        });
    }
    // Each corner between two moved pieces, rounded or trimmed: `joints[k]`
    // goes after piece `k`.
    let mut joints: Vec<Option<Piece>> = vec![None; count];
    let corners = if closed { count } else { count - 1 };
    for k in 0..corners {
        let next = (k + 1) % count;
        let (Some(a), Some(b)) = (moved.get(k).copied(), moved.get(next).copied()) else {
            continue;
        };
        // The corner on the curve: where the piece after it starts.
        let corner = steps.get(b.place).map_or(curve.start, |(f, _)| *f);
        let gap = a.piece.to().truncate().distance(b.start.truncate());
        if gap <= tol {
            if let Some(m) = moved.get_mut(next) {
                m.start = a.piece.to();
            }
            continue;
        }
        let (arriving, leaving) = (a.heads.1, b.heads.0);
        if arriving.perp_dot(leaving) * d < 0.0 {
            // Turning away from the side it is moved to: round the outside of
            // the corner, about it, as far as it turns.
            if let Some(j) = joints.get_mut(k) {
                *j = Some(Piece::Arc {
                    to: b.start,
                    centre: corner.truncate(),
                    sweep: camcraft_geom::arc::turn(arriving, leaving),
                });
            }
        } else {
            // Turning towards it: the two cross before the corner.
            let at = crossing((a.start, a.piece), (b.start, b.piece), corner.truncate()).ok_or(
                TooTight {
                    at: corner.truncate(),
                },
            )?;
            if let Some(m) = moved.get_mut(k) {
                m.piece = m.piece.ending_at(at);
            }
            if let Some(m) = moved.get_mut(next) {
                m.start = at;
            }
        }
    }
    // Each piece must still run the way it did: trimmed from both ends past
    // each other, the curve turns too tightly there.
    if let Some(m) = moved.iter().find(|m| !still_forward(m)) {
        return Err(TooTight {
            at: m.start.truncate(),
        });
    }
    let first = moved.first().map_or(curve.start, |m| m.start);
    let mut pieces = Vec::with_capacity(count * 2);
    for (m, joint) in moved.iter().zip(&joints) {
        pieces.push(match m.piece {
            Piece::Arc { to, centre, sweep } => Piece::Arc {
                to,
                centre,
                sweep: sweep_from(m.start.truncate(), to.truncate(), centre, sweep),
            },
            line => line,
        });
        pieces.extend(*joint);
    }
    Ok(Curve {
        start: first,
        pieces,
    })
}

/// Each piece's headings, those that are nothing (a piece straight up or
/// down) taken from the piece before, or failing that after.
fn fill_headings(heads: &mut [(DVec2, DVec2)]) {
    let mut last = DVec2::ZERO;
    for h in heads.iter_mut() {
        if h.0 == DVec2::ZERO {
            h.0 = last;
        }
        if h.1 == DVec2::ZERO {
            h.1 = h.0;
        }
        last = h.1;
    }
    let mut next = DVec2::ZERO;
    for h in heads.iter_mut().rev() {
        if h.1 == DVec2::ZERO {
            h.1 = next;
        }
        if h.0 == DVec2::ZERO {
            h.0 = h.1;
        }
        next = h.0;
    }
}

/// Whether a moved piece, trimmed at its ends, still runs the way it did.
fn still_forward(m: &Moved) -> bool {
    match m.piece {
        Piece::Line { to } => {
            let run = (to - m.start).truncate();
            run.length() <= 1.0e-9 || run.dot(m.heads.0) > -1.0e-9
        }
        Piece::Arc { to, centre, sweep } => {
            let left = sweep_from(m.start.truncate(), to.truncate(), centre, sweep);
            left.abs() <= sweep.abs() + 1.0e-9
        }
    }
}

/// How far along the piece from `start` the point `x` is, as a fraction of
/// it, seen from above: a line's projection, an arc's turn to it.
fn along(start: DVec3, piece: &Piece, x: DVec2) -> f64 {
    match *piece {
        Piece::Line { to } => {
            let ab = (to - start).truncate();
            let l2 = ab.length_squared();
            if l2 <= 1.0e-18 {
                return 0.0;
            }
            (x - start.truncate()).dot(ab) / l2
        }
        Piece::Arc { centre, sweep, .. } => {
            if sweep.abs() <= 1.0e-12 {
                return 0.0;
            }
            let u = start.truncate() - centre;
            let v = x - centre;
            let mut at = u.perp_dot(v).atan2(u.dot(v)) * sweep.signum();
            if at < -1.0e-9 {
                at += TAU;
            }
            at / sweep.abs()
        }
    }
}

/// Where two pieces cross, seen from above, within both, nearest `near`: the
/// point on the first, at its height there.
fn crossing((a0, a): (DVec3, Piece), (b0, b): (DVec3, Piece), near: DVec2) -> Option<DVec3> {
    const SLACK: f64 = 1.0e-6;
    let candidates: Vec<DVec2> = match (a, b) {
        (Piece::Line { to: a1 }, Piece::Line { to: b1 }) => {
            lines_cross(a0.truncate(), a1.truncate(), b0.truncate(), b1.truncate())
                .into_iter()
                .collect()
        }
        (Piece::Line { to }, Piece::Arc { centre, .. }) => line_circle(
            a0.truncate(),
            to.truncate(),
            centre,
            b0.truncate().distance(centre),
        ),
        (Piece::Arc { centre, .. }, Piece::Line { to }) => line_circle(
            b0.truncate(),
            to.truncate(),
            centre,
            a0.truncate().distance(centre),
        ),
        (Piece::Arc { centre: c1, .. }, Piece::Arc { centre: c2, .. }) => circles_cross(
            (c1, a0.truncate().distance(c1)),
            (c2, b0.truncate().distance(c2)),
        ),
    };
    let x = candidates
        .into_iter()
        .filter(|&x| {
            let (ta, tb) = (along(a0, &a, x), along(b0, &b, x));
            (-SLACK..=1.0 + SLACK).contains(&ta) && (-SLACK..=1.0 + SLACK).contains(&tb)
        })
        .min_by(|p, q| p.distance(near).total_cmp(&q.distance(near)))?;
    let t = along(a0, &a, x).clamp(0.0, 1.0);
    Some(x.extend(a.point_at(a0, t).z))
}

/// Where the lines through `a0`, `a1` and through `b0`, `b1` cross, if they do.
fn lines_cross(a0: DVec2, a1: DVec2, b0: DVec2, b1: DVec2) -> Option<DVec2> {
    let (r, s) = (a1 - a0, b1 - b0);
    let den = r.perp_dot(s);
    if den.abs() <= 1.0e-12 * r.length() * s.length() + 1.0e-18 {
        return None;
    }
    let t = (b0 - a0).perp_dot(s) / den;
    Some(a0 + r * t)
}

/// Where the line through `a0`, `a1` meets the circle of radius `r` about `c`.
fn line_circle(a0: DVec2, a1: DVec2, c: DVec2, r: f64) -> Vec<DVec2> {
    let d = a1 - a0;
    let l2 = d.length_squared();
    if l2 <= 1.0e-18 {
        return Vec::new();
    }
    let f = a0 - c;
    let b = f.dot(d) / l2;
    let disc = b * b - (f.length_squared() - r * r) / l2;
    if disc < 0.0 {
        return Vec::new();
    }
    let root = disc.sqrt();
    vec![a0 + d * (-b - root), a0 + d * (-b + root)]
}

/// Where two circles, each a centre and a radius, cross.
fn circles_cross((c1, r1): (DVec2, f64), (c2, r2): (DVec2, f64)) -> Vec<DVec2> {
    let between = c2 - c1;
    let d = between.length();
    if d <= 1.0e-12 || d > r1 + r2 || d < (r1 - r2).abs() {
        return Vec::new();
    }
    let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let h = (r1 * r1 - a * a).max(0.0).sqrt();
    let u = between / d;
    let m = c1 + u * a;
    vec![m + u.perp() * h, m - u.perp() * h]
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::panic
)]
mod tests;
