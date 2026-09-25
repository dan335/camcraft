//! Choosing an operation's tool, as in Fusion: from the tools the job already has,
//! or from a folder of the library.
//!
//! A library tool is copied into the job when chosen, never linked, and a copy the
//! job already holds unchanged is used again rather than copied twice. One of the
//! tool's presets is chosen with it, its default to begin with, and its values are
//! copied into the operation.

use camcraft_core::job::{Job, ToolSpec};
use camcraft_session::library::{Folder, FolderPath, folder_label, tool_matches};
use eframe::egui;

use crate::tools::drawing::{kind_name, paint_tool};
use crate::tools::table::{column_header, dim, name_cell, number, number_cell, tool_table};
use crate::ui::measure::Measure;
use crate::ui::theme;
use crate::ui::widgets::hint_text;
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

/// What the picker has come to.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Nothing yet: it is still open.
    Open,
    /// Closed without choosing.
    Cancelled,
    /// A tool, and which of its presets to copy into the operation, if any.
    Chosen {
        /// The tool.
        tool: Picked,
        /// Its preset, by name.
        preset: Option<String>,
    },
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
    /// The chosen tool's preset to apply, by name; none keeps the operation's values.
    preset: Option<String>,
}

impl ToolPicker {
    /// A picker for operation `op` of `setup` in job `doc`, opening on the job's
    /// tools, with the operation's own chosen, when it has any.
    pub fn new(doc: DocId, setup: String, op: usize, job: &Job) -> Self {
        let operation = job
            .setups
            .iter()
            .find(|s| s.name == setup)
            .and_then(|s| s.operations.get(op));
        let current = operation
            .and_then(|o| o.tool.clone())
            .filter(|t| job.tools.contains_key(t));
        let preset = operation.and_then(|o| o.preset.clone());
        Self {
            target: (doc, setup, op),
            source: if job.tools.is_empty() {
                Source::Library(Vec::new())
            } else {
                Source::Job
            },
            filter: String::new(),
            chosen: current.map(|t| (Source::Job, Vec::new(), t)),
            preset,
        }
    }

