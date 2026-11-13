//! Picked shapes engraved with a V: Fusion's Engrave.
//!
//! # The shape
//!
//! A shape is an area seen from above, closed curves of lines and level arcs
//! round it: its outline, and the outline of any island in it. The outline runs
//! with the shape on its left, anticlockwise round the outside and clockwise
//! round each island ([`shapes`]).
//!
//! # Where the tool goes
//!
//! The V's flank touches the outline, so how deep it goes at any point is set
//! by how far the outline is: `depth = (r - tip) / tan(half angle)` for a
//! circle of radius `r` about the tool's axis that just touches the outline, no
//! deeper than the Bottom, `depth`, and no higher than the top.
//!
//! The path is a walk round the outline. At each point of it the largest circle
//! that fits in the shape and touches the outline there, no larger than the
//! Bottom allows, `cap`, is stood on the point, and the tool's axis goes
//! through its centre at the depth its radius gives. Along a straight wall of
//! a shape wider than the cap, the centre runs parallel to the wall at the
//! Bottom; coming to a convex corner it runs up the corner's bisector to the
//! corner itself at the surface and back down the other side; in a stroke
//! narrower than twice the cap it runs along the stroke's middle, as deep as
//! makes the flank touch both walls, once for each wall. At a reflex corner the
//! point of the outline stays put while its normal turns, and the centre runs
//! round what is a parabola where the circle also touches a wall, or an arc
//! at the Bottom.
//!
//! The largest circle touching the outline at `p`, its centre `p + r n` along
//! the inward normal `n`, is limited by every other piece of the outline: a
//! corner `q` limits it to `|q - p|^2 / 2 n.(q - p)`; a line or an arc, where the
//! circle touches it inside its ends, to the radius it touches at. Each stretch
//! of the walk limited by one thing is one curve: a straight one where a wall
//! limits it, or the cap does along a straight wall, which is one move; an arc
//! where the cap does along an arc or round a reflex corner; otherwise curved,
//! cut into moves within half the tolerance.
//!
//! A convex corner whose edges meet at more than the sharp corner angle is not
//! run into: the walk goes straight on from one wall's stretch to the next.
//! With a flat tip, what of the walk is narrower than the tip, where the tip
//! would only ride the surface, is left out.
//!
//! # Order
//!
//! Each shape is cut in the order given, its outline before its islands'. A
//! walk starts at the outline's lowest corner, then leftmost, that is run into,
//! at the surface, so it goes in without a plunge; and stays down round the
//! whole outline. What the walk would go back over at its end, having cut it
//! already, is left off. With multiple depths, each loop is walked once at
//! each level, no deeper than it, down to the last.

use std::f64::consts::{PI, TAU};

use glam::{DVec2, DVec3};

use crate::path::toolpath::{MoveRole, Toolpath, ToolpathError};
use crate::strategy::curve::{Curve, Piece};
use crate::strategy::planar::levels;
use crate::strategy::{StrategyError, check};

/// A shape to engrave: its outline, the outside first, the shape on the left of
/// each loop, and the height it is engraved from.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// Closed loops: the outline round it, anticlockwise, then any island's,
    /// clockwise.
    pub loops: Vec<Curve>,
    /// The surface it is engraved into: where the walk is at the top.
    pub top: f64,
}

/// How the shapes are engraved.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EngraveParams {
    /// The radius of the tool's flat tip; zero for a point.
    pub tip: f64,
    /// How far the flank rises for each millimetre out: `1 / tan(half angle)`.
    pub rise: f64,
    /// The cone's full radius.
    pub radius: f64,
    /// The deepest below its top a shape is engraved, in millimetres.
    pub depth: f64,
    /// Convex corners whose edges meet at less than this, in radians, are run
    /// into; the rest are gone round.
    pub sharp_corner: f64,
    /// The deepest a level may be below the one above, with multiple depths.
    pub stepdown: Option<f64>,
    /// Cutting feed, in mm/min.
    pub feed: f64,
    /// The job's tolerance: curved stretches are cut into moves no further
    /// than half of it from them.
    pub tolerance: f64,
}

impl EngraveParams {
    /// How far from the outline the walk's centre goes at most: where the
    /// flank touches the outline at the surface with the tip at the Bottom.
    pub fn reach(&self) -> f64 {
        self.tip + self.depth / self.rise
    }

    /// How deep the flank reaches: the most `depth` may be.
    pub fn flank(&self) -> f64 {
        (self.radius - self.tip) * self.rise
    }
}

/// What an engraving did.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EngraveReport {
    /// Shapes engraved.
    pub shapes: u64,
    /// Loops walked: outlines and islands.
    pub loops: u64,
    /// Passes, every level's.
    pub passes: u64,
    /// The most levels any shape was cut at.
    pub levels: u64,
    /// Corners run into.
    pub corners: u64,
    /// How deep the deepest point went below its shape's top, in millimetres.
    pub deepest: f64,
    /// Where the V goes furthest below where the part lets the tool rest, by
    /// more than the tolerance, seen from above, and how far, in millimetres:
    /// a shape sunk less deep in the model than the engraving goes, or a face
    /// with nothing sunk into it. Found by the run, which has the part.
    pub into_part: Option<([f64; 2], f64)>,
}

