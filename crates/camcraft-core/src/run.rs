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

use camcraft_geom::contour::Region;
use camcraft_geom::mesh::{Aabb, TriMesh};
use camcraft_sim::stock::{PartSurface, SimReport, Stock as SimStock, StockError, VerifyReport};
use glam::{DVec2, dvec2};

use crate::dropcutter::DropCutter;
use crate::features::{FeatureError, machining_boundary};
use crate::frame::Frame;
use crate::job::{
    Coolant, HeightContext, Job, JobError, Operation, SpindleDirection, StrategySpec,
};
use crate::linker::{ClearanceField, LinkError, LinkParams, LinkReport, link};
use crate::ordering::order_passes;
use crate::parallel::{ParallelParams, StrategyError, StrategyReport, parallel_finish_within};
use crate::reach::{LevelReach, ReachError};
use crate::roughing::{RoughingParams, RoughingReport, zlevel_rough};
use crate::setup::{Bodies, PrepareError, Prepared, prepare};
use crate::simadapt::sim_moves;
use crate::stock::{EffectiveTool, Stock as ToLeave, StockError as ToLeaveError};
use crate::tool::{ToolError, ToolProfile};
use crate::toolpath::{Toolpath, ToolpathError};

pub use crate::toolpath::PLUNGE_RATIO;

/// Material further above the part than this, in millimetres, counts as left over.
///
/// Leftover is not a failure; it is where the cutter could not reach. It is named
/// here so that anything drawing the material classifies it by the same rule the
/// verifier counts it by.
pub const LEFTOVER_LIMIT: f64 = 1.0;

