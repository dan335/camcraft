//! Reordering passes to cut down travel.
//!
//! A `Toolpath -> Toolpath` transform, and the stage the pipeline was shaped
//! for. Strategies emit passes in whatever order suits their own geometry — a raster
//! sweeps line by line, roughing works ring by ring — and that order is rarely the
//! cheapest to travel. Pngcam names this as its own weakness: *"it still spends more
//! time on travel moves than it could do if it ordered the cutting segments more
//! intelligently."*
//!
//! # What may and may not be reordered
//!
//! Only whole passes move, never the moves inside one, and, for an operation cut
//! in levels, never across a change of level. Reordering within a pass would
//! change the cut; reordering across levels would let the tool return to a level
//! it has already left, which is a different operation wearing the same name. A
//! surface finished in one go, as parallel's, has no levels: each of its passes
//! starts at its own height on a curved part, and all are one group.
//!
//! # Greedy, and deliberately so
//!
//! Nearest-neighbour from the current position, with each pass considered in both
//! directions where reversing it is safe. This is the travelling-salesman problem,
//! so optimal is out of reach; greedy gets most of the benefit for a fraction of the
//! complexity, and — the part that matters here — it is deterministic. Ties break on
//! the original index, so the same input always yields the same program.

use glam::DVec3;

use crate::path::toolpath::{Move, MoveRole, Toolpath, ToolpathError};

/// What reordering achieved.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct OrderReport {
    /// Passes found.
    pub passes: u64,
    /// Passes emitted reversed.
    pub reversed: u64,
    /// Straight-line travel between passes before reordering, in millimetres.
    pub travel_before_mm: f64,
    /// And after.
    pub travel_after_mm: f64,
}

impl OrderReport {
    /// Fraction of inter-pass travel removed, in `0..=1`.
    pub fn saved(&self) -> f64 {
        if self.travel_before_mm <= 0.0 {
            return 0.0;
        }
        ((self.travel_before_mm - self.travel_after_mm) / self.travel_before_mm).clamp(0.0, 1.0)
    }
}

/// One contiguous run of cutting, as the strategy emitted it.
#[derive(Debug, Clone)]
struct Pass {
    moves: Vec<Move>,
    closed: bool,
}

impl Pass {
    fn start(&self) -> Option<DVec3> {
        self.moves.first().map(|m| m.to)
    }

    fn end(&self) -> Option<DVec3> {
        self.moves.last().map(|m| m.to)
    }

    fn depth(&self) -> f64 {
        self.moves.first().map_or(0.0, |m| m.to.z)
    }

    /// Whether it may be run backwards: an open pass of more than one move
    /// that does not ramp. Running a pass backwards flips its cutting
    /// direction, climb for conventional, so it is done only where its
    /// operation cuts both ways or it is asked; a closed ring ends where it
    /// starts, so is never reversed, as that would bring it no nearer and only
    /// turn it the other way. Nor is a ring ramped down onto the ring below
    /// it, open only for ending a level lower: backwards it would climb the
    /// ramp and cut both rings the other way.
    fn reversible(&self) -> bool {
        !self.closed
            && self.moves.len() > 1
            && !self.moves.iter().any(|m| m.role == MoveRole::Transition)
    }

    fn reversed(&self) -> Self {
        // The pass's first move is its link, to where it starts; walked backwards
        // the link goes to where it ended, and each move after runs back over the
        // one it was, arcs turning the other way.
        let mut moves = Vec::with_capacity(self.moves.len());
        if let (Some(link), Some(end)) = (self.moves.first(), self.end()) {
            moves.push(Move { to: end, ..*link });
        }
        for pair in self.moves.windows(2).rev() {
            if let [before, m] = pair {
                moves.push(m.reversed(before.to));
            }
        }
        Self {
            moves,
            closed: self.closed,
        }
    }
}

