//! Linking: turning a strategy's holes into real traverses.
//!
//! A strategy emits [`MoveRole::Link`] moves meaning "get here, somehow". This
//! resolves each: staying down where the next cut is near and the way there is
//! clear, otherwise out of the cut, across at a safe height, and back in. A
//! program that still contains links is not machinable, and [`Toolpath::has_unplanned_links`]
//! makes that checkable rather than assumed.
//!
//! What it does is Fusion's, and why is in `docs/decisions/0004-linking-and-entries.md`.
//!
//! # Clearance is a field, not a plane
//!
//! [`Clearance`] asks for the safe height *over a given traverse*, not for a single
//! number. [`ClearancePlane`] is a constant plane, which is all a flat spoilboard
//! needs; [`ClearanceField`] follows the material, so the tool stays low where it
//! can. Fixture and clamp avoidance, "keep tool down", and stay-down distance all
//! need the answer to vary with position too, and each is one more implementation
//! rather than a change at every call site.
//!
//! # Measured against the material, as it is cut
//!
//! Where the tool may rapid and where it must feed is decided by the material under
//! it ([`Material`]): the operation's stock, with every move linked so far already
//! cut into it. So a re-entry knows what the passes before it cleared. The tool
//! rapids down only to [`LinkParams::safe_distance`] above the material and feeds
//! from there, and comes up out of a cut at feed only until it is that far clear.
//! Nothing rapids out of material, and nothing plunges through metres of air.
//!
//! # Staying down, leads and ramps
//!
//! A link no longer than the stay-down distance, whose way is clear, is made
//! without lifting ([`StayDown`]). A pass is entered and left along a vertical arc
//! where one fits the part ([`LinkParams::lead_radius`]), and a pass that starts in
//! material is ramped into along its own path rather than plunged
//! ([`LinkParams::ramp`]). Each is checked against the part ([`Surface`]) before
//! it is used, and left out, for the plain move, where it does not fit.

use std::f64::consts::{FRAC_PI_2, PI};

use camcraft_geom::heightmap::{HeightMap, cell_index};
use camcraft_geom::mesh::distance_to_segment;
use glam::{DVec2, DVec3, dvec3};

use crate::is_positive;
use crate::toolpath::{Move, MoveRole, PLUNGE_RATIO, Toolpath, ToolpathError};

/// How high the tool must be to cross safely.
pub trait Clearance {
    /// Safe tool-tip Z for a traverse from `from` to `to` with a tool of the given
    /// radius.
    ///
    /// The radius is supplied so an implementation can account for the tool's
    /// footprint sweeping past an obstacle, not just its axis.
    fn safe_z(&self, from: DVec2, to: DVec2, tool_radius: f64) -> f64;
}

/// A single horizontal plane, high enough to clear everything.
///
/// Sufficient when the only thing to avoid is the part itself on a flat bed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClearancePlane {
    z: f64,
}

impl ClearancePlane {
    /// A plane at absolute height `z`.
    pub fn new(z: f64) -> Result<Self, LinkError> {
        z.is_finite()
            .then_some(Self { z })
            .ok_or(LinkError::BadParameter {
                name: "clearance",
                value: z,
            })
    }

    /// The plane height.
    pub fn z(&self) -> f64 {
        self.z
    }
}

impl Clearance for ClearancePlane {
    fn safe_z(&self, _from: DVec2, _to: DVec2, _tool_radius: f64) -> f64 {
        self.z
    }
}

/// Clearance taken from a height field, plus a margin.
///
/// This is what [`Clearance`] existed for. A plane must clear the tallest thing on
/// the table everywhere, so every traverse flies at the height of the worst obstacle
/// on the part. A field answers per traverse, so the tool stays low where it can.
///
/// The height is sampled **along** the traverse and across the tool's own width,
/// because the cutter is not a point: a rapid whose axis misses a boss can still put
/// the cutter's shoulder through it.
#[derive(Debug, Clone)]
pub struct ClearanceField {
    field: HeightMap,
    margin: f64,
    fallback: f64,
}

impl ClearanceField {
    /// Clear `field` by `margin`, falling back to `fallback` where the field has no
    /// data.
    ///
    /// The fallback matters: an unsampled column is an unknown height, and flying
    /// low over an unknown is exactly the wrong guess.
    pub fn new(field: HeightMap, margin: f64, fallback: f64) -> Result<Self, LinkError> {
        if !margin.is_finite() || margin < 0.0 {
            return Err(LinkError::BadParameter {
                name: "margin",
                value: margin,
            });
        }
        if !fallback.is_finite() {
            return Err(LinkError::BadParameter {
                name: "fallback",
                value: fallback,
            });
        }
        Ok(Self {
            field,
            margin,
            fallback,
        })
    }
}

impl Clearance for ClearanceField {
    /// The highest material under everything the cutter sweeps: every cell whose
    /// column the tool's disc can reach anywhere along the traverse, not a handful of
    /// probes. A rapid running tangent to a terrace passes material on its diagonal,
    /// and probes on the axis and the four compass points never see it.
    fn safe_z(&self, from: DVec2, to: DVec2, tool_radius: f64) -> f64 {
        // A cell's value stands for its whole square, so the disc reaches it if it
        // reaches its farthest corner.
        let r = if is_positive(tool_radius) {
            tool_radius
        } else {
            0.0
        };
        let reach = r + self.field.pitch() * std::f64::consts::FRAC_1_SQRT_2;
        let under = highest_under(&self.field, from, to, reach);
        let from_field = under.top.map_or(f64::MIN, |t| t + self.margin);
        // Anywhere the field is silent, the fallback governs. Never fly under an
        // unknown.
        if under.unknown || from_field <= f64::MIN {
            from_field.max(self.fallback)
        } else {
            from_field
        }
    }
}

/// What a height field has under a disc swept along a line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Under {
    /// The highest height anywhere under it, if it is over any cell with one.
    pub top: Option<f64>,
    /// Whether any of it is off the field, or over a cell with no height.
    pub unknown: bool,
}

/// The highest cell of `field` whose centre is within `reach` of the line from
/// `from` to `to`.
///
/// Within the tool's radius is the cells the simulator cuts and checks a rapid
/// against; a traverse that must clear every cell it could touch asks for more.
pub fn highest_under(field: &HeightMap, from: DVec2, to: DVec2, reach: f64) -> Under {
    let reach = if reach.is_finite() && reach > 0.0 {
        reach
    } else {
        0.0
    };
    let f = field;
    let pitch = f.pitch();
    let origin = f.origin();

    let lo = from.min(to) - DVec2::splat(reach);
    let hi = from.max(to) + DVec2::splat(reach);
    let first = ((lo - origin) / pitch).floor();
    let last = ((hi - origin) / pitch).ceil();
    let (nx, ny) = (f64::from(f.nx()), f64::from(f.ny()));
    let finite = first.is_finite() && last.is_finite();
    let mut unknown = !finite || first.x < 0.0 || first.y < 0.0 || last.x >= nx || last.y >= ny;

    let mut top: Option<f64> = None;
    if finite {
        let (x0, y0, x1, y1) = (
            cell_index(first.x, f.nx()),
            cell_index(first.y, f.ny()),
            cell_index(last.x, f.nx()),
            cell_index(last.y, f.ny()),
        );
        for iy in y0..=y1 {
            for ix in x0..=x1 {
                let Some(c) = f.position(ix, iy) else {
                    continue;
                };
                if distance_to_segment(c, from, to) > reach {
                    continue;
                }
                match f.get(ix, iy) {
                    Some(h) => top = Some(top.map_or(h, |t: f64| t.max(h))),
                    None => unknown = true,
                }
            }
        }
    }
    Under { top, unknown }
}

