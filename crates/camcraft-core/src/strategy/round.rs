//! Round walls: holes and bosses picked by their walls, and milled round them.
//! Fusion's Bore and Circular.
//!
//! # From the picked walls
//!
//! A hole or a boss is picked by its round wall, a face, as Fusion's Bore and
//! Circular select theirs. A face whose edges are circles about one upright axis,
//! all the same size, is a straight round wall ([`rounds`]); which side of it the
//! material is on says whether it is a hole or a boss. A wall split into several
//! faces, as a file often splits a cylinder in halves, is one wall: the faces on
//! one axis, the same size and the same way round, are taken together, from the
//! highest of them to the lowest.
//!
//! # Passes out to the wall
//!
//! The tool's centre goes round a circle its radius and the stock to leave inside
//! a hole's wall, or outside a boss's. With more than one stepover, the passes
//! before it are a stepover apart further from the wall: from the middle out in a
//! hole, from outside in round a boss, so the last pass is the one at the wall. A
//! hole's pass that would be past its middle is left out, as Fusion's Bore leaves
//! it out, and the report counts it. A hole the passes do not reach the middle
//! of is left with a core standing in it, as Fusion's are, and the report says
//! so.
//!
//! A bore's finishing passes go on out from where those stop, a finishing
//! stepover apart, the last at the wall.
//!
//! # Bore
//!
//! Each pass starts at the top, comes onto its circle along a level lead from the
//! waste, goes down a helix round it to the bottom, at the pitch (or as steeply
//! as the angle asked for takes it round the circle the tool's outer edge
//! sweeps, as Fusion's does), once round level there,
//! and off along a lead. Every move is a true arc, of at most half a turn: one
//! that ends where it starts, seen from above, reads to the linker's ramp as
//! straight down.
//!
//! # Circular
//!
//! The same circles, cut level by level: at each level, each pass from the
//! middle out comes onto its circle along a lead, goes once round it, and leads
//! off, as Fusion's Circular does. The linker takes the tool down onto each
//! lead along its vertical arc, into the material where the level is still
//! there, and lifts it out after.
//!
//! # Direction
//!
//! Climb by default. With a clockwise spindle a climb cut keeps the material on
//! the tool's right: round a hole that is anticlockwise, round a boss clockwise.

use std::f64::consts::{PI, TAU};

use camcraft_geom::ring::Span;
use glam::{DVec2, DVec3, dvec3};

use crate::path::toolpath::{Arc, MoveRole, PLUNGE_RATIO, Toolpath, ToolpathError};
use crate::strategy::nesting::take_nearest;
use crate::strategy::planar::{LeadArc, WallFinish, levels};
use crate::strategy::{StrategyError, check, slabs};

/// A circle no smaller than this, as the tool's centre goes, in millimetres: a
/// hole that leaves its centre less room is no wider than the tool.
const LEAST_RADIUS: f64 = 1.0e-3;

/// A pass round a hole on a circle no bigger than this, as the tool's centre
/// goes, is past its middle, and left out, in millimetres.
const SLIVER: f64 = 0.1;

/// The most turns a helix makes down one pass.
const MOST_TURNS: f64 = 10_000.0;

/// One straight round wall, a hole's or a boss's, in the setup's frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Round {
    /// Where its axis is.
    pub at: DVec2,
    /// Its radius.
    pub radius: f64,
    /// The height it is cut down from.
    pub top: f64,
    /// The height the tool's tip goes down to.
    pub bottom: f64,
    /// A boss, cut round from outside; otherwise a hole, cut from inside.
    pub boss: bool,
}

/// A picked face, as found on its model in the setup's frame: what [`rounds`]
/// reads a round wall from.
#[derive(Debug, Clone, PartialEq)]
pub struct Wall {
    /// The model it is on.
    pub model: String,
    /// Its number there.
    pub face: u32,
    /// Its edges that are circles or arcs of them: each a centre, a radius, and
    /// whether it lies level.
    pub circles: Vec<(DVec3, f64, bool)>,
    /// Its triangles, wound as the model winds them: outward.
    pub triangles: Vec<[DVec3; 3]>,
}