/// Reorder the passes of a path to shorten the travel between them, `by_level`
/// for an operation cut in levels: those at one depth, in their order. With
/// `allow_reverse`, an open pass may be taken from its end where that is
/// nearer: for an operation that cuts both ways, or is asked to
/// ([`Operation::reverses`](crate::job::Operation::reverses)).
///
/// The input must still be unlinked — passes are identified by their leading
/// [`MoveRole::Link`]. Reordering after linking would move retracts and plunges
/// around with no way to know what they were for.
pub fn order_passes(
    path: &Toolpath,
    allow_reverse: bool,
    by_level: bool,
) -> Result<(Toolpath, OrderReport), ToolpathError> {
    let passes = split_passes(path);
    let mut report = OrderReport {
        passes: passes.len() as u64,
        ..Default::default()
    };
    if passes.len() < 2 {
        return Ok((path.clone(), report));
    }
    report.travel_before_mm = travel(&passes);

    // Group by depth and keep the groups in their original order. A pass must never
    // migrate to a different level.
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut current_depth = f64::NAN;
    for (i, p) in passes.iter().enumerate() {
        let d = p.depth();
        if groups.is_empty() || (by_level && (d - current_depth).abs() > 1.0e-9) {
            groups.push(Vec::new());
            current_depth = d;
        }
        if let Some(g) = groups.last_mut() {
            g.push(i);
        }
    }

    let mut out = Toolpath::new();
    let mut at: Option<DVec3> = None;
    let mut ordered: Vec<Pass> = Vec::with_capacity(passes.len());

    for group in &groups {
        let mut remaining: Vec<usize> = group.clone();
        while !remaining.is_empty() {
            // Nearest start, considering a reversed entry where that is allowed.
            // Ties break on the original index, which is what keeps this
            // deterministic.
            let mut best = (f64::MAX, 0usize, false);
            for (slot, &idx) in remaining.iter().enumerate() {
                let Some(p) = passes.get(idx) else { continue };
                let (Some(s), Some(e)) = (p.start(), p.end()) else {
                    continue;
                };
                let d_fwd = at.map_or(0.0, |a| (s - a).length());
                if d_fwd < best.0 {
                    best = (d_fwd, slot, false);
                }
                if allow_reverse && p.reversible() {
                    let d_rev = at.map_or(0.0, |a| (e - a).length());
                    if d_rev < best.0 {
                        best = (d_rev, slot, true);
                    }
                }
            }
            let (_, slot, reverse) = best;
            let Some(idx) = remaining.get(slot).copied() else {
                break;
            };
            remaining.remove(slot);
            let Some(p) = passes.get(idx) else { continue };
            let chosen = if reverse {
                report.reversed += 1;
                p.reversed()
            } else {
                p.clone()
            };
            at = chosen.end();
            ordered.push(chosen);
        }
    }

    for p in &ordered {
        out.append(&p.moves)?;
    }
    report.travel_after_mm = travel(&ordered);
    Ok((out, report))
}

/// The passes of a path in the order they were laid, each open one taken from
/// whichever end is nearer where the last left the tool: for an operation that
/// cuts both ways and keeps its own order, as a field's rings are, so the
/// pieces a ring is cut into after it is laid, by a height or a face avoided,
/// go back and forth as cut pieces should, and none is moved. As
/// [`order_passes`], the input must still be unlinked.
pub fn nearer_ends(path: &Toolpath) -> Result<(Toolpath, OrderReport), ToolpathError> {
    let passes = split_passes(path);
    let mut report = OrderReport {
        passes: passes.len() as u64,
        travel_before_mm: travel(&passes),
        ..Default::default()
    };
    let mut at: Option<DVec3> = None;
    let mut ordered = Vec::with_capacity(passes.len());
    for p in passes {
        let nearer_end = match (at, p.start(), p.end()) {
            (Some(a), Some(s), Some(e)) => p.reversible() && (e - a).length() < (s - a).length(),
            _ => false,
        };
        let chosen = if nearer_end {
            report.reversed += 1;
            p.reversed()
        } else {
            p
        };
        at = chosen.end().or(at);
        ordered.push(chosen);
    }
    let mut out = Toolpath::new();
    for p in &ordered {
        out.append(&p.moves)?;
    }
    report.travel_after_mm = travel(&ordered);
    Ok((out, report))
}

