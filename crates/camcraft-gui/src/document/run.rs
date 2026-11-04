//! Running a job's setups, and what came of it: what a run needs and what it does
//! with each operation, where each setup and operation stands, and each setup laid
//! out for drawing.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use camcraft_core::control::{Control, share};
use camcraft_core::job::{Job, ModelDef, Operation, Origin, Setup, StockDef, ToolSpec};
use camcraft_core::run::{Made, OperationResult, Plan, Stage};
use camcraft_core::setup::frame::Orientation;
use camcraft_core::setup::{PrepareError, Prepared, prepare};
use camcraft_geom::shadow::Shadow;
use camcraft_session::{Model, Ran, SessionError, check_runnable};
use glam::DVec3;

use super::{
    Document, Halt, Layout, Level, ModelKey, ModelSlot, OpWork, RanSetup, Running, SetupState,
    next_key,
};
use crate::transport::marks;
use crate::view::scene::{Scene, box_edges};
use crate::view::viewport::{Highlight, Outline, OutlineKind};

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

    /// The paths in these, found from `from`, written from `to`, as a job saved
    /// in another folder has them ([`super::rebase`]).
    pub(super) fn rebase(&mut self, from: &Path, to: &Path) {
        self.setup.post = super::moved_post(&self.setup.post, from, to);
        for (m, _) in &mut self.models {
            m.path = super::moved(&m.path, from, to);
        }
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
        job.setups
            .get(index)
            .is_some_and(|setup| same_but_operations(&self.setup, setup))
            && self.cut_is_now(job, loaded, index)
    }

    /// Whether operation `i` of `setup`, as `job` has it now, is as these took
    /// it: the operation, and its tool as it was taken then. Where the part and
    /// stock sit is the setup's to ask, once for all its operations.
    fn op_is_now(&self, job: &Job, setup: &Setup, i: usize) -> bool {
        let (Some(then), Some(now)) = (self.setup.operations.get(i), setup.operations.get(i))
        else {
            return false;
        };
        let tool_then = then.tool.as_ref().map(|t| self.tools.get(t));
        let tool_now = now.tool.as_ref().map(|t| job.tools.get(t));
        then == now && tool_then == tool_now
    }

    /// The first operation of setup `index` of `job`, with the models
    /// `loaded`, that would cut otherwise now than these took it: changed,
    /// retooled or removed; the first of all with what shapes every cut
    /// changed, or the setup gone. `None` where every one would cut the same,
    /// one added after them all included.
    pub(super) fn first_changed(
        &self,
        job: &Job,
        loaded: &BTreeMap<String, ModelSlot>,
        index: usize,
    ) -> Option<usize> {
        let Some(setup) = job.setups.get(index) else {
            return Some(0);
        };
        if !self.cut_is_now(job, loaded, index) {
            return Some(0);
        }
        (0..self.setup.operations.len()).find(|&i| !self.op_is_now(job, setup, i))
    }

    /// Whether what shapes the cuts, not what is only posted with them, is as
    /// setup `index` of `job` has it now: the tolerance, how the part sits, and
    /// the stocks and models. A setup renamed, or given another work offset or
    /// post, still cuts the same paths.
    fn cut_is_now(&self, job: &Job, loaded: &BTreeMap<String, ModelSlot>, index: usize) -> bool {
        let Some(setup) = job.setups.get(index) else {
            return false;
        };
        let models = job.models_for(index);
        let models_now = models
            .iter()
            .map(|m| (*m, loaded.get(&m.name).map(ModelSlot::key)));
        self.tolerance == job.tolerance.to_bits()
            && self.setup.models == setup.models
            && self.setup.stocks == setup.stocks
            && self.setup.orientation == setup.orientation
            && self.setup.origin == setup.origin
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

/// What a setup's part's shadow in its turned frame is cast from: how the part
/// sits, and each of its models as loaded, in the setup's order (its file, units,
/// tolerance and placement, [`ModelKey`]), or `None` for one not loaded. Two
/// layouts with the same cast the same shadow there, whatever their stock or
/// work zero, which only moves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowKey {
    orientation: Orientation,
    models: Vec<Option<ModelKey>>,
}