/// Why picked faces are not round walls, or a round wall cannot be cut.
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum RoundError {
    /// Nothing was picked.
    #[error("pick the round wall of each hole or boss to cut")]
    NothingPicked,
    /// A picked face has no round edge, or its round edges are not about one
    /// axis.
    #[error("face {face} of '{model}' is not a round wall")]
    NotRound {
        /// The model.
        model: String,
        /// Its number there.
        face: u32,
    },
    /// A picked round wall does not stand upright in the setup.
    #[error("the round wall at face {face} of '{model}' does not stand upright in this setup")]
    NotUpright {
        /// The model.
        model: String,
        /// Its number there.
        face: u32,
    },
    /// A picked round wall narrows or widens as it goes down.
    #[error("face {face} of '{model}' is a tapered wall: only straight walls are cut yet")]
    Tapered {
        /// The model.
        model: String,
        /// Its number there.
        face: u32,
    },
    /// A hole no wider than the tool and the stock to leave beside it.
    #[error(
        "the hole at ({x:.3}, {y:.3}) is {across:.3} mm across: no wider than the \
         {tool:.3} mm tool and the stock to leave beside it; drill it, or use a smaller tool"
    )]
    TooSmall {
        /// Where.
        x: f64,
        /// Where.
        y: f64,
        /// Its diameter.
        across: f64,
        /// The tool's.
        tool: f64,
    },
    /// A helix that would fall more steeply than a plunge.
    #[error(
        "the helix round the circle {across:.3} mm across at ({x:.3}, {y:.3}) falls \
         {pitch:.3} mm a turn, more steeply than a plunge: use a smaller pitch"
    )]
    TooSteep {
        /// Where.
        x: f64,
        /// Where.
        y: f64,
        /// The circle's diameter, as the tool's centre goes.
        across: f64,
        /// How far it falls a turn.
        pitch: f64,
    },
    /// A hole or boss whose bottom is at or above its top.
    #[error("the round wall at ({x:.3}, {y:.3}) has its bottom at or above its top")]
    NoDepth {
        /// Where.
        x: f64,
        /// Where.
        y: f64,
    },
    /// The path could not be made.
    #[error(transparent)]
    Strategy(#[from] StrategyError),
}

impl From<ToolpathError> for RoundError {
    fn from(e: ToolpathError) -> Self {
        RoundError::Strategy(e.into())
    }
}

/// The round walls `walls` make, in the order first picked: one per axis, size
/// and side, from the highest of its faces to the lowest. `tol` is how close two
/// circles' axes and sizes must be to be one wall's.
pub fn rounds(walls: &[Wall], tol: f64) -> Result<Vec<Round>, RoundError> {
    if walls.is_empty() {
        return Err(RoundError::NothingPicked);
    }
    let tol = tol.max(1.0e-6);
    let mut out: Vec<Round> = Vec::new();
    for w in walls {
        let not_round = || RoundError::NotRound {
            model: w.model.clone(),
            face: w.face,
        };
        let Some(&(first, _, _)) = w.circles.first() else {
            return Err(not_round());
        };
        if w.circles.iter().any(|&(_, _, level)| !level) {
            return Err(RoundError::NotUpright {
                model: w.model.clone(),
                face: w.face,
            });
        }
        let at = first.truncate();
        if w.circles
            .iter()
            .any(|(c, _, _)| c.truncate().distance(at) > tol)
        {
            return Err(not_round());
        }
        let (least, most) = w
            .circles
            .iter()
            .fold((f64::MAX, f64::MIN), |(lo, hi), &(_, r, _)| {
                (lo.min(r), hi.max(r))
            });
        if most - least > tol {
            return Err(RoundError::Tapered {
                model: w.model.clone(),
                face: w.face,
            });
        }
        let radius = most;
        // How high it goes, from its triangles; from its circles where it has
        // none.
        let zs = w
            .triangles
            .iter()
            .flatten()
            .map(|p| p.z)
            .chain(w.circles.iter().map(|(c, _, _)| c.z));
        let (bottom, top) = zs.fold((f64::MAX, f64::MIN), |(lo, hi), z| (lo.min(z), hi.max(z)));
        // Which way its triangles face, from the axis or towards it, weighed
        // by their size: a hole's face the empty middle, a boss's face out.
        let facing: f64 = w
            .triangles
            .iter()
            .map(|[a, b, c]| {
                let n = (*b - *a).cross(*c - *a);
                let middle = (*a + *b + *c) / 3.0;
                n.truncate().dot(middle.truncate() - at)
            })
            .sum();
        let boss = facing > 0.0;
        let same = |r: &Round| {
            r.at.distance(at) <= tol && (r.radius - radius).abs() <= tol && r.boss == boss
        };
        match out.iter_mut().find(|r| same(r)) {
            Some(r) => {
                r.top = r.top.max(top);
                r.bottom = r.bottom.min(bottom);
            }
            None => out.push(Round {
                at,
                radius,
                top,
                bottom,
                boss,
            }),
        }
    }
    Ok(out)
}

