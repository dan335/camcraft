//! Picked edges chamfered: Fusion's 2D Chamfer.
//!
//! # Where the tool goes
//!
//! A chamfer mill or V-bit is a cone: its flank rises `rise` for each unit
//! out from its tip's radius, `rise` being `1 / tan` of its half-angle. Seen
//! square to an edge, with `d` the distance across from the edge into the
//! part, the chamfer is the line the flank runs along: through the point the
//! chamfer width in from the edge at the edge's height, `z = z_edge + (d -
//! width) * rise`. On a sharp edge that line cuts a chamfer `width` across
//! and `width * rise` down; on a chamfer the model has, picked by its lower
//! edge, it is the model's own face moved `width` into the part. The tool's
//! tip goes the tip offset further down that line, below the chamfer's lower
//! edge, so its axis is its tip's radius and `tip_offset * tan` out from the
//! edge on the side away from the part, and its tip `width * rise +
//! tip_offset` below the edge: how far out does not change with the width.
//!
//! # Which side
//!
//! Each curve is run with the part on the side a climbing cut keeps it,
//! the tool's right with a clockwise spindle, so the tool is offset from it
//! the other way ([`super::curve::offset`]), its outside corners rolled
//! round, as Fusion's Outer Corner Mode starts.
//!
//! # On and off
//!
//! Onto each pass along a straight from the waste, square to it, and off it
//! the same: a tenth of the tool, as a new Fusion 2D Chamfer's linear lead-in
//! and lead-out. A closed curve's pass goes on past where it started by the
//! finishing overlap before it leaves.
//!
//! # Clearance
//!
//! Where the tool, grown by the chamfer clearance, would come down onto the
//! part's faces other than those its edges join ([`ChamferParams::lift`]),
//! it is lifted until it does not, and the chamfer is narrower there.
//!
//! # What it means to cut
//!
//! Each edge's chamfer is kept with the report as a [`Bevel`]: what the part
//! is meant to be once chamfered, so the verifier can tell a chamfer cut
//! into the part on purpose from a gouge ([`Bevel::height_at`]).

use glam::{DVec2, DVec3};

use crate::path::toolpath::{MoveRole, Toolpath, ToolpathError};
use crate::strategy::curve::{Curve, Piece, offset};
use crate::strategy::nesting::take_nearest;
use crate::strategy::{StrategyError, at_least_zero, check};

/// An edge to chamfer: a curve along it, and which side of it the part is.
#[derive(Debug, Clone, PartialEq)]
pub struct ChamferEdge {
    /// The edge, as picked.
    pub curve: Curve,
    /// Whether the part is on the curve's left as it runs, seen from above.
    pub part_on_left: bool,
}

/// How the edges are chamfered.
#[derive(Clone, Copy)]
pub struct ChamferParams<'a> {
    /// The tool's flat tip's radius, in millimetres.
    pub tip: f64,
    /// The tool's widest radius, in millimetres.
    pub radius: f64,
    /// How far its flank rises for each unit out: `1 / tan` of its
    /// half-angle.
    pub rise: f64,
    /// The chamfer width, in millimetres.
    pub width: f64,
    /// How far below the chamfer's lower edge the tip goes, in millimetres.
    pub tip_offset: f64,
    /// How far past its start a closed edge's pass goes on, in millimetres.
    pub overlap: f64,
    /// How long the straight onto and off each pass is, in millimetres.
    pub lead: f64,
    /// The edges in the order given, rather than always the nearest next.
    pub preserve_order: bool,
    /// Whether the material cut is kept on the tool's right: climbing with
    /// a clockwise spindle.
    pub kept_on_right: bool,
    /// Cutting feed, in mm/min.
    pub feed: f64,
    /// How close two ends must be to be one, and how far apart the points a
    /// pass is checked for clearance at may be, in millimetres.
    pub tolerance: f64,
    /// Where the tool's tip must be at least, at a point seen from above, to
    /// keep the chamfer clearance off the part's other faces: `None` where
    /// it is clear at any height. `None` for no clearance.
    pub lift: Option<&'a (dyn Fn(DVec2) -> Option<f64> + Sync)>,
}

impl std::fmt::Debug for ChamferParams<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChamferParams")
            .field("tip", &self.tip)
            .field("radius", &self.radius)
            .field("rise", &self.rise)
            .field("width", &self.width)
            .field("tip_offset", &self.tip_offset)
            .field("lift", &self.lift.is_some())
            .finish_non_exhaustive()
    }
}

