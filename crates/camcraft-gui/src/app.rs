//! The window: model and job on the left, the part in the middle, a log beneath.
//!
//! What runs is exactly what `camcraft job` runs. A run reads the job file afresh,
//! loads the model at the job's tolerance and goes through `camcraft-session`, so
//! the app never holds a second opinion about the engine.

use std::path::{Path, PathBuf};

use camcraft_core::job::{Job, StrategySpec};
use camcraft_session::{Model, SessionError, Units, read_job};
use eframe::egui;

use crate::file_dialog::{AppFileDialog, Pick};
use crate::measure::Measure;
use crate::scene::{Scene, Steep, clock};
use crate::settings::Settings;
use crate::stock_view::{CLASSES, StockImage, class_color, class_name};
use crate::theme;
use crate::tool_manager::ToolManager;
use crate::viewport::{Content, View, Viewport};
use crate::worker::{Outcome, Reply, Request, Worker};

/// Tessellation tolerance for looking at a model before any job is open. A run
/// always reloads the model at its job's own tolerance.
const PREVIEW_TOLERANCE: f64 = 0.05;

const STORAGE_DIALOG: &str = "file_dialog";
const STORAGE_SETTINGS: &str = "settings";

/// Which material to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StockMode {
    /// None.
    Off,
    /// As it stood when the scrubbed operation began, or after it once the scrub
    /// reaches its end.
    Follow,
    /// A fixed stage: the block before everything is 0, after operation `n` is
    /// `n + 1`.
    Stage(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Info,
    Warn,
    Error,
}

/// The app's state.
pub struct App {
    dialog: AppFileDialog,
    worker: Worker,
    viewport: Viewport,
    model_path: Option<PathBuf>,
    /// What an STL's numbers mean. Chosen by the person; never guessed.
    units: Option<Units>,
    job_path: Option<PathBuf>,
    job: Option<Job>,
    model: Option<Model>,
    /// Changes whenever `model` does, so the viewport uploads a new mesh once.
    generation: u64,
    outcome: Option<Outcome>,
    /// What the last run drew: toolpaths, stock and cutters.
    scene: Option<Scene>,
    /// Changes whenever `scene` does, so it is uploaded once.
    scene_generation: u64,
    /// Changes only when a different model is opened, so the view is framed on it
    /// once and never snapped back by a re-run.
    opened: u64,
    view: View,
    /// Segments shown, in machine order: the scrub.
    scrub: usize,
    show_cutter: bool,
    /// All toolpaths at once, over the per-role choices, so the material can be
    /// seen without them.
    show_paths: bool,
    stock_mode: StockMode,
    /// The material covers the part, so the part is hidden while it shows.
    hide_part_with_stock: bool,
    /// The material at each stage, classified and coloured, built when first shown.
    stock_images: Vec<Option<StockImage>>,
    /// The tool manager, in its own window when open.
    tools: ToolManager,
    settings: Settings,
    settings_open: bool,
    log: Vec<(Level, String)>,
}

/// `12345` as `12,345`, as the inspector printed counts.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A checkbox for one bit of a mask.
fn bit(ui: &mut egui::Ui, mask: &mut u32, index: usize, label: impl Into<egui::WidgetText>) {
    let Some(flag) = u32::try_from(index).ok().and_then(|i| 1u32.checked_shl(i)) else {
        return;
    };
    let mut on = *mask & flag != 0;
    if ui.checkbox(&mut on, label).changed() {
        if on {
            *mask |= flag;
        } else {
            *mask &= !flag;
        }
    }
}

fn is_stl(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("stl"))
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// A session error, worded for this app rather than for the command line.
fn explain(e: &SessionError) -> String {
    match e {
        SessionError::StlNeedsUnits => {
            "This STL carries no units. Choose millimetres or inches under Model; \
             guessing would make an inch part 25.4 times too small."
                .into()
        }
        other => other.to_string(),
    }
}