/// Why picked shapes cannot be engraved.
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum EngraveError {
    /// Nothing was picked.
    #[error("pick the shapes to engrave: the edges round each, or its floor")]
    NothingPicked,
    /// A picked outline does not close.
    #[error(
        "the edges picked do not close round a shape: they end at ({x:.3}, {y:.3}); pick \
         the whole loop"
    )]
    Open {
        /// Where.
        x: f64,
        /// Where.
        y: f64,
    },
    /// A loop that closes round nothing.
    #[error("a picked loop at ({x:.3}, {y:.3}) closes round no area")]
    NoArea {
        /// Where.
        x: f64,
        /// Where.
        y: f64,
    },
    /// Deeper than the flank reaches.
    #[error(
        "the engraving is {depth:.3} mm deep, but the tool's flank reaches only {most:.3} mm: \
         use a wider tool, or engrave less deep"
    )]
    TooDeep {
        /// How deep it was asked to go.
        depth: f64,
        /// How deep the flank reaches.
        most: f64,
    },
    /// The path could not be made.
    #[error(transparent)]
    Strategy(#[from] StrategyError),
}

impl From<ToolpathError> for EngraveError {
    fn from(e: ToolpathError) -> Self {
        EngraveError::Strategy(e.into())
    }
}

/// Whether `q` is inside the polygon `ring`, by crossings.
fn inside(ring: &[DVec2], q: DVec2) -> bool {
    let mut odd = false;
    let n = ring.len();
    for i in 0..n {
        let (Some(&a), Some(&b)) = (ring.get(i), ring.get((i + 1) % n)) else {
            continue;
        };
        if (a.y > q.y) != (b.y > q.y) {
            let x = a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if x > q.x {
                odd = !odd;
            }
        }
    }
    odd
}

/// The shapes `loops` make, each a closed curve and the height of the surface
/// it lies in, read even-odd: a loop inside an odd number of others is an
/// island of the smallest of them. Each shape is in the order its outline is
/// given, its outline anticlockwise and its islands clockwise; its top is the
/// highest of its loops'.
pub fn shapes(loops: &[(Curve, f64)], tol: f64) -> Result<Vec<Shape>, EngraveError> {
    if loops.is_empty() {
        return Err(EngraveError::NothingPicked);
    }
    let mut rings: Vec<Vec<DVec2>> = Vec::with_capacity(loops.len());
    let mut areas: Vec<f64> = Vec::with_capacity(loops.len());
    for (c, _) in loops {
        if !c.is_closed(tol) {
            let e = c.end();
            return Err(EngraveError::Open { x: e.x, y: e.y });
        }
        let area = c.signed_area();
        if area.abs() <= 1e-9 {
            return Err(EngraveError::NoArea {
                x: c.start.x,
                y: c.start.y,
            });
        }
        rings.push(c.sample(tol).iter().map(|p| p.truncate()).collect());
        areas.push(area.abs());
    }
    // A point of each loop's that is surely not on another: the middle of its
    // longest piece is as good as any.
    let probe = |i: usize| -> DVec2 {
        loops.get(i).map_or(DVec2::ZERO, |(c, _)| {
            c.steps()
                .max_by(|a, b| a.1.flat_length(a.0).total_cmp(&b.1.flat_length(b.0)))
                .map_or(c.start.truncate(), |(from, p)| {
                    p.point_at(from, 0.5).truncate()
                })
        })
    };
    let around = |i: usize| -> Vec<usize> {
        (0..loops.len())
            .filter(|&j| j != i && rings.get(j).is_some_and(|r| inside(r, probe(i))))
            .collect()
    };
    let depth: Vec<usize> = (0..loops.len()).map(|i| around(i).len()).collect();
    let mut out: Vec<(usize, Shape)> = Vec::new();
    for (i, (c, top)) in loops.iter().enumerate() {
        if depth.get(i).copied().unwrap_or(0) % 2 == 1 {
            continue;
        }
        let outer = if c.signed_area() < 0.0 {
            c.reversed()
        } else {
            c.clone()
        };
        out.push((
            i,
            Shape {
                loops: vec![outer],
                top: *top,
            },
        ));
    }
    for (i, (c, top)) in loops.iter().enumerate() {
        let d = depth.get(i).copied().unwrap_or(0);
        if d % 2 == 0 {
            continue;
        }
        // Its outline: the smallest loop round it one level out.
        let parent = around(i)
            .into_iter()
            .filter(|&j| depth.get(j).copied() == Some(d - 1))
            .min_by(|&a, &b| {
                let area = |i: usize| areas.get(i).copied().unwrap_or(f64::INFINITY);
                area(a).total_cmp(&area(b))
            });
        let hole = if c.signed_area() > 0.0 {
            c.reversed()
        } else {
            c.clone()
        };
        if let Some((_, shape)) = out.iter_mut().find(|(j, _)| Some(*j) == parent) {
            shape.top = shape.top.max(*top);
            shape.loops.push(hole);
        }
    }
    Ok(out.into_iter().map(|(_, s)| s).collect())
}

