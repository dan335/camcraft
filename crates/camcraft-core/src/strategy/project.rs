//! Project finishing: picked curves dropped onto the part, the tool run along
//! each where it rests on the part below it. Fusion's Project, in its Drop
//! mode, the only one it has for a 3-axis job.
//!
//! Each curve is seen from above, as its points are wherever it lies: an edge
//! of the part's top or of its floor is followed alike, at the height the tool
//! comes to rest on the part over it. That is a pattern dropped onto the part,
//! as a spiral's is, one line a curve, in the order picked
//! ([`crate::strategy::projected::project`]); what is Project's own is which
//! way each runs, where it starts, and how far above the part.
//!
//! # Which way
//!
//! Fusion's programs for `ops_3d` (decision 0029) run a closed curve clockwise
//! seen from above, as its new chains are, and an open one from its first
//! edge's start; *Other Way* the reverse of both. *Both Ways* runs each as
//! *One Way* does, but an open curve from whichever end is nearer where the
//! last one left the tool. A closed curve starts where the last left off, at
//! its nearest point; the first at its own start.

use camcraft_geom::contour::Region;
use glam::DVec2;

use crate::cutter::dropcutter::{DropCutter, DropTest};
use crate::path::toolpath::Toolpath;
use crate::strategy::StrategyError;
use crate::strategy::curve::Curve as Picked;
use crate::strategy::projected::{Curve, ProjectParams, ProjectReport, project};

/// Which way each curve is cut: Fusion's Direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Way {
    /// As one way, but an open curve from its nearer end.
    #[default]
    BothWays,
    /// A closed curve clockwise, an open one as picked.
    OneWay,
    /// A closed curve anticlockwise, an open one from its other end.
    OtherWay,
}

/// How the curves are cut.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectCurvesParams {
    /// How each is dropped onto the part; its `raise` the axial offset.
    pub drop: ProjectParams,
    /// Which way each runs.
    pub way: Way,
}

/// `curves`, in the order picked, each run the way asked, dropped onto the part
/// within `area` and cut along it.
///
/// The path holds [`MoveRole::Cut`](crate::path::toolpath::MoveRole::Cut)
/// passes separated by links, for the linker.
pub fn project_curves<T: DropTest + Sync>(
    cutter: &DropCutter<'_>,
    tool: &T,
    area: &Region,
    curves: &[Picked],
    params: &ProjectCurvesParams,
) -> Result<(Toolpath, ProjectReport), StrategyError> {
    let tol = params.drop.tolerance;
    let laid = directed(curves, tol, params.way);
    project(cutter, tool, area, &laid, &params.drop)
}

/// `curves` seen from above, each within `tol` of its arcs, run the way `way`
/// asks; an open one, with both ways, from the end nearer where the one before
/// left the tool.
fn directed(curves: &[Picked], tol: f64, way: Way) -> Vec<Curve> {
    let mut at: Option<DVec2> = None;
    let mut out = Vec::with_capacity(curves.len());
    for picked in curves {
        let closed = picked.is_closed(tol);
        let shaped = if closed {
            picked.wound(way == Way::OtherWay)
        } else if way == Way::OtherWay {
            picked.reversed()
        } else {
            picked.clone()
        };
        let mut points: Vec<DVec2> = shaped.sample(tol).iter().map(|p| p.truncate()).collect();
        if closed {
            // A closed curve does not repeat its first point.
            points.pop();
        }
        if !closed
            && way == Way::BothWays
            && let (Some(from), Some(first), Some(last)) = (at, points.first(), points.last())
            && from.distance(*last) < from.distance(*first)
        {
            points.reverse();
        }
        // Where it leaves the tool: an open curve at its end, a closed one
        // where it started, nearest where the tool was.
        at = if closed {
            match at {
                Some(from) => points
                    .iter()
                    .copied()
                    .min_by(|a, b| a.distance(from).total_cmp(&b.distance(from))),
                None => points.first().copied(),
            }
        } else {
            points.last().copied()
        }
        .or(at);
        out.push(if closed {
            Curve::closed(points)
        } else {
            Curve::open(points)
        });
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)]
mod tests;
