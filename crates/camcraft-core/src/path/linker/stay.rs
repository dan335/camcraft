//! Links made without lifting clear: round onto the next pass, straight over
//! the part, straight beside it, straight at a level, and across and down onto
//! a lower level. Which a strategy may use is [`Ways::of`]'s to say; each way
//! is tried where it applies, checked as it is made, and refused, for the next
//! way or a lift, where it does not fit.

use std::f64::consts::{FRAC_PI_2, PI};

use glam::{DVec2, DVec3, dvec3};

use super::entry::Way;
use super::{Clearance, LinkError, Linker, Material, StayDown, Surface};
use crate::path::toolpath::{ARC_END, Arc, Move, MoveRole, PLUNGE_RATIO, steep};

/// A piece of a link: where it goes to, along an arc or straight.
type Piece = (DVec3, Option<Arc>);

/// The ways a link may be made without lifting clear, each with the longest
/// it may be, as [`StayDown`] allows them: the one place that says which ways
/// a strategy may use.
#[derive(Debug, Clone, Copy, Default)]
struct Ways {
    /// Over the part, as a finish runs on it: round onto the next pass, or
    /// straight between the cuts, over the part or beside it.
    over_part: Option<f64>,
    /// Straight and level between two cuts at one height.
    level: Option<f64>,
    /// Off the last cut, across at the height it came off and down onto the
    /// next, lower.
    across_and_down: Option<f64>,
}

impl Ways {
    /// The ways `stay_down` allows.
    fn of(stay_down: StayDown) -> Self {
        match stay_down {
            StayDown::Never => Self::default(),
            StayDown::OverPart { max } => Self {
                over_part: Some(max),
                ..Self::default()
            },
            StayDown::Level { max } => Self {
                level: Some(max),
                across_and_down: Some(max),
                ..Self::default()
            },
        }
    }
}

/// The most the cosine between the way a pass ends and the way the next sets
/// off may be for them to run opposite ways, side by side, and be turned
/// between: about 8° off.
const OPPOSITE: f64 = -0.99;

/// How many points along each piece of a turn it is looked at, close enough
/// that the tool's own width covers what lies between.
const TURN_LOOKS: u32 = 32;

/// How far a link at feed may brush what stands beside its way, as a share of
/// the tool's radius: half its radius in at most, and no more.
const BRUSH: f64 = 0.5;

