//! Choosing an operation's tool, as in Fusion: from the tools the job already has,
//! or from a folder of the library.
//!
//! A library tool is copied into the job when chosen, never linked, and a copy the
//! job already holds unchanged is used again rather than copied twice.

use camcraft_core::job::{Job, ToolSpec};
use camcraft_session::library::{Folder, FolderPath, folder_label};
use eframe::egui;

use crate::measure::Measure;
use crate::tool_editor::{kind_name, paint_tool};
use crate::worker::DocId;

/// Where the table's tools come from.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    /// The job's own tools.
    Job,
    /// A folder of the library, and the folders inside it.
    Library(FolderPath),
}

/// What was chosen.
#[derive(Debug, Clone, PartialEq)]
pub enum Picked {
    /// One of the job's own tools, by name.
    Job(String),
    /// A library tool: its name there, and what it is.
    Library(String, Box<ToolSpec>),
}

/// The picker, while it is open.
#[derive(Debug, Clone)]
pub struct ToolPicker {
    /// Which job, setup and operation it is choosing for.
    pub target: (DocId, String, usize),
    source: Source,
    filter: String,
    /// The row chosen: its folder (for the library) and name.
    chosen: Option<(Source, FolderPath, String)>,
}

impl ToolPicker {
    /// A picker for operation `op` of `setup` in job `doc`, opening on the job's
    /// tools, with the operation's own chosen, when it has any.
    pub fn new(doc: DocId, setup: String, op: usize, job: &Job) -> Self {
        let current = job
            .setups
            .iter()
            .find(|s| s.name == setup)
            .and_then(|s| s.operations.get(op))
            .and_then(|o| o.tool.clone())
            .filter(|t| job.tools.contains_key(t));
        Self {
            target: (doc, setup, op),
            source: if job.tools.is_empty() {
                Source::Library(Vec::new())
            } else {
                Source::Job
            },
            filter: String::new(),
            chosen: current.map(|t| (Source::Job, Vec::new(), t)),
        }
    }

