//! Rings of straight lines and circular arcs: the exact shape a 2D cut follows.
//!
//! A [`Region`](crate::contour::Region) is polygons, as its clipping needs, and
//! every curve in one is short straight lines. A cut round a part should follow
//! the part: a round corner of the model as one arc, and the corner the tool's
//! centre turns round a sharp one as another. A [`Ring`] says that.
//!
//! # Exact, never fitted
//!
//! A ring's arcs come from two places only. [`Ring::snapped`] makes an arc of a
//! run of a polygon's points that lie on one of the model's own circles, as a
//! tessellation's points on a round edge do, taking the circle's centre and radius
//! from the model. [`Ring::offset`] moves every line and arc of a ring square to
//! itself, and turns the corners it opens round an arc about the corner. Nothing
//! is ever fitted to points.
//!
//! # Checked against the polygons
//!
//! An offset can change a shape's topology: a narrow neck closes, a small hole
//! vanishes. The polygon clipping gets that right, and this does not try to.
//! [`Ring::close_to`] checks a ring against the polygon the clipping made, both
//! ways, so a ring is only ever used where it is the same shape.

use std::f64::consts::{FRAC_PI_2, TAU};

use glam::DVec2;

/// One piece of a ring, from where the piece before it ended.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Span {
    /// Straight to `to`.
    Line {
        /// Where it ends.
        to: DVec2,
    },
    /// Round `centre` to `to`, turning `sweep` radians: counter-clockwise when
    /// positive. A full turn ends where it starts.
    Arc {
        /// Where it ends.
        to: DVec2,
        /// Its centre.
        centre: DVec2,
        /// How far it turns.
        sweep: f64,
    },
}

impl Span {
    /// Where it ends.
    pub fn to(&self) -> DVec2 {
        match *self {
            Span::Line { to } | Span::Arc { to, .. } => to,
        }
    }

    /// Its length, from `from`.
    pub fn length(&self, from: DVec2) -> f64 {
        match *self {
            Span::Line { to } => from.distance(to),
            Span::Arc { centre, sweep, .. } => from.distance(centre) * sweep.abs(),
        }
    }

    /// The point a fraction `t` of the way along it, from `from`.
    pub fn point_at(&self, from: DVec2, t: f64) -> DVec2 {
        match *self {
            Span::Line { to } => from.lerp(to, t),
            Span::Arc { to, centre, sweep } => {
                if t >= 1.0 {
                    to
                } else {
                    centre + DVec2::from_angle(sweep * t).rotate(from - centre)
                }
            }
        }
    }

    /// The direction it runs a fraction `t` of the way along, from `from`, a unit
    /// vector; zero for a span that goes nowhere.
    pub fn tangent_at(&self, from: DVec2, t: f64) -> DVec2 {
        match *self {
            Span::Line { to } => (to - from).normalize_or_zero(),
            Span::Arc { centre, sweep, .. } => {
                let r = DVec2::from_angle(sweep * t).rotate(from - centre);
                (r.perp() * sweep.signum()).normalize_or_zero()
            }
        }
    }

    /// The two spans it is cut into a fraction `t` of the way along, from
    /// `from`, and the point between them.
    fn split(&self, from: DVec2, t: f64) -> (Span, DVec2, Span) {
        let mid = self.point_at(from, t);
        match *self {
            Span::Line { to } => (Span::Line { to: mid }, mid, Span::Line { to }),
            Span::Arc { to, centre, sweep } => (
                Span::Arc {
                    to: mid,
                    centre,
                    sweep: sweep * t,
                },
                mid,
                Span::Arc {
                    to,
                    centre,
                    sweep: sweep * (1.0 - t),
                },
            ),
        }
    }