impl<C: Clearance, M: Material, S: Surface> Linker<'_, C, M, S> {
    /// Make the link from `from` to `to`, where `pass` starts, without lifting
    /// clear, the first way that applies and fits, if any: over the part,
    /// round onto the next pass, otherwise straight over the part, or straight
    /// beside it where an end is off it; at a level, straight and level. No
    /// way is taken for a link longer than the ways allow.
    pub(super) fn stay_down(
        &mut self,
        from: DVec3,
        to: DVec3,
        pass: &[Move],
        feed: f64,
    ) -> Result<bool, LinkError> {
        let ways = Ways::of(self.p.stay_down);
        let span = from.truncate().distance(to.truncate());
        let too_far = |max: f64| span > max;
        let way = if let Some(max) = ways.over_part
            && !too_far(max)
        {
            if self.off_part(from) || self.off_part(to) {
                self.beside_part(from, to, pass)
            } else {
                self.turn_round(from, to, pass)
                    .or_else(|| self.over_part(from, to, pass))
            }
        } else if let Some(max) = ways.level
            && !too_far(max)
        {
            self.level(from, to, pass)
        } else {
            None
        };
        let Some(way) = way else {
            return Ok(false);
        };
        for (p, arc) in way {
            match arc {
                Some(arc) => self.out.arc_to(p, arc, MoveRole::Transition, feed)?,
                None => self.out.feed_to(p, MoveRole::Transition, feed)?,
            }
        }
        Ok(true)
    }

    /// The turn from the end of the last pass, at `from`, round onto the next
    /// at `to`, where they run opposite ways side by side, as zigzag passes
    /// do: Fusion's default transition, smooth ("smooth tangential movements
    /// using true arcs where appropriate"), as [`half_turn`] makes it.
    ///
    /// `None` where the passes do not run so ([`half_turn`]), or where the
    /// part rises into the turn, the tool would rest in it on a face it is
    /// kept clear of, the turn would cut more than a cut step into the
    /// material, or it may not be fed along ([`Linker::may_feed`]). The
    /// straight line over the part is made instead.
    fn turn_round(&self, from: DVec3, to: DVec3, pass: &[Move]) -> Option<Vec<Piece>> {
        let out = self.heading()?.truncate().try_normalize()?;
        let into = pass
            .first()?
            .tangent_at(to, 0.0)?
            .truncate()
            .try_normalize()?;
        let pieces = half_turn(from, to, out, into, self.p.tolerance, self.p.tool_radius)?;
        let way = looked_along(from, &pieces);
        // Clear of the part; and of the material, brushing what stands beside
        // it as a level link may: at the next pass's start, what it meets is
        // that pass's own cut.
        let b = to.truncate();
        let clear = way.iter().all(|p| {
            let xy = p.truncate();
            !self.surface.rests_avoided(*p)
                && self
                    .surface
                    .at(xy)
                    .is_none_or(|z| z <= p.z + self.p.tolerance)
                && (xy.distance(b) < 1e-6
                    || self
                        .material
                        .highest(xy, xy, BRUSH * self.p.tool_radius)
                        .is_none_or(|top| top <= p.z + self.p.max_cut_step))
        });
        (clear && self.may_feed(from, &way, pass)).then_some(pieces)
    }

    /// The straight line from `from` to `to`, both on the part, lifted onto
    /// the part wherever the part is above it, so it cannot gouge.
    ///
    /// `None` where the tool would rest on it on a face it is kept clear of,
    /// rubbing what the passes were cut out over, or it may not be fed along
    /// ([`Linker::may_feed`]).
    fn over_part(&self, from: DVec3, to: DVec3, pass: &[Move]) -> Option<Vec<Piece>> {
        let mut way = self.lifted_onto_part(from, to);
        way.push(to);
        if way.iter().any(|&p| self.surface.rests_avoided(p)) {
            return None;
        }
        self.may_feed(from, &way, pass)
            .then(|| way.into_iter().map(|p| (p, None)).collect())
    }

    /// The straight line from `from` to `to` where either is off the part.
    ///
    /// A pass beside a wall, below where the tool would come to rest from
    /// above, is not on the part, nor is one with nothing under the tool, as a
    /// ring round a wall's foot beyond the part: the part sampled between two
    /// such points says nothing of the way between them, which may be through
    /// the wall. That way is straight, and safe where the tool all along it, at
    /// the lower of its two ends' heights, would come to rest no higher, as a
    /// link at a level is: the tool is clear at any height above where it would
    /// rest. It may graze the wall it runs beside.
    ///
    /// `None` where the part reaches above the lower end along it, the tool
    /// would rub a face it is kept clear of, or it may not be fed along
    /// ([`Linker::may_feed`]).
    fn beside_part(&self, from: DVec3, to: DVec3, pass: &[Move]) -> Option<Vec<Piece>> {
        let low = from.z.min(to.z);
        if self.part_above(from.truncate(), to.truncate(), low)
            || self.surface.rubs_avoided(from, to)
        {
            return None;
        }
        self.straight(from, to, pass)
    }

    /// The straight level line from `from` to `to`, at the same height, as
    /// clearing at a level goes from one cut to the next.
    ///
    /// `None` between cuts at different heights, or where the part reaches up
    /// into the way, it would plough through material the cuts leave standing
    /// ([`Linker::ploughs`]), the tool would rub a face it is kept clear of,
    /// beside or over it, as the rings were cut out along it, or it may not be
    /// fed along ([`Linker::may_feed`]).
    fn level(&self, from: DVec3, to: DVec3, pass: &[Move]) -> Option<Vec<Piece>> {
        let (a, b) = (from.truncate(), to.truncate());
        if (from.z - to.z).abs() > 1e-6
            || self.part_above(a, b, to.z)
            || self.ploughs(a, b, to.z)
            || self.surface.rubs_avoided(from, to)
        {
            return None;
        }
        self.straight(from, to, pass)
    }

    /// From `off`, where the tool came off the last cut, across without
    /// lifting and down `way` onto `to` at a lower level, as Fusion goes down
    /// to a ring's next depth: across level at the height it came off, then
    /// down, where the ways allow it ([`Ways::of`]).
    ///
    /// Not where `way` is a ramp; nor where `to` is no lower than `off`, or
    /// further off than the ways allow; nor where the part reaches above
    /// `off` along the way across, the tool would rub a face it is kept clear
    /// of, or it would cut more across than a descent at cutting feed may.
    pub(super) fn across_and_down(
        &mut self,
        off: DVec3,
        way: &Way,
        to: DVec3,
        feed: f64,
    ) -> Result<bool, LinkError> {
        let Some(max) = Ways::of(self.p.stay_down).across_and_down else {
            return Ok(false);
        };
        if let Way::Ramp { .. } = way {
            return Ok(false);
        }
        // Where the way down starts, seen from above, and how low it goes
        // before any lead.
        let xy = self.start_of(way, to, Some(off.z)).truncate();
        let lowest = way.bottom(to).z;
        if off.z <= lowest + 1e-6 || off.truncate().distance(xy) > max {
            return Ok(false);
        }
        let cuts = self
            .material
            .highest(off.truncate(), xy, self.p.tool_radius)
            .is_some_and(|h| h > off.z + self.p.max_cut_step);
        let across = xy.extend(off.z);
        if self.part_above(off.truncate(), xy, off.z)
            || cuts
            || self.surface.rubs_avoided(off, across)
        {
            return Ok(false);
        }
        self.out.feed_to(across, MoveRole::Transition, feed)?;
        self.descend(way, to, feed)?;
        Ok(true)
    }

    /// The straight line from `from` to `to`, where it may be fed along.
    fn straight(&self, from: DVec3, to: DVec3, pass: &[Move]) -> Option<Vec<Piece>> {
        self.may_feed(from, &[to], pass).then(|| vec![(to, None)])
    }

    /// Whether `p` is off the part: below where the tool would come to rest
    /// there from above, or over nothing.
    fn off_part(&self, p: DVec3) -> bool {
        self.surface
            .at(p.truncate())
            .is_none_or(|z| z > p.z + self.p.tolerance)
    }

    /// Whether the part reaches above `z` anywhere along the straight line
    /// from `a` to `b`, for a tool that may graze it.
    fn part_above(&self, a: DVec2, b: DVec2, z: f64) -> bool {
        self.surface
            .grazing_along(a, b)
            .iter()
            .any(|p| p.z > z + 1e-6)
    }

    /// Where the tool rests on the part along the straight line from `from` to
    /// `to`, between them, each point no lower than the line: in order along
    /// it.
    fn lifted_onto_part(&self, from: DVec3, to: DVec3) -> Vec<DVec3> {
        let (a, b) = (from.truncate(), to.truncate());
        let span = a.distance(b);
        if span <= 1e-9 {
            return Vec::new();
        }
        let mut lifted: Vec<(f64, DVec3)> = self
            .surface
            .along(a, b)
            .into_iter()
            .filter_map(|p| {
                let t = (p.truncate() - a).dot(b - a) / (span * span);
                (t > 1e-9 && t < 1.0 - 1e-9).then(|| {
                    let chord = from.z + (to.z - from.z) * t;
                    (t, dvec3(p.x, p.y, p.z.max(chord)))
                })
            })
            .collect();
        lifted.sort_by(|x, y| x.0.total_cmp(&y.0));
        lifted.into_iter().map(|(_, p)| p).collect()
    }

    /// Whether a link at level `z` from `a` to `b` would plough through material
    /// the cuts either end of it leave standing: as Fusion's, a link stays down
    /// only where it does not violate the work. Its first and last tool's radius
    /// are the cuts' own, coming off one and into the next as they engage; between,
    /// the tool may brush what stands beside its way ([`BRUSH`]), and no more.
    fn ploughs(&self, a: DVec2, b: DVec2, z: f64) -> bool {
        let r = self.p.tool_radius;
        let span = a.distance(b);
        if span <= 2.0 * r {
            return false;
        }
        let d = (b - a) / span;
        self.material
            .highest(a + d * r, b - d * r, BRUSH * r)
            .is_some_and(|top| top > z + self.p.tolerance)
    }

    /// Whether the tool may feed from `from` through `way` onto `pass` without
    /// lifting: falling steeply no further than a descent at cutting feed may,
    /// with the fall the last cut was making as it ended ([`falls_steeply`]),
    /// and with the rest of the tool clear of the material
    /// ([`Clearance::keeps_clear`]).
    fn may_feed(&self, from: DVec3, way: &[DVec3], pass: &[Move]) -> bool {
        let fallen = trailing_fall(self.out.moves());
        !falls_steeply(fallen, from, way, pass, self.p.max_cut_step)
            && self.clearance.keeps_clear(from, way)
    }
}

