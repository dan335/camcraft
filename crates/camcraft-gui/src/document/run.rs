//! Running a job's setups, and what came of it: what a run needs and what it does
//! with each operation, where each setup and operation stands, and each setup laid
//! out for drawing.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use camcraft_core::job::{Job, ModelDef, Origin, Setup, StockDef, ToolSpec};
use camcraft_core::run::{Made, OperationResult, Plan, Stage};
use camcraft_core::setup::{Bodies, PrepareError, Prepared, prepare};
use camcraft_session::{Model, Ran, SessionError, check_runnable};
use glam::DVec3;

use super::{Document, Layout, Level, ModelKey, ModelSlot, RanSetup, next_key};
use crate::view::scene::{Playhead, Scene, box_edges};
use crate::view::viewport::{Outline, OutlineKind};

/// Everything a setup's run was made from. When this differs from what the job
/// holds now, the result is out of date.
#[derive(Debug, Clone, PartialEq)]
pub struct SetupInputs {
    tolerance: u64,
    setup: Setup,
    tools: BTreeMap<String, ToolSpec>,
    stocks: Vec<StockDef>,
    models: Vec<(ModelDef, Option<ModelKey>)>,
}

impl SetupInputs {
    pub(super) fn of(
        job: &Job,
        loaded: &BTreeMap<String, ModelSlot>,
        index: usize,
    ) -> Option<Self> {
        let setup = job.setups.get(index)?.clone();
        let tools = setup
            .operations
            .iter()
            .filter_map(|op| {
                let name = op.tool.as_ref()?;
                Some((name.clone(), job.tools.get(name)?.clone()))
            })
            .collect();
        let stocks = setup
            .stocks
            .iter()
            .filter_map(|s| job.stock(s).cloned())
            .collect();
        let models = job
            .models_for(index)
            .into_iter()
            .map(|m| (m.clone(), loaded.get(&m.name).map(|s| s.key().clone())))
            .collect();
        Some(Self {
            tolerance: job.tolerance.to_bits(),
            setup,
            tools,
            stocks,
            models,
        })
    }

    /// The part of this that decides where the part and stock sit: not the
    /// operations or their tools.
    fn into_layout(mut self) -> Self {
        self.setup.operations.clear();
        self.tools.clear();
        self
    }

    /// Whether these are what setup `index` of `job` is made from now, with the
    /// models `loaded`: `SetupInputs::of(job, loaded, index) == Some(self)`, found
    /// without copying the setup. The tree asks for every operation every frame.
    pub(super) fn is_now(
        &self,
        job: &Job,
        loaded: &BTreeMap<String, ModelSlot>,
        index: usize,
    ) -> bool {
        let Some(setup) = job.setups.get(index) else {
            return false;
        };
        // With the same operations, the tools taken are the ones they name: each
        // must be as it was, or missing as it was.
        self.setup.operations == setup.operations
            && setup
                .operations
                .iter()
                .filter_map(|op| op.tool.as_ref())
                .all(|t| self.tools.get(t) == job.tools.get(t))
            && self.layout_is_now(job, loaded, index)
    }

    /// Whether the part of these that decides where the part and stock sit (see
    /// [`SetupInputs::into_layout`]) is as setup `index` of `job` has it now.
    fn layout_is_now(&self, job: &Job, loaded: &BTreeMap<String, ModelSlot>, index: usize) -> bool {
        let Some(setup) = job.setups.get(index) else {
            return false;
        };
        let models = job.models_for(index);
        let models_now = models
            .iter()
            .map(|m| (*m, loaded.get(&m.name).map(ModelSlot::key)));
        self.tolerance == job.tolerance.to_bits()
            && same_but_operations(&self.setup, setup)
            && self
                .stocks
                .iter()
                .eq(setup.stocks.iter().filter_map(|s| job.stock(s)))
            && self
                .models
                .iter()
                .map(|(m, key)| (m, key.as_ref()))
                .eq(models_now)
    }
}

/// Whether two setups are the same but for their operations. Every field is named,
/// so one added to `Setup` has to be decided on here.
fn same_but_operations(a: &Setup, b: &Setup) -> bool {
    let Setup {
        name,
        models,
        stocks,
        wcs,
        post,
        orientation,
        origin,
        operations: _,
    } = a;
    *name == b.name
        && *models == b.models
        && *stocks == b.stocks
        && *wcs == b.wcs
        && *post == b.post
        && *orientation == b.orientation
        && *origin == b.origin
}

