//! The rings cut into the path, in the tree's order, each the way it cuts.
//!
//! The regions each ring bounds nest, and are cut region by region, the
//! nearest next ([`Nested`]): from the boundary in (outside in), or each
//! region's inner ones first (inside out). Measured out from the inner edges,
//! inside out is the field's own order, as outside in is from the boundary;
//! the far end's nodes are cut last. Climbing with a clockwise spindle has the
//! side not yet finished on the tool's right. Both ways keeps closed rings
//! climbing, and starts an open pass at whichever end is nearer. Each closed
//! ring starts where it crosses the line out from where the first started, so
//! the seam where each steps to the next is straight seen from above.

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
use super::rings::Plan;

/// Whether a region's children are cut before its own rings: inside out,
/// but where the field is measured out from the inner edges (`outward`), its
/// own order then.
pub(super) fn children_first(order: Order, outward: bool) -> bool {
    order == Order::InsideOut && !outward
}

/// The rings of `plans`, laid on the part as `worked`, cut into a path from
/// `first`, then the tree's roots, then `last`, each the nearest next; with
/// how many passes it has. `outward` where the field is measured out from the
/// inner edges.
pub(super) fn cut(
    seat: &Seat<'_>,
    (plans, worked): (Vec<Plan>, Vec<Worked>),
    (first, last): (Vec<usize>, Vec<usize>),
    outward: bool,
    params: &ScallopParams,
) -> Result<(Toolpath, u64), StrategyError> {
    let inner_first = children_first(params.order, outward);
    let toward_higher = !inner_first;
    let mut tree = Nested::new(inner_first);
    let mut roots = Vec::new();
    for (plan, w) in plans.into_iter().zip(worked) {
        let i = tree.push(w.runs);
        // A child nested before it is pushed: only the parent must be there.
        for c in plan.children {
            tree.nest(i, c);
        }
        if plan.root {
            roots.push(i);
        }
    }
    tree.settle(|r| r.points.first().map(|p| p.truncate()));

    let conventional = params.direction == Direction::Conventional;
    // The higher side of a line as found is on its left: the side still to
    // finish, cutting toward the higher.
    let reverse = toward_higher == (params.climb_right != conventional);
    let mut emit = Emit::new(params.feed);
    // Each ring started where it crosses the line out from where the first
    // started to the side still to finish, so the seam where each steps to
    // the next is straight seen from above.
    emit.seam_left = Some(toward_higher != reverse);
    // Where the seam runs straight down a wall, the way from one ring onto
    // the next below is a plunge further than a cut step: down along the ring
    // instead, at the linking's default ramp angle, as contour finishing goes
    // from level to level, onto a ring that starts within half the tool of
    // where the last ended.
    emit.ramp = Some((params.ramp, 0.5 * seat.tool().max_radius()));
    emit.ramp_past = Some(params.max_cut_step);
    let mut cut = Cut {
        emit,
        reverse,
        both_ways: params.direction == Direction::BothWays,
        seat,
        tolerance: params.tolerance,
    };
    for mut roots in [first, roots, last] {
        loop {
            let at = cut.emit.at;
            let Some(root) = take_nearest(&mut roots, |&r| distance(tree.anchor(r), at)) else {
                break;
            };
            tree.cut(root, &mut cut)?;
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
}

impl Cuts<Run> for Cut<'_> {
    type Error = StrategyError;

    fn at(&self) -> Option<glam::DVec2> {
        self.emit.at
    }

    /// Runs the same distance in, the nearest next, each the way it cuts.
    fn cut(&mut self, mut runs: Vec<Run>) -> Result<(), StrategyError> {
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