/// The material an operation cuts, as it stands while its moves are linked.
pub trait Material {
    /// The top of the material anywhere under the tool's disc swept from `from` to
    /// `to`, or `None` where there is none: the cells the tool covers, as the
    /// simulator counts them, not the material standing beside it.
    fn highest(&self, from: DVec2, to: DVec2, radius: f64) -> Option<f64>;

    /// Cut `moves` into the material. The first is where the tool already is:
    /// the last move of the previous call, or the program's start.
    fn cut(&mut self, moves: &[Move]);
}

/// No material anywhere: every entry is into air.
#[derive(Debug, Clone, Copy, Default)]
pub struct Air;

impl Material for Air {
    fn highest(&self, _from: DVec2, _to: DVec2, _radius: f64) -> Option<f64> {
        None
    }

    fn cut(&mut self, _moves: &[Move]) {}
}

/// Material that stays as a height field says, whatever is cut: for tests, and for
/// deciding against the material before an operation.
#[derive(Debug, Clone)]
pub struct Unchanging(pub HeightMap);

impl Material for Unchanging {
    fn highest(&self, from: DVec2, to: DVec2, radius: f64) -> Option<f64> {
        highest_under(&self.0, from, to, radius).top
    }

    fn cut(&mut self, _moves: &[Move]) {}
}

/// The part, as the tool meets it: where its tip rests without gouging.
pub trait Surface {
    /// The tool's tip on the part at `p`, or `None` where nothing is under it.
    fn at(&self, p: DVec2) -> Option<f64>;

    /// The tool's tip on the part along the line from `from` to `to`, sampled
    /// finely enough to follow the part within the tolerance. Only points with
    /// part under them; ends included where there is.
    fn along(&self, from: DVec2, to: DVec2) -> Vec<DVec3>;
}

/// No part anywhere.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoPart;

impl Surface for NoPart {
    fn at(&self, _p: DVec2) -> Option<f64> {
        None
    }

    fn along(&self, _from: DVec2, _to: DVec2) -> Vec<DVec3> {
        Vec::new()
    }
}

/// When a link is made without lifting clear.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StayDown {
    /// Never: every link lifts clear.
    Never,
    /// A link up to `max` long, riding over the part: the straight line between
    /// the two cuts, lifted wherever the part rises above it. For finishing, which
    /// runs on the part.
    OverPart {
        /// The longest link made this way, in millimetres.
        max: f64,
    },
    /// A link up to `max` long between two cuts at the same height, straight and
    /// level, where the part does not reach up into it. For clearing at a level.
    Level {
        /// The longest link made this way, in millimetres.
        max: f64,
    },
}

/// Ramping into material along the pass about to be cut.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ramp {
    /// How steeply the ramp descends, in radians from level.
    pub angle: f64,
    /// Feed rate along it, in mm/min.
    pub feed: f64,
}

/// How to resolve a link.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinkParams {
    /// Feed rate for coming up out of a cut, in mm/min.
    pub retract_feed: f64,
    /// Feed rate for descending into material, in mm/min.
    pub plunge_feed: f64,
    /// Tool radius, passed to the clearance source.
    pub tool_radius: f64,
    /// How far above the material the tool stops rapiding down and feeds, and up
    /// to which it feeds on the way out, in millimetres.
    pub safe_distance: f64,
    /// When a link stays down.
    pub stay_down: StayDown,
    /// The radius of the vertical arcs a pass is entered and left along, in
    /// millimetres; zero for none.
    pub lead_radius: f64,
    /// Ramping into material, where a pass starts in it; `None` plunges.
    pub ramp: Option<Ramp>,
    /// Deepest a descent at cutting feed may fall steeply, in millimetres: a
    /// stay-down link that would fall further lifts clear instead.
    pub max_cut_step: f64,
    /// Chordal tolerance for the arcs, in millimetres.
    pub tolerance: f64,
    /// The height the tool goes up to after the last cut, if any.
    pub end_z: Option<f64>,
}

impl LinkParams {
    /// Lift clear for every link and plunge straight in: nothing but the feeds and
    /// the tool, the rest at its plainest.
    pub fn plain(retract_feed: f64, plunge_feed: f64, tool_radius: f64) -> Self {
        Self {
            retract_feed,
            plunge_feed,
            tool_radius,
            safe_distance: 2.0,
            stay_down: StayDown::Never,
            lead_radius: 0.0,
            ramp: None,
            max_cut_step: 0.5,
            tolerance: 0.01,
            end_z: None,
        }
    }
}

/// Linking could not proceed.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum LinkError {
    /// A parameter was outside its usable range.
    #[error("{name} must be finite and greater than zero, got {value}")]
    BadParameter {
        /// Which parameter.
        name: &'static str,
        /// What it was.
        value: f64,
    },
    /// The clearance source returned a height that is not usable.
    #[error("clearance height is not finite at {at:?}")]
    BadClearance {
        /// Where it was asked about.
        at: DVec2,
    },
    /// A move could not be added.
    #[error(transparent)]
    Toolpath(#[from] ToolpathError),
}

/// What linking did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LinkReport {
    /// How many links were resolved.
    pub links_resolved: u64,
    /// How many of them stayed down.
    pub stayed_down: u64,
    /// How many passes were ramped into.
    pub ramps: u64,
    /// How many plunges start at or below the surface they descend into.
    ///
    /// Always zero for a plane clearance; meaningful once clearance varies.
    pub suspicious_plunges: u64,
}

/// Resolve every [`MoveRole::Link`], cutting what is linked into `material` as it
/// goes.
///
/// Cutting moves pass through byte for byte: linking decides how to travel between
/// cuts and must never alter the cuts themselves.
pub fn link<C: Clearance, M: Material, S: Surface>(
    path: &Toolpath,
    clearance: &C,
    material: &mut M,
    surface: &S,
    params: &LinkParams,
) -> Result<(Toolpath, LinkReport), LinkError> {
    check("retract_feed", params.retract_feed)?;
    check("plunge_feed", params.plunge_feed)?;
    check("tool_radius", params.tool_radius)?;
    check("tolerance", params.tolerance)?;
    check("max_cut_step", params.max_cut_step)?;
    if !params.safe_distance.is_finite() || params.safe_distance < 0.0 {
        return Err(LinkError::BadParameter {
            name: "safe_distance",
            value: params.safe_distance,
        });
    }
    if !params.lead_radius.is_finite() || params.lead_radius < 0.0 {
        return Err(LinkError::BadParameter {
            name: "lead_radius",
            value: params.lead_radius,
        });
    }
    if let Some(r) = params.ramp {
        check("ramp feed", r.feed)?;
        if !(r.angle > 0.0 && r.angle < FRAC_PI_2) {
            return Err(LinkError::BadParameter {
                name: "ramp angle",
                value: r.angle,
            });
        }
    }

    let mut l = Linker {
        clearance,
        material,
        surface,
        p: params,
        out: Toolpath::new(),
        report: LinkReport::default(),
        cut: 0,
    };
    let moves = path.moves();
    let mut at: Option<DVec3> = None;
    let mut i = 0;
    while let Some(m) = moves.get(i) {
        if m.role != MoveRole::Link {
            l.out.push(*m)?;
            at = Some(m.to);
            i += 1;
            continue;
        }
        // The pass this link leads into: every move up to the next link.
        let end = moves
            .get(i + 1..)
            .and_then(|rest| rest.iter().position(|n| n.role == MoveRole::Link))
            .map_or(moves.len(), |k| i + 1 + k);
        let pass = moves.get(i + 1..end).unwrap_or(&[]);
        l.settle();
        l.link(at, m.to, pass)?;
        for n in pass {
            l.out.push(*n)?;
        }
        at = pass.last().map_or(Some(m.to), |n| Some(n.to));
        i = end;
    }
    if let (Some(from), Some(z)) = (at, params.end_z) {
        l.settle();
        let feed = last_feed(l.out.moves()).unwrap_or(params.retract_feed);
        let top = l.leave(from, feed)?;
        let over = l
            .clearance
            .safe_z(top.truncate(), top.truncate(), params.tool_radius);
        let up = z.max(over);
        if up > top.z {
            l.out.rapid_to(dvec3(top.x, top.y, up))?;
        }
    }
    l.settle();
    Ok((l.out, l.report))
}