    /// Where along it, from `from`, as a fraction, it comes nearest `p`, and the
    /// point there.
    fn nearest(&self, from: DVec2, p: DVec2) -> (f64, DVec2) {
        match *self {
            Span::Line { to } => {
                let d = to - from;
                let l = d.length_squared();
                let t = if l > 1e-24 {
                    ((p - from).dot(d) / l).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (t, from + d * t)
            }
            Span::Arc { to, centre, sweep } => {
                let (r0, v) = (from - centre, p - centre);
                let mut angle = r0.perp_dot(v).atan2(r0.dot(v)) * sweep.signum();
                angle = angle.rem_euclid(TAU);
                if angle <= sweep.abs() {
                    let t = angle / sweep.abs();
                    (t, self.point_at(from, t))
                } else if p.distance(from) <= p.distance(to) {
                    (0.0, from)
                } else {
                    (1.0, to)
                }
            }
        }
    }

    /// Points along it within `tolerance` of it, from `from`, the start left out
    /// and its end last.
    fn samples(&self, from: DVec2, tolerance: f64) -> Vec<DVec2> {
        match *self {
            Span::Line { to } => vec![to],
            Span::Arc { to, centre, sweep } => {
                let r = from.distance(centre);
                let widest = if tolerance < r {
                    2.0 * (1.0 - tolerance / r).acos()
                } else {
                    FRAC_PI_2
                }
                .clamp(1e-3, FRAC_PI_2);
                let n = (sweep.abs() / widest).ceil().clamp(1.0, 1e5);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let n = n as u32;
                let mut out: Vec<DVec2> = (1..n)
                    .map(|k| self.point_at(from, f64::from(k) / f64::from(n)))
                    .collect();
                out.push(to);
                out
            }
        }
    }
}

/// A closed ring of lines and arcs.
#[derive(Debug, Clone, PartialEq)]
pub struct Ring {
    start: DVec2,
    spans: Vec<Span>,
}

impl Ring {
    /// A ring of straight lines through `points`, closing back to the first.
    /// `None` for fewer than two distinct points.
    pub fn from_points(points: &[DVec2]) -> Option<Self> {
        let start = *points.first()?;
        let mut spans: Vec<Span> = Vec::with_capacity(points.len());
        let mut at = start;
        for &p in points.iter().skip(1).chain(std::iter::once(&start)) {
            if p.distance(at) > 1e-12 {
                spans.push(Span::Line { to: p });
                at = p;
            }
        }
        (spans.len() >= 2).then_some(Self { start, spans })
    }

    /// Where it starts, and ends.
    pub fn start(&self) -> DVec2 {
        self.start
    }

    /// Its spans, in order, the first from its start.
    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// Each span with where it starts.
    pub fn pieces(&self) -> impl Iterator<Item = (DVec2, &Span)> + '_ {
        let mut at = self.start;
        self.spans
            .iter()
            .map(move |s| (std::mem::replace(&mut at, s.to()), s))
    }

    /// Whether any span is an arc.
    pub fn has_arcs(&self) -> bool {
        self.spans.iter().any(|s| matches!(s, Span::Arc { .. }))
    }

    /// Its length.
    pub fn length(&self) -> f64 {
        self.pieces().map(|(from, s)| s.length(from)).sum()
    }

    /// Points round it within `tolerance` of it, its start first and not
    /// repeated at the end.
    pub fn points(&self, tolerance: f64) -> Vec<DVec2> {
        let mut out = vec![self.start];
        for (from, s) in self.pieces() {
            out.extend(s.samples(from, tolerance));
        }
        out.pop();
        out
    }

    /// The direction it sets off in from its start.
    pub fn start_tangent(&self) -> DVec2 {
        self.spans
            .first()
            .map_or(DVec2::ZERO, |s| s.tangent_at(self.start, 0.0))
    }

    /// The direction it comes back into its start.
    pub fn end_tangent(&self) -> DVec2 {
        self.pieces()
            .last()
            .map_or(DVec2::ZERO, |(from, s)| s.tangent_at(from, 1.0))
    }

