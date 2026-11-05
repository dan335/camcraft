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
//! A 3D operation whose boundary is its selection is kept to what its picked
//! faces and edges enclose ([`Picking::machining_boundary`]) as well as to the
//! stock's footprint. Picks nothing of the operation reads are kept, and never
//! found.

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
use std::sync::mpsc::{Receiver, channel};

use camcraft_sim::stock::{PackedStock, PartSurface, Stock};
use glam::DVec2;

use crate::control::{Control, Watch};
use crate::cutter::allowance::{Allowance, EffectiveTool};
use crate::cutter::dropcutter::DropCutter;
use crate::job::{
    HeightContext, HolderSegment, Job, JobError, Machine, Operation, Setup, ToolSpec,
};
use crate::path::simadapt::sim_moves;
use crate::setup::features::Picking;
use crate::setup::{Bodies, Prepared};
use crate::strategy::adaptive::rest::Earlier;

pub use crate::path::toolpath::PLUNGE_RATIO;
pub use error::RunError;
pub use operation::Planned;
pub use result::{
    Contact, Kept, Made, OperationResult, Plan, Progress, SetupResult, Stage, StrategyDetail,
};

use error::in_op;
use operation::{
    Target, Work, cut_operation, drill_holes, heights_of, not_made, plan_operation, planar_areas,
};
use pitch::sim_pitch;

/// Material further above the part than this, in millimetres, counts as left over.
///
/// Leftover is not a failure; it is where the cutter could not reach. It is named
/// here so that anything drawing the material classifies it by the same rule the
/// verifier counts it by.
pub const LEFTOVER_LIMIT: f64 = 1.0;

/// Check, lay out and run setup `index` of `job`, with its models by name.
#[cfg(test)]
pub(crate) fn run_setup(
    job: &Job,
    index: usize,
    bodies: &Bodies<'_>,
) -> Result<SetupResult, RunError> {
    job.check_common()?;
    job.check_setup(index)?;
    let prepared = crate::setup::prepare(job, index, bodies)?;
    let operations = job.setups.get(index).map_or(0, |s| s.operations.len());
    run_prepared_planned(
        job,
        index,
        &prepared,
        bodies,
        &[],
        &Control::new(operations),
        &mut |_| {},
    )
}

/// How much of an operation working out its passes is, as it says how far it
/// has got: the rest is linking them and cutting them into the material.
const PLANNING: f64 = 0.85;

