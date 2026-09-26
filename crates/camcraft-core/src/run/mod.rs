//! Running a setup: its operations in order, against one block of material.
//!
//! Every operation reads the stock as it currently stands and writes back what it
//! removed, so the next one never re-cuts cleared air. That ordering is the whole of
//! rest machining.
//!
//! Each operation is simulated as it runs, not at the end. A gouge in operation one
//! is worth knowing about before operation two is planned against the material it
//! left behind.
//!
//! An operation with picked faces or edges is kept to their boundary
//! ([`crate::features::machining_boundary`]) as well as to the stock's footprint.

mod error;
mod operation;
mod pitch;
mod result;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests;

use std::sync::Arc;

use camcraft_sim::stock::{PackedStock, PartSurface, Stock};
use glam::DVec2;

use crate::allowance::{Allowance, EffectiveTool};
use crate::dropcutter::DropCutter;
use crate::features::machining_boundary;
use crate::job::{HeightContext, Job, JobError, Operation, StrategySpec, ToolSpec};
use crate::setup::{Bodies, Prepared, prepare};
use crate::simadapt::sim_moves;

pub use crate::toolpath::PLUNGE_RATIO;
pub use error::RunError;
pub use pitch::{CELLS_PER_TOOL, FINEST_PITCH, MAX_SIM_CELLS, SIM_CELLS, sim_pitch};
pub use result::{Made, OperationResult, Plan, SetupResult, Stage, StrategyDetail};

use error::in_op;
use operation::{Target, Work, drill_holes, not_made, planar_areas, run_operation};

/// Material further above the part than this, in millimetres, counts as left over.
///
/// Leftover is not a failure; it is where the cutter could not reach. It is named
/// here so that anything drawing the material classifies it by the same rule the
/// verifier counts it by.
pub const LEFTOVER_LIMIT: f64 = 1.0;

/// Check, lay out and run setup `index` of `job`, with its models by name.
pub fn run_setup(job: &Job, index: usize, bodies: &Bodies<'_>) -> Result<SetupResult, RunError> {
    job.check_common()?;
    job.check_setup(index)?;
    let prepared = prepare(job, index, bodies)?;
    run_prepared_planned(job, index, &prepared, bodies, &[], &mut |_| {})
}