/// A piece of an outline: a line, or an arc.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Elem {
    /// From `a` to `b`.
    Line { a: DVec2, b: DVec2 },
    /// About `o`, radius `rho`, from angle `from` turning `sweep`,
    /// anticlockwise when positive.
    Arc {
        o: DVec2,
        rho: f64,
        from: f64,
        sweep: f64,
    },
}

impl Elem {
    /// The point a fraction `s` along it.
    fn at(&self, s: f64) -> DVec2 {
        match *self {
            Elem::Line { a, b } => a.lerp(b, s),
            Elem::Arc {
                o,
                rho,
                from,
                sweep,
            } => o + DVec2::from_angle(from + sweep * s) * rho,
        }
    }

    /// The way it runs a fraction `s` along it: a unit vector.
    fn tangent(&self, s: f64) -> DVec2 {
        match *self {
            Elem::Line { a, b } => (b - a).normalize_or_zero(),
            Elem::Arc { from, sweep, .. } => {
                DVec2::from_angle(from + sweep * s).perp() * sweep.signum()
            }
        }
    }

    /// The normal to its left, into the shape.
    fn normal(&self, s: f64) -> DVec2 {
        self.tangent(s).perp()
    }

    fn length(&self) -> f64 {
        match *self {
            Elem::Line { a, b } => a.distance(b),
            Elem::Arc { rho, sweep, .. } => rho * sweep.abs(),
        }
    }

    /// Its bounds, grown by `by`.
    fn bounds(&self, by: f64) -> (DVec2, DVec2) {
        let (lo, hi) = match *self {
            Elem::Line { a, b } => (a.min(b), a.max(b)),
            Elem::Arc { o, rho, .. } => (o - DVec2::splat(rho), o + DVec2::splat(rho)),
        };
        (lo - DVec2::splat(by), hi + DVec2::splat(by))
    }
}

/// `curve`, closed, as pieces of outline: lines that run on straight joined as
/// one, and nothing of no length.
fn elems(curve: &Curve) -> Vec<Elem> {
    let mut out: Vec<Elem> = Vec::new();
    let push = |e: Elem, out: &mut Vec<Elem>| {
        if e.length() <= 1e-9 {
            return;
        }
        if let (Elem::Line { b, .. }, Some(Elem::Line { a: pa, b: pb })) = (e, out.last().copied())
        {
            let (u, v) = ((pb - pa).normalize_or_zero(), (b - pb).normalize_or_zero());
            if u.perp_dot(v).abs() <= 1e-9 && u.dot(v) > 0.0 {
                if let Some(last) = out.last_mut() {
                    *last = Elem::Line { a: pa, b };
                }
                return;
            }
        }
        out.push(e);
    };
    for (from, p) in curve.steps() {
        let a = from.truncate();
        match *p {
            Piece::Line { to } => push(
                Elem::Line {
                    a,
                    b: to.truncate(),
                },
                &mut out,
            ),
            Piece::Arc { centre, sweep, .. } => {
                let v = a - centre;
                push(
                    Elem::Arc {
                        o: centre,
                        rho: v.length(),
                        from: v.y.atan2(v.x),
                        sweep,
                    },
                    &mut out,
                );
            }
        }
    }
    let (end, start) = (curve.end().truncate(), curve.start.truncate());
    if end.distance(start) > 1e-9 {
        push(Elem::Line { a: end, b: start }, &mut out);
    }
    // The last line and the first, where they run on straight, are one.
    if out.len() > 2
        && let (Some(Elem::Line { a: la, b: lb }), Some(Elem::Line { a: fa, b: fb })) =
            (out.last().copied(), out.first().copied())
    {
        let (u, v) = ((lb - la).normalize_or_zero(), (fb - fa).normalize_or_zero());
        if u.perp_dot(v).abs() <= 1e-9 && u.dot(v) > 0.0 && lb.distance(fa) <= 1e-9 {
            out.pop();
            if let Some(first) = out.first_mut() {
                *first = Elem::Line { a: la, b: fb };
            }
        }
    }
    out
}

/// What limits the circle at a point of the walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Who {
    /// The Bottom: the circle is as large as it may be.
    Cap,
    /// A piece of the outline, by loop and place, touched inside its ends.
    Piece(usize, usize),
    /// A corner, by loop and place.
    Corner(usize, usize),
}

/// What limits the circle, and whether the circle is narrower than the tip.
type Key = (Who, bool);

/// Where along the outline the walk is: along a piece, or turning at a
/// reflex corner.
#[derive(Debug, Clone, Copy)]
enum Track {
    /// Piece `elem` of loop `ring`.
    Along { ring: usize, elem: usize },
    /// Round the reflex corner `at`, between pieces `before` and `after` of
    /// loop `ring`, the normal turning from `n0` by `turn` radians.
    Fan {
        ring: usize,
        at: DVec2,
        n0: DVec2,
        turn: f64,
        before: usize,
        after: usize,
    },
}

/// A shape's outline, as the walk measures circles against it.
struct Outline {
    rings: Vec<Vec<Elem>>,
    bounds: Vec<Vec<(DVec2, DVec2)>>,
    corners: Vec<Vec<DVec2>>,
    cap: f64,
}

