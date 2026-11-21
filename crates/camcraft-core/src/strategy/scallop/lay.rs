//! Each node's lines laid on the part: kept to the slope range and the area,
//! the field's lines and the ground's edges placed by the field, height and
//! all ([`guided`]), the area's edge dropped onto the part seen from above;
//! and the rings below where the tool leaves the part, found beside the wall
//! already, as they are. Each line is laid on its [`Side`] of its region, which
//! decides when it is cut ([`order`](super::order)).

use camcraft_geom::contour::{Region, polyline_length, simplify};
use camcraft_geom::trigrid::TriSurface;
use glam::DVec2;

use crate::cutter::surfaces::Seat;
use crate::cutter::tool::ToolProfile;
use crate::strategy::StrategyError;
use crate::strategy::parallel::{ParallelParams, ParallelReport, Run, sweep_polyline};
use crate::strategy::slope::{SlopeCounts, Sloped};

use super::distance::at;
use super::ground::Ground;
use super::guided;
use super::order::Side;
use super::rings::Plan;
use super::{length, turns_left};

/// A node's lines, laid on the part, and what was left out.
#[derive(Default)]
pub(super) struct Worked {
    /// Its lines on [`Side::Outer`] of its region.
    pub(super) outer: Vec<Run>,
    /// Its lines on [`Side::Hole`].
    pub(super) holes: Vec<Run>,
    /// Its sequence's rings, each one's runs, listed from the boundary in.
    pub(super) sequence: Vec<Vec<Run>>,
    pub(super) sweep: ParallelReport,
    pub(super) slopes: SlopeCounts,
    pub(super) fragments: u64,
}

/// What laying lines on the part needs.
pub(super) struct Layer<'a> {
    pub(super) seat: &'a Seat<'a>,
    /// The ground the field runs over: its grid, cut where it ends.
    pub(super) ground: &'a Ground,
    /// The area, its own edge counted in: a point on it is in, or clipping
    /// to the area would cut it into bits ([`with_edge`](crate::strategy::with_edge)).
    pub(super) within: Region,
    /// Measured out from the inner edges, each node's distance along the
    /// surface from the outer boundary.
    pub(super) beyond: Option<Vec<f64>>,
    /// How each is sampled on the part, as a parallel pass is.
    pub(super) sweep: ParallelParams,
    /// How near the line through its neighbours a point is thinned out.
    pub(super) thin: f64,
    /// The slopes cut, from and to, in radians from level.
    pub(super) slopes: (f64, f64),
}