    /// Draw it, and say what it has come to.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        job: &Job,
        library: &Folder,
        m: Measure,
    ) -> Outcome {
        let mut outcome = Outcome::Open;
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
                    self.table(ui, job, library, m, body.y - 30.0);
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_width(240.0);
                    if let Some(tool) = self.chosen_tool(job, library) {
                        paint_tool(ui, tool, m);
                        ui.label(&tool.comment);
                        ui.add_space(6.0);
                        self.preset_choice(ui, tool);
                    } else {
                        ui.label(hint_text("Choose a tool to see it."));
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                let picked = self.picked(job, library);
                if ui
                    .add_enabled(picked.is_some(), egui::Button::new("Use this tool"))
                    .clicked()
                    && let Some(tool) = picked
                {
                    outcome = Outcome::Chosen {
                        tool,
                        preset: self.preset.clone(),
                    };
                }
                if ui.button("Cancel").clicked() {
                    outcome = Outcome::Cancelled;
                }
                if matches!(self.chosen, Some((Source::Library(_), _, _))) {
                    ui.label(hint_text(
                        "A copy goes into the job; the library is not linked.",
                    ));
                }
            });
        });
        if outcome == Outcome::Open && modal.should_close() {
            outcome = Outcome::Cancelled;
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
        ui.label(hint_text("Library"));
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
    ) -> Vec<(FolderPath, &'a str, &'a ToolSpec)> {
        let mut rows: Vec<(FolderPath, &str, &ToolSpec)> = match &self.source {
            Source::Job => job
                .tools
                .iter()
                .filter(|(n, t)| tool_matches(n, t, &self.filter))
                .map(|(n, t)| (Vec::new(), n.as_str(), t))
                .collect(),
            Source::Library(path) => {
                let Some(here) = library.at(path) else {
                    return Vec::new();
                };
                here.find(true, &self.filter)
            }
        };
        rows.sort_by_key(|(f, n, t)| (t.number, f.clone(), *n));
        rows
    }

    /// The tools of the source showing, as the tool library lists them, in a
    /// table at most `height` tall. Clicking one chooses it.
    fn table(&mut self, ui: &mut egui::Ui, job: &Job, library: &Folder, m: Measure, height: f32) {
        let rows = self.rows(job, library);
        if rows.is_empty() {
            ui.label(hint_text(match self.source {
                Source::Job => "The job has no tools yet: choose one from the library.",
                Source::Library(_) => "No tools here. Add them in the tool library.",
            }));
            return;
        }
        let mut clicked = None;
        let right = egui::Layout::right_to_left(egui::Align::Center);
        tool_table(ui, "picker-tools", egui::Sense::click())
            .max_scroll_height(height)
            .column(egui_extras::Column::exact(44.0))
            .column(
                egui_extras::Column::initial(132.0)
                    .at_least(72.0)
                    .clip(true),
            )
            .column(egui_extras::Column::initial(84.0).at_least(56.0).clip(true))
            .column(egui_extras::Column::exact(56.0))
            .column(egui_extras::Column::remainder().at_least(40.0).clip(true))
            .header(theme::CONTROL_MD, |mut header| {
                header.col(|ui| {
                    ui.add_space(theme::SPACE_3);
                    column_header(ui, "T", None, None, false);
                });
                header.col(|ui| {
                    column_header(ui, "Name", None, None, false);
                });
                header.col(|ui| {
                    column_header(ui, "Type", None, None, false);
                });
                header.col(|ui| {
                    column_header(ui, "Ø", Some(m.unit()), None, true);
                });
                header.col(|ui| {
                    column_header(ui, "In", None, None, false);
                });
            })
            .body(|mut body| {
                for (sub, name, tool) in rows {
                    let is = self
                        .chosen
                        .as_ref()
                        .is_some_and(|(s, f, n)| *s == self.source && *f == sub && n == name);
                    body.row(theme::CONTROL_MD, |mut row| {
                        row.set_selected(is);
                        row.col(|ui| number_cell(ui, tool.number, is));
                        row.col(|ui| name_cell(ui, name, is));
                        row.col(|ui| {
                            ui.add(dim(kind_name(tool.kind).to_string()).selectable(false));
                        });
                        row.col(|ui| {
                            ui.with_layout(right, |ui| ui.add(number(m.number(tool.diameter, 2))));
                        });
                        row.col(|ui| {
                            let folder = if sub.is_empty() {
                                String::new()
                            } else {
                                folder_label(&sub)
                            };
                            ui.add(dim(folder).selectable(false));
                        });
                        if row.response().clicked() {
                            clicked = Some((sub.clone(), name.to_string()));
                        }
                    });
                }
            });
        if let Some((sub, name)) = clicked {
            self.chosen = Some((self.source.clone(), sub, name));
            self.preset = self
                .chosen_tool(job, library)
                .and_then(|t| t.presets.first())
                .map(|p| p.name.clone());
        }
    }

    /// Which of the tool's presets to copy into the operation.
    fn preset_choice(&mut self, ui: &mut egui::Ui, tool: &ToolSpec) {
        if self
            .preset
            .as_ref()
            .is_some_and(|name| tool.preset(name).is_none())
        {
            self.preset = None;
        }
        if tool.presets.is_empty() {
            ui.label(hint_text(
                "This tool has no presets; the operation keeps its speeds and feeds.",
            ));
            return;
        }
        ui.label("Preset");
        egui::ComboBox::from_id_salt("picker-preset")
            .width(220.0)
            .selected_text(
                self.preset
                    .as_deref()
                    .unwrap_or("none: keep the operation's"),
            )
            .show_ui(ui, |ui| {
                for p in &tool.presets {
                    ui.selectable_value(&mut self.preset, Some(p.name.clone()), &p.name);
                }
                ui.selectable_value(&mut self.preset, None, "none: keep the operation's");
            });
        if let Some(p) = self.preset.as_ref().and_then(|n| tool.preset(n)) {
            if !p.comment.is_empty() {
                ui.label(hint_text(&p.comment));
            }
            ui.label(hint_text("Its values are copied into the operation."));
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
