//! The rings cut into the path, in the tree's order, each the way it cuts.
//!
//! # The rule
//!
//! The regions each ring bounds nest, and are cut region by region, the
//! nearest next ([`Nested`]). Each line lies on one side of its region
//! ([`Side`]): round its outside, the region inside it, or round a hole of
//! it, the region outside it. A line is cut before its region's children
//! where the cut goes on into its region ([`before_children`]): from the
//! boundary in (outside in), the rings round a region's outside, then what
//! is inside them, then the rings round its holes; from the middle out
//! (inside out), the other way, so the two families are not cut by turns
//! (decision 0041). A field measured out from inner edges grows every ring
//! out from a hole, so every line of it is a hole's, and inside out is its
//! own order. Each line is cut toward the side still to finish, its field's
//! higher side where its field's lines are cut before their children.
//!
//! Rings that are no region's, cut one after another (the band into a far
//! edge, the rings below where the tool leaves the part, the rings round a
//! spine), are one node's sequence ([`Pass::Sequence`]), listed from the
//! boundary in, cut in that order from the boundary in and the other way
//! from the middle out.
//!
//! Climbing with a clockwise spindle has the side not yet finished on the
//! tool's right. Both ways keeps closed rings climbing, and starts an open
//! pass at whichever end is nearer. Each closed ring starts where it crosses
//! the line out from where the first started, so the seam where each steps
//! to the next is straight seen from above. Where strips break the ground
//! into zones (decision 0039), each zone's tree is cut whole before the
//! next's, on the one seam.

use glam::DVec3;

use crate::cutter::surfaces::Seat;
use crate::cutter::tool::ToolProfile;
use crate::path::toolpath::Toolpath;
use crate::strategy::StrategyError;
use crate::strategy::adaptive::Direction;
use crate::strategy::nesting::{Cuts, Nested, Order, distance, take_nearest};
use crate::strategy::parallel::Run;
use crate::strategy::projected::Emit;

use super::ScallopParams;
use super::lay::Worked;
use super::rings::Planned;

/// Which side of a line its region lies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Side {
    /// Round the region's outside: the region inside it.
    Outer,
    /// Round a hole of the region: the region outside it.
    Hole,
}

/// Whether a line on `side` of its region is cut before the region's
/// children, cut in `order`: where the cut goes on into its region.
pub(super) fn before_children(order: Order, side: Side) -> bool {
    (order == Order::OutsideIn) == (side == Side::Outer)
}

/// What a node of the tree cuts: a pass, taken with the rest of its slot the
/// nearest next, or a sequence of groups, each taken so in turn.
enum Pass {
    Run(Run),
    Sequence(Vec<Vec<Run>>),
}

impl Pass {
    /// Where it starts, seen from above.
    fn start(&self) -> Option<glam::DVec2> {
        match self {
            Self::Run(run) => run.points.first(),
            Self::Sequence(groups) => groups.iter().flatten().next()?.points.first(),
        }
        .map(|p| p.truncate())
    }
}

