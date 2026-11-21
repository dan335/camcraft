//! Scallop finishing: passes a constant distance apart along the surface, offset
//! in from the boundary.
//!
//! Parallel finishing lays its passes a stepover apart seen from above, so on a
//! slope they are further apart along the surface, and the cusps between them
//! taller, the steeper it is. Scallop's are a stepover apart along the surface
//! everywhere, so the cusp is the same everywhere.
//!
//! # How the pieces fit, in the order they happen
//!
//! - **The ground and its strips** ([`ground`]). Where the tool sits over the
//!   part (its cutter location) is sampled on a grid by exact drops, each node
//!   classed by whether a face it machines holds the tool there. The ground is
//!   where one does; the grid's cells are cut where it ends, found exactly, so
//!   it runs right to its edges: where a face avoided begins, and where the
//!   tool leaves the part (decision 0037). Inside out, where the tool rolls
//!   over a sharp convex edge along a strip that runs right round a hole with
//!   ground either side, as over a bowl's rim, the strip is a break in the
//!   ground too, its two borders edges of it (decision 0039). The grid decides
//!   where a pass runs, seen from above, and nothing else: decision 0009, a
//!   narrow exception to 0003.
//! - **Zones and their fields** ([`distance`]). The distance along that
//!   surface from the edges the rings start from, by fast marching: outside
//!   in, the boundary and every edge of the ground; inside out, its inner
//!   edges alone, where it has any, the rest its far end. Where strips break
//!   the ground, each piece of it is a zone with a field of its own, cut one
//!   after the next from the innermost.
//! - **The rings** ([`rings`]). The lines where a field is a whole number of
//!   stepovers, and the edges, in a tree of the regions they bound. A region
//!   rising more than half a stepover past its last ring gets one more,
//!   halfway up to its crest.
//! - **The spine** ([`spine`]). Inside out, going in down a chain of the
//!   regions, inside the last ring with no corner the rings go out from a
//!   line midway between its sides and blend into it, where offsets in to the
//!   middle of an oval came to points at its ends (decision 0040).
//! - **The blend** ([`blend`]). Where another closed edge of the ground lies
//!   at a slant to the rings, as where the tool leaves a bulging bowl's
//!   outside, lower on some sides than others, or the near border of a strip,
//!   the ground between the last ring that goes all the way round and that
//!   edge is filled with closed rings that turn gradually from the one into
//!   the other, and below where the tool leaves the part rings go down beside
//!   the wall to the first level ring, rather than both being cut into arcs by
//!   it (decision 0038).
//! - **Laid on the part** ([`lay`], [`guided`]). Every point of every pass is
//!   an exact cutter location, refined along the pass until it is within
//!   tolerance of the part, as a parallel pass is. Where a line crosses a grid
//!   edge its height is as well known as its place along it, so on ground
//!   steeper than 45° a point is found round the wall at that height, across
//!   the slope, rather than dropped where it falls seen from above: the passes
//!   cut every slope themselves, walls included (decision 0035). Nothing is
//!   smoothed; a line is only thinned where a point is within a quarter of the
//!   tolerance, and a twentieth of the grid's pitch, of the line through its
//!   neighbours. The slope limits leave out what is outside the range asked
//!   for.
//! - **Cut in order** ([`order`]): zone by zone, and in each region by region,
//!   from the boundary in or from the inner edges out, along one straight
//!   seam.
//!
//! What the tool cannot reach from above, down a part's outside below where it
//! leaves it, the operation finishes with level rings, which has the part and
//! its Bottom to find them on ([`ScallopReport::walls`]).

use camcraft_geom::contour::Region;
use camcraft_geom::shadow::Shadow;
use camcraft_geom::trigrid::TriSurface;
use glam::{DVec2, DVec3};
use rayon::prelude::*;

use crate::control::{Watch, share};
use crate::cutter::surfaces::Seat;
use crate::cutter::tool::ToolProfile;
use crate::path::toolpath::Toolpath;
use crate::strategy::adaptive::Direction;
pub use crate::strategy::nesting::Order;
use crate::strategy::parallel::{ParallelParams, ParallelReport, Run};
use crate::strategy::{StrategyError, check, slope, with_edge};

