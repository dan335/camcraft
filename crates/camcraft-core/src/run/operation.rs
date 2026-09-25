//! Running one operation: its strategy's paths, ordered, linked over the material
//! as it stands, and cut into it.

use camcraft_geom::contour::Region;
use camcraft_geom::mesh::TriMesh;
use camcraft_geom::topology::EdgeKind;
use camcraft_sim::stock::{SimReport, Stock};
use glam::{DVec2, dvec2};

use super::error::{OperationError, in_op};
use super::{Made, OperationResult, PLUNGE_RATIO, RunError, StrategyDetail};
use crate::allowance::EffectiveTool;
use crate::drilling::{DrillParams, DrillReport, Hole, drill, holes, nearest_first};
use crate::dropcutter::DropCutter;
use crate::features::{Chain, picked_chains};
use crate::job::{HeightContext, Job, Operation, StrategySpec, ToolSpec};
use crate::linker::{ClearanceField, LinkParams, LinkReport, link};
use crate::ordering::order_passes;
use crate::parallel::{ParallelParams, parallel_finish_within};
use crate::reach::LevelReach;
use crate::roughing::{RoughingParams, zlevel_rough};
use crate::setup::{Bodies, Prepared};
use crate::simadapt::sim_moves;
use crate::strategy::StrategyError;
use crate::tool::ToolProfile;
use crate::toolpath::Toolpath;

/// One operation, and everything its strategy works from.
pub(super) struct Work<'a> {
    /// The job it is in.
    pub(super) job: &'a Job,
    /// The operation.
    pub(super) op: &'a Operation,
    /// Its tool, as the job describes it.
    pub(super) spec: &'a ToolSpec,
    /// Its tool as it cuts: grown by the stock to leave.
    pub(super) tool: &'a EffectiveTool,
    /// What its heights resolve against.
    pub(super) heights: &'a HeightContext,
    /// The setup's part.
    pub(super) mesh: &'a TriMesh,
    /// The drop-cutter over the part.
    pub(super) cutter: &'a DropCutter<'a>,
    /// The stock's footprint in XY, lower and upper corner.
    pub(super) bounds: (DVec2, DVec2),
}

impl Work<'_> {
    /// Where the tool's centre may sweep: the stock's footprint, plus the tool,
    /// plus whatever extra was asked for.
    fn sweep(&self) -> (DVec2, DVec2) {
        let (lo, hi) = self.bounds;
        let reach = self.tool.max_radius() + self.op.geometry.margin;
        (
            dvec2(lo.x - reach, lo.y - reach),
            dvec2(hi.x + reach, hi.y + reach),
        )
    }
}

/// A strategy's paths, before ordering and linking: the path, how many passes it
/// has, and what the strategy reported.
type Paths = (Toolpath, u64, StrategyDetail);

