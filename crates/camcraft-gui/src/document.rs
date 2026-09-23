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

use camcraft_core::job::{
    Job, LengthUnit, ModelDef, Operation, Setup, StockDef, StockSpec, StrategySpec, ToolSpec,
};
use camcraft_core::setup::{Meshes, Prepared, prepare};
use camcraft_session::fs::{relative_path, resolve};
use camcraft_session::{Model, Ran, SessionError, write_job};
use glam::DVec3;

use crate::camera::Camera;
use crate::scene::{Scene, box_edges};
use crate::stock_view::StockImage;
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
    /// As it stood when the scrubbed operation began, or after it once the scrub
    /// reaches its end.
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
    /// Loaded.
    Ready(ModelKey, Arc<Model>),
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
            ModelSlot::Loading(k) | ModelSlot::Ready(k, _) | ModelSlot::Failed { key: k, .. } => k,
        }
    }

    /// The model, if loaded.
    pub fn model(&self) -> Option<&Arc<Model>> {
        match self {
            ModelSlot::Ready(_, m) => Some(m),
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
    /// Segments shown, in machine order: the scrub.
    pub scrub: usize,
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
    /// The material at each stage, classified and coloured, built when first shown.
    pub stock_images: Vec<Option<StockImage>>,
}

impl Default for SetupView {
    fn default() -> Self {
        Self {
            camera: None,
            framing: next_key(),
            scrub: 0,
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
#[derive(Debug, Clone, Default)]
pub struct SetupState {
    /// Its last run.
    pub ran: Option<RanSetup>,
    /// The run in flight, if any, and the job as it was sent.
    pub running: Option<(u64, Job)>,
    /// How it is laid out now.
    pub layout: Option<Layout>,
    /// How it is being looked at.
    pub view: SetupView,
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
    /// Ran and did not verify: it would be refused.
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
        self.path.as_deref().map_or_else(
            || std::env::current_dir().unwrap_or_default(),
            crate::worker::job_dir,
        )
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
    pub fn loaded(&mut self, name: &str, key: &ModelKey, result: Result<Arc<Model>, SessionError>) {
        match self.models.get(name) {
            Some(ModelSlot::Loading(k)) if k == key => {}
            _ => return,
        }
        let slot = match result {
            Ok(model) => {
                for n in &model.notes {
                    self.log.push((Level::Info, format!("{name}: {n}")));
                }
                for w in &model.warnings {
                    self.log.push((Level::Warn, format!("{name}: {w}")));
                }
                ModelSlot::Ready(key.clone(), model)
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
        };
        let name = free(base, s.operations.iter().map(|o| o.name.as_str()));
        let tool = s.operations.last().and_then(|o| o.tool.clone());
        s.operations.push(Operation {
            name,
            strategy,
            tool,
            geometry: Default::default(),
            heights: Default::default(),
            passes: Default::default(),
            linking: Default::default(),
        });
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

    /// Move an operation up (`-1`) or down (`1`) within its setup.
    pub fn move_operation(&mut self, setup: &str, index: usize, by: isize) {
        if let Some(s) = self.job.setups.iter_mut().find(|s| s.name == setup)
            && let Some(to) = shift(&mut s.operations, index, by)
        {
            self.selection = Selection::Operation(setup.to_string(), to);
        }
    }

    /// Change an operation's strategy, keeping what the two share.
    pub fn set_strategy(op: &mut Operation, strategy: StrategySpec) {
        if std::mem::discriminant(&op.strategy) != std::mem::discriminant(&strategy) {
            op.strategy = strategy;
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
        let path = with_extension(path);
        let new_dir = crate::worker::job_dir(&path);
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
        if let Err(e) = self.job.check_common() {
            return Some(e.to_string());
        }
        if let Err(e) = self.job.check_setup(index) {
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

    /// Note that setup `name` has started running from the job as it is now, and
    /// say which run it is.
    pub fn started(&mut self, name: &str) -> u64 {
        self.seq += 1;
        let seq = self.seq;
        let job = self.job.clone();
        self.setups.entry(name.to_string()).or_default().running = Some((seq, job));
        seq
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
                lines.push(if ran.result.is_clean() {
                    (Level::Info, format!("'{name}' verified: it may be posted"))
                } else {
                    (
                        Level::Error,
                        format!("'{name}' NOT verified: it would be refused"),
                    )
                });
                let Some(setup) = job.setups.get(index) else {
                    return;
                };
                let scene = Scene::build(&job, setup, &ran.result);
                let view = &mut state.view;
                // A new run starts fully shown, every operation visible.
                view.scrub = scene.segments.len();
                view.view.ops = u32::MAX;
                view.stock_images = vec![None; ran.result.stages.len()];
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
        if now.as_ref() != Some(&ran.inputs) {
            Status::Stale
        } else if !ran.ran.result.is_clean() {
            Status::Refused
        } else if ran.ran.warnings.is_empty() {
            Status::Verified
        } else {
            Status::Warnings
        }
    }

    /// Lay out setup `name` for drawing, if anything it depends on has changed.
    pub fn refresh_layout(&mut self, name: &str) {
        let Some(index) = self.setup_index(name) else {
            return;
        };
        let Some(inputs) = SetupInputs::of(&self.job, &self.models, index).map(|i| i.layout())
        else {
            return;
        };
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
            return;
        }
        let meshes: Meshes<'_> = self
            .models
            .iter()
            .filter_map(|(n, s)| Some((n.as_str(), &s.model()?.mesh)))
            .collect();
        let prepared = prepare(&self.job, index, &meshes).map_err(|e| e.to_string());
        let outline = prepared.as_ref().map(outline).unwrap_or_default();
        let state = self.setups.entry(name.to_string()).or_default();
        state.layout = Some(Layout {
            inputs,
            prepared,
            outline,
            key: next_key(),
        });
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

/// `path` with the job extension, if it has none.
pub fn with_extension(path: &Path) -> PathBuf {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("camcraft"))
    {
        path.to_path_buf()
    } else {
        let mut p = path.as_os_str().to_owned();
        p.push(".camcraft");
        PathBuf::from(p)
    }
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

    fn example() -> Document {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/rail-jig.camcraft");
        let job = camcraft_session::read_job(&path).unwrap();
        Document::opened(1, Some(path), job)
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