/// `rounds` in the order cut: the first, then always the nearest not yet cut.
/// Ties go to the earlier, so the order depends on nothing but the rounds.
pub fn nearest_first(rounds: &[Round]) -> Vec<Round> {
    let mut left: Vec<Round> = rounds.to_vec();
    let mut out = Vec::with_capacity(left.len());
    let Some(mut at) = left.first().map(|r| r.at) else {
        return out;
    };
    while let Some(r) = take_nearest(&mut left, |r| r.at.distance_squared(at)) {
        at = r.at;
        out.push(r);
    }
    out
}

/// How the passes round each wall are laid out, a bore's and a circular's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoundParams {
    /// The tool's radius.
    pub radius: f64,
    /// The stock to leave on the wall, in millimetres; negative cuts past it.
    pub leave: f64,
    /// How many passes out to the wall, the last at it.
    pub stepovers: u32,
    /// The distance between them, in millimetres.
    pub stepover: f64,
    /// A bore's finishing passes, out from where those stop to the wall.
    pub finishing: Option<WallFinish>,
    /// Whether the last pass is cut twice.
    pub repeat: bool,
    /// Cutting feed, in mm/min.
    pub feed: f64,
    /// Whether the material kept is on the tool's right: climbing with a
    /// clockwise spindle.
    pub kept_on_right: bool,
    /// The level arcs each pass is entered and left along, if any.
    pub leads: Option<LeadArc>,
    /// Each lead from the hole's centre, and back to it.
    pub to_centre: bool,
}

/// How far a bore's helix goes down in a turn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Down {
    /// So many millimetres a turn, round every circle.
    Pitch(f64),
    /// At this angle from level, in radians, as Fusion's Bore sets it: each
    /// turn down as far as the angle takes it round the circle the tool's
    /// outer edge sweeps, the pass's circle widened by the tool.
    Angle(f64),
}

/// A circular's levels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Levels {
    /// The deepest a level may be below the one above, in millimetres; the
    /// bottom alone when `None`.
    pub stepdown: Option<f64>,
    /// Spaced evenly, rather than each a whole stepdown and the last what is
    /// left.
    pub even: bool,
}

/// What a bore or a circular did.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RoundReport {
    /// Holes cut.
    pub holes: u64,
    /// Bosses cut round.
    pub bosses: u64,
    /// Passes round them, every level's.
    pub passes: u64,
    /// The most levels any was cut in.
    pub levels: u64,
    /// Holes the passes do not reach the middle of, so leave a core standing.
    pub cores: u64,
    /// Passes asked for past a hole's middle, and left out.
    pub dropped: u64,
    /// The widest core left, in millimetres across.
    pub widest_core: f64,
    /// Passes asked for leads that had no room for them inside the hole, and
    /// are entered and left without.
    pub no_room_to_lead: u64,
}

/// One pass round a wall: the radius of its circle, as the tool's centre goes,
/// and its feed.
type Pass = (f64, f64);

