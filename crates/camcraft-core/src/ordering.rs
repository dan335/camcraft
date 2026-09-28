//! Reordering passes to cut down travel.
//!
//! The first `Toolpath -> Toolpath` transform, and the stage the pipeline was shaped
//! for. Strategies emit passes in whatever order suits their own geometry — a raster
//! sweeps line by line, roughing works ring by ring — and that order is rarely the
//! cheapest to travel. Pngcam names this as its own weakness: *"it still spends more
//! time on travel moves than it could do if it ordered the cutting segments more
//! intelligently."*
//!
//! # What may and may not be reordered
//!
//! Only whole passes move, never the moves inside one, and never across a change of
//! depth. Reordering within a pass would change the cut; reordering across depths
//! would let the tool return to a level it has already left, which is a different
//! operation wearing the same name.
//!
//! # Greedy, and deliberately so
//!
//! Nearest-neighbour from the current position, with each pass considered in both
//! directions where reversing it is safe. This is the travelling-salesman problem,
//! so optimal is out of reach; greedy gets most of the benefit for a fraction of the
//! complexity, and — the part that matters here — it is deterministic. Ties break on
//! the original index, so the same input always yields the same program.

use glam::DVec3;

use crate::toolpath::{Move, MoveRole, Toolpath, ToolpathError};

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

    /// A closed ring can be entered anywhere, but reversing an open pass flips the
    /// cutting direction — climb becomes conventional — so only closed rings and
    /// same-depth open passes are reversed, and only when asked.
    fn reversible(&self) -> bool {
        !self.closed && self.moves.len() > 1
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

/// Reorder the passes of a path to shorten the travel between them.
///
/// The input must still be unlinked — passes are identified by their leading
/// [`MoveRole::Link`]. Reordering after linking would move retracts and plunges
/// around with no way to know what they were for.
pub fn order_passes(
    path: &Toolpath,
    allow_reverse: bool,
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
        if groups.is_empty() || (d - current_depth).abs() > 1.0e-9 {
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
        for m in &p.moves {
            out.push(*m)?;
        }
    }
    report.travel_after_mm = travel(&ordered);
    Ok((out, report))
}

/// Split a path into passes at each unresolved link.
fn split_passes(path: &Toolpath) -> Vec<Pass> {
    let mut passes: Vec<Pass> = Vec::new();
    for m in path.moves() {
        if m.role == MoveRole::Link || passes.is_empty() {
            passes.push(Pass {
                moves: Vec::new(),
                closed: false,
            });
        }
        if let Some(p) = passes.last_mut() {
            p.moves.push(*m);
        }
    }
    for p in &mut passes {
        let (Some(s), Some(e)) = (p.start(), p.end()) else {
            continue;
        };
        p.closed = (s - e).length() < 1.0e-9 && p.moves.len() > 2;
    }
    passes
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
        let (out, report) = order_passes(&t, true).unwrap();
        assert_eq!(out, t);
        assert_eq!(report.passes, 1);
    }

    /// The point of the transform.
    #[test]
    fn reordering_shortens_the_travel() {
        let (out, report) = order_passes(&scattered(), false).unwrap();
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
        let (out, _) = order_passes(&input, true).unwrap();

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
        let (out, _) = order_passes(&t, true).unwrap();

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
        let off = order_passes(&far, false).unwrap().1;
        let on = order_passes(&far, true).unwrap().1;
        assert_eq!(off.reversed, 0, "reversal must not happen unasked");
        assert!(on.reversed > 0, "reversal should help here");
        assert!(on.travel_after_mm < off.travel_after_mm);
    }

    /// Same input, same program. Ties break on the original index precisely so this
    /// holds.
    #[test]
    fn ordering_is_deterministic() {
        let input = scattered();
        let first = order_passes(&input, true).unwrap().0;
        for _ in 0..5 {
            assert_eq!(order_passes(&input, true).unwrap().0, first);
        }
    }

    #[test]
    fn an_empty_path_is_handled() {
        let (out, report) = order_passes(&Toolpath::new(), true).unwrap();
        assert!(out.is_empty());
        assert_eq!(report.passes, 0);
        assert_eq!(report.saved(), 0.0);
    }
}