impl Outline {
    fn new(shape: &Shape, cap: f64) -> Self {
        let rings: Vec<Vec<Elem>> = shape.loops.iter().map(elems).collect();
        let bounds = rings
            .iter()
            .map(|r| r.iter().map(|e| e.bounds(2.0 * cap)).collect())
            .collect();
        let corners = rings
            .iter()
            .map(|r| r.iter().map(|e| e.at(0.0)).collect())
            .collect();
        Self {
            rings,
            bounds,
            corners,
            cap,
        }
    }

    fn elem(&self, ring: usize, i: usize) -> Option<Elem> {
        self.rings.get(ring).and_then(|r| r.get(i)).copied()
    }

    /// The largest circle at `p`, its centre along `n`, no larger than the
    /// cap, the pieces in `skip` aside: its radius and what limits it.
    fn largest(&self, p: DVec2, n: DVec2, skip: &[(usize, usize)]) -> (f64, Who) {
        let mut best = (self.cap, Who::Cap);
        let mut take = |r: f64, who: Who| {
            if r.is_finite() && r >= 0.0 && r < best.0 {
                best = (r, who);
            }
        };
        for (ri, ring) in self.rings.iter().enumerate() {
            for (ei, e) in ring.iter().enumerate() {
                let Some(&(lo, hi)) = self.bounds.get(ri).and_then(|b| b.get(ei)) else {
                    continue;
                };
                if p.x < lo.x || p.y < lo.y || p.x > hi.x || p.y > hi.y {
                    continue;
                }
                // Its corner where it starts.
                if let Some(&q) = self.corners.get(ri).and_then(|c| c.get(ei)) {
                    let d = q - p;
                    let dn = d.dot(n);
                    if d.length() > 1e-9 && dn > 1e-12 * d.length() {
                        take(d.length_squared() / (2.0 * dn), Who::Corner(ri, ei));
                    }
                }
                if skip.contains(&(ri, ei)) {
                    continue;
                }
                match *e {
                    Elem::Line { a, b } => {
                        let len = a.distance(b);
                        let u = (b - a) / len;
                        let m = u.perp();
                        let off = m.dot(p - a);
                        let denom = 1.0 - m.dot(n);
                        if off.abs() <= 1e-12 || denom <= 1e-12 {
                            continue;
                        }
                        let r = off / denom;
                        if r <= 0.0 {
                            continue;
                        }
                        let c = p + n * r;
                        let along = (c - m * r - a).dot(u);
                        if (0.0..=len).contains(&along) {
                            take(r, Who::Piece(ri, ei));
                        }
                    }
                    Elem::Arc {
                        o,
                        rho,
                        from,
                        sweep,
                    } => {
                        let d = p.distance(o);
                        let into = sweep > 0.0;
                        if (d - rho).abs() <= 1e-9 {
                            // On its circle: within it, no larger than it.
                            if into {
                                take(rho, Who::Piece(ri, ei));
                            }
                            continue;
                        }
                        let po = p - o;
                        let r = if into {
                            if d >= rho {
                                continue;
                            }
                            (rho * rho - d * d) / (2.0 * (n.dot(po) + rho))
                        } else {
                            if d <= rho {
                                continue;
                            }
                            (d * d - rho * rho) / (2.0 * (rho - n.dot(po)))
                        };
                        if r.is_nan() || r <= 0.0 {
                            continue;
                        }
                        let c = p + n * r - o;
                        if c.length() <= 1e-12 {
                            if into {
                                take(r, Who::Piece(ri, ei));
                            }
                            continue;
                        }
                        let mut round = (c.y.atan2(c.x) - from) * sweep.signum();
                        round = round.rem_euclid(TAU);
                        if round <= sweep.abs() + 1e-12 {
                            take(r, Who::Piece(ri, ei));
                        }
                    }
                }
            }
        }
        best
    }
}

/// One loop's walk: the outline, and the tool.
struct Walk<'a> {
    outline: &'a Outline,
    p: &'a EngraveParams,
    top: f64,
}

