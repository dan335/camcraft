//! The window: open jobs in tabs, each with its tree on the left, the properties of
//! what is selected on the right, the part in the middle and a log beneath.
//!
//! A job is a document, as in any program: New, Open, Save, Save As and Close,
//! with a prompt before unsaved work is lost. Each tab keeps its own models,
//! results and view, and switching between them is immediate.
//!
//! What runs is exactly what `camcraft job` runs. Generate takes the setup as the
//! app holds it and goes through `camcraft-session`, so the app never holds a
//! second opinion about the engine.

use std::path::{Path, PathBuf};

use camcraft_session::SessionError;
use eframe::egui;

use crate::browser::{self, BrowserAction};
use crate::document::{Document, Level, ModelSlot, Status, next_key};
use crate::file_dialog::{AppFileDialog, Pick};
use crate::measure::Measure;
use crate::picking::Picking;
use crate::playback;
use crate::posting::{self, Plan};
use crate::properties::{self, Picks, PropAction};
use crate::settings::Settings;
use crate::theme;
use crate::tool_manager::ToolManager;
use crate::tool_picker::{Picked, ToolPicker};
use crate::transport;
use crate::viewport::{Content, Viewport};
use crate::worker::{DocId, Reply, Request, Task, Worker};

const STORAGE_DIALOG: &str = "file_dialog";
const STORAGE_SETTINGS: &str = "settings";
const STORAGE_RECENT: &str = "recent_jobs";
/// Jobs remembered under Open Recent.
const RECENT: usize = 10;

/// What answering a prompt does, once the prompt is drawn.
type Answer = Box<dyn FnOnce(&mut App)>;

/// A question the app is waiting on.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Prompt {
    /// Close job `doc`, which has unsaved changes?
    Close(DocId),
    /// Quit, with unsaved changes in some jobs?
    Quit,
    /// Job `doc` names models that are not where it says.
    Missing(DocId),
    /// Post the planned programs? Something stands in the way: see `post_plan`.
    Post,
}

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