/// The feed of the last feeding move, the pass just cut.
fn last_feed(moves: &[Move]) -> Option<f64> {
    moves.iter().rev().find_map(|m| m.feed)
}

/// Where a path heads: the unit direction from `from` to `to`, if they differ.
fn heading(from: DVec3, to: DVec3) -> Option<DVec3> {
    let d = to - from;
    (d.length() > 1e-9).then(|| d.normalize())
}

/// One linking in progress.
struct Linker<'a, C, M, S> {
    clearance: &'a C,
    material: &'a mut M,
    surface: &'a S,
    p: &'a LinkParams,
    out: Toolpath,
    report: LinkReport,
    /// How many of `out`'s moves are cut into the material.
    cut: usize,
}

impl<C: Clearance, M: Material, S: Surface> Linker<'_, C, M, S> {
    /// Cut everything emitted since last time into the material, from the move
    /// before it, so the pieces sweep exactly what the whole would.
    fn settle(&mut self) {
        let moves = self.out.moves();
        if self.cut < moves.len() {
            let from = self.cut.saturating_sub(1);
            if let Some(slice) = moves.get(from..) {
                self.material.cut(slice);
            }
            self.cut = moves.len();
        }
    }

    /// The top of the material under the tool at `p`.
    fn top_at(&self, p: DVec2) -> Option<f64> {
        self.material.highest(p, p, self.p.tool_radius)
    }

    /// The safe distance above the material under the tool at `p`, or above `z`,
    /// whichever is higher: where rapids stop and feeds start.
    fn feed_height(&self, p: DVec2, z: f64) -> f64 {
        self.top_at(p).map_or(z, |t| t.max(z)) + self.p.safe_distance
    }

    /// Resolve the link from `at` (where the last pass ended, if any) to `to`,
    /// the start of `pass`.
    fn link(&mut self, at: Option<DVec3>, to: DVec3, pass: &[Move]) -> Result<(), LinkError> {
        let feed = pass
            .first()
            .and_then(|m| m.feed)
            .unwrap_or(self.p.plunge_feed);
        if let Some(from) = at
            && self.stay_down(from, to, feed)?
        {
            self.report.stayed_down += 1;
            self.report.links_resolved += 1;
            return Ok(());
        }

        // Where the entry starts: a ramp's top, a lead's top, or the pass itself.
        let entry = self.entry(to, pass);
        let start = entry.first().copied().unwrap_or(to);

        let top = match at {
            Some(from) => {
                let out_feed = last_feed(self.out.moves()).unwrap_or(feed);
                Some(self.leave(from, out_feed)?)
            }
            None => None,
        };
        let from_xy = top.map_or(start.truncate(), |t| t.truncate());
        let safe = self
            .clearance
            .safe_z(from_xy, start.truncate(), self.p.tool_radius);
        if !safe.is_finite() {
            return Err(LinkError::BadClearance { at: from_xy });
        }
        // Never across lower than the tool came out: it would rapid down on the way.
        let safe = top.map_or(safe, |t| safe.max(t.z));
        if let Some(t) = top
            && safe > t.z
        {
            self.out.rapid_to(dvec3(t.x, t.y, safe))?;
        }
        // Across at the safe height.
        self.out.rapid_to(dvec3(start.x, start.y, safe))?;

        match entry.as_slice() {
            // Plunged: down at rapid to the safe distance above the material, then
            // at the plunge feed.
            [] | [_] => {
                let stop = self.feed_height(to.truncate(), to.z).min(safe);
                if stop <= to.z {
                    self.report.suspicious_plunges += 1;
                } else if stop < safe {
                    self.out.rapid_to(dvec3(to.x, to.y, stop))?;
                }
                self.out.feed_to(to, MoveRole::Plunge, self.p.plunge_feed)?;
            }
            [first, rest @ ..] => {
                let ramp = self
                    .p
                    .ramp
                    .filter(|_| first.z > to.z && rest.len() > 1 && self.ramped(to));
                if let Some(r) = ramp {
                    // A ramp starts the safe distance above the material it cuts.
                    if first.z < safe {
                        self.out.rapid_to(*first)?;
                    }
                    for p in rest {
                        self.out.feed_to(*p, MoveRole::Ramp, r.feed)?;
                    }
                    self.report.ramps += 1;
                } else {
                    // A lead: down to its top at rapid, then the plunge feed, then
                    // around it at the pass's feed.
                    let stop = self.feed_height(first.truncate(), first.z).min(safe);
                    if stop < safe {
                        self.out.rapid_to(dvec3(first.x, first.y, stop))?;
                    }
                    if stop > first.z {
                        self.out
                            .feed_to(*first, MoveRole::Plunge, self.p.plunge_feed)?;
                    }
                    for p in rest {
                        self.out.feed_to(*p, MoveRole::Lead, feed)?;
                    }
                }
            }
        }
        self.report.links_resolved += 1;
        Ok(())
    }

    /// Whether the pass starting at `to` starts in material, so is ramped into.
    fn ramped(&self, to: DVec3) -> bool {
        self.top_at(to.truncate()).is_some_and(|t| t > to.z + 1e-6)
    }

    /// How `pass`, starting at `to`, is entered: the points of a ramp or a lead,
    /// from its top down to `to`; empty for a plain plunge.
    fn entry(&self, to: DVec3, pass: &[Move]) -> Vec<DVec3> {
        let pts: Vec<DVec3> = std::iter::once(to)
            .chain(pass.iter().map(|m| m.to))
            .collect();
        if let Some(r) = self.p.ramp
            && self.ramped(to)
        {
            return self.ramp_down(&pts, r).unwrap_or_default();
        }
        if self.p.lead_radius > 0.0
            && let Some(next) = pass.first()
            && let Some(t) = heading(to, next.to)
            && let Some(arc) = lead_in(to, t, self.p.lead_radius, self.p.tolerance)
            && self.clear_of_part(&arc)
            && arc
                .first()
                .is_some_and(|s| self.top_at(s.truncate()).is_none_or(|m| m <= s.z))
        {
            return arc;
        }
        Vec::new()
    }

    /// A ramp down onto the start of the pass through `pts`, along the pass's own
    /// path: backwards round it if it closes, back and forth along it if not. The
    /// pass stays within what may be cut at its height, and so at every height
    /// above, so the ramp cannot reach the part.
    ///
    /// `None` where the pass is too short to ramp along: under a quarter of the
    /// tool's width round, as Fusion's smallest helix.
    fn ramp_down(&self, pts: &[DVec3], ramp: Ramp) -> Option<Vec<DVec3>> {
        let to = *pts.first()?;
        let flat: Vec<DVec2> = pts.iter().map(|p| p.truncate()).collect();
        let length: f64 = flat
            .iter()
            .zip(flat.iter().skip(1))
            .map(|(a, b)| a.distance(*b))
            .sum();
        if length < PI * 0.5 * self.p.tool_radius {
            return None;
        }
        // Start above the highest material anywhere along the pass, so the ramp
        // never begins inside it.
        let top = flat
            .iter()
            .zip(flat.iter().skip(1))
            .filter_map(|(a, b)| self.material.highest(*a, *b, self.p.tool_radius))
            .fold(to.z, f64::max);
        let high = top + self.p.safe_distance;
        let run = (high - to.z) / ramp.angle.tan();
        let closed = flat.first()?.distance(*flat.last()?) < 1e-6;
        // Walk away from the start along the path, as far as the ramp runs.
        let walk = backwards(&flat, closed, run)?;
        // Then down it, towards the start.
        let mut out = Vec::with_capacity(walk.len());
        let mut left = run;
        let mut prev: Option<DVec2> = None;
        for p in walk.iter().rev() {
            if let Some(q) = prev {
                left -= q.distance(*p);
            }
            out.push(dvec3(p.x, p.y, to.z + left.max(0.0) * ramp.angle.tan()));
            prev = Some(*p);
        }
        if let Some(last) = out.last_mut() {
            *last = to;
        }
        Some(out)
    }

    /// Whether every point of `arc` is at or above the part under it.
    fn clear_of_part(&self, arc: &[DVec3]) -> bool {
        arc.iter().all(|p| {
            self.surface
                .at(p.truncate())
                .is_none_or(|z| z <= p.z + 1e-9)
        })
    }

    /// Leave the cut at `from`: the lead-out, where it fits, then up at the
    /// retract feed until the safe distance above the material. Returns where
    /// the tool is.
    fn leave(&mut self, from: DVec3, feed: f64) -> Result<DVec3, LinkError> {
        let mut at = from;
        if self.p.lead_radius > 0.0 {
            let before = self.out.moves().iter().rev().nth(1).map(|m| m.to);
            if let Some(prev) = before
                && let Some(t) = heading(prev, from)
                && let Some(arc) = lead_out(from, t, self.p.lead_radius, self.p.tolerance)
                && self.clear_of_part(&arc)
            {
                for p in &arc {
                    self.out.feed_to(*p, MoveRole::Lead, feed)?;
                }
                at = arc.last().copied().unwrap_or(from);
            }
        }
        let up = self.feed_height(at.truncate(), at.z);
        if up > at.z {
            at = dvec3(at.x, at.y, up);
            self.out
                .feed_to(at, MoveRole::Retract, self.p.retract_feed)?;
        }
        Ok(at)
    }

    /// Make the link from `from` to `to` without lifting clear, if it may be.
    fn stay_down(&mut self, from: DVec3, to: DVec3, feed: f64) -> Result<bool, LinkError> {
        let (a, b) = (from.truncate(), to.truncate());
        let span = a.distance(b);
        let way: Vec<DVec3> = match self.p.stay_down {
            StayDown::Never => return Ok(false),
            StayDown::OverPart { max } => {
                if span > max {
                    return Ok(false);
                }
                // The straight line between the cuts, lifted onto the part wherever
                // the part is above it.
                let mut lifted: Vec<(f64, DVec3)> = self
                    .surface
                    .along(a, b)
                    .into_iter()
                    .filter_map(|p| {
                        let t = if span > 1e-9 {
                            (p.truncate() - a).dot(b - a) / (span * span)
                        } else {
                            return None;
                        };
                        (t > 1e-9 && t < 1.0 - 1e-9).then(|| {
                            let chord = from.z + (to.z - from.z) * t;
                            (t, dvec3(p.x, p.y, p.z.max(chord)))
                        })
                    })
                    .collect();
                lifted.sort_by(|x, y| x.0.total_cmp(&y.0));
                let mut way: Vec<DVec3> = lifted.into_iter().map(|(_, p)| p).collect();
                way.push(to);
                way
            }
            StayDown::Level { max } => {
                if span > max || (from.z - to.z).abs() > 1e-6 {
                    return Ok(false);
                }
                let reached = self.surface.along(a, b).iter().any(|p| p.z > to.z + 1e-6);
                if reached {
                    return Ok(false);
                }
                vec![to]
            }
        };
        if falls_steeply(from, &way, self.p.max_cut_step) {
            return Ok(false);
        }
        for p in way {
            self.out.feed_to(p, MoveRole::Transition, feed)?;
        }
        Ok(true)
    }
}