/// Work out one operation's paths, link them over the material as it stands, and
/// cut them into it. `limit` is the boundary it is kept to, if any, and `holes`
/// are a drilling operation's.
pub(super) fn run_operation(
    work: &Work<'_>,
    limit: Option<&Region>,
    holes: &[Hole],
    stock: &mut Stock,
) -> Result<OperationResult, RunError> {
    let op = work.op;
    let ctx = work.heights;
    let mut warnings = Vec::new();

    let (raw, passes, detail) = match op.strategy {
        StrategySpec::Parallel { angle } => parallel_paths(work, angle, limit, &mut warnings)?,
        StrategySpec::ZlevelRough {
            stepdown,
            min_ring_length,
        } => zlevel_paths(work, stepdown, min_ring_length, limit, stock, &mut warnings)?,
        StrategySpec::Drill { peck, dwell } => {
            drill_paths(work, peck, dwell, holes, &mut warnings)?
        }
    };

    // Ordering is a Toolpath -> Toolpath transform, applied before linking so the
    // retracts it needs are planned around the order actually used. Holes are
    // already in order, and a hole reversed would be drilled from the bottom up.
    let drilling = matches!(op.strategy, StrategySpec::Drill { .. });
    let (ordered, order) = if op.linking.reorder && !drilling {
        order_passes(&raw, op.linking.allow_reverse)?
    } else {
        (raw, Default::default())
    };

    // Clearance comes from the material as it currently stands, so the tool stays
    // low where roughing has already been.
    let clearance_z = op.heights.clearance.resolve(ctx);
    let clearance = ClearanceField::new(
        stock.field().clone(),
        op.heights.clearance.offset,
        clearance_z,
    )
    .map_err(in_op(op))?;

    let (path, links) = link(
        &ordered,
        &clearance,
        &LinkParams {
            retract_feed: op.linking.retract_feed,
            plunge_feed: op.linking.plunge_feed,
            tool_radius: work.tool.max_radius(),
        },
    )
    .map_err(in_op(op))?;
    if links.suspicious_plunges > 0 {
        warnings.push(format!(
            "{} plunge(s) do not start above the surface",
            links.suspicious_plunges
        ));
    }
    if path.has_unplanned_links() {
        warnings.push("path still contains unresolved links and is NOT machinable".into());
    }

    // Simulate now, not at the end: a gouge here is worth knowing before the next
    // operation is planned against the material this one left.
    let sim = stock.apply(&sim_moves(&path), work.tool);
    if sim.rapid_contacts > 0 {
        warnings.push(format!(
            "{} rapid move(s) pass through material, deepest {:.2} mm",
            sim.rapid_contacts, sim.deepest_rapid_contact_mm
        ));
    }
    let start = path.moves().first().map_or(glam::DVec3::ZERO, |m| m.to);
    let (steep, deepest) = path.steep_cut_descents(start, PLUNGE_RATIO);
    if deepest > op.passes.max_cut_step {
        warnings.push(format!(
            "a steep cutting descent of {deepest:.2} mm exceeds the {:.2} mm \
             allowance: a plunge at cutting feed ({steep} steep descent(s))",
            op.passes.max_cut_step
        ));
    }

    Ok(OperationResult {
        path,
        sim,
        passes,
        travel_saved: order.saved(),
        detail,
        links,
        clearance_z,
        steep_cut_descents: steep,
        deepest_cut_descent_mm: deepest,
        warnings,
        // The rest is the operation's own: its name, strategy and tool.
        ..not_made(op, Some(work.spec), Made::Generated)
    })
}

/// Raster finishing, with the tool's centre kept to `limit` as well as the sweep.
fn parallel_paths(
    work: &Work<'_>,
    angle: f64,
    limit: Option<&Region>,
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let (job, op) = (work.job, work.op);
    let (sweep_lo, sweep_hi) = work.sweep();
    let params = ParallelParams {
        stepover: op.passes.stepover,
        angle: angle.to_radians(),
        tolerance: job.tolerance,
        feed: op.passes.feed,
        max_cut_step: op.passes.max_cut_step,
        ..Default::default()
    };
    let (path, report) =
        parallel_finish_within(work.cutter, work.tool, sweep_lo, sweep_hi, limit, &params)
            .map_err(in_op(op))?;
    if !report.is_clean() {
        warnings.push(format!(
            "{} span(s) hit the refinement limit; the path is coarser than the \
             requested tolerance",
            report.refinement_limited
        ));
    }
    Ok((path, report.passes, StrategyDetail::Parallel(report)))
}

/// Z-level clearing of the material as it stands, `stepdown` a level, within the
/// sweep and `limit`.
fn zlevel_paths(
    work: &Work<'_>,
    stepdown: f64,
    min_ring_length: f64,
    limit: Option<&Region>,
    stock: &Stock,
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let (job, op, ctx) = (work.job, work.op, work.heights);
    let mut reach = LevelReach::new(work.mesh, work.tool, job.tolerance).map_err(in_op(op))?;
    let params = RoughingParams {
        stepdown,
        stepover: op.passes.stepover,
        feed: op.passes.feed,
        top: op.geometry.top.map_or(ctx.stock_top, |h| h.resolve(ctx)),
        bottom: op
            .geometry
            .bottom
            .map_or(ctx.model_bottom, |h| h.resolve(ctx)),
        min_ring_length,
    };
    let (sweep_lo, sweep_hi) = work.sweep();
    let sweep = Region::rectangle(sweep_lo, sweep_hi);
    let area = match limit {
        Some(l) => sweep
            .intersection(l)
            .map_err(|e| StrategyError::Region(e.to_string()).named(op.name.clone()))?,
        None => sweep,
    };
    let (path, report) =
        zlevel_rough(&mut reach, &area, stock.field(), &params).map_err(in_op(op))?;
    if report.rings == 0 {
        warnings.push(
            "found nothing to clear: the stock may already be below this \
             operation's top"
                .into(),
        );
    }
    Ok((path, report.rings, StrategyDetail::ZlevelRough(report)))
}