impl Layer<'_> {
    /// `plan`'s lines, its own taken, laid on the part, each on its side of
    /// its region, `side` that of every line of a field measured out from
    /// inner edges, where every ring grows out from a hole and so do its
    /// pieces where the boundary cuts it; and its sequence, ring by ring.
    pub(super) fn lay(&self, plan: &mut Plan, side: Side) -> Result<Worked, StrategyError> {
        let mut w = Worked::default();
        // A line has its region on its left, so a closed one turning right
        // goes round a hole of it.
        let side_of = |line: &Sloped| {
            if side == Side::Hole || (line.closed && !turns_left(&line.points)) {
                Side::Hole
            } else {
                Side::Outer
            }
        };
        let families = |lines: Vec<Sloped>| -> (Vec<Sloped>, Vec<Sloped>) {
            lines.into_iter().partition(|l| side_of(l) == Side::Outer)
        };
        let (lines, hole_lines) = families(std::mem::take(&mut plan.lines));
        let (edges, hole_edges) = families(std::mem::take(&mut plan.edges));
        let (found, hole_found) = families(std::mem::take(&mut plan.found));
        let mut outer = Vec::new();
        self.lay_into((lines, found, edges), &mut outer, &mut w)?;
        let mut holes = Vec::new();
        self.lay_into((hole_lines, hole_found, hole_edges), &mut holes, &mut w)?;
        w.outer = outer;
        w.holes = holes;
        // Each ring of the sequence, all its runs one group, cut in turn.
        for mut ring in std::mem::take(&mut plan.sequence) {
            let mut runs = Vec::new();
            let lines = (
                std::mem::take(&mut ring.lines),
                std::mem::take(&mut ring.found),
                std::mem::take(&mut ring.edges),
            );
            self.lay_into(lines, &mut runs, &mut w)?;
            // The rings below where the tool leaves the part, as they are.
            runs.append(&mut ring.placed);
            w.sequence.push(runs);
        }
        Ok(w)
    }

    /// The field's `lines`, and the ground's edges `found` on its grid, placed
    /// by it; the area's `edges`, seen from above: laid into `runs`.
    fn lay_into(
        &self,
        (lines, found, edges): (Vec<Sloped>, Vec<Sloped>, Vec<Sloped>),
        runs: &mut Vec<Run>,
        w: &mut Worked,
    ) -> Result<(), StrategyError> {
        let (from, to) = self.slopes;
        let kept = |lines: Vec<Sloped>, w: &mut Worked| {
            lines
                .into_iter()
                .flat_map(|line| line.within(from, to, to, &mut w.slopes))
                .collect::<Vec<_>>()
        };
        let lines = kept(lines, w);
        let edges = kept(edges, w);
        let found = kept(found, w);
        let lines = lines
            .into_iter()
            .filter(|(p, _)| self.clear_of_the_boundary(p))
            .collect();
        // The field's lines, and the edges of the ground found on its grid,
        // placed by it; the area's edge, seen from above.
        for (stretches, by_field) in [(lines, true), (found, true), (edges, false)] {
            for (points, closed) in self.within.clip_lines(&stretches)? {
                if !closed && polyline_length(&points) < 2.0 * self.ground.cut.grid().pitch() {
                    w.fragments += 1;
                    continue;
                }
                if by_field {
                    self.by_field((points.as_slice(), closed), runs, w);
                } else {
                    // Placed seen from above: the tool dropped along it.
                    let points = simplify(&points, closed, self.thin);
                    runs.extend(sweep_polyline(
                        self.seat,
                        &points,
                        closed,
                        &self.sweep,
                        &mut w.sweep,
                    ));
                }
            }
        }
        Ok(())
    }

    /// `line`, placed by the field, height and all, into `runs`: round a wall
    /// at its height where the ground is steep. Broken where the part ends
    /// under the tool, along the area's edge, a line may leave bits shorter
    /// than the tool's radius: dimples between the passes either side, each
    /// lifted to and from, so left out.
    fn by_field(&self, line: (&[DVec2], bool), runs: &mut Vec<Run>, w: &mut Worked) {
        let laid = guided::sweep(
            self.seat,
            self.ground,
            line,
            self.thin,
            &self.sweep,
            &mut w.sweep,
        );
        for run in laid {
            if run.closed || length(&run.points) >= self.seat.tool().max_radius() {
                runs.push(run);
            } else {
                w.fragments += 1;
            }
        }
    }
    /// Whether a line of the field with `points` is clear of the outer
    /// boundary, measured out from the inner edges. There the field runs on
    /// to the boundary at any distance, so some of its lines near it are
    /// slivers along it, in bits where the distance there wavers. A line all
    /// of whose points are within half a stepover of the boundary along the
    /// surface is left to the boundary's ring, cut last; any other is kept
    /// whole, cutting none into bits where it runs along the boundary at
    /// about that distance.
    fn clear_of_the_boundary(&self, points: &[DVec2]) -> bool {
        let Some(beyond) = &self.beyond else {
            return true;
        };
        let grid = &self.ground.cut.view(None);
        let value = |n: u32| at(beyond, n);
        points.iter().any(|&p| {
            // A line's point is on a grid edge: as far as the field says
            // along it, whichever of the triangles beside it is whole.
            let along = guided::edge_at(grid, p).and_then(|(a, b)| {
                let (pa, pb) = (grid.xy(a)?, grid.xy(b)?);
                let t = (p.distance(pa) / pa.distance(pb)).clamp(0.0, 1.0);
                Some(value(a)? + t * (value(b)? - value(a)?))
            });
            along
                .or_else(|| grid.interpolate(p, value))
                .is_none_or(|v| v >= 0.5 * self.sweep.stepover)
        })
    }
}