impl Walk<'_> {
    /// The point of the outline and its normal a fraction `t` along `track`,
    /// and the pieces whose own circles are not measured there.
    fn point(&self, track: Track, t: f64) -> (DVec2, DVec2, [(usize, usize); 2]) {
        match track {
            Track::Along { ring, elem } => {
                let Some(e) = self.outline.elem(ring, elem) else {
                    return (DVec2::ZERO, DVec2::Y, [(ring, elem); 2]);
                };
                // Never quite at a corner, where the piece beyond would read
                // as touched there.
                let edge = (1e-7 / e.length()).min(1e-3);
                let t = t.clamp(edge, 1.0 - edge);
                // A line is its own; an arc limits the circle on it.
                let own = match e {
                    Elem::Line { .. } => (ring, elem),
                    Elem::Arc { .. } => (usize::MAX, usize::MAX),
                };
                (e.at(t), e.normal(t), [own; 2])
            }
            Track::Fan {
                ring,
                at,
                n0,
                turn,
                before,
                after,
            } => (
                at,
                DVec2::from_angle(turn * t).rotate(n0),
                [(ring, before), (ring, after)],
            ),
        }
    }

    /// Where the tool's tip is a fraction `t` along `track`, and what puts it
    /// there.
    fn centre(&self, track: Track, t: f64) -> (DVec3, Key) {
        let (q, n, skip) = self.point(track, t);
        let (r, who) = self.outline.largest(q, n, &skip);
        let deep = ((r - self.p.tip) * self.p.rise).clamp(0.0, self.p.depth);
        ((q + n * r).extend(self.top - deep), (who, r < self.p.tip))
    }

    /// Where along `track` what limits the circle changes, between `t0` and
    /// `t1`, found to within a part in 1e13: each where, and what after.
    fn changes(
        &self,
        track: Track,
        (t0, k0): (f64, Key),
        (t1, k1): (f64, Key),
        out: &mut Vec<(f64, Key)>,
    ) {
        let mut stack = vec![(t0, k0, t1, k1, 0u32)];
        // Bounded: each step halves its stretch, at most 48 times deep, and
        // at most 4096 halvings between two samples, however many times what
        // limits the circle changes between them.
        let mut halvings = 0u32;
        while let Some((a, ka, b, kb, depth)) = stack.pop() {
            if ka == kb {
                continue;
            }
            if depth >= 48 || b - a <= 1e-13 || halvings >= 4096 {
                out.push((b, kb));
                continue;
            }
            halvings += 1;
            let m = 0.5 * (a + b);
            let (_, km) = self.centre(track, m);
            // The later half first off the stack is the earlier half.
            stack.push((m, km, b, kb, depth + 1));
            stack.push((a, ka, m, km, depth + 1));
        }
    }

    /// `track`'s stretches, each from where to where and what limits it.
    fn stretches(&self, track: Track) -> Vec<(f64, f64, Key)> {
        let samples: u32 = match track {
            Track::Along { ring, elem } => {
                let len = self.outline.elem(ring, elem).map_or(0.0, |e| e.length());
                camcraft_geom::steps::bounded_count((len / (0.25 * self.outline.cap)).ceil())
                    .clamp(4, 4096)
            }
            Track::Fan { turn, .. } => {
                camcraft_geom::steps::bounded_count((turn.abs() / (PI / 32.0)).ceil()).clamp(2, 64)
            }
        };
        let at = |i: u32| f64::from(i) / f64::from(samples);
        let mut changes: Vec<(f64, Key)> = Vec::new();
        let mut last = (0.0, self.centre(track, 0.0).1);
        for i in 1..=samples {
            let here = (at(i), self.centre(track, at(i)).1);
            self.changes(track, last, here, &mut changes);
            last = here;
        }
        let mut out = Vec::with_capacity(changes.len() + 1);
        let mut from = (0.0, self.centre(track, 0.0).1);
        for (t, k) in changes {
            if t > from.0 {
                out.push((from.0, t, from.1));
            }
            from = (t, k);
        }
        if from.0 < 1.0 {
            out.push((from.0, 1.0, from.1));
        }
        out
    }

    /// The stretch of `track` from `t0` to `t1`, limited by `who`, as pieces
    /// from where it starts, which is put in `out` first if it is empty.
    fn emit(
        &self,
        track: Track,
        (t0, t1): (f64, f64),
        who: Who,
        out: &mut Vec<Piece>,
        start: &mut Option<DVec3>,
    ) {
        let (a, _) = self.centre(track, t0);
        let (b, _) = self.centre(track, t1);
        if start.is_none() {
            *start = Some(a);
        }
        let elem = match track {
            Track::Along { ring, elem } => self.outline.elem(ring, elem),
            Track::Fan { .. } => None,
        };
        // A wall or the cap along a straight wall: straight.
        let line_limited = match who {
            Who::Cap => true,
            Who::Piece(ri, ei) => {
                matches!(self.outline.elem(ri, ei), Some(Elem::Line { .. }))
            }
            Who::Corner(..) => false,
        };
        match (elem, who) {
            (Some(Elem::Line { .. }), _) if line_limited => {
                out.push(Piece::Line { to: b });
                return;
            }
            // The cap round an arc, or round a reflex corner: an arc.
            (Some(Elem::Arc { o, sweep, .. }), Who::Cap) => {
                out.push(Piece::Arc {
                    to: b,
                    centre: o,
                    sweep: sweep * (t1 - t0),
                });
                return;
            }
            (None, Who::Cap) => {
                if let Track::Fan { at, turn, .. } = track {
                    out.push(Piece::Arc {
                        to: b,
                        centre: at,
                        sweep: turn * (t1 - t0),
                    });
                    return;
                }
            }
            _ => {}
        }
        // Anything else is curved: halved until each move is within half the
        // tolerance of it.
        let most = 0.5 * self.p.tolerance;
        let mut stack = vec![(t0, a, t1, b, 0u32)];
        // Bounded: at most 12 halvings deep, 4096 moves a stretch.
        while let Some((ta, ca, tb, cb, depth)) = stack.pop() {
            let off = |t: f64| segment_distance(self.centre(track, t).0, ca, cb);
            let span = tb - ta;
            let close = off(ta + 0.5 * span) <= most
                && off(ta + 0.25 * span) <= most
                && off(ta + 0.75 * span) <= most;
            if close || depth >= 12 {
                out.push(Piece::Line { to: cb });
                continue;
            }
            let tm = ta + 0.5 * span;
            let (cm, _) = self.centre(track, tm);
            stack.push((tm, cm, tb, cb, depth + 1));
            stack.push((ta, ca, tm, cm, depth + 1));
        }
    }
}