    /// Draw it. `Some(Some(_))` once a tool is chosen, `Some(None)` if cancelled,
    /// `None` while still open.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        job: &Job,
        library: &Folder,
        m: Measure,
    ) -> Option<Option<Picked>> {
        let mut outcome = None;
        let modal = egui::Modal::new(egui::Id::new("tool-picker")).show(ctx, |ui| {
            // A fixed size throughout. The tool's picture fills the width it is
            // given and a divider the height, and a window sizes itself to what it
            // holds, so anything left free would grow the window every frame.
            ui.set_width(860.0);
            ui.heading("Select tool");
            ui.separator();
            let body = egui::vec2(860.0, 420.0);
            let row = egui::Layout::left_to_right(egui::Align::Min);
            ui.allocate_ui_with_layout(body, row, |ui| {
                ui.set_min_size(body);
                ui.vertical(|ui| {
                    ui.set_width(200.0);
                    egui::ScrollArea::vertical()
                        .id_salt("picker-sources")
                        .max_height(body.y)
                        .show(ui, |ui| self.sources(ui, job, library));
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_width(380.0);
                    ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("filter"));
                    egui::ScrollArea::vertical()
                        .id_salt("picker-tools")
                        .max_height(body.y - 30.0)
                        .show(ui, |ui| self.table(ui, job, library, m));
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_width(240.0);
                    if let Some(tool) = self.chosen_tool(job, library) {
                        paint_tool(ui, tool, m);
                        ui.label(&tool.comment);
                    } else {
                        ui.weak("Choose a tool to see it.");
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                let picked = self.picked(job, library);
                if ui
                    .add_enabled(picked.is_some(), egui::Button::new("Use this tool"))
                    .clicked()
                {
                    outcome = Some(picked);
                }
                if ui.button("Cancel").clicked() {
                    outcome = Some(None);
                }
                if matches!(self.chosen, Some((Source::Library(_), _, _))) {
                    ui.weak("A copy goes into the job; the library is not linked.");
                }
            });
        });
        if outcome.is_none() && modal.should_close() {
            outcome = Some(None);
        }
        outcome
    }

    fn sources(&mut self, ui: &mut egui::Ui, job: &Job, library: &Folder) {
        let text = format!("In this job ({})", job.tools.len());
        if ui
            .selectable_label(self.source == Source::Job, text)
            .clicked()
        {
            self.source = Source::Job;
        }
        ui.add_space(6.0);
        ui.weak("Library");
        for path in library.folder_paths() {
            let count = library.at(&path).map_or(0, Folder::count);
            let name = path.last().map_or("Library", String::as_str);
            ui.horizontal(|ui| {
                // Whole points of indent per level: a few levels at most.
                #[allow(clippy::cast_precision_loss)]
                let indent = path.len() as f32 * 14.0;
                ui.add_space(indent);
                let here = self.source == Source::Library(path.clone());
                if ui
                    .selectable_label(here, format!("{name} ({count})"))
                    .clicked()
                {
                    self.source = Source::Library(path.clone());
                }
            });
        }
    }

    /// The rows showing: folder (below the source), name and tool.
    fn rows<'a>(
        &self,
        job: &'a Job,
        library: &'a Folder,
    ) -> Vec<(FolderPath, &'a String, &'a ToolSpec)> {
        let filter = self.filter.to_lowercase();
        let keep = |n: &String, t: &ToolSpec| {
            filter.is_empty()
                || n.to_lowercase().contains(&filter)
                || t.comment.to_lowercase().contains(&filter)
        };
        let mut rows: Vec<(FolderPath, &String, &ToolSpec)> = match &self.source {
            Source::Job => job
                .tools
                .iter()
                .filter(|(n, t)| keep(n, t))
                .map(|(n, t)| (Vec::new(), n, t))
                .collect(),
            Source::Library(path) => {
                let Some(here) = library.at(path) else {
                    return Vec::new();
                };
                here.folder_paths()
                    .into_iter()
                    .filter_map(|sub| here.at(&sub).map(|f| (sub, f)))
                    .flat_map(|(sub, f)| f.tools.iter().map(move |(n, t)| (sub.clone(), n, t)))
                    .filter(|(_, n, t)| keep(n, t))
                    .collect()
            }
        };
        rows.sort_by_key(|(f, n, t)| (t.number, f.clone(), (*n).clone()));
        rows
    }

    fn table(&mut self, ui: &mut egui::Ui, job: &Job, library: &Folder, m: Measure) {
        let rows = self.rows(job, library);
        if rows.is_empty() {
            ui.weak(match self.source {
                Source::Job => "The job has no tools yet: choose one from the library.",
                Source::Library(_) => "No tools here. Add them in the tool library.",
            });
            return;
        }
        let mut clicked = None;
        egui::Grid::new("picker-tools")
            .striped(true)
            .num_columns(5)
            .show(ui, |ui| {
                ui.strong("T");
                ui.strong("name");
                ui.strong("type");
                ui.strong(format!("Ø {}", m.unit()));
                ui.strong("in");
                ui.end_row();
                for (sub, name, tool) in rows {
                    ui.label(format!("T{}", tool.number));
                    let is = self
                        .chosen
                        .as_ref()
                        .is_some_and(|(s, f, n)| *s == self.source && *f == sub && n == name);
                    let r = ui.selectable_label(is, name.as_str());
                    if r.clicked() {
                        clicked = Some((sub.clone(), name.clone()));
                    }
                    ui.label(kind_name(tool.kind));
                    ui.label(m.number(tool.diameter, 2));
                    ui.weak(if sub.is_empty() {
                        String::new()
                    } else {
                        folder_label(&sub)
                    });
                    ui.end_row();
                }
            });
        if let Some((sub, name)) = clicked {
            self.chosen = Some((self.source.clone(), sub, name));
        }
    }

    fn chosen_tool<'a>(&self, job: &'a Job, library: &'a Folder) -> Option<&'a ToolSpec> {
        let (source, sub, name) = self.chosen.as_ref()?;
        match source {
            Source::Job => job.tools.get(name),
            Source::Library(path) => {
                let mut at = path.clone();
                at.extend(sub.iter().cloned());
                library.at(&at)?.tools.get(name)
            }
        }
    }

    fn picked(&self, job: &Job, library: &Folder) -> Option<Picked> {
        let (source, _, name) = self.chosen.as_ref()?;
        let tool = self.chosen_tool(job, library)?;
        Some(match source {
            Source::Job => Picked::Job(name.clone()),
            Source::Library(_) => Picked::Library(name.clone(), Box::new(tool.clone())),
        })
    }
}
