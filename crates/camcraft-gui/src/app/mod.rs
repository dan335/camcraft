//! The window: open jobs in tabs, each with its tree on the left, the properties of
//! what is selected on the right and the part in the middle; the log, the checks,
//! the tool library and the settings in windows of their own.
//!
//! A job is a document, as in any program: New, Open, Save, Save As and Close,
//! with a prompt before unsaved work is lost. Each tab keeps its own models,
//! results and view, and switching between them is immediate.
//!
//! What runs is exactly what `camcraft job` runs. Generate takes the setup as the
//! app holds it and goes through `camcraft-session`, so the app never holds a
//! second opinion about the engine.
//!
//! The menu bar, the job tabs and the start screen are in [`menu`], the questions
//! the app stops to ask in [`prompts`], and posting in [`post`].

mod menu;
mod post;
mod prompts;
#[cfg(all(test, feature = "ui-snapshots"))]
mod snapshot;

use std::path::{Path, PathBuf};

use camcraft_session::{Posting, SessionError};
use eframe::egui;

use crate::browser::{self, BrowserAction};
use crate::document::{Document, Level, ModelSlot, Selection, Status, Wanted, next_key};
use crate::file_dialog::{AppFileDialog, FileRequest};
use crate::picking::Picking;
use crate::playback;
use crate::properties::{self, HeightPlanes, PropAction, ViewPicks};
use crate::settings::Settings;
use crate::tools::manager::ToolManager;
use crate::tools::picker::{Outcome, Picked, ToolPicker};
use crate::transport;
use crate::ui::measure::Measure;
use crate::ui::theme;
use crate::ui::widgets;
use crate::view::viewport::{Content, Viewport};
use crate::worker::{DocId, Reply, Request, Task, Worker};

use prompts::Prompt;

const STORAGE_DIALOG: &str = "file_dialog";
const STORAGE_SETTINGS: &str = "settings";
const STORAGE_RECENT: &str = "recent_jobs";
/// Jobs remembered under Open Recent.
const RECENT: usize = 10;

/// A session error, worded for this app rather than for the command line.
pub fn explain(e: &SessionError) -> String {
    match e {
        SessionError::InModel { name, source }
            if matches!(**source, SessionError::StlNeedsUnits) =>
        {
            format!(
                "model '{name}' is an STL, which carries no units. Choose millimetres or \
                 inches for it; guessing would make an inch part 25.4 times too small."
            )
        }
        other => other.to_string(),
    }
}

/// The colour a log line of `level` is written in: body text, or the status
/// colour for a warning or an error.
fn log_color(level: Level) -> egui::Color32 {
    match level {
        Level::Info => theme::egui_color(theme::TEXT_2),
        Level::Warn => widgets::Level::Warn.color(),
        Level::Error => widgets::Level::Err.color(),
    }
}

/// The app's state.
pub struct App {
    docs: Vec<Document>,
    /// The tab showing.
    active: usize,
    /// Whose camera the viewport holds: a job and a setup.
    shown: Option<(DocId, String)>,
    worker: Worker,
    viewport: Viewport,
    /// Picking in the view: what is armed, under the pointer, and looked at.
    picking: Picking,
    /// The tab of the selected operation the properties panel shows, as of the
    /// panel's last drawing.
    operation_tab: Option<properties::OperationTab>,
    /// The selected operation's heights, drawn while its Heights tab shows.
    planes: HeightPlanes,
    dialog: AppFileDialog,
    /// The tool library, in a window inside the app when open.
    tools: ToolManager,
    picker: Option<ToolPicker>,
    settings: Settings,
    settings_open: bool,
    /// Whether the log window is open.
    log_open: bool,
    /// Whether the checks window is open.
    checks_open: bool,
    recent: Vec<PathBuf>,
    prompt: Option<Prompt>,
    /// Close this job once it has been saved.
    close_after_save: Option<DocId>,
    /// What happened outside any job: a file that would not open.
    log: Vec<(Level, String)>,
    /// Programs rendered and waiting on [`Prompt::Post`], for job `doc`.
    post_plan: Option<(DocId, Posting)>,
    /// Whether posting setups that did not pass their checks has been confirmed.
    post_anyway: bool,
}