use blend::Band;
use distance::{Field, Fields};
use ground::Ground;
use lay::{Layer, Worked};
use rings::{Levels, Planned};

/// The most halvings a search along a line makes, for where the ground ends
/// along a grid edge or for a point round a wall: far past any tolerance.
const MAX_HALVINGS: u32 = 48;

/// How to lay out the passes.
#[derive(Debug, Clone, PartialEq)]
pub struct ScallopParams {
    /// Distance between neighbouring passes along the surface, in millimetres.
    pub stepover: f64,
    /// Chordal tolerance along each pass, in millimetres.
    pub tolerance: f64,
    /// Cutting feed, in mm/min.
    pub feed: f64,
    /// Largest Z drop, in millimetres, that may be cut rather than linked.
    pub max_cut_step: f64,
    /// Which way round.
    pub order: Order,
    /// Which way each pass cuts.
    pub direction: Direction,
    /// Whether climbing puts the side still to finish on the tool's right, as it
    /// does with a clockwise spindle.
    pub climb_right: bool,
    /// Where the surface is at least this steep, in radians from level.
    pub slope_from: f64,
    /// And at most this steep.
    pub slope_to: f64,
    /// The most grid nodes the field may have; past it the grid is coarsened.
    pub max_nodes: u32,
    /// How steeply, in radians from level, the tool goes down along a ring
    /// onto it where going straight down onto it from the ring above would
    /// plunge further than `max_cut_step`, as down a wall.
    pub ramp: f64,
    /// The lowest the rings below where the tool leaves the part go: the
    /// operation's Bottom, where the level rings below them end.
    pub bottom: f64,
    /// What it tells how far it has got, and asks whether to stop.
    pub watch: Watch,
}

impl Default for ScallopParams {
    fn default() -> Self {
        Self {
            stepover: 0.5,
            tolerance: 0.01,
            feed: 1000.0,
            max_cut_step: 0.5,
            order: Order::OutsideIn,
            direction: Direction::BothWays,
            climb_right: true,
            slope_from: 0.0,
            slope_to: std::f64::consts::FRAC_PI_2,
            max_nodes: 1 << 23,
            ramp: 2f64.to_radians(),
            bottom: f64::NEG_INFINITY,
            watch: Watch::default(),
        }
    }
}

/// What happened while laying out the passes.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ScallopReport {
    /// Passes cut.
    pub passes: u64,
    /// Lines of equal distance found, the boundary's included.
    pub lines: u64,
    /// Levels of passes in from the boundary.
    pub levels: u64,
    /// Regions given a pass halfway up to their crest.
    pub top_ups: u64,
    /// Grid nodes the field was worked out on.
    pub nodes: u64,
    /// Their spacing, in millimetres.
    pub pitch: f64,
    /// Whether the grid was coarsened to stay within its budget.
    pub coarsened: bool,
    /// Level rings a stepover apart down the walls below where the tool rests
    /// from above, which the field cannot cover: by the operation that runs
    /// it, which has the part and its Bottom to find them on.
    pub walls: u64,
    /// Stretches left out for being outside the slope range asked for.
    pub outside_slope: u64,
    /// Stretches too short to be worth cutting.
    pub fragments: u64,
    /// Nodes inside the boundary the field never reached.
    pub unreached: u64,
    /// The highest point of where the tool leaves the part from above, if it
    /// does inside the area.
    pub leaves_at: Option<f64>,
    /// Where the level rings down the walls below the field start, a stepover
    /// under: where the tool leaves the part, its highest point; where rings
    /// blend down from that line to the level rings, half a stepover under
    /// its lowest (decision 0038).
    pub walls_from: Option<f64>,
    /// Rings blended into an edge the field's rings meet at a slant, above it
    /// and below it (decision 0038).
    pub blended: u64,
    /// What sampling the passes on the part found, as for parallel finishing.
    pub sweep: ParallelReport,
}

impl ScallopReport {
    /// Whether anything needs the caller's attention.
    pub fn is_clean(&self) -> bool {
        self.sweep.is_clean() && self.unreached == 0
    }
}