/// Whether a way from `from` through `way` falls further than `max` in one
/// continuous descent steeper than [`PLUNGE_RATIO`]: the plunge at cutting feed
/// the gate refuses.
fn falls_steeply(from: DVec3, way: &[DVec3], max: f64) -> bool {
    let mut at = from;
    let mut run = 0.0;
    for &p in way {
        let drop = at.z - p.z;
        let flat = (p.truncate() - at.truncate()).length();
        if drop > 0.0 && drop > flat * PLUNGE_RATIO {
            run += drop;
            if run > max {
                return true;
            }
        } else {
            run = 0.0;
        }
        at = p;
    }
    false
}

/// Walk `run` millimetres from the start of `path` away from it: backwards round
/// it if `closed`, otherwise out along it and back, as often as it takes. The
/// points passed, from the start (included) to where the walk ends.
fn backwards(path: &[DVec2], closed: bool, run: f64) -> Option<Vec<DVec2>> {
    let start = *path.first()?;
    // The order the walk visits the path's points, one lap or one there and back.
    let lap: Vec<DVec2> = if closed {
        path.iter().rev().skip(1).copied().collect()
    } else {
        path.iter()
            .skip(1)
            .copied()
            .chain(path.iter().rev().skip(1).copied())
            .collect()
    };
    if lap.is_empty() {
        return None;
    }
    let mut out = vec![start];
    let mut left = run;
    let mut at = start;
    // Bounded: each lap is at least the ramp's minimum length long.
    for p in lap.iter().cycle().take(lap.len().saturating_mul(10_000)) {
        let d = at.distance(*p);
        if d >= left {
            if d > 1e-12 {
                out.push(at + (*p - at) * (left / d));
            }
            return Some(out);
        }
        left -= d;
        out.push(*p);
        at = *p;
    }
    None
}

/// Chords of an arc about `centre` of `radius` in the vertical plane along `u`,
/// from angle `a0` to `a1` (radians, measured from `u` towards up), within
/// `tolerance` of it. Excludes the start.
fn arc(centre: DVec3, u: DVec3, radius: f64, a0: f64, a1: f64, tolerance: f64) -> Vec<DVec3> {
    let sweep = a1 - a0;
    let step = 2.0 * (1.0 - (tolerance / radius).min(1.0)).acos();
    let n = if step > 1e-9 {
        (sweep.abs() / step).ceil().max(4.0)
    } else {
        4.0
    };
    let n = camcraft_geom::steps::bounded_count(n);
    (1..=n)
        .map(|k| {
            let a = a0 + sweep * f64::from(k) / f64::from(n);
            centre + radius * (a.cos() * u + a.sin() * DVec3::Z)
        })
        .collect()
}