/// How often what operations being worked out have shown is drawn again: a
/// large one shows a million segments, each drawing of which is uploaded
/// whole.
pub const PREVIEW_EVERY: std::time::Duration = std::time::Duration::from_millis(500);

/// How far apart the points of a pass being worked out are drawn, at least:
/// finer than shows at any zoom a part is looked at, and far fewer.
const PREVIEW_POINTS: f64 = 0.25;

/// `pass`, a line of points, as lines to draw in its preview: its points no
/// nearer than [`PREVIEW_POINTS`] to the last drawn, but its last.
fn preview_lines(pass: &[DVec3]) -> Vec<Outline> {
    let mut out = Vec::new();
    let Some((&first, rest)) = pass.split_first() else {
        return out;
    };
    let mut at = first;
    for (i, &p) in rest.iter().enumerate() {
        if p.distance(at) >= PREVIEW_POINTS || i + 1 == rest.len() {
            out.push(Outline {
                a: at,
                b: p,
                kind: OutlineKind::Preview,
            });
            at = p;
        }
    }
    out
}

/// How many parts' shadows a job keeps: enough for its setups and an undo or
/// two, and never without bound.
const KEPT_SHADOWS: usize = 8;

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
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpStatus {
    /// Its setup has never run, or ran before it was added.
    NotRun,
    /// Its setup is running, and has not started on it yet: it waits for the
    /// operations before it.
    Waiting,
    /// Under way: doing `work` since `since`, `done` of the way through, from
    /// 0 to 1.
    Running {
        /// Since when.
        since: Instant,
        /// What it is doing.
        work: OpWork,
        /// How far it has got.
        done: f64,
    },
    /// Its run failed here.
    Failed,
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
    /// Ran, but has changed since, or did not run to the end: generated again
    /// by itself, as soon as editing settles.
    Stale,
    /// Stopped, and not generated again by itself until it changes.
    Stopped,
    /// Its run failed, and it is not generated again by itself until it
    /// changes.
    Failed,
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
        let kept = only.map(|_| self.kept(setup)).unwrap_or_default();
        let runs = |i: usize| match only {
            Some(k) => i == k || kept.get(i).is_some_and(Option::is_some),
            None => true,
        };
        if let Err(e) = check_runnable(&self.job, index, &runs) {
            return Some(e.to_string());
        }
        let dir = self.dir();
        for m in self.job.models_for(index) {
            match self.models.get(&m.name) {
                Some(ModelSlot::Ready(k, ..))
                    if *k != ModelKey::of(&dir, m, self.job.tolerance) =>
                {
                    return Some(format!("'{}' is being placed", m.name));
                }
                Some(ModelSlot::Ready(..)) => {}
                Some(ModelSlot::Loading(_)) => return Some(format!("'{}' is loading", m.name)),
                Some(ModelSlot::Failed { why, .. }) => return Some(why.clone()),
                None => return Some(format!("'{}' is not loaded", m.name)),
            }
        }
        None
    }

    /// What a run of setup `index` does with each operation: keeps every one
    /// made before and unchanged since, up to the first that is not, and works
    /// out that one and every one after it, since each cuts what the ones
    /// before it left; or, for operation `only`, works it out alone, keeps the
    /// others it can, and leaves out the rest.
    pub fn plans(&self, index: usize, only: Option<usize>) -> Vec<Plan> {
        let Some(setup) = self.job.setups.get(index) else {
            return Vec::new();
        };
        let kept = self.kept(setup);
        let standing = (0..setup.operations.len())
            .take_while(|&i| kept.get(i).copied().flatten().is_some())
            .count();
        (0..setup.operations.len())
            .map(|i| match (only, kept.get(i).copied().flatten()) {
                (Some(k), _) if i == k => Plan::Generate,
                (Some(_), Some(op)) => Plan::Keep(Box::new(op.clone())),
                (Some(_), None) => Plan::Skip,
                (None, Some(op)) if i < standing => Plan::Keep(Box::new(op.clone())),
                (None, _) => Plan::Generate,
            })
            .collect()
    }

    /// What `setup` last made of each of its operations, where a run of another
    /// operation alone can keep it: it has paths, and nothing it was made from
    /// has changed. Empty when it has not run.
    fn kept(&self, setup: &Setup) -> Vec<Option<&OperationResult>> {
        let Some(ran) = self.setups.get(&setup.name).and_then(|s| s.ran.as_ref()) else {
            return Vec::new();
        };
        let unchanged = self.unchanged(&setup.name);
        let made = &ran.ran.result.operations;
        unchanged
            .iter()
            .enumerate()
            .map(|(i, &same)| made.get(i).filter(|op| same && op.made.has_paths()))
            .collect()
    }

    /// Whether each operation of setup `name` is as it was when the setup last
    /// ran: the operation, its tool, and where the part and stock sit, which is
    /// found once for them all. Empty when it has not run.
    fn unchanged(&self, name: &str) -> Vec<bool> {
        let then = self
            .setups
            .get(name)
            .and_then(|s| s.ran.as_ref())
            .map(|r| &r.inputs);
        let index = self.setup_index(name);
        let (Some(then), Some(index)) = (then, index) else {
            return Vec::new();
        };
        let Some(setup) = self.job.setups.get(index) else {
            return Vec::new();
        };
        let laid_out = then.layout_is_now(&self.job, &self.models, index);
        (0..setup.operations.len())
            .map(|i| laid_out && then.op_is_now(&self.job, setup, i))
            .collect()
    }

    /// Take out of each setup's last run what no longer stands: from its first
    /// operation that would cut otherwise now (changed, retooled, turned off,
    /// moved, removed, or with something ahead of it added), to the end. Each
    /// operation cuts the material the ones before it left, so none after a
    /// changed one stands either. Their toolpaths, the cutter and material along
    /// them, and the playhead past them go; the operations before are kept,
    /// drawn and ready to be kept again. With the part, stock or tolerance
    /// changed, nothing stands, and the run is forgotten.
    pub fn invalidate_changed_operations(&mut self) {
        let cuts: Vec<(String, usize)> = self
            .job
            .setups
            .iter()
            .enumerate()
            .filter_map(|(index, setup)| {
                let ran = self.setups.get(&setup.name)?.ran.as_ref()?;
                let len = ran.ran.result.operations.len();
                // What shapes every cut (see [`SetupInputs::cut_is_now`]), then
                // each operation and its tool.
                let then = &ran.inputs;
                let cut = then.cut_is_now(&self.job, &self.models, index);
                let first = (0..len).find(|&i| !(cut && then.op_is_now(&self.job, setup, i)))?;
                Some((setup.name.clone(), first))
            })
            .collect();
        for (name, first) in cuts {
            if let Some(state) = self.setups.get_mut(&name) {
                state.cut_run_back(first);
            }
        }
    }

    /// Turn operation `index` of `setup` on or off.
    pub fn set_enabled(&mut self, setup: &str, index: usize, on: bool) {
        if let Some(op) = self.operation_mut(setup, index) {
            op.enabled = on;
        }
    }

    /// Note that setup `name` has started running from the job as it is now,
    /// with `control` to stop it and see how far it has got, keeping each
    /// operation `plans` keeps, and say which run it is.
    pub fn started(&mut self, name: &str, control: Control, plans: &[Plan]) -> u64 {
        self.seq += 1;
        let seq = self.seq;
        let job = self.job.clone();
        let Some(inputs) = self
            .setup_index(name)
            .and_then(|i| SetupInputs::of(&job, &self.models, i))
        else {
            return seq;
        };
        let count = inputs.setup.operations.len();
        let kept = (0..count)
            .map(|i| matches!(plans.get(i), Some(Plan::Keep(_))))
            .collect();
        let state = self.setups.entry(name.to_string()).or_default();
        state.running = Some(Running {
            seq,
            job,
            inputs,
            control,
            stage: (Stage::Preparing, Instant::now()),
            ops: vec![None; count],
            kept,
            made: 0,
            shown: vec![(Vec::new(), 0); count],
            preview: Highlight::default(),
            preview_key: next_key(),
            previewed: Instant::now(),
        });
        // Asked for, so whatever held it back is let go.
        state.halted = None;
        seq
    }

    /// The setup whose run is `seq`, by its name now: renamed since it was
    /// sent, it is found all the same. `None` for a run since superseded, or a
    /// setup since removed.
    fn running_as(&self, seq: u64) -> Option<String> {
        self.setups
            .iter()
            .find(|(_, s)| s.running.as_ref().is_some_and(|r| r.seq == seq))
            .map(|(name, _)| name.clone())
    }

    /// The run in flight that is `seq`: `None` for a run since superseded.
    fn running_mut(&mut self, seq: u64) -> Option<&mut SetupState> {
        let name = self.running_as(seq)?;
        self.setups.get_mut(&name)
    }

    /// Run `seq` has started on `stage`. Word from a run since superseded is
    /// dropped.
    pub fn progressed(&mut self, seq: u64, stage: Stage) {
        let Some(running) = self.running_mut(seq).and_then(|s| s.running.as_mut()) else {
            return;
        };
        let now = Instant::now();
        match stage {
            Stage::Operation(i) => {
                if let Some(op) = running.ops.get_mut(i) {
                    *op = Some((OpWork::Planning, now));
                }
            }
            Stage::Cutting(i) => {
                if let Some(op) = running.ops.get_mut(i) {
                    // Since its work began, not since its turn came.
                    let since = op.map_or(now, |(_, since)| since);
                    *op = Some((OpWork::Cutting, since));
                }
            }
            Stage::Preparing | Stage::Verifying | Stage::Checking => {
                running.stage = (stage, now);
            }
        }
    }

    /// Run `seq` has made its first operations, what it has made so far, with
    /// their toolpaths laid out for drawing: shown at once, before the rest are
    /// made. Word from a run since superseded is dropped.
    pub fn so_far(&mut self, seq: u64, ran: Ran, scene: Scene) {
        let Some(state) = self.running_mut(seq) else {
            return;
        };
        let Some(running) = state.running.as_mut() else {
            return;
        };
        running.made = ran.result.operations.len();
        // What is made is drawn as made: its preview goes.
        running.preview_again();
        let (inputs, job) = (running.inputs.clone(), running.job.clone());
        state.show_run(RanSetup {
            inputs,
            job,
            marks: marks(&ran, &scene),
            ran,
            scene,
            scene_key: next_key(),
        });
    }

    /// How far setup `name`'s run in flight has got with its operations
    /// together, from 0 to 1: each operation it makes an equal share, one
    /// made or kept all of its own, one under way as far as it has got.
    /// `None` until it has started on one.
    pub fn generated_share(&self, name: &str) -> Option<f64> {
        let running = self.setups.get(name)?.running.as_ref()?;
        let ops = &running.inputs.setup.operations;
        let kept = |i: usize| running.kept.get(i).copied().unwrap_or(false);
        let started = running.made > 0
            || running.ops.iter().any(Option::is_some)
            || running.kept.iter().any(|k| *k);
        let making: Vec<usize> = (0..ops.len())
            .filter(|&i| ops.get(i).is_some_and(|o| o.enabled))
            .collect();
        if !started || making.is_empty() {
            return None;
        }
        let each = share(1, making.len());
        let done: f64 = making
            .iter()
            .map(|&i| {
                if i < running.made || kept(i) {
                    1.0
                } else {
                    running.control.done(i)
                }
            })
            .sum();
        Some((done * each).clamp(0.0, 1.0))
    }

    /// Take into each run in flight the passes its operations' work has shown
    /// since it was last asked, as of `now`, and draw them again: no more
    /// often than [`PREVIEW_EVERY`], since a large operation shows many.
    pub fn take_previews(&mut self, now: Instant) {
        for state in self.setups.values_mut() {
            let Some(running) = state.running.as_mut() else {
                continue;
            };
            if now.saturating_duration_since(running.previewed) < PREVIEW_EVERY {
                continue;
            }
            let mut grew = false;
            for (op, (lines, taken)) in running.shown.iter_mut().enumerate() {
                if op < running.made || running.kept.get(op).copied().unwrap_or(false) {
                    continue;
                }
                let new = running.control.shown_since(op, *taken);
                *taken += new.len();
                grew |= !new.is_empty();
                lines.extend(new.iter().flat_map(|pass| preview_lines(pass)));
            }
            running.previewed = now;
            if grew {
                running.preview_again();
            }
        }
    }

    /// Where operation `index` of setup `name` stands: one of
    /// [`Document::op_statuses`], as the tests ask.
    #[cfg(test)]
    pub fn op_status(&self, name: &str, index: usize) -> OpStatus {
        self.op_statuses(name)
            .get(index)
            .copied()
            .unwrap_or(OpStatus::NotRun)
    }

    /// Where each operation of setup `name` stands: the tree asks for every
    /// one every frame, so what they share is found once.
    pub fn op_statuses(&self, name: &str) -> Vec<OpStatus> {
        let Some(setup) = self.setup(name) else {
            return Vec::new();
        };
        let state = self.setups.get(name);
        let running = state.and_then(|s| s.running.as_ref());
        let ran = state.and_then(|s| s.ran.as_ref());
        let unchanged = if ran.is_some() {
            self.unchanged(name)
        } else {
            Vec::new()
        };
        let failed_at = state
            .and_then(|s| s.halted.as_ref())
            .zip(self.setup_index(name))
            .and_then(|((inputs, halt), i)| match halt {
                Halt::Failed { at, .. } if inputs.is_now(&self.job, &self.models, i) => Some(*at),
                _ => None,
            });
        let status = |(index, op): (usize, &Operation)| {
            if !op.enabled {
                return OpStatus::Disabled;
            }
            // In flight: what it has made is as it stands, below; what it keeps
            // stands as it was; the rest is under way or waiting its turn.
            if let Some(r) = running
                && index >= r.made
                && !r.kept.get(index).copied().unwrap_or(false)
            {
                return match r.ops.get(index).copied().flatten() {
                    Some((work, since)) => OpStatus::Running {
                        since,
                        work,
                        done: r.control.done(index),
                    },
                    None => OpStatus::Waiting,
                };
            }
            if failed_at == Some(index) {
                return OpStatus::Failed;
            }
            match ran.and_then(|r| r.ran.result.operations.get(index)) {
                None => OpStatus::NotRun,
                Some(op) if !op.made.has_paths() => OpStatus::NotRun,
                Some(_) if unchanged.get(index) != Some(&true) => OpStatus::Stale,
                Some(op) if !op.warnings.is_empty() => OpStatus::Warnings,
                Some(_) => OpStatus::Done,
            }
        };
        setup.operations.iter().enumerate().map(status).collect()
    }

    /// Run `seq` has finished: what it made, with its
    /// toolpaths laid out for drawing ([`Scene::build`]) where it ran; or it
    /// was stopped, and what it made so far stands; or it failed, and the
    /// setup is left as it is until it changes. A reply for a run since
    /// superseded, or for a setup since removed, is dropped; one for a setup
    /// renamed since goes to it by its new name.
    pub fn finished(&mut self, seq: u64, result: Result<(Ran, Scene), SessionError>) {
        let Some(name) = self.running_as(seq) else {
            return;
        };
        let name = name.as_str();
        let Some(running) = self.setups.get_mut(name).and_then(|s| s.running.take()) else {
            return;
        };
        let Some(state) = self.setups.get_mut(name) else {
            return;
        };
        match result {
            Ok((ran, scene)) => {
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
                state.show_run(RanSetup {
                    inputs: running.inputs,
                    job: running.job,
                    marks: marks(&ran, &scene),
                    ran,
                    scene,
                    scene_key: next_key(),
                });
                self.log.extend(lines);
            }
            // What it made before it was stopped stands, as it was told.
            Err(e) if e.is_stopped() => {}
            Err(e) => {
                let why = crate::app::explain(&e);
                state.halted = Some((
                    running.inputs,
                    Halt::Failed {
                        at: running.made,
                        why: why.clone(),
                    },
                ));
                self.note(Level::Error, format!("'{name}': {why}"));
            }
        }
    }

    /// Run `seq` ended without a word, for `why`: the setup is left as it is
    /// until it changes, as for a run that failed.
    pub fn lost(&mut self, seq: u64, why: String) {
        let Some(state) = self.running_mut(seq) else {
            return;
        };
        if let Some(running) = state.running.take() {
            state.halted = Some((
                running.inputs,
                Halt::Failed {
                    at: running.made,
                    why,
                },
            ));
        }
    }

    /// Stop generating setup `name`: its run in flight, if any, and any run of
    /// it by itself until something it is made from changes. What it has made
    /// stands.
    pub fn stop_generating(&mut self, name: &str) {
        let Some(index) = self.setup_index(name) else {
            return;
        };
        let Some(inputs) = SetupInputs::of(&self.job, &self.models, index) else {
            return;
        };
        let state = self.setups.entry(name.to_string()).or_default();
        if let Some(running) = &state.running {
            running.control.stop();
        }
        state.halted = Some((inputs, Halt::Stopped));
    }

    /// Generate setup `name` by itself again, as it stands: let go of whatever
    /// stopped it, or the failure it is left at.
    pub fn resume_generating(&mut self, name: &str) {
        if let Some(state) = self.setups.get_mut(name) {
            state.halted = None;
        }
    }

    /// Stop each run in flight from the first of its operations that has
    /// changed since it was sent: each one after a changed one cuts what it
    /// leaves, so none of them stands, while the ones before it go on and do.
    /// With what every cut is made from changed, the whole run stops.
    pub fn stop_changed_runs(&self) {
        for (index, setup) in self.job.setups.iter().enumerate() {
            let Some(running) = self
                .setups
                .get(&setup.name)
                .and_then(|s| s.running.as_ref())
            else {
                continue;
            };
            if let Some(first) = running.inputs.first_changed(&self.job, &self.models, index) {
                running.control.stop_from(first);
            }
        }
    }

    /// The setups that want generating now, by themselves: each that has
    /// something to make and is out of date or has never been generated, that
    /// can be, and is neither running nor left as it is where it stands.
    pub fn wanting_runs(&self) -> Vec<String> {
        self.job
            .setups
            .iter()
            .enumerate()
            .filter(|(index, setup)| {
                let state = self.setups.get(&setup.name);
                let busy = state.is_some_and(|s| s.running.is_some());
                let held = state
                    .and_then(|s| s.halted.as_ref())
                    .is_some_and(|(inputs, _)| inputs.is_now(&self.job, &self.models, *index));
                !busy
                    && !held
                    && setup.operations.iter().any(|o| o.enabled)
                    && matches!(self.status(&setup.name), Status::NotRun | Status::Stale)
                    && self.cannot_generate(&setup.name, None).is_none()
            })
            .map(|(_, setup)| setup.name.clone())
            .collect()
    }

    /// Where setup `name` stands.
    pub fn status(&self, name: &str) -> Status {
        let Some(state) = self.setups.get(name) else {
            return Status::NotRun;
        };
        if state.running.is_some() {
            return Status::Running;
        }
        let index = self.setup_index(name);
        if let Some((inputs, halt)) = &state.halted
            && index.is_some_and(|i| inputs.is_now(&self.job, &self.models, i))
        {
            return match halt {
                Halt::Stopped => Status::Stopped,
                Halt::Failed { .. } => Status::Failed,
            };
        }
        let Some(ran) = &state.ran else {
            return Status::NotRun;
        };
        let now = index.is_some_and(|i| ran.inputs.is_now(&self.job, &self.models, i));
        // Anything changed, an operation that should run left out of it, or
        // not run to the end.
        let partial = !ran.ran.finished
            || ran
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
        let meshes = self.bodies();
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
        let prepared = prepared
            .map(|(p, _)| Arc::new(p))
            .map_err(|e| e.to_string());
        let shadow_key = self.shadow_key(index)?;
        // The same models turned the same way cast the same shadow: a stock or
        // a work zero changed only moves it.
        if let (Ok(p), Some(kept)) = (&prepared, self.kept_shadow(&shadow_key)) {
            p.adopt_shadow(&kept);
        }
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
            shadow_failed: None,
            shadow_key,
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

    /// Each layout whose part's shadow should be cast, off the window's
    /// thread, and is not: by its key, with the setup laid out, to cast it on.
    /// What the view and the panel draw from the shadow (a silhouette
    /// boundary, what a tool cuts outside the part) waits for it rather than
    /// casting it while a frame is drawn, which for a large part takes a
    /// while. A setup with one being cast waits for it to finish before it
    /// asks for another.
    pub fn shadows_wanted(&self) -> Vec<(u64, Arc<Prepared>)> {
        self.setups
            .values()
            .filter(|s| s.casting.is_none())
            .filter_map(|s| {
                let layout = s.layout.as_ref()?;
                let prepared = layout.prepared.as_ref().ok()?;
                let wanted = prepared.cast_shadow().is_none() && layout.shadow_failed.is_none();
                wanted.then(|| (layout.key, prepared.clone()))
            })
            .collect()
    }

    /// A worker has started casting the shadow of the layout keyed `key`.
    pub fn casting(&mut self, key: u64) {
        if let Some(state) = self
            .setups
            .values_mut()
            .find(|s| s.layout.as_ref().is_some_and(|l| l.key == key))
            && let Some(layout) = &state.layout
        {
            state.casting = Some((key, layout.shadow_key.clone()));
        }
    }

    /// The shadow of the layout keyed `key` has been cast, and is `result`,
    /// in its setup's turned frame, or could not be, and why. Kept for any
    /// layout or run of the same models turned the same way; the setup's layout
    /// now, made again since it was asked for, takes it if it is one of those.
    /// Its setup may ask for another.
    pub fn cast(&mut self, key: u64, result: Result<Arc<Shadow>, String>) {
        let Some(state) = self
            .setups
            .values_mut()
            .find(|s| s.casting.as_ref().is_some_and(|(k, _)| *k == key))
        else {
            return;
        };
        let Some((_, shadow_key)) = state.casting.take() else {
            return;
        };
        match result {
            Ok(turned) => {
                if let Some(layout) = &state.layout
                    && layout.shadow_key == shadow_key
                    && let Ok(p) = &layout.prepared
                {
                    p.adopt_shadow(&turned);
                }
                self.turned_shadows.retain(|(k, _)| *k != shadow_key);
                self.turned_shadows.push((shadow_key, turned));
                if self.turned_shadows.len() > KEPT_SHADOWS {
                    self.turned_shadows.remove(0);
                }
            }
            Err(why) => {
                if let Some(layout) = state.layout.as_mut()
                    && layout.key == key
                {
                    layout.shadow_failed = Some(why);
                }
            }
        }
    }

    /// What setup `index`'s part's shadow would be cast from now.
    fn shadow_key(&self, index: usize) -> Option<ShadowKey> {
        let setup = self.job.setups.get(index)?;
        Some(ShadowKey {
            orientation: setup.orientation,
            models: self
                .job
                .models_for(index)
                .iter()
                .map(|m| self.models.get(&m.name).map(|s| s.key().clone()))
                .collect(),
        })
    }

    /// The shadow kept for `key`, if one is.
    fn kept_shadow(&self, key: &ShadowKey) -> Option<Arc<Shadow>> {
        self.turned_shadows
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, s)| Arc::clone(s))
    }

    /// The shadow kept for setup `index` as it would be laid out now, in its
    /// turned frame, if one is: for a run of it to take rather than cast.
    pub fn turned_shadow(&self, index: usize) -> Option<Arc<Shadow>> {
        self.kept_shadow(&self.shadow_key(index)?)
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
