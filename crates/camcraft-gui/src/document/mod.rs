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
//!
//! The types a setup keeps that are not in the job are in [`setup_state`]; editing
//! the job is in [`edit`]; running it, and what came of a run, in [`run`].

mod edit;
mod run;
mod setup_state;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]
mod tests;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use camcraft_core::job::{Job, LengthUnit, ModelDef, Origin, Setup, StockDef, StockSpec};
use camcraft_geom::bvh::MeshBvh;
use camcraft_session::fs::{job_dir, relative_path, resolve, with_job_extension};
use camcraft_session::{Model, SessionError, write_job};

use crate::worker::DocId;

pub use run::{OpStatus, Status};
pub use setup_state::{Layout, RanSetup, SetupState, SetupView, StockMode, Transport};

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
            Selection::Operation(s, i) => self.operation(s, *i).is_some(),
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

/// Move item `i` by `by` places, within the list; where it ended up.
fn shift<T>(items: &mut [T], i: usize, by: isize) -> Option<usize> {
    let to = i.checked_add_signed(by)?;
    if to >= items.len() || i >= items.len() {
        return None;
    }
    items.swap(i, to);
    Some(to)
}