/// The vertical lead into a pass starting at `to` along `t`, as Fusion's: an arc
/// of `radius` in the pass's vertical plane, from straight down to the pass's own
/// slope. Its points from its top (first) to `to` (last).
fn lead_in(to: DVec3, t: DVec3, radius: f64, tolerance: f64) -> Option<Vec<DVec3>> {
    let flat = t.truncate();
    if flat.length() < 1e-9 {
        return None;
    }
    let u = flat.normalize().extend(0.0);
    let theta = t.z.atan2(flat.length());
    let normal = -theta.sin() * u + theta.cos() * DVec3::Z;
    let centre = to + radius * normal;
    let top = centre - radius * u;
    let mut pts = vec![top];
    pts.extend(arc(centre, u, radius, PI, 1.5 * PI + theta, tolerance));
    if let Some(last) = pts.last_mut() {
        *last = to;
    }
    Some(pts)
}

/// The vertical lead out of a pass ending at `from` along `t`: the lead in,
/// mirrored. Its points after `from`, to its top.
fn lead_out(from: DVec3, t: DVec3, radius: f64, tolerance: f64) -> Option<Vec<DVec3>> {
    let flat = t.truncate();
    if flat.length() < 1e-9 {
        return None;
    }
    let u = flat.normalize().extend(0.0);
    let theta = t.z.atan2(flat.length());
    let normal = -theta.sin() * u + theta.cos() * DVec3::Z;
    let centre = from + radius * normal;
    Some(arc(
        centre,
        u,
        radius,
        1.5 * PI + theta,
        2.0 * PI,
        tolerance,
    ))
}