/// Why `names` of `doc` cannot be posted at all, if they cannot: each must have a
/// result, finished and up to date. A result that did not pass can be posted, but
/// only once that is confirmed.
fn post_blocker(doc: &Document, names: &[String]) -> Option<String> {
    if names.is_empty() {
        return Some("the job has no setups".into());
    }
    names.iter().find_map(|n| match doc.status(n) {
        Status::NotRun => Some(format!("'{n}' has not been generated")),
        Status::Running => Some(format!("'{n}' is still generating")),
        Status::Stale => Some(format!("'{n}' is out of date: generate it again")),
        Status::Verified | Status::Warnings | Status::Refused => None,
    })
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
    dialog: AppFileDialog,
    /// The tool library, in a window inside the app when open.
    tools: ToolManager,
    picker: Option<ToolPicker>,
    settings: Settings,
    settings_open: bool,
    recent: Vec<PathBuf>,
    prompt: Option<Prompt>,
    /// Close this job once it has been saved.
    close_after_save: Option<DocId>,
    /// What happened outside any job: a file that would not open.
    log: Vec<(Level, String)>,
    /// Programs rendered and waiting on [`Prompt::Post`], for job `doc`.
    post_plan: Option<(DocId, Plan)>,
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
            dialog: AppFileDialog::new(dialog),
            tools: ToolManager::new(),
            picker: None,
            settings,
            settings_open: false,
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
        self.dialog.open(Pick::SaveJob(id), &name);
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

    /// Run setup `setup` of the showing job.
    fn generate(&mut self, ctx: &egui::Context, setup: &str) {
        let Some(doc) = self.docs.get_mut(self.active) else {
            return;
        };
        let Some(index) = doc.setup_index(setup) else {
            return;
        };
        if let Some(why) = doc.cannot_run(index) {
            doc.note(Level::Warn, format!("'{setup}' cannot run: {why}"));
            return;
        }
        let Some(models) = doc.models_for_run(index) else {
            return;
        };
        let seq = doc.started(setup);
        doc.note(Level::Info, format!("generating '{setup}'…"));
        let request = Request::Run {
            job: Box::new(doc.job.clone()),
            setup: index,
            models,
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
            self.generate(ctx, &name);
        }
    }

    /// Run the showing setup, as Generate does. Only the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn start_run(&mut self, ctx: &egui::Context) {
        if let Some(name) = self.doc().and_then(|d| d.active.clone()) {
            self.generate(ctx, &name);
        }
    }

    /// Whether anything is running. Only the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn busy(&self) -> Option<&'static str> {
        (!self.worker.idle()).then_some("working")
    }

    fn accept(&mut self, id: DocId, task: Task, reply: Reply) {
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
            (Task::Run { setup, seq }, Reply::Ran(ran)) => {
                if let Some(doc) = self.find(id) {
                    doc.finished(&setup, seq, *ran);
                }
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
            for (def, key) in doc.wanted_loads() {
                doc.loading(&def.name, key.clone());
                let task = Task::Load {
                    name: def.name.clone(),
                    key,
                };
                let request = Request::Load {
                    dir: dir.clone(),
                    model: def,
                    tolerance: doc.job.tolerance,
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
            self.dialog.open(Pick::OpenJobs, "");
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

    fn menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let id = self.doc().map(|d| d.id);
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New").clicked() {
                    self.new_job();
                    ui.close();
                }
                if ui.button("Open…").clicked() {
                    self.dialog.open(Pick::OpenJobs, "");
                    ui.close();
                }
                ui.add_enabled_ui(!self.recent.is_empty(), |ui| {
                    ui.menu_button("Open Recent", |ui| {
                        for path in self.recent.clone() {
                            if ui.button(path.display().to_string()).clicked() {
                                self.open_job(&ctx, path);
                                ui.close();
                            }
                        }
                    });
                });
                ui.separator();
                ui.add_enabled_ui(id.is_some(), |ui| {
                    if ui.button("Save").clicked()
                        && let Some(id) = id
                    {
                        self.save_job(id, None);
                        ui.close();
                    }
                    if ui.button("Save As…").clicked()
                        && let Some(id) = id
                    {
                        self.save_as(id);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Close").clicked()
                        && let Some(id) = id
                    {
                        self.close(id);
                        ui.close();
                    }
                });
            });
            ui.menu_button("Edit", |ui| {
                let (undo, redo) = self
                    .doc()
                    .map_or((false, false), |d| (d.can_undo(), d.can_redo()));
                if ui.add_enabled(undo, egui::Button::new("Undo")).clicked() {
                    if let Some(d) = self.doc_mut() {
                        d.undo();
                    }
                    ui.close();
                }
                if ui.add_enabled(redo, egui::Button::new("Redo")).clicked() {
                    if let Some(d) = self.doc_mut() {
                        d.redo();
                    }
                    ui.close();
                }
            });
            ui.menu_button("Setup", |ui| {
                let active = self.doc().and_then(|d| d.active.clone());
                if ui
                    .add_enabled(active.is_some(), egui::Button::new("Generate"))
                    .clicked()
                    && let Some(name) = active
                {
                    self.generate(&ctx, &name);
                    ui.close();
                }
                if ui
                    .add_enabled(id.is_some(), egui::Button::new("Generate all"))
                    .clicked()
                {
                    self.generate_all(&ctx);
                    ui.close();
                }
            });
            ui.menu_button("View", |ui| {
                if ui.checkbox(&mut self.tools.open, "Tool library").clicked() {
                    ui.close();
                }
                if ui.checkbox(&mut self.settings_open, "Settings").clicked() {
                    ui.close();
                }
            });
        });
    }

    fn tabs(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut close = None;
        let mut generate = None;
        let mut all = false;
        let mut post: Option<(DocId, Option<String>)> = None;
        ui.horizontal(|ui| {
            for (i, doc) in self.docs.iter().enumerate() {
                let mut title = doc.title();
                if doc.dirty() {
                    title.push_str(" •");
                }
                let r = ui.selectable_label(i == self.active, title);
                let r = match &doc.path {
                    Some(p) => r.on_hover_text(p.display().to_string()),
                    None => r.on_hover_text("not saved yet"),
                };
                if r.clicked() {
                    self.active = i;
                }
                if ui
                    .small_button("×")
                    .on_hover_text("close this job")
                    .clicked()
                {
                    close = Some(doc.id);
                }
                ui.separator();
            }
            if ui.small_button("+").on_hover_text("new job").clicked() {
                self.new_job();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .selectable_label(self.tools.open, "Tool library")
                    .on_hover_text("your tools, in folders")
                    .clicked()
                {
                    self.tools.open = !self.tools.open;
                }
                if ui
                    .selectable_label(self.settings_open, "Settings")
                    .on_hover_text("units, and how the app shows things")
                    .clicked()
                {
                    self.settings_open = !self.settings_open;
                }
                ui.separator();
                let Some(doc) = self.docs.get(self.active) else {
                    return;
                };
                if ui.button("Generate all").clicked() {
                    all = true;
                }
                if let Some(name) = doc.active.clone() {
                    let why = doc.setup_index(&name).and_then(|i| doc.cannot_run(i));
                    if ui
                        .add_enabled(why.is_none(), egui::Button::new(format!("Generate {name}")))
                        .on_disabled_hover_text(why.unwrap_or_default())
                        .clicked()
                    {
                        generate = Some(name.clone());
                    }
                    let why = post_blocker(doc, std::slice::from_ref(&name));
                    if ui
                        .add_enabled(why.is_none(), egui::Button::new(format!("Post {name}…")))
                        .on_hover_text("write its programs, one per operation, into a folder")
                        .on_disabled_hover_text(why.unwrap_or_default())
                        .clicked()
                    {
                        post = Some((doc.id, Some(name)));
                    }
                }
                let names: Vec<String> = doc.job.setups.iter().map(|s| s.name.clone()).collect();
                let why = post_blocker(doc, &names);
                if ui
                    .add_enabled(why.is_none(), egui::Button::new("Post all…"))
                    .on_hover_text("every setup's programs, or none")
                    .on_disabled_hover_text(why.unwrap_or_default())
                    .clicked()
                {
                    post = Some((doc.id, None));
                }
                if let Some(what) = self.worker.busy(doc.id) {
                    ui.label(what);
                    ui.spinner();
                }
            });
        });
        if let Some(id) = close {
            self.close(id);
        }
        if let Some((id, scope)) = post {
            self.dialog.open(Pick::PostTo(id, scope), "");
        }
        if all {
            self.generate_all(&ctx);
        } else if let Some(name) = generate {
            self.generate(&ctx, &name);
        }
    }

    /// With no job open: what can be done.
    fn start_screen(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.25);
            ui.heading("camcraft");
            ui.add_space(12.0);
            if ui.button("New job").clicked() {
                self.new_job();
            }
            if ui.button("Open job…").clicked() {
                self.dialog.open(Pick::OpenJobs, "");
            }
            if !self.worker.idle() {
                ui.add_space(8.0);
                ui.spinner();
            }
            if !self.recent.is_empty() {
                ui.add_space(16.0);
                ui.weak("Recent");
                for path in self.recent.clone() {
                    let name = path.file_name().map_or_else(
                        || path.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    if ui
                        .link(name)
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        self.open_job(&ctx, path);
                    }
                }
            }
            for (level, text) in &self.log {
                let color = match level {
                    Level::Error => ui.visuals().error_fg_color,
                    Level::Warn => ui.visuals().warn_fg_color,
                    Level::Info => ui.visuals().text_color(),
                };
                ui.colored_label(color, text);
            }
        });
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
                ui.heading(format!("Setup {name}"));
                if state.ran.is_some() {
                    if status == Status::Stale {
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            "Out of date: the setup has changed since this ran. Generate it again.",
                        );
                    }
                    playback::result(ui, state, m);
                    ui.separator();
                    focus = playback::checks(ui, state);
                    ui.separator();
                } else if state.running.is_none() {
                    ui.weak("Not generated yet.");
                }
                focus = playback::controls(ui, state, m).or(focus);
            });
        });
        if let Some(at) = focus {
            self.viewport.focus(at, 12.0);
        }
        for a in actions {
            match a {
                BrowserAction::AddModels => self.dialog.open(Pick::AddModels(id, None), ""),
                BrowserAction::Locate(name) => self.dialog.open(Pick::Locate(id, name), ""),
                BrowserAction::Generate(name) => self.generate(&ctx, &name),
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
        let mut actions = Vec::new();
        let picks = Picks {
            armed: self.picking.armed(id),
            shown: self.picking.shown(id),
        };
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| actions = properties::show(ui, doc, m, picks));
        for a in actions {
            match a {
                PropAction::Arm(slot) => self.picking.arm(id, slot),
                PropAction::Disarm => self.picking.disarm(),
                PropAction::HoverRow(kind, i) => {
                    if self.picking.hover_row(kind, i) {
                        ctx.request_repaint();
                    }
                }
                PropAction::AddModels(setup) => self.dialog.open(Pick::AddModels(id, setup), ""),
                PropAction::Locate(name) => self.dialog.open(Pick::Locate(id, name), ""),
                PropAction::ProfileFile(setup) => self.dialog.open(Pick::Profile(id, setup), ""),
                PropAction::PickTool(setup, op) => {
                    if let Some(doc) = self.find(id) {
                        let picker = ToolPicker::new(id, setup, op, &doc.job);
                        self.picker = Some(picker);
                    }
                }
            }
        }
    }

    fn log_panel(&self, ui: &mut egui::Ui) {
        let lines = self.doc().map_or(&self.log, |d| &d.log);
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (level, text) in lines {
                    let color = match level {
                        Level::Info => ui.visuals().text_color(),
                        Level::Warn => ui.visuals().warn_fg_color,
                        Level::Error => ui.visuals().error_fg_color,
                    };
                    ui.label(egui::RichText::new(text).monospace().color(color));
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

    fn picks(&mut self, ctx: &egui::Context) {
        let Some((pick, paths)) = self.dialog.update(ctx) else {
            return;
        };
        match pick {
            Pick::OpenJobs => {
                for p in paths {
                    self.open_job(ctx, p);
                }
            }
            Pick::SaveJob(id) => {
                if let Some(path) = paths.into_iter().next() {
                    self.save_job(id, Some(path));
                }
            }
            Pick::AddModels(id, setup) => {
                if let Some(doc) = self.find(id) {
                    let names = doc.add_models(&paths);
                    if let Some(setup) = setup {
                        doc.include_models(&setup, &names);
                    }
                    doc.note(Level::Info, format!("added {}", names.join(", ")));
                }
            }
            Pick::Locate(id, name) => {
                if let (Some(doc), Some(path)) = (self.find(id), paths.first()) {
                    doc.locate_model(&name, path);
                }
            }
            Pick::PostTo(id, scope) => {
                if let Some(dir) = paths.into_iter().next() {
                    self.prepare_post(id, scope, &dir);
                }
            }
            Pick::ImportTools => {
                for path in &paths {
                    self.tools.import_fusion(path);
                }
            }
            Pick::Profile(id, setup) => {
                if let (Some(doc), Some(path)) = (self.find(id), paths.first()) {
                    let post = match &doc.path {
                        Some(_) => camcraft_session::fs::relative_path(&doc.dir(), path),
                        None => path.display().to_string(),
                    };
                    if let Some(s) = doc.job.setups.iter_mut().find(|s| s.name == setup) {
                        s.post = post;
                    }
                }
            }
        }
    }

    /// Render job `id`'s programs for `scope` (one setup, or all) into `dir`, and
    /// write them, or ask first when something stands in the way.
    fn prepare_post(&mut self, id: DocId, scope: Option<String>, dir: &Path) {
        let Some(doc) = self.find(id) else {
            return;
        };
        let names: Vec<String> = match scope {
            Some(name) => vec![name],
            None => doc.job.setups.iter().map(|s| s.name.clone()).collect(),
        };
        if let Some(why) = post_blocker(doc, &names) {
            doc.note(Level::Warn, format!("not posted: {why}"));
            return;
        }
        // The job each setup ran from: up to date, so the same as the one edited.
        let entries: Vec<_> = names
            .iter()
            .filter_map(|n| {
                let ran = doc.setups.get(n)?.ran.as_ref()?;
                let setup = ran.job.setups.iter().find(|s| &s.name == n)?;
                Some((setup, &ran.ran))
            })
            .collect();
        match posting::plan(&entries, &doc.dir(), dir) {
            Err(e) => doc.note(Level::Error, format!("not posted: {e}")),
            Ok(plan) if plan.needs_confirming() => {
                self.post_plan = Some((id, plan));
                self.post_anyway = false;
                self.prompt = Some(Prompt::Post);
            }
            Ok(plan) => self.write_post(id, &plan),
        }
    }

    fn write_post(&mut self, id: DocId, plan: &Plan) {
        let Some(doc) = self.find(id) else {
            return;
        };
        match posting::write(plan) {
            Ok(n) => {
                for (setup, _) in &plan.refused {
                    doc.note(
                        Level::Warn,
                        format!("posted '{setup}' although it did NOT pass its checks"),
                    );
                }
                doc.note(
                    Level::Info,
                    format!("posted {n} program(s) to {}", plan.dir.display()),
                );
            }
            Err(e) => doc.note(Level::Error, format!("posting failed: {e}")),
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
        let Some(outcome) = picker.show(ctx, &doc.job, library, m) else {
            return;
        };
        self.picker = None;
        let Some((picked, preset)) = outcome else {
            return;
        };
        let name = match picked {
            Picked::Job(name) => name,
            Picked::Library(hint, spec) => doc.adopt_tool(&hint, &spec),
        };
        doc.use_tool(&setup, op, name, preset.as_deref());
    }

    fn prompts(&mut self, ctx: &egui::Context) {
        let Some(prompt) = self.prompt.clone() else {
            return;
        };
        let mut done = false;
        let mut then: Option<Answer> = None;
        let modal = egui::Modal::new(egui::Id::new("prompt")).show(ctx, |ui| {
            ui.set_width(420.0);
            match &prompt {
                Prompt::Close(id) => {
                    let id = *id;
                    let title = self
                        .docs
                        .iter()
                        .find(|d| d.id == id)
                        .map(Document::title)
                        .unwrap_or_default();
                    ui.heading(format!("Save changes to {title}?"));
                    ui.label("They are lost if you close without saving.");
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            then = Some(Box::new(move |app: &mut Self| {
                                app.close_after_save = Some(id);
                                app.save_job(id, None);
                            }));
                        }
                        if ui.button("Don't save").clicked() {
                            then = Some(Box::new(move |app: &mut Self| app.remove(id)));
                        }
                        if ui.button("Cancel").clicked() {
                            done = true;
                        }
                    });
                }
                Prompt::Quit => {
                    ui.heading("Quit with unsaved changes?");
                    let unsaved: Vec<&Document> = self.docs.iter().filter(|d| d.dirty()).collect();
                    for d in &unsaved {
                        ui.label(format!("• {}", d.title()));
                    }
                    let all_have_a_place = unsaved.iter().all(|d| d.path.is_some());
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(all_have_a_place, egui::Button::new("Save all and quit"))
                            .on_disabled_hover_text(
                                "a new job needs a place first: save it with Save As",
                            )
                            .clicked()
                        {
                            then = Some(Box::new(|app: &mut Self| {
                                let ids: Vec<DocId> = app
                                    .docs
                                    .iter()
                                    .filter(|d| d.dirty())
                                    .map(|d| d.id)
                                    .collect();
                                for id in ids {
                                    app.save_job(id, None);
                                }
                                if app.docs.iter().all(|d| !d.dirty()) {
                                    app.docs.clear();
                                }
                            }));
                        }
                        if ui.button("Quit without saving").clicked() {
                            then = Some(Box::new(|app: &mut Self| app.docs.clear()));
                        }
                        if ui.button("Cancel").clicked() {
                            done = true;
                        }
                    });
                }
                Prompt::Missing(id) => {
                    let id = *id;
                    let (title, missing) = self
                        .docs
                        .iter()
                        .find(|d| d.id == id)
                        .map(|d| (d.title(), d.missing_models()))
                        .unwrap_or_default();
                    ui.heading(format!("{title}: models not found"));
                    ui.label("These are not where the job says. Find each, or leave it for now.");
                    for name in missing {
                        ui.horizontal(|ui| {
                            ui.label(&name);
                            if ui.button("Find…").clicked() {
                                then = Some(Box::new(move |app: &mut Self| {
                                    app.dialog.open(Pick::Locate(id, name), "");
                                }));
                            }
                        });
                    }
                    if ui.button("Later").clicked() {
                        done = true;
                    }
                }
                Prompt::Post => {
                    let Some((id, plan)) = self.post_plan.as_ref() else {
                        done = true;
                        return;
                    };
                    ui.heading(format!(
                        "Post {} program(s) to {}?",
                        plan.programs.len(),
                        plan.dir.display()
                    ));
                    if !plan.refused.is_empty() {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            "These setups did NOT pass their checks. Their programs are \
                             not safe to run as they stand:",
                        );
                        egui::ScrollArea::vertical()
                            .id_salt("refused")
                            .max_height(180.0)
                            .show(ui, |ui| {
                                for (setup, why) in &plan.refused {
                                    ui.strong(setup);
                                    for w in why {
                                        ui.label(format!("• {w}"));
                                    }
                                }
                            });
                        ui.checkbox(
                            &mut self.post_anyway,
                            "I know why, and want these programs anyway",
                        );
                    }
                    if !plan.overwrites.is_empty() {
                        ui.label("These files are there already, and will be replaced:");
                        for p in plan.overwrites.iter().take(8) {
                            let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
                            ui.monospace(name.unwrap_or_default());
                        }
                        if plan.overwrites.len() > 8 {
                            ui.weak(format!("and {} more", plan.overwrites.len() - 8));
                        }
                    }
                    let go = if plan.refused.is_empty() {
                        "Replace and post"
                    } else {
                        "Post anyway"
                    };
                    let allowed = plan.refused.is_empty() || self.post_anyway;
                    let id = *id;
                    ui.horizontal(|ui| {
                        if ui.add_enabled(allowed, egui::Button::new(go)).clicked() {
                            then = Some(Box::new(move |app: &mut Self| {
                                if let Some((_, plan)) = app.post_plan.take() {
                                    app.write_post(id, &plan);
                                }
                            }));
                        }
                        if ui.button("Cancel").clicked() {
                            done = true;
                        }
                    });
                }
            }
        });
        if let Some(f) = then {
            self.prompt = None;
            f(self);
            if prompt == Prompt::Quit && self.docs.is_empty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        } else if done || modal.should_close() {
            self.prompt = None;
            self.post_plan = None;
        }
    }

    /// The showing setup's state, if there is one.
    fn active_state_mut(&mut self) -> Option<&mut crate::document::SetupState> {
        let d = self.docs.get_mut(self.active)?;
        let name = d.active.clone()?;
        d.setups.get_mut(&name)
    }

    /// What the viewport should draw this frame.
    fn content(&self) -> Content<'_> {
        let state = self.doc().and_then(|d| d.setups.get(d.active.as_ref()?));
        let mut content = match state {
            Some(state) => playback::content(state),
            None => Content::empty(),
        };
        if let Some(doc) = self.doc() {
            self.picking.decorate(doc.id, &mut content);
        }
        content
    }

    // ---- for the snapshot harness ------------------------------------------------

    /// The showing setup's last run, if any. Only the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn outcome(&self) -> Option<&camcraft_session::Ran> {
        let d = self.doc()?;
        Some(&d.setups.get(d.active.as_ref()?)?.ran.as_ref()?.ran)
    }

    /// The showing setup's state. Only the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    fn state_for_snapshot(&mut self) -> Option<&mut crate::document::SetupState> {
        self.active_state_mut()
    }

    /// The showing job. Only the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn doc_for_snapshot(&mut self) -> Option<&mut Document> {
        self.doc_mut()
    }

    /// Select operation `op` of the showing setup and pick faces for it in the
    /// view: the two largest that face up. Only the snapshot harness asks. How many faces the model has.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn pick_faces_for_snapshot(&mut self, op: usize) -> usize {
        use camcraft_core::job::FeatureRef;
        use camcraft_geom::topology::FaceKind;
        let Some(doc) = self.docs.get_mut(self.active) else {
            return 0;
        };
        let (Some(setup), id) = (doc.active.clone(), doc.id) else {
            return 0;
        };
        let Some(model) = doc
            .job
            .setups
            .iter()
            .find(|s| s.name == setup)
            .and_then(|s| s.models.first().cloned())
        else {
            return 0;
        };
        let Some((loaded, _)) = doc.models.get(&model).and_then(|s| s.loaded()) else {
            return 0;
        };
        let t = loaded.body.topology();
        let mut up: Vec<(u32, f64, glam::DVec3)> = t
            .faces()
            .iter()
            .enumerate()
            .filter_map(|(i, f)| match f.kind {
                FaceKind::Plane { normal, .. } if normal.z > 0.9 => {
                    let tri = loaded
                        .body
                        .mesh()
                        .triangle(*f.triangles.first()? as usize)?;
                    let at = (tri[0] + tri[1] + tri[2]) / 3.0;
                    Some((u32::try_from(i).ok()?, f.area, at))
                }
                _ => None,
            })
            .collect();
        up.sort_by(|a, b| b.1.total_cmp(&a.1));
        let faces = t.faces().len();
        doc.selection = crate::document::Selection::Operation(setup.clone(), op);
        let Some(o) = doc
            .job
            .setups
            .iter_mut()
            .find(|s| s.name == setup)
            .and_then(|s| s.operations.get_mut(op))
        else {
            return faces;
        };
        o.geometry.faces = up
            .iter()
            .take(2)
            .map(|(i, _, at)| FeatureRef {
                model: model.clone(),
                index: *i,
                at: at.to_array(),
            })
            .collect();
        self.picking.arm(
            id,
            crate::slots::Slot::Faces {
                setup: setup.clone(),
                op,
            },
        );
        faces
    }

    /// Select the showing setup and arm its work zero for picking; where a round
    /// rim is drawn, and the setup-frame centre it should snap to. Only the
    /// snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn arm_work_zero_for_snapshot(&mut self) -> Option<(egui::Pos2, glam::DVec3)> {
        let doc = self.docs.get_mut(self.active)?;
        let setup = doc.active.clone()?;
        doc.selection = crate::document::Selection::Setup(setup.clone());
        let id = doc.id;
        self.picking.arm(id, crate::slots::Slot::WorkZero { setup });
        let eye = self.viewport.camera().eye().as_dvec3();
        let (rim, centre, rect) = self.picking.rim_for_snapshot(eye)?;
        let aspect = rect.width() / rect.height().max(1.0);
        let proj =
            crate::pick::Projector::new(self.viewport.camera().view_projection(aspect), rect);
        Some((proj.screen(rim)?, centre))
    }

    /// Arm `slot` for picking in the showing job. Only the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn arm_for_snapshot(&mut self, slot: crate::slots::Slot) {
        if let Some(doc) = self.docs.get(self.active) {
            let id = doc.id;
            self.picking.arm(id, slot);
        }
    }

    /// Press Esc: stop picking. Only the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn escape_for_snapshot(&mut self) {
        self.picking.escape();
    }

    /// Open the job dialog, as File › Open would. Only the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn open_dialog_for_snapshot(&mut self) {
        self.dialog.open(Pick::OpenJobs, "");
    }

    /// Ask to close the showing job, as ⌘W would. Only the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn close_for_snapshot(&mut self) {
        if let Some(id) = self.doc().map(|d| d.id) {
            self.close(id);
        }
    }

    /// Open the tool picker for an operation, or close it for `None`. Only the
    /// snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn pick_tool_for_snapshot(&mut self, target: Option<(&str, usize)>) {
        self.picker = target.and_then(|(setup, op)| {
            let d = self.doc()?;
            Some(ToolPicker::new(d.id, setup.into(), op, &d.job))
        });
    }

    /// Put the scrub part-way through operation `op`, and show only `roles`. Only
    /// the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn set_view_for_snapshot(&mut self, op: usize, fraction: f64, roles: u32) {
        let Some(state) = self.state_for_snapshot() else {
            return;
        };
        if let Some(span) = state.ran.as_ref().and_then(|r| r.scene.ops.get(op)) {
            // A fraction of a count, for a test's choice of frame.
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let into = (span.count as f64 * fraction) as usize;
            state.view.at = crate::scene::Playhead::at(span.first + into.min(span.count));
        }
        state.view.view.roles = roles;
    }

    /// Show the stock at a fixed stage, or following the scrub. Only the snapshot
    /// harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn set_stock_for_snapshot(&mut self, stage: Option<usize>) {
        use crate::document::StockMode;
        if let Some(state) = self.state_for_snapshot() {
            state.view.stock_mode = stage.map_or(StockMode::Follow, StockMode::Stage);
            state.view.view.roles = u32::MAX;
            state.view.show_paths = false;
        }
    }

    /// Hide the material. Only the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn stock_off_for_snapshot(&mut self) {
        if let Some(state) = self.state_for_snapshot() {
            state.view.stock_mode = crate::document::StockMode::Off;
        }
    }

    /// Bring the camera in on `at`. Only the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn focus_for_snapshot(&mut self, at: glam::DVec3, radius: f32) {
        self.viewport.focus(at, radius);
    }

    /// The showing setup's playhead, and whether it is playing. Only the snapshot
    /// harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn playhead_for_snapshot(&mut self) -> Option<(crate::scene::Playhead, bool)> {
        let view = &self.state_for_snapshot()?.view;
        Some((view.at, view.transport.playing))
    }

    /// Whether the playback material stands at the playhead. Only the snapshot
    /// harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn live_caught_up_for_snapshot(&mut self) -> bool {
        self.state_for_snapshot()
            .and_then(|s| s.live.as_ref())
            .is_some_and(crate::live::LiveStock::caught_up)
    }

    /// Go to the deepest steep descent, as clicking it would. Only the snapshot
    /// harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn jump_to_deepest_for_snapshot(&mut self) -> Option<f64> {
        let state = self.state_for_snapshot()?;
        let deepest = state
            .ran
            .as_ref()?
            .scene
            .steep
            .iter()
            .copied()
            .max_by(|a, b| a.drop.total_cmp(&b.drop))?;
        let at = playback::jump_to(state, deepest);
        self.viewport.focus(at, 12.0);
        Some(deepest.drop)
    }

    /// Make one of the showing setup's checks fail, as a rapid through the part
    /// would, then post every setup into `dir`: the HALT banner, and the prompt
    /// that asks before posting anyway. Only the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn fail_a_check_and_post_for_snapshot(&mut self, dir: &Path) {
        use camcraft_session::checks::Level as CheckLevel;
        let id = self.doc().map(|d| d.id);
        if let Some(ran) = self.state_for_snapshot().and_then(|s| s.ran.as_mut()) {
            let len = ran
                .ran
                .result
                .operations
                .first()
                .map_or(0, |o| o.path.len());
            if let Some(f) = ran
                .ran
                .checks
                .groups
                .get_mut(1)
                .and_then(|g| g.findings.iter_mut().find(|f| f.code == "CC102"))
            {
                f.level = CheckLevel::Fail;
                f.message = "1 rapid(s) pass through the part".into();
                f.detail = "deepest 2.40 mm under the cutter".into();
                f.at_move = Some(len / 2);
            }
        }
        if let Some(id) = id {
            self.prepare_post(id, None, dir);
        }
    }

    /// Use a library of two machines' folders, kept at `path`. Only the snapshot
    /// harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn library_for_snapshot(&mut self, path: PathBuf) {
        self.tools = ToolManager::with_library(path);
        self.tools.folders_for_snapshot();
    }

    /// Show lengths in `measure`, with the settings window open. Only the snapshot
    /// harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn settings_for_snapshot(&mut self, measure: Measure) {
        self.settings.measure = measure;
        self.settings_open = true;
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        for (id, task, reply) in self.worker.poll() {
            self.accept(id, task, reply);
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
                transport::keys(&ctx, state);
                transport::tick(&ctx, state);
                playback::ensure_stock_image(state);
                playback::follow_stock(&ctx, state);
            }
        }
        self.swap_camera();
        let m = self.measure();
        self.picking.refresh(self.docs.get(self.active), m);
        self.tools.measure = m;
        self.tools.show(&ctx);
        if std::mem::take(&mut self.tools.import_requested) {
            self.dialog.open(Pick::ImportTools, "");
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        egui::Panel::top("tabs").show(ui, |ui| self.tabs(ui));
        self.settings.show(&ctx, &mut self.settings_open);
        self.picks(&ctx);
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
            egui::Panel::bottom("log")
                .resizable(true)
                .default_size(130.0)
                .show(ui, |ui| self.log_panel(ui));
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    if let Some(state) = self.active_state_mut().filter(|s| s.ran.is_some()) {
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
                    // The viewport is taken out for the call, because the content it
                    // draws borrows the rest of the app.
                    let mut viewport = std::mem::take(&mut self.viewport);
                    let event = viewport.show(ui, frame.wgpu_render_state(), &self.content());
                    self.viewport = viewport;
                    // A frame with a GPU uploads whatever of the material changed.
                    if frame.wgpu_render_state().is_some()
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