/// An edge's chamfer as it is meant to be cut: the part, once chamfered, is
/// no higher than the flank's line across from the edge, on the part's side
/// of it and within the flank's reach. What the verifier lowers the part to
/// where the chamfer cut, so a chamfer of the width asked is not a gouge.
#[derive(Debug, Clone, PartialEq)]
pub struct Bevel {
    /// Points along the edge, no further from it than the tolerance.
    pub points: Vec<DVec3>,
    /// Whether it closes on itself.
    pub closed: bool,
    /// Whether the part is on its left as it runs.
    pub part_on_left: bool,
    /// The chamfer width, in millimetres.
    pub width: f64,
    /// How far the flank rises for each unit across.
    pub rise: f64,
    /// How far across from the edge into the part the flank reaches, in
    /// millimetres: as far as the tool is wide.
    pub reach: f64,
}

impl Bevel {
    /// How high the part is meant to be at `p`, seen from above, once this
    /// chamfer is cut: the flank's line, `z_edge + (d - width) * rise`, `d`
    /// across from the nearest point of the edge, where `p` is on the part's
    /// side of it and within the flank's reach; `None` anywhere else.
    pub fn height_at(&self, p: DVec2) -> Option<f64> {
        let (mut best, mut at) = (f64::INFINITY, None);
        let n = self.points.len();
        let segments = if self.closed { n } else { n.saturating_sub(1) };
        for i in 0..segments {
            let (Some(a), Some(b)) = (self.points.get(i), self.points.get((i + 1) % n)) else {
                continue;
            };
            let (a2, b2) = (a.truncate(), b.truncate());
            let ab = b2 - a2;
            let l2 = ab.length_squared();
            let t = if l2 <= 1e-18 {
                0.0
            } else {
                ((p - a2).dot(ab) / l2).clamp(0.0, 1.0)
            };
            let q = a2 + ab * t;
            let d = p.distance(q);
            if d < best {
                best = d;
                at = Some((i, t, a.z + (b.z - a.z) * t));
            }
        }
        let (i, _, z) = at?;
        // On the edge itself, either side is the part's.
        if best > self.reach || (best > 1e-9 && !self.on_part_side(p, i)) {
            return None;
        }
        Some(z + (best - self.width) * self.rise)
    }

    /// Whether `p`, nearest segment `i` of the edge, is on the part's side.
    fn on_part_side(&self, p: DVec2, i: usize) -> bool {
        if self.closed {
            // Inside a loop run anticlockwise is on its left.
            let inside = winds_round(&self.points, p);
            let ccw = signed_area(&self.points) > 0.0;
            return inside == (ccw == self.part_on_left);
        }
        let n = self.points.len();
        let (Some(a), Some(b)) = (self.points.get(i), self.points.get((i + 1).min(n - 1))) else {
            return false;
        };
        let left = (b.truncate() - a.truncate()).perp_dot(p - a.truncate()) > 0.0;
        left == self.part_on_left
    }
}