/// Where the tool touches the part: within its widest radius of the part's
/// `shadow` seen from above, a tolerance short so as never to reach past it.
pub fn contact_region(
    shadow: &Shadow,
    max_radius: f64,
    tolerance: f64,
) -> Result<Region, StrategyError> {
    shadow
        .grown((max_radius - tolerance).max(tolerance), tolerance * 0.5)
        .map_err(StrategyError::from)
}

/// Up to a tolerance past where the tool touches the part: within its widest
/// radius of the part's `shadow` seen from above, and a tolerance more. For
/// a pattern that finds where the tool leaves the part itself, exactly, as
/// scallop's field does: drawn short of it, the area's edge would be a
/// tolerance short, down a wall's vertical fall to where the tool leaves the
/// part a millimetre or more of height.
pub fn past_contact(
    shadow: &Shadow,
    max_radius: f64,
    tolerance: f64,
) -> Result<Region, StrategyError> {
    shadow
        .grown(max_radius + tolerance, tolerance * 0.5)
        .map_err(StrategyError::from)
}

/// Scallop passes over `area`, where the tool's centre may go and touches the part.
///
/// The path holds [`MoveRole::Cut`] passes separated by [`MoveRole::Link`] moves, in
/// the order they are cut. The linker resolves them.
///
/// [`MoveRole::Cut`]: crate::path::toolpath::MoveRole::Cut
/// [`MoveRole::Link`]: crate::path::toolpath::MoveRole::Link
pub fn scallop_finish(
    seat: &Seat<'_>,
    area: &Region,
    params: &ScallopParams,
) -> Result<(Toolpath, ScallopReport), StrategyError> {
    scallop_plan(seat, area, params)?.cut(seat, params, None)
}

/// Scallop's rings over an area, laid on the part, yet to be cut into a path
/// ([`ScallopPlan::cut`]).
pub struct ScallopPlan {
    zones: Vec<(Planned, Vec<Worked>)>,
    report: ScallopReport,
}

impl ScallopPlan {
    /// What was found laying them out: where the tool leaves the part, and
    /// where the level rings below the rings start
    /// ([`ScallopReport::walls_from`]), among it.
    pub fn report(&self) -> &ScallopReport {
        &self.report
    }

    /// The rings cut into a path, as `params` ask, the first started nearest
    /// where the tool is, `from`, where it is anywhere, and the seam running
    /// on from there; with what happened.
    pub fn cut(
        self,
        seat: &Seat<'_>,
        params: &ScallopParams,
        from: Option<DVec3>,
    ) -> Result<(Toolpath, ScallopReport), StrategyError> {
        let mut report = self.report;
        let (path, passes) = order::cut(seat, self.zones, params, from)?;
        report.passes = passes;
        Ok((path, report))
    }
}

