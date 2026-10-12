//! How a pass is entered: straight down, down a vertical lead, down a helix
//! beside it, or ramped along the pass itself.

use std::f64::consts::{PI, TAU};

use camcraft_geom::arc::Arc;
use glam::{DVec2, DVec3, dvec3};

use super::{Clearance, EntryKind, Linker, Material, Ramp, Surface};
use crate::path::toolpath::{Move, MoveKind, MoveRole, turns_about_z};

/// How a pass is entered, planned before the height it starts from is known.
pub(super) enum Way {
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

impl Way {
    /// How many of the pass's first moves it replaces, which are left out.
    pub(super) fn skip(&self) -> usize {
        match self {
            Way::Ramp { skip, .. } => *skip,
            _ => 0,
        }
    }

    /// Where it starts going down onto `to`: `to` itself for a plunge, the
    /// lead's top, where a ramp starts, or where `helix`, the helix it goes
    /// down, starts; where that helix would end, if there is none.
    pub(super) fn start(&self, to: DVec3, helix: Option<&Helix>) -> DVec3 {
        match self {
            Way::Plunge => to,
            Way::Lead { top, .. } => *top,
            Way::Helix { end, .. } => helix.map_or(*end, |h| h.start),
            Way::Ramp { start, .. } => *start,
        }
    }

    /// Where it has come down to onto `to` before any vertical lead: `to`
    /// itself, the lead's top, or the helix's end.
    pub(super) fn bottom(&self, to: DVec3) -> DVec3 {
        match self {
            Way::Plunge | Way::Ramp { .. } => to,
            Way::Lead { top, .. } => *top,
            Way::Helix { end, .. } => *end,
        }
    }
}

/// A helix, as the tool goes down it.
pub(super) struct Helix {
    /// Where it starts.
    pub(super) start: DVec3,
    /// Its turns, each at most one, the last a level one at the bottom, ending
    /// where it ends.
    pub(super) moves: Vec<(DVec3, Arc)>,
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
        self.m.flat_length(self.from)
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
    // Measured once, however many times round it goes.
    let lengths: Vec<f64> = lap.iter().map(Seg::flat_length).collect();
    let total: f64 = lengths.iter().sum();
    if total.is_nan() || total <= 1e-9 {
        return None;
    }
    let mut out = Vec::new();
    let mut left = run;
    // Bounded: each lap is at least the ramp's minimum length long.
    for (s, &length) in lap
        .iter()
        .zip(&lengths)
        .cycle()
        .take(lap.len().saturating_mul(10_000))
    {
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

impl<C: Clearance, M: Material, S: Surface> Linker<'_, C, M, S> {
    /// How `pass`, starting at `to`, is entered.
    pub(super) fn entry(&self, to: DVec3, pass: &[Move]) -> Way {
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
                    None => plain(),
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
    pub(super) fn helix(
        &self,
        end: DVec3,
        aside: DVec2,
        radius: f64,
        from_z: Option<f64>,
    ) -> Option<Helix> {
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
        // never begins inside it: the material the tool would cut, a tolerance
        // in from its edge, as an arc's chords stray that far inside it and the
        // tool on them reaches past the slot the arc cut.
        let mut top = to.z;
        let reach = (self.p.tool_radius - self.p.tolerance).max(0.0);
        for s in pass {
            let mut a = s.from.truncate();
            for p in s.m.chords(s.from, self.p.tolerance) {
                if let Some(h) = self.material.highest(a, p.truncate(), reach) {
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
                MoveKind::Arc(a) if turns_about_z(&a) && !straight_down => {
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

    /// The helix `way` goes down, where it is one, from no higher than
    /// `from_z`, if given.
    pub(super) fn helix_down(&self, way: &Way, from_z: Option<f64>) -> Option<Helix> {
        let Way::Helix {
            end, aside, radius, ..
        } = way
        else {
            return None;
        };
        self.helix(*end, *aside, *radius, from_z)
    }

    /// Whether `from` and every point of `way` are at or above the part under
    /// them.
    pub(super) fn clear_of_part(&self, from: DVec3, way: &[DVec3]) -> bool {
        std::iter::once(&from).chain(way).all(|p| {
            self.surface
                .grazing_at(p.truncate())
                .is_none_or(|z| z <= p.z + 1e-9)
        })
    }
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
pub(super) fn lead_out(from: DVec3, t: DVec3, radius: f64) -> Option<(DVec3, Arc)> {
    let (u, axis, centre) = lead_plane(from, t, radius)?;
    let end = centre + radius * u;
    let arc = Arc::about(centre, axis, from, end, false, radius * 1e-9)?;
    Some((end, arc))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests;