/// Where an operation stands, for the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpStatus {
    /// Its setup has never run, or ran before it was added.
    NotRun,
    /// Its setup is running, and has not reached it yet.
    Waiting,
    /// Running now, since then.
    Running(Instant),
    /// Made, in this run or the last.
    Done,
    /// Made, with warnings worth reading.
    Warnings,
    /// Made, but it, its tool or its setup's layout has changed since.
    Stale,
    /// Turned off: left out of every run.
    Disabled,
}

/// Where a setup stands, for the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Never run.
    NotRun,
    /// Running now.
    Running,
    /// Ran and verified.
    Verified,
    /// Ran and verified, with warnings worth reading.
    Warnings,
    /// Ran and did not verify, or a check failed: it would be refused.
    Refused,
    /// Ran, but has changed since.
    Stale,
}

impl Document {
    /// The models setup `index` needs, if every one is loaded.
    pub fn models_for_run(&self, index: usize) -> Option<BTreeMap<String, Arc<Model>>> {
        self.job
            .models_for(index)
            .iter()
            .map(|m| {
                let model = self.models.get(&m.name)?.model()?;
                Some((m.name.clone(), model.clone()))
            })
            .collect()
    }

    /// Why setup `name`, or its operation `only` alone, cannot be generated
    /// now, if it cannot; nothing for a setup there is not. Only the operations
    /// the run would work out or keep are checked: the rest are left out, and may
    /// be unfinished.
    pub fn cannot_generate(&self, name: &str, only: Option<usize>) -> Option<String> {
        let index = self.setup_index(name)?;
        let setup = self.job.setups.get(index)?;
        if let Some(k) = only
            && setup.operations.get(k).is_some_and(|o| !o.enabled)
        {
            return Some("it is turned off".into());
        }
        // Which operations the run works out or keeps, as `plans` has them, asked
        // of each without copying what would be kept: this is asked every frame.
        let runs = |i: usize| match only {
            Some(k) => i == k || self.kept(setup, i).is_some(),
            None => true,
        };
        if let Err(e) = check_runnable(&self.job, index, &runs) {
            return Some(e.to_string());
        }
        for m in self.job.models_for(index) {
            match self.models.get(&m.name) {
                Some(ModelSlot::Ready(..)) => {}
                Some(ModelSlot::Loading(_)) => return Some(format!("'{}' is loading", m.name)),
                Some(ModelSlot::Failed { why, .. }) => return Some(why.clone()),
                None => return Some(format!("'{}' is not loaded", m.name)),
            }
        }
        None
    }

    /// What a run of setup `index` does with each operation: works them all out;
    /// or, for operation `only`, works it out alone, keeps every other one made
    /// before and unchanged since, and leaves out the rest.
    pub fn plans(&self, index: usize, only: Option<usize>) -> Vec<Plan> {
        let (Some(k), Some(setup)) = (only, self.job.setups.get(index)) else {
            return Vec::new();
        };
        (0..setup.operations.len())
            .map(|i| {
                if i == k {
                    return Plan::Generate;
                }
                match self.kept(setup, i) {
                    Some(op) => Plan::Keep(Box::new(op.clone())),
                    None => Plan::Skip,
                }
            })
            .collect()
    }

    /// What `setup` last made of its operation `i`, if a run of another operation
    /// alone can keep it: it has paths, and nothing it was made from has changed.
    fn kept(&self, setup: &Setup, i: usize) -> Option<&OperationResult> {
        let ran = self.setups.get(&setup.name)?.ran.as_ref()?;
        ran.ran
            .result
            .operations
            .get(i)
            .filter(|op| op.made.has_paths() && self.op_unchanged(&setup.name, i))
    }

    /// Whether operation `i` of setup `name` is as it was when the setup last
    /// ran: the operation, its tool, and where the part and stock sit.
    fn op_unchanged(&self, name: &str, i: usize) -> bool {
        let then = self
            .setups
            .get(name)
            .and_then(|s| s.ran.as_ref())
            .map(|r| &r.inputs);
        let (Some(then), Some(index)) = (then, self.setup_index(name)) else {
            return false;
        };
        let now = self.job.setups.get(index).and_then(|s| s.operations.get(i));
        let (Some(a), Some(b)) = (then.setup.operations.get(i), now) else {
            return false;
        };
        // The tool as it was taken then, and as the job has it now.
        let tool_then = a.tool.as_ref().map(|t| then.tools.get(t));
        let tool_now = b.tool.as_ref().map(|t| self.job.tools.get(t));
        a == b && tool_then == tool_now && then.layout_is_now(&self.job, &self.models, index)
    }