fn check_params(p: &RoundParams) -> Result<(), StrategyError> {
    check("tool radius", p.radius)?;
    check("feed", p.feed)?;
    if !p.leave.is_finite() {
        return Err(StrategyError::BadParameter {
            name: "stock to leave",
            value: p.leave,
        });
    }
    if p.stepovers > 1 {
        check("stepover", p.stepover)?;
    }
    if let Some(f) = &p.finishing {
        check("finishing stepover", f.stepover)?;
        check("finishing feed", f.feed)?;
    }
    if let Some(l) = &p.leads {
        check("lead radius", l.radius)?;
        check("lead sweep", l.sweep)?;
        crate::strategy::at_least_zero("lead straight", l.straight)?;
    }
    Ok(())
}

/// The passes round `round`, in the order cut; how wide a core they leave
/// standing in the middle of a hole, if any; and how many past its middle
/// were left out.
fn passes(round: &Round, p: &RoundParams) -> Result<(Vec<Pass>, Option<f64>, u64), RoundError> {
    let out = if round.boss { 1.0 } else { -1.0 };
    let wall = round.radius + out * (p.radius + p.leave);
    if !round.boss && (wall.is_nan() || wall <= LEAST_RADIUS) {
        return Err(RoundError::TooSmall {
            x: round.at.x,
            y: round.at.y,
            across: 2.0 * round.radius,
            tool: 2.0 * p.radius,
        });
    }
    let (count, step, last_feed) = p
        .finishing
        .map_or((0, 0.0, p.feed), |f| (f.passes, f.stepover, f.feed));
    let mut all: Vec<Pass> = Vec::new();
    let roughing = wall + out * f64::from(count) * step;
    for k in (0..p.stepovers.max(1)).rev() {
        all.push((roughing + out * f64::from(k) * p.stepover, p.feed));
    }
    for j in (0..count).rev() {
        let feed = if j == 0 { last_feed } else { p.feed };
        all.push((wall + out * f64::from(j) * step, feed));
    }
    let asked = all.len();
    if round.boss {
        // Further out is further from the wall: nothing to keep from.
        all.retain(|(r, _)| r.is_finite());
    } else {
        // A pass past the middle is left out, as Fusion's Bore leaves it out
        // ("Some passes were discarded"), never the one at the wall. Where
        // Fusion's cut-off lies between nothing and a millimetre is not
        // known: here it is a sliver more than nothing.
        let last = all.len().saturating_sub(1);
        all = all
            .into_iter()
            .enumerate()
            .filter(|&(i, (r, _))| i == last || r > SLIVER)
            .map(|(_, pass)| pass)
            .collect();
    }
    let dropped = (asked - all.len()) as u64;
    if p.repeat
        && let Some(last) = all.last().copied()
    {
        all.push(last);
    }
    let core = if round.boss {
        None
    } else {
        all.first()
            .map(|(r, _)| r - p.radius)
            .filter(|&gap| gap > 1.0e-6)
            .map(|gap| 2.0 * gap)
    };
    Ok((all, core, dropped))
}

/// Which way round the passes round `round` go: anticlockwise, seen from above,
/// or not. Round a hole, the material is outside the circle: on the right of a
/// pass going anticlockwise.
fn anticlockwise(round: &Round, p: &RoundParams) -> bool {
    p.kept_on_right != round.boss
}

/// How a pass comes onto its circle and goes off it.
#[derive(Debug, Clone, PartialEq)]
struct Lead {
    /// Where the lead onto the circle starts.
    start: DVec2,
    /// Onto the circle, from `start`.
    lead_in: Vec<Span>,
}

/// The point at `angle` round the circle of `rho` about `centre`.
fn on_circle(centre: DVec2, rho: f64, angle: f64) -> DVec2 {
    centre + DVec2::from_angle(angle) * rho
}