/// Run every operation of setup `index` against its block of material, already
/// laid out in its frame, telling `progress` what it starts on and each
/// operation as it is made ([`Progress`]), and `control` how far each has got.
/// The job must have been checked. `bodies` are the models it was laid out
/// from, where picked faces and edges are found.
///
/// Each operation is done as `plans` says, by its place: an operation with no
/// plan is worked out, and one turned off is left out whatever its plan. Kept
/// operations are cut into the material again, so what comes after them is
/// planned against what they leave.
///
/// Every operation's passes that are worked out from the part alone are
/// worked out at once, each on a thread of its own; one that clears what the
/// ones before it left ([`Uses::rest`](crate::job::Uses::rest)) waits for
/// them. Each is linked over the material and cut into it in order, as soon as
/// its passes and the ones before it are done, so the result is the same
/// however the threads go.
///
/// Stopped from an operation on (`control`), the run ends there with
/// [`RunError::Stopped`]: every operation before it was made, and told.
pub fn run_prepared_planned(
    job: &Job,
    index: usize,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    plans: &[Plan],
    control: &Control,
    progress: &mut dyn FnMut(Progress<'_>),
) -> Result<SetupResult, RunError> {
    let setup = job.setups.get(index).ok_or(JobError::NoSetups)?;
    let all = prepared.stock_bounds();
    let lo = all.min.truncate();
    let hi = all.max.truncate();
    let ctx = prepared.heights();

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
    let mut stock = Stock::from_boxes(&blocks, ctx.stock_bottom - 1.0, sim_pitch)?;
    // What an operation worked out at once with the ones before it is given:
    // the stock uncut, which none of them reads.
    let uncut = stock.clone();
    let cutter = prepared.drop_cutter();
    let shared = Shared {
        job,
        prepared,
        bodies,
        cutter: &cutter,
        bounds: (lo, hi),
        blocks: &blocks,
        heights: &ctx,
        nose: job.machine_for(setup).and_then(Machine::nose),
    };
    let plan_of = |i: usize| plans.get(i).unwrap_or(&Plan::Generate);
    // Smoothing strays from the passes by up to its tolerance, on top of the
    // job's, as Fusion's total tolerance is the two together.
    let smoothing = setup
        .operations
        .iter()
        .filter_map(|op| op.smoothing(job.tolerance))
        .fold(0.0, f64::max);
    let budget = job.tolerance + smoothing + sim_pitch;

    let ran = std::thread::scope(|scope| {
        // Every operation worked out from the part alone, at once.
        let mut early: Vec<Option<Receiver<Planning<'_>>>> =
            setup.operations.iter().map(|_| None).collect();
        for (i, op) in setup.operations.iter().enumerate() {
            let made_here = op.enabled && matches!(plan_of(i), Plan::Generate);
            if !made_here || op.strategy.uses().rest {
                continue;
            }
            let (tx, rx) = channel();
            if let Some(slot) = early.get_mut(i) {
                *slot = Some(rx);
            }
            let (shared, uncut) = (&shared, &uncut);
            let watch = control.watch(i).part(0.0, PLANNING);
            scope.spawn(move || {
                // Gone only if the run ended first, when the answer is wanted
                // by no one.
                let _ = tx.send(shared.plan(op, watch, uncut, &[]));
            });
            progress(Progress::Stage(Stage::Operation(i)));
        }
        let ran = shared.run(
            setup,
            (plan_of, &mut early),
            (&mut stock, sim_pitch, budget),
            control,
            progress,
        );
        // Whatever ended it, nothing still being worked out is wanted.
        if ran.is_err() {
            control.stop();
        }
        ran
    });
    let (operations, keyframes, base) = match ran {
        Ok(ran) => ran,
        Err(e) if e.is_stopped() => return Err(RunError::Stopped),
        Err(e) => return Err(e),
    };

    // The simulator resolves the surface on a grid, so it cannot be held finer than
    // one cell plus the chordal tolerance the paths were built to; and that holds
    // across as well as down, or every vertical wall is a gouge beside it.
    progress(Progress::Stage(Stage::Verifying));
    let part = cut_past(setup, base, &keyframes)?;
    let part = Arc::new(part.reaching(budget));
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

/// An operation's passes, worked out, with what they were worked out with.
type Planning<'j> = Result<(Tooled<'j>, Planned), RunError>;

/// What an operation is worked out and cut with, found once for it: its tool,
/// as the job describes it and as it cuts, and what its heights resolve
/// against.
struct Tooled<'j> {
    spec: &'j ToolSpec,
    tool: EffectiveTool,
    heights: HeightContext,
}

/// What a setup's operations share while it runs.
struct Shared<'a> {
    job: &'a Job,
    prepared: &'a Prepared,
    bodies: &'a Bodies<'a>,
    cutter: &'a DropCutter<'a>,
    bounds: (DVec2, DVec2),
    blocks: &'a [(DVec2, DVec2, f64)],
    heights: &'a HeightContext,
    /// What holds a tool that has no holder of its own: the setup's
    /// machine's spindle nose, where it has one.
    nose: Option<&'a [HolderSegment]>,
}

/// The material a run cuts into, the grid's pitch it is simulated at, and the
/// budget it is verified to.
type Material<'s> = (&'s mut Stock, f64, f64);

/// The material after each operation, the first the stock uncut.
type Keyframes = Vec<Arc<PackedStock>>;