/// Run every operation of setup `index` against its block of material, already
/// laid out in its frame, telling `progress` each [`Stage`] as it starts. The job
/// must have been checked. `bodies` are the models it was laid out from, where
/// picked faces and edges are found.
///
/// Each operation is done as `plans` says, by its place: an operation with no
/// plan is worked out, and one turned off is left out whatever its plan. Kept
/// operations are cut into the material again, so what comes after them is
/// planned against what they leave.
pub fn run_prepared_planned(
    job: &Job,
    index: usize,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    plans: &[Plan],
    progress: &mut dyn FnMut(Stage),
) -> Result<SetupResult, RunError> {
    let setup = job.setups.get(index).ok_or(JobError::NoSetups)?;
    let mesh = &prepared.part;
    let bounds = prepared.model_bounds;
    let all = prepared.stock_bounds();
    let lo = all.min.truncate();
    let hi = all.max.truncate();
    let stock_top = all.max.z;
    let stock_bottom = all.min.z;

    let ctx = HeightContext {
        stock_top,
        stock_bottom,
        model_top: bounds.max.z,
        model_bottom: bounds.min.z,
    };

    let narrowest = setup
        .operations
        .iter()
        .filter_map(|op| job.tool_for(op))
        .map(|t| t.diameter)
        .fold(None, |n: Option<f64>, d| Some(n.map_or(d, |n| n.min(d))));
    let sim_pitch = sim_pitch(job.tolerance, hi - lo, narrowest);
    let blocks: Vec<(DVec2, DVec2, f64)> = prepared
        .stocks
        .iter()
        .map(|b| (b.min.truncate(), b.max.truncate(), b.max.z))
        .collect();
    let mut stock = Stock::from_boxes(&blocks, stock_bottom - 1.0, sim_pitch)?;
    let cutter = DropCutter::new(mesh);

    let mut operations = Vec::with_capacity(setup.operations.len());
    let mut keyframes = Vec::with_capacity(setup.operations.len() + 1);
    keyframes.push(Arc::new(PackedStock::pack(&stock)));
    for (i, op) in setup.operations.iter().enumerate() {
        let plan = plans.get(i).unwrap_or(&Plan::Generate);
        if !op.enabled || matches!(plan, Plan::Skip) {
            let made = if op.enabled {
                Made::Skipped
            } else {
                Made::Disabled
            };
            operations.push(not_made(op, job.tool_for(op), made));
            keyframes.push(Arc::new(PackedStock::pack(&stock)));
            continue;
        }
        progress(Stage::Operation(i));
        let (spec, tool) = tool_of(job, op)?;
        if let Plan::Keep(before) = plan {
            let mut kept = (**before).clone();
            kept.sim = stock.apply(&sim_moves(&kept.path), &tool);
            kept.made = Made::Kept;
            operations.push(kept);
            keyframes.push(Arc::new(PackedStock::pack(&stock)));
            continue;
        }
        // A drilling operation's picked edges are its holes, and a 2D one's
        // boundary is what it cuts round or clears, not a limit.
        let target = match op.strategy {
            StrategySpec::Drill { .. } => {
                Target::Holes(drill_holes(job, op, prepared, bodies, &ctx)?)
            }
            StrategySpec::Profile { .. } | StrategySpec::Pocket { .. } => {
                Target::Areas(planar_areas(job, op, prepared, bodies, &ctx, &tool)?)
            }
            StrategySpec::Parallel { .. } | StrategySpec::ZlevelRough { .. } => Target::Limit(
                machining_boundary(
                    &op.geometry,
                    prepared,
                    bodies,
                    tool.cutter_radius(),
                    job.tolerance,
                )
                .map_err(in_op(op))?,
            ),
        };
        let work = Work {
            job,
            op,
            spec,
            tool: &tool,
            heights: &ctx,
            mesh,
            cutter: &cutter,
            bounds: (lo, hi),
        };
        operations.push(run_operation(&work, &target, &mut stock)?);
        keyframes.push(Arc::new(PackedStock::pack(&stock)));
    }

    // The simulator resolves the surface on a grid, so it cannot be held finer than
    // one cell plus the chordal tolerance the paths were built to; and that holds
    // across as well as down, or every vertical wall is a gouge beside it.
    progress(Stage::Verifying);
    let budget = job.tolerance + sim_pitch;
    let part = Arc::new(PartSurface::from_mesh(mesh, lo, hi, sim_pitch)?.reaching(budget));
    let verify = stock.verify(&part, budget, LEFTOVER_LIMIT);

    Ok(SetupResult {
        setup: setup.name.clone(),
        frame: prepared.frame,
        operations,
        part,
        verify,
        verify_budget: budget,
        stocks: prepared.stocks.clone(),
        stock_min: lo,
        stock_max: hi,
        heights: ctx,
        keyframes,
    })
}

/// The tool an operation cuts with, as the simulator sweeps it: the job's cutter,
/// grown by the operation's stock to leave.
///
/// The one place this is built, so a replay of the material sweeps exactly the
/// tool the run did.
pub fn cutting_tool(job: &Job, op: &Operation) -> Result<EffectiveTool, RunError> {
    tool_of(job, op).map(|(_, tool)| tool)
}

/// The tool `op` names, both as the job describes it and as it cuts
/// ([`cutting_tool`]), looked up and built once.
fn tool_of<'j>(job: &'j Job, op: &Operation) -> Result<(&'j ToolSpec, EffectiveTool), RunError> {
    let spec = job.tool_for(op).ok_or_else(|| unknown_tool(op))?;
    let base = spec.build()?;
    let leave = Allowance::new(
        op.passes.stock_to_leave.radial,
        op.passes.stock_to_leave.axial,
    )?;
    Ok((spec, EffectiveTool::of(&base, leave)?))
}

fn unknown_tool(op: &Operation) -> JobError {
    match &op.tool {
        Some(tool) => JobError::UnknownTool {
            op: op.name.clone(),
            tool: tool.clone(),
        },
        None => JobError::NoTool {
            op: op.name.clone(),
        },
    }
}