    /// Whether it turns a corner at its start rather than running smoothly on.
    pub fn starts_at_corner(&self) -> bool {
        self.end_tangent().dot(self.start_tangent()) < 1.0 - 1e-9
    }

    /// The same ring run the other way.
    pub fn reversed(&self) -> Self {
        let pieces: Vec<(DVec2, Span)> = self.pieces().map(|(f, s)| (f, *s)).collect();
        let spans = pieces
            .iter()
            .rev()
            .map(|(from, s)| match *s {
                Span::Line { .. } => Span::Line { to: *from },
                Span::Arc { centre, sweep, .. } => Span::Arc {
                    to: *from,
                    centre,
                    sweep: -sweep,
                },
            })
            .collect();
        Self {
            start: self.start,
            spans,
        }
    }

    /// The same ring started at span `i`, a fraction `t` of the way along it.
    fn started_at(&self, i: usize, t: f64) -> Self {
        let n = self.spans.len();
        let pieces: Vec<(DVec2, Span)> = self.pieces().map(|(f, s)| (f, *s)).collect();
        let Some(&(from, span)) = pieces.get(i) else {
            return self.clone();
        };
        let (head, mid, tail) = if t <= 1e-12 {
            (None, from, Some(span))
        } else if t >= 1.0 - 1e-12 {
            (Some(span), span.to(), None)
        } else {
            let (a, m, b) = span.split(from, t);
            (Some(a), m, Some(b))
        };
        let mut spans = Vec::with_capacity(n + 1);
        spans.extend(tail);
        spans.extend((1..n).filter_map(|k| pieces.get((i + k) % n).map(|p| p.1)));
        spans.extend(head);
        Self { start: mid, spans }
    }

    /// The same ring, started where it comes nearest `p`.
    pub fn starting_near(&self, p: DVec2) -> Self {
        let best = self
            .pieces()
            .enumerate()
            .map(|(i, (from, s))| {
                let (t, q) = s.nearest(from, p);
                (i, t, q.distance_squared(p))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2));
        match best {
            Some((i, t, _)) => self.started_at(i, t),
            None => self.clone(),
        }
    }

    /// The way round it from its start, `distance` long, as many times round as
    /// that takes: its spans, the last cut short where the way ends, and where
    /// that is.
    pub fn along(&self, distance: f64) -> (Vec<Span>, DVec2) {
        let mut out = Vec::new();
        let mut left = distance;
        let pieces: Vec<(DVec2, Span)> = self.pieces().map(|(f, s)| (f, *s)).collect();
        let total: f64 = pieces.iter().map(|(f, s)| s.length(*f)).sum();
        if total <= 1e-12 || left <= 1e-12 {
            return (out, self.start);
        }
        // Bounded: each time round is the ring's whole length.
        let laps = (left / total).ceil().min(1e4);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let limit = pieces.len().saturating_mul(laps as usize + 1);
        for &(from, span) in pieces.iter().cycle().take(limit) {
            let l = span.length(from);
            if l >= left - 1e-12 {
                if l - left <= 1e-12 {
                    out.push(span);
                    return (out, span.to());
                }
                let (head, at, _) = span.split(from, left / l);
                out.push(head);
                return (out, at);
            }
            left -= l;
            out.push(span);
        }
        (out, self.start)
    }

