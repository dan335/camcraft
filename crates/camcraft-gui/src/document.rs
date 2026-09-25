//! One open job: the job as edited, as saved, its models as loaded, and what each
//! of its setups last ran to.
//!
//! Everything a tab holds is here, and nothing about how it is drawn, so the rules
//! (what counts as unsaved, what a rename carries with it, when a result is out of
//! date) are tested without a window.
//!
//! Setups are kept by name, not position, so reordering, undo and redo never hand
//! one setup's result to another. A result is out of date when anything it was run
//! from has changed since: the setup, the tools it uses, its stock or its models.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use camcraft_core::job::{
    Job, LengthUnit, ModelDef, Operation, Origin, Setup, StockDef, StockSpec, StrategySpec,
    ToolSpec,
};
use camcraft_core::run::{Made, Plan, Stage};
use camcraft_core::setup::{Bodies, PrepareError, Prepared, prepare};
use camcraft_geom::bvh::MeshBvh;
use camcraft_session::fs::{job_dir, relative_path, resolve, with_job_extension};
use camcraft_session::{Model, Ran, SessionError, check_runnable, write_job};
use glam::DVec3;

use crate::camera::Camera;
use crate::live::LiveStock;
use crate::scene::Playhead;
use crate::scene::{Scene, box_edges};
use crate::stock_view::{ColorMode, StockImage};
use crate::viewport::{Outline, OutlineKind, View};
use crate::worker::DocId;

/// A number never handed out before, for telling uploads apart.
pub fn next_key() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// How much a log line matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// For the record.
    Info,
    /// Worth reading.
    Warn,
    /// Something failed.
    Error,
}

/// Which material to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StockMode {
    /// None.
    Off,
    /// As it stands at the playhead, cut as the program plays.
    Follow,
    /// A fixed stage: the block before everything is 0, after operation `n` is
    /// `n + 1`.
    Stage(usize),
}

/// What a model was loaded from: when this changes, it is loaded again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelKey {
    /// The file.
    pub path: PathBuf,
    /// What an STL's numbers mean.
    pub units: Option<LengthUnit>,
    /// The machining tolerance it was tessellated for, as bits.
    pub tolerance: u64,
}

impl ModelKey {
    /// The key for `def` in a job in `dir`, at `tolerance`.
    pub fn of(dir: &Path, def: &ModelDef, tolerance: f64) -> Self {
        Self {
            path: resolve(dir, &def.path),
            units: def.units,
            tolerance: tolerance.to_bits(),
        }
    }
}

/// A model, as far as loading it has got.
#[derive(Debug, Clone)]
pub enum ModelSlot {
    /// On its way.
    Loading(ModelKey),
    /// Loaded, with the index picking casts rays against.
    Ready(ModelKey, Arc<Model>, Arc<MeshBvh>),
    /// Not loaded, and why; `missing` when the file is not where the job says.
    Failed {
        /// What was asked for.
        key: ModelKey,
        /// Why not.
        why: String,
        /// The file is not there.
        missing: bool,
    },
}

impl ModelSlot {
    fn key(&self) -> &ModelKey {
        match self {
            ModelSlot::Loading(k) | ModelSlot::Ready(k, ..) | ModelSlot::Failed { key: k, .. } => k,
        }
    }

    /// The model, if loaded.
    pub fn model(&self) -> Option<&Arc<Model>> {
        match self {
            ModelSlot::Ready(_, m, _) => Some(m),
            _ => None,
        }
    }

    /// The model and its picking index, if loaded.
    pub fn loaded(&self) -> Option<(&Arc<Model>, &Arc<MeshBvh>)> {
        match self {
            ModelSlot::Ready(_, m, bvh) => Some((m, bvh)),
            _ => None,
        }
    }
}

/// What is selected in the job's tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// The job itself.
    Job,
    /// The models group.
    Models,
    /// One model, by name.
    Model(String),
    /// The stocks group.
    Stocks,
    /// One stock, by name.
    Stock(String),
    /// The setups group.
    Setups,
    /// One setup, by name.
    Setup(String),
    /// One operation: its setup's name and its place in it.
    Operation(String, usize),
}

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
    fn of(job: &Job, loaded: &BTreeMap<String, ModelSlot>, index: usize) -> Option<Self> {
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
    fn layout(&self) -> Self {
        let mut setup = self.setup.clone();
        setup.operations.clear();
        Self {
            tolerance: self.tolerance,
            setup,
            tools: BTreeMap::new(),
            stocks: self.stocks.clone(),
            models: self.models.clone(),
        }
    }
}

/// A setup's last run, and what is drawn from it.
#[derive(Debug, Clone)]
pub struct RanSetup {
    /// What it was run from.
    pub inputs: SetupInputs,
    /// The job as it was run.
    pub job: Job,
    /// What running it produced.
    pub ran: Ran,
    /// Its toolpaths and cutters, for drawing.
    pub scene: Scene,
    /// Tells this scene's upload from any other.
    pub scene_key: u64,
}

/// A setup laid out for drawing, before or without a run.
#[derive(Debug, Clone)]
pub struct Layout {
    /// What it was laid out from, and which of its models had loaded.
    inputs: (SetupInputs, Vec<String>),
    /// The part and stock in the setup's frame, or why not.
    pub prepared: Result<Prepared, String>,
    /// Stock edges and the work-zero axes.
    pub outline: Vec<Outline>,
    /// Tells this layout's uploads from any other.
    pub key: u64,
}

/// How a setup is being looked at: its own camera, scrub and choices, kept while
/// another is shown.
#[derive(Debug, Clone)]
pub struct SetupView {
    /// The camera, once it has been shown.
    pub camera: Option<Camera>,
    /// Frames the camera once, on the first layout that has a part.
    pub framing: u64,
    /// Where the program stands: the scrub, and what playback moves.
    pub at: Playhead,
    /// Playing, and how.
    pub transport: Transport,
    /// The cutter at the scrub.
    pub show_cutter: bool,
    /// All toolpaths at once, over the per-role choices.
    pub show_paths: bool,
    /// Which material to draw.
    pub stock_mode: StockMode,
    /// The material covers the part, so the part is hidden while it shows.
    pub hide_part_with_stock: bool,
    /// What to draw.
    pub view: View,
    /// The material at each stage, classified and coloured, built when first shown,
    /// each with a key for its upload.
    pub stock_images: Vec<Option<(u64, StockImage)>>,
}

/// Playback: whether it plays, which way, how fast, and where it stops by itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transport {
    /// Playing.
    pub playing: bool,
    /// Playing backwards.
    pub reverse: bool,
    /// Machining seconds per second.
    pub speed: f64,
    /// Pause on reaching a failed check or a steep descent.
    pub stop_at_problems: bool,
    /// Pause at the end of each operation.
    pub stop_at_op_end: bool,
    /// How the material is coloured.
    pub color: ColorMode,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            playing: false,
            reverse: false,
            speed: 10.0,
            stop_at_problems: true,
            stop_at_op_end: false,
            color: ColorMode::Material,
        }
    }
}

impl Default for SetupView {
    fn default() -> Self {
        Self {
            camera: None,
            framing: next_key(),
            at: Playhead::default(),
            transport: Transport::default(),
            show_cutter: true,
            show_paths: true,
            stock_mode: StockMode::Off,
            hide_part_with_stock: true,
            view: View {
                stock: true,
                ..View::default()
            },
            stock_images: Vec::new(),
        }
    }
}

