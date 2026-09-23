//! Running a job: operations in order, against one block of material.
//!
//! Every operation reads the stock as it currently stands and writes back what it
//! removed, so the next one never re-cuts cleared air. That ordering is the whole of
//! rest machining.
//!
//! Each operation is simulated as it runs, not at the end. A gouge in operation one
//! is worth knowing about before operation two is planned against the material it
//! left behind.

use camcraft_geom::mesh::TriMesh;
use camcraft_sim::stock::{PartSurface, SimReport, Stock as SimStock, StockError, VerifyReport};
use glam::{DVec2, dvec2};

use crate::clmap::{ClMap, ClMapError};
use crate::dropcutter::DropCutter;
use crate::job::{HeightContext, Job, JobError, Operation, StockSpec, StrategySpec};
use crate::linker::{ClearanceField, LinkError, LinkParams, LinkReport, link};
use crate::ordering::order_passes;
use crate::parallel::{ParallelParams, StrategyError, StrategyReport, parallel_finish};
use crate::roughing::{RoughingParams, RoughingReport, zlevel_rough};
use crate::simadapt::sim_moves;
use crate::stock::{EffectiveTool, Stock as ToLeave, StockError as ToLeaveError};
use crate::tool::{ToolError, ToolProfile};
use crate::toolpath::{Toolpath, ToolpathError};

/// Descent-to-advance ratio past which a cutting move is really a plunge.
///
/// Two conditions make a plunge, not one. Steepness alone is safe: a ball riding
/// down a near-vertical but *smooth* face descends faster than it advances and is
/// fully supported. Depth alone is safe too: a long gentle descent is just a cut.
/// The hazard is both together -- dropping a long way while barely advancing, which
/// engages the end of the cutter rather than its side. Ten is about 84 degrees;
/// real surfaces measured on the router jig top out near 3.
pub const PLUNGE_RATIO: f64 = 10.0;

/// Material further above the part than this, in millimetres, counts as left over.
///
/// Leftover is not a failure; it is where the cutter could not reach. It is named
/// here so that anything drawing the material classifies it by the same rule the
/// verifier counts it by.
pub const LEFTOVER_LIMIT: f64 = 1.0;

