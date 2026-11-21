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
//! # The order a link is made in
//!
//! Each link is made the first way that fits: without lifting, as [`StayDown`]
//! allows (round onto the next pass, straight over the part, straight beside
//! it, straight at a level); otherwise off the last cut along its lead-out,
//! and, clearing at a level, across at that height and down onto the next;
//! otherwise up to the safe distance above the material, across at the safe
//! height, down at rapid to the safe distance above where the way in starts,
//! and down the way in: straight, a vertical lead, a helix or a ramp.
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
mod stay;
mod world;

use glam::{DVec2, DVec3, dvec3};

pub(crate) use clearance::highest_under;
pub use clearance::{Clearance, ClearanceField, ClearancePlane};
use entry::{Way, lead_out};
pub use params::{EntryKind, LinkError, LinkParams, LinkReport, Ramp, StayDown};
pub use world::{Air, Material, NoPart, Surface, Unchanging};

use crate::control::{Watch, share};
use crate::path::toolpath::{Move, MoveRole, Toolpath};

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
    link_watched(
        path,
        clearance,
        material,
        surface,
        params,
        &Watch::default(),
    )
}

/// As [`link`], telling `watch` how far it has got, pass by pass, and stopping
/// between passes once it is to.
pub fn link_watched<C: Clearance, M: Material, S: Surface>(
    path: &Toolpath,
    clearance: &C,
    material: &mut M,
    surface: &S,
    params: &LinkParams,
    watch: &Watch,
) -> Result<(Toolpath, LinkReport), LinkError> {
    params.validate()?;
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
    let passes = path.passes().count();
    for (done, moves) in path.passes().enumerate() {
        watch.check()?;
        watch.report(share(done, passes));
        match moves.split_first() {
            // A link, and the pass it leads into: every move up to the next.
            Some((m, pass)) if m.role == MoveRole::Link => {
                l.settle();
                let skip = l.link(at, m.to, pass)?;
                for n in pass.get(skip..).unwrap_or(&[]) {
                    l.out.push(*n)?;
                }
                at = Some(pass.last().map_or(m.to, |n| n.to));
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
        l.end(from, z)?;
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
    /// Resolve the link from `at`, where the last pass ended, if any, to `to`,
    /// the start of `pass`, the first way that fits: without lifting
    /// ([`Linker::stay_down`]); otherwise off the last cut and across and down
    /// onto the next ([`Linker::across_and_down`]); otherwise up clear of the
    /// material, or from the clearance height for the first, across and down
    /// the way in ([`Linker::across_and_in`]). Returns how many of the pass's
    /// first moves the way in replaced, which are left out.
    fn link(&mut self, at: Option<DVec3>, to: DVec3, pass: &[Move]) -> Result<usize, LinkError> {
        self.report.links_resolved += 1;
        let feed = pass
            .first()
            .and_then(|m| m.feed)
            .unwrap_or(self.p.plunge_feed);
        if let Some(from) = at
            && self.stay_down(from, to, pass, feed)?
        {
            self.report.stayed_down += 1;
            return Ok(0);
        }
        let way = self.entry(to, pass);
        let top = match at {
            Some(from) => {
                let off = self.lead_off(from, last_feed(self.out.moves()).unwrap_or(feed))?;
                if self.across_and_down(off, &way, to, feed)? {
                    self.report.stayed_down += 1;
                    return Ok(way.skip());
                }
                Some(self.rise(off)?)
            }
            None => None,
        };
        self.across_and_in(top, &way, to, feed)?;
        Ok(way.skip())
    }

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

    /// Whether `from` and every point of `way` are at or above the part under
    /// them, for a tool that may graze it.
    fn clear_of_part(&self, from: DVec3, way: &[DVec3]) -> bool {
        std::iter::once(&from).chain(way).all(|p| {
            self.surface
                .grazing_at(p.truncate())
                .is_none_or(|z| z <= p.z + 1e-9)
        })
    }

    /// The way the tool is heading at the end of the last move made: along the
    /// last pass, as it ends.
    fn heading(&self) -> Option<DVec3> {
        let moves = self.out.moves();
        let [before, last] = moves.get(moves.len().checked_sub(2)?..)? else {
            return None;
        };
        last.tangent_at(before.to, 1.0)
    }

    /// Off the cut at `from` along the vertical lead-out, where it fits the
    /// part. Returns where the tool is.
    fn lead_off(&mut self, from: DVec3, feed: f64) -> Result<DVec3, LinkError> {
        if let Some(t) = self.heading()
            && let Some((end, arc)) = lead_out(from, t, self.p.lead_out())
            && self.clear_of_part(from, &arc.chords(from, end, self.p.tolerance))
        {
            self.out
                .arc_or_chords_to(end, arc, MoveRole::Lead, feed, self.p.tolerance)?;
            self.settle();
            return Ok(end);
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

    /// From `top`, where the tool rose to off the last cut, or from the
    /// clearance height for the first pass: across to above where `way`
    /// starts, down at rapid to where it starts feeding, and down it onto
    /// `to`.
    fn across_and_in(
        &mut self,
        top: Option<DVec3>,
        way: &Way,
        to: DVec3,
        feed: f64,
    ) -> Result<(), LinkError> {
        let start = self.start_of(way, to, None);
        let safe = self.across(top, start)?;
        self.down_to_feed(way, to, start, safe)?;
        self.descend(way, to, feed)
    }

    /// Across at the safe height from `top`, or from the clearance height
    /// for the first pass, to above `start`. Returns the safe height.
    fn across(&mut self, top: Option<DVec3>, start: DVec3) -> Result<f64, LinkError> {
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
        Ok(safe)
    }

    /// From `safe` above `start`, where `way` onto `to` starts, down at rapid
    /// to where it starts feeding: the safe distance above the material, or
    /// for a helix or a ramp above the part, then at the plunge feed to where
    /// it starts.
    fn down_to_feed(
        &mut self,
        way: &Way,
        to: DVec3,
        start: DVec3,
        safe: f64,
    ) -> Result<(), LinkError> {
        match way {
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
        Ok(())
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

    /// After the last cut, at `from`: off it along the lead-out, up clear of
    /// the material, and up to `z`, or as high as the clearance asks over
    /// where it rose to, so the program never ends with the cutter on the
    /// part.
    fn end(&mut self, from: DVec3, z: f64) -> Result<(), LinkError> {
        let feed = last_feed(self.out.moves()).unwrap_or(self.p.retract_feed);
        let off = self.lead_off(from, feed)?;
        let top = self.rise(off)?;
        let over = self
            .clearance
            .safe_z(top.truncate(), top.truncate(), self.p.tool_radius);
        let up = z.max(over);
        if up > top.z {
            self.out.rapid_to(dvec3(top.x, top.y, up))?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests;