impl<'a> Shared<'a> {
    /// Operation `op`'s work, with `tooled`, telling `watch`.
    fn work<'s>(
        &'s self,
        op: &'s Operation,
        tooled: &'s Tooled<'a>,
        before: &'s [Earlier],
        watch: Watch,
    ) -> Work<'s> {
        Work {
            before,
            job: self.job,
            op,
            spec: tooled.spec,
            tool: &tooled.tool,
            heights: &tooled.heights,
            prepared: self.prepared,
            cutter: self.cutter,
            bounds: self.bounds,
            blocks: self.blocks,
            watch,
            nose: self.nose,
        }
    }

    /// Work out `op`'s passes, telling `watch`, over `stock`, the material as
    /// it stands, and after `before`, the operations before it as cut, which
    /// only a strategy that clears what is left reads.
    fn plan(
        &self,
        op: &'a Operation,
        watch: Watch,
        stock: &Stock,
        before: &[Earlier],
    ) -> Planning<'a> {
        // Its picks, found once, however many of its settings read them.
        let picking = self.picking(op);
        let Tooled {
            spec,
            tool,
            heights,
        } = self.tooled(op, &picking)?;
        // A drilling operation's picked edges are its holes, and a 2D one's
        // outline is what it cuts round or clears, not a limit.
        let uses = op.strategy.uses();
        let target = if uses.boundary {
            Target::Limit(
                picking
                    .machining_boundary(&op.boundary, tool.cutter_radius())
                    .map_err(in_op(op))?,
            )
        } else if uses.outline {
            Target::Areas(planar_areas(
                self.job,
                op,
                &picking,
                self.bodies,
                &heights,
                &tool,
            )?)
        } else if uses.holes {
            Target::Holes(drill_holes(
                self.job,
                op,
                self.prepared,
                self.bodies,
                &heights,
            )?)
        } else {
            // Facing works from the stock alone.
            Target::Limit(None)
        };
        let tooled = Tooled {
            spec,
            tool,
            heights,
        };
        let planned = plan_operation(&self.work(op, &tooled, before, watch), &target, stock)?;
        Ok((tooled, planned))
    }

    /// What `op` picks on the part, found when first asked.
    fn picking(&self, op: &'a Operation) -> Picking<'a> {
        Picking::new(
            &op.selection,
            self.prepared,
            self.bodies,
            self.job.tolerance,
        )
    }

    /// What `op` is worked out and cut with: its tool, and its heights,
    /// resolved against its picks.
    fn tooled(&self, op: &Operation, picking: &Picking<'_>) -> Result<Tooled<'a>, RunError> {
        let (spec, tool) = tool_of(self.job, op)?;
        let heights = heights_of(op, picking, self.heights)?;
        Ok(Tooled {
            spec,
            tool,
            heights,
        })
    }

    /// Cut each of `setup`'s operations into the material in order, each as
    /// `plan_of` says, those worked out already (`early`) waited for, telling
    /// `progress` each as it is made. Each operation's result, the material
    /// after each, and the part as the simulator sees it.
    fn run<'p>(
        &self,
        setup: &'a Setup,
        (plan_of, early): (
            impl Fn(usize) -> &'p Plan,
            &mut [Option<Receiver<Planning<'a>>>],
        ),
        (stock, sim_pitch, budget): Material<'_>,
        control: &Control,
        progress: &mut dyn FnMut(Progress<'_>),
    ) -> Result<(Vec<OperationResult>, Keyframes, PartSurface), RunError> {
        let job = self.job;
        let (lo, hi) = self.bounds;
        // The part as the simulator sees it: worked out while the operations'
        // passes are, and what a result is drawn against from its first.
        let base = (*self.prepared.part_surface(sim_pitch)?).clone();
        let mut keyframes = vec![Arc::new(PackedStock::pack(stock))];
        let begun = SetupResult {
            setup: setup.name.clone(),
            frame: self.prepared.frame,
            operations: Vec::new(),
            part: Arc::new(base.clone().reaching(budget)),
            verify: Default::default(),
            verify_budget: budget,
            stocks: self.prepared.stocks.clone(),
            stock_min: lo,
            stock_max: hi,
            heights: *self.heights,
            keyframes: keyframes.clone(),
        };
        progress(Progress::Begun(&begun));

        let mut operations: Vec<OperationResult> = Vec::with_capacity(setup.operations.len());
        // Whether the material is as the run the kept operations came from
        // left it here: so long as every operation so far was kept, it is.
        let mut as_before = true;
        for (i, op) in setup.operations.iter().enumerate() {
            let watch = control.watch(i);
            if watch.stopped() {
                return Err(RunError::Stopped);
            }
            let plan = if op.enabled { plan_of(i) } else { &Plan::Skip };
            match plan {
                Plan::Skip => {
                    let made = if op.enabled {
                        Made::Skipped
                    } else {
                        Made::Disabled
                    };
                    operations.push(not_made(op, job.tool_for(op), made));
                    // Nothing cut: the material is as it was, and so is its
                    // keyframe; but not as it was when it was made.
                    let same = keyframes
                        .last()
                        .map_or_else(|| Arc::new(PackedStock::pack(stock)), Arc::clone);
                    keyframes.push(same);
                    as_before = false;
                }
                Plan::Keep(kept) => {
                    progress(Progress::Stage(Stage::Cutting(i)));
                    let mut made = kept.result.clone();
                    made.made = Made::Kept;
                    // The material it left, put back, where the material before
                    // it is what it cut, on the same grid; else it is cut
                    // again, into this material.
                    let after = kept
                        .after
                        .as_ref()
                        .filter(|after| as_before && after.restore(stock).is_some());
                    match after {
                        Some(after) => keyframes.push(Arc::clone(after)),
                        None => {
                            let (_, tool) = tool_of(job, op)?;
                            made.sim = stock.apply(&sim_moves(&made.path), &tool);
                            keyframes.push(Arc::new(PackedStock::pack(stock)));
                        }
                    }
                    operations.push(made);
                }
                Plan::Relink(planned) => {
                    progress(Progress::Stage(Stage::Cutting(i)));
                    let tooled = self.tooled(op, &self.picking(op))?;
                    let work = self.work(op, &tooled, &[], Watch::default());
                    let mut made = cut_operation(&work, planned, stock, &watch)?;
                    made.made = Made::Relinked;
                    operations.push(made);
                    keyframes.push(Arc::new(PackedStock::pack(stock)));
                    as_before = false;
                }
                Plan::Generate => {
                    let planned = match early.get_mut(i).and_then(Option::take) {
                        Some(rx) => rx.recv().unwrap_or_else(|_| {
                            Err(RunError::Lost {
                                op: op.name.clone(),
                            })
                        }),
                        None => {
                            progress(Progress::Stage(Stage::Operation(i)));
                            let before = if op.strategy.uses().rest {
                                earlier(job, setup, &operations)?
                            } else {
                                Vec::new()
                            };
                            self.plan(op, watch.part(0.0, PLANNING), stock, &before)
                        }
                    };
                    let (tooled, planned) = planned?;
                    progress(Progress::Stage(Stage::Cutting(i)));
                    let work = self.work(op, &tooled, &[], Watch::default());
                    let made = cut_operation(
                        &work,
                        &Arc::new(planned),
                        stock,
                        &watch.part(PLANNING, 1.0),
                    )?;
                    operations.push(made);
                    keyframes.push(Arc::new(PackedStock::pack(stock)));
                    as_before = false;
                }
            }
            watch.report(1.0);
            if let (Some(op), Some(after)) = (operations.last(), keyframes.last()) {
                progress(Progress::Made {
                    index: i,
                    op,
                    after,
                });
            }
            // Made, it is drawn as made: what it showed while worked out goes.
            control.forget_shown(i);
        }
        Ok((operations, keyframes, base))
    }
}

/// The part as the setup's operations mean to leave it: cut past where one with
/// negative stock to leave cut, which is then no gouge there, and nowhere else.
/// Where each cut is the material it took down, between the stock before it and
/// after (`keyframes`).
fn cut_past(
    setup: &Setup,
    mut part: PartSurface,
    keyframes: &[Arc<PackedStock>],
) -> Result<PartSurface, RunError> {
    for (i, op) in setup.operations.iter().enumerate() {
        let leave = op.stock_to_leave();
        let (radial, axial) = (-leave.radial.min(0.0), -leave.axial.min(0.0));
        if !op.enabled || (radial == 0.0 && axial == 0.0) {
            continue;
        }
        let (Some(before), Some(after)) = (keyframes.get(i), keyframes.get(i + 1)) else {
            continue;
        };
        let (before, after) = (before.unpack()?, after.unpack()?);
        let (before, after) = (before.field(), after.field());
        part = part.cut_past(radial, axial, |ix, iy| {
            matches!((before.get(ix, iy), after.get(ix, iy)), (Some(b), Some(a)) if a < b - 1e-6)
        });
    }
    Ok(part)
}

/// The tool an operation cuts with: the job's cutter, grown by the stock to
/// leave it cuts with to find where the tip goes, and swept as it is.
///
/// The one place this is built, so a replay of the material sweeps exactly the
/// tool the run did.
pub fn cutting_tool(job: &Job, op: &Operation) -> Result<EffectiveTool, RunError> {
    tool_of(job, op).map(|(_, tool)| tool)
}

/// The operations of `setup` made so far (`made`), as cut, each with the real
/// tool it was cut with: what a rest operation clears after.
fn earlier(job: &Job, setup: &Setup, made: &[OperationResult]) -> Result<Vec<Earlier>, RunError> {
    setup
        .operations
        .iter()
        .zip(made)
        .filter(|(_, r)| !r.path.is_empty())
        .map(|(op, r)| {
            let (_, tool) = tool_of(job, op)?;
            Ok(Earlier {
                path: Arc::clone(&r.path),
                cutter: *tool.base(),
            })
        })
        .collect()
}

/// The tool `op` names, both as the job describes it and as it cuts
/// ([`cutting_tool`]), looked up and built once.
fn tool_of<'j>(job: &'j Job, op: &Operation) -> Result<(&'j ToolSpec, EffectiveTool), RunError> {
    let spec = job.tool_for(op).ok_or_else(|| unknown_tool(op))?;
    let base = spec.build()?;
    let leave = op.stock_to_leave();
    let leave = Allowance::new(leave.radial, leave.axial)?;
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