/// Whether the closed polygon `points` winds round `p`, seen from above.
fn winds_round(points: &[DVec3], p: DVec2) -> bool {
    let n = points.len();
    let mut inside = false;
    for i in 0..n {
        let (Some(a), Some(b)) = (points.get(i), points.get((i + 1) % n)) else {
            continue;
        };
        let (a, b) = (a.truncate(), b.truncate());
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if p.x < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// The area the closed polygon `points` encloses, seen from above: positive
/// anticlockwise.
fn signed_area(points: &[DVec3]) -> f64 {
    let n = points.len();
    let twice: f64 = (0..n)
        .filter_map(|i| {
            let a = points.get(i)?.truncate();
            let b = points.get((i + 1) % n)?.truncate();
            Some(a.perp_dot(b))
        })
        .sum();
    twice / 2.0
}

/// What a 2D chamfer did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChamferReport {
    /// Edges chamfered, each joined end to end with those it runs on into.
    pub edges: u64,
    /// Of them, closed.
    pub closed: u64,
    /// Passes.
    pub passes: u64,
    /// Stretches lifted to keep the chamfer clearance.
    pub lifted: u64,
    /// How far below the chamfered edges the tool's tip goes, in millimetres.
    pub depth: f64,
    /// Each edge's chamfer, as it is meant to be cut.
    pub bevels: Vec<Bevel>,
}

/// Why picked edges cannot be chamfered.
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum ChamferError {
    /// Nothing was picked.
    #[error("pick the edges to chamfer, or the faces whose edges to chamfer")]
    NothingPicked,
    /// Which side of an edge the part is could not be told.
    #[error(
        "cannot tell which side of the edge at ({x:.3}, {y:.3}) the part is: pick an edge \
         with the part on one side of it and lower ground on the other"
    )]
    NoSide {
        /// Where.
        x: f64,
        /// Where.
        y: f64,
    },
    /// An edge that turns too tightly for the tool to keep beside it.
    #[error(
        "the edge turns too tightly at ({x:.3}, {y:.3}) for the tool to keep beside it: use a \
         tool with a smaller tip, or a smaller tip offset"
    )]
    TooTight {
        /// Where.
        x: f64,
        /// Where.
        y: f64,
    },
    /// The chamfer is wider than the tool's flank reaches.
    #[error(
        "a chamfer {width:.3} mm wide with a {tip_offset:.3} mm tip offset needs the flank to \
         reach {needed:.3} mm from the tool's axis, but it reaches {radius:.3} mm: use a wider \
         tool, or a smaller chamfer width or tip offset"
    )]
    TooWide {
        /// The width asked.
        width: f64,
        /// The tip offset asked.
        tip_offset: f64,
        /// How far out it needs.
        needed: f64,
        /// How far out the tool reaches.
        radius: f64,
    },
    /// The path could not be made.
    #[error(transparent)]
    Strategy(#[from] StrategyError),
}

impl From<ToolpathError> for ChamferError {
    fn from(e: ToolpathError) -> Self {
        ChamferError::Strategy(e.into())
    }
}

fn check_params(p: &ChamferParams<'_>) -> Result<(), ChamferError> {
    check("feed", p.feed)?;
    check("tolerance", p.tolerance)?;
    check("tool radius", p.radius)?;
    check("flank rise", p.rise)?;
    at_least_zero("tip radius", p.tip)?;
    at_least_zero("chamfer width", p.width)?;
    at_least_zero("tip offset", p.tip_offset)?;
    at_least_zero("finishing overlap", p.overlap)?;
    at_least_zero("lead", p.lead)?;
    let needed = p.tip + p.tip_offset / p.rise + p.width;
    if needed > p.radius + 1e-9 {
        return Err(ChamferError::TooWide {
            width: p.width,
            tip_offset: p.tip_offset,
            needed,
            radius: p.radius,
        });
    }
    Ok(())
}

/// The first `run` millimetres of `curve`, seen from above, at most all of it.
fn head(curve: &Curve, run: f64) -> Curve {
    curve.reversed().back_from_end(run)
}

/// `curve`'s points no further apart than `step` seen from above, and no
/// further from it than `tol`, from its start to its end.
fn points_along(curve: &Curve, step: f64, tol: f64) -> Vec<DVec3> {
    let sampled = curve.sample(tol);
    let mut out: Vec<DVec3> = Vec::with_capacity(sampled.len());
    for w in sampled.windows(2) {
        let [a, b] = w else { continue };
        let n = camcraft_geom::steps::bounded_count(
            (a.truncate().distance(b.truncate()) / step).ceil(),
        );
        for k in 0..n {
            out.push(a.lerp(*b, f64::from(k) / f64::from(n)));
        }
    }
    out.extend(sampled.last());
    out
}