impl App {
    /// The app, restoring what it remembers.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let dialog = cc
            .storage
            .and_then(|s| eframe::get_value(s, STORAGE_DIALOG));
        let settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, STORAGE_SETTINGS))
            .unwrap_or_default();
        let recent = cc
            .storage
            .and_then(|s| eframe::get_value(s, STORAGE_RECENT))
            .unwrap_or_default();
        Self {
            docs: Vec::new(),
            active: 0,
            shown: None,
            worker: Worker::default(),
            viewport: Viewport::default(),
            picking: Picking::default(),
            operation_tab: None,
            planes: HeightPlanes::default(),
            dialog: AppFileDialog::new(dialog),
            tools: ToolManager::new(),
            picker: None,
            settings,
            settings_open: false,
            log_open: false,
            checks_open: false,
            recent,
            prompt: None,
            close_after_save: None,
            log: Vec::new(),
            post_plan: None,
            post_anyway: false,
        }
    }

    /// The app, with jobs already open, as from the command line.
    pub fn with_jobs(cc: &eframe::CreationContext<'_>, jobs: Vec<PathBuf>) -> Self {
        let mut app = Self::new(cc);
        for job in jobs {
            app.open_job(&cc.egui_ctx, job);
        }
        app
    }

    fn measure(&self) -> Measure {
        self.settings.measure
    }

    fn doc(&self) -> Option<&Document> {
        self.docs.get(self.active)
    }

    fn doc_mut(&mut self) -> Option<&mut Document> {
        self.docs.get_mut(self.active)
    }

    fn find(&mut self, id: DocId) -> Option<&mut Document> {
        self.docs.iter_mut().find(|d| d.id == id)
    }

    // ---- files ----------------------------------------------------------------

    /// A new, empty job in its own tab.
    pub fn new_job(&mut self) {
        self.docs.push(Document::untitled(next_key()));
        self.active = self.docs.len() - 1;
    }

    /// Open a job file in its own tab, or show its tab if it is open already.
    pub fn open_job(&mut self, ctx: &egui::Context, path: PathBuf) {
        let wanted = path.canonicalize().ok();
        let same = |p: &Path| p == path || (wanted.is_some() && p.canonicalize().ok() == wanted);
        if let Some(i) = self
            .docs
            .iter()
            .position(|d| d.path.as_deref().is_some_and(same))
        {
            self.active = i;
            return;
        }
        self.worker
            .start(ctx, next_key(), Task::Open, Request::Open { path });
    }

    fn remember(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(RECENT);
    }

    /// Save job `id` to `path`, or where it was when `path` is `None`, asking where
    /// if it has never been saved.
    fn save_job(&mut self, id: DocId, path: Option<PathBuf>) {
        let Some(doc) = self.find(id) else {
            return;
        };
        let Some(path) = path.or_else(|| doc.path.clone()) else {
            self.save_as(id);
            return;
        };
        match doc.save_to(&path) {
            Ok(()) => {
                let saved = doc.path.clone().unwrap_or(path);
                doc.note(Level::Info, format!("saved {}", saved.display()));
                self.remember(&saved);
                if self.close_after_save == Some(id) {
                    self.close_after_save = None;
                    self.remove(id);
                }
            }
            Err(e) => doc.note(Level::Error, explain(&e)),
        }
    }

    fn save_as(&mut self, id: DocId) {
        let name = self
            .find(id)
            .map(|d| format!("{}.camcraft", d.title()))
            .unwrap_or_default();
        self.dialog.open(FileRequest::SaveJob(id), &name);
    }

    /// Close job `id`, asking first if it has unsaved changes.
    fn close(&mut self, id: DocId) {
        if self.find(id).is_some_and(|d| d.dirty()) {
            self.prompt = Some(Prompt::Close(id));
        } else {
            self.remove(id);
        }
    }

    fn remove(&mut self, id: DocId) {
        if let Some(i) = self.docs.iter().position(|d| d.id == id) {
            self.docs.remove(i);
            if self.active >= i && self.active > 0 {
                self.active -= 1;
            }
        }
        if self.shown.as_ref().is_some_and(|(d, _)| *d == id) {
            self.shown = None;
        }
    }

    // ---- work -----------------------------------------------------------------

    /// Generate setup `setup` of the showing job: all its operations, or its
    /// operation `only` alone, keeping the others as they were made.
    fn generate(&mut self, ctx: &egui::Context, setup: &str, only: Option<usize>) {
        let Some(doc) = self.docs.get_mut(self.active) else {
            return;
        };
        let Some(index) = doc.setup_index(setup) else {
            return;
        };
        if let Some(why) = doc.cannot_generate(setup, only) {
            doc.note(Level::Warn, format!("'{setup}' cannot run: {why}"));
            return;
        }
        let Some(models) = doc.models_for_run(index) else {
            return;
        };
        let plans = doc.plans(index, only);
        let what = only.and_then(|k| doc.operation(setup, k)).map_or_else(
            || format!("'{setup}'"),
            |op| format!("'{setup}/{}'", op.name),
        );
        let seq = doc.started(setup);
        doc.note(Level::Info, format!("generating {what}…"));
        let request = Request::Run {
            job: Box::new(doc.job.clone()),
            setup: index,
            models,
            plans,
        };
        let task = Task::Run {
            setup: setup.to_string(),
            seq,
        };
        self.worker.start(ctx, doc.id, task, request);
    }

    /// Run every setup of the showing job.
    fn generate_all(&mut self, ctx: &egui::Context) {
        let names: Vec<String> = self
            .doc()
            .map(|d| d.job.setups.iter().map(|s| s.name.clone()).collect())
            .unwrap_or_default();
        for name in names {
            self.generate(ctx, &name, None);
        }
    }

    fn accept(&mut self, ctx: &egui::Context, id: DocId, task: Task, reply: Reply) {
        match (task, reply) {
            (Task::Open, Reply::Opened(opened)) => match *opened {
                Ok((path, job)) => {
                    let mut doc = Document::opened(id, Some(path.clone()), job);
                    let text = format!(
                        "opened {}: {} model(s), {} setup(s)",
                        path.display(),
                        doc.job.models.len(),
                        doc.job.setups.len()
                    );
                    doc.note(Level::Info, text);
                    if let Err(e) = doc.job.check_common() {
                        doc.note(Level::Warn, e.to_string());
                    }
                    self.remember(&path);
                    self.docs.push(doc);
                    self.active = self.docs.len() - 1;
                }
                Err(e) => self.log.push((Level::Error, explain(&e))),
            },
            (Task::Load { name, key }, Reply::Loaded(result)) => {
                if let Some(doc) = self.find(id) {
                    doc.loaded(&name, &key, result);
                }
            }
            (Task::Run { seq, .. }, Reply::Progress(stage)) => {
                if let Some(doc) = self.find(id) {
                    doc.progressed(seq, stage);
                }
            }
            (Task::Run { setup, seq }, Reply::Ran(ran)) => {
                if let Some(doc) = self.find(id) {
                    doc.finished(&setup, seq, *ran);
                }
            }
            (Task::Post, Reply::Planned(planned)) => self.planned(ctx, id, *planned),
            (Task::Write { dir, refused }, Reply::Written(written)) => {
                self.written(id, (&dir, &refused), written);
            }
            (task, Reply::Died(what)) => {
                let text = format!(
                    "the engine stopped while {what} and gave no answer. This is a bug; the \
                     command line may say more."
                );
                match self.find(id) {
                    Some(doc) => {
                        if let Task::Run { setup, seq } = &task {
                            doc.finished(setup, *seq, Err(SessionError::Refused));
                        }
                        doc.note(Level::Error, text);
                    }
                    None => self.log.push((Level::Error, text)),
                }
            }
            _ => {}
        }
    }

    /// Start loading every model a job needs and does not have.
    fn load_models(&mut self, ctx: &egui::Context) {
        for doc in &mut self.docs {
            let dir = doc.dir();
            for (def, key, how) in doc.wanted_loads() {
                let request = match how {
                    Wanted::Read => {
                        doc.loading(&def.name, key.clone());
                        Request::Load {
                            dir: dir.clone(),
                            model: def.clone(),
                            tolerance: doc.job.tolerance,
                        }
                    }
                    Wanted::Place(model) => {
                        doc.placing(&def.name, key.clone());
                        Request::Place {
                            model,
                            transform: def.transform,
                        }
                    }
                };
                let task = Task::Load {
                    name: def.name,
                    key,
                };
                self.worker.start(ctx, doc.id, task, request);
            }
            // Once loading settles, say once which models were not found.
            let settled = !doc
                .models
                .values()
                .any(|s| matches!(s, ModelSlot::Loading(_)));
            if settled && !doc.asked_missing && !doc.missing_models().is_empty() {
                doc.asked_missing = true;
                if self.prompt.is_none() {
                    self.prompt = Some(Prompt::Missing(doc.id));
                }
            }
        }
    }

    // ---- the frame ------------------------------------------------------------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let typing = ctx.memory(|m| m.focused().is_some());
        let pressed =
            |mods, key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(mods, key)));
        let shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let id = self.doc().map(|d| d.id);
        if pressed(shift, Key::S) {
            if let Some(id) = id {
                self.save_as(id);
            }
        } else if pressed(Modifiers::COMMAND, Key::S)
            && let Some(id) = id
        {
            self.save_job(id, None);
        }
        if pressed(Modifiers::COMMAND, Key::N) {
            self.new_job();
        }
        if pressed(Modifiers::COMMAND, Key::O) {
            self.dialog.open(FileRequest::OpenJobs, "");
        }
        if pressed(Modifiers::COMMAND, Key::W)
            && let Some(id) = id
        {
            self.close(id);
        }
        // Esc stops picking, unless a window over everything wants it.
        let modal = self.prompt.is_some() || self.picker.is_some();
        if !typing && !modal && ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.picking.escape();
        }
        if !typing {
            if pressed(shift, Key::Z) {
                if let Some(d) = self.doc_mut() {
                    d.redo();
                }
            } else if pressed(Modifiers::COMMAND, Key::Z)
                && let Some(d) = self.doc_mut()
            {
                d.undo();
            }
        }
        let n = self.docs.len();
        if n > 1 {
            if pressed(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab) {
                self.active = (self.active + n - 1) % n;
            } else if pressed(Modifiers::CTRL, Key::Tab) {
                self.active = (self.active + 1) % n;
            }
        }
    }

    fn left(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let m = self.measure();
        let Some(doc) = self.docs.get_mut(self.active) else {
            return;
        };
        let id = doc.id;
        let mut actions = Vec::new();
        egui::Panel::top("browser")
            .resizable(true)
            .default_size(300.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| actions = browser::show(ui, doc));
            });
        let mut focus = None;
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let Some(name) = doc.active.clone() else {
                    return;
                };
                let status = doc.status(&name);
                let state = doc.setups.entry(name.clone()).or_default();
                if state.ran.is_some() {
                    if status == Status::Stale {
                        widgets::diagnostic(
                            ui,
                            widgets::Level::Warn,
                            "Out of date: the setup has changed since this ran. Generate it again.",
                            |_| {},
                        );
                    }
                    playback::result(ui, state, m);
                    ui.separator();
                }
                focus = playback::controls(ui, state, m).or(focus);
            });
        });
        if let Some(at) = focus {
            self.viewport.focus(at, 12.0);
        }
        for a in actions {
            match a {
                BrowserAction::AddModels => self.dialog.open(FileRequest::AddModels(id, None), ""),
                BrowserAction::Locate(name) => self.dialog.open(FileRequest::Locate(id, name), ""),
                BrowserAction::Generate(name) => self.generate(&ctx, &name, None),
                BrowserAction::GenerateOperation(name, i) => {
                    self.generate(&ctx, &name, Some(i));
                }
            }
        }
    }

    fn right(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let m = self.measure();
        let Some(doc) = self.docs.get_mut(self.active) else {
            return;
        };
        let id = doc.id;
        let picks = ViewPicks {
            armed: self.picking.armed(id),
            shown: self.picking.shown(id),
        };
        // An operation's panel ends with a way to generate it, kept in sight
        // below its fields however far they scroll.
        let mut generate = None;
        if let Selection::Operation(setup, index) = &doc.selection {
            egui::Panel::bottom("operation-generate")
                .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 8)))
                .show(ui, |ui| {
                    let why = doc.cannot_generate(setup, Some(*index));
                    let running = doc.status(setup) == Status::Running;
                    ui.horizontal(|ui| {
                        let label = if running { "Generating…" } else { "Generate" };
                        let clicked = ui
                            .add_enabled_ui(why.is_none() && !running, |ui| {
                                widgets::button(
                                    ui,
                                    widgets::Variant::Primary,
                                    widgets::Size::Md,
                                    Some(widgets::Icon::Play),
                                    label,
                                )
                            })
                            .inner
                            .on_hover_text(
                                "Generate this operation alone: the others keep what they \
                                 have, and are cut again into the material, so it cuts what \
                                 they leave",
                            )
                            .on_disabled_hover_text(why.unwrap_or_default())
                            .clicked();
                        if running {
                            ui.spinner();
                        }
                        if clicked {
                            generate = Some((setup.clone(), *index));
                        }
                    });
                });
        }
        let panel = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| properties::show(ui, doc, m, picks))
            .inner;
        self.operation_tab = panel.operation_tab;
        if let Some((setup, index)) = generate {
            self.generate(&ctx, &setup, Some(index));
        }
        for a in panel.actions {
            match a {
                PropAction::Arm(slot) => self.picking.arm(id, slot),
                PropAction::Disarm => self.picking.disarm(),
                PropAction::HoverRow(kind, i) => {
                    if self.picking.hover_row(kind, i) {
                        ctx.request_repaint();
                    }
                }
                PropAction::AddModels(setup) => {
                    self.dialog.open(FileRequest::AddModels(id, setup), "")
                }
                PropAction::Locate(name) => self.dialog.open(FileRequest::Locate(id, name), ""),
                PropAction::ProfileFile(setup) => {
                    self.dialog.open(FileRequest::Profile(id, setup), "")
                }
                PropAction::PickTool(setup, op) => {
                    if let Some(doc) = self.find(id) {
                        let picker = ToolPicker::new(id, setup, op, &doc.job);
                        self.picker = Some(picker);
                    }
                }
            }
        }
    }

    /// What the checks found for the setup showing, in its own window, opened
    /// from the View menu. A finding at a move, clicked, brings the view to it.
    fn checks_window(&mut self, ctx: &egui::Context) {
        let mut open = self.checks_open;
        let mut focus = None;
        egui::Window::new("Checks")
            .open(&mut open)
            .default_size([420.0, 360.0])
            .resizable(true)
            .show(ctx, |ui| {
                let Some(doc) = self.docs.get_mut(self.active) else {
                    ui.label(widgets::hint_text("No job is open."));
                    return;
                };
                let Some(name) = doc.active.clone() else {
                    ui.label(widgets::hint_text("No setup is showing."));
                    return;
                };
                let state = doc.setups.entry(name.clone()).or_default();
                if state.ran.is_none() {
                    ui.label(widgets::hint_text(format!(
                        "'{name}' has not been generated yet."
                    )));
                    return;
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    focus = playback::checks(ui, state);
                });
            });
        self.checks_open = open;
        if let Some(at) = focus {
            self.viewport.focus(at, 12.0);
        }
    }

    /// What happened, in its own window, opened from the View menu: the tree,
    /// the panels and the view already say what matters as it happens.
    fn log_window(&mut self, ctx: &egui::Context) {
        let mut open = self.log_open;
        egui::Window::new("Log")
            .open(&mut open)
            .default_size([640.0, 280.0])
            .resizable(true)
            .show(ctx, |ui| self.log_panel(ui));
        self.log_open = open;
    }

    fn log_panel(&self, ui: &mut egui::Ui) {
        let lines = self.doc().map_or(&self.log, |d| &d.log);
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (level, text) in lines {
                    ui.label(
                        egui::RichText::new(text)
                            .monospace()
                            .color(log_color(*level)),
                    );
                }
            });
    }

    /// Hand the viewport the camera of the setup now showing, keeping the last one's.
    fn swap_camera(&mut self) {
        let now = self.doc().and_then(|d| Some((d.id, d.active.clone()?)));
        if now == self.shown {
            return;
        }
        let camera = self.viewport.camera();
        if let Some((id, name)) = self.shown.take()
            && let Some(state) = self
                .docs
                .iter_mut()
                .find(|d| d.id == id)
                .and_then(|d| d.setups.get_mut(&name))
        {
            state.view.camera = Some(camera);
        }
        if let Some((id, name)) = &now
            && let Some(doc) = self.docs.iter_mut().find(|d| d.id == *id)
        {
            let state = doc.setups.entry(name.clone()).or_default();
            self.viewport
                .use_camera(state.view.camera, state.view.framing);
        }
        self.shown = now;
    }

    /// Do what a file dialog that has just finished was for.
    fn file_dialog_results(&mut self, ctx: &egui::Context) {
        let Some((request, paths)) = self.dialog.update(ctx) else {
            return;
        };
        match request {
            FileRequest::OpenJobs => {
                for p in paths {
                    self.open_job(ctx, p);
                }
            }
            FileRequest::SaveJob(id) => {
                if let Some(path) = paths.into_iter().next() {
                    self.save_job(id, Some(path));
                }
            }
            FileRequest::AddModels(id, setup) => {
                if let Some(doc) = self.find(id) {
                    let names = doc.add_models(&paths);
                    if let Some(setup) = setup {
                        doc.include_models(&setup, &names);
                    }
                    doc.note(Level::Info, format!("added {}", names.join(", ")));
                }
            }
            FileRequest::Locate(id, name) => {
                if let (Some(doc), Some(path)) = (self.find(id), paths.first()) {
                    doc.locate_model(&name, path);
                }
            }
            FileRequest::PostTo(id, scope) => {
                if let Some(dir) = paths.into_iter().next() {
                    self.prepare_post(ctx, id, scope, &dir);
                }
            }
            FileRequest::ImportTools => {
                for path in &paths {
                    self.tools.import_fusion(path);
                }
            }
            FileRequest::Profile(id, setup) => {
                if let (Some(doc), Some(path)) = (self.find(id), paths.first()) {
                    let post = match &doc.path {
                        Some(_) => camcraft_session::fs::relative_path(&doc.dir(), path),
                        None => path.display().to_string(),
                    };
                    if let Some(s) = doc.setup_mut(&setup) {
                        s.post = post;
                    }
                }
            }
        }
    }

    fn tool_picker(&mut self, ctx: &egui::Context) {
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        let (id, setup, op) = picker.target.clone();
        let m = self.settings.measure;
        let library = self.tools.library();
        let Some(doc) = self.docs.iter_mut().find(|d| d.id == id) else {
            self.picker = None;
            return;
        };
        let (picked, preset) = match picker.show(ctx, &doc.job, library, m) {
            Outcome::Open => return,
            Outcome::Cancelled => {
                self.picker = None;
                return;
            }
            Outcome::Chosen { tool, preset } => {
                self.picker = None;
                (tool, preset)
            }
        };
        let name = match picked {
            Picked::Job(name) => name,
            Picked::Library(hint, spec) => doc.adopt_tool(&hint, &spec),
        };
        doc.use_tool(&setup, op, name, preset.as_deref());
    }

    /// The showing setup's state, if there is one.
    fn active_state_mut(&mut self) -> Option<&mut crate::document::SetupState> {
        let d = self.docs.get_mut(self.active)?;
        let name = d.active.clone()?;
        d.setups.get_mut(&name)
    }

    /// What the viewport should draw this frame.
    fn content(&self) -> Content<'_> {
        let shown = self.doc().and_then(|d| {
            let name = d.active.as_ref()?;
            Some((d.setups.get(name)?, name, &d.selection))
        });
        let mut content = match shown {
            Some((state, name, selection)) => playback::content(state, name, selection),
            None => Content::empty(),
        };
        if let Some(doc) = self.doc() {
            let geometry = self.operation_tab == Some(properties::OperationTab::Geometry);
            self.picking.decorate(doc.id, geometry, &mut content);
        }
        content.planes = self.planes.planes();
        content
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        for (id, task, reply) in self.worker.poll() {
            self.accept(&ctx, id, task, reply);
        }
        // Not while a field is being typed in or dragged: dragging the tolerance
        // would otherwise tessellate every model at every step of the drag.
        let editing = ctx.memory(|m| m.focused().is_some()) || ctx.dragged_id().is_some();
        if !editing {
            self.load_models(&ctx);
        }
        self.shortcuts(&ctx);
        if self.active >= self.docs.len() {
            self.active = self.docs.len().saturating_sub(1);
        }
        if let Some(doc) = self.docs.get_mut(self.active) {
            doc.tidy_selection();
            doc.invalidate_changed_operations();
            if let Some(name) = doc.active.clone() {
                // The work zero moved: keep the view on the part, which moved with
                // it. The camera is the viewport's if this setup is showing, else
                // the one kept for it.
                if let Some(by) = doc.refresh_layout(&name) {
                    if self.shown.as_ref() == Some(&(doc.id, name.clone())) {
                        self.viewport.shift(by);
                    } else if let Some(c) = doc
                        .setups
                        .get_mut(&name)
                        .and_then(|s| s.view.camera.as_mut())
                    {
                        c.shift(by.as_vec3());
                    }
                }
                let state = doc.setups.entry(name).or_default();
                playback::settle_mode(state);
                transport::keys(&ctx, state);
                transport::tick(&ctx, state);
                playback::follow_stock(&ctx, state);
            }
        }
        self.swap_camera();
        let m = self.measure();
        self.picking.refresh(self.docs.get(self.active), m);
        self.tools.measure = m;
        self.tools.show(&ctx);
        if std::mem::take(&mut self.tools.import_requested) {
            self.dialog.open(FileRequest::ImportTools, "");
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        egui::Panel::top("tabs").show(ui, |ui| self.tabs(ui));
        self.settings.show(&ctx, &mut self.settings_open);
        self.log_window(&ctx);
        self.checks_window(&ctx);
        self.file_dialog_results(&ctx);
        // A tab set to close once saved closes on that save or not at all: the
        // save it waits for was cancelled, or failed, once nothing is asking
        // where to save it.
        if let Some(id) = self.close_after_save
            && !self.dialog.saving(id)
        {
            self.close_after_save = None;
        }
        self.tool_picker(&ctx);
        self.prompts(&ctx);

        if self.docs.is_empty() {
            egui::CentralPanel::default().show(ui, |ui| self.start_screen(ui));
        } else {
            egui::Panel::left("side")
                .resizable(true)
                .default_size(320.0)
                .show(ui, |ui| self.left(ui));
            egui::Panel::right("properties")
                .resizable(true)
                .default_size(380.0)
                .show(ui, |ui| self.right(ui));
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    if let Some(state) = self.active_state_mut() {
                        egui::Panel::bottom("transport")
                            .frame(
                                egui::Frame::new()
                                    .fill(theme::egui_color(theme::SURFACE_1))
                                    .inner_margin(egui::Margin::symmetric(8, 6)),
                            )
                            .show(ui, |ui| transport::bar(ui, state, m));
                    }
                    #[cfg(all(test, feature = "ui-snapshots"))]
                    let rect = ui.available_rect_before_wrap();
                    // After the panel, so the heights drawn are the ones just edited.
                    let tab = self.operation_tab;
                    match self.docs.get(self.active) {
                        Some(d) => {
                            self.planes.update(d, m, tab);
                        }
                        None => self.planes = HeightPlanes::default(),
                    }
                    // The viewport is taken out for the call, because the content it
                    // draws borrows the rest of the app.
                    let mut viewport = std::mem::take(&mut self.viewport);
                    let content = self.content();
                    let drew_live = content.stock.is_some();
                    let event = viewport.show(ui, frame.wgpu_render_state(), &content);
                    drop(content);
                    self.viewport = viewport;
                    // A frame with a GPU that drew the material uploads whatever of
                    // it changed; one that did not leaves it for one that does.
                    if drew_live
                        && frame.wgpu_render_state().is_some()
                        && let Some(live) = self.active_state_mut().and_then(|s| s.live.as_mut())
                    {
                        live.uploaded();
                        // What one frame did not take, the next does.
                        if live.uploading() {
                            ctx.request_repaint();
                        }
                    }
                    if let Some(doc) = self.docs.get_mut(self.active)
                        && self.picking.pointer(doc, event)
                    {
                        ctx.request_repaint();
                    }
                    #[cfg(all(test, feature = "ui-snapshots"))]
                    self.picking.view_for_snapshot(rect);
                });
        }

        // An edit is finished once no field is being typed in or dragged: then it
        // becomes one step to undo.
        if ctx.memory(|m| m.focused().is_none()) && ctx.dragged_id().is_none() {
            for d in &mut self.docs {
                d.checkpoint();
            }
        }

        if ctx.input(|i| i.viewport().close_requested()) && self.docs.iter().any(Document::dirty) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.prompt = Some(Prompt::Quit);
        }

        // Keep the spinners turning while work runs; its reply wakes us anyway.
        if !self.worker.idle() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // eframe saves on the way out, so a library edit still in a field is kept.
        self.tools.save_library();
        eframe::set_value(storage, STORAGE_DIALOG, self.dialog.storage());
        eframe::set_value(storage, STORAGE_SETTINGS, &self.settings);
        eframe::set_value(storage, STORAGE_RECENT, &self.recent);
    }
}