/// Everything about one setup that is not in the job.
#[derive(Debug, Default)]
pub struct SetupState {
    /// Its last run.
    pub ran: Option<RanSetup>,
    /// The run in flight, if any, and the job as it was sent.
    pub running: Option<(u64, Job)>,
    /// Where the run in flight has got to, and since when.
    pub progress: Option<(Stage, Instant)>,
    /// How it is laid out now.
    pub layout: Option<Layout>,
    /// How it is being looked at.
    pub view: SetupView,
    /// Its material during playback, once it has been followed.
    pub live: Option<LiveStock>,
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

/// One open job.
#[derive(Debug)]
pub struct Document {
    /// Tells this job's work from any other's.
    pub id: DocId,
    /// Where it is saved; `None` until it is.
    pub path: Option<PathBuf>,
    /// The job as edited.
    pub job: Job,
    saved: Job,
    /// The job when last checkpointed, and what came before and after it.
    checkpoint: Job,
    undo: Vec<Job>,
    redo: Vec<Job>,
    /// Its models, by name, as far as loading has got.
    pub models: BTreeMap<String, ModelSlot>,
    /// Its setups' state, by name.
    pub setups: BTreeMap<String, SetupState>,
    /// What is selected in its tree.
    pub selection: Selection,
    /// The setup the view shows.
    pub active: Option<String>,
    /// What has happened, for the log.
    pub log: Vec<(Level, String)>,
    /// Missing models have been asked about.
    pub asked_missing: bool,
    /// Runs asked for so far, to tell the latest.
    seq: u64,
    /// The selection the view last followed.
    followed: Option<Selection>,
}

impl Document {
    /// A new, empty job: one stock around whatever models are added, and one setup
    /// using it.
    pub fn untitled(id: DocId) -> Self {
        let mut job = Job::default();
        job.stocks.push(StockDef {
            name: "Stock 1".into(),
            spec: StockSpec::default(),
        });
        let mut setup = Setup::new("Setup 1");
        setup.stocks = vec!["Stock 1".into()];
        setup.origin = Origin::stock_corner();
        job.setups.push(setup);
        let mut doc = Self::opened(id, None, job);
        doc.selection = Selection::Setup("Setup 1".into());
        doc
    }

    /// A job read from `path`, or new when `path` is `None`.
    pub fn opened(id: DocId, path: Option<PathBuf>, job: Job) -> Self {
        let active = job.setups.first().map(|s| s.name.clone());
        let selection = active.clone().map_or(Selection::Job, Selection::Setup);
        Self {
            id,
            path,
            saved: job.clone(),
            checkpoint: job.clone(),
            job,
            undo: Vec::new(),
            redo: Vec::new(),
            models: BTreeMap::new(),
            setups: BTreeMap::new(),
            selection,
            active,
            log: Vec::new(),
            asked_missing: false,
            seq: 0,
            followed: None,
        }
    }

    /// The name on its tab.
    pub fn title(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map_or_else(|| "Untitled".into(), |s| s.to_string_lossy().into_owned())
    }

    /// Whether it has changed since it was saved.
    pub fn dirty(&self) -> bool {
        self.job != self.saved
    }

    /// The folder its relative paths start from.
    pub fn dir(&self) -> PathBuf {
        self.path
            .as_deref()
            .map_or_else(|| std::env::current_dir().unwrap_or_default(), job_dir)
    }

    /// Say something in the log.
    pub fn note(&mut self, level: Level, text: impl Into<String>) {
        self.log.push((level, text.into()));
    }

    // ---- models ---------------------------------------------------------------

    /// Models that should be loaded and are not, or are loaded from something that
    /// has since changed: each with what to load it as.
    pub fn wanted_loads(&self) -> Vec<(ModelDef, ModelKey)> {
        let dir = self.dir();
        self.job
            .models
            .iter()
            .filter_map(|def| {
                let key = ModelKey::of(&dir, def, self.job.tolerance);
                let current = self.models.get(&def.name);
                // An STL waits for its units rather than failing without them.
                let stl = Path::new(&def.path)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("stl"));
                if stl && def.units.is_none() {
                    return None;
                }
                (current.map(ModelSlot::key) != Some(&key)).then(|| (def.clone(), key))
            })
            .collect()
    }

    /// A model has started loading.
    pub fn loading(&mut self, name: &str, key: ModelKey) {
        self.models
            .insert(name.to_string(), ModelSlot::Loading(key));
    }

    /// A model has loaded, or failed to. A reply for something since asked for
    /// differently is dropped.
    pub fn loaded(
        &mut self,
        name: &str,
        key: &ModelKey,
        result: Result<(Arc<Model>, Arc<MeshBvh>), SessionError>,
    ) {
        match self.models.get(name) {
            Some(ModelSlot::Loading(k)) if k == key => {}
            _ => return,
        }
        let slot = match result {
            Ok((model, bvh)) => {
                for n in &model.notes {
                    self.log.push((Level::Info, format!("{name}: {n}")));
                }
                for w in &model.warnings {
                    self.log.push((Level::Warn, format!("{name}: {w}")));
                }
                ModelSlot::Ready(key.clone(), model, bvh)
            }
            Err(e) => {
                let missing = matches!(e, SessionError::ModelMissing { .. });
                let why = crate::app::explain(&e);
                self.note(Level::Error, why.clone());
                ModelSlot::Failed {
                    key: key.clone(),
                    why,
                    missing,
                }
            }
        };
        self.models.insert(name.to_string(), slot);
    }

    /// The names of models whose files are not where the job says.
    pub fn missing_models(&self) -> Vec<String> {
        self.models
            .iter()
            .filter(|(_, s)| matches!(s, ModelSlot::Failed { missing: true, .. }))
            .map(|(n, _)| n.clone())
            .collect()
    }

    /// How the job names `file`: relative to it when it has been saved, whole when
    /// it has not.
    fn job_path_for(&self, file: &Path) -> String {
        match &self.path {
            Some(_) => relative_path(&self.dir(), file),
            None => file.display().to_string(),
        }
    }