/// How far `q` is from the segment from `a` to `b`, in space.
fn segment_distance(q: DVec3, a: DVec3, b: DVec3) -> f64 {
    let ab = b - a;
    let l2 = ab.length_squared();
    if l2 <= 1e-24 {
        return q.distance(a);
    }
    let t = ((q - a).dot(ab) / l2).clamp(0.0, 1.0);
    q.distance(a + ab * t)
}

/// The way a corner turns, from one piece to the next: positive to the left,
/// a convex corner of the shape.
fn turn(before: Elem, after: Elem) -> f64 {
    let (u, v) = (before.tangent(1.0), after.tangent(0.0));
    u.perp_dot(v).atan2(u.dot(v))
}

/// The smallest turn that is a corner at all, in radians.
const STRAIGHT: f64 = 1e-6;

/// One loop of `outline` walked: runs of pieces, each a start and its pieces,
/// broken where the tip would only ride the surface; and the corners it ran
/// into.
fn walk_loop(walk: &Walk<'_>, ring: usize) -> (Vec<Curve>, u64) {
    let Some(elems) = walk.outline.rings.get(ring) else {
        return (Vec::new(), 0);
    };
    let n = elems.len();
    if n == 0 {
        return (Vec::new(), 0);
    }
    let elem = |i: usize| elems.get(i % n).copied();
    // The turn at the corner where piece `i` starts.
    let turn_at = |i: usize| match (elem(i + n - 1), elem(i)) {
        (Some(b), Some(a)) => turn(b, a),
        _ => 0.0,
    };
    // Run into: convex, and sharper than the sharp corner angle.
    let sharp = |i: usize| {
        let t = turn_at(i);
        t > STRAIGHT && PI - t < walk.p.sharp_corner
    };
    let blunt = |i: usize| {
        let t = turn_at(i);
        t > STRAIGHT && PI - t >= walk.p.sharp_corner
    };
    // Start at the lowest corner run into, then the leftmost; failing one,
    // the lowest corner.
    let lowest = |i: &usize, j: &usize| {
        let (a, b) = (
            elem(*i).map_or(DVec2::ZERO, |e| e.at(0.0)),
            elem(*j).map_or(DVec2::ZERO, |e| e.at(0.0)),
        );
        a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x))
    };
    let first = (0..n)
        .filter(|&i| sharp(i))
        .min_by(lowest)
        .or_else(|| (0..n).min_by(lowest))
        .unwrap_or(0);
    let mut runs: Vec<Curve> = Vec::new();
    let mut pieces: Vec<Piece> = Vec::new();
    let mut start: Option<DVec3> = None;
    let mut corners = 0;
    let close = |pieces: &mut Vec<Piece>, start: &mut Option<DVec3>, runs: &mut Vec<Curve>| {
        if let Some(s) = start.take()
            && !pieces.is_empty()
        {
            runs.push(Curve {
                start: s,
                pieces: std::mem::take(pieces),
            });
        }
        pieces.clear();
    };
    for k in 0..n {
        let i = (first + k) % n;
        let next = (i + 1) % n;
        let track = Track::Along { ring, elem: i };
        let before = (i + n - 1) % n;
        if sharp(i) {
            corners += 1;
        }
        for (t0, t1, (who, shallow)) in walk.stretches(track) {
            // Gone round a blunt corner: not up its bisector.
            if blunt(i) && who == Who::Piece(ring, before) && t0 <= 1e-9 {
                continue;
            }
            if blunt(next) && who == Who::Piece(ring, next) && t1 >= 1.0 - 1e-9 {
                continue;
            }
            if shallow {
                close(&mut pieces, &mut start, &mut runs);
                continue;
            }
            // Joined to where the run is, if it was left short of here.
            let (here, _) = walk.centre(track, t0);
            if let Some(s) = start {
                let end = pieces.last().map_or(s, Piece::to);
                if end.distance(here) > 1e-6 {
                    pieces.push(Piece::Line { to: here });
                }
            }
            walk.emit(track, (t0, t1), who, &mut pieces, &mut start);
        }
        // Round a reflex corner at its end.
        let t = turn_at(next);
        if t < -STRAIGHT
            && let (Some(e), Some(_)) = (elem(i), elem(next))
        {
            let fan = Track::Fan {
                ring,
                at: e.at(1.0),
                n0: e.normal(1.0),
                turn: t,
                before: i,
                after: next,
            };
            for (t0, t1, (who, shallow)) in walk.stretches(fan) {
                if shallow {
                    close(&mut pieces, &mut start, &mut runs);
                    continue;
                }
                walk.emit(fan, (t0, t1), who, &mut pieces, &mut start);
            }
        }
    }
    close(&mut pieces, &mut start, &mut runs);
    // A corner run into is reached exactly, at the surface.
    for run in &mut runs {
        snap_corners(run, elems, walk.top);
    }
    (runs, corners)
}