/// The leads onto the circle of `rho` about `round`'s axis at `angle` and off
/// it at `off`, in the waste: inside a hole, outside a boss. Each the largest of
/// the asked arc, its half and its quarter that stays inside a hole's circle;
/// from the centre and back, a half turn each, `to_centre`. `None` for each that
/// does not fit, or was not asked for.
fn leads(
    round: &Round,
    rho: f64,
    (angle, off): (f64, f64),
    ccw: bool,
    p: &RoundParams,
) -> (Option<Lead>, Option<Vec<Span>>) {
    let c = round.at;
    let Some(asked) = p.leads else {
        return (None, None);
    };
    let to_centre = p.to_centre && !round.boss;
    // In a hole, the arc no wider than half the circle, as Fusion's is made
    // smaller where there is no room; then, where the straight still leaves
    // it, half and a quarter of both.
    let radius = if round.boss {
        asked.radius
    } else {
        asked.radius.min(0.5 * rho)
    };
    let shapes: Vec<(f64, f64, f64)> = if to_centre {
        vec![(0.5 * rho, PI, 0.0)]
    } else {
        [1.0, 0.5, 0.25]
            .into_iter()
            .map(|k| (radius * k, asked.sweep, asked.straight * k))
            .collect()
    };
    // Which way the leads turn: round a hole as the pass does, about a centre
    // inside the circle; round a boss the other way, about one outside it.
    let turn = if ccw == round.boss { -1.0 } else { 1.0 };
    let inside = |pts: &[DVec2]| {
        round.boss
            || pts
                .iter()
                .all(|q| q.distance(c) <= rho * (1.0 + 1.0e-9) + 1.0e-9)
    };
    let geometry = |at: f64, (a, sweep, straight): (f64, f64, f64), into: bool| {
        if a <= LEAST_RADIUS {
            return None;
        }
        let u = DVec2::from_angle(at);
        let p0 = c + u * rho;
        let n = if round.boss { u } else { -u };
        let q = p0 + n * a;
        let s = turn * sweep;
        // The arc's far end, and the way the tool goes there.
        let far = q + DVec2::from_angle(if into { -s } else { s }).rotate(p0 - q);
        let heading = (far - q).perp() * turn / a;
        let mut pts: Vec<DVec2> = (1..16)
            .map(|k| {
                let f = f64::from(k) / 16.0;
                q + DVec2::from_angle(if into { -s * f } else { s * f }).rotate(p0 - q)
            })
            .collect();
        pts.push(far);
        let end = if into {
            far - heading * straight
        } else {
            far + heading * straight
        };
        pts.push(end);
        inside(&pts).then_some((p0, q, s, far, end, straight > 0.0))
    };
    let lead_in = shapes.iter().find_map(|&shape| {
        let (p0, q, s, far, end, straight) = geometry(angle, shape, true)?;
        let mut spans = Vec::new();
        if straight {
            spans.push(Span::Line { to: far });
        }
        spans.push(Span::Arc {
            to: p0,
            centre: q,
            sweep: s,
        });
        Some(Lead {
            start: if straight { end } else { far },
            lead_in: spans,
        })
    });
    let lead_out = shapes.iter().find_map(|&shape| {
        let (_, q, s, far, end, straight) = geometry(off, shape, false)?;
        let mut spans = vec![Span::Arc {
            to: far,
            centre: q,
            sweep: s,
        }];
        if straight {
            spans.push(Span::Line { to: end });
        }
        Some(spans)
    });
    (lead_in, lead_out)
}

/// A span at height `z`, from where the path is.
fn level(
    path: &mut Toolpath,
    span: &Span,
    z: f64,
    role: MoveRole,
    feed: f64,
) -> Result<(), ToolpathError> {
    match *span {
        Span::Line { to } => path.feed_to(dvec3(to.x, to.y, z), role, feed),
        Span::Arc { to, centre, sweep } => path.arc_to(
            dvec3(to.x, to.y, z),
            Arc {
                centre: dvec3(centre.x, centre.y, z),
                axis: DVec3::Z,
                sweep,
            },
            role,
            feed,
        ),
    }
}