/// `edges` chamfered: Fusion's 2D Chamfer. Each a pass of its own, a link to
/// where its lead starts, the lead, the edge as the tool's tip follows it,
/// and the lead off it.
pub fn chamfer(
    edges: &[ChamferEdge],
    p: &ChamferParams<'_>,
) -> Result<(Toolpath, ChamferReport), ChamferError> {
    check_params(p)?;
    if edges.is_empty() {
        return Err(ChamferError::NothingPicked);
    }
    let tol = p.tolerance;
    // How far out from the edge the axis is, and how far below it the tip.
    let out = p.tip + p.tip_offset / p.rise;
    let depth = p.width * p.rise + p.tip_offset;
    let mut report = ChamferReport {
        depth,
        ..ChamferReport::default()
    };
    // Each run with the part on the side climbing keeps the material: the
    // right with `kept_on_right`.
    let runs: Vec<ChamferEdge> = edges
        .iter()
        .map(|e| {
            if e.part_on_left == p.kept_on_right {
                ChamferEdge {
                    curve: e.curve.reversed(),
                    part_on_left: !e.part_on_left,
                }
            } else {
                e.clone()
            }
        })
        .collect();
    let ordered = if p.preserve_order {
        runs
    } else {
        let mut left = runs;
        let mut out = Vec::with_capacity(left.len());
        let mut at = left.first().map(|e| e.curve.start);
        while let Some(e) = take_nearest(&mut left, |e| {
            at.map_or(0.0, |a| a.truncate().distance(e.curve.start.truncate()))
        }) {
            at = Some(e.curve.end());
            out.push(e);
        }
        out
    };
    let mut path = Toolpath::new();
    for e in &ordered {
        let closed = e.curve.is_closed(tol);
        report.edges += 1;
        report.closed += u64::from(closed);
        report.bevels.push(Bevel {
            points: {
                let mut pts = e.curve.sample(tol * 0.5);
                if closed {
                    pts.pop();
                }
                pts
            },
            closed,
            part_on_left: e.part_on_left,
            width: p.width,
            rise: p.rise,
            reach: (p.radius - out).max(0.0),
        });
        // Away from the part: the left is positive.
        let away = if e.part_on_left { -1.0 } else { 1.0 };
        let beside = offset(&e.curve, away * out, tol).map_err(|t| ChamferError::TooTight {
            x: t.at.x,
            y: t.at.y,
        })?;
        let mut pass = beside.raised(-depth);
        if closed && p.overlap > 0.0 {
            let more = head(&pass, p.overlap);
            pass.pieces.extend(more.pieces);
        }
        // Square to the pass, into the waste, at each end.
        let side = |h: DVec2| h.perp() * away;
        let start = pass.start;
        let end = pass.end();
        let lead_from = start + (side(pass.heading(false)) * p.lead).extend(0.0);
        let lead_to = end + (side(pass.heading(true)) * p.lead).extend(0.0);
        let mut whole = Curve {
            start: lead_from,
            pieces: Vec::with_capacity(pass.pieces.len() + 2),
        };
        if p.lead > 0.0 {
            whole.pieces.push(Piece::Line { to: start });
        } else {
            whole.start = start;
        }
        whole.pieces.extend(pass.pieces.iter().copied());
        if p.lead > 0.0 {
            whole.pieces.push(Piece::Line { to: lead_to });
        }
        path.feed_to(whole.start, MoveRole::Link, p.feed)?;
        match p.lift {
            Some(lift) => cut_clear(&whole, lift, p, &mut path, &mut report)?,
            None => whole.cut_into(&mut path, MoveRole::Cut, p.feed)?,
        }
        report.passes += 1;
    }
    Ok((path, report))
}

/// `pass` cut into `path`, lifted where `lift` says the tool must be higher
/// to keep its clearance: as it is where it need not be anywhere, else as
/// points along it, each at least as high as it must be.
fn cut_clear(
    pass: &Curve,
    lift: &(dyn Fn(DVec2) -> Option<f64> + Sync),
    p: &ChamferParams<'_>,
    path: &mut Toolpath,
    report: &mut ChamferReport,
) -> Result<(), ChamferError> {
    let step = (p.radius * 0.25).max(p.tolerance);
    let points = points_along(pass, step, p.tolerance);
    let raised: Vec<DVec3> = points
        .iter()
        .map(|q| match lift(q.truncate()) {
            Some(z) if z > q.z => DVec3::new(q.x, q.y, z),
            _ => *q,
        })
        .collect();
    let mut was = false;
    let mut lifted = 0;
    for (q, r) in points.iter().zip(&raised) {
        let up = r.z > q.z + p.tolerance;
        if up && !was {
            lifted += 1;
        }
        was = up;
    }
    report.lifted += lifted;
    if lifted == 0 {
        pass.cut_into(path, MoveRole::Cut, p.feed)?;
        return Ok(());
    }
    for r in raised.iter().skip(1) {
        if path.end().is_none_or(|e| e.distance(*r) > 1e-9) {
            path.feed_to(*r, MoveRole::Cut, p.feed)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::panic
)]
mod tests;