/// Each point of `run` within a micron of a corner of `elems`, seen from above,
/// put at it at `top`.
fn snap_corners(run: &mut Curve, elems: &[Elem], top: f64) {
    let snap = |p: DVec3| -> DVec3 {
        elems
            .iter()
            .map(|e| e.at(0.0))
            .find(|c| c.distance(p.truncate()) <= 1e-6 && (p.z - top).abs() <= 1e-5)
            .map_or(p, |c| c.extend(top))
    };
    run.start = snap(run.start);
    for p in &mut run.pieces {
        if let Piece::Line { to } = p {
            *to = snap(*to);
        }
    }
}

/// `run` no deeper than `level`.
fn no_deeper(run: &Curve, level: f64) -> Curve {
    let mut out = Curve {
        start: run.start.max(DVec3::new(f64::MIN, f64::MIN, level)),
        pieces: Vec::with_capacity(run.pieces.len()),
    };
    let mut from = run.start;
    for p in &run.pieces {
        match *p {
            Piece::Line { to } => {
                // Where it crosses the level, it turns there.
                if (from.z - level) * (to.z - level) < 0.0 {
                    let t = (from.z - level) / (from.z - to.z);
                    let mut x = from.lerp(to, t);
                    x.z = level;
                    out.pieces.push(Piece::Line { to: x });
                }
                let mut to2 = to;
                to2.z = to2.z.max(level);
                out.pieces.push(Piece::Line { to: to2 });
            }
            Piece::Arc { to, centre, sweep } => {
                let mut to2 = to;
                to2.z = to2.z.max(level);
                out.pieces.push(Piece::Arc {
                    to: to2,
                    centre,
                    sweep,
                });
            }
        }
        from = p.to();
    }
    joined_straight(&out)
}

/// `run` with lines that run on straight, in space, joined.
fn joined_straight(run: &Curve) -> Curve {
    let mut pieces: Vec<Piece> = Vec::with_capacity(run.pieces.len());
    let mut from = run.start;
    // Where the last line kept starts.
    let mut last_from = run.start;
    for p in &run.pieces {
        let to = p.to();
        if to.distance(from) <= 1e-9 && matches!(p, Piece::Line { .. }) {
            continue;
        }
        if let (Piece::Line { .. }, Some(Piece::Line { to: was })) = (p, pieces.last().copied()) {
            let (u, v) = (
                (was - last_from).normalize_or_zero(),
                (to - was).normalize_or_zero(),
            );
            if u.cross(v).length() <= 1e-9 && u.dot(v) > 0.0 {
                if let Some(last) = pieces.last_mut() {
                    *last = Piece::Line { to };
                }
                from = to;
                continue;
            }
        }
        if matches!(p, Piece::Line { .. }) {
            last_from = from;
        }
        pieces.push(*p);
        from = to;
    }
    Curve {
        start: run.start,
        pieces,
    }
}

/// Points along `curve`, within `tol` of it.
fn points(curve: &Curve, tol: f64) -> Vec<DVec3> {
    let mut out = vec![curve.start];
    for (from, p) in curve.steps() {
        if let Piece::Arc { .. } = p {
            let n = camcraft_geom::steps::bounded_count(
                (p.flat_length(from) / tol.max(1e-3)).sqrt().ceil() * 2.0,
            )
            .clamp(2, 256);
            for k in 1..n {
                out.push(p.point_at(from, f64::from(k) / f64::from(n)));
            }
        }
        out.push(p.to());
    }
    out
}

/// Whether the polyline `line` passes within `tol` of `q`.
fn near(line: &[DVec3], q: DVec3, tol: f64) -> bool {
    line.windows(2).any(|w| match w {
        [a, b] => segment_distance(q, *a, *b) <= tol,
        _ => false,
    }) || line.first().is_some_and(|a| a.distance(q) <= tol)
}

/// `run`, the last of a loop's, with what it goes back over at its end that
/// was cut before it, or earlier in it, `before`, left off.
fn without_retraced_end(run: &Curve, before: &[Curve], tol: f64) -> Curve {
    let mut run = run.clone();
    let close = 1.01 * tol + 1e-6;
    let earlier: Vec<Vec<DVec3>> = before.iter().map(|c| points(c, tol)).collect();
    // Bounded: a piece is taken off each time round.
    while let Some(&last) = run.pieces.last() {
        let n = run.pieces.len();
        let from = if n >= 2 {
            run.pieces.get(n - 2).map_or(run.start, Piece::to)
        } else {
            run.start
        };
        let head = Curve {
            start: run.start,
            pieces: run
                .pieces
                .get(..n - 1)
                .map(<[Piece]>::to_vec)
                .unwrap_or_default(),
        };
        let mine = points(&head, tol);
        let covered = |q: DVec3| near(&mine, q, close) || earlier.iter().any(|l| near(l, q, close));
        let gone = [0.0, 0.25, 0.5, 0.75, 1.0]
            .iter()
            .all(|&t| covered(last.point_at(from, t)));
        if !gone || n == 1 {
            break;
        }
        run.pieces.pop();
    }
    run
}