/// Scallop's rings over `area`, as [`scallop_finish`] lays them, not yet cut.
pub fn scallop_plan(
    seat: &Seat<'_>,
    area: &Region,
    params: &ScallopParams,
) -> Result<ScallopPlan, StrategyError> {
    check("stepover", params.stepover)?;
    check("tolerance", params.tolerance)?;
    check("feed", params.feed)?;
    check("max_cut_step", params.max_cut_step)?;
    slope::check_range(params.slope_from, params.slope_to)?;
    let mut report = ScallopReport::default();
    let Some(bounds) = area.bounds() else {
        return Ok(ScallopPlan {
            zones: Vec::new(),
            report,
        });
    };
    let sampled = Ground::sampled(seat, area, bounds, params, &mut report)?;
    // Strips break the ground into zones so that every ring closes. Where a
    // zone's rings cannot close, no band into its far edge that can be placed,
    // the zones are not doing their job, and the ground is cut as one, its
    // strips ground again (decision 0039). The levels and bands found go on
    // to the plan, so what passes is what is planned.
    let fields = Field::zones(seat, &sampled, area, params, &mut report);
    let levelled = level_zones(seat, &sampled, &fields, area, params);
    let unstripped;
    let (ground, Fields { zones, beyond }, levelled) =
        if !sampled.has_strips() || every_zone_closes(&levelled) {
            (&sampled, fields, levelled)
        } else {
            unstripped = sampled.without_strips(seat, params)?;
            let fields = Field::zones(seat, &unstripped, area, params, &mut report);
            let levelled = level_zones(seat, &unstripped, &fields, area, params);
            (&unstripped, fields, levelled)
        };
    report.leaves_at = ground.leaves_at(area);
    report.walls_from = report.leaves_at;
    let view = ground.cut.view(None);
    let mut planned: Vec<Planned> = zones
        .iter()
        .zip(levelled)
        .map(|(field, levelled)| {
            rings::plan(seat, &view, (field, area), levelled, params, &mut report)
        })
        .collect();

    // The field and its lines are done: what is left is laying each node's
    // lines on the part, an equal share of the rest each.
    params.watch.check()?;
    params.watch.report(0.4);
    let layer = Layer {
        seat,
        ground,
        within: with_edge(area, params.tolerance),
        beyond,
        sweep: ParallelParams {
            stepover: params.stepover,
            tolerance: params.tolerance,
            feed: params.feed,
            max_cut_step: params.max_cut_step,
            ..Default::default()
        },
        thin: (0.25 * params.tolerance).min(0.05 * view.pitch()),
        slopes: (params.slope_from, params.slope_to),
    };
    let each = params.watch.part(0.4, 1.0);
    let share = share(1, planned.iter().map(|p| p.plans.len()).sum());
    let mut worked = Vec::with_capacity(planned.len());
    for zone in &mut planned {
        let laid: Vec<Result<Worked, StrategyError>> = zone
            .plans
            .par_iter_mut()
            .map(|plan| {
                params.watch.check()?;
                let w = layer.lay(plan, zone.side)?;
                if params.watch.showing() {
                    params.watch.show(
                        w.outer
                            .iter()
                            .chain(&w.holes)
                            .chain(w.sequence.iter().flatten())
                            .map(Run::shown),
                    );
                }
                each.add(share);
                Ok(w)
            })
            .collect();
        let laid: Vec<Worked> = laid.into_iter().collect::<Result<_, _>>()?;
        for w in &laid {
            report.outside_slope += w.slopes.outside_slope;
            report.fragments += w.fragments;
            report.sweep += w.sweep;
        }
        worked.push(laid);
    }
    Ok(ScallopPlan {
        zones: planned.into_iter().zip(worked).collect(),
        report,
    })
}

/// A zone's levels, and its band into each of its far edges with the band's
/// home, where its rings blend into that edge.
type Levelled = (Levels, Vec<Option<(Band, usize)>>);

/// Each zone of `fields` over `ground`, levelled: a band only where it can be
/// placed, its home found ([`blend::band`], [`Levels::home`]).
fn level_zones(
    seat: &Seat<'_>,
    ground: &Ground,
    fields: &Fields<'_>,
    area: &Region,
    params: &ScallopParams,
) -> Vec<Levelled> {
    let grid = ground.cut.view(None);
    let diameter = 2.0 * seat.tool().max_radius();
    fields
        .zones
        .iter()
        .map(|zone| {
            let levels = Levels::of(&grid, zone, params.stepover);
            let bands = zone
                .far_edges
                .iter()
                .map(|far| {
                    let band = blend::band(&grid, (zone, area), far, params.stepover, diameter)?;
                    let home = levels.home(&band)?;
                    Some((band, home))
                })
                .collect();
            (levels, bands)
        })
        .collect()
}

/// Whether every zone closes its rings, `levelled`: it has no far edge, or
/// blends into one.
fn every_zone_closes(levelled: &[Levelled]) -> bool {
    levelled
        .iter()
        .all(|(_, bands)| bands.is_empty() || bands.iter().any(Option::is_some))
}

/// The length of a polyline.
fn length(points: &[DVec3]) -> f64 {
    points
        .iter()
        .zip(points.iter().skip(1))
        .map(|(a, b)| a.distance(*b))
        .sum()
}

/// Whether closed `points` turn left round what they bound, seen from above.
fn turns_left(points: &[DVec2]) -> bool {
    let n = points.len();
    let twice: f64 = (0..n)
        .filter_map(|i| Some(points.get(i)?.perp_dot(*points.get((i + 1) % n)?)))
        .sum();
    twice > 0.0
}

mod blend;
mod distance;
mod ground;
mod guided;
mod lay;
mod order;
mod rings;
mod spine;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)]
mod tests;