/// A job that could not be run.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The job was not valid.
    #[error(transparent)]
    Job(#[from] JobError),
    /// A strategy failed.
    #[error("operation '{op}': {source}")]
    Strategy {
        /// Which operation.
        op: String,
        /// Why.
        source: StrategyError,
    },
    /// Linking failed.
    #[error("operation '{op}': {source}")]
    Link {
        /// Which operation.
        op: String,
        /// Why.
        source: LinkError,
    },
    /// A cutter-location map could not be built.
    #[error("operation '{op}': {source}")]
    ClMap {
        /// Which operation.
        op: String,
        /// Why.
        source: ClMapError,
    },
    /// The stock could not be built.
    #[error("cannot build stock: {0}")]
    Stock(#[from] StockError),
    /// A tool dimension was not usable.
    #[error(transparent)]
    Tool(#[from] ToolError),
    /// Stock to leave was not usable.
    #[error(transparent)]
    ToLeave(#[from] ToLeaveError),
    /// A move could not be added.
    #[error(transparent)]
    Toolpath(#[from] ToolpathError),
    /// The model had no geometry.
    #[error("model has no geometry")]
    EmptyModel,
}

/// What a strategy reported, in its own terms.
///
/// Carried through rather than flattened into warnings, so that anything looking
/// at a result -- the inspector above all -- sees the same numbers the run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyDetail {
    /// Raster finishing.
    Parallel(StrategyReport),
    /// Z-level clearing.
    ZlevelRough(RoughingReport),
}

/// What one operation produced.
#[derive(Debug, Clone)]
pub struct OperationResult {
    /// The operation's name.
    pub name: String,
    /// Which strategy produced it, for the program's section comment.
    pub strategy: &'static str,
    /// The machinable path.
    pub path: Toolpath,
    /// Tool number, for the post.
    pub tool_number: u32,
    /// Spindle speed.
    pub rpm: u32,
    /// Tool description.
    pub tool_comment: String,
    /// What the simulator saw while this operation ran.
    pub sim: SimReport,
    /// Cutting passes produced.
    pub passes: u64,
    /// Inter-pass travel removed by reordering, as a fraction.
    pub travel_saved: f64,
    /// What the strategy reported.
    pub detail: StrategyDetail,
    /// What linking did.
    pub links: LinkReport,
    /// The clearance height, resolved. Traverses fly over the material by the
    /// clearance offset, and never lower than this where the material is unknown.
    pub clearance_z: f64,
    /// Cutting moves steeper than [`PLUNGE_RATIO`].
    pub steep_cut_descents: u64,
    /// Deepest of those, in millimetres.
    pub deepest_cut_descent_mm: f64,
    /// Anything the caller should look at.
    pub warnings: Vec<String>,
}

impl OperationResult {
    /// Whether this operation is safe to run as it stands.
    pub fn is_clean(&self) -> bool {
        self.sim.is_clean() && !self.path.has_unplanned_links()
    }
}

/// What the whole job produced.
#[derive(Debug, Clone)]
pub struct JobResult {
    /// Each operation, in order.
    pub operations: Vec<OperationResult>,
    /// The material as it stands at the end.
    pub stock: SimStock,
    /// The part the job was supposed to produce.
    pub part: PartSurface,
    /// How the finished stock compares with the part.
    pub verify: VerifyReport,
    /// Tolerance the comparison allowed.
    pub verify_budget: f64,
    /// Footprint of the block in XY, lower corner.
    pub stock_min: DVec2,
    /// Footprint of the block in XY, upper corner.
    pub stock_max: DVec2,
    /// The heights symbolic references resolved against.
    pub heights: HeightContext,
    /// The material before the first operation, then after each: one more than
    /// there are operations, the last equal to `stock`. The keyframes a replay
    /// starts from.
    pub stages: Vec<SimStock>,
}

impl JobResult {
    /// Whether the whole job verified.
    pub fn is_clean(&self) -> bool {
        self.verify.is_clean() && self.operations.iter().all(OperationResult::is_clean)
    }

    /// Everything worth a person's attention, across every operation.
    pub fn warnings(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .operations
            .iter()
            .flat_map(|o| o.warnings.iter().map(|w| format!("{}: {w}", o.name)))
            .collect();
        if self.verify.gouged > 0 {
            out.push(format!(
                "finished stock is below the part in {} column(s), deepest {:.3} mm \
                 against a {:.3} mm budget",
                self.verify.gouged, self.verify.deepest_gouge_mm, self.verify_budget
            ));
        }
        out
    }
}

/// Run every operation of a job against one block of material.
pub fn run_job(job: &Job, mesh: &TriMesh) -> Result<JobResult, RunError> {
    job.validate()?;
    let bounds = mesh.bounds().ok_or(RunError::EmptyModel)?;

    let StockSpec::Box { margin } = job.setup.stock;
    // Z margin grows upward only. Material below the model is the part of the block
    // the cutter never reaches, so growing downward would make the simulation taller
    // without changing anything.
    let lo = dvec2(bounds.min.x - margin[0], bounds.min.y - margin[1]);
    let hi = dvec2(bounds.max.x + margin[0], bounds.max.y + margin[1]);
    let stock_top = bounds.max.z + margin[2];
    let stock_bottom = bounds.min.z;

    let ctx = HeightContext {
        stock_top,
        stock_bottom,
        model_top: bounds.max.z,
        model_bottom: bounds.min.z,
    };

    // One grid pitch for the material, derived from the tolerance rather than
    // guessed, and bounded so a fine tolerance on a large part cannot allocate
    // without limit.
    let sim_pitch = (job.tolerance * 8.0).clamp(0.05, 1.0);
    let mut stock = SimStock::block(lo, hi, stock_top, stock_bottom - 1.0, sim_pitch)?;
    let cutter = DropCutter::new(mesh);

    let mut operations = Vec::with_capacity(job.operations.len());
    let mut stages = Vec::with_capacity(job.operations.len() + 1);
    stages.push(stock.clone());
    for op in &job.operations {
        operations.push(run_operation(
            job, op, &cutter, &ctx, lo, hi, &mut stock, sim_pitch,
        )?);
        stages.push(stock.clone());
    }

    let part = PartSurface::from_mesh(mesh, lo, hi, sim_pitch)?;
    // The simulator resolves the surface on a grid, so it cannot be held finer than
    // one cell plus the chordal tolerance the paths were built to.
    let budget = job.tolerance + sim_pitch;
    let verify = stock.verify(&part, budget, LEFTOVER_LIMIT);

    Ok(JobResult {
        operations,
        stock,
        part,
        verify,
        verify_budget: budget,
        stock_min: lo,
        stock_max: hi,
        heights: ctx,
        stages,
    })
}

/// The tool an operation cuts with, as the simulator sweeps it: the job's cutter,
/// grown by the operation's stock to leave.
///
/// The one place this is built, so a replay of the material sweeps exactly the
/// tool the run did.
pub fn cutting_tool(job: &Job, op: &Operation) -> Result<EffectiveTool, RunError> {
    let spec = job.tool_for(op).ok_or_else(|| JobError::UnknownTool {
        op: op.name.clone(),
        tool: op.tool.clone(),
    })?;
    let base = spec.build()?;
    let leave = ToLeave::new(
        op.passes.stock_to_leave.radial,
        op.passes.stock_to_leave.axial,
    )?;
    Ok(EffectiveTool::new(&base, leave)?)
}

#[allow(clippy::too_many_arguments)]
fn run_operation(
    job: &Job,
    op: &Operation,
    cutter: &DropCutter<'_>,
    ctx: &HeightContext,
    lo: DVec2,
    hi: DVec2,
    stock: &mut SimStock,
    sim_pitch: f64,
) -> Result<OperationResult, RunError> {
    let spec = job.tool_for(op).ok_or_else(|| JobError::UnknownTool {
        op: op.name.clone(),
        tool: op.tool.clone(),
    })?;
    let tool = cutting_tool(job, op)?;
    let mut warnings = Vec::new();

    // The sweep covers the model plus the tool, plus whatever extra was asked for.
    let reach = tool.max_radius() + op.geometry.margin;
    let (sweep_lo, sweep_hi) = (
        dvec2(lo.x - reach, lo.y - reach),
        dvec2(hi.x + reach, hi.y + reach),
    );

    let (raw, passes, detail) = match op.strategy {
        StrategySpec::Parallel { angle } => {
            let params = ParallelParams {
                stepover: op.passes.stepover,
                angle: angle.to_radians(),
                tolerance: job.tolerance,
                feed: op.passes.feed,
                max_cut_step: op.passes.max_cut_step,
                ..Default::default()
            };
            let (path, report) = parallel_finish(cutter, &tool, sweep_lo, sweep_hi, &params)
                .map_err(|source| RunError::Strategy {
                    op: op.name.clone(),
                    source,
                })?;
            if !report.is_clean() {
                warnings.push(format!(
                    "{} span(s) hit the refinement limit; the path is coarser than the \
                     requested tolerance",
                    report.refinement_limited
                ));
            }
            (path, report.passes, StrategyDetail::Parallel(report))
        }
        StrategySpec::ZlevelRough {
            stepdown,
            min_ring_length,
        } => {
            let cl =
                ClMap::compute(cutter, &tool, sweep_lo, sweep_hi, sim_pitch).map_err(|source| {
                    RunError::ClMap {
                        op: op.name.clone(),
                        source,
                    }
                })?;
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
            let (path, report) =
                zlevel_rough(&cl, stock.field(), &params).map_err(|source| RunError::Strategy {
                    op: op.name.clone(),
                    source,
                })?;
            if report.rings == 0 {
                warnings.push(
                    "found nothing to clear: the stock may already be below this \
                     operation's top"
                        .into(),
                );
            }
            (path, report.rings, StrategyDetail::ZlevelRough(report))
        }
    };

    // Ordering is a Toolpath -> Toolpath transform, applied before linking so the
    // retracts it needs are planned around the order actually used.
    let (ordered, order) = if op.linking.reorder {
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
    .map_err(|source| RunError::Link {
        op: op.name.clone(),
        source,
    })?;

    let (path, links) = link(
        &ordered,
        &clearance,
        &LinkParams {
            retract_feed: op.linking.retract_feed,
            plunge_feed: op.linking.plunge_feed,
            tool_radius: tool.max_radius(),
        },
    )
    .map_err(|source| RunError::Link {
        op: op.name.clone(),
        source,
    })?;
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
    let sim = stock.apply(&sim_moves(&path), &tool);
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
            "deepest cutting descent {deepest:.2} mm exceeds the {:.2} mm allowance \
             ({steep} steep move(s))",
            op.passes.max_cut_step
        ));
    }

    Ok(OperationResult {
        name: op.name.clone(),
        strategy: match op.strategy {
            StrategySpec::Parallel { .. } => "parallel",
            StrategySpec::ZlevelRough { .. } => "zlevel-rough",
        },
        path,
        tool_number: spec.number,
        rpm: spec.rpm,
        tool_comment: spec.comment.clone(),
        sim,
        passes,
        travel_saved: order.saved(),
        detail,
        links,
        clearance_z,
        steep_cut_descents: steep,
        deepest_cut_descent_mm: deepest,
        warnings,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests {
    use super::*;
    use camcraft_geom::synthetic;

    const JOB: &str = r#"
tolerance = 0.05

[setup]
stock = { kind = "box", margin = [1.0, 1.0, 2.0] }

[tools.rough]
kind = "flat"
diameter = 6.0
number = 1

[tools.finish]
kind = "ball"
diameter = 4.0
number = 2

[[operation]]
name = "rough"
strategy = "zlevel-rough"
tool = "rough"
stepdown = 2.5
passes = { stepover = 3.0, feed = 1800.0, stock_to_leave = { radial = 0.5, axial = 0.5 } }
heights = { clearance = { ref = "stock-top", offset = 6.0 } }

[[operation]]
name = "finish"
strategy = "parallel"
tool = "finish"
passes = { stepover = 0.8, feed = 2000.0 }
heights = { clearance = { ref = "stock-top", offset = 6.0 } }
"#;

    fn dome() -> camcraft_geom::mesh::TriMesh {
        synthetic::uv_sphere(8.0, 100, 140)
    }

    #[test]
    fn runs_both_operations_in_order() {
        let job: Job = toml::from_str(JOB).unwrap();
        let result = run_job(&job, &dome()).unwrap();

        assert_eq!(result.operations.len(), 2);
        let names: Vec<&str> = result.operations.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, vec!["rough", "finish"], "order must be preserved");
        for o in &result.operations {
            assert!(
                !o.path.has_unplanned_links(),
                "{} is not machinable",
                o.name
            );
            assert!(o.passes > 0, "{} produced no passes", o.name);
        }
    }

    /// Whatever looks at a result sees the run's own reports, not a recomputation.
    #[test]
    fn results_carry_what_the_run_reported() {
        let job: Job = toml::from_str(JOB).unwrap();
        let result = run_job(&job, &dome()).unwrap();
        let [rough, finish] = result.operations.as_slice() else {
            panic!("expected two operations");
        };

        let StrategyDetail::ZlevelRough(r) = rough.detail else {
            panic!("rough reported {:?}", rough.detail);
        };
        assert_eq!(r.rings, rough.passes);
        let StrategyDetail::Parallel(p) = finish.detail else {
            panic!("finish reported {:?}", finish.detail);
        };
        assert_eq!(p.passes, finish.passes);

        // Clearance resolved against this job's block: stock-top plus 6.
        assert!((result.heights.stock_top - (8.0 + 2.0)).abs() < 1e-6);
        for o in &result.operations {
            assert!((o.clearance_z - (result.heights.stock_top + 6.0)).abs() < 1e-9);
            assert!(o.links.links_resolved > 0, "{} resolved no links", o.name);
        }
        assert!(result.stock_min.x < -8.0 && result.stock_max.x > 8.0);
    }

    /// The point of threading: the second operation sees what the first removed.
    #[test]
    fn roughing_removes_material_before_finishing_runs() {
        let job: Job = toml::from_str(JOB).unwrap();
        let result = run_job(&job, &dome()).unwrap();
        let rough = result.operations.first().unwrap();
        assert!(
            rough.sim.removed_mm3 > 0.0,
            "roughing should remove material"
        );
    }

    /// Every operation is simulated as it runs, and nothing may traverse material.
    #[test]
    fn no_operation_rapids_through_stock() {
        let job: Job = toml::from_str(JOB).unwrap();
        let result = run_job(&job, &dome()).unwrap();
        for o in &result.operations {
            assert!(
                o.sim.is_clean(),
                "{} put {} rapid(s) through material, deepest {:.3} mm",
                o.name,
                o.sim.rapid_contacts,
                o.sim.deepest_rapid_contact_mm
            );
        }
    }

    /// The whole job must land on the part.
    #[test]
    fn the_finished_result_is_the_part() {
        let job: Job = toml::from_str(JOB).unwrap();
        let result = run_job(&job, &dome()).unwrap();
        assert!(result.verify.compared > 500);
        assert!(
            result.is_clean(),
            "job did not verify: {:?}",
            result.warnings()
        );
    }

    /// A validation failure must stop the job before anything is computed.
    #[test]
    fn an_invalid_job_never_starts() {
        let bad = JOB.replace("stepdown = 2.5", "stepdown = 0.0");
        let job: Job = toml::from_str(&bad).unwrap();
        assert!(matches!(run_job(&job, &dome()), Err(RunError::Job(_))));
    }

    #[test]
    fn an_empty_model_is_refused() {
        let job: Job = toml::from_str(JOB).unwrap();
        let empty = camcraft_geom::mesh::TriMesh::default();
        assert!(matches!(run_job(&job, &empty), Err(RunError::EmptyModel)));
    }
}