    /// Turn operation `index` of `setup` on or off.
    pub fn set_enabled(&mut self, setup: &str, index: usize, on: bool) {
        if let Some(op) = self.operation_mut(setup, index) {
            op.enabled = on;
        }
    }

    /// Note that setup `name` has started running from the job as it is now, and
    /// say which run it is.
    pub fn started(&mut self, name: &str) -> u64 {
        self.seq += 1;
        let seq = self.seq;
        let job = self.job.clone();
        let state = self.setups.entry(name.to_string()).or_default();
        state.running = Some((seq, job));
        state.progress = Some((Stage::Preparing, Instant::now()));
        seq
    }

    /// Setup `name`'s run `seq` has got to `stage`. Word from a run since
    /// superseded is dropped.
    pub fn progressed(&mut self, name: &str, seq: u64, stage: Stage) {
        if let Some(state) = self.setups.get_mut(name)
            && state.running.as_ref().is_some_and(|(s, _)| *s == seq)
        {
            state.progress = Some((stage, Instant::now()));
        }
    }

    /// Where operation `index` of setup `name` stands.
    pub fn op_status(&self, name: &str, index: usize) -> OpStatus {
        let off = self.operation(name, index).is_some_and(|o| !o.enabled);
        if off {
            return OpStatus::Disabled;
        }
        let Some(state) = self.setups.get(name) else {
            return OpStatus::NotRun;
        };
        if state.running.is_some() {
            return match state.progress {
                Some((Stage::Operation(at), since)) if at == index => OpStatus::Running(since),
                Some((Stage::Operation(at), _)) if at > index => OpStatus::Done,
                Some((Stage::Verifying | Stage::Checking, _)) => OpStatus::Done,
                _ => OpStatus::Waiting,
            };
        }
        let made = state
            .ran
            .as_ref()
            .and_then(|r| r.ran.result.operations.get(index));
        match made {
            None => OpStatus::NotRun,
            Some(op) if !op.made.has_paths() => OpStatus::NotRun,
            Some(_) if !self.op_unchanged(name, index) => OpStatus::Stale,
            Some(op) if !op.warnings.is_empty() => OpStatus::Warnings,
            Some(_) => OpStatus::Done,
        }
    }

    /// Setup `name`'s run `seq` has finished. A reply for a run since superseded,
    /// or for a setup since removed, is dropped.
    pub fn finished(&mut self, name: &str, seq: u64, result: Result<Ran, SessionError>) {
        let Some(state) = self.setups.get_mut(name) else {
            return;
        };
        let job = match state.running.take() {
            Some((s, job)) if s == seq => job,
            other => {
                state.running = other;
                return;
            }
        };
        state.progress = None;
        // What the run was made from: the job as sent, with the models as they
        // were loaded then.
        let Some(index) = job.setups.iter().position(|s| s.name == name) else {
            return;
        };
        let inputs = SetupInputs::of(&job, &self.models, index);
        let Some(state) = self.setups.get_mut(name) else {
            return;
        };
        match result {
            Ok(ran) => {
                let mut lines: Vec<(Level, String)> =
                    ran.notes.iter().map(|n| (Level::Info, n.clone())).collect();
                lines.extend(ran.warnings.iter().map(|w| (Level::Warn, w.clone())));
                lines.push(if ran.passed() {
                    (
                        Level::Info,
                        format!("'{name}' passed its checks: it may be posted"),
                    )
                } else {
                    (
                        Level::Error,
                        format!("'{name}' did NOT pass its checks: it would be refused"),
                    )
                });
                let Some(setup) = job.setups.get(index) else {
                    return;
                };
                let scene = Scene::build(&job, setup, &ran.result);
                let view = &mut state.view;
                // A new run starts fully shown, every operation visible.
                view.at = Playhead::at(scene.segments.len());
                view.transport.playing = false;
                view.view.ops = u32::MAX;
                view.stock_images = vec![None; ran.result.keyframes.len()];
                // Then shown as the selection has it.
                self.followed = None;
                if let Some(inputs) = inputs {
                    state.ran = Some(RanSetup {
                        inputs,
                        job: job.clone(),
                        ran,
                        scene,
                        scene_key: next_key(),
                    });
                }
                self.log.extend(lines);
            }
            Err(e) => self.note(
                Level::Error,
                format!("'{name}': {}", crate::app::explain(&e)),
            ),
        }
    }