    /// The same ring, started halfway along its longest span.
    pub fn longest_middle(&self) -> Self {
        let best = self
            .pieces()
            .enumerate()
            .map(|(i, (from, s))| (i, s.length(from)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match best {
            Some((i, _)) => self.started_at(i, 0.5),
            None => self.clone(),
        }
    }

    /// The ring through `points`, a closed polygon, with every run of its points
    /// on one of `circles` (a centre and a radius each) made an arc of that
    /// circle. A point is on a circle within `eps` of it; a run is two or more
    /// sides each turning less than a quarter turn about the circle, all the same
    /// way, whose chords stray no more than `sag` from it: a round edge's
    /// tessellation, not a chord across it.
    pub fn snapped(points: &[DVec2], circles: &[(DVec2, f64)], eps: f64, sag: f64) -> Option<Self> {
        let n = points.len();
        if n < 3 {
            return Self::from_points(points);
        }
        let at = |i: usize| points.get(i % n).copied().unwrap_or(DVec2::ZERO);
        // The circle each side lies along, if any, and how far it turns about it.
        let side = |i: usize| -> Option<(usize, f64)> {
            let (a, b) = (at(i), at(i + 1));
            circles.iter().enumerate().find_map(|(k, &(c, r))| {
                let on = |p: DVec2| (p.distance(c) - r).abs() <= eps;
                if r <= eps || !on(a) || !on(b) {
                    return None;
                }
                let (ra, rb) = (a - c, b - c);
                let turn = ra.perp_dot(rb).atan2(ra.dot(rb));
                let chord_sag = r * (1.0 - (turn / 2.0).cos());
                (turn.abs() > 1e-12 && turn.abs() < FRAC_PI_2 && chord_sag <= sag)
                    .then_some((k, turn))
            })
        };
        let sides: Vec<Option<(usize, f64)>> = (0..n).map(side).collect();
        let side_of = |i: usize| sides.get(i % n).copied().flatten();
        let circle_of = |k: usize| circles.get(k).copied().unwrap_or((DVec2::ZERO, 0.0));
        let same = |x: Option<(usize, f64)>, y: Option<(usize, f64)>| match (x, y) {
            (Some((k, a)), Some((l, b))) => k == l && a.signum() == b.signum(),
            _ => false,
        };
        // Start where a run of sides begins, or anywhere if all are one run or
        // none: so no arc is cut in two by where the ring starts.
        let first = (0..n)
            .find(|&i| !same(side_of(i + n - 1), side_of(i)))
            .unwrap_or(0);
        // The runs of sides along one circle, as (first side, sides, circle),
        // counted from `first`.
        let mut runs: Vec<(usize, usize, Option<usize>)> = Vec::new();
        let mut i = 0;
        while i < n {
            let s = (first + i) % n;
            let mut j = i + 1;
            while j < n && same(side_of(s), side_of(first + j)) {
                j += 1;
            }
            let circle = side_of(s).map(|x| x.0).filter(|_| j - i >= 2 || j - i == n);
            runs.push((i, j - i, circle));
            i = j;
        }
        // Every point an arc starts or ends at, on its circle exactly, so a line
        // into an arc ends where the arc starts.
        let mut pt: Vec<DVec2> = (0..=n).map(|m| at(first + m)).collect();
        for &(i, len, circle) in &runs {
            if let Some(k) = circle {
                let (c, r) = circle_of(k);
                for m in i..=i + len {
                    if let Some(p) = pt.get_mut(m) {
                        *p = c + (*p - c).normalize_or_zero() * r;
                    }
                }
            }
        }
        // The ring's start is its end too: on the circle if either is.
        let (head, tail) = (pt.first().copied(), pt.last().copied());
        if let (Some(h), Some(t)) = (head, tail) {
            let shared = if h == at(first) { t } else { h };
            if let Some(p) = pt.first_mut() {
                *p = shared;
            }
            if let Some(p) = pt.last_mut() {
                *p = shared;
            }
        }
        let point = |m: usize| pt.get(m).copied().unwrap_or(DVec2::ZERO);
        let mut spans = Vec::new();
        for &(i, len, circle) in &runs {
            match circle {
                Some(k) => {
                    let sweep: f64 = (i..i + len)
                        .filter_map(|m| side_of(first + m).map(|x| x.1))
                        .sum();
                    spans.push(Span::Arc {
                        to: point(i + len),
                        centre: circle_of(k).0,
                        sweep,
                    });
                }
                None => {
                    for m in i..i + len {
                        spans.push(Span::Line { to: point(m + 1) });
                    }
                }
            }
        }
        let start = point(0);
        (spans.len() >= 2 || spans.iter().any(|s| matches!(s, Span::Arc { .. })))
            .then_some(Self { start, spans })
    }

    /// The ring moved `d` to its right, square to every span, the corners it
    /// opens up turned round an arc about each, and the ones it closes cut off
    /// where its spans cross. Where anything would turn inside out, as an arc
    /// shrunk past its centre or a span cut past its own end, `None`.
    pub fn offset(&self, d: f64) -> Option<Self> {
        if d.abs() < 1e-12 {
            return Some(self.clone());
        }
        let pieces: Vec<(DVec2, Span)> = self.pieces().map(|(f, s)| (f, *s)).collect();
        let n = pieces.len();
        // Each span moved, from its moved start.
        let moved: Vec<(DVec2, Span)> = pieces
            .iter()
            .map(|&(from, s)| moved(from, s, d))
            .collect::<Option<_>>()?;
        // At each corner, the end of span `i` and the start of span `i + 1`.
        let mut starts: Vec<DVec2> = moved.iter().map(|m| m.0).collect();
        let mut ends: Vec<DVec2> = moved.iter().map(|m| m.1.to()).collect();
        let mut joins: Vec<Option<Span>> = vec![None; n];
        for i in 0..n {
            let k = (i + 1) % n;
            let (&(from_i, span_i), &(from_k, span_k)) = (pieces.get(i)?, pieces.get(k)?);
            let t1 = span_i.tangent_at(from_i, 1.0);
            let t2 = span_k.tangent_at(from_k, 0.0);
            let turn = t1.perp_dot(t2).atan2(t1.dot(t2));
            let (e, f) = (*ends.get(i)?, *starts.get(k)?);
            let corner: (DVec2, Option<Span>) = if e.distance(f) < 1e-9 {
                (e, None)
            } else if turn * d > 0.0 {
                // Opened up: round the corner.
                let arc = Span::Arc {
                    to: f,
                    centre: span_i.to(),
                    sweep: turn,
                };
                (f, Some(arc))
            } else {
                // Closed up: where the two cross, nearest the corner.
                let x = crossing(*moved.get(i)?, *moved.get(k)?, (e + f) * 0.5)?;
                *ends.get_mut(i)? = x;
                (x, None)
            };
            *starts.get_mut(k)? = corner.0;
            *joins.get_mut(i)? = corner.1;
        }
        let mut spans = Vec::with_capacity(2 * n);
        for (((&(orig_from, orig), &a), &b), join) in
            moved.iter().zip(&starts).zip(&ends).zip(&joins)
        {
            spans.push(trimmed(orig_from, orig, a, b)?);
            spans.extend(*join);
        }
        let start = *starts.first()?;
        Some(Self { start, spans })
    }

    /// The distance from `p` to the nearest point of the ring.
    pub fn distance_to(&self, p: DVec2) -> f64 {
        self.pieces()
            .map(|(from, s)| s.nearest(from, p).1.distance(p))
            .fold(f64::INFINITY, f64::min)
    }

    /// Whether the ring and the closed polygon through `points` are the same
    /// shape, within `tolerance`: every point of either within it of the other.
    pub fn close_to(&self, points: &[DVec2], tolerance: f64) -> bool {
        let n = points.len();
        if n < 2 {
            return false;
        }
        let to_polygon = |p: DVec2| {
            points
                .iter()
                .zip(points.iter().cycle().skip(1))
                .map(|(&a, &b)| {
                    let d = b - a;
                    let l = d.length_squared();
                    let t = if l > 1e-24 {
                        ((p - a).dot(d) / l).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    (a + d * t).distance_squared(p)
                })
                .fold(f64::INFINITY, f64::min)
                .sqrt()
        };
        let t2 = tolerance;
        points.iter().all(|&p| self.distance_to(p) <= t2)
            && self
                .points(tolerance * 0.25)
                .iter()
                .all(|&p| to_polygon(p) <= t2)
    }
}

/// Span `s`, from `from`, moved `d` to its right: its new start, and the span.
fn moved(from: DVec2, s: Span, d: f64) -> Option<(DVec2, Span)> {
    match s {
        Span::Line { to } => {
            let n = (to - from).normalize_or_zero();
            let right = DVec2::new(n.y, -n.x) * d;
            Some((from + right, Span::Line { to: to + right }))
        }
        Span::Arc { to, centre, sweep } => {
            // Counter-clockwise, its centre is on the left: the right is out.
            let r = from.distance(centre);
            let r2 = r + d * sweep.signum();
            if r <= 1e-12 || r2 <= 1e-9 {
                return None;
            }
            let k = r2 / r;
            Some((
                centre + (from - centre) * k,
                Span::Arc {
                    to: centre + (to - centre) * k,
                    centre,
                    sweep,
                },
            ))
        }
    }
}

/// Where two moved spans, extended as far as they go, cross nearest `near`.
fn crossing(a: (DVec2, Span), b: (DVec2, Span), near: DVec2) -> Option<DVec2> {
    let circle = |(from, s): (DVec2, Span)| match s {
        Span::Line { .. } => None,
        Span::Arc { centre, .. } => Some((centre, from.distance(centre))),
    };
    let line = |(from, s): (DVec2, Span)| (from, (s.to() - from).normalize_or_zero());
    let candidates: Vec<DVec2> = match (circle(a), circle(b)) {
        (None, None) => {
            let ((p, u), (q, v)) = (line(a), line(b));
            let den = u.perp_dot(v);
            if den.abs() < 1e-12 {
                return None;
            }
            let t = (q - p).perp_dot(v) / den;
            vec![p + u * t]
        }
        (Some(c), None) => line_circle(line(b), c),
        (None, Some(c)) => line_circle(line(a), c),
        (Some((c1, r1)), Some((c2, r2))) => circles(c1, r1, c2, r2),
    };
    candidates
        .into_iter()
        .min_by(|x, y| x.distance(near).total_cmp(&y.distance(near)))
}

/// Where the line through `p` along unit `u` meets the circle about `c` of
/// radius `r`.
fn line_circle((p, u): (DVec2, DVec2), (c, r): (DVec2, f64)) -> Vec<DVec2> {
    let f = p - c;
    let b = f.dot(u);
    let disc = b * b - (f.length_squared() - r * r);
    if disc < -1e-12 {
        return Vec::new();
    }
    let root = disc.max(0.0).sqrt();
    vec![p + u * (-b - root), p + u * (-b + root)]
}

/// Where two circles meet.
fn circles(c1: DVec2, r1: f64, c2: DVec2, r2: f64) -> Vec<DVec2> {
    let d = c1.distance(c2);
    if d < 1e-12 || d > r1 + r2 + 1e-9 || d < (r1 - r2).abs() - 1e-9 {
        return Vec::new();
    }
    let along = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let h = (r1 * r1 - along * along).max(0.0).sqrt();
    let u = (c2 - c1) / d;
    let mid = c1 + u * along;
    vec![mid + u.perp() * h, mid - u.perp() * h]
}

/// Moved span `s`, which ran from `from`, cut to run from `a` to `b` instead:
/// `None` where that would run it backwards, or round an arc further than it
/// went.
fn trimmed(from: DVec2, s: Span, a: DVec2, b: DVec2) -> Option<Span> {
    match s {
        Span::Line { to } => {
            let dir = to - from;
            ((b - a).dot(dir) > 0.0).then_some(Span::Line { to: b })
        }
        Span::Arc { centre, sweep, .. } => {
            let (ra, rb) = (a - centre, b - centre);
            let mut turn = ra.perp_dot(rb).atan2(ra.dot(rb)) * sweep.signum();
            turn = turn.rem_euclid(TAU);
            // A full turn stays one.
            if turn < 1e-9 && sweep.abs() > TAU - 1e-6 {
                turn = TAU;
            }
            (turn > 1e-12 && turn <= sweep.abs() + 1e-9).then_some(Span::Arc {
                to: b,
                centre,
                sweep: turn * sweep.signum(),
            })
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use glam::dvec2;
    use std::f64::consts::PI;

    /// A rounded rectangle `w` by `h`, corners of radius `r`, counter-clockwise,
    /// as a tessellation gives it: each corner as `steps` chords.
    fn rounded(w: f64, h: f64, r: f64, steps: u32) -> (Vec<DVec2>, Vec<(DVec2, f64)>) {
        let centres = [
            dvec2(w - r, r),
            dvec2(w - r, h - r),
            dvec2(r, h - r),
            dvec2(r, r),
        ];
        let mut pts = Vec::new();
        for (k, c) in centres.iter().enumerate() {
            let a0 = -FRAC_PI_2 + f64::from(u32::try_from(k).unwrap()) * FRAC_PI_2;
            for s in 0..=steps {
                let a = a0 + FRAC_PI_2 * f64::from(s) / f64::from(steps);
                pts.push(*c + DVec2::from_angle(a) * r);
            }
        }
        (pts, centres.iter().map(|c| (*c, r)).collect())
    }

    /// A tessellated rounded rectangle snaps to four lines and four exact
    /// quarter arcs of the model's corners.
    #[test]
    fn a_tessellated_round_corner_snaps_to_its_circle() {
        let (pts, circles) = rounded(100.0, 50.0, 3.175, 9);
        let ring = Ring::snapped(&pts, &circles, 1e-3, 0.05).unwrap();
        let arcs: Vec<&Span> = ring
            .spans()
            .iter()
            .filter(|s| matches!(s, Span::Arc { .. }))
            .collect();
        assert_eq!(arcs.len(), 4, "{ring:?}");
        for a in arcs {
            let Span::Arc { sweep, .. } = a else {
                unreachable!()
            };
            assert!((sweep - FRAC_PI_2).abs() < 1e-9, "{sweep}");
        }
        assert_eq!(ring.spans().len(), 8);
        assert!((ring.length() - (2.0 * (100.0 + 50.0 - 4.0 * 3.175) + TAU * 3.175)).abs() < 1e-9);
        assert!(!ring.starts_at_corner(), "a round ring has no corner");
    }

    /// A chord across a circle is not an arc of it: a D whose flat's ends are on
    /// the circle keeps its flat.
    #[test]
    fn a_chord_is_not_an_arc() {
        let c = dvec2(0.0, 0.0);
        let mut pts: Vec<DVec2> = (0..=18)
            .map(|k| DVec2::from_angle(-FRAC_PI_2 + PI * f64::from(k) / 18.0) * 10.0)
            .collect();
        pts.dedup();
        let ring = Ring::snapped(&pts, &[(c, 10.0)], 1e-3, 0.05).unwrap();
        assert_eq!(ring.spans().len(), 2, "{ring:?}");
        assert!(
            matches!(ring.spans()[0], Span::Arc { .. })
                != matches!(ring.spans()[1], Span::Arc { .. })
        );
    }

    /// Moved out, a rounded rectangle's arcs grow by the distance and its lines
    /// move out square to themselves: still tangent, still eight spans.
    #[test]
    fn an_offset_moves_lines_and_grows_arcs() {
        let (pts, circles) = rounded(100.0, 50.0, 3.175, 9);
        let ring = Ring::snapped(&pts, &circles, 1e-3, 0.05).unwrap();
        // Counter-clockwise: its right is outside.
        let out = ring.offset(3.175).unwrap();
        assert_eq!(out.spans().len(), 8, "{out:?}");
        for (from, s) in out.pieces() {
            if let Span::Arc { centre, .. } = s {
                assert!((from.distance(*centre) - 6.35).abs() < 1e-9);
            }
        }
        let expected = 2.0 * (100.0 + 50.0 - 4.0 * 3.175) + TAU * 6.35;
        assert!((out.length() - expected).abs() < 1e-9, "{}", out.length());
        assert!(!out.starts_at_corner());
        // Back in again, the same ring.
        let back = out.offset(-3.175).unwrap();
        assert!((back.length() - ring.length()).abs() < 1e-9);
    }

    /// Round a sharp corner the tool's centre turns an arc about the corner,
    /// the tool's radius; inside one, the moved sides are cut where they cross.
    #[test]
    fn corners_open_round_and_close_cut() {
        let square = Ring::from_points(&[
            dvec2(0.0, 0.0),
            dvec2(10.0, 0.0),
            dvec2(10.0, 10.0),
            dvec2(0.0, 10.0),
        ])
        .unwrap();
        let out = square.offset(2.0).unwrap();
        assert_eq!(out.spans().len(), 8);
        let expected = 40.0 + TAU * 2.0;
        assert!((out.length() - expected).abs() < 1e-9);
        let pts: Vec<DVec2> = out.points(0.001);
        assert!(
            pts.iter()
                .all(|p| (p.x.max(p.y) - 12.0 < 1e-9) && (p.x.min(p.y) + 2.0 > -1e-9))
        );
        let inside = square.offset(-2.0).unwrap();
        assert_eq!(inside.spans().len(), 4);
        assert!((inside.length() - 24.0).abs() < 1e-9);
        // Too far in, it turns inside out.
        assert!(square.offset(-6.0).is_none());
    }

    /// Round a ring by a distance, as often as it takes, ends that far on.
    #[test]
    fn along_a_ring() {
        let square = Ring::from_points(&[
            dvec2(0.0, 0.0),
            dvec2(10.0, 0.0),
            dvec2(10.0, 10.0),
            dvec2(0.0, 10.0),
        ])
        .unwrap();
        let (spans, end) = square.along(15.0);
        assert_eq!(spans.len(), 2);
        assert!((end - dvec2(10.0, 5.0)).length() < 1e-12);
        let (spans, end) = square.along(45.0);
        assert_eq!(spans.len(), 5, "once round and a quarter more");
        assert!((end - dvec2(5.0, 0.0)).length() < 1e-12);
        let (_, end) = square.along(40.0);
        assert!(end.length() < 1e-12, "exactly round is back at the start");
    }

    /// A ring is the same shape as a polygon only where every point of each is
    /// near the other.
    #[test]
    fn close_to_checks_both_ways() {
        let (pts, circles) = rounded(100.0, 50.0, 3.175, 9);
        let ring = Ring::snapped(&pts, &circles, 1e-3, 0.05).unwrap();
        assert!(ring.close_to(&pts, 0.02));
        let mut short = pts.clone();
        short.truncate(pts.len() / 2);
        assert!(!ring.close_to(&short, 0.02));
    }

    /// Started anywhere, reversed, or halfway along its longest span, a ring is
    /// the same shape.
    #[test]
    fn restarting_and_reversing() {
        let (pts, circles) = rounded(100.0, 50.0, 3.175, 9);
        let ring = Ring::snapped(&pts, &circles, 1e-3, 0.05).unwrap();
        let length = ring.length();
        for r in [
            ring.starting_near(dvec2(50.0, -5.0)),
            ring.starting_near(dvec2(101.0, -1.0)),
            ring.longest_middle(),
            ring.reversed(),
        ] {
            assert!((r.length() - length).abs() < 1e-9);
            assert!(r.close_to(&pts, 0.02));
        }
        let mid = ring.longest_middle();
        assert!(
            (mid.start() - dvec2(50.0, 0.0)).length() < 1e-9,
            "{:?}",
            mid.start()
        );
        let near = ring.starting_near(dvec2(101.0, -1.0));
        assert!(
            near.start()
                .distance(dvec2(100.0 - 3.175, 3.175) + DVec2::from_angle(-PI / 4.0) * 3.175)
                < 1e-9
        );
    }
}