/// Split a path into passes at each unresolved link, what comes before the
/// first link its own.
fn split_passes(path: &Toolpath) -> Vec<Pass> {
    path.passes()
        .map(|moves| {
            let closed = match (moves.first(), moves.last()) {
                (Some(s), Some(e)) => (s.to - e.to).length() < 1.0e-9 && moves.len() > 2,
                _ => false,
            };
            Pass {
                moves: moves.to_vec(),
                closed,
            }
        })
        .collect()
}

/// Straight-line distance between the end of each pass and the start of the next.
fn travel(passes: &[Pass]) -> f64 {
    let mut total = 0.0;
    let mut at: Option<DVec3> = None;
    for p in passes {
        if let (Some(a), Some(s)) = (at, p.start()) {
            total += (s - a).length();
        }
        at = p.end().or(at);
    }
    total
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests {
    use super::*;
    use crate::path::toolpath::MoveRole;
    use glam::dvec3;

    /// Four short passes deliberately emitted in the worst order: each one starts
    /// far from where the last ended.
    fn scattered() -> Toolpath {
        let mut t = Toolpath::new();
        for (x, y) in [(0.0, 0.0), (50.0, 0.0), (1.0, 0.0), (51.0, 0.0)] {
            t.feed_to(dvec3(x, y, -1.0), MoveRole::Link, 900.0).unwrap();
            t.feed_to(dvec3(x, y + 5.0, -1.0), MoveRole::Cut, 900.0)
                .unwrap();
        }
        t
    }

    #[test]
    fn a_single_pass_is_left_alone() {
        let mut t = Toolpath::new();
        t.feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Link, 900.0)
            .unwrap();
        t.feed_to(dvec3(5.0, 0.0, 0.0), MoveRole::Cut, 900.0)
            .unwrap();
        let (out, report) = order_passes(&t, true, true).unwrap();
        assert_eq!(out, t);
        assert_eq!(report.passes, 1);
    }

    /// The point of the transform.
    #[test]
    fn reordering_shortens_the_travel() {
        let (out, report) = order_passes(&scattered(), false, true).unwrap();
        assert_eq!(report.passes, 4);
        assert!(
            report.travel_after_mm < report.travel_before_mm,
            "travel should fall: {:?}",
            report
        );
        assert!(
            report.saved() > 0.4,
            "expected a real saving, got {}",
            report.saved()
        );
        assert_eq!(out.len(), scattered().len(), "no moves may be lost");
    }

    /// Cutting geometry is not the ordering's business. Every point must survive.
    #[test]
    fn no_cutting_point_is_lost_or_moved() {
        let input = scattered();
        let (out, _) = order_passes(&input, true, true).unwrap();

        let mut before: Vec<[u64; 3]> = input
            .moves()
            .iter()
            .map(|m| [m.to.x.to_bits(), m.to.y.to_bits(), m.to.z.to_bits()])
            .collect();
        let mut after: Vec<[u64; 3]> = out
            .moves()
            .iter()
            .map(|m| [m.to.x.to_bits(), m.to.y.to_bits(), m.to.z.to_bits()])
            .collect();
        before.sort_unstable();
        after.sort_unstable();
        assert_eq!(before, after, "the set of points must be unchanged");
    }

    /// A pass must never migrate to a different level: that would be a different
    /// operation wearing the same name.
    #[test]
    fn passes_never_cross_depths() {
        let mut t = Toolpath::new();
        // Two levels, each with a near and a far pass, emitted worst-first.
        for (z, xs) in [(-1.0, [0.0, 90.0]), (-2.0, [0.0, 90.0])] {
            for x in xs {
                t.feed_to(dvec3(x, 0.0, z), MoveRole::Link, 900.0).unwrap();
                t.feed_to(dvec3(x, 5.0, z), MoveRole::Cut, 900.0).unwrap();
            }
        }
        let (out, _) = order_passes(&t, true, true).unwrap();

        // Depths must appear in runs, never interleaved.
        let mut seen: Vec<f64> = Vec::new();
        for m in out.moves() {
            if seen
                .last()
                .map(|z| (z - m.to.z).abs() > 1e-9)
                .unwrap_or(true)
            {
                seen.push(m.to.z);
            }
        }
        let mut sorted = seen.clone();
        sorted.dedup();
        assert_eq!(seen.len(), sorted.len(), "a depth was revisited: {seen:?}");
    }

    /// A surface finished in one go has no levels: on a curved part each pass
    /// starts at its own height, and all are reordered together.
    #[test]
    fn passes_of_one_surface_are_reordered_together() {
        let mut t = Toolpath::new();
        // Far and near passes each starting a little lower than the last.
        for (x, z) in [(0.0, 0.0), (90.0, -0.1), (5.0, -0.2), (85.0, -0.3)] {
            t.feed_to(dvec3(x, 0.0, z), MoveRole::Link, 900.0).unwrap();
            t.feed_to(dvec3(x, 5.0, z - 0.05), MoveRole::Cut, 900.0)
                .unwrap();
        }
        let (_, levels) = order_passes(&t, false, true).unwrap();
        assert_eq!(levels.saved(), 0.0, "every pass is a level of its own");
        let (_, one) = order_passes(&t, false, false).unwrap();
        assert!(one.saved() > 0.5, "{one:?}");
    }

    /// Reversing an open pass flips climb for conventional, so it only happens when
    /// asked for.
    #[test]
    fn reversal_is_opt_in() {
        let far = {
            let mut t = Toolpath::new();
            t.feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Link, 900.0)
                .unwrap();
            t.feed_to(dvec3(1.0, 0.0, 0.0), MoveRole::Cut, 900.0)
                .unwrap();
            // The next pass ends near the first, but starts far away.
            t.feed_to(dvec3(40.0, 0.0, 0.0), MoveRole::Link, 900.0)
                .unwrap();
            t.feed_to(dvec3(2.0, 0.0, 0.0), MoveRole::Cut, 900.0)
                .unwrap();
            t
        };
        let off = order_passes(&far, false, true).unwrap().1;
        let on = order_passes(&far, true, true).unwrap().1;
        assert_eq!(off.reversed, 0, "reversal must not happen unasked");
        assert!(on.reversed > 0, "reversal should help here");
        assert!(on.travel_after_mm < off.travel_after_mm);
    }

    /// What kept reordering off for scallop and the patterns: rings on a
    /// flat all start level, so they are one group, and reordering them must
    /// not shuffle them or turn them round. Laid one inside the next, each
    /// starting nearest where the last ended, as scallop lays them, they keep
    /// their order and their way round, reversal allowed or not: the next
    /// ring in is always the nearest, and a closed ring is never reversed.
    #[test]
    fn rings_laid_one_inside_the_next_keep_their_order_and_way() {
        let mut t = Toolpath::new();
        for k in 0..6 {
            let (lo, hi) = (f64::from(k), 20.0 - f64::from(k));
            t.feed_to(dvec3(lo, lo, 0.0), MoveRole::Link, 900.0)
                .unwrap();
            for (x, y) in [(hi, lo), (hi, hi), (lo, hi), (lo, lo)] {
                t.feed_to(dvec3(x, y, 0.0), MoveRole::Cut, 900.0).unwrap();
            }
        }
        for reverse in [false, true] {
            let (out, report) = order_passes(&t, reverse, false).unwrap();
            assert_eq!(out, t, "reversal allowed: {reverse}");
            assert_eq!(report.reversed, 0);
        }
    }

    /// Open pieces of rings, each running the same way round, laid ring by
    /// ring: allowed to, each is taken from whichever end is nearer, so the
    /// tool goes back and forth across them rather than back to the start of
    /// each; not allowed to, each is cut as it was laid.
    #[test]
    fn pieces_are_turned_round_only_where_allowed() {
        let mut t = Toolpath::new();
        for k in 0..4 {
            let y = f64::from(k);
            t.feed_to(dvec3(0.0, y, 0.0), MoveRole::Link, 900.0)
                .unwrap();
            t.feed_to(dvec3(10.0, y, 0.0), MoveRole::Cut, 900.0)
                .unwrap();
        }
        let (kept, one_way) = order_passes(&t, false, false).unwrap();
        assert_eq!(kept, t);
        assert_eq!(one_way.reversed, 0);
        let (out, both) = order_passes(&t, true, false).unwrap();
        assert_eq!(both.reversed, 2, "{out:?}");
        assert!(both.travel_after_mm < one_way.travel_after_mm);
        // Every other one turned round, starting where the last ended.
        let starts: Vec<f64> = out.passes().map(|p| p.first().unwrap().to.x).collect();
        assert_eq!(starts, [0.0, 10.0, 0.0, 10.0]);
    }

    /// Same input, same program. Ties break on the original index precisely so this
    /// holds.
    #[test]
    fn ordering_is_deterministic() {
        let input = scattered();
        let first = order_passes(&input, true, true).unwrap().0;
        for _ in 0..5 {
            assert_eq!(order_passes(&input, true, true).unwrap().0, first);
        }
    }

    /// Kept in the order laid, each open pass is taken from its nearer end:
    /// the pieces go back and forth, and none moves, though the nearest next
    /// would take another first.
    #[test]
    fn nearer_ends_keep_the_order_laid() {
        let input = scattered();
        let (out, report) = nearer_ends(&input).unwrap();
        let starts: Vec<DVec3> = split_passes(&out).iter().filter_map(Pass::start).collect();
        assert_eq!(
            starts,
            vec![
                dvec3(0.0, 0.0, -1.0),
                dvec3(50.0, 5.0, -1.0),
                dvec3(1.0, 0.0, -1.0),
                dvec3(51.0, 5.0, -1.0),
            ]
        );
        assert_eq!(report.reversed, 2);
    }

    /// A ring ramped down onto the ring below it ends a level lower, not
    /// where it started, but is no piece to take from either end: run
    /// backwards it would climb the ramp, and cut both rings the other way.
    #[test]
    fn a_ring_ramped_onto_the_next_is_not_turned_round() {
        let mut t = Toolpath::new();
        t.feed_to(dvec3(0.0, 0.0, -1.0), MoveRole::Link, 900.0)
            .unwrap();
        t.feed_to(dvec3(5.0, 0.0, -1.0), MoveRole::Cut, 900.0)
            .unwrap();
        t.feed_to(dvec3(5.0, 5.0, -1.0), MoveRole::Cut, 900.0)
            .unwrap();
        t.feed_to(dvec3(0.0, 0.0, -1.0), MoveRole::Cut, 900.0)
            .unwrap();
        t.feed_to(dvec3(5.0, 0.0, -2.0), MoveRole::Transition, 900.0)
            .unwrap();
        t.feed_to(dvec3(5.0, 5.0, -2.0), MoveRole::Cut, 900.0)
            .unwrap();
        t.feed_to(dvec3(0.0, 0.0, -2.0), MoveRole::Cut, 900.0)
            .unwrap();
        t.feed_to(dvec3(5.0, 0.0, -2.0), MoveRole::Cut, 900.0)
            .unwrap();
        // Started from beside where it ends.
        let mut from = Toolpath::new();
        from.feed_to(dvec3(6.0, 0.0, -2.0), MoveRole::Link, 900.0)
            .unwrap();
        from.feed_to(dvec3(6.0, 1.0, -2.0), MoveRole::Cut, 900.0)
            .unwrap();
        from.append(t.moves()).unwrap();
        let (out, report) = nearer_ends(&from).unwrap();
        assert_eq!(report.reversed, 0);
        assert_eq!(out, from);
    }
    #[test]
    fn an_empty_path_is_handled() {
        let (out, report) = order_passes(&Toolpath::new(), true, true).unwrap();
        assert!(out.is_empty());
        assert_eq!(report.passes, 0);
        assert_eq!(report.saved(), 0.0);
    }
}
