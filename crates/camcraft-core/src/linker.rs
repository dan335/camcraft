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

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use camcraft_geom::arc::Arc;
use camcraft_geom::heightmap::{HeightMap, cell_index};
use camcraft_geom::mesh::distance_to_segment;
use glam::{DVec2, DVec3, dvec3};

use crate::is_positive;
use crate::toolpath::{Move, MoveKind, MoveRole, PLUNGE_RATIO, Toolpath, ToolpathError};

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
    floor: f64,
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
            floor: f64::MIN,
        })
    }

    /// Never lower than `z`: a full retraction, to the retract height, or
    /// higher where the material is.
    pub fn at_least(self, z: f64) -> Self {
        Self { floor: z, ..self }
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
        let z = if under.unknown || from_field <= f64::MIN {
            from_field.max(self.fallback)
        } else {
            from_field
        };
        z.max(self.floor)
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

    /// [`at`](Self::at), for a tool that may graze the part: one running beside
    /// a finished wall, as a 2D cut's ring does, touches it without cutting it,
    /// so contact within the tolerance is not contact. For moves at feed; a rapid
    /// keeps clear by `at`.
    fn grazing_at(&self, p: DVec2) -> Option<f64> {
        self.at(p)
    }

    /// [`along`](Self::along), for a tool that may graze the part.
    fn grazing_along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
        self.along(from, to)
    }
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