/// The circles round `round` gone down a helix, each pass from the top to the
/// bottom: Fusion's Bore. Each pass a link to where it starts at the top, a
/// lead onto its circle, the helix, once round level at the bottom, and a lead
/// off.
pub fn bore(
    rounds: &[Round],
    down: Down,
    p: &RoundParams,
) -> Result<(Toolpath, RoundReport), RoundError> {
    check_params(p)?;
    match down {
        Down::Pitch(pitch) => check("pitch", pitch)?,
        Down::Angle(a) if !(a > 0.0 && a < 0.5 * PI) => {
            return Err(StrategyError::BadParameter {
                name: "ramp angle",
                value: a,
            }
            .into());
        }
        Down::Angle(_) => {}
    }
    let mut path = Toolpath::new();
    let mut report = RoundReport::default();
    for round in rounds {
        let depth = round.top - round.bottom;
        if depth.is_nan() || depth <= 1.0e-9 {
            return Err(RoundError::NoDepth {
                x: round.at.x,
                y: round.at.y,
            });
        }
        let (all, core, dropped) = passes(round, p)?;
        count(round, core, &mut report);
        report.dropped += dropped;
        report.levels = report.levels.max(1);
        let ccw = anticlockwise(round, p);
        let way = if ccw { 1.0 } else { -1.0 };
        for (rho, feed) in all {
            // By its angle, as Fusion's Bore: the circle the tool's outer
            // edge sweeps, its diameter times the angle's tangent, a turn.
            let pitch = match down {
                Down::Pitch(pitch) => pitch,
                Down::Angle(a) => 2.0 * (rho + p.radius) * a.tan(),
            };
            if pitch > PLUNGE_RATIO * TAU * rho {
                return Err(RoundError::TooSteep {
                    x: round.at.x,
                    y: round.at.y,
                    across: 2.0 * rho,
                    pitch,
                });
            }
            let turns = depth / pitch;
            if !(turns.is_finite() && turns <= MOST_TURNS) {
                return Err(StrategyError::BadParameter {
                    name: "pitch",
                    value: pitch,
                }
                .into());
            }
            let total = way * TAU * turns;
            let (lead, off) = leads(round, rho, (0.0, total), ccw, p);
            if p.leads.is_some() && (lead.is_none() || off.is_none()) {
                report.no_room_to_lead += 1;
            }
            helix(
                &mut path,
                round,
                (rho, total),
                (lead.as_ref(), off.as_deref()),
                feed,
            )?;
            report.passes += 1;
        }
    }
    Ok((path, report))
}

/// One bore pass round `round`: down a helix round the circle of `rho` from
/// the top to the bottom, turning `total` radians, anticlockwise when positive.
fn helix(
    path: &mut Toolpath,
    round: &Round,
    (rho, total): (f64, f64),
    (lead, off): (Option<&Lead>, Option<&[Span]>),
    feed: f64,
) -> Result<(), RoundError> {
    let c = round.at;
    let (top, bottom) = (round.top, round.bottom);
    let start = on_circle(c, rho, 0.0);
    let arrive = lead.map_or(start, |l| l.start);
    path.feed_to(dvec3(arrive.x, arrive.y, top), MoveRole::Link, feed)?;
    for span in lead.map_or(&[][..], |l| l.lead_in.as_slice()) {
        level(path, span, top, MoveRole::Lead, feed)?;
    }
    round_about(path, c, rho, (0.0, total), (top, bottom), feed)?;
    // Once round level at the bottom, as Fusion's bore ends: the helix leaves
    // the floor a turn's fall higher on one side than the other.
    round_about(
        path,
        c,
        rho,
        (total, total + TAU.copysign(total)),
        (bottom, bottom),
        feed,
    )?;
    for span in off.unwrap_or(&[]) {
        level(path, span, bottom, MoveRole::Lead, feed)?;
    }
    Ok(())
}