/// Each of `holes` drilled straight down, pecking by `peck` if given; a hole the
/// tool is not the size of is drilled, and said.
fn drill_paths(
    work: &Work<'_>,
    peck: Option<f64>,
    dwell: Option<f64>,
    holes: &[Hole],
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let (job, op) = (work.job, work.op);
    let params = DrillParams {
        peck,
        feed: op.linking.plunge_feed,
        retract_feed: op.linking.retract_feed,
    };
    let path = drill(holes, &params).map_err(in_op(op))?;
    let width = work.spec.diameter;
    for h in holes {
        if (2.0 * h.radius - width).abs() > job.tolerance.max(0.01) {
            warnings.push(format!(
                "the hole at ({:.3}, {:.3}) is {:.3} mm across but the tool is {:.3} mm",
                h.at.x,
                h.at.y,
                2.0 * h.radius,
                width
            ));
        }
    }
    let report = DrillReport {
        holes: holes.to_vec(),
        peck,
        dwell,
        feed: params.feed,
    };
    Ok((path, holes.len() as u64, StrategyDetail::Drill(report)))
}

/// The holes a drilling operation's picked rims make, in the setup's frame, in the
/// order they are drilled.
pub(super) fn drill_holes(
    job: &Job,
    op: &Operation,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    ctx: &HeightContext,
) -> Result<Vec<Hole>, RunError> {
    let chains = picked_chains(&op.geometry, prepared, bodies, job.tolerance).map_err(in_op(op))?;
    let circle = |c: &Chain| {
        let body = bodies.get(c.model.as_str())?;
        match body.topology().edge(c.edge)?.kind {
            EdgeKind::Circle(circle) => {
                Some((prepared.frame.to_setup(circle.center), circle.radius))
            }
            _ => None,
        }
    };
    let bottom = op
        .geometry
        .bottom
        .map_or(ctx.model_bottom, |h| h.resolve(ctx));
    let found = holes(&chains, circle, bottom, job.tolerance.max(0.01)).map_err(in_op(op))?;
    Ok(nearest_first(&found))
}

/// What stands for an operation left out of a run: no paths, nothing cut. `spec`
/// is its tool, if it names one the job has.
///
/// Its name, strategy and tool are what any result of the operation carries, so
/// a result worked out takes them from here too.
pub(super) fn not_made(op: &Operation, spec: Option<&ToolSpec>, made: Made) -> OperationResult {
    let detail = match op.strategy {
        StrategySpec::Parallel { .. } => StrategyDetail::Parallel(Default::default()),
        StrategySpec::ZlevelRough { .. } => StrategyDetail::ZlevelRough(Default::default()),
        StrategySpec::Drill { peck, dwell } => StrategyDetail::Drill(DrillReport {
            holes: Vec::new(),
            peck,
            dwell,
            feed: op.linking.plunge_feed,
        }),
    };
    OperationResult {
        name: op.name.clone(),
        strategy: op.strategy.name(),
        path: Toolpath::new(),
        tool_number: spec.map_or(0, |t| t.number),
        rpm: op.passes.rpm.or(spec.map(|t| t.rpm)).unwrap_or(0),
        tool_comment: spec.map(|t| t.comment.clone()).unwrap_or_default(),
        spindle: spec.map(|t| t.spindle).unwrap_or_default(),
        coolant: spec.map(|t| t.coolant).unwrap_or_default(),
        sim: SimReport::default(),
        passes: 0,
        travel_saved: 0.0,
        detail,
        links: LinkReport::default(),
        clearance_z: 0.0,
        steep_cut_descents: 0,
        deepest_cut_descent_mm: 0.0,
        max_cut_step: op.passes.max_cut_step,
        warnings: Vec::new(),
        made,
    }
}
