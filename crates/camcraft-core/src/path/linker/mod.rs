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
//! can. Fixture and clamp avoidance needs the answer to vary with position too,
//! and is one more implementation rather than a change at every call site.
//! Keeping the tool down between cuts is not a clearance at all: it is
//! [`StayDown`].
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

mod clearance;
mod entry;
mod params;
mod world;

use std::f64::consts::{FRAC_PI_2, PI};

use glam::{DVec2, DVec3, dvec3};

pub use clearance::{Clearance, ClearanceField, ClearancePlane, Under, highest_under};
use entry::{Way, lead_out};
pub use params::{EntryKind, LinkError, LinkParams, LinkReport, Ramp, StayDown};
pub use world::{Air, Material, NoPart, Surface, Unchanging};

use crate::is_positive;
use crate::path::toolpath::{Arc, Move, MoveRole, PLUNGE_RATIO, Toolpath, steep};

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
            must: NOT_NEGATIVE,
            value: params.safe_distance,
        });
    }
    if !params.lead_radius.is_finite() || params.lead_radius < 0.0 {
        return Err(LinkError::BadParameter {
            name: "lead_radius",
            must: NOT_NEGATIVE,
            value: params.lead_radius,
        });
    }
    if let Some(r) = params.ramp {
        check("ramp feed", r.feed)?;
        if !(r.angle > 0.0 && r.angle < FRAC_PI_2) {
            return Err(LinkError::BadParameter {
                name: "ramp angle",
                must: "greater than zero and less than a right angle, in radians",
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
    let mut at: Option<DVec3> = None;
    for moves in path.passes() {
        match moves.split_first() {
            // A link, and the pass it leads into: every move up to the next.
            Some((m, pass)) if m.role == MoveRole::Link => {
                l.settle();
                let skip = l.link(at, m.to, pass)?;
                for n in pass.get(skip..).unwrap_or(&[]) {
                    l.out.push(*n)?;
                }
                at = pass.last().map_or(Some(m.to), |n| Some(n.to));
            }
            // What comes before the first link, as it is.
            _ => {
                for m in moves {
                    l.out.push(*m)?;
                    at = Some(m.to);
                }
            }
        }
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
            && self.stay_down(from, to, pass, feed)?
        {
            self.report.stayed_down += 1;
            self.report.links_resolved += 1;
            return Ok(0);
        }

        let way = self.entry(to, pass);
        let skip = way.skip();

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
        let start = way.start(to, self.helix_down(&way, None).as_ref());
        let from_xy = top.map_or(start.truncate(), |t| t.truncate());
        let safe = self
            .clearance
            .safe_z(from_xy, start.truncate(), self.p.tool_radius);
        if !safe.is_finite() {
            return Err(LinkError::BadClearance { at: from_xy });
        }
        // Never across lower than the tool came out: it would rapid down on the
        // way. Nor lower than the way in starts, which it goes down to.
        let safe = top.map_or(safe, |t| safe.max(t.z)).max(start.z);
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
                // Arriving at `to` itself is no plunge where it is already the
                // safe distance clear, as a helix's start is.
                let clear = self.feed_height(to.truncate(), f64::MIN) <= to.z + 1e-9;
                if stop <= to.z && !clear {
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
        if let Way::Ramp { .. } = way {
            return Ok(false);
        }
        // Where the way down starts, seen from above, and how low it goes before
        // any lead.
        let helix = self.helix_down(way, Some(off.z));
        let xy = way.start(to, helix.as_ref()).truncate();
        let lowest = way.bottom(to).z;
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

    /// Whether a link at level `z` from `a` to `b` would plough through material
    /// the cuts either end of it leave standing: as Fusion's, a link stays down
    /// only where it does not violate the work. Its first and last tool's radius
    /// are the cuts' own, coming off one and into the next as they engage; between,
    /// the tool may brush what stands beside its way, half its radius in at
    /// most, and no more.
    fn ploughs(&self, a: DVec2, b: DVec2, z: f64) -> bool {
        let r = self.p.tool_radius;
        let span = a.distance(b);
        if span <= 2.0 * r {
            return false;
        }
        let d = (b - a) / span;
        self.material
            .highest(a + d * r, b - d * r, 0.5 * r)
            .is_some_and(|top| top > z + self.p.tolerance)
    }

    /// The turn from the end of the last pass, at `from`, round onto the next
    /// at `to`, where they run opposite ways side by side, as zigzag passes
    /// do: Fusion's default transition, smooth ("smooth tangential movements
    /// using true arcs where appropriate"). A half turn as wide as the passes
    /// are apart, out past their ends the way the last was heading, tangent to
    /// both; where one pass ends short of the other, straight on along it
    /// first, or after, so the turn is at the further end. Level as seen from
    /// above, it goes from one pass's height to the other's as it goes.
    ///
    /// `None` where the passes do not run so, or are further apart than the
    /// tool is wide, as a turn that size would swing far off the work; or
    /// where the part rises into the turn, the turn would cut more than a cut
    /// step into the material, or would fall steeply further than a descent at
    /// cutting feed may. The lifted straight line is made instead.
    fn turn_around(
        &self,
        from: DVec3,
        to: DVec3,
        pass: &[Move],
    ) -> Option<Vec<(DVec3, Option<Arc>)>> {
        let moves = self.out.moves();
        let [before, last] = moves.get(moves.len().checked_sub(2)?..)? else {
            return None;
        };
        let out = last
            .tangent_at(before.to, 1.0)?
            .truncate()
            .try_normalize()?;
        let into = pass
            .first()?
            .tangent_at(to, 0.0)?
            .truncate()
            .try_normalize()?;
        if out.dot(into) > -0.99 {
            return None;
        }
        let (a, b) = (from.truncate(), to.truncate());
        // How far the next pass starts along the way the last was heading,
        // and the turn's width: the gap square to the passes.
        let along = (b - a).dot(out);
        let width = (b - a - out * along).length();
        if width < 1e-6 || width > 2.0 * self.p.tool_radius {
            return None;
        }
        // The straight on, before the turn or after it, and the turn's ends.
        let (turn_from, turn_to) = if along >= 0.0 {
            (a + out * along, b)
        } else {
            (a, b + out * -along)
        };
        let straight = along.abs();
        let length = straight + std::f64::consts::FRAC_PI_2 * width;
        let z_at = |run: f64| from.z + (to.z - from.z) * (run / length);
        let (p, q) = if along >= 0.0 {
            (turn_from.extend(z_at(straight)), to)
        } else {
            (from, turn_to.extend(z_at(length - straight)))
        };
        let centre = (turn_from + turn_to) / 2.0;
        // Counter-clockwise starts off along Z × the radius: the way the pass
        // was heading, or it turns the other way.
        let ccw = (turn_from - centre).perp().dot(out) > 0.0;
        let arc = Arc {
            centre: centre.extend(p.z),
            axis: DVec3::Z,
            sweep: if ccw { PI } else { -PI },
        };
        let mut pieces = Vec::new();
        if along >= 0.0 && straight > 1e-9 {
            pieces.push((p, None));
        }
        pieces.push((q, Some(arc)));
        if along < 0.0 && straight > 1e-9 {
            pieces.push((to, None));
        }
        // What it passes through, close enough that the tool's own width
        // covers what lies between.
        let mut way: Vec<DVec3> = Vec::new();
        let mut at = from;
        for &(end, arc) in &pieces {
            let n = 32;
            way.extend((1..=n).map(|k| {
                let t = f64::from(k) / f64::from(n);
                arc.map_or_else(|| at.lerp(end, t), |c| c.point_at(at, end, t))
            }));
            at = end;
        }
        // Clear of the part; and of the material, brushing what stands beside
        // it half the tool's radius in at most, as a level link may: at the
        // next pass's start, what it meets is that pass's own cut.
        let clear = way.iter().all(|p| {
            let xy = p.truncate();
            self.surface
                .at(xy)
                .is_none_or(|z| z <= p.z + self.p.tolerance)
                && (xy.distance(b) < 1e-6
                    || self
                        .material
                        .highest(xy, xy, 0.5 * self.p.tool_radius)
                        .is_none_or(|top| top <= p.z + self.p.max_cut_step))
        });
        let fallen = trailing_fall(moves);
        (clear && !falls_steeply(fallen, from, &way, pass, self.p.max_cut_step)).then_some(pieces)
    }

    /// Make the link from `from` to `to`, where `pass` starts, without lifting
    /// clear, if it may be.
    fn stay_down(
        &mut self,
        from: DVec3,
        to: DVec3,
        pass: &[Move],
        feed: f64,
    ) -> Result<bool, LinkError> {
        let (a, b) = (from.truncate(), to.truncate());
        let span = a.distance(b);
        let way: Vec<DVec3> = match self.p.stay_down {
            StayDown::Never => return Ok(false),
            StayDown::OverPart { max } => {
                if span > max {
                    return Ok(false);
                }
                // Only between points on the part. A pass beside a wall, below
                // where the tool would come to rest from above, is not on it: the
                // way from it to another is through the wall, however short, and
                // the part sampled between them may not show it.
                let below = |p: DVec3| {
                    self.surface
                        .at(p.truncate())
                        .is_some_and(|z| z > p.z + self.p.tolerance)
                };
                if below(from) || below(to) {
                    return Ok(false);
                }
                // Fusion's default transition, smooth: a half turn tangent to
                // both passes, where it fits.
                if let Some(pieces) = self.turn_around(from, to, pass) {
                    for (p, arc) in pieces {
                        match arc {
                            Some(arc) => self.out.arc_to(p, arc, MoveRole::Transition, feed)?,
                            None => self.out.feed_to(p, MoveRole::Transition, feed)?,
                        }
                    }
                    return Ok(true);
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
                if reached || self.ploughs(a, b, to.z) {
                    return Ok(false);
                }
                vec![to]
            }
        };
        let fallen = trailing_fall(self.out.moves());
        if falls_steeply(fallen, from, &way, pass, self.p.max_cut_step) {
            return Ok(false);
        }
        for p in way {
            self.out.feed_to(p, MoveRole::Transition, feed)?;
        }
        Ok(true)
    }
}

/// Whether a move falls steeply at the cutting feed, as the gate judges one:
/// see [`Toolpath::steep_descents`].
fn falls(m: &Move, at: DVec3) -> bool {
    matches!(m.role, MoveRole::Cut | MoveRole::Transition)
        && steep(at.z - m.to.z, m.flat_length(at), PLUNGE_RATIO)
}

/// How far `moves` have been falling steeply at the cutting feed at their end:
/// a cut coming down a wall, which a link going on down would add to.
fn trailing_fall(moves: &[Move]) -> f64 {
    moves
        .windows(2)
        .rev()
        .map_while(|w| match w {
            [a, m] if falls(m, a.to) => Some(a.to.z - m.to.z),
            _ => None,
        })
        .sum()
}

/// Whether a way from `from` through `way` into `pass` falls further than `max`
/// in one continuous descent steeper than [`PLUNGE_RATIO`], counting the `fallen`
/// it carries on from: the plunge at cutting feed the gate refuses. The gate
/// judges the whole program, so a descent the way joins, before or after it, is
/// as much the way's as its own.
fn falls_steeply(fallen: f64, from: DVec3, way: &[DVec3], pass: &[Move], max: f64) -> bool {
    let mut depth = fallen;
    let mut at = from;
    for &p in way {
        let drop = at.z - p.z;
        let flat = p.truncate().distance(at.truncate());
        depth = if steep(drop, flat, PLUNGE_RATIO) {
            depth + drop
        } else {
            0.0
        };
        if depth > max {
            return true;
        }
        at = p;
    }
    // And on into the pass, while it keeps falling.
    for m in pass {
        if depth <= 0.0 || !falls(m, at) {
            break;
        }
        depth += at.z - m.to.z;
        if depth > max {
            return true;
        }
        at = m.to;
    }
    false
}

/// What a feed, a tool's radius or a tolerance must be.
const POSITIVE: &str = "finite and greater than zero";

/// What a distance that may be nothing must be.
const NOT_NEGATIVE: &str = "finite and not negative";

/// `value`, for the parameter called `name`, if it is usable: finite and
/// greater than zero.
fn check(name: &'static str, value: f64) -> Result<(), LinkError> {
    is_positive(value)
        .then_some(())
        .ok_or(LinkError::BadParameter {
            name,
            must: POSITIVE,
            value,
        })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests;