/// The circles round `round` cut level by level: Fusion's Circular. At each
/// level, each pass a link to where its lead starts, the lead onto its circle,
/// once round, and a lead off.
pub fn circular(
    rounds: &[Round],
    levels_asked: Levels,
    p: &RoundParams,
) -> Result<(Toolpath, RoundReport), RoundError> {
    check_params(p)?;
    if let Some(d) = levels_asked.stepdown {
        check("stepdown", d)?;
    }
    let mut path = Toolpath::new();
    let mut report = RoundReport::default();
    // Its last pass is cut twice at the last level only.
    let once = RoundParams {
        repeat: false,
        ..*p
    };
    for round in rounds {
        let zs: Vec<f64> = match levels_asked.stepdown {
            Some(d) if !levels_asked.even && round.top - round.bottom > 1.0e-9 => {
                slabs(round.top, round.bottom, d, &[])
                    .into_iter()
                    .map(|(z, _)| z)
                    .collect()
            }
            stepdown => levels(round.top, round.bottom, stepdown),
        };
        if zs.is_empty() {
            return Err(RoundError::NoDepth {
                x: round.at.x,
                y: round.at.y,
            });
        }
        let (all, core, dropped) = passes(round, &once)?;
        count(round, core, &mut report);
        report.dropped += dropped;
        report.levels = report.levels.max(zs.len() as u64);
        let ccw = anticlockwise(round, p);
        let way = if ccw { 1.0 } else { -1.0 };
        let last = zs.len() - 1;
        for (i, &z) in zs.iter().enumerate() {
            let mut at_level = all.clone();
            if p.repeat
                && i == last
                && let Some(wall) = all.last().copied()
            {
                at_level.push(wall);
            }
            for &(rho, feed) in &at_level {
                let (lead, off) = leads(round, rho, (0.0, 0.0), ccw, p);
                if p.leads.is_some() && (lead.is_none() || off.is_none()) {
                    report.no_room_to_lead += 1;
                }
                ring(
                    &mut path,
                    round.at,
                    (rho, z, way),
                    (lead.as_ref(), off.as_deref()),
                    feed,
                )?;
                report.passes += 1;
            }
        }
    }
    Ok((path, report))
}

/// Once round the circle of `rho` about `c` at height `z`, from its point on
/// +X, anticlockwise when `way` is positive: a link to where the lead onto it
/// starts, the lead, the circle, and the lead off.
fn ring(
    path: &mut Toolpath,
    c: DVec2,
    (rho, z, way): (f64, f64, f64),
    (lead, off): (Option<&Lead>, Option<&[Span]>),
    feed: f64,
) -> Result<(), RoundError> {
    let start = on_circle(c, rho, 0.0);
    let arrive = lead.map_or(start, |l| l.start);
    path.feed_to(dvec3(arrive.x, arrive.y, z), MoveRole::Link, feed)?;
    for span in lead.map_or(&[][..], |l| l.lead_in.as_slice()) {
        level(path, span, z, MoveRole::Lead, feed)?;
    }
    round_about(path, c, rho, (0.0, TAU.copysign(way)), (z, z), feed)?;
    for span in off.unwrap_or(&[]) {
        level(path, span, z, MoveRole::Lead, feed)?;
    }
    Ok(())
}

/// Round the circle of `rho` about `c`, cutting, from angle `from` to `to`
/// (anticlockwise where `to` is the greater), from height `high` to `low`
/// evenly as it turns: level, or a helix. In arcs of at most half a turn, so
/// no move ends where it starts, seen from above, and each is one a program
/// writes as it is.
fn round_about(
    path: &mut Toolpath,
    c: DVec2,
    rho: f64,
    (from, to): (f64, f64),
    (high, low): (f64, f64),
    feed: f64,
) -> Result<(), ToolpathError> {
    let total = to - from;
    let n = (2.0 * total.abs() / TAU - 1.0e-9).ceil().max(1.0);
    let n = camcraft_geom::steps::bounded_count(n);
    for k in 1..=n {
        let f = f64::from(k) / f64::from(n);
        let at = on_circle(c, rho, from + total * f);
        path.arc_to(
            dvec3(at.x, at.y, high + (low - high) * f),
            Arc {
                centre: dvec3(c.x, c.y, low),
                axis: DVec3::Z,
                sweep: total / f64::from(n),
            },
            MoveRole::Cut,
            feed,
        )?;
    }
    Ok(())
}

/// `round` counted in `report`, with the `core` its passes leave, if any.
fn count(round: &Round, core: Option<f64>, report: &mut RoundReport) {
    if round.boss {
        report.bosses += 1;
    } else {
        report.holes += 1;
    }
    if let Some(across) = core {
        report.cores += 1;
        report.widest_core = report.widest_core.max(across);
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::panic
)]
mod tests;