    /// Where setup `name` stands.
    pub fn status(&self, name: &str) -> Status {
        let Some(state) = self.setups.get(name) else {
            return Status::NotRun;
        };
        if state.running.is_some() {
            return Status::Running;
        }
        let Some(ran) = &state.ran else {
            return Status::NotRun;
        };
        let now = self
            .setup_index(name)
            .is_some_and(|i| ran.inputs.is_now(&self.job, &self.models, i));
        // Anything changed, or an operation that should run left out of it.
        let partial = ran
            .ran
            .result
            .operations
            .iter()
            .any(|o| o.made == Made::Skipped);
        if !now || partial {
            Status::Stale
        } else if !ran.ran.passed() {
            Status::Refused
        } else if ran.ran.warnings.is_empty() {
            Status::Verified
        } else {
            Status::Warnings
        }
    }

    /// Lay out setup `name` for drawing, if anything it depends on has changed.
    ///
    /// Returns how far everything drawn moved when only the work zero did: the
    /// setup is drawn measured from it, so a view kept on the part moves with it.
    pub fn refresh_layout(&mut self, name: &str) -> Option<DVec3> {
        let index = self.setup_index(name)?;
        // A model that finishes loading changes the layout without changing the job.
        let ready = || {
            self.models
                .iter()
                .filter(|(_, s)| s.model().is_some())
                .map(|(n, _)| n)
        };
        // Asked every frame, so compared in place and copied only when changed.
        if self
            .setups
            .get(name)
            .and_then(|s| s.layout.as_ref())
            .is_some_and(|l| {
                l.inputs.0.layout_is_now(&self.job, &self.models, index)
                    && l.inputs.1.iter().eq(ready())
            })
        {
            return None;
        }
        let inputs = SetupInputs::of(&self.job, &self.models, index)?.into_layout();
        let inputs = (inputs, ready().cloned().collect());
        let meshes: Bodies<'_> = self
            .models
            .iter()
            .filter_map(|(n, s)| Some((n.as_str(), &s.model()?.body)))
            .collect();
        let prepared = match prepare(&self.job, index, &meshes) {
            // A work zero picked on something no longer there: draw the part from
            // the model's own zero instead, without the axes, which would be a lie,
            // so it can be seen to pick again. The panel says what is wrong, and a
            // run refuses the setup until it is fixed.
            Err(PrepareError::Feature(_)) => {
                let mut job = self.job.clone();
                if let Some(s) = job.setups.get_mut(index) {
                    s.origin = Origin::ModelOrigin {
                        offset: s.origin.offset(),
                    };
                }
                prepare(&job, index, &meshes).map(|p| (p, false))
            }
            other => other.map(|p| (p, true)),
        };
        let outline = prepared
            .as_ref()
            .map(|(p, found)| {
                let mut lines = outline(p);
                if !found {
                    lines.retain(|l| l.kind == OutlineKind::Stock);
                }
                lines
            })
            .unwrap_or_default();
        let prepared = prepared.map(|(p, _)| p).map_err(|e| e.to_string());
        let state = self.setups.entry(name.to_string()).or_default();
        let before = state
            .layout
            .as_ref()
            .and_then(|l| l.prepared.as_ref().ok())
            .map(|p| p.frame);
        let after = prepared.as_ref().ok().map(|p| p.frame);
        state.layout = Some(Layout {
            inputs,
            prepared,
            outline,
            key: next_key(),
        });
        match (before, after) {
            (Some(b), Some(a)) if b.orientation == a.orientation && b.origin != a.origin => {
                Some(b.origin - a.origin)
            }
            _ => None,
        }
    }
}

/// The stock's edges and the work zero's axes, sized to the part.
fn outline(p: &Prepared) -> Vec<Outline> {
    let mut out: Vec<Outline> = p
        .stocks
        .iter()
        .flat_map(box_edges)
        .map(|(a, b)| Outline {
            a,
            b,
            kind: OutlineKind::Stock,
        })
        .collect();
    let size = p.stock_bounds().extent().length();
    let arm = (size * 0.12).clamp(5.0, 60.0);
    for (axis, kind) in [
        (DVec3::X, OutlineKind::AxisX),
        (DVec3::Y, OutlineKind::AxisY),
        (DVec3::Z, OutlineKind::AxisZ),
    ] {
        out.push(Outline {
            a: DVec3::ZERO,
            b: axis * arm,
            kind,
        });
    }
    out
}