/// The half turn from `from`, where a pass heading `out` ends, round onto
/// `to`, where the next sets off `into`, seen from above: as wide as the
/// passes are apart, out past their ends the way the last was heading,
/// tangent to both; where one pass ends short of the other, straight on along
/// it first, or after, so the turn is at the further end. Level as seen from
/// above, it goes from one pass's height to the other's as it goes. Its
/// pieces.
///
/// `None` where the passes do not run opposite ways ([`OPPOSITE`]); where
/// they are in line, no further apart than the `tolerance`, as a spoke that
/// stops short of the middle and the next one out from it are; or where they
/// are further apart than the tool, of `tool_radius`, is wide, as a turn that
/// size would swing far off the work.
fn half_turn(
    from: DVec3,
    to: DVec3,
    out: DVec2,
    into: DVec2,
    tolerance: f64,
    tool_radius: f64,
) -> Option<Vec<Piece>> {
    if out.dot(into) > OPPOSITE {
        return None;
    }
    let (a, b) = (from.truncate(), to.truncate());
    // How far the next pass starts along the way the last was heading,
    // and the turn's width: the gap square to the passes.
    let along = (b - a).dot(out);
    let width = (b - a - out * along).length();
    // In line, one behind the other, they are not side by side: a turn
    // of less radius than the tolerance is the straight line, and one of
    // less than an arc may have is no arc.
    let least = 2.0 * tolerance.max(ARC_END);
    if width <= least || width > 2.0 * tool_radius {
        return None;
    }
    // The straight on, before the turn or after it, and the turn's ends.
    let (turn_from, turn_to) = if along >= 0.0 {
        (a + out * along, b)
    } else {
        (a, b + out * -along)
    };
    let straight = along.abs();
    let length = straight + FRAC_PI_2 * width;
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
    Some(pieces)
}

/// Where a link from `from` along `pieces` is looked at: [`TURN_LOOKS`]
/// points along each piece, its start left out.
fn looked_along(from: DVec3, pieces: &[Piece]) -> Vec<DVec3> {
    let mut way = Vec::new();
    let mut at = from;
    for &(end, arc) in pieces {
        way.extend((1..=TURN_LOOKS).map(|k| {
            let t = f64::from(k) / f64::from(TURN_LOOKS);
            arc.map_or_else(|| at.lerp(end, t), |c| c.point_at(at, end, t))
        }));
        at = end;
    }
    way
}

/// How far `moves` have been falling steeply at the cutting feed at their end:
/// a cut coming down a wall, which a link going on down would add to.
fn trailing_fall(moves: &[Move]) -> f64 {
    moves
        .windows(2)
        .rev()
        .map_while(|w| match w {
            [a, m] if m.falls_steeply(a.to, PLUNGE_RATIO) => Some(a.to.z - m.to.z),
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
        if depth <= 0.0 || !m.falls_steeply(at, PLUNGE_RATIO) {
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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests;