/// The rings of each zone's tree, laid on the part as `worked`, cut into a
/// path zone by zone, each from its `first`, then its roots, then its
/// `last`, each the nearest next; with how many passes it has. One seam runs
/// through them all, from where the first ring starts: nearest where the tool
/// is, `from`, where it is anywhere, as where the level rings below the
/// field end, cut before it from the boundary in.
pub(super) fn cut(
    seat: &Seat<'_>,
    zones: Vec<(Planned, Vec<Worked>)>,
    params: &ScallopParams,
    from: Option<DVec3>,
) -> Result<(Toolpath, u64), StrategyError> {
    let conventional = params.direction == Direction::Conventional;
    let mut emit = Emit::new(params.feed);
    // Where the seam runs straight down a wall, the way from one ring onto
    // the next below is a plunge further than a cut step: down along the ring
    // instead, at the linking's default ramp angle, as contour finishing goes
    // from level to level, onto a ring that starts within half the tool of
    // where the last ended.
    emit.ramp = Some((params.ramp, 0.5 * seat.tool().max_radius()));
    emit.ramp_past = Some(params.max_cut_step);
    if let Some(at) = from {
        emit.after(at);
    }
    let mut cut = Cut {
        emit,
        reverse: false,
        both_ways: params.direction == Direction::BothWays,
        seat,
        tolerance: params.tolerance,
    };
    let inside_out = params.order == Order::InsideOut;
    for (planned, worked) in zones {
        let toward_higher = before_children(params.order, planned.side);
        let mut tree = Nested::new();
        let mut roots = Vec::new();
        for (plan, w) in planned.plans.into_iter().zip(worked) {
            let runs = |runs: Vec<Run>| runs.into_iter().map(Pass::Run).collect::<Vec<_>>();
            // A sequence sits where a line round the region's outside does.
            let mut outer = runs(w.outer);
            if !w.sequence.is_empty() {
                let mut sequence = w.sequence;
                if inside_out {
                    sequence.reverse();
                }
                outer.push(Pass::Sequence(sequence));
            }
            let holes = runs(w.holes);
            let i = tree.push(if before_children(params.order, Side::Outer) {
                (outer, holes)
            } else {
                (holes, outer)
            });
            // A child nested before it is pushed: only the parent must be there.
            for c in plan.children {
                tree.nest(i, c);
            }
            if plan.root {
                roots.push(i);
            }
        }
        tree.settle(Pass::start);
        // The higher side of a line as found is on its left: the side still
        // to finish, cutting toward the higher.
        cut.reverse = toward_higher == (params.climb_right != conventional);
        // Each ring started where it crosses the line out from where the
        // first started to the side still to finish, so the seam where each
        // steps to the next is straight seen from above.
        cut.emit.seam_left = Some(toward_higher != cut.reverse);
        for mut roots in [planned.first, roots, planned.last] {
            loop {
                let at = cut.emit.at;
                let Some(root) = take_nearest(&mut roots, |&r| distance(tree.anchor(r), at)) else {
                    break;
                };
                tree.cut(root, &mut cut)?;
            }
        }
    }
    Ok((cut.emit.path, cut.emit.passes))
}

/// Cutting the rings into the path, in order, from where the tool is.
struct Cut<'s> {
    emit: Emit,
    /// Whether a line as found is reversed to cut the way asked for.
    reverse: bool,
    /// Whether an open pass may be started at either end.
    both_ways: bool,
    /// The part, to put a point of each ring where it crosses the seam.
    seat: &'s Seat<'s>,
    tolerance: f64,
}

impl Cut<'_> {
    /// `ring`, closed, with a point of its own where it crosses the seam
    /// ([`Seam::crossing`](crate::strategy::projected::Seam::crossing)), so
    /// it starts on it: on the chord there, raised onto the part where the
    /// chord dips into it, as it may by the tolerance. Where the tool there
    /// would be below where it rests, beside a wall the chord bows into, it
    /// starts at the nearer of the chord's ends instead.
    fn on_the_seam(&self, ring: &mut Run) {
        let (Some(seam), Some(at)) = (self.emit.seam(), self.emit.at) else {
            return;
        };
        let Some((i, t)) = seam.crossing(ring, at) else {
            return;
        };
        let n = ring.points.len();
        let (Some(&a), Some(&b)) = (ring.points.get(i), ring.points.get((i + 1) % n)) else {
            return;
        };
        let q = a.lerp(b, t);
        if q.distance(a) < 1e-6 || q.distance(b) < 1e-6 {
            return;
        }
        match self.seat.drop_at(q.x, q.y) {
            Some(rest) if rest > q.z + self.tolerance => {}
            rest => ring
                .points
                .insert(i + 1, q.truncate().extend(rest.map_or(q.z, |z| z.max(q.z)))),
        }
    }

    /// `runs`, the nearest next, each the way it cuts.
    fn runs(&mut self, mut runs: Vec<Run>) -> Result<(), StrategyError> {
        for run in &mut runs {
            if self.reverse {
                run.points.reverse();
            }
            if run.closed {
                self.on_the_seam(run);
            }
        }
        self.emit.nearest_first(runs, self.both_ways)
    }
}

impl Cuts<Pass> for Cut<'_> {
    type Error = StrategyError;

    fn at(&self) -> Option<glam::DVec2> {
        self.emit.at
    }

    /// A slot's runs, the nearest next; then each sequence, group by group.
    fn cut(&mut self, passes: Vec<Pass>) -> Result<(), StrategyError> {
        let mut runs = Vec::new();
        let mut sequences = Vec::new();
        for pass in passes {
            match pass {
                Pass::Run(run) => runs.push(run),
                Pass::Sequence(groups) => sequences.push(groups),
            }
        }
        if !runs.is_empty() {
            self.runs(runs)?;
        }
        for group in sequences.into_iter().flatten() {
            if !group.is_empty() {
                self.runs(group)?;
            }
        }
        Ok(())
    }
}