/// How a pass that starts in material is entered, where [`LinkParams::ramp`]
/// gives a ramp angle and feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// Down along the pass itself, in the direction it cuts; a ring with leads
    /// is entered at its start, without its lead.
    Ramp,
    /// Down a helix beside where the pass starts, then along the vertical lead,
    /// if there is one; straight down where there is no room for one.
    Helix,
    /// Straight down at the plunge feed onto the vertical lead, or the pass.
    Plunge,
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
    /// How a pass that starts in material is entered, with a ramp.
    pub entry: EntryKind,
    /// The radius of a helical entry, as the tool's centre goes, in millimetres.
    pub helix_radius: f64,
    /// Whether a helix turns counter-clockwise seen from above, which climbs
    /// with a clockwise spindle.
    pub helix_ccw: bool,
    /// The most a helix, or a ramp round a closed pass, descends in one lap, in
    /// millimetres; `None` for no limit but the ramp angle.
    pub ramp_stepdown: Option<f64>,
    /// Whether the vertical lead may go down through material, from a plunge
    /// onto its top, as a 2D profile's does; otherwise it is left out wherever
    /// its top is in material, as a finishing pass's is.
    pub lead_in_material: bool,
    /// Deepest a descent at cutting feed may fall steeply, in millimetres: a
    /// stay-down link that would fall further lifts clear instead.
    pub max_cut_step: f64,
    /// Chordal tolerance for the arcs, in millimetres.
    pub tolerance: f64,
    /// The height the tool goes up to after the last cut, if any.
    pub end_z: Option<f64>,
    /// The height the tool arrives at, above the first pass, before it goes down
    /// to where it starts feeding, if any: the clearance height.
    pub start_z: Option<f64>,
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
            entry: EntryKind::Ramp,
            helix_radius: 0.0,
            helix_ccw: true,
            ramp_stepdown: None,
            lead_in_material: false,
            max_cut_step: 0.5,
            tolerance: 0.01,
            end_z: None,
            start_z: None,
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
    /// How many passes were entered down a helix.
    pub helixes: u64,
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
        let skip = l.link(at, m.to, pass)?;
        for n in pass.get(skip..).unwrap_or(&[]) {
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

/// How a pass is entered, planned before the height it starts from is known.
enum Way {
    /// Straight down onto its start.
    Plunge,
    /// Straight down onto `top`, then along a vertical arc onto its start.
    Lead {
        /// Where the arc starts.
        top: DVec3,
        /// The arc.
        arc: Arc,
    },
    /// Down a helix in the waste onto `end`, then along `lead`, a vertical arc
    /// onto its start, if there is one; otherwise `end` is its start.
    Helix {
        /// Where the helix ends.
        end: DVec3,
        /// The level direction into the waste, square to the ring where it
        /// starts, which the helix is off to.
        aside: DVec2,
        /// Its radius, as the tool's centre goes.
        radius: f64,
        /// The vertical arc after it.
        lead: Option<Arc>,
    },
    /// Down along the pass itself, from `start`, through `moves`, leaving out
    /// the pass's first `skip` moves, which it replaces.
    Ramp {
        /// Where it starts.
        start: DVec3,
        /// Its moves, the last ending where the pass is entered.
        moves: Vec<Move>,
        /// How many of the pass's first moves it replaces.
        skip: usize,
    },
}

/// A helix, as the tool goes down it.
struct Helix {
    /// Where it starts.
    start: DVec3,
    /// Its turns, each at most one, the last a level one at the bottom, ending
    /// where it ends.
    moves: Vec<(DVec3, Arc)>,
}

/// A piece of a path: where it starts, and the move from there.
#[derive(Debug, Clone, Copy)]
struct Seg {
    from: DVec3,
    m: Move,
}

impl Seg {
    /// How far it runs, seen from above.
    fn flat_length(&self) -> f64 {
        match self.m.kind {
            MoveKind::Linear => self.from.truncate().distance(self.m.to.truncate()),
            MoveKind::Arc(a) if a.axis.z.abs() > 1.0 - 1e-9 => a.radius(self.from) * a.sweep.abs(),
            MoveKind::Arc(_) => {
                let mut at = self.from;
                let mut length = 0.0;
                for p in self.m.chords(self.from, 1e-3) {
                    length += at.truncate().distance(p.truncate());
                    at = p;
                }
                length
            }
        }
    }

    /// It run backwards.
    fn reversed(&self) -> Self {
        Seg {
            from: self.m.to,
            m: self.m.reversed(self.from),
        }
    }

    /// Its first fraction `f`.
    fn head(&self, f: f64) -> Self {
        let m = match self.m.kind {
            MoveKind::Linear => Move {
                to: self.from.lerp(self.m.to, f),
                ..self.m
            },
            MoveKind::Arc(a) => {
                let (head, mid, _) = a.split(self.from, self.m.to, f);
                Move {
                    to: mid,
                    kind: MoveKind::Arc(head),
                    ..self.m
                }
            }
        };
        Seg { from: self.from, m }
    }
}

/// The pieces of `moves`, the first starting at `from`.
fn segs(from: DVec3, moves: &[Move]) -> Vec<Seg> {
    let mut at = from;
    moves
        .iter()
        .map(|m| Seg {
            from: std::mem::replace(&mut at, m.to),
            m: *m,
        })
        .collect()
}

/// Walk `run` millimetres, seen from above, along `lap`, pieces in the order
/// walked, round it as often as it takes: the pieces passed, the last cut short
/// where the walk ends. `None` for a lap with no length.
fn walk(lap: &[Seg], run: f64) -> Option<Vec<Seg>> {
    let total: f64 = lap.iter().map(Seg::flat_length).sum();
    if total.is_nan() || total <= 1e-9 {
        return None;
    }
    let mut out = Vec::new();
    let mut left = run;
    // Bounded: each lap is at least the ramp's minimum length long.
    for s in lap.iter().cycle().take(lap.len().saturating_mul(10_000)) {
        let length = s.flat_length();
        if length >= left {
            if left > 1e-12 && length > 1e-12 {
                out.push(s.head(left / length));
            }
            return Some(out);
        }
        left -= length;
        out.push(*s);
    }
    None
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
    ///
    /// The part counts as well as the material: a rapid never comes down, or
    /// goes up, beside a wall it would touch, such as the one a 2D ring runs
    /// along.
    fn feed_height(&self, p: DVec2, z: f64) -> f64 {
        let part = self.surface.at(p).unwrap_or(f64::MIN);
        self.top_at(p).map_or(z, |t| t.max(z)).max(part) + self.p.safe_distance
    }

    /// Resolve the link from `at` (where the last pass ended, if any) to `to`,
    /// the start of `pass`. Returns how many of the pass's first moves the way in
    /// replaced, which are left out.
    fn link(&mut self, at: Option<DVec3>, to: DVec3, pass: &[Move]) -> Result<usize, LinkError> {
        let feed = pass
            .first()
            .and_then(|m| m.feed)
            .unwrap_or(self.p.plunge_feed);
        if let Some(from) = at
            && self.stay_down(from, to, feed)?
        {
            self.report.stayed_down += 1;
            self.report.links_resolved += 1;
            return Ok(0);
        }

        let way = self.entry(to, pass);
        let skip = match &way {
            Way::Ramp { skip, .. } => *skip,
            _ => 0,
        };

        let top = match at {
            Some(from) => {
                let out_feed = last_feed(self.out.moves()).unwrap_or(feed);
                let off = self.lead_off(from, out_feed)?;
                // Down to the next level of the same ring without lifting clear,
                // where the way there is short and clear.
                if self.down_into(off, &way, to, feed)? {
                    self.report.stayed_down += 1;
                    self.report.links_resolved += 1;
                    return Ok(skip);
                }
                Some(self.rise(off)?)
            }
            None => None,
        };
        // Where the way in starts, seen from above.
        let start = match &way {
            Way::Plunge => to,
            Way::Lead { top, .. } => *top,
            Way::Helix {
                end, aside, radius, ..
            } => self
                .helix(*end, *aside, *radius, None)
                .map_or(*end, |h| h.start),
            Way::Ramp { start, .. } => *start,
        };
        let from_xy = top.map_or(start.truncate(), |t| t.truncate());
        let safe = self
            .clearance
            .safe_z(from_xy, start.truncate(), self.p.tool_radius);
        if !safe.is_finite() {
            return Err(LinkError::BadClearance { at: from_xy });
        }
        // Never across lower than the tool came out: it would rapid down on the
        // way. Nor lower than a helix or ramp starts, which it goes down to.
        let safe = top.map_or(safe, |t| safe.max(t.z));
        let safe = match &way {
            Way::Helix { .. } | Way::Ramp { .. } => safe.max(start.z),
            _ => safe,
        };
        if let Some(t) = top
            && safe > t.z
        {
            self.out.rapid_to(dvec3(t.x, t.y, safe))?;
        }
        // Arriving, at the clearance height above the start, then down to the
        // safe height; otherwise across at it.
        if let (None, Some(z)) = (top, self.p.start_z)
            && z > safe
        {
            self.out.rapid_to(dvec3(start.x, start.y, z))?;
        }
        self.out.rapid_to(dvec3(start.x, start.y, safe))?;

        // Down at rapid to where the way in starts feeding.
        match &way {
            Way::Plunge => {
                let stop = self.feed_height(to.truncate(), to.z).min(safe);
                if stop <= to.z {
                    self.report.suspicious_plunges += 1;
                } else if stop < safe {
                    self.out.rapid_to(dvec3(to.x, to.y, stop))?;
                }
            }
            Way::Lead { top, .. } => {
                let stop = self.feed_height(top.truncate(), top.z).min(safe);
                if stop < safe {
                    self.out.rapid_to(dvec3(top.x, top.y, stop))?;
                }
            }
            Way::Helix { .. } | Way::Ramp { .. } => {
                // Down at rapid no nearer the part than the safe distance, then
                // at the plunge feed to where it starts.
                let stop = self
                    .surface
                    .at(start.truncate())
                    .map_or(start.z, |z| start.z.max(z + self.p.safe_distance))
                    .min(safe);
                if stop < safe {
                    self.out.rapid_to(dvec3(start.x, start.y, stop))?;
                }
                if stop > start.z {
                    self.out
                        .feed_to(start, MoveRole::Plunge, self.p.plunge_feed)?;
                }
            }
        }
        self.descend(&way, to, feed)?;
        self.report.links_resolved += 1;
        Ok(skip)
    }

    /// From where the tool is, above where `way` starts, down it onto `to`.
    fn descend(&mut self, way: &Way, to: DVec3, feed: f64) -> Result<(), LinkError> {
        let at = self.out.end().unwrap_or(to);
        match way {
            Way::Plunge => self.vertical(at, to)?,
            Way::Lead { top, arc } => {
                self.vertical(at, *top)?;
                self.out
                    .arc_or_chords_to(to, *arc, MoveRole::Lead, feed, self.p.tolerance)?;
            }
            Way::Helix {
                end,
                aside,
                radius,
                lead,
            } => {
                let ramp_feed = self.p.ramp.map_or(self.p.plunge_feed, |r| r.feed);
                match self.helix(*end, *aside, *radius, Some(at.z)) {
                    Some(h) => {
                        self.vertical(at, h.start)?;
                        for (p, arc) in h.moves {
                            self.out.arc_to(p, arc, MoveRole::Ramp, ramp_feed)?;
                        }
                        self.report.helixes += 1;
                    }
                    None => self.vertical(at, *end)?,
                }
                if let Some(arc) = lead {
                    self.out
                        .arc_or_chords_to(to, *arc, MoveRole::Lead, feed, self.p.tolerance)?;
                }
            }
            Way::Ramp { moves, .. } => {
                for m in moves {
                    self.out.push(*m)?;
                }
                self.report.ramps += 1;
            }
        }
        Ok(())
    }

    /// Straight up or down from `at` to `to`, which is above or below it: down
    /// at the plunge feed, up at the retract feed.
    fn vertical(&mut self, at: DVec3, to: DVec3) -> Result<(), LinkError> {
        if to.z < at.z - 1e-9 {
            self.out.feed_to(to, MoveRole::Plunge, self.p.plunge_feed)?;
        } else if to.z > at.z + 1e-9 {
            self.out
                .feed_to(to, MoveRole::Retract, self.p.retract_feed)?;
        }
        Ok(())
    }

    /// Whether the pass starting at `to` starts in material, so is ramped into.
    fn ramped(&self, to: DVec3) -> bool {
        self.top_at(to.truncate()).is_some_and(|t| t > to.z + 1e-6)
    }

    /// How `pass`, starting at `to`, is entered.
    fn entry(&self, to: DVec3, pass: &[Move]) -> Way {
        // The vertical lead onto the pass, where it may be used: clear of the
        // part, and, unless it may go down through material, not in any.
        let lead = || {
            let t = pass.first().and_then(|m| m.tangent_at(to, 0.0))?;
            let (top, arc) = lead_in(to, t, self.p.lead_radius)?;
            let clear = self.clear_of_part(top, &arc.chords(top, to, self.p.tolerance));
            let dry =
                self.p.lead_in_material || self.top_at(top.truncate()).is_none_or(|m| m <= top.z);
            (clear && dry).then_some((top, arc))
        };
        let lead = if self.p.lead_radius > 0.0 {
            lead()
        } else {
            None
        };
        let plain = || match lead {
            Some((top, arc)) => Way::Lead { top, arc },
            None => Way::Plunge,
        };
        let Some(ramp) = self.p.ramp else {
            return plain();
        };
        match self.p.entry {
            EntryKind::Plunge => plain(),
            EntryKind::Helix => {
                let end = lead.map_or(to, |(top, _)| top);
                // The ring where it starts: its first cut, after any lead. The
                // waste is on the side the helix turns towards, as the cut
                // climbs, so a helix off that side, touching the ring's line
                // where it ends, runs on the way the ring does.
                let mut from = to;
                let mut ring = None;
                for m in pass {
                    if m.role == MoveRole::Cut {
                        ring = m.tangent_at(from, 0.0);
                        break;
                    }
                    from = m.to;
                }
                let Some(along) = ring
                    .or_else(|| pass.first().and_then(|m| m.tangent_at(to, 0.0)))
                    .map(|t| t.truncate())
                    .and_then(DVec2::try_normalize)
                else {
                    return plain();
                };
                let aside = if self.p.helix_ccw {
                    along.perp()
                } else {
                    -along.perp()
                };
                // The largest helix clear of the part, if there is material to
                // go down through at all.
                let fits = [1.0, 0.5, 0.25]
                    .into_iter()
                    .map(|k| self.p.helix_radius * k)
                    .find(|&r| r > 1e-6 && self.helix_clear(end, aside, r));
                match fits {
                    Some(radius) if self.helix(end, aside, radius, None).is_some() => Way::Helix {
                        end,
                        aside,
                        radius,
                        lead: lead.map(|(_, arc)| arc),
                    },
                    _ => plain(),
                }
            }
            EntryKind::Ramp => {
                // A ring with leads is ramped into along the ring itself, from
                // its start, and entered there without its lead.
                let lead_in = pass.iter().take_while(|m| m.role == MoveRole::Lead).count();
                let lead_out = pass
                    .iter()
                    .rev()
                    .take_while(|m| m.role == MoveRole::Lead)
                    .count();
                let (onto, ring, skip) =
                    match pass.get(lead_in..pass.len().saturating_sub(lead_out)) {
                        Some(ring) if lead_in > 0 && !ring.is_empty() => {
                            let onto = pass.get(lead_in - 1).map_or(to, |m| m.to);
                            (onto, segs(onto, ring), lead_in)
                        }
                        _ => (to, segs(to, pass), 0),
                    };
                if !self.ramped(onto) {
                    return plain();
                }
                match self.ramp_down(onto, &ring, ramp) {
                    Some((start, moves)) => Way::Ramp { start, moves, skip },
                    None if skip > 0 => plain(),
                    None => Way::Plunge,
                }
            }
        }
    }

    /// Whether a helix of `radius` off to `aside` of `end` keeps clear of the
    /// part down to `end`, grazing it at most.
    fn helix_clear(&self, end: DVec3, aside: DVec2, radius: f64) -> bool {
        let centre = end.truncate() + aside * radius;
        (0..32).all(|k| {
            let p = centre + DVec2::from_angle(f64::from(k) * PI / 16.0) * radius;
            self.surface.grazing_at(p).is_none_or(|z| z <= end.z + 1e-9)
        })
    }

    /// The helix of `radius` down onto `end`, behind it as the pass sets off
    /// `aside`, from the safe distance above the material it goes down through,
    /// or from `from_z` if that is lower: at the ramp angle, and no steeper than
    /// the ramp's most in one turn. `None` where there is no material above `end`
    /// to go down through.
    fn helix(&self, end: DVec3, aside: DVec2, radius: f64, from_z: Option<f64>) -> Option<Helix> {
        let ramp = self.p.ramp?;
        let centre = end.truncate() + aside * radius;
        let material = self
            .material
            .highest(centre, centre, radius + self.p.tool_radius)?;
        if material <= end.z + 1e-6 {
            return None;
        }
        let top = material + self.p.safe_distance;
        let h0 = from_z.map_or(top, |z| z.min(top));
        if h0 <= end.z + 1e-9 {
            return None;
        }
        let lap = TAU * radius;
        let angle = self
            .p
            .ramp_stepdown
            .map_or(ramp.angle, |d| ramp.angle.min((d / lap).atan()));
        let turn = if self.p.helix_ccw { 1.0 } else { -1.0 };
        let total = (h0 - end.z) / (radius * angle.tan());
        let back = end.truncate() - centre;
        let start = (centre + DVec2::from_angle(-turn * total).rotate(back)).extend(h0);
        let n = (total / TAU - 1e-9).ceil().max(1.0);
        let n = camcraft_geom::steps::bounded_count(n);
        let piece = turn * total / f64::from(n);
        let moves = (1..=n)
            .map(|k| {
                let f = f64::from(k) / f64::from(n);
                let p = if k == n {
                    end
                } else {
                    let xy = centre
                        + DVec2::from_angle(turn * total * f).rotate(start.truncate() - centre);
                    xy.extend(h0 + (end.z - h0) * f)
                };
                let arc = Arc {
                    centre: centre.extend(end.z),
                    axis: DVec3::Z,
                    sweep: piece,
                };
                (p, arc)
            })
            .collect::<Vec<_>>();
        // Then once round level at the bottom, as Fusion's helix ends: a helix
        // leaves its circle cut deeper on one side than the other, by up to a
        // turn's descent, and the next depth's way in would cut that at feed.
        let mut moves = moves;
        moves.push((
            end,
            Arc {
                centre: centre.extend(end.z),
                axis: DVec3::Z,
                sweep: turn * TAU,
            },
        ));
        Some(Helix { start, moves })
    }

    /// A ramp down onto `to` along `pass`, the pieces of a pass from `to`: in the
    /// direction it cuts, round it if it closes, back and forth along it if not.
    /// The pass stays within what may be cut at its height, and so at every
    /// height above, so the ramp cannot reach the part. It is the pass's own
    /// shape, arcs as helices, no steeper round a closed pass than the ramp's
    /// most in one lap. Where it starts, and its moves.
    ///
    /// `None` where the pass is too short to ramp along: under a quarter of the
    /// tool's width round, as Fusion's smallest helix.
    fn ramp_down(&self, to: DVec3, pass: &[Seg], ramp: Ramp) -> Option<(DVec3, Vec<Move>)> {
        let length: f64 = pass.iter().map(Seg::flat_length).sum();
        if length < PI * 0.5 * self.p.tool_radius {
            return None;
        }
        let end = pass.last()?.m.to;
        let closed = end.truncate().distance(to.truncate()) < 1e-6;
        // Start above the highest material anywhere along the pass, so the ramp
        // never begins inside it.
        let mut top = to.z;
        for s in pass {
            let mut a = s.from.truncate();
            for p in s.m.chords(s.from, self.p.tolerance) {
                if let Some(h) = self.material.highest(a, p.truncate(), self.p.tool_radius) {
                    top = top.max(h);
                }
                a = p.truncate();
            }
        }
        let high = top + self.p.safe_distance;
        let angle = match self.p.ramp_stepdown {
            Some(d) if closed => ramp.angle.min((d / length).atan()),
            _ => ramp.angle,
        };
        let run = (high - to.z) / angle.tan();
        // Walk away from the start: back round a closed pass, out along an open
        // one and back.
        let lap: Vec<Seg> = if closed {
            pass.iter().rev().map(Seg::reversed).collect()
        } else {
            pass.iter()
                .copied()
                .chain(pass.iter().rev().map(Seg::reversed))
                .collect()
        };
        let walked = walk(&lap, run)?;
        // Then down it, towards the start: never below the pass itself, so it
        // rides over anything the pass rises over, such as a tab.
        let pieces: Vec<Seg> = walked.iter().rev().map(Seg::reversed).collect();
        let first = pieces.first()?;
        let start = dvec3(first.from.x, first.from.y, high.max(first.from.z));
        let mut left = run;
        let mut at = start;
        let mut moves = Vec::with_capacity(pieces.len());
        let count = pieces.len();
        for (i, s) in pieces.iter().enumerate() {
            left -= s.flat_length();
            let z = (to.z + left.max(0.0) * angle.tan()).max(s.m.to.z);
            let p = if i + 1 == count {
                to
            } else {
                dvec3(s.m.to.x, s.m.to.y, z)
            };
            let straight_down = p.z < at.z && p.truncate().distance(at.truncate()) < 1e-9;
            let (role, feed) = if straight_down {
                (MoveRole::Plunge, self.p.plunge_feed)
            } else {
                (MoveRole::Ramp, ramp.feed)
            };
            match s.m.kind {
                MoveKind::Arc(a) if a.axis.z.abs() > 1.0 - 1e-9 && !straight_down => {
                    moves.push(Move {
                        to: p,
                        kind: MoveKind::Arc(a),
                        role,
                        feed: Some(feed),
                    });
                }
                _ => moves.push(Move {
                    to: p,
                    kind: MoveKind::Linear,
                    role,
                    feed: Some(feed),
                }),
            }
            at = p;
        }
        Some((start, moves))
    }

    /// Whether `from` and every point of `way` are at or above the part under
    /// them.
    fn clear_of_part(&self, from: DVec3, way: &[DVec3]) -> bool {
        std::iter::once(&from).chain(way).all(|p| {
            self.surface
                .grazing_at(p.truncate())
                .is_none_or(|z| z <= p.z + 1e-9)
        })
    }

    /// Off the cut at `from` along the vertical lead-out, where it fits. Returns
    /// where the tool is.
    fn lead_off(&mut self, from: DVec3, feed: f64) -> Result<DVec3, LinkError> {
        if self.p.lead_radius > 0.0 {
            let moves = self.out.moves();
            let last = moves.len().checked_sub(2).and_then(|k| moves.get(k..));
            if let Some([before, m]) = last
                && let Some(t) = m.tangent_at(before.to, 1.0)
                && let Some((end, arc)) = lead_out(from, t, self.p.lead_radius)
                && self.clear_of_part(from, &arc.chords(from, end, self.p.tolerance))
            {
                self.out
                    .arc_or_chords_to(end, arc, MoveRole::Lead, feed, self.p.tolerance)?;
                self.settle();
                return Ok(end);
            }
        }
        Ok(from)
    }

    /// Up from `at` at the retract feed until the safe distance above the
    /// material. Returns where the tool is.
    fn rise(&mut self, at: DVec3) -> Result<DVec3, LinkError> {
        let up = self.feed_height(at.truncate(), at.z);
        if up > at.z {
            let top = dvec3(at.x, at.y, up);
            self.out
                .feed_to(top, MoveRole::Retract, self.p.retract_feed)?;
            return Ok(top);
        }
        Ok(at)
    }

    /// Leave the cut at `from`: the lead-out, where it fits, then up at the
    /// retract feed until the safe distance above the material. Returns where
    /// the tool is.
    fn leave(&mut self, from: DVec3, feed: f64) -> Result<DVec3, LinkError> {
        let off = self.lead_off(from, feed)?;
        self.rise(off)
    }

    /// From `off`, where the tool came off the last cut, across without lifting
    /// and down `way` onto `to` at a lower level, if the link is short and its
    /// way clear: as Fusion goes down to a ring's next depth. Across level at
    /// the height it came off, clear of the part and cutting little more than a
    /// descent at cutting feed may; then down.
    fn down_into(
        &mut self,
        off: DVec3,
        way: &Way,
        to: DVec3,
        feed: f64,
    ) -> Result<bool, LinkError> {
        let StayDown::Level { max } = self.p.stay_down else {
            return Ok(false);
        };
        // Where the way down starts, seen from above, and how low it starts.
        let (xy, lowest) = match way {
            Way::Plunge => (to.truncate(), to.z),
            Way::Lead { top, .. } => (top.truncate(), top.z),
            Way::Helix {
                end, aside, radius, ..
            } => match self.helix(*end, *aside, *radius, Some(off.z)) {
                Some(h) => (h.start.truncate(), end.z),
                None => (end.truncate(), end.z),
            },
            Way::Ramp { .. } => return Ok(false),
        };
        if off.z <= lowest + 1e-6 || off.truncate().distance(xy) > max {
            return Ok(false);
        }
        let reached = self
            .surface
            .grazing_along(off.truncate(), xy)
            .iter()
            .any(|p| p.z > off.z + 1e-6);
        let cuts = self
            .material
            .highest(off.truncate(), xy, self.p.tool_radius)
            .is_some_and(|h| h > off.z + self.p.max_cut_step);
        if reached || cuts {
            return Ok(false);
        }
        self.out
            .feed_to(xy.extend(off.z), MoveRole::Transition, feed)?;
        self.descend(way, to, feed)?;
        Ok(true)
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
                let reached = self
                    .surface
                    .grazing_along(a, b)
                    .iter()
                    .any(|p| p.z > to.z + 1e-6);
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

/// The vertical plane a vertical lead turns in, for a pass running along `t`
/// at `p`: the level direction `u` it runs in, the axis the lead turns about,
/// and the lead's centre, `radius` above the pass square to its slope.
fn lead_plane(p: DVec3, t: DVec3, radius: f64) -> Option<(DVec3, DVec3, DVec3)> {
    let flat = t.truncate();
    if flat.length() < 1e-9 || radius.is_nan() || radius <= 0.0 {
        return None;
    }
    let u = flat.normalize().extend(0.0);
    let theta = t.z.atan2(flat.length());
    let normal = -theta.sin() * u + theta.cos() * DVec3::Z;
    Some((u, DVec3::Z.cross(u), p + radius * normal))
}

/// The vertical lead into a pass starting at `to` along `t`, as Fusion's: an arc
/// of `radius` in the pass's vertical plane, from straight down to the pass's own
/// slope. Where it starts, and the arc from there to `to`.
fn lead_in(to: DVec3, t: DVec3, radius: f64) -> Option<(DVec3, Arc)> {
    let (u, axis, centre) = lead_plane(to, t, radius)?;
    let top = centre - radius * u;
    let arc = Arc::about(centre, axis, top, to, false, radius * 1e-9)?;
    Some((top, arc))
}

/// The vertical lead out of a pass ending at `from` along `t`: the lead in,
/// mirrored. Where it ends, and the arc from `from` to there.
fn lead_out(from: DVec3, t: DVec3, radius: f64) -> Option<(DVec3, Arc)> {
    let (u, axis, centre) = lead_plane(from, t, radius)?;
    let end = centre + radius * u;
    let arc = Arc::about(centre, axis, from, end, false, radius * 1e-9)?;
    Some((end, arc))
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
        // Down to a lower level it goes across at the height it is, then down:
        // as a profile goes to its ring's next depth.
        let (out, stepped) = link(&deeper, &plane, &mut Air, &FlatPart(-5.0), &level).unwrap();
        assert_eq!(stepped.stayed_down, 1);
        assert_eq!(
            roles(&out)[4..6],
            [MoveRole::Transition, MoveRole::Plunge],
            "{:?}",
            out.moves()
        );
        assert_eq!(out.moves()[4].to, dvec3(5.0, 1.0, 0.0), "across, level");
        // Up to a higher one it lifts clear.
        let higher = {
            let mut t = Toolpath::new();
            for m in deeper.moves() {
                t.push(Move {
                    to: dvec3(m.to.x, m.to.y, -m.to.z),
                    ..*m
                })
                .unwrap();
            }
            t
        };
        let (_, up) = link(&higher, &plane, &mut Air, &FlatPart(-5.0), &level).unwrap();
        assert_eq!(up.stayed_down, 0, "never across and up");
    }

    /// Fusion's vertical lead: a quarter turn of the lead radius from straight
    /// down to the pass, in the pass's vertical plane, ending on it level.
    #[test]
    fn a_lead_in_arcs_down_onto_the_pass() {
        let r = 0.6;
        let to = dvec3(1.0, 2.0, 3.0);
        let (top, arc) = lead_in(to, DVec3::X, r).unwrap();
        assert!(
            (top - dvec3(1.0 - r, 2.0, 3.0 + r)).length() < 1e-12,
            "{top:?}"
        );
        assert!(arc.ends_at(top, to, 1e-12));
        assert!((arc.centre - dvec3(1.0, 2.0, 3.0 + r)).length() < 1e-12);
        assert!((arc.sweep.abs() - FRAC_PI_2).abs() < 1e-12);
        // In the pass's plane: about Y, so every point keeps its Y.
        assert_eq!(arc.axis, DVec3::Y);
        // Straight down at the top, level as the pass runs at the bottom.
        assert!((arc.tangent_at(top, to, 0.0) + DVec3::Z).length() < 1e-9);
        assert!((arc.tangent_at(top, to, 1.0) - DVec3::X).length() < 1e-9);

        let (end, out) = lead_out(to, DVec3::X, r).unwrap();
        assert!((end - dvec3(1.0 + r, 2.0, 3.0 + r)).length() < 1e-12);
        assert!((out.tangent_at(to, end, 0.0) - DVec3::X).length() < 1e-9);
        assert!((out.tangent_at(to, end, 1.0) - DVec3::Z).length() < 1e-9);
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

    /// The tool arrives at the clearance height above the first pass, and only
    /// then comes down to where it goes across between passes; a full
    /// retraction never goes across lower than the retract height.
    #[test]
    fn arriving_from_the_clearance_and_retracting_fully() {
        let field = ClearanceField::new(
            HeightMap::filled(dvec2(-20.0, -20.0), 0.5, 81, 81, 0.0).unwrap(),
            2.0,
            30.0,
        )
        .unwrap();
        let arriving = LinkParams {
            start_z: Some(30.0),
            ..params()
        };
        let (out, _) = link(&zigzag(15.0), &field, &mut Air, &NoPart, &arriving).unwrap();
        let first: Vec<DVec3> = out.moves()[..2].iter().map(|m| m.to).collect();
        assert_eq!(first, [dvec3(0.0, 0.0, 30.0), dvec3(0.0, 0.0, 2.0)]);
        // Across between passes at the material's safe distance, 2 mm up.
        assert!(
            out.moves()
                .iter()
                .filter(|m| m.role == MoveRole::Rapid)
                .skip(1)
                .all(|m| m.to.z <= 2.0 + 1e-9),
            "{:?}",
            out.moves()
        );
        let full = field.at_least(5.0);
        let (out, _) = link(&zigzag(15.0), &full, &mut Air, &NoPart, &params()).unwrap();
        let across: Vec<f64> = out
            .moves()
            .windows(2)
            .filter(|w| w[1].role == MoveRole::Rapid && w[0].to.z == w[1].to.z)
            .map(|w| w[1].to.z)
            .collect();
        assert!(
            !across.is_empty() && across.iter().all(|z| *z == 5.0),
            "{across:?}"
        );
    }

    fn entering(entry: EntryKind) -> LinkParams {
        LinkParams {
            ramp: Some(Ramp {
                angle: 2f64.to_radians(),
                feed: 250.0,
            }),
            entry,
            helix_radius: 2.0,
            ..params()
        }
    }

    /// Into material down a helix beside where the pass starts, off to the side
    /// it climbs from and touching its line there: turns about Z, each at most a
    /// whole one, ramp role and feed, from the safe distance above the material,
    /// ending on the pass heading the way it runs.
    #[test]
    fn a_helix_goes_down_beside_the_pass() {
        let plane = ClearancePlane::new(10.0).unwrap();
        let p = entering(EntryKind::Helix);
        let (out, report) = link(&ring(), &plane, &mut level_material(), &NoPart, &p).unwrap();
        assert_eq!(report.helixes, 1);
        let moves = out.moves();
        let first = moves.iter().position(|m| m.role == MoveRole::Ramp).unwrap();
        let start = moves[first - 1].to;
        assert!(
            (start.z - 2.0).abs() < 1e-9,
            "from the safe distance: {start}"
        );
        let helix: Vec<&Move> = moves[first..]
            .iter()
            .take_while(|m| m.role == MoveRole::Ramp)
            .collect();
        let (bottom, down) = helix.split_last().unwrap();
        let mut at = start;
        let mut turned = 0.0;
        let mut before = start;
        for m in down {
            let MoveKind::Arc(a) = m.kind else {
                panic!("a helix is arcs: {m:?}");
            };
            assert!((a.centre.truncate() - dvec2(0.0, 2.0)).length() < 1e-9);
            assert!(a.sweep.abs() <= TAU + 1e-9);
            assert!(m.to.z < at.z);
            assert_eq!(m.feed, Some(250.0));
            turned += a.sweep.abs();
            before = at;
            at = m.to;
        }
        assert_eq!(at, dvec3(0.0, 0.0, -2.0), "it ends on the pass");
        let last = down.last().unwrap();
        let heading = last.tangent_at(before, 1.0).unwrap().truncate().normalize();
        assert!((heading - dvec2(1.0, 0.0)).length() < 1e-6, "{heading}");
        // Then once round level, clearing its circle to the depth.
        let MoveKind::Arc(a) = bottom.kind else {
            panic!("{bottom:?}");
        };
        assert_eq!(bottom.to, at);
        assert!((a.sweep.abs() - TAU).abs() < 1e-12);
        // Four millimetres down at two degrees round a circle of radius two.
        let expected = 4.0 / (2.0 * 2f64.to_radians().tan());
        assert!((turned - expected).abs() < 1e-6, "{turned} vs {expected}");

        // No steeper than the most a lap may go down.
        let shallow = LinkParams {
            ramp_stepdown: Some(0.1),
            ..p
        };
        let (out, _) = link(&ring(), &plane, &mut level_material(), &NoPart, &shallow).unwrap();
        let mut at = out.moves()[0].to;
        for m in out.moves() {
            if let (MoveKind::Arc(a), MoveRole::Ramp) = (m.kind, m.role) {
                let per_turn = (at.z - m.to.z) / (a.sweep.abs() / TAU);
                assert!(per_turn <= 0.1 + 1e-9, "{per_turn}");
            }
            at = m.to;
        }
    }

    /// The helix goes into the waste, the side the cut climbs from, never over
    /// the part it is cutting beside, however low the part is there.
    #[test]
    fn a_helix_is_in_the_waste() {
        // The part is beside the ring, on its right as it runs along X, below
        // the pass: a helix over it would not touch it at this depth, but
        // would at the next.
        struct Right;
        impl Surface for Right {
            fn at(&self, p: DVec2) -> Option<f64> {
                (p.y < -0.01).then_some(-3.0)
            }
            fn along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
                vec![from.extend(-3.0), to.extend(-3.0)]
            }
        }
        let plane = ClearancePlane::new(10.0).unwrap();
        let p = entering(EntryKind::Helix);
        let (out, report) = link(&ring(), &plane, &mut level_material(), &Right, &p).unwrap();
        assert_eq!(report.helixes, 1);
        for m in out.moves() {
            if let (MoveKind::Arc(a), MoveRole::Ramp) = (m.kind, m.role) {
                assert!(a.centre.y > 0.0, "the helix is off the waste side: {a:?}");
            }
        }
    }

    /// Where the part leaves no room for a helix beside the pass, even a quarter
    /// the size, the tool goes straight down instead.
    #[test]
    fn no_room_for_a_helix_is_a_plunge() {
        struct Behind;
        impl Surface for Behind {
            fn at(&self, p: DVec2) -> Option<f64> {
                Some(if p.x < -0.01 { 1.0 } else { -5.0 })
            }
            fn along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
                vec![from.extend(-5.0), to.extend(-5.0)]
            }
        }
        let plane = ClearancePlane::new(10.0).unwrap();
        let p = entering(EntryKind::Helix);
        let (out, report) = link(&ring(), &plane, &mut level_material(), &Behind, &p).unwrap();
        assert_eq!(report.helixes, 0);
        let plunge = out
            .moves()
            .iter()
            .find(|m| m.role == MoveRole::Plunge)
            .unwrap();
        assert_eq!(plunge.to, dvec3(0.0, 0.0, -2.0));
    }

    /// A ring with leads ramped into goes down along the ring itself, in the
    /// direction it cuts, onto its start, and is entered there without its
    /// lead-in: never back along it, against the cut.
    #[test]
    fn a_ring_with_leads_is_ramped_along_the_way_it_cuts() {
        let mut t = Toolpath::new();
        t.feed_to(dvec3(-1.0, -1.0, -2.0), MoveRole::Link, 1000.0)
            .unwrap();
        t.feed_to(dvec3(0.0, 0.0, -2.0), MoveRole::Lead, 1000.0)
            .unwrap();
        for p in [(10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)] {
            t.feed_to(dvec3(p.0, p.1, -2.0), MoveRole::Cut, 1000.0)
                .unwrap();
        }
        t.feed_to(dvec3(-1.0, 1.0, -2.0), MoveRole::Lead, 1000.0)
            .unwrap();
        let plane = ClearancePlane::new(10.0).unwrap();
        let p = entering(EntryKind::Ramp);
        let (out, report) = link(&t, &plane, &mut level_material(), &NoPart, &p).unwrap();
        assert_eq!(report.ramps, 1);
        let moves = out.moves();
        let first_cut = moves.iter().position(|m| m.role == MoveRole::Cut).unwrap();
        assert!(
            moves[..first_cut].iter().all(|m| m.role != MoveRole::Lead),
            "the lead-in is left out: {moves:?}"
        );
        // Down onto the ring's start along its last side, as the ring runs.
        let last = moves[first_cut - 1];
        let before = moves[first_cut - 2].to;
        assert_eq!(last.to, dvec3(0.0, 0.0, -2.0));
        assert!(before.x.abs() < 1e-9 && before.y > 0.0, "{before}");
        let mut z = f64::INFINITY;
        for m in &moves[..first_cut] {
            if m.role == MoveRole::Ramp {
                assert!(m.to.z <= z + 1e-12);
                z = m.to.z;
            }
        }
        // Round a lap of 40 mm, no more than the most a lap may go down.
        let lapped = LinkParams {
            ramp_stepdown: Some(0.5),
            ..p
        };
        let (out, _) = link(&t, &plane, &mut level_material(), &NoPart, &lapped).unwrap();
        let mut at = out.moves()[0].to;
        for m in out.moves() {
            if m.role == MoveRole::Ramp {
                let flat = (m.to - at).truncate().length();
                assert!(at.z - m.to.z <= flat * 0.5 / 40.0 + 1e-9);
            }
            at = m.to;
        }
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

    /// A ramp rides over a tab on the pass, as the pass does, and comes straight
    /// down off it as the pass does: at the plunge feed, not the ramp's.
    #[test]
    fn a_ramp_comes_down_off_a_tab_at_the_plunge_feed() {
        let mut t = Toolpath::new();
        t.feed_to(dvec3(0.0, 0.0, -2.0), MoveRole::Link, 1000.0)
            .unwrap();
        for p in [
            (10.0, 0.0, -2.0),
            (10.0, 4.0, -2.0),
            (10.0, 4.0, -0.5),
            (10.0, 6.0, -0.5),
            (10.0, 6.0, -2.0),
            (10.0, 10.0, -2.0),
            (0.0, 10.0, -2.0),
            (0.0, 0.0, -2.0),
        ] {
            t.feed_to(dvec3(p.0, p.1, p.2), MoveRole::Cut, 1000.0)
                .unwrap();
        }
        let plane = ClearancePlane::new(10.0).unwrap();
        let ramping = LinkParams {
            ramp: Some(Ramp {
                angle: 2f64.to_radians(),
                feed: 250.0,
            }),
            ..params()
        };
        let (out, report) = link(&t, &plane, &mut level_material(), &NoPart, &ramping).unwrap();
        assert_eq!(report.ramps, 1);
        let entry: Vec<&Move> = out
            .moves()
            .iter()
            .take_while(|m| m.role != MoveRole::Cut)
            .collect();
        let mut downs = 0;
        for w in entry.windows(2) {
            let (a, b) = (w[0].to, w[1].to);
            if b.z < a.z - 1e-9
                && a.truncate().distance(b.truncate()) < 1e-9
                && w[1].role != MoveRole::Rapid
            {
                downs += 1;
                assert_eq!(
                    w[1].role,
                    MoveRole::Plunge,
                    "straight down in a ramp at {b}"
                );
                assert_eq!(w[1].feed, Some(ramping.plunge_feed));
            }
        }
        assert!(downs > 0, "the ramp never came down off the tab");
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