/// `island`, a closed walk, gone round from the first point of `run` it meets
/// within `tol`, at an end of a piece of each, and `run` gone on from there:
/// `None` where they do not meet, or `island` is not closed.
fn spliced(run: &Curve, island: &Curve, tol: f64) -> Option<Curve> {
    if !island.is_closed(1e-6) {
        return None;
    }
    let ends = |c: &Curve| -> Vec<DVec3> {
        std::iter::once(c.start)
            .chain(c.pieces.iter().map(Piece::to))
            .collect()
    };
    let (here, there) = (ends(run), ends(island));
    let m = island.pieces.len();
    let (i, j) = here.iter().enumerate().find_map(|(i, a)| {
        there
            .iter()
            .take(m)
            .position(|b| a.distance(*b) <= tol)
            .map(|j| (i, j))
    })?;
    let (a, b) = (*here.get(i)?, *there.get(j)?);
    let mut pieces: Vec<Piece> = run.pieces.get(..i)?.to_vec();
    if a.distance(b) > 1e-9 {
        pieces.push(Piece::Line { to: b });
    }
    pieces.extend_from_slice(island.pieces.get(j..)?);
    pieces.extend_from_slice(island.pieces.get(..j)?);
    if a.distance(b) > 1e-9 {
        pieces.push(Piece::Line { to: a });
    }
    pieces.extend_from_slice(run.pieces.get(i..)?);
    Some(Curve {
        start: run.start,
        pieces,
    })
}

fn check_params(p: &EngraveParams) -> Result<(), EngraveError> {
    check("feed", p.feed)?;
    check("tolerance", p.tolerance)?;
    check("depth", p.depth)?;
    check("flank", p.rise)?;
    check("tool radius", p.radius)?;
    if !(p.tip >= 0.0 && p.tip < p.radius) {
        return Err(StrategyError::BadParameter {
            name: "tip radius",
            value: p.tip,
        }
        .into());
    }
    if !(p.sharp_corner >= 0.0 && p.sharp_corner <= PI + 1e-12) {
        return Err(StrategyError::BadParameter {
            name: "sharp corner angle",
            value: p.sharp_corner,
        }
        .into());
    }
    if let Some(d) = p.stepdown {
        check("stepdown", d)?;
    }
    if p.depth > p.flank() + 1e-9 {
        return Err(EngraveError::TooDeep {
            depth: p.depth,
            most: p.flank(),
        });
    }
    Ok(())
}

/// `shapes` engraved: Fusion's Engrave. Each pass a link to where it starts and
/// the walk.
pub fn engrave(
    shapes: &[Shape],
    p: &EngraveParams,
) -> Result<(Toolpath, EngraveReport), EngraveError> {
    check_params(p)?;
    if shapes.is_empty() {
        return Err(EngraveError::NothingPicked);
    }
    let mut report = EngraveReport::default();
    let mut path = Toolpath::new();
    for shape in shapes {
        report.shapes += 1;
        let outline = Outline::new(shape, p.reach());
        let walk = Walk {
            outline: &outline,
            p,
            top: shape.top,
        };
        let floors = levels(shape.top, shape.top - p.depth, p.stepdown);
        report.levels = report.levels.max(floors.len() as u64);
        let mut walks: Vec<Vec<Curve>> = Vec::with_capacity(outline.rings.len());
        for ring in 0..outline.rings.len() {
            report.loops += 1;
            let (runs, corners) = walk_loop(&walk, ring);
            report.corners += corners;
            walks.push(runs);
        }
        // An island's walk that meets the outline's, in a channel narrower
        // than twice the reach, is gone round from where they meet: one pass.
        let mut rest: Vec<Vec<Curve>> = walks.split_off(1.min(walks.len()));
        if let Some(outer) = walks.first_mut() {
            rest.retain(|island| {
                let [only] = island.as_slice() else {
                    return true;
                };
                for run in outer.iter_mut() {
                    if let Some(joined) = spliced(run, only, p.tolerance) {
                        *run = joined;
                        return false;
                    }
                }
                true
            });
        }
        walks.extend(rest);
        let mut cut: Vec<Curve> = Vec::new();
        for runs in walks {
            let count = floors.len();
            for (l, &floor) in floors.iter().enumerate() {
                let last_level = l + 1 == count;
                let total = runs.len();
                for (k, run) in runs.iter().enumerate() {
                    let mut run = no_deeper(run, floor);
                    if last_level && k + 1 == total {
                        run = without_retraced_end(&run, &cut, p.tolerance);
                    }
                    if run.pieces.is_empty() {
                        continue;
                    }
                    let deepest = run
                        .pieces
                        .iter()
                        .map(|q| shape.top - q.to().z)
                        .fold(shape.top - run.start.z, f64::max);
                    report.deepest = report.deepest.max(deepest);
                    // One run on from where the last ended is the same pass.
                    let there = path.end().is_some_and(|e| e.distance(run.start) <= 1e-9);
                    if !there {
                        path.feed_to(run.start, MoveRole::Link, p.feed)?;
                        report.passes += 1;
                    }
                    run.cut_into(&mut path, MoveRole::Cut, p.feed)?;
                    cut.push(run);
                }
            }
        }
    }
    Ok((path, report))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::panic
)]
mod tests;