fn check(name: &'static str, value: f64) -> Result<(), LinkError> {
    is_positive(value)
        .then_some(())
        .ok_or(LinkError::BadParameter { name, value })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use glam::dvec2;

    fn params() -> LinkParams {
        LinkParams::plain(800.0, 300.0, 3.0)
    }

    /// Link over no material and no part.
    fn plain<C: Clearance>(
        path: &Toolpath,
        clearance: &C,
        params: &LinkParams,
    ) -> Result<(Toolpath, LinkReport), LinkError> {
        link(path, clearance, &mut Air, &NoPart, params)
    }

    /// Two short passes with a link in front of each, as a strategy would emit.
    fn strategy_output() -> Toolpath {
        let mut t = Toolpath::new();
        t.feed_to(dvec3(0.0, 0.0, -2.0), MoveRole::Link, 1000.0)
            .unwrap();
        t.feed_to(dvec3(5.0, 0.0, -2.0), MoveRole::Cut, 1000.0)
            .unwrap();
        t.feed_to(dvec3(5.0, 1.0, -3.0), MoveRole::Link, 1000.0)
            .unwrap();
        t.feed_to(dvec3(0.0, 1.0, -3.0), MoveRole::Cut, 1000.0)
            .unwrap();
        t
    }

    fn roles(path: &Toolpath) -> Vec<MoveRole> {
        path.moves().iter().map(|m| m.role).collect()
    }

    #[test]
    fn rejects_unusable_parameters() {
        let p = ClearancePlane::new(10.0).unwrap();
        for bad in [
            LinkParams {
                retract_feed: 0.0,
                ..params()
            },
            LinkParams {
                plunge_feed: f64::NAN,
                ..params()
            },
            LinkParams {
                tool_radius: -1.0,
                ..params()
            },
            LinkParams {
                safe_distance: -1.0,
                ..params()
            },
            LinkParams {
                lead_radius: f64::NAN,
                ..params()
            },
            LinkParams {
                ramp: Some(Ramp {
                    angle: 0.0,
                    feed: 100.0,
                }),
                ..params()
            },
        ] {
            assert!(plain(&strategy_output(), &p, &bad).is_err());
        }
        assert!(ClearancePlane::new(f64::INFINITY).is_err());
    }

    /// The whole point: a linked path is machinable, and can prove it.
    #[test]
    fn resolves_every_link() {
        let input = strategy_output();
        assert!(input.has_unplanned_links());

        let plane = ClearancePlane::new(10.0).unwrap();
        let (out, report) = plain(&input, &plane, &params()).unwrap();

        assert!(!out.has_unplanned_links(), "output must be machinable");
        assert_eq!(report.links_resolved, 2);
    }

    /// Linking decides how to travel between cuts. It must not touch the cuts.
    #[test]
    fn cutting_moves_pass_through_unchanged() {
        let input = strategy_output();
        let plane = ClearancePlane::new(10.0).unwrap();
        let (out, _) = plain(&input, &plane, &params()).unwrap();

        let cuts = |t: &Toolpath| -> Vec<Move> {
            t.moves()
                .iter()
                .filter(|m| m.role == MoveRole::Cut)
                .copied()
                .collect()
        };
        assert_eq!(cuts(&input), cuts(&out), "cuts were altered by linking");
    }

    /// Every traverse happens at the safe height, not somewhere convenient.
    #[test]
    fn traverses_happen_at_clearance() {
        let plane = ClearancePlane::new(12.5).unwrap();
        let (out, _) = plain(&strategy_output(), &plane, &params()).unwrap();

        let mut at = DVec3::ZERO;
        let mut across = 0;
        for (i, m) in out.moves().iter().enumerate() {
            let moved = (m.to.truncate() - at.truncate()).length() > 1e-9;
            if i > 0 && m.role == MoveRole::Rapid && moved {
                assert_eq!(
                    m.to.z, 12.5,
                    "rapid across at {:?} is not at clearance",
                    m.to
                );
                assert_eq!(at.z, 12.5, "rapid across from {at:?} is not at clearance");
                across += 1;
            }
            at = m.to;
        }
        assert_eq!(across, 1);
    }

    /// Retracts go straight up and plunges straight down. A diagonal exit drags the
    /// cutter sideways through the wall it just cut.
    #[test]
    fn retracts_and_plunges_are_vertical() {
        let plane = ClearancePlane::new(10.0).unwrap();
        let (out, _) = plain(&strategy_output(), &plane, &params()).unwrap();

        let mut at = DVec3::ZERO;
        let (mut retracts, mut plunges) = (0, 0);
        for (i, m) in out.moves().iter().enumerate() {
            if i > 0 {
                match m.role {
                    MoveRole::Retract => {
                        assert_eq!(m.to.truncate(), at.truncate(), "retract moved sideways");
                        assert!(m.to.z > at.z, "retract must go up");
                        retracts += 1;
                    }
                    MoveRole::Plunge => {
                        assert_eq!(m.to.truncate(), at.truncate(), "plunge moved sideways");
                        assert!(m.to.z < at.z, "plunge must go down");
                        plunges += 1;
                    }
                    _ => {}
                }
            }
            at = m.to;
        }
        assert_eq!(plunges, 2);
        assert_eq!(retracts, 1, "only the second link follows a cut");
    }

    /// Nothing has been cut yet at the first link, so there is nothing to retract
    /// from. Emitting one would be a move from an unknown position.
    #[test]
    fn the_first_link_does_not_retract() {
        let plane = ClearancePlane::new(10.0).unwrap();
        let (out, _) = plain(&strategy_output(), &plane, &params()).unwrap();
        let first = out.moves().first().unwrap();
        assert_eq!(
            first.role,
            MoveRole::Rapid,
            "a program must open by moving to clearance, not by retracting from nowhere"
        );
    }

    /// Out of a cut at the retract feed, into one at the plunge feed.
    ///
    /// Counted, not just filtered: an earlier version of this test only iterated
    /// over retract moves and asserted their feed, so replacing the retract with a
    /// rapid left zero of them and the test passed vacuously.
    #[test]
    fn retracts_and_plunges_use_their_feeds() {
        let plane = ClearancePlane::new(10.0).unwrap();
        let (out, _) = plain(&strategy_output(), &plane, &params()).unwrap();
        let feeds = |role| -> Vec<Option<f64>> {
            out.moves()
                .iter()
                .filter(|m| m.role == role)
                .map(|m| m.feed)
                .collect()
        };
        assert_eq!(feeds(MoveRole::Retract), [Some(800.0)]);
        assert_eq!(feeds(MoveRole::Plunge), [Some(300.0), Some(300.0)]);
    }

    /// Flat at 0 over the whole field: the material every entry here goes into.
    fn level_material() -> Unchanging {
        Unchanging(HeightMap::filled(dvec2(-20.0, -20.0), 0.5, 81, 81, 0.0).unwrap())
    }

    /// Fusion's safe distance: the tool rapids down until it is that far above the
    /// material, and only feeds from there; and feeds out of a cut only until it is
    /// that far clear, then rapids. Not metres of air at the plunge feed.
    #[test]
    fn feeds_only_within_the_safe_distance_of_the_material() {
        let plane = ClearancePlane::new(30.0).unwrap();
        let (out, _) = link(
            &strategy_output(),
            &plane,
            &mut level_material(),
            &NoPart,
            &params(),
        )
        .unwrap();
        let moves = out.moves();
        for (i, m) in moves.iter().enumerate() {
            let before = if i > 0 { moves[i - 1].to } else { DVec3::ZERO };
            match m.role {
                MoveRole::Plunge => assert_eq!(before.z, 2.0, "plunge starts at {before:?}"),
                MoveRole::Retract => assert_eq!(m.to.z, 2.0, "retract ends at {:?}", m.to),
                _ => {}
            }
        }
        assert_eq!(
            roles(&out),
            [
                MoveRole::Rapid,
                MoveRole::Rapid,
                MoveRole::Plunge,
                MoveRole::Cut,
                MoveRole::Retract,
                MoveRole::Rapid,
                MoveRole::Rapid,
                MoveRole::Rapid,
                MoveRole::Plunge,
                MoveRole::Cut,
            ]
        );
    }

    /// A flat part at `z`.
    struct FlatPart(f64);

    impl Surface for FlatPart {
        fn at(&self, _p: DVec2) -> Option<f64> {
            Some(self.0)
        }

        fn along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
            vec![from.extend(self.0), to.extend(self.0)]
        }
    }

    /// A part flat at 0 with a gentle ridge 1 mm high across x = 1 to 4, at its
    /// highest at x = 2.5.
    struct Ridge;

    impl Surface for Ridge {
        fn at(&self, p: DVec2) -> Option<f64> {
            let u = (p.x - 2.5) / 1.5;
            Some((1.0 - u * u).max(0.0))
        }

        fn along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
            (0..=100)
                .map(|k| {
                    let p = from.lerp(to, f64::from(k) / 100.0);
                    p.extend(self.at(p).unwrap_or(0.0))
                })
                .collect()
        }
    }

    /// Two passes on the part at 0, the second `gap` along from the first's end.
    fn zigzag(gap: f64) -> Toolpath {
        let mut t = Toolpath::new();
        t.feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Link, 1000.0)
            .unwrap();
        t.feed_to(dvec3(5.0, 0.0, 0.0), MoveRole::Cut, 1000.0)
            .unwrap();
        t.feed_to(dvec3(5.0, gap, 0.0), MoveRole::Link, 1000.0)
            .unwrap();
        t.feed_to(dvec3(0.0, gap, 0.0), MoveRole::Cut, 1000.0)
            .unwrap();
        t
    }

    fn staying(max: f64) -> LinkParams {
        LinkParams {
            stay_down: StayDown::OverPart { max },
            ..params()
        }
    }

    /// Zigzag passes a stepover apart are joined without lifting, at the cutting
    /// feed, as Fusion's are; further apart than the stay-down distance, they lift.
    #[test]
    fn near_passes_are_joined_without_lifting() {
        let plane = ClearancePlane::new(10.0).unwrap();
        let (out, report) = link(
            &zigzag(1.0),
            &plane,
            &mut Air,
            &FlatPart(0.0),
            &staying(50.0),
        )
        .unwrap();
        assert_eq!(report.stayed_down, 1);
        assert_eq!(
            roles(&out),
            [
                MoveRole::Rapid,
                MoveRole::Rapid,
                MoveRole::Plunge,
                MoveRole::Cut,
                MoveRole::Transition,
                MoveRole::Cut,
            ]
        );
        let transition = out.moves()[4];
        assert_eq!(transition.feed, Some(1000.0));
        assert_eq!(transition.to, dvec3(5.0, 1.0, 0.0));

        let (far, report) = link(
            &zigzag(60.0),
            &plane,
            &mut Air,
            &FlatPart(0.0),
            &staying(50.0),
        )
        .unwrap();
        assert_eq!(report.stayed_down, 0);
        assert!(!roles(&far).contains(&MoveRole::Transition));
    }

    /// A link that stays down rides over the part where it rises above the line
    /// between the cuts, and never reaches into it.
    #[test]
    fn staying_down_rides_over_the_part() {
        let mut t = Toolpath::new();
        t.feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Link, 1000.0)
            .unwrap();
        t.feed_to(dvec3(0.0, 1.0, 0.0), MoveRole::Cut, 1000.0)
            .unwrap();
        t.feed_to(dvec3(5.0, 1.0, 0.0), MoveRole::Link, 1000.0)
            .unwrap();
        t.feed_to(dvec3(5.0, 2.0, 0.0), MoveRole::Cut, 1000.0)
            .unwrap();
        let plane = ClearancePlane::new(10.0).unwrap();
        let (out, report) = link(&t, &plane, &mut Air, &Ridge, &staying(50.0)).unwrap();
        assert_eq!(report.stayed_down, 1);
        let over: Vec<DVec3> = out
            .moves()
            .iter()
            .filter(|m| m.role == MoveRole::Transition)
            .map(|m| m.to)
            .collect();
        assert!(over.iter().any(|p| p.z == 1.0), "it rises over the ridge");
        for p in &over {
            assert!(
                p.z >= Ridge.at(p.truncate()).unwrap(),
                "{p:?} is in the part"
            );
        }
    }

    /// Descending further than `max_cut_step` in one steep fall at the cutting feed
    /// is the plunge the gate refuses, so such a link lifts clear instead.
    #[test]
    fn a_link_down_a_wall_lifts_clear() {
        let mut t = Toolpath::new();
        t.feed_to(dvec3(0.0, 0.0, 5.0), MoveRole::Link, 1000.0)
            .unwrap();
        t.feed_to(dvec3(1.0, 0.0, 5.0), MoveRole::Cut, 1000.0)
            .unwrap();
        t.feed_to(dvec3(1.1, 0.0, 0.0), MoveRole::Link, 1000.0)
            .unwrap();
        t.feed_to(dvec3(3.0, 0.0, 0.0), MoveRole::Cut, 1000.0)
            .unwrap();
        let plane = ClearancePlane::new(10.0).unwrap();
        let (out, report) = link(&t, &plane, &mut Air, &FlatPart(-10.0), &staying(50.0)).unwrap();
        assert_eq!(report.stayed_down, 0);
        assert!(!roles(&out).contains(&MoveRole::Transition));
    }

    /// Clearing at a level stays down only level, and only where the part does not
    /// reach up into the way.
    #[test]
    fn a_level_link_stays_down_only_clear_of_the_part() {
        let plane = ClearancePlane::new(10.0).unwrap();
        let level = LinkParams {
            stay_down: StayDown::Level { max: 50.0 },
            ..params()
        };
        let (_, clear) = link(&zigzag(1.0), &plane, &mut Air, &FlatPart(-1.0), &level).unwrap();
        assert_eq!(clear.stayed_down, 1);
        let (_, reached) = link(&zigzag(1.0), &plane, &mut Air, &FlatPart(1.0), &level).unwrap();
        assert_eq!(reached.stayed_down, 0);
        let mut deeper = Toolpath::new();
        deeper
            .feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Link, 1000.0)
            .unwrap();
        deeper
            .feed_to(dvec3(5.0, 0.0, 0.0), MoveRole::Cut, 1000.0)
            .unwrap();
        deeper
            .feed_to(dvec3(5.0, 1.0, -1.0), MoveRole::Link, 1000.0)
            .unwrap();
        deeper
            .feed_to(dvec3(0.0, 1.0, -1.0), MoveRole::Cut, 1000.0)
            .unwrap();
        let (_, stepped) = link(&deeper, &plane, &mut Air, &FlatPart(-5.0), &level).unwrap();
        assert_eq!(stepped.stayed_down, 0, "a level move cannot change level");
    }

    /// Fusion's vertical lead: an arc of the lead radius from straight down to the
    /// pass, in the pass's vertical plane, ending on it.
    #[test]
    fn a_lead_in_arcs_down_onto_the_pass() {
        let r = 0.6;
        let arc = lead_in(dvec3(1.0, 2.0, 3.0), DVec3::X, r, 0.001).unwrap();
        let first = arc[0];
        assert!(
            (first - dvec3(1.0 - r, 2.0, 3.0 + r)).length() < 1e-12,
            "{first:?}"
        );
        assert_eq!(*arc.last().unwrap(), dvec3(1.0, 2.0, 3.0));
        let centre = dvec3(1.0, 2.0, 3.0 + r);
        for p in &arc {
            assert!(
                ((*p - centre).length() - r).abs() < 1e-9,
                "{p:?} is off the arc"
            );
            assert_eq!(p.y, 2.0, "the arc is in the pass's plane");
        }
        // It ends level, as the pass runs.
        let n = arc.len();
        let tangent = arc[n - 1] - arc[n - 2];
        // Within half a chord's turn of level.
        assert!(tangent.z.abs() < 0.1 * tangent.length(), "{tangent:?}");

        let out = lead_out(dvec3(1.0, 2.0, 3.0), DVec3::X, r, 0.001).unwrap();
        assert!((*out.last().unwrap() - dvec3(1.0 + r, 2.0, 3.0 + r)).length() < 1e-12);
    }

    /// Entered along a lead where one fits; straight down where the part rises
    /// behind the pass's start, as Fusion leaves the lead out there.
    #[test]
    fn a_lead_is_used_only_where_it_fits() {
        let plane = ClearancePlane::new(10.0).unwrap();
        let leading = LinkParams {
            lead_radius: 0.5,
            ..params()
        };
        let (out, _) = link(&zigzag(60.0), &plane, &mut Air, &FlatPart(0.0), &leading).unwrap();
        let r = roles(&out);
        assert!(r.contains(&MoveRole::Lead));
        // In, then out of the first pass and into the second.
        assert_eq!(r.iter().filter(|x| **x == MoveRole::Plunge).count(), 2);

        // A part a millimetre above the start of the pass, behind it: no lead in.
        struct Behind;
        impl Surface for Behind {
            fn at(&self, p: DVec2) -> Option<f64> {
                Some(if p.x < -0.01 { 1.0 } else { 0.0 })
            }
            fn along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
                vec![from.extend(0.0), to.extend(0.0)]
            }
        }
        let mut t = Toolpath::new();
        t.feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Link, 1000.0)
            .unwrap();
        t.feed_to(dvec3(5.0, 0.0, 0.0), MoveRole::Cut, 1000.0)
            .unwrap();
        let (out, _) = link(&t, &plane, &mut Air, &Behind, &leading).unwrap();
        assert_eq!(
            roles(&out)[..3],
            [MoveRole::Rapid, MoveRole::Rapid, MoveRole::Plunge]
        );
        assert_eq!(
            out.moves()[2].to,
            dvec3(0.0, 0.0, 0.0),
            "plunged to the pass"
        );
    }

    /// A ring at -2 in material up to 0.
    fn ring() -> Toolpath {
        let mut t = Toolpath::new();
        t.feed_to(dvec3(0.0, 0.0, -2.0), MoveRole::Link, 1000.0)
            .unwrap();
        for p in [(10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)] {
            t.feed_to(dvec3(p.0, p.1, -2.0), MoveRole::Cut, 1000.0)
                .unwrap();
        }
        t
    }

    /// Into material along the pass itself, at the ramp angle and feed, from the
    /// safe distance above the material: no plunge, and never off the pass's path.
    #[test]
    fn a_pass_in_material_is_ramped_into_along_itself() {
        let plane = ClearancePlane::new(10.0).unwrap();
        let angle = 2f64.to_radians();
        let ramping = LinkParams {
            ramp: Some(Ramp { angle, feed: 250.0 }),
            ..params()
        };
        let (out, report) =
            link(&ring(), &plane, &mut level_material(), &NoPart, &ramping).unwrap();
        assert_eq!(report.ramps, 1);
        assert!(!roles(&out).contains(&MoveRole::Plunge));
        let ramp: Vec<&Move> = out
            .moves()
            .iter()
            .filter(|m| m.role == MoveRole::Ramp)
            .collect();
        assert!(ramp.iter().all(|m| m.feed == Some(250.0)));
        assert_eq!(ramp.last().unwrap().to, dvec3(0.0, 0.0, -2.0));
        // It starts the safe distance above the material, so runs 4 mm down.
        let start = out
            .moves()
            .iter()
            .take_while(|m| m.role != MoveRole::Ramp)
            .last()
            .unwrap()
            .to;
        assert!((start.z - 2.0).abs() < 1e-9, "{start:?}");
        let mut at = start;
        let mut run = 0.0;
        for m in &ramp {
            let flat = (m.to.truncate() - at.truncate()).length();
            assert!(m.to.z <= at.z + 1e-12, "a ramp only descends");
            run += flat;
            let on_ring = [m.to.x, m.to.y]
                .iter()
                .any(|v| v.abs() < 1e-9 || (v - 10.0).abs() < 1e-9);
            assert!(on_ring, "{:?} is off the ring", m.to);
            at = m.to;
        }
        assert!((run - 4.0 / angle.tan()).abs() < 1e-6, "ran {run}");

        // In air, nothing to ramp into.
        let (out, report) = link(&ring(), &plane, &mut Air, &NoPart, &ramping).unwrap();
        assert_eq!(report.ramps, 0);
        assert!(roles(&out).contains(&MoveRole::Plunge));
    }

    /// The program does not end with the cutter on the part: after the last cut it
    /// goes up to the height asked for.
    #[test]
    fn the_last_cut_is_left_and_the_tool_raised() {
        let plane = ClearancePlane::new(10.0).unwrap();
        let ending = LinkParams {
            end_z: Some(25.0),
            ..params()
        };
        let (out, _) = plain(&strategy_output(), &plane, &ending).unwrap();
        let n = out.len();
        let (up, last) = (out.moves()[n - 2], out.moves()[n - 1]);
        assert_eq!(up.role, MoveRole::Retract);
        assert_eq!(last.role, MoveRole::Rapid);
        assert_eq!(last.to, dvec3(0.0, 1.0, 25.0));
    }

    /// Every move is cut into the material once, in order, each piece starting
    /// from the move before it, so the pieces sweep what the whole would.
    #[test]
    fn the_material_is_cut_as_it_is_linked() {
        #[derive(Default)]
        struct Recording(Vec<Vec<Move>>);
        impl Material for Recording {
            fn highest(&self, _: DVec2, _: DVec2, _: f64) -> Option<f64> {
                None
            }
            fn cut(&mut self, moves: &[Move]) {
                self.0.push(moves.to_vec());
            }
        }
        let plane = ClearancePlane::new(10.0).unwrap();
        let mut rec = Recording::default();
        let ending = LinkParams {
            end_z: Some(20.0),
            ..params()
        };
        let (out, _) = link(&strategy_output(), &plane, &mut rec, &NoPart, &ending).unwrap();
        assert!(rec.0.len() >= 2, "cut as it went, not once at the end");
        let mut seen: Vec<Move> = Vec::new();
        for piece in &rec.0 {
            if let Some(last) = seen.last() {
                assert_eq!(
                    piece.first(),
                    Some(last),
                    "each piece starts where the last ended"
                );
                seen.extend(piece.iter().skip(1));
            } else {
                seen.extend(piece.iter());
            }
        }
        assert_eq!(seen.as_slice(), out.moves());
    }

    /// A clearance below the work cannot produce a safe plunge, and the report has
    /// to say so rather than quietly emitting it. The second entry starts from where
    /// the tool came up out of the first cut, above the work, so only the first is.
    #[test]
    fn a_clearance_below_the_work_is_reported() {
        let plane = ClearancePlane::new(-50.0).unwrap();
        let (_, report) = plain(&strategy_output(), &plane, &params()).unwrap();
        assert_eq!(report.suspicious_plunges, 1);
    }

    /// The clearance source is asked about the traverse, not about a fixed height.
    /// This is what lets a varying field drop in later without touching callers.
    #[test]
    fn clearance_is_queried_per_traverse() {
        struct Recording(std::cell::RefCell<Vec<(DVec2, DVec2, f64)>>);
        impl Clearance for Recording {
            fn safe_z(&self, from: DVec2, to: DVec2, r: f64) -> f64 {
                self.0.borrow_mut().push((from, to, r));
                20.0
            }
        }
        let rec = Recording(std::cell::RefCell::new(Vec::new()));
        let (_, _) = plain(&strategy_output(), &rec, &params()).unwrap();

        let calls = rec.0.borrow();
        assert_eq!(calls.len(), 2, "one query per link");
        let second = calls.get(1).unwrap();
        assert_eq!(
            second.0,
            dvec2(5.0, 0.0),
            "asked from the end of the last cut"
        );
        assert_eq!(second.1, dvec2(5.0, 1.0), "asked to the start of the next");
        assert_eq!(second.2, 3.0, "tool radius is passed through");
    }

    fn stepped_field() -> HeightMap {
        // Flat at 0, with a 20 mm boss in the middle.
        let mut f = HeightMap::filled(dvec2(-10.0, -10.0), 1.0, 21, 21, 0.0).unwrap();
        for iy in 8..13 {
            for ix in 8..13 {
                f.set(ix, iy, 20.0);
            }
        }
        f
    }

    #[test]
    fn rejects_an_unusable_field() {
        let f = stepped_field();
        assert!(ClearanceField::new(f.clone(), -1.0, 0.0).is_err());
        assert!(ClearanceField::new(f, 5.0, f64::NAN).is_err());
    }

    /// The whole point of a field over a plane: stay low where it is safe to.
    #[test]
    fn a_traverse_clear_of_the_boss_flies_low() {
        let c = ClearanceField::new(stepped_field(), 5.0, 0.0).unwrap();
        let away = c.safe_z(dvec2(-9.0, -9.0), dvec2(-7.0, -9.0), 0.5);
        assert!(
            (away - 5.0).abs() < 1e-9,
            "clear of the boss it should be flat + margin, got {away}"
        );
    }

    /// And rise where it is not.
    #[test]
    fn a_traverse_over_the_boss_rises() {
        let c = ClearanceField::new(stepped_field(), 5.0, 0.0).unwrap();
        let over = c.safe_z(dvec2(-9.0, 0.0), dvec2(9.0, 0.0), 0.5);
        assert!(
            (over - 25.0).abs() < 1e-9,
            "crossing the boss it should be 20 + margin, got {over}"
        );
    }

    /// The cutter is not a point. A rapid whose axis misses a boss can still put
    /// the tool's shoulder through it.
    #[test]
    fn the_tool_width_is_accounted_for() {
        let c = ClearanceField::new(stepped_field(), 5.0, 0.0).unwrap();
        // The boss spans cell centres -2..2. A traverse at y = -4 clears it by 2 mm
        // on the axis, but a 3 mm cutter does not.
        let thin = c.safe_z(dvec2(-9.0, -4.0), dvec2(9.0, -4.0), 0.1);
        let wide = c.safe_z(dvec2(-9.0, -4.0), dvec2(9.0, -4.0), 3.0);
        assert!(
            thin < 25.0,
            "a thin tool passes beside the boss, got {thin}"
        );
        assert!(
            (wide - 25.0).abs() < 1e-9,
            "a wide tool must clear it, got {wide}"
        );
    }

    /// An unsampled column is an unknown height. Flying low over an unknown is
    /// exactly the wrong guess.
    #[test]
    fn unknown_ground_falls_back_rather_than_flying_low() {
        let c = ClearanceField::new(stepped_field(), 5.0, 99.0).unwrap();
        let outside = c.safe_z(dvec2(500.0, 500.0), dvec2(520.0, 500.0), 1.0);
        assert_eq!(outside, 99.0, "off the field, the fallback governs");
    }

    /// Fields and planes must be interchangeable at the call site, since that is the
    /// entire reason clearance is a trait.
    #[test]
    fn a_field_substitutes_for_a_plane() {
        let field = ClearanceField::new(stepped_field(), 5.0, 0.0).unwrap();
        let (out, report) = plain(&strategy_output(), &field, &params()).unwrap();
        assert!(!out.has_unplanned_links());
        assert_eq!(report.links_resolved, 2);
    }

    #[test]
    fn an_empty_path_links_to_an_empty_path() {
        let plane = ClearancePlane::new(10.0).unwrap();
        let (out, report) = plain(&Toolpath::new(), &plane, &params()).unwrap();
        assert!(out.is_empty());
        assert_eq!(report, LinkReport::default());
    }
}
