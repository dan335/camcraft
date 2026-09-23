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
use crate::viewport::Viewport;
use crate::worker::{Outcome, Reply, Request, Worker};

/// Tessellation tolerance for looking at a model before any job is open. A run
/// always reloads the model at its job's own tolerance.
const PREVIEW_TOLERANCE: f64 = 0.05;

const STORAGE_DIALOG: &str = "file_dialog";

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
    log: Vec<(Level, String)>,
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
        let storage = cc
            .storage
            .and_then(|s| eframe::get_value(s, STORAGE_DIALOG));
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
            log: Vec::new(),
        }
    }

    fn note(&mut self, level: Level, text: impl Into<String>) {
        self.log.push((level, text.into()));
    }

    /// Open a model file, as if picked.
    pub fn open_model(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.note(Level::Info, format!("model: {}", path.display()));
        self.model = None;
        self.outcome = None;
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
        self.outcome = None;
        match read_job(&path) {
            Ok(job) => {
                self.note(
                    Level::Info,
                    format!(
                        "job: {} ({} operation(s), tolerance {} mm)",
                        path.display(),
                        job.operations.len(),
                        job.tolerance
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
                    self.outcome = None;
                    self.load(&ctx);
                }
            }
        }
        if let Some(model) = &self.model {
            let e = model.bounds.extent();
            egui::Grid::new("model").num_columns(2).show(ui, |ui| {
                ui.label("units");
                ui.label(&model.units);
                ui.end_row();
                ui.label("size");
                ui.label(format!("{:.1} × {:.1} × {:.1} mm", e.x, e.y, e.z));
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
            ui.label(format!("tolerance {} mm", job.tolerance));
            for (i, op) in job.operations.iter().enumerate() {
                let strategy = match op.strategy {
                    StrategySpec::Parallel { .. } => "parallel",
                    StrategySpec::ZlevelRough { .. } => "z-level rough",
                };
                let tool = job.tool_for(op).map_or_else(String::new, |t| {
                    let kind = format!("{:?}", t.kind).to_lowercase();
                    format!(", {kind} Ø{}", t.diameter)
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
                        ui.label(format!("{:.1} cm³", op.sim.removed_mm3 / 1000.0));
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
                        "NOT verified: {} gouged column(s), deepest {:.3} mm.",
                        v.gouged, v.deepest_gouge_mm
                    ),
                );
            }
            for w in &outcome.ran.warnings {
                ui.colored_label(ui.visuals().warn_fg_color, w);
            }
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
                let model = self.model.as_ref().map(|m| (self.generation, &m.mesh));
                self.viewport.show(ui, frame.wgpu_render_state(), model);
            });

        // Keep the spinner turning while the worker runs; its reply wakes us anyway.
        if self.busy().is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_DIALOG, self.dialog.storage());
    }
}