    /// Add model files, each under a name from its file name. A setup that has no
    /// models yet takes them. Returns the names given.
    pub fn add_models(&mut self, files: &[PathBuf]) -> Vec<String> {
        let mut names = Vec::new();
        for file in files {
            let base = file
                .file_stem()
                .map_or_else(|| "model".into(), |s| s.to_string_lossy().into_owned());
            let name = free(&base, self.job.models.iter().map(|m| m.name.as_str()));
            let stl = file
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("stl"));
            self.job.models.push(ModelDef {
                name: name.clone(),
                path: self.job_path_for(file),
                // An STL says nothing of its units; millimetres is a start the
                // person sees, and can change, before anything is cut.
                units: stl.then_some(LengthUnit::Mm),
            });
            names.push(name);
        }
        for setup in &mut self.job.setups {
            if setup.models.is_empty() {
                setup.models.clone_from(&names);
            }
        }
        if let Some(last) = names.last() {
            self.selection = Selection::Model(last.clone());
        }
        names
    }

    /// Have setup `setup` machine `names` too, and show it: models added from its
    /// panel are added to it.
    pub fn include_models(&mut self, setup: &str, names: &[String]) {
        let Some(s) = self.job.setups.iter_mut().find(|s| s.name == setup) else {
            return;
        };
        for n in names {
            if !s.models.contains(n) {
                s.models.push(n.clone());
            }
        }
        self.selection = Selection::Setup(setup.to_string());
    }

    /// Point model `name` at `file`, as when it has been found elsewhere.
    pub fn locate_model(&mut self, name: &str, file: &Path) {
        let path = self.job_path_for(file);
        if let Some(m) = self.job.models.iter_mut().find(|m| m.name == name) {
            m.path = path;
        }
    }

    /// Rename a model, and every reference to it.
    pub fn rename_model(&mut self, from: &str, to: &str) -> Result<(), String> {
        let to = checked_name(to, "model", self.job.models.iter().map(|m| &*m.name), from)?;
        if to == from {
            return Ok(());
        }
        let Some(m) = self.job.models.iter_mut().find(|m| m.name == from) else {
            return Err(format!("there is no model '{from}'"));
        };
        m.name.clone_from(&to);
        for s in &mut self.job.setups {
            rename_in(&mut s.models, from, &to);
        }
        for s in &mut self.job.stocks {
            if let StockSpec::AroundModels { models, .. } = &mut s.spec {
                rename_in(models, from, &to);
            }
        }
        self.job.rename_model_refs(from, &to);
        if let Some(slot) = self.models.remove(from) {
            self.models.insert(to.clone(), slot);
        }
        if self.selection == Selection::Model(from.to_string()) {
            self.selection = Selection::Model(to);
        }
        Ok(())
    }

    /// Remove a model nothing uses.
    pub fn remove_model(&mut self, name: &str) -> Result<(), String> {
        let users: Vec<String> = self
            .job
            .setups
            .iter()
            .filter(|s| s.models.iter().any(|m| m == name))
            .map(|s| format!("setup '{}'", s.name))
            .chain(
                self.job
                    .stocks
                    .iter()
                    .filter(|s| {
                        matches!(&s.spec, StockSpec::AroundModels { models, .. }
                            if models.iter().any(|m| m == name))
                    })
                    .map(|s| format!("stock '{}'", s.name)),
            )
            .collect();
        if !users.is_empty() {
            return Err(format!("'{name}' is used by {}", users.join(", ")));
        }
        self.job.models.retain(|m| m.name != name);
        self.models.remove(name);
        self.selection = Selection::Models;
        Ok(())
    }

    // ---- stocks ---------------------------------------------------------------

    /// Add a stock around the models of whichever setup uses it.
    pub fn add_stock(&mut self) -> String {
        let name = free("Stock", self.job.stocks.iter().map(|s| s.name.as_str()));
        self.job.stocks.push(StockDef {
            name: name.clone(),
            spec: StockSpec::default(),
        });
        self.selection = Selection::Stock(name.clone());
        name
    }

    /// A copy of stock `name`, after it, and shown. No setup uses it yet.
    pub fn duplicate_stock(&mut self, name: &str) -> Option<String> {
        let i = self.job.stocks.iter().position(|s| s.name == name)?;
        let mut copy = self.job.stocks.get(i)?.clone();
        copy.name = free(
            &format!("{name} copy"),
            self.job.stocks.iter().map(|s| s.name.as_str()),
        );
        let new = copy.name.clone();
        self.job.stocks.insert(i + 1, copy);
        self.selection = Selection::Stock(new.clone());
        Some(new)
    }

    /// Rename a stock, and every setup's reference to it.
    pub fn rename_stock(&mut self, from: &str, to: &str) -> Result<(), String> {
        let to = checked_name(to, "stock", self.job.stocks.iter().map(|s| &*s.name), from)?;
        if to == from {
            return Ok(());
        }
        let Some(s) = self.job.stocks.iter_mut().find(|s| s.name == from) else {
            return Err(format!("there is no stock '{from}'"));
        };
        s.name.clone_from(&to);
        for s in &mut self.job.setups {
            rename_in(&mut s.stocks, from, &to);
        }
        if self.selection == Selection::Stock(from.to_string()) {
            self.selection = Selection::Stock(to);
        }
        Ok(())
    }

    /// Remove a stock no setup uses.
    pub fn remove_stock(&mut self, name: &str) -> Result<(), String> {
        let users: Vec<&str> = self
            .job
            .setups
            .iter()
            .filter(|s| s.stocks.iter().any(|m| m == name))
            .map(|s| s.name.as_str())
            .collect();
        if !users.is_empty() {
            return Err(format!(
                "'{name}' is used by setup '{}'",
                users.join("', '")
            ));
        }
        self.job.stocks.retain(|s| s.name != name);
        self.selection = Selection::Stocks;
        Ok(())
    }

    // ---- setups ---------------------------------------------------------------

    /// Where setup `name` is in the job.
    pub fn setup_index(&self, name: &str) -> Option<usize> {
        self.job.setups.iter().position(|s| s.name == name)
    }

    /// Add a setup machining every model, from the first stock.
    pub fn add_setup(&mut self) -> String {
        let name = free("Setup", self.job.setups.iter().map(|s| s.name.as_str()));
        let mut setup = Setup::new(name.clone());
        setup.origin = Origin::stock_corner();
        setup.models = self.job.models.iter().map(|m| m.name.clone()).collect();
        setup.stocks = self
            .job
            .stocks
            .first()
            .map(|s| vec![s.name.clone()])
            .unwrap_or_default();
        self.job.setups.push(setup);
        self.show(&name);
        name
    }

    /// A copy of setup `name`, after it.
    pub fn duplicate_setup(&mut self, name: &str) -> Option<String> {
        let i = self.setup_index(name)?;
        let mut copy = self.job.setups.get(i)?.clone();
        copy.name = free(
            &format!("{name} copy"),
            self.job.setups.iter().map(|s| s.name.as_str()),
        );
        let new = copy.name.clone();
        self.job.setups.insert(i + 1, copy);
        self.show(&new);
        Some(new)
    }

    /// Rename a setup, carrying its state with it.
    pub fn rename_setup(&mut self, from: &str, to: &str) -> Result<(), String> {
        let to = checked_name(to, "setup", self.job.setups.iter().map(|s| &*s.name), from)?;
        if to == from {
            return Ok(());
        }
        let Some(s) = self.job.setups.iter_mut().find(|s| s.name == from) else {
            return Err(format!("there is no setup '{from}'"));
        };
        s.name.clone_from(&to);
        if let Some(state) = self.setups.remove(from) {
            self.setups.insert(to.clone(), state);
        }
        if self.active.as_deref() == Some(from) {
            self.active = Some(to.clone());
        }
        self.selection = match &self.selection {
            Selection::Setup(s) if s == from => Selection::Setup(to),
            Selection::Operation(s, i) if s == from => Selection::Operation(to, *i),
            other => other.clone(),
        };
        Ok(())
    }

    /// Remove a setup and what it ran.
    pub fn remove_setup(&mut self, name: &str) {
        self.job.setups.retain(|s| s.name != name);
        self.setups.remove(name);
        if self.active.as_deref() == Some(name) {
            self.active = self.job.setups.first().map(|s| s.name.clone());
        }
        self.selection = Selection::Setups;
    }

    /// Move a setup up (`-1`) or down (`1`).
    pub fn move_setup(&mut self, name: &str, by: isize) {
        if let Some(i) = self.setup_index(name) {
            shift(&mut self.job.setups, i, by);
        }
    }

    /// Show setup `name` in the view, and select it.
    pub fn show(&mut self, name: &str) {
        self.active = Some(name.to_string());
        self.selection = Selection::Setup(name.to_string());
    }

    // ---- operations -----------------------------------------------------------

    /// Add an operation of `strategy` at the end of setup `setup`, with the tool
    /// its last operation used.
    pub fn add_operation(&mut self, setup: &str, strategy: StrategySpec) -> Option<usize> {
        let s = self.job.setups.iter_mut().find(|s| s.name == setup)?;
        let base = match strategy {
            StrategySpec::Parallel { .. } => "finish",
            StrategySpec::ZlevelRough { .. } => "rough",
            StrategySpec::Drill { .. } => "drill",
        };
        let name = free(base, s.operations.iter().map(|o| o.name.as_str()));
        let tool = s.operations.last().and_then(|o| o.tool.clone());
        let preset = tool
            .as_ref()
            .and_then(|t| self.job.tools.get(t))
            .and_then(|t| t.presets.first());
        let mut op = Operation {
            name,
            strategy,
            tool,
            preset: None,
            geometry: Default::default(),
            heights: Default::default(),
            passes: Default::default(),
            linking: Default::default(),
            enabled: true,
        };
        if let Some(p) = preset {
            op.apply_preset(p);
        }
        s.operations.push(op);
        let i = s.operations.len() - 1;
        self.active = Some(setup.to_string());
        self.selection = Selection::Operation(setup.to_string(), i);
        Some(i)
    }

    /// A copy of an operation, after it.
    pub fn duplicate_operation(&mut self, setup: &str, index: usize) {
        let Some(s) = self.job.setups.iter_mut().find(|s| s.name == setup) else {
            return;
        };
        let Some(mut op) = s.operations.get(index).cloned() else {
            return;
        };
        op.name = free(
            &format!("{} copy", op.name),
            s.operations.iter().map(|o| o.name.as_str()),
        );
        s.operations.insert(index + 1, op);
        self.selection = Selection::Operation(setup.to_string(), index + 1);
    }

    /// Remove an operation.
    pub fn remove_operation(&mut self, setup: &str, index: usize) {
        if let Some(s) = self.job.setups.iter_mut().find(|s| s.name == setup)
            && index < s.operations.len()
        {
            s.operations.remove(index);
        }
        self.selection = Selection::Setup(setup.to_string());
    }

    /// Move operation `from` of `setup` so it is at `to`, the others closing up
    /// behind it and making room; the selection stays on the operation it was on.
    pub fn reorder_operation(&mut self, setup: &str, from: usize, to: usize) {
        let Some(s) = self.job.setups.iter_mut().find(|s| s.name == setup) else {
            return;
        };
        let n = s.operations.len();
        if from >= n || to >= n || from == to {
            return;
        }
        let op = s.operations.remove(from);
        s.operations.insert(to, op);
        if let Selection::Operation(sel, k) = &mut self.selection
            && sel == setup
        {
            *k = moved_index(*k, from, to);
        }
    }

    /// Move operation `from` of setup `from_setup` to place `to` of setup
    /// `to_setup`: within one setup, as [`Document::reorder_operation`]; into
    /// another, under a name free there, and shown there. It keeps its tool, its
    /// heights and its picks, which find what they can among the new setup's
    /// models.
    pub fn move_operation_to(&mut self, from_setup: &str, from: usize, to_setup: &str, to: usize) {
        if from_setup == to_setup {
            let last = self
                .setup_index(from_setup)
                .and_then(|i| self.job.setups.get(i))
                .map_or(0, |s| s.operations.len().saturating_sub(1));
            self.reorder_operation(from_setup, from, to.min(last));
            return;
        }
        let (Some(si), Some(ti)) = (self.setup_index(from_setup), self.setup_index(to_setup))
        else {
            return;
        };
        let Some(source) = self.job.setups.get_mut(si) else {
            return;
        };
        if from >= source.operations.len() {
            return;
        }
        let mut op = source.operations.remove(from);
        let Some(target) = self.job.setups.get_mut(ti) else {
            return;
        };
        op.name = free(&op.name, target.operations.iter().map(|o| o.name.as_str()));
        let to = to.min(target.operations.len());
        target.operations.insert(to, op);
        self.show(to_setup);
        self.selection = Selection::Operation(to_setup.to_string(), to);
    }

    /// Move an operation up (`-1`) or down (`1`) within its setup.
    pub fn move_operation(&mut self, setup: &str, index: usize, by: isize) {
        if let Some(s) = self.job.setups.iter_mut().find(|s| s.name == setup)
            && let Some(to) = shift(&mut s.operations, index, by)
        {
            self.selection = Selection::Operation(setup.to_string(), to);
        }
    }

    // ---- tools ----------------------------------------------------------------

    /// Use `spec`, from the library as `hint`, for an operation: the job's own copy
    /// if it already has an identical one, otherwise a new copy under a free name.
    /// Returns the name in the job.
    pub fn adopt_tool(&mut self, hint: &str, spec: &ToolSpec) -> String {
        if let Some((name, _)) = self.job.tools.iter().find(|(_, t)| *t == spec) {
            return name.clone();
        }
        let name = camcraft_session::library::free_name(hint, &self.job.tools);
        self.job.tools.insert(name.clone(), spec.clone());
        name
    }

    /// Give operation `op` of `setup` the job's tool `tool`, and copy in the values
    /// of its preset `preset`. Without one, the operation keeps its own values and
    /// no longer names a preset.
    pub fn use_tool(&mut self, setup: &str, op: usize, tool: String, preset: Option<&str>) {
        let preset = preset.and_then(|p| self.job.tools.get(&tool)?.preset(p).cloned());
        let Some(o) = self
            .job
            .setups
            .iter_mut()
            .find(|s| s.name == setup)
            .and_then(|s| s.operations.get_mut(op))
        else {
            return;
        };
        o.tool = Some(tool);
        match preset {
            Some(p) => o.apply_preset(&p),
            None => o.preset = None,
        }
    }

    /// Another tool the job's operations use with the same number as tool `name`,
    /// when `name` is used too: the machine would load one and cut with the other.
    pub fn number_clash(&self, name: &str) -> Option<String> {
        let number = self.job.tools.get(name)?.number;
        let used = self.used_tools();
        if !used.iter().any(|n| n == name) {
            return None;
        }
        used.into_iter()
            .find(|n| n != name && self.job.tools.get(n).is_some_and(|t| t.number == number))
    }

    /// A tool number no tool in the job has.
    pub fn free_number(&self) -> u32 {
        self.job.tools.values().map(|t| t.number).max().unwrap_or(0) + 1
    }

    /// The names of the tools the job's operations use.
    pub fn used_tools(&self) -> Vec<String> {
        let mut used: Vec<String> = self
            .job
            .setups
            .iter()
            .flat_map(|s| &s.operations)
            .filter_map(|o| o.tool.clone())
            .collect();
        used.sort();
        used.dedup();
        used
    }

    // ---- saving and undo ------------------------------------------------------

    /// Save to `path`: model paths made relative to it, tools no operation uses
    /// dropped, written whole or not at all.
    pub fn save_to(&mut self, path: &Path) -> Result<(), SessionError> {
        let path = with_job_extension(path);
        let new_dir = job_dir(&path);
        let old_dir = self.dir();
        let mut job = self.job.clone();
        for m in &mut job.models {
            let file = resolve(&old_dir, &m.path);
            m.path = relative_path(&new_dir, &file);
        }
        let used = self.used_tools();
        job.tools.retain(|name, _| used.contains(name));
        write_job(&path, &job)?;
        // The keys are unchanged, since each still resolves to the same file.
        self.job = job.clone();
        self.checkpoint = job.clone();
        self.saved = job;
        self.path = Some(path);
        Ok(())
    }

    /// Note an edit as finished: what the job was before it becomes one undo.
    pub fn checkpoint(&mut self) {
        if self.job != self.checkpoint {
            let before = std::mem::replace(&mut self.checkpoint, self.job.clone());
            self.undo.push(before);
            // Plenty to recover from a slip, and never unbounded.
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    /// Whether there is an edit to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.job != self.checkpoint
    }

    /// Whether there is an undone edit to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Undo the last edit.
    pub fn undo(&mut self) {
        self.checkpoint();
        if let Some(before) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.job, before));
            self.checkpoint = self.job.clone();
            self.tidy_selection();
        }
    }

    /// Redo the last undone edit.
    pub fn redo(&mut self) {
        if let Some(after) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.job, after));
            self.checkpoint = self.job.clone();
            self.tidy_selection();
        }
    }

    /// Show the toolpaths of what is selected: all of a setup's, or an
    /// operation's alone. Only when the selection changes, or a run finishes,
    /// so what is ticked under Show in between is left as it is.
    pub fn follow_selection(&mut self) {
        if self.followed.as_ref() == Some(&self.selection) {
            return;
        }
        self.followed = Some(self.selection.clone());
        let (setup, only) = match &self.selection {
            Selection::Setup(s) => (s, None),
            Selection::Operation(s, i) => (s, Some(*i)),
            _ => return,
        };
        let view = &mut self.setups.entry(setup.clone()).or_default().view;
        view.show_paths = true;
        view.view.ops = match only {
            None => u32::MAX,
            Some(i) => u32::try_from(i)
                .ok()
                .and_then(|i| 1u32.checked_shl(i))
                .unwrap_or(0),
        };
    }

    /// Keep the selection and the view on things that still exist.
    pub fn tidy_selection(&mut self) {
        let setups: Vec<&str> = self.job.setups.iter().map(|s| s.name.as_str()).collect();
        let ok = match &self.selection {
            Selection::Model(m) => self.job.model(m).is_some(),
            Selection::Stock(s) => self.job.stock(s).is_some(),
            Selection::Setup(s) => setups.contains(&s.as_str()),
            Selection::Operation(s, i) => self
                .job
                .setups
                .iter()
                .find(|x| x.name == *s)
                .is_some_and(|x| *i < x.operations.len()),
            _ => true,
        };
        if !ok {
            self.selection = Selection::Job;
        }
        if self.active.as_deref().is_none_or(|a| !setups.contains(&a)) {
            self.active = setups.first().map(|s| (*s).to_string());
        }
        self.setups
            .retain(|name, _| setups.contains(&name.as_str()));
    }

    // ---- running --------------------------------------------------------------

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

    /// Why setup `index` cannot run now, if it cannot.
    pub fn cannot_run(&self, index: usize) -> Option<String> {
        self.cannot_generate(index, None)
    }

    /// Why setup `index`, or its operation `only` alone, cannot be generated
    /// now, if it cannot. Only the operations the run would work out or keep
    /// are checked: the rest are left out, and may be unfinished.
    pub fn cannot_generate(&self, index: usize, only: Option<usize>) -> Option<String> {
        if let Some(k) = only
            && self
                .job
                .setups
                .get(index)
                .and_then(|s| s.operations.get(k))
                .is_some_and(|o| !o.enabled)
        {
            return Some("it is turned off".into());
        }
        if let Err(e) = check_runnable(&self.job, index, &self.plans(index, only)) {
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
        let ran = self.setups.get(&setup.name).and_then(|s| s.ran.as_ref());
        (0..setup.operations.len())
            .map(|i| {
                if i == k {
                    return Plan::Generate;
                }
                match ran.and_then(|r| r.ran.result.operations.get(i)) {
                    Some(op) if op.made.has_paths() && self.op_unchanged(&setup.name, i) => {
                        Plan::Keep(Box::new(op.clone()))
                    }
                    _ => Plan::Skip,
                }
            })
            .collect()
    }

    /// Whether operation `i` of setup `name` is as it was when the setup last
    /// ran: the operation, its tool, and where the part and stock sit.
    fn op_unchanged(&self, name: &str, i: usize) -> bool {
        let then = self
            .setups
            .get(name)
            .and_then(|s| s.ran.as_ref())
            .map(|r| &r.inputs);
        let now = self
            .setup_index(name)
            .and_then(|x| SetupInputs::of(&self.job, &self.models, x));
        let (Some(then), Some(now)) = (then, now) else {
            return false;
        };
        let (Some(a), Some(b)) = (then.setup.operations.get(i), now.setup.operations.get(i)) else {
            return false;
        };
        let tool = |inputs: &SetupInputs, op: &Operation| {
            op.tool.as_ref().map(|t| inputs.tools.get(t).cloned())
        };
        a == b && tool(then, a) == tool(&now, b) && then.layout() == now.layout()
    }

    /// Turn operation `index` of `setup` on or off.
    pub fn set_enabled(&mut self, setup: &str, index: usize, on: bool) {
        if let Some(op) = self
            .job
            .setups
            .iter_mut()
            .find(|s| s.name == setup)
            .and_then(|s| s.operations.get_mut(index))
        {
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
        let off = self
            .setup_index(name)
            .and_then(|i| self.job.setups.get(i)?.operations.get(index))
            .is_some_and(|o| !o.enabled);
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
            .and_then(|i| SetupInputs::of(&self.job, &self.models, i));
        // Anything changed, or an operation that should run left out of it.
        let partial = ran
            .ran
            .result
            .operations
            .iter()
            .any(|o| o.made == Made::Skipped);
        if now.as_ref() != Some(&ran.inputs) || partial {
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
        let inputs = SetupInputs::of(&self.job, &self.models, index).map(|i| i.layout())?;
        // A model that finishes loading changes the layout without changing the job.
        let ready: Vec<String> = self
            .models
            .iter()
            .filter(|(_, s)| s.model().is_some())
            .map(|(n, _)| n.clone())
            .collect();
        let inputs = (inputs, ready);
        if self
            .setups
            .get(name)
            .and_then(|s| s.layout.as_ref())
            .is_some_and(|l| l.inputs == inputs)
        {
            return None;
        }
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

/// A name not in `taken`. Setups and stocks count from one ("Setup 1", "Setup
/// 2"); anything else keeps `base` until it is taken ("rough", "rough 2").
fn free<'a>(base: &str, taken: impl Iterator<Item = &'a str>) -> String {
    let taken: Vec<&str> = taken.collect();
    let counted = matches!(base, "Setup" | "Stock");
    (1..)
        .map(|n| {
            if n == 1 && !counted {
                base.to_string()
            } else {
                format!("{base} {n}")
            }
        })
        .find(|n| !taken.contains(&n.as_str()))
        .unwrap_or_else(|| base.to_string())
}

/// `to`, trimmed, if it can name a `what` among `names` in place of `from`.
fn checked_name<'a>(
    to: &str,
    what: &str,
    mut names: impl Iterator<Item = &'a str>,
    from: &str,
) -> Result<String, String> {
    let to = to.trim();
    if to.is_empty() {
        return Err(format!("a {what} needs a name"));
    }
    if to != from && names.any(|n| n == to) {
        return Err(format!("there is already a {what} called '{to}'"));
    }
    Ok(to.to_string())
}

fn rename_in(names: &mut [String], from: &str, to: &str) {
    for n in names.iter_mut().filter(|n| *n == from) {
        *n = to.to_string();
    }
}

/// Move item `i` by `by` places, within the list; where it ended up.
/// Where the item at `k` ends up when the one at `from` is moved to `to`.
fn moved_index(k: usize, from: usize, to: usize) -> usize {
    if k == from {
        to
    } else if from < k && k <= to {
        k - 1
    } else if to <= k && k < from {
        k + 1
    } else {
        k
    }
}

fn shift<T>(items: &mut [T], i: usize, by: isize) -> Option<usize> {
    let to = i.checked_add_signed(by)?;
    if to >= items.len() || i >= items.len() {
        return None;
    }
    items.swap(i, to);
    Some(to)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]
mod tests {
    use super::*;
    use camcraft_core::job::Preset;

    fn example() -> Document {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/rail-jig.camcraft");
        let job = camcraft_session::read_job(&path).unwrap();
        Document::opened(1, Some(path), job)
    }

    /// A new job whose setup machines a plate with a hole, loaded as if from disk.
    fn with_plate() -> (Document, String) {
        let mut doc = Document::untitled(1);
        let def = ModelDef {
            name: "p".into(),
            path: "p.stl".into(),
            units: Some(LengthUnit::Mm),
        };
        let body = camcraft_geom::topology::Body::derived(
            camcraft_geom::synthetic::plate_with_hole(20.0, 5.0, 4.0, 32),
        );
        let bvh = Arc::new(MeshBvh::new(body.mesh()));
        let model = Arc::new(Model {
            bounds: body.mesh().bounds().unwrap(),
            body,
            path: "p.stl".into(),
            units: "mm".into(),
            notes: Vec::new(),
            warnings: Vec::new(),
        });
        let key = ModelKey::of(Path::new("."), &def, doc.job.tolerance);
        doc.models
            .insert("p".into(), ModelSlot::Ready(key, model, bvh));
        doc.job.models.push(def);
        doc.job.setups[0].models = vec!["p".into()];
        let setup = doc.job.setups[0].name.clone();
        (doc, setup)
    }

    /// Moving the work zero moves everything drawn, by as much the other way:
    /// the layout says so, so the view can stay on the part.
    #[test]
    fn a_moved_work_zero_says_how_far_the_drawing_moved() {
        let (mut doc, setup) = with_plate();
        doc.job.setups[0].origin = Origin::ModelOrigin { offset: [0.0; 3] };
        assert_eq!(
            doc.refresh_layout(&setup),
            None,
            "the first layout moves nothing"
        );
        doc.job.setups[0].origin = Origin::ModelOrigin {
            offset: [1.0, 2.0, 5.0],
        };
        assert_eq!(
            doc.refresh_layout(&setup),
            Some(DVec3::new(-1.0, -2.0, -5.0))
        );
        assert_eq!(doc.refresh_layout(&setup), None, "nothing changed since");
        // Turned over, the picture changes altogether: nothing to keep.
        doc.job.setups[0].orientation = doc.job.setups[0].orientation.flipped();
        assert_eq!(doc.refresh_layout(&setup), None);
    }

    /// A work zero picked on something no longer there still draws the part, from
    /// the model's own zero and without axes, so it can be picked again.
    #[test]
    fn a_lost_work_zero_still_shows_the_part() {
        use camcraft_core::job::{PointRef, Snap};
        let (mut doc, setup) = with_plate();
        doc.job.setups[0].origin = Origin::Point {
            point: PointRef {
                model: "p".into(),
                snap: Snap::Vertex,
                index: 0,
                at: [99.0, 99.0, 99.0],
            },
            offset: [0.0; 3],
        };
        doc.refresh_layout(&setup);
        let layout = doc.setups[&setup].layout.as_ref().unwrap();
        let p = layout.prepared.as_ref().unwrap();
        assert!(p.frame.origin.length() < 1e-12);
        assert!(!layout.outline.is_empty());
        assert!(layout.outline.iter().all(|l| l.kind == OutlineKind::Stock));
    }

    #[test]
    fn a_new_job_is_one_setup_from_one_stock() {
        let doc = Document::untitled(1);
        assert_eq!(doc.title(), "Untitled");
        assert!(!doc.dirty());
        assert_eq!(doc.job.setups.len(), 1);
        assert_eq!(doc.job.setups[0].stocks, ["Stock 1"]);
        assert!(doc.job.check_common().is_ok());
    }

    #[test]
    fn an_edit_is_unsaved_until_saved() {
        let mut doc = example();
        assert!(!doc.dirty());
        doc.job.tolerance = 0.1;
        assert!(doc.dirty());
        doc.job.tolerance = 0.05;
        assert!(!doc.dirty(), "back as it was is not a change");
    }

    /// Models added to a new job land in its setup, and save relative to the job.
    #[test]
    fn models_are_added_and_saved_relative() {
        let dir = std::env::temp_dir().join(format!("camcraft-doc-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("jobs")).unwrap();
        std::fs::create_dir_all(dir.join("models")).unwrap();
        let model = dir.join("models/part.step");
        std::fs::write(&model, "").unwrap();

        let mut doc = Document::untitled(1);
        let names = doc.add_models(std::slice::from_ref(&model));
        assert_eq!(names, ["part"]);
        assert_eq!(doc.job.setups[0].models, ["part"]);
        assert!(Path::new(&doc.job.models[0].path).is_absolute());

        doc.save_to(&dir.join("jobs/first")).unwrap();
        assert_eq!(
            doc.path.as_deref(),
            Some(dir.join("jobs/first.camcraft").as_path())
        );
        assert_eq!(doc.job.models[0].path, "../models/part.step");
        assert!(!doc.dirty());

        // Saved somewhere else, it still finds the model.
        std::fs::create_dir_all(dir.join("jobs/deeper")).unwrap();
        doc.save_to(&dir.join("jobs/deeper/second.camcraft"))
            .unwrap();
        assert_eq!(doc.job.models[0].path, "../../models/part.step");
        let again = camcraft_session::read_job(&dir.join("jobs/deeper/second.camcraft")).unwrap();
        assert_eq!(again, doc.job);
    }

    /// Saving drops tools no operation uses.
    #[test]
    fn unused_tools_are_not_saved() {
        let dir = std::env::temp_dir().join(format!("camcraft-doc-prune-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut doc = example();
        doc.job.tools.insert("spare".into(), ToolSpec::default());
        doc.save_to(&dir.join("pruned.camcraft")).unwrap();
        assert!(!doc.job.tools.contains_key("spare"));
        assert_eq!(doc.job.tools.len(), 2);
    }

    #[test]
    fn renames_carry_every_reference() {
        let mut doc = example();
        doc.rename_model("sled_base", "sled").unwrap();
        assert_eq!(doc.job.setups[0].models, ["sled"]);
        doc.rename_stock("block", "blank").unwrap();
        assert_eq!(doc.job.setups[0].stocks, ["blank"]);
        doc.setups.insert("top".into(), SetupState::default());
        doc.selection = Selection::Operation("top".into(), 1);
        doc.rename_setup("top", "front").unwrap();
        assert!(doc.setups.contains_key("front"));
        assert_eq!(doc.active.as_deref(), Some("front"));
        assert_eq!(doc.selection, Selection::Operation("front".into(), 1));
        assert!(doc.rename_setup("front", " ").is_err());
        doc.job.check_common().unwrap();
        doc.job.check_setup(0).unwrap();
    }

    /// A stock's copy is the same block under a free name, after it, shown, and
    /// used by nothing, so it can be changed or removed on its own.
    #[test]
    fn a_stock_is_duplicated() {
        let mut doc = example();
        let copy = doc.duplicate_stock("block").unwrap();
        assert_eq!(copy, "block copy");
        assert_eq!(doc.job.stocks[1].name, "block copy");
        assert_eq!(doc.job.stocks[1].spec, doc.job.stocks[0].spec);
        assert_eq!(doc.selection, Selection::Stock(copy.clone()));
        assert_eq!(doc.duplicate_stock("block").unwrap(), "block copy 2");
        doc.remove_stock(&copy).unwrap();
        assert!(doc.duplicate_stock("nothing").is_none());
    }

    /// While a setup runs, its operations wait, run and are done in turn as
    /// word comes in; word from a run since superseded is dropped.
    #[test]
    fn operations_follow_their_run() {
        let mut doc = example();
        assert_eq!(doc.op_status("top", 0), OpStatus::NotRun);
        let seq = doc.started("top");
        assert_eq!(doc.op_status("top", 0), OpStatus::Waiting);
        doc.progressed("top", seq, Stage::Operation(0));
        assert!(matches!(doc.op_status("top", 0), OpStatus::Running(_)));
        assert_eq!(doc.op_status("top", 1), OpStatus::Waiting);
        doc.progressed("top", seq + 1, Stage::Operation(1));
        assert!(matches!(doc.op_status("top", 0), OpStatus::Running(_)));
        doc.progressed("top", seq, Stage::Operation(1));
        assert_eq!(doc.op_status("top", 0), OpStatus::Done);
        assert!(matches!(doc.op_status("top", 1), OpStatus::Running(_)));
        doc.progressed("top", seq, Stage::Checking);
        assert_eq!(doc.op_status("top", 1), OpStatus::Done);
        doc.finished("top", seq, Err(SessionError::Refused));
        assert_eq!(doc.op_status("top", 0), OpStatus::NotRun);
        assert!(doc.setups["top"].progress.is_none());
    }

    /// An operation dragged to a new place takes it, the others close up, and
    /// the selection stays on the operation it was on.
    #[test]
    fn operations_are_reordered() {
        let mut doc = example();
        let names = |d: &Document| -> Vec<String> {
            d.job.setups[0]
                .operations
                .iter()
                .map(|o| o.name.clone())
                .collect()
        };
        doc.duplicate_operation("top", 1);
        assert_eq!(names(&doc), ["rough", "finish", "finish copy"]);
        doc.selection = Selection::Operation("top".into(), 1);
        doc.reorder_operation("top", 2, 0);
        assert_eq!(names(&doc), ["finish copy", "rough", "finish"]);
        assert_eq!(doc.selection, Selection::Operation("top".into(), 2));
        doc.reorder_operation("top", 0, 2);
        assert_eq!(names(&doc), ["rough", "finish", "finish copy"]);
        assert_eq!(doc.selection, Selection::Operation("top".into(), 1));
        doc.reorder_operation("top", 1, 1);
        doc.reorder_operation("top", 0, 9);
        assert_eq!(names(&doc), ["rough", "finish", "finish copy"]);
    }

    /// Selecting a setup shows all its toolpaths; selecting an operation, its
    /// alone; and the choice is made once, not held against what Show says after.
    #[test]
    fn the_view_follows_the_selection() {
        let mut doc = example();
        let ops = |d: &Document| {
            let v = &d.setups["top"].view;
            (v.show_paths, v.view.ops)
        };
        doc.selection = Selection::Operation("top".into(), 1);
        doc.follow_selection();
        assert_eq!(ops(&doc), (true, 0b10));
        doc.setups.get_mut("top").unwrap().view.view.ops = 0b11;
        doc.follow_selection();
        assert_eq!(
            ops(&doc),
            (true, 0b11),
            "left alone until the selection changes"
        );
        doc.selection = Selection::Setup("top".into());
        doc.follow_selection();
        assert_eq!(ops(&doc), (true, u32::MAX));
        doc.selection = Selection::Operation("top".into(), 0);
        doc.follow_selection();
        assert_eq!(ops(&doc), (true, 0b01));
        doc.selection = Selection::Models;
        doc.follow_selection();
        assert_eq!(ops(&doc), (true, 0b01), "anything else leaves the view be");
    }

    /// An operation dragged into another setup leaves its own, takes its place
    /// in the other under a name free there, and is shown there.
    #[test]
    fn an_operation_moves_to_another_setup() {
        let mut doc = example();
        let other = doc.duplicate_setup("top").unwrap();
        let names = |d: &Document, s: usize| -> Vec<String> {
            d.job.setups[s]
                .operations
                .iter()
                .map(|o| o.name.clone())
                .collect()
        };
        doc.move_operation_to("top", 0, &other, 1);
        assert_eq!(names(&doc, 0), ["finish"]);
        assert_eq!(names(&doc, 1), ["rough", "rough 2", "finish"]);
        assert_eq!(doc.selection, Selection::Operation(other.clone(), 1));
        assert_eq!(doc.active.as_deref(), Some(other.as_str()));
        // Past the end is the end; within a setup it is a reorder.
        doc.move_operation_to("top", 0, &other, 99);
        assert_eq!(names(&doc, 1), ["rough", "rough 2", "finish", "finish 2"]);
        assert!(names(&doc, 0).is_empty());
        doc.move_operation_to(&other, 0, &other, 99);
        assert_eq!(names(&doc, 1), ["rough 2", "finish", "finish 2", "rough"]);
    }

    /// After a run, generating one operation keeps the others that have not
    /// changed and leaves out the ones that have; an operation turned off is
    /// shown so and cannot be generated.
    #[test]
    fn one_operation_is_generated_alone() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let text = std::fs::read_to_string(dir.join("rail-jig.camcraft")).unwrap();
        // The small fixture, so the test runs quickly.
        let text = text.replace("sled_base.step", "hinge_jig.step");
        let job = camcraft_session::parse_job(&text, "rail-jig").unwrap();
        let mut doc = Document::opened(1, Some(dir.join("rail-jig.camcraft")), job);
        let seq = doc.started("top");
        let ran = crate::test_jobs::run_setup_zero(&doc.job, &dir);
        doc.finished("top", seq, Ok(ran));
        let kinds = |plans: &[Plan]| -> Vec<&'static str> {
            plans
                .iter()
                .map(|p| match p {
                    Plan::Generate => "generate",
                    Plan::Keep(_) => "keep",
                    Plan::Skip => "skip",
                })
                .collect()
        };
        assert!(matches!(
            doc.op_status("top", 0),
            OpStatus::Done | OpStatus::Warnings
        ));
        assert_eq!(kinds(&doc.plans(0, Some(1))), ["keep", "generate"]);
        assert!(
            doc.plans(0, None).is_empty(),
            "the setup generates everything"
        );

        doc.job.setups[0].operations[0].passes.feed += 100.0;
        assert_eq!(doc.op_status("top", 0), OpStatus::Stale);
        assert!(matches!(
            doc.op_status("top", 1),
            OpStatus::Done | OpStatus::Warnings
        ));
        assert_eq!(kinds(&doc.plans(0, Some(1))), ["skip", "generate"]);
        assert_eq!(doc.status("top"), Status::Stale);

        doc.set_enabled("top", 0, false);
        assert_eq!(doc.op_status("top", 0), OpStatus::Disabled);
        assert!(doc.cannot_generate(0, Some(0)).is_some());
    }

    #[test]
    fn what_is_in_use_cannot_be_removed() {
        let mut doc = example();
        assert!(doc.remove_model("sled_base").is_err());
        assert!(doc.remove_stock("block").is_err());
        doc.remove_setup("top");
        doc.remove_stock("block").unwrap();
        doc.remove_model("sled_base").unwrap();
        assert!(doc.job.models.is_empty());
    }

    #[test]
    fn operations_are_added_moved_and_removed() {
        let mut doc = example();
        let i = doc
            .add_operation("top", StrategySpec::Parallel { angle: 90.0 })
            .unwrap();
        assert_eq!(i, 2);
        let op = &doc.job.setups[0].operations[2];
        assert_eq!(op.name, "finish 2");
        assert_eq!(
            op.tool.as_deref(),
            Some("finish-6-ball"),
            "the last tool, again"
        );
        doc.move_operation("top", 2, -1);
        assert_eq!(doc.job.setups[0].operations[1].name, "finish 2");
        assert_eq!(doc.selection, Selection::Operation("top".into(), 1));
        doc.move_operation("top", 0, -1);
        assert_eq!(
            doc.job.setups[0].operations[0].name, "rough",
            "nowhere to go"
        );
        doc.remove_operation("top", 1);
        assert_eq!(doc.job.setups[0].operations.len(), 2);
    }

    /// A library tool is copied in once: picking the same one again reuses it, and
    /// a different one under a taken name gets a free name.
    #[test]
    fn tools_are_adopted_as_copies() {
        let mut doc = example();
        let same = doc.job.tools["finish-6-ball"].clone();
        assert_eq!(doc.adopt_tool("anything", &same), "finish-6-ball");
        let other = ToolSpec {
            diameter: 3.0,
            ..same
        };
        assert_eq!(doc.adopt_tool("finish-6-ball", &other), "finish-6-ball-2");
        assert_eq!(doc.job.tools.len(), 3);
        // Used by nothing yet, so no clash.
        doc.job.tools.get_mut("finish-6-ball-2").unwrap().number = 1;
        assert_eq!(doc.number_clash("finish-6-ball-2"), None);
        doc.job.setups[0].operations[1].tool = Some("finish-6-ball-2".into());
        assert_eq!(
            doc.number_clash("finish-6-ball-2").as_deref(),
            Some("rough-6-flat")
        );
    }

    /// Choosing a tool with a preset fills in the operation from it; choosing one
    /// without keeps the operation's values and forgets the preset.
    #[test]
    fn a_tool_chosen_with_a_preset_fills_in_the_operation() {
        let mut doc = example();
        let mut spec = doc.job.tools["finish-6-ball"].clone();
        spec.diameter = 3.0;
        spec.presets = vec![Preset {
            name: "Finishing".into(),
            rpm: Some(20000),
            feed: Some(900.0),
            stepover: Some(0.2),
            ..Preset::default()
        }];
        let name = doc.adopt_tool("small-ball", &spec);
        let before = doc.job.setups[0].operations[1].clone();
        doc.use_tool("top", 1, name.clone(), Some("Finishing"));
        let op = &doc.job.setups[0].operations[1];
        assert_eq!(op.tool.as_deref(), Some(name.as_str()));
        assert_eq!(op.preset.as_deref(), Some("Finishing"));
        assert_eq!(op.passes.rpm, Some(20000));
        assert_eq!(op.passes.feed, 900.0);
        assert_eq!(op.passes.stepover, 0.2);
        assert_eq!(op.linking, before.linking, "the preset has no plunge feed");

        doc.use_tool("top", 1, "finish-6-ball".into(), None);
        let op = &doc.job.setups[0].operations[1];
        assert_eq!(op.preset, None);
        assert_eq!(op.passes.feed, 900.0, "values stay without a preset");
    }

    #[test]
    fn undo_and_redo_step_through_finished_edits() {
        let mut doc = example();
        doc.job.tolerance = 0.1;
        doc.checkpoint();
        doc.add_setup();
        doc.checkpoint();
        assert_eq!(doc.job.setups.len(), 2);
        doc.undo();
        assert_eq!(doc.job.setups.len(), 1);
        assert_eq!(doc.job.tolerance, 0.1);
        doc.undo();
        assert_eq!(doc.job.tolerance, 0.05);
        assert!(!doc.dirty());
        doc.redo();
        doc.redo();
        assert_eq!(doc.job.setups.len(), 2);
        assert!(!doc.can_redo());
    }

    /// A run's reply lands only if it is the latest asked for, and its result goes
    /// out of date when the setup changes.
    #[test]
    fn results_know_when_they_are_out_of_date() {
        let mut doc = example();
        let first = doc.started("top");
        let second = doc.started("top");
        assert_eq!(doc.status("top"), Status::Running);
        let failed = || Err(SessionError::Refused);
        doc.finished("top", first, failed());
        assert_eq!(
            doc.status("top"),
            Status::Running,
            "a superseded reply is dropped"
        );
        doc.finished("top", second, failed());
        assert_eq!(doc.status("top"), Status::NotRun);

        // A stand-in result, as a run would leave it.
        let inputs = SetupInputs::of(&doc.job, &doc.models, 0).unwrap();
        assert_eq!(
            Some(&inputs),
            SetupInputs::of(&doc.job, &doc.models, 0).as_ref()
        );
        doc.job.setups[0].operations[0].passes.stepover = 2.5;
        assert_ne!(
            Some(&inputs),
            SetupInputs::of(&doc.job, &doc.models, 0).as_ref()
        );
        doc.job.setups[0].operations[0].passes.stepover = 3.0;
        doc.job.tools.get_mut("rough-6-flat").unwrap().rpm = 12000;
        assert_ne!(
            Some(&inputs),
            SetupInputs::of(&doc.job, &doc.models, 0).as_ref()
        );
        doc.job.tools.get_mut("rough-6-flat").unwrap().rpm = 16000;
        // A tool no operation of this setup uses does not matter to it.
        doc.job.tools.insert("spare".into(), ToolSpec::default());
        assert_eq!(
            Some(&inputs),
            SetupInputs::of(&doc.job, &doc.models, 0).as_ref()
        );
    }

    #[test]
    fn free_names_count_up() {
        assert_eq!(free("Setup", ["Setup 1"].into_iter()), "Setup 2");
        assert_eq!(free("Stock", std::iter::empty()), "Stock 1");
        assert_eq!(free("rough", ["rough"].into_iter()), "rough 2");
        assert_eq!(free("part", std::iter::empty()), "part");
    }
}