/// A setup that could not be run.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The job was not valid.
    #[error(transparent)]
    Job(#[from] JobError),
    /// The setup's models and stock could not be laid out.
    #[error(transparent)]
    Prepare(#[from] PrepareError),
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
    /// Where the cutter may not go could not be worked out from the model.
    #[error("operation '{op}': {source}")]
    Reach {
        /// Which operation.
        op: String,
        /// Why.
        source: ReachError,
    },
    /// What the operation was kept to could not be found or used.
    #[error("operation '{op}': {source}")]
    Feature {
        /// Which operation.
        op: String,
        /// Why.
        source: FeatureError,
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
    /// Which way the spindle turns.
    pub spindle: SpindleDirection,
    /// Coolant while cutting.
    pub coolant: Coolant,
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
    /// Continuous cutting descents steeper than [`PLUNGE_RATIO`].
    pub steep_cut_descents: u64,
    /// Deepest of those, in millimetres.
    pub deepest_cut_descent_mm: f64,
    /// How deep a steep descent may be before it is a plunge, in millimetres: the
    /// operation's `max_cut_step`.
    pub max_cut_step: f64,
    /// Anything the caller should look at.
    pub warnings: Vec<String>,
}

impl OperationResult {
    /// Whether this operation is safe to run as it stands.
    ///
    /// A plunge at cutting feed is as unsafe as a rapid through material, so a
    /// steep descent deeper than the allowance fails the operation, not just warns.
    pub fn is_clean(&self) -> bool {
        self.sim.is_clean()
            && !self.path.has_unplanned_links()
            && self.deepest_cut_descent_mm <= self.max_cut_step
    }
}

/// What one setup produced. Everything in it is in the setup's frame.
#[derive(Debug, Clone)]
pub struct SetupResult {
    /// Which setup.
    pub setup: String,
    /// How model coordinates became the setup's.
    pub frame: Frame,
    /// Each operation, in order.
    pub operations: Vec<OperationResult>,
    /// The material as it stands at the end.
    pub stock: SimStock,
    /// The part the setup was supposed to produce.
    pub part: PartSurface,
    /// How the finished stock compares with the part.
    pub verify: VerifyReport,
    /// Tolerance the comparison allowed.
    pub verify_budget: f64,
    /// Each block of stock.
    pub stocks: Vec<Aabb>,
    /// Footprint of all the stock in XY, lower corner.
    pub stock_min: DVec2,
    /// Footprint of all the stock in XY, upper corner.
    pub stock_max: DVec2,
    /// The heights symbolic references resolved against.
    pub heights: HeightContext,
    /// The material before the first operation, then after each: one more than
    /// there are operations, the last equal to `stock`. The keyframes a replay
    /// starts from.
    pub stages: Vec<SimStock>,
}

impl SetupResult {
    /// Whether the whole setup verified.
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

/// Check, lay out and run setup `index` of `job`, with its models by name.
pub fn run_setup(job: &Job, index: usize, bodies: &Bodies<'_>) -> Result<SetupResult, RunError> {
    job.check_common()?;
    job.check_setup(index)?;
    let prepared = prepare(job, index, bodies)?;
    run_prepared(job, index, &prepared, bodies)
}

/// Run every operation of setup `index` against its block of material, already
/// laid out in its frame. The job must have been checked. `bodies` are the models
/// it was laid out from, where picked faces and edges are found.
pub fn run_prepared(
    job: &Job,
    index: usize,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
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

    // One grid pitch for the material, derived from the tolerance rather than
    // guessed, and bounded so a fine tolerance on a large part cannot allocate
    // without limit.
    let sim_pitch = (job.tolerance * 8.0).clamp(0.05, 1.0);
    let blocks: Vec<(DVec2, DVec2, f64)> = prepared
        .stocks
        .iter()
        .map(|b| (b.min.truncate(), b.max.truncate(), b.max.z))
        .collect();
    let mut stock = SimStock::from_boxes(&blocks, stock_bottom - 1.0, sim_pitch)?;
    let cutter = DropCutter::new(mesh);

    let mut operations = Vec::with_capacity(setup.operations.len());
    let mut stages = Vec::with_capacity(setup.operations.len() + 1);
    stages.push(stock.clone());
    for op in &setup.operations {
        let tool = cutting_tool(job, op)?;
        let limit = machining_boundary(
            &op.geometry,
            prepared,
            bodies,
            tool.max_radius(),
            job.tolerance,
        )
        .map_err(|source| RunError::Feature {
            op: op.name.clone(),
            source,
        })?;
        operations.push(run_operation(
            job,
            op,
            mesh,
            &cutter,
            &ctx,
            (lo, hi),
            limit.as_ref(),
            &mut stock,
        )?);
        stages.push(stock.clone());
    }

    let part = PartSurface::from_mesh(mesh, lo, hi, sim_pitch)?;
    // The simulator resolves the surface on a grid, so it cannot be held finer than
    // one cell plus the chordal tolerance the paths were built to.
    let budget = job.tolerance + sim_pitch;
    let verify = stock.verify(&part, budget, LEFTOVER_LIMIT);

    Ok(SetupResult {
        setup: setup.name.clone(),
        frame: prepared.frame,
        operations,
        stock,
        part,
        verify,
        verify_budget: budget,
        stocks: prepared.stocks.clone(),
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
    let spec = job.tool_for(op).ok_or_else(|| unknown_tool(op))?;
    let base = spec.build()?;
    let leave = ToLeave::new(
        op.passes.stock_to_leave.radial,
        op.passes.stock_to_leave.axial,
    )?;
    Ok(EffectiveTool::of(&base, leave)?)
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

#[allow(clippy::too_many_arguments)]
fn run_operation(
    job: &Job,
    op: &Operation,
    mesh: &TriMesh,
    cutter: &DropCutter<'_>,
    ctx: &HeightContext,
    (lo, hi): (DVec2, DVec2),
    limit: Option<&Region>,
    stock: &mut SimStock,
) -> Result<OperationResult, RunError> {
    let spec = job.tool_for(op).ok_or_else(|| unknown_tool(op))?;
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
            let (path, report) = parallel_finish_within(
                cutter, &tool, sweep_lo, sweep_hi, limit, &params,
            )
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
            let mut reach =
                LevelReach::new(mesh, &tool, job.tolerance).map_err(|source| RunError::Reach {
                    op: op.name.clone(),
                    source,
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
            let strategy = |source| RunError::Strategy {
                op: op.name.clone(),
                source,
            };
            let sweep = Region::rectangle(sweep_lo, sweep_hi);
            let area = match limit {
                Some(l) => sweep
                    .intersection(l)
                    .map_err(|e| strategy(StrategyError::Region(e.to_string())))?,
                None => sweep,
            };
            let (path, report) =
                zlevel_rough(&mut reach, &area, stock.field(), &params).map_err(strategy)?;
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
            "a steep cutting descent of {deepest:.2} mm exceeds the {:.2} mm \
             allowance: a plunge at cutting feed ({steep} steep descent(s))",
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
        rpm: op.passes.rpm.unwrap_or(spec.rpm),
        tool_comment: spec.comment.clone(),
        spindle: spec.spindle,
        coolant: spec.coolant,
        sim,
        passes,
        travel_saved: order.saved(),
        detail,
        links,
        clearance_z,
        steep_cut_descents: steep,
        deepest_cut_descent_mm: deepest,
        max_cut_step: op.passes.max_cut_step,
        warnings,
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use camcraft_geom::synthetic;
    use camcraft_geom::topology::Body;

    const JOB: &str = r#"
version = 1
tolerance = 0.05

[tools.rough]
kind = "flat"
diameter = 6.0
number = 1

[tools.finish]
kind = "ball"
diameter = 4.0
number = 2

[[model]]
name = "dome"
path = "dome.stl"
units = "mm"

[[stock]]
name = "block"
kind = "around-models"
margin = [1.0, 1.0, 2.0]

[[setup]]
name = "top"
models = ["dome"]
stocks = ["block"]

[[setup.operation]]
name = "rough"
strategy = "zlevel-rough"
tool = "rough"
stepdown = 2.5
passes = { stepover = 3.0, feed = 1800.0, stock_to_leave = { radial = 0.5, axial = 0.5 } }
heights = { clearance = { ref = "stock-top", offset = 6.0 } }

[[setup.operation]]
name = "finish"
strategy = "parallel"
tool = "finish"
passes = { stepover = 0.8, feed = 2000.0 }
heights = { clearance = { ref = "stock-top", offset = 6.0 } }
"#;

    fn dome() -> camcraft_geom::mesh::TriMesh {
        synthetic::uv_sphere(8.0, 100, 140)
    }

    fn run_job(job: &Job, mesh: &camcraft_geom::mesh::TriMesh) -> Result<SetupResult, RunError> {
        let body = Body::derived(mesh.clone());
        let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
        run_setup(job, 0, &bodies)
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
    /// A plunge at cutting feed is refused like a rapid through material, not
    /// merely warned about. The strategy breaks passes before it gets here; this is
    /// the backstop, so it is tested directly.
    #[test]
    fn a_descent_past_the_allowance_fails_the_operation() {
        let job: Job = toml::from_str(JOB).unwrap();
        let result = run_job(&job, &dome()).unwrap();
        let mut finish = result.operations[1].clone();
        assert!(finish.is_clean(), "{:?}", finish.warnings);
        finish.deepest_cut_descent_mm = finish.max_cut_step + 0.01;
        assert!(!finish.is_clean());
    }

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

    /// A V-bit finishes the dome after roughing: its path runs, sweeps through the
    /// simulator as a cone, and the result verifies against the part.
    #[test]
    fn a_vbit_finishes_and_verifies() {
        let vbit = JOB.replace(
            "[tools.finish]\nkind = \"ball\"\ndiameter = 4.0\n",
            "[tools.finish]\nkind = \"cone\"\ndiameter = 6.0\nangle = 90.0\n",
        );
        assert_ne!(vbit, JOB, "the edit must land");
        let job: Job = toml::from_str(&vbit).unwrap();
        let result = run_job(&job, &dome()).unwrap();
        let finish = &result.operations[1];
        assert!(finish.passes > 0);
        assert!(finish.sim.is_clean(), "{:?}", finish.warnings);
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
        assert!(matches!(
            run_job(&job, &empty),
            Err(RunError::Prepare(PrepareError::EmptyModel(_)))
        ));
    }

    /// Turned over, the setup machines the other side: a dome standing 11 mm up
    /// and hanging 5 mm down shows its 5 mm side on top, and still verifies.
    #[test]
    fn a_flipped_setup_machines_the_underside() {
        let mut job: Job = toml::from_str(JOB).unwrap();
        job.setups[0].orientation = crate::frame::Orientation::default().flipped();
        let dome = dome().mapped(|p| p + glam::dvec3(0.0, 0.0, 3.0)).unwrap();
        let result = run_job(&job, &dome).unwrap();
        assert!((result.heights.model_top - 5.0).abs() < 1e-6);
        assert!((result.heights.model_bottom + 11.0).abs() < 1e-6);
        assert!(result.is_clean(), "{:?}", result.warnings());
    }

    /// Two setups start from their own stock: the second's first stage is a whole
    /// block, not what the first left.
    #[test]
    fn each_setup_starts_from_its_own_stock() {
        let mut job: Job = toml::from_str(JOB).unwrap();
        let mut second = job.setups[0].clone();
        second.name = "again".into();
        job.setups.push(second);
        let body = Body::derived(dome());
        let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
        let first = run_setup(&job, 0, &bodies).unwrap();
        let again = run_setup(&job, 1, &bodies).unwrap();
        assert_eq!(first.stages[0], again.stages[0]);
        assert_eq!(again.setup, "again");
    }

    /// Two blocks side by side make one stock: the taller one sets stock-top, and
    /// both are cut.
    #[test]
    fn two_blocks_are_one_stock() {
        let mut job: Job = toml::from_str(JOB).unwrap();
        job.stocks = vec![
            crate::job::StockDef {
                name: "left".into(),
                spec: crate::job::StockSpec::FixedBox {
                    corner: [-10.0, -10.0, -8.0],
                    size: [10.0, 20.0, 17.0],
                },
            },
            crate::job::StockDef {
                name: "right".into(),
                spec: crate::job::StockSpec::FixedBox {
                    corner: [0.0, -10.0, -8.0],
                    size: [10.0, 20.0, 18.0],
                },
            },
        ];
        job.setups[0].stocks = vec!["left".into(), "right".into()];
        let result = run_job(&job, &dome()).unwrap();
        assert_eq!(result.stocks.len(), 2);
        assert_eq!(result.heights.stock_top, 10.0);
        assert_eq!(result.stock_min, dvec2(-10.0, -10.0));
        assert_eq!(result.stock_max, dvec2(10.0, 10.0));
        assert!(result.is_clean(), "{:?}", result.warnings());
    }

    /// The step as the "dome" of `JOB`, with one operation left: `finish` or
    /// `rough`, kept to the plateau.
    fn kept_to_plateau(keep: &str, containment: crate::job::Containment) -> (Job, Body) {
        use crate::features::find_face;
        use crate::job::FeatureRef;
        let body = Body::derived(synthetic::step(5.0, 10.0));
        let mut job: Job = toml::from_str(JOB).unwrap();
        job.setups[0].operations.retain(|op| op.name == keep);
        let pick = FeatureRef {
            model: "dome".into(),
            index: 0,
            at: [-5.0, 0.0, 5.0],
        };
        let face = find_face(&body, &pick, 0.01).unwrap().index;
        let g = &mut job.setups[0].operations[0].geometry;
        g.faces.push(FeatureRef {
            index: face,
            ..pick
        });
        g.containment = containment;
        (job, body)
    }

    fn cuts(result: &SetupResult) -> Vec<DVec2> {
        result.operations[0]
            .path
            .moves()
            .iter()
            .filter(|m| m.role == crate::toolpath::MoveRole::Cut)
            .map(|m| m.to.truncate())
            .collect()
    }

    /// Finishing kept to the plateau: every cut has the tool's centre over it, and
    /// with the whole tool inside, a radius in from its edges.
    #[test]
    fn parallel_keeps_to_a_picked_face() {
        use crate::job::Containment;
        for (containment, inset) in [(Containment::Center, 0.0), (Containment::Inside, 2.0)] {
            let (job, body) = kept_to_plateau("finish", containment);
            let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
            let result = run_setup(&job, 0, &bodies).unwrap();
            let cuts = cuts(&result);
            assert!(cuts.len() > 20, "{containment:?}: only {} cuts", cuts.len());
            let slack = 1e-6 + 0.01;
            for p in &cuts {
                assert!(
                    p.x <= -inset + slack
                        && p.x >= -10.0 + inset - slack
                        && p.y.abs() <= 10.0 - inset + slack,
                    "{containment:?}: a cut at {p} leaves the plateau"
                );
            }
            // And the passes reach across it, not a stepover short.
            let reach = cuts.iter().map(|p| p.x).fold(f64::MIN, f64::max);
            assert!(
                reach > -inset - 0.1,
                "{containment:?}: stops at x = {reach}"
            );
        }
        // Outside, the tool's side reaches the plateau from past its edge.
        let (job, body) = kept_to_plateau("finish", Containment::Outside);
        let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
        let result = run_setup(&job, 0, &bodies).unwrap();
        let far = cuts(&result).iter().map(|p| p.x).fold(f64::MIN, f64::max);
        assert!(far > 1.5, "outside stops at x = {far}");
    }

    /// Roughing kept to the plateau clears only around it, and less than the whole
    /// block.
    #[test]
    fn roughing_keeps_to_a_picked_face() {
        use crate::job::Containment;
        let (job, body) = kept_to_plateau("rough", Containment::Center);
        let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
        let kept = run_setup(&job, 0, &bodies).unwrap();
        for p in cuts(&kept) {
            assert!(
                p.x <= 0.01 && p.x >= -10.01 && p.y.abs() <= 10.01,
                "a roughing cut at {p} leaves the plateau"
            );
        }
        let mut whole = job.clone();
        whole.setups[0].operations[0].geometry.faces.clear();
        let all = run_setup(&whole, 0, &bodies).unwrap();
        assert!(
            kept.operations[0].sim.removed_mm3 < all.operations[0].sim.removed_mm3,
            "kept {} vs whole {}",
            kept.operations[0].sim.removed_mm3,
            all.operations[0].sim.removed_mm3
        );
    }

    /// A pick on nothing stops the run and names the operation.
    #[test]
    fn a_gone_pick_stops_the_run() {
        let (mut job, body) = kept_to_plateau("finish", crate::job::Containment::Center);
        job.setups[0].operations[0].geometry.faces[0].at = [40.0, 40.0, 40.0];
        let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
        let err = run_setup(&job, 0, &bodies).unwrap_err();
        assert!(
            matches!(err, RunError::Feature { ref op, .. } if op == "finish"),
            "{err}"
        );
    }

    /// Kept to a boundary, the same job gives the same moves on any number of
    /// threads.
    #[test]
    fn bounded_runs_are_deterministic() {
        let (job, body) = kept_to_plateau("rough", crate::job::Containment::Center);
        let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
        let on = |threads: usize| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap()
                .install(|| run_setup(&job, 0, &bodies).unwrap())
        };
        let (one, eight) = (on(1), on(8));
        assert_eq!(one.operations[0].path, eight.operations[0].path);
    }
}