impl App {
    /// The app, restoring what the file dialog remembers.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let storage = cc
            .storage
            .and_then(|s| eframe::get_value(s, STORAGE_DIALOG));
        let settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, STORAGE_SETTINGS))
            .unwrap_or_default();
        Self {
            dialog: AppFileDialog::new(storage),
            worker: Worker::default(),
            viewport: Viewport::default(),
            model_path: None,
            units: None,
            job_path: None,
            job: None,
            model: None,
            generation: 0,
            outcome: None,
            scene: None,
            scene_generation: 0,
            opened: 0,
            view: View::default(),
            scrub: 0,
            show_cutter: true,
            show_paths: true,
            stock_mode: StockMode::Off,
            hide_part_with_stock: true,
            stock_images: Vec::new(),
            tools: ToolManager::new(),
            settings,
            settings_open: false,
            log: Vec::new(),
        }
    }

    /// Forget the last run: its result and everything drawn from it.
    fn clear_run(&mut self) {
        self.outcome = None;
        self.scene = None;
        self.stock_images.clear();
    }

    /// The stage the stock view shows, if any.
    fn stage_shown(&self) -> Option<usize> {
        match self.stock_mode {
            StockMode::Off => None,
            StockMode::Stage(i) => Some(i),
            StockMode::Follow => {
                let scene = self.scene.as_ref()?;
                let at = scene.position(self.scrub);
                let moves = scene.ops.get(at.op).map_or(0, |o| o.count + 1);
                Some(if at.moves >= moves { at.op + 1 } else { at.op })
            }
        }
    }

    /// Classify the shown stage's material, the first time it is shown.
    fn ensure_stock_image(&mut self) {
        let (Some(stage), Some(outcome)) = (self.stage_shown(), &self.outcome) else {
            return;
        };
        let result = &outcome.ran.result;
        if let (Some(slot @ None), Some(stock)) =
            (self.stock_images.get_mut(stage), result.stages.get(stage))
        {
            *slot = Some(StockImage::build(stock, &result.part, result.verify_budget));
        }
    }

    /// Move the scrub to a steep descent and look at it closely, with the material
    /// as it stood.
    fn jump_to(&mut self, steep: Steep) {
        self.scrub = steep.index + 1;
        self.stock_mode = StockMode::Follow;
        self.viewport.focus(steep.at, 12.0);
    }

    /// The app, with files already open, as from the command line.
    ///
    /// The job is read first, so the model is tessellated once, at the job's own
    /// tolerance.
    pub fn with_files(
        cc: &eframe::CreationContext<'_>,
        model: Option<PathBuf>,
        job: Option<PathBuf>,
    ) -> Self {
        let mut app = Self::new(cc);
        if let Some(job) = job {
            app.open_job(&cc.egui_ctx, job);
        }
        if let Some(model) = model {
            app.open_model(&cc.egui_ctx, model);
        }
        app
    }

    fn measure(&self) -> Measure {
        self.settings.measure
    }

    fn note(&mut self, level: Level, text: impl Into<String>) {
        self.log.push((level, text.into()));
    }

    /// Open a model file, as if picked.
    pub fn open_model(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.note(Level::Info, format!("model: {}", path.display()));
        self.model = None;
        self.clear_run();
        self.opened += 1;
        if !is_stl(&path) {
            // STEP states its own units; anything given here would only be a
            // chance to contradict it.
            self.units = None;
        }
        self.model_path = Some(path);
        self.load(ctx);
    }

    /// Open a job file, as if picked.
    pub fn open_job(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.clear_run();
        match read_job(&path) {
            Ok(job) => {
                self.note(
                    Level::Info,
                    format!(
                        "job: {} ({} operation(s), tolerance {})",
                        path.display(),
                        job.operations.len(),
                        self.measure().length_trimmed(job.tolerance)
                    ),
                );
                self.job = Some(job);
                self.job_path = Some(path);
                // The preview was tessellated without a job; look again at the
                // job's own tolerance.
                if self.model_path.is_some() {
                    self.load(ctx);
                }
            }
            Err(e) => {
                self.note(Level::Error, explain(&e));
                self.job = None;
                self.job_path = None;
            }
        }
    }

    /// Run the job, the way `camcraft job` does.
    pub fn start_run(&mut self, ctx: &egui::Context) {
        let (Some(model), Some(job)) = (self.model_path.clone(), self.job_path.clone()) else {
            return;
        };
        self.note(Level::Info, "running…");
        self.worker.start(
            ctx,
            Request::Run {
                model,
                units: self.units,
                job,
            },
        );
    }

    /// Whether the worker is busy, and with what.
    pub fn busy(&self) -> Option<&'static str> {
        self.worker.busy()
    }

    /// The last run's result, if any. Only the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn outcome(&self) -> Option<&Outcome> {
        self.outcome.as_ref()
    }

    /// Open the model dialog, as the button would. Only the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn open_dialog_for_snapshot(&mut self) {
        self.dialog.open(Pick::Model);
    }

    /// Put the scrub part-way through operation `op`, and show only `roles`. Only
    /// the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn set_view_for_snapshot(&mut self, op: usize, fraction: f64, roles: u32) {
        if let Some(span) = self.scene.as_ref().and_then(|s| s.ops.get(op)) {
            // A fraction of a count, for a test's choice of frame.
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let into = (span.count as f64 * fraction) as usize;
            self.scrub = span.first + into.min(span.count);
        }
        self.view.roles = roles;
    }

    /// Show the stock at a fixed stage, or following the scrub. Only the snapshot
    /// harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn set_stock_for_snapshot(&mut self, stage: Option<usize>) {
        self.stock_mode = stage.map_or(StockMode::Follow, StockMode::Stage);
        self.view.roles = u32::MAX;
        self.show_paths = false;
    }

    /// Go to the deepest steep descent, as clicking it would. Only the snapshot
    /// harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn jump_to_deepest_for_snapshot(&mut self) -> Option<f64> {
        let deepest = self
            .scene
            .as_ref()?
            .steep
            .iter()
            .copied()
            .max_by(|a, b| a.drop.total_cmp(&b.drop))?;
        self.jump_to(deepest);
        Some(deepest.drop)
    }

    /// Show lengths in `measure`, with the settings window open. Only the snapshot
    /// harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn settings_for_snapshot(&mut self, measure: Measure) {
        self.settings.measure = measure;
        self.settings_open = true;
    }

    fn load(&mut self, ctx: &egui::Context) {
        let Some(path) = self.model_path.clone() else {
            return;
        };
        if is_stl(&path) && self.units.is_none() {
            self.note(
                Level::Warn,
                "choose the units this STL is in before it can be loaded",
            );
            return;
        }
        let tolerance = self.job.as_ref().map_or(PREVIEW_TOLERANCE, |j| j.tolerance);
        self.worker.start(
            ctx,
            Request::Load {
                path,
                units: self.units,
                tolerance,
            },
        );
    }

    fn can_run(&self) -> bool {
        let units_ok = self
            .model_path
            .as_deref()
            .is_some_and(|p| !is_stl(p) || self.units.is_some());
        units_ok && self.job_path.is_some() && self.busy().is_none()
    }

    fn accept(&mut self, reply: Reply) {
        match reply {
            Reply::Loaded(Ok(model)) => {
                for n in &model.notes {
                    self.log.push((Level::Info, n.clone()));
                }
                for w in &model.warnings {
                    self.log.push((Level::Warn, w.clone()));
                }
                self.model = Some(model);
                self.generation += 1;
            }
            Reply::Loaded(Err(e)) => self.note(Level::Error, explain(&e)),
            Reply::Ran(outcome) => match *outcome {
                Ok(outcome) => {
                    for n in outcome.model.notes.iter().chain(&outcome.ran.notes) {
                        self.log.push((Level::Info, n.clone()));
                    }
                    for w in &outcome.ran.warnings {
                        self.log.push((Level::Warn, w.clone()));
                    }
                    let verdict = if outcome.ran.result.is_clean() {
                        (Level::Info, "verified: the job may be posted")
                    } else {
                        (Level::Error, "NOT verified: the job would be refused")
                    };
                    self.note(verdict.0, verdict.1);
                    self.model = Some(outcome.model.clone());
                    self.job = Some(outcome.job.clone());
                    self.generation += 1;
                    let scene = Scene::build(&outcome.job, &outcome.ran.result);
                    // A new run starts fully shown, every operation visible.
                    self.scrub = scene.segments.len();
                    self.view.ops = u32::MAX;
                    self.stock_images = vec![None; outcome.ran.result.stages.len()];
                    self.scene = Some(scene);
                    self.scene_generation += 1;
                    self.outcome = Some(outcome);
                }
                Err(e) => self.note(Level::Error, explain(&e)),
            },
            Reply::Died(what) => self.note(
                Level::Error,
                format!(
                    "the engine stopped while {what} and gave no answer. This is a bug; \
                     the command line may say more."
                ),
            ),
        }
    }

    fn side(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let m = self.measure();
        let idle = self.busy().is_none() && !self.dialog.is_open();

        ui.heading("Model");
        if ui
            .add_enabled(idle, egui::Button::new("Open model…"))
            .clicked()
        {
            self.dialog.open(Pick::Model);
        }
        if let Some(path) = self.model_path.clone() {
            ui.monospace(file_name(&path));
            if is_stl(&path) {
                let before = self.units;
                ui.horizontal(|ui| {
                    ui.label("Units:");
                    ui.add_enabled_ui(idle, |ui| {
                        ui.radio_value(&mut self.units, Some(Units::Millimetres), "mm");
                        ui.radio_value(&mut self.units, Some(Units::Inches), "inch");
                    });
                });
                if self.units != before {
                    self.model = None;
                    self.clear_run();
                    self.load(&ctx);
                }
            }
        }
        if let Some(model) = &self.model {
            let e = model.bounds.extent();
            egui::Grid::new("model").num_columns(2).show(ui, |ui| {
                ui.label("file units");
                ui.label(&model.units);
                ui.end_row();
                ui.label("size");
                ui.label(format!(
                    "{} × {} × {}",
                    m.number(e.x, 1),
                    m.number(e.y, 1),
                    m.length(e.z, 1)
                ));
                ui.end_row();
                ui.label("triangles");
                ui.label(model.mesh.len().to_string());
                ui.end_row();
            });
        }

        ui.separator();
        ui.heading("Job");
        if ui
            .add_enabled(idle, egui::Button::new("Open job…"))
            .clicked()
        {
            self.dialog.open(Pick::Job);
        }
        if let (Some(path), Some(job)) = (&self.job_path, &self.job) {
            ui.monospace(file_name(path));
            ui.label(format!("tolerance {}", m.length_trimmed(job.tolerance)));
            for (i, op) in job.operations.iter().enumerate() {
                let strategy = match op.strategy {
                    StrategySpec::Parallel { .. } => "parallel",
                    StrategySpec::ZlevelRough { .. } => "z-level rough",
                };
                let tool = job.tool_for(op).map_or_else(String::new, |t| {
                    let kind = format!("{:?}", t.kind).to_lowercase();
                    format!(", {kind} Ø{}", m.length_trimmed(t.diameter))
                });
                ui.label(format!("{}. {} — {strategy}{tool}", i + 1, op.name));
            }
        }

        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.can_run(), egui::Button::new("Run"))
                .clicked()
            {
                self.start_run(&ctx);
            }
            if let Some(what) = self.busy() {
                ui.spinner();
                ui.label(what);
            }
        });

        if let Some(outcome) = &self.outcome {
            ui.separator();
            ui.heading("Result");
            egui::Grid::new("result")
                .num_columns(4)
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("operation");
                    ui.strong("passes");
                    ui.strong("removed");
                    ui.strong("rapid hits");
                    ui.end_row();
                    for op in &outcome.ran.result.operations {
                        ui.label(&op.name);
                        ui.label(op.passes.to_string());
                        ui.label(m.volume(op.sim.removed_mm3));
                        ui.label(op.sim.rapid_contacts.to_string());
                        ui.end_row();
                    }
                });
            let v = &outcome.ran.result.verify;
            if outcome.ran.result.is_clean() {
                ui.colored_label(
                    ui.visuals().text_color(),
                    format!("Verified: {} columns compared, no gouges.", v.compared),
                );
            } else {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!(
                        "NOT verified: {} gouged column(s), deepest {}.",
                        v.gouged,
                        m.length(v.deepest_gouge_mm, 3)
                    ),
                );
            }
            for w in &outcome.ran.warnings {
                ui.colored_label(ui.visuals().warn_fg_color, w);
            }
        }
        self.controls(ui);
    }

    /// What to show, and the scrub through the program in machine order.
    fn controls(&mut self, ui: &mut egui::Ui) {
        let Some(scene) = &self.scene else {
            return;
        };
        let m = self.settings.measure;
        let view = &mut self.view;

        ui.separator();
        ui.heading("Show");
        ui.checkbox(&mut view.part, "part");
        ui.checkbox(&mut view.stock, "stock outline");
        ui.checkbox(&mut self.show_cutter, "cutter at the scrub");
        ui.checkbox(&mut self.show_paths, "toolpaths");
        ui.add_space(4.0);
        for (i, role) in theme::ROLES.iter().enumerate() {
            let n = scene.role_counts.get(i).copied().unwrap_or(0);
            if n == 0 {
                continue;
            }
            ui.horizontal(|ui| {
                bit(ui, &mut view.roles, i, "");
                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(r, 2.0, theme::egui_color(theme::role_srgb(*role)));
                ui.label(theme::role_name(*role));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(thousands(n));
                });
            });
        }
        let mut jump = None;
        if scene.ops.len() > 1 {
            ui.add_space(4.0);
            for (i, op) in scene.ops.iter().enumerate() {
                ui.horizontal(|ui| {
                    bit(ui, &mut view.ops, i, format!("{}. {}", i + 1, op.name));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button("scrub to end")
                            .on_hover_text("show everything up to the end of this operation")
                            .clicked()
                        {
                            jump = Some(op.first + op.count);
                        }
                        ui.weak(thousands(op.count));
                    });
                });
            }
        }
        if let Some(end) = jump {
            self.scrub = end;
        }

        ui.separator();
        ui.heading("Move order");
        let total = scene.segments.len();
        ui.spacing_mut().slider_width = ui.available_width() - 8.0;
        ui.add(egui::Slider::new(&mut self.scrub, 0..=total).show_value(false));
        ui.label(format!(
            "showing {} / {}",
            thousands(self.scrub),
            thousands(total)
        ));
        if let Some(at) = scene.at(self.scrub) {
            let s = at.segment;
            ui.label(format!("{}. {}", at.op + 1, at.op_name));
            let feed = s
                .feed
                .map_or_else(String::new, |f| format!("  F{}", m.feed_number(f)));
            ui.monospace(format!(
                "{} to {}, {}, {} {}{feed}",
                theme::role_name(s.role),
                m.number(s.b.x, 2),
                m.number(s.b.y, 2),
                m.number(s.b.z, 2),
                m.unit()
            ));
        }
        if let Some(at) = scene.at(self.scrub) {
            ui.label(format!(
                "{} of {}, estimated",
                clock(at.segment.ends_at),
                clock(scene.total_time())
            ));
        }
        ui.weak("Left and Right arrows step one move; with Shift, a hundred.");

        ui.separator();
        ui.heading("Stock");
        let stage_name = |i: usize| match i {
            0 => "the block, before anything".to_string(),
            n => scene
                .ops
                .get(n - 1)
                .map_or_else(String::new, |o| format!("after {n}. {}", o.name)),
        };
        let selected = match self.stock_mode {
            StockMode::Off => "off".to_string(),
            StockMode::Follow => "follow the scrub".to_string(),
            StockMode::Stage(i) => stage_name(i),
        };
        egui::ComboBox::from_id_salt("stock")
            .selected_text(selected)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.stock_mode, StockMode::Off, "off");
                ui.selectable_value(&mut self.stock_mode, StockMode::Follow, "follow the scrub");
                for i in 0..=scene.ops.len() {
                    ui.selectable_value(&mut self.stock_mode, StockMode::Stage(i), stage_name(i));
                }
            });
        ui.checkbox(
            &mut self.hide_part_with_stock,
            "hide the part while showing it",
        );
        let image = self
            .stage_shown()
            .and_then(|i| self.stock_images.get(i))
            .and_then(Option::as_ref);
        if let Some(image) = image {
            for class in CLASSES {
                ui.horizontal(|ui| {
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(r, 2.0, theme::egui_color(class_color(class)));
                    ui.label(class_name(class));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.weak(thousands(image.count(class)));
                    });
                });
            }
            ui.weak("cells of the simulated material, by the verifier's rule");
        }

        let mut jump = None;
        if !scene.steep.is_empty() {
            ui.separator();
            ui.heading("Steep descents");
            ui.weak("cutting moves that drop further than allowed while barely advancing");
            let job = self.outcome.as_ref().map(|o| &o.job);
            for (i, op) in scene.ops.iter().enumerate() {
                let mine: Vec<&Steep> = scene.steep.iter().filter(|s| s.op == i).collect();
                if mine.is_empty() {
                    continue;
                }
                let deepest = mine.iter().map(|s| s.drop).fold(0.0, f64::max);
                let allowance = job
                    .and_then(|j| j.operations.get(i))
                    .map_or(0.0, |o| o.passes.max_cut_step);
                ui.label(format!(
                    "{}. {}: {} move(s), deepest {} of {} allowed",
                    i + 1,
                    op.name,
                    mine.len(),
                    m.length(deepest, 2),
                    m.length(allowance, 2)
                ));
            }
            // Deepest first: the worst is the one to look at.
            let mut worst: Vec<Steep> = scene.steep.clone();
            worst.sort_by(|a, b| b.drop.total_cmp(&a.drop));
            ui.weak(format!("drop, operation and position ({})", m.unit()));
            egui::ScrollArea::vertical()
                .id_salt("steep")
                .max_height(160.0)
                .show(ui, |ui| {
                    for s in worst.iter().take(200) {
                        let label = format!(
                            "{}  {}. at {}, {}, {}",
                            m.number(s.drop, 2),
                            s.op + 1,
                            m.number(s.at.x, 1),
                            m.number(s.at.y, 1),
                            m.number(s.at.z, 1)
                        );
                        if ui
                            .selectable_label(false, egui::RichText::new(label).monospace())
                            .on_hover_text("go there: scrub to it and follow the stock")
                            .clicked()
                        {
                            jump = Some(*s);
                        }
                    }
                });
        }
        if let Some(s) = jump {
            self.jump_to(s);
        }
    }

    /// Arrow keys step the scrub, unless a widget has the keyboard.
    fn step_scrub(&mut self, ctx: &egui::Context) {
        let Some(total) = self.scene.as_ref().map(|s| s.segments.len()) else {
            return;
        };
        if ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        let (left, right, shift) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.modifiers.shift,
            )
        });
        let step = if shift { 100 } else { 1 };
        if right {
            self.scrub = (self.scrub + step).min(total);
        }
        if left {
            self.scrub = self.scrub.saturating_sub(step);
        }
    }

    /// What the viewport should draw this frame.
    fn content(&self) -> Content<'_> {
        let total = self.scene.as_ref().map_or(0, |s| s.segments.len());
        let cutter = self
            .scene
            .as_ref()
            .filter(|_| self.show_cutter && self.scrub < total)
            .and_then(|s| s.at(self.scrub))
            .map(|at| (at.op, at.segment.b));
        let stock = self.stage_shown().and_then(|i| {
            let image = self.stock_images.get(i)?.as_ref()?;
            Some(((self.scene_generation, i), image))
        });
        let part = self.view.part && !(stock.is_some() && self.hide_part_with_stock);
        Content {
            part: self.model.as_ref().map(|m| (self.generation, &m.mesh)),
            framing: self.opened,
            scene: self.scene.as_ref().map(|s| (self.scene_generation, s)),
            stock,
            view: View {
                part,
                roles: if self.show_paths { self.view.roles } else { 0 },
                shown: self.scrub,
                cutter,
                ..self.view
            },
        }
    }

    fn log_panel(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (level, text) in &self.log {
                    let color = match level {
                        Level::Info => ui.visuals().text_color(),
                        Level::Warn => ui.visuals().warn_fg_color,
                        Level::Error => ui.visuals().error_fg_color,
                    };
                    ui.label(egui::RichText::new(text).monospace().color(color));
                }
            });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if let Some(reply) = self.worker.poll() {
            self.accept(reply);
        }
        self.step_scrub(&ctx);
        self.ensure_stock_image();
        self.tools.measure = self.measure();
        self.tools.show(&ctx);

        // The setup toolbar: tools and settings now, work offsets later.
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(self.tools.open, "Tools")
                    .on_hover_text("your tool library")
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
            });
        });
        self.settings.show(&ctx, &mut self.settings_open);
        if let Some((pick, path)) = self.dialog.update(&ctx) {
            match pick {
                Pick::Model => self.open_model(&ctx, path),
                Pick::Job => self.open_job(&ctx, path),
            }
        }

        egui::Panel::left("side")
            .resizable(true)
            .default_size(320.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.side(ui));
            });
        egui::Panel::bottom("log")
            .resizable(true)
            .default_size(150.0)
            .show(ui, |ui| self.log_panel(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                // The viewport is taken out for the call, because the content it
                // draws borrows the rest of the app.
                let mut viewport = std::mem::take(&mut self.viewport);
                viewport.show(ui, frame.wgpu_render_state(), &self.content());
                self.viewport = viewport;
            });

        // Keep the spinner turning while the worker runs; its reply wakes us anyway.
        if self.busy().is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // eframe saves on the way out, so a library edit still in a field is kept.
        self.tools.save_library();
        eframe::set_value(storage, STORAGE_DIALOG, self.dialog.storage());
        eframe::set_value(storage, STORAGE_SETTINGS, &self.settings);
    }
}
