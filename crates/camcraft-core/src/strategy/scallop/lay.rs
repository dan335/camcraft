//! Each node's lines laid on the part: kept to the slope range and the area,
//! the field's lines and the ground's edges placed by the field, height and
//! all ([`guided`]), the area's edge dropped onto the part seen from above.

use camcraft_geom::contour::{Region, polyline_length, simplify};
use camcraft_geom::cut::View;
use camcraft_geom::trigrid::TriSurface;
use glam::DVec2;

use crate::cutter::surfaces::Seat;
use crate::cutter::tool::ToolProfile;
use crate::strategy::StrategyError;
use crate::strategy::parallel::{ParallelParams, ParallelReport, Run, sweep_polyline};
use crate::strategy::slope::{SlopeCounts, Sloped};

use super::guided;
use super::rings::Plan;

/// A node's lines, laid on the part, and what was left out.
#[derive(Default)]
pub(super) struct Worked {
    pub(super) runs: Vec<Run>,
    pub(super) sweep: ParallelReport,
    pub(super) slopes: SlopeCounts,
    pub(super) fragments: u64,
}

/// What laying lines on the part needs.
pub(super) struct Layer<'a> {
    pub(super) seat: &'a Seat<'a>,
    /// The field's grid, cut where the ground ends.
    pub(super) grid: &'a View<'a>,
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
    /// `plan`'s lines, its own taken, laid on the part.
    pub(super) fn lay(&self, plan: &mut Plan) -> Result<Worked, StrategyError> {
        let mut w = Worked::default();
        let (from, to) = self.slopes;
        let kept = |lines: Vec<Sloped>, w: &mut Worked| {
            lines
                .into_iter()
                .flat_map(|line| line.within(from, to, to, &mut w.slopes))
                .collect::<Vec<_>>()
        };
        let lines = kept(std::mem::take(&mut plan.lines), &mut w);
        let edges = kept(std::mem::take(&mut plan.edges), &mut w);
        let found = kept(std::mem::take(&mut plan.found), &mut w);
        let lines = lines
            .into_iter()
            .filter(|(p, _)| self.clear_of_the_boundary(p))
            .collect();
        // The field's lines, and the edges of the ground found on its grid,
        // placed by it; the area's edge, seen from above.
        for (stretches, by_field) in [(lines, true), (found, true), (edges, false)] {
            for (points, closed) in self.within.clip_lines(&stretches)? {
                if !closed && polyline_length(&points) < 2.0 * self.grid.pitch() {
                    w.fragments += 1;
                    continue;
                }
                if by_field {
                    self.by_field((points.as_slice(), closed), &mut w);
                } else {
                    // Placed seen from above: the tool dropped along it.
                    let points = simplify(&points, closed, self.thin);
                    w.runs.extend(sweep_polyline(
                        self.seat,
                        &points,
                        closed,
                        &self.sweep,
                        &mut w.sweep,
                    ));
                }
            }
        }
        Ok(w)
    }

    /// `line`, placed by the field, height and all: round a wall at its
    /// height where the ground is steep. Broken where the part ends under the
    /// tool, along the area's edge, a line may leave bits shorter than the
    /// tool's radius: dimples between the passes either side, each lifted to
    /// and from, so left out.
    fn by_field(&self, line: (&[DVec2], bool), w: &mut Worked) {
        let runs = guided::sweep(
            self.seat,
            self.grid,
            line,
            self.thin,
            &self.sweep,
            &mut w.sweep,
        );
        for run in runs {
            if run.closed || guided::length(&run.points) >= self.seat.tool().max_radius() {
                w.runs.push(run);
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
        let grid = self.grid;
        let value = |n: u32| {
            usize::try_from(n)
                .ok()
                .and_then(|i| beyond.get(i))
                .copied()
                .filter(|v| v.is_finite())
        };
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
