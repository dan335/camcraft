//! The tool manager: the job's tools and the user's library, side by side, after
//! Fusion's Tool Library.
//!
//! It opens in its own OS window. Tools are edited in place and saved per source.
//! The job's are written back into its file with the rest of the file left as it
//! was (`camcraft_session::job_file`); the library's go to the user's app-data
//! folder (`camcraft_session::library`). Copying between the two is literal, and a
//! job keeps copies, never links, so it still cuts what was verified after the
//! library changes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use camcraft_core::job::{
    CuttingData, Holder, HolderSegment, Job, ToolFieldError, ToolKind, ToolMaterial, ToolSpec,
};
use camcraft_session::job_file;
use camcraft_session::library::{Library, free_name, library_path};
use eframe::egui;

use crate::theme;
use crate::tool_drawing::{Part, draw};

/// Which set of tools is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The open job's own tools.
    Job,
    /// The user's library.
    Library,
}

/// What the app should do after a frame of the manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The job file was saved; read it again.
    JobSaved(PathBuf),
}

/// One source's tools: as saved, as edited, and the renames between.
#[derive(Debug, Clone, Default)]
struct Set {
    original: BTreeMap<String, ToolSpec>,
    edited: BTreeMap<String, ToolSpec>,
    /// `(from, to)` for tools that existed when last saved.
    renames: Vec<(String, String)>,
}

impl Set {
    fn new(tools: BTreeMap<String, ToolSpec>) -> Self {
        Self {
            original: tools.clone(),
            edited: tools,
            renames: Vec::new(),
        }
    }

    fn dirty(&self) -> bool {
        self.edited != self.original || !self.renames.is_empty()
    }

    fn revert(&mut self) {
        self.edited = self.original.clone();
        self.renames.clear();
    }

    fn saved(&mut self) {
        self.original = self.edited.clone();
        self.renames.clear();
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), String> {
        let to = to.trim();
        if to.is_empty() {
            return Err("a tool needs a name".into());
        }
        if to == from {
            return Ok(());
        }
        if self.edited.contains_key(to) {
            return Err(format!("there is already a tool called '{to}'"));
        }
        let tool = self
            .edited
            .remove(from)
            .ok_or_else(|| format!("no tool called '{from}'"))?;
        self.edited.insert(to.to_string(), tool);
        // A chain of renames is one rename from the saved name.
        if let Some(r) = self.renames.iter_mut().find(|(_, t)| t == from) {
            r.1 = to.to_string();
        } else if self.original.contains_key(from) {
            self.renames.push((from.to_string(), to.to_string()));
        }
        self.renames.retain(|(a, b)| a != b);
        Ok(())
    }

    /// A tool number no tool here uses yet.
    fn next_number(&self) -> u32 {
        self.edited.values().map(|t| t.number).max().unwrap_or(0) + 1
    }
}

struct JobSide {
    path: PathBuf,
    /// Operations using each tool, by the tool's current name.
    users: BTreeMap<String, Vec<String>>,
    set: Set,
}

/// How the table is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sort {
    Number,
    Name,
    Diameter,
}

/// The manager's state.
pub struct ToolManager {
    /// Whether its window is showing.
    pub open: bool,
    source: Source,
    job: Option<JobSide>,
    library_path: Option<PathBuf>,
    library: Set,
    library_error: Option<String>,
    selected: Option<String>,
    name_buffer: String,
    filter: String,
    sort: Sort,
    message: Option<(bool, String)>,
    /// Closing was asked for with unsaved changes.
    closing: bool,
}

impl ToolManager {
    /// A manager for the user's own library.
    pub fn new() -> Self {
        match library_path() {
            Ok(path) => Self::with_library(path),
            Err(e) => {
                let mut m = Self::with_library(PathBuf::new());
                m.library_path = None;
                m.library_error = Some(e.to_string());
                m
            }
        }
    }

    /// A manager for the library at `path`.
    pub fn with_library(path: PathBuf) -> Self {
        let (library, library_error) = match Library::load(&path) {
            Ok(lib) => (lib, None),
            Err(e) => (Library::default(), Some(e.to_string())),
        };
        Self {
            open: false,
            source: Source::Library,
            job: None,
            library_path: Some(path),
            library: Set::new(library.tools),
            library_error,
            selected: None,
            name_buffer: String::new(),
            filter: String::new(),
            sort: Sort::Number,
            message: None,
            closing: false,
        }
    }

    /// Show the job at `path`, unless its tools have unsaved edits.
    pub fn set_job(&mut self, path: &Path, job: &Job) {
        if self
            .job
            .as_ref()
            .is_some_and(|j| j.path == path && j.set.dirty())
        {
            return;
        }
        let mut users: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for op in &job.operations {
            users
                .entry(op.tool.clone())
                .or_default()
                .push(op.name.clone());
        }
        self.job = Some(JobSide {
            path: path.to_path_buf(),
            users,
            set: Set::new(job.tools.clone()),
        });
        self.source = Source::Job;
        self.select(None);
    }

    /// Show `source` with `name` selected, after `edit` is applied to that tool.
    /// Only the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn show_for_snapshot(&mut self, source: Source, name: &str, tool: Option<ToolSpec>) {
        self.source = source;
        if let (Some(tool), Some(set)) = (tool, self.set_mut()) {
            set.edited.insert(name.to_string(), tool);
        }
        self.select(Some(name.to_string()));
    }

    /// Whether anything is unsaved.
    pub fn dirty(&self) -> bool {
        self.library.dirty() || self.job.as_ref().is_some_and(|j| j.set.dirty())
    }

    fn set(&self) -> Option<&Set> {
        match self.source {
            Source::Job => self.job.as_ref().map(|j| &j.set),
            Source::Library => Some(&self.library),
        }
    }

    fn set_mut(&mut self) -> Option<&mut Set> {
        match self.source {
            Source::Job => self.job.as_mut().map(|j| &mut j.set),
            Source::Library => Some(&mut self.library),
        }
    }

    fn select(&mut self, name: Option<String>) {
        self.name_buffer = name.clone().unwrap_or_default();
        self.selected = name;
    }

    /// Show the window, if open. Returns what the app should do.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Event> {
        if !self.open {
            return None;
        }
        let mut event = None;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("camcraft-tool-manager"),
            egui::ViewportBuilder::default()
                .with_title("camcraft — tools")
                .with_inner_size([1180.0, 720.0])
                .with_min_inner_size([860.0, 480.0]),
            |ui, _class| {
                if ui.ctx().input(|i| i.viewport().close_requested()) {
                    if self.dirty() {
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                        self.closing = true;
                    } else {
                        self.open = false;
                    }
                }
                event = self.ui(ui);
            },
        );
        event
    }

    /// The whole manager, in `ui`.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<Event> {
        let mut event = None;
        egui::Panel::top("tools-top").show(ui, |ui| {
            event = self.top_bar(ui);
        });
        egui::Panel::right("tools-editor")
            .resizable(true)
            .default_size(420.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.editor(ui));
            });
        egui::CentralPanel::default().show(ui, |ui| self.table(ui));
        event
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) -> Option<Event> {
        let mut event = None;
        if self.closing {
            ui.horizontal(|ui| {
                ui.colored_label(ui.visuals().warn_fg_color, "There are unsaved changes.");
                if ui.button("Discard and close").clicked() {
                    self.library.revert();
                    if let Some(j) = self.job.as_mut() {
                        j.set.revert();
                    }
                    self.closing = false;
                    self.open = false;
                }
                if ui.button("Keep editing").clicked() {
                    self.closing = false;
                }
            });
            ui.separator();
        }
        ui.horizontal(|ui| {
            let job_label = self.job.as_ref().map_or_else(
                || "This job (none open)".to_string(),
                |j| {
                    let name = j
                        .path
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    format!("This job: {name} ({})", j.set.edited.len())
                },
            );
            let job_dirty = self.job.as_ref().is_some_and(|j| j.set.dirty());
            ui.add_enabled_ui(self.job.is_some(), |ui| {
                let text = if job_dirty {
                    format!("{job_label} •")
                } else {
                    job_label
                };
                if ui
                    .selectable_label(self.source == Source::Job, text)
                    .clicked()
                {
                    self.source = Source::Job;
                    self.select(None);
                }
            });
            let lib_text = format!(
                "Library ({}){}",
                self.library.edited.len(),
                if self.library.dirty() { " •" } else { "" }
            );
            if ui
                .selectable_label(self.source == Source::Library, lib_text)
                .clicked()
            {
                self.source = Source::Library;
                self.select(None);
            }
            ui.separator();
            let dirty = self.set().is_some_and(Set::dirty);
            if ui.add_enabled(dirty, egui::Button::new("Save")).clicked() {
                event = self.save();
            }
            if ui.add_enabled(dirty, egui::Button::new("Revert")).clicked()
                && let Some(set) = self.set_mut()
            {
                set.revert();
                self.select(None);
            }
            if let Some((ok, text)) = &self.message {
                let color = if *ok {
                    ui.visuals().text_color()
                } else {
                    ui.visuals().error_fg_color
                };
                ui.colored_label(color, text);
            }
        });
        if self.source == Source::Library {
            if let Some(e) = &self.library_error {
                ui.colored_label(ui.visuals().error_fg_color, e);
            } else if let Some(p) = &self.library_path {
                ui.weak(format!("kept in {}", p.display()));
            }
        }
        event
    }

    fn save(&mut self) -> Option<Event> {
        let problems: Vec<String> = self
            .set()
            .map(|s| {
                s.edited
                    .iter()
                    .flat_map(|(n, t)| t.check().into_iter().map(move |e| format!("{n}: {e}")))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(first) = problems.first() {
            self.message = Some((false, format!("not saved: {first}")));
            return None;
        }
        match self.source {
            Source::Job => {
                let j = self.job.as_mut()?;
                match job_file::save_tools(&j.path, &j.set.original, &j.set.edited, &j.set.renames)
                {
                    Ok(()) => {
                        j.set.saved();
                        self.message = Some((true, "saved to the job file".into()));
                        Some(Event::JobSaved(j.path.clone()))
                    }
                    Err(e) => {
                        self.message = Some((false, format!("not saved: {e}")));
                        None
                    }
                }
            }
            Source::Library => {
                let Some(path) = self.library_path.clone() else {
                    self.message = Some((false, "there is nowhere to keep the library".into()));
                    return None;
                };
                let lib = Library {
                    tools: self.library.edited.clone(),
                };
                match lib.save(&path) {
                    Ok(()) => {
                        self.library.saved();
                        self.message = Some((true, "library saved".into()));
                    }
                    Err(e) => self.message = Some((false, format!("not saved: {e}"))),
                }
                None
            }
        }
    }

    fn table(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("filter"));
            let has_set = self.set().is_some();
            if ui.add_enabled(has_set, egui::Button::new("New")).clicked() {
                self.new_tool();
            }
            let selected = self.selected.clone();
            if ui
                .add_enabled(selected.is_some(), egui::Button::new("Duplicate"))
                .clicked()
                && let Some(name) = &selected
            {
                self.duplicate(name);
            }
            if ui
                .add_enabled(selected.is_some(), egui::Button::new("Delete"))
                .clicked()
                && let Some(name) = &selected
            {
                self.delete(name);
            }
            let (label, target) = match self.source {
                Source::Job => ("Copy to library", Source::Library),
                Source::Library => ("Copy to job", Source::Job),
            };
            let can_copy = selected.is_some() && (target == Source::Library || self.job.is_some());
            if ui.add_enabled(can_copy, egui::Button::new(label)).clicked()
                && let Some(name) = &selected
            {
                self.copy(name, target);
            }
        });
        ui.separator();

        let Some(set) = self.set() else {
            ui.weak("Open a job to see its tools.");
            return;
        };
        let filter = self.filter.to_lowercase();
        let mut rows: Vec<(&String, &ToolSpec)> = set
            .edited
            .iter()
            .filter(|(n, t)| {
                filter.is_empty()
                    || n.to_lowercase().contains(&filter)
                    || t.comment.to_lowercase().contains(&filter)
            })
            .collect();
        match self.sort {
            Sort::Number => rows.sort_by_key(|(n, t)| (t.number, (*n).clone())),
            Sort::Name => {}
            Sort::Diameter => rows.sort_by(|a, b| a.1.diameter.total_cmp(&b.1.diameter)),
        }
        let users = self
            .job
            .as_ref()
            .map(|j| j.users.clone())
            .unwrap_or_default();
        let source = self.source;
        let mut clicked = None;
        let mut sort = self.sort;
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("tools-table")
                .striped(true)
                .num_columns(7)
                .min_col_width(40.0)
                .show(ui, |ui| {
                    for (label, key) in [("T", Some(Sort::Number)), ("name", Some(Sort::Name))] {
                        header(ui, label, key, &mut sort);
                    }
                    ui.strong("type");
                    header(ui, "Ø", Some(Sort::Diameter), &mut sort);
                    ui.strong("flutes");
                    ui.strong("stickout");
                    ui.strong("description");
                    ui.end_row();
                    for (name, tool) in rows {
                        ui.label(format!("T{}", tool.number));
                        let is = self.selected.as_deref() == Some(name.as_str());
                        let mut text = egui::RichText::new(name.as_str());
                        if source == Source::Job && users.contains_key(name) {
                            text = text.strong();
                        }
                        if ui.selectable_label(is, text).clicked() {
                            clicked = Some(name.clone());
                        }
                        ui.label(kind_name(tool.kind));
                        ui.label(format!("{:.2}", tool.diameter));
                        ui.label(tool.flutes.map_or_else(|| "–".into(), |f| f.to_string()));
                        ui.label(
                            tool.stickout
                                .map_or_else(|| "–".into(), |s| format!("{s:.1}")),
                        );
                        ui.label(&tool.comment);
                        ui.end_row();
                    }
                });
            if source == Source::Job {
                ui.add_space(6.0);
                ui.weak("Names in bold are used by the job's operations.");
            }
        });
        self.sort = sort;
        if let Some(name) = clicked {
            self.select(Some(name));
        }
    }

    fn new_tool(&mut self) {
        let Some(set) = self.set_mut() else {
            return;
        };
        let name = free_name("tool", &set.edited);
        let tool = ToolSpec {
            number: set.next_number(),
            ..ToolSpec::default()
        };
        set.edited.insert(name.clone(), tool);
        self.select(Some(name));
    }

    fn duplicate(&mut self, name: &str) {
        let Some(set) = self.set_mut() else {
            return;
        };
        let Some(tool) = set.edited.get(name).cloned() else {
            return;
        };
        let copy = free_name(&format!("{name}-copy"), &set.edited);
        let number = set.next_number();
        set.edited.insert(copy.clone(), ToolSpec { number, ..tool });
        self.select(Some(copy));
    }

    fn delete(&mut self, name: &str) {
        if self.source == Source::Job
            && let Some(ops) = self.job.as_ref().and_then(|j| j.users.get(name))
        {
            self.message = Some((
                false,
                format!(
                    "'{name}' is used by {}; change the operation first",
                    ops.join(", ")
                ),
            ));
            return;
        }
        if let Some(set) = self.set_mut() {
            set.edited.remove(name);
        }
        self.select(None);
    }

    fn copy(&mut self, name: &str, target: Source) {
        let Some(tool) = self.set().and_then(|s| s.edited.get(name)).cloned() else {
            return;
        };
        let into = match target {
            Source::Job => self.job.as_mut().map(|j| &mut j.set),
            Source::Library => Some(&mut self.library),
        };
        let Some(set) = into else {
            return;
        };
        let as_name = free_name(name, &set.edited);
        set.edited.insert(as_name.clone(), tool);
        let place = match target {
            Source::Job => "the job",
            Source::Library => "the library",
        };
        self.message = Some((
            true,
            format!("copied to {place} as '{as_name}'; save it there to keep it"),
        ));
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let Some(name) = self.selected.clone() else {
            ui.weak("Select a tool to edit it.");
            return;
        };
        let Some(original) = self.set().and_then(|s| s.edited.get(&name)).cloned() else {
            self.select(None);
            return;
        };
        let mut tool = original.clone();
        let errors = tool.check();

        // The name, applied when editing finishes.
        ui.horizontal(|ui| {
            ui.label("Name");
            let r = ui.text_edit_singleline(&mut self.name_buffer);
            if r.lost_focus() && self.name_buffer != name {
                let new = self.name_buffer.clone();
                let result = self.set_mut().map(|s| s.rename(&name, &new));
                match result {
                    Some(Ok(())) => {
                        if let Some(j) = self.job.as_mut().filter(|_| self.source == Source::Job)
                            && let Some(ops) = j.users.remove(&name)
                        {
                            j.users.insert(new.trim().to_string(), ops);
                        }
                        self.selected = Some(new.trim().to_string());
                        self.name_buffer = new.trim().to_string();
                    }
                    Some(Err(e)) => {
                        self.message = Some((false, e));
                        self.name_buffer = name.clone();
                    }
                    None => {}
                }
            }
        });
        if self.source == Source::Job
            && let Some(ops) = self.job.as_ref().and_then(|j| j.users.get(&name))
        {
            ui.weak(format!("used by {}", ops.join(", ")));
        }

        paint_tool(ui, &tool);

        let section = |ui: &mut egui::Ui, title: &str, body: &mut dyn FnMut(&mut egui::Ui)| {
            egui::CollapsingHeader::new(title)
                .default_open(true)
                .show(ui, |ui| {
                    egui::Grid::new(title)
                        .num_columns(2)
                        .spacing([12.0, 6.0])
                        .show(ui, |ui| body(ui));
                });
        };

        section(ui, "General", &mut |ui| {
            ui.label("Description");
            ui.text_edit_singleline(&mut tool.comment);
            ui.end_row();
            opt_text(ui, "Vendor", &mut tool.vendor);
            opt_text(ui, "Product", &mut tool.product);
            ui.label("Material");
            egui::ComboBox::from_id_salt("material")
                .selected_text(match tool.material {
                    None => "not given",
                    Some(ToolMaterial::Carbide) => "carbide",
                    Some(ToolMaterial::Hss) => "HSS",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut tool.material, None, "not given");
                    ui.selectable_value(&mut tool.material, Some(ToolMaterial::Carbide), "carbide");
                    ui.selectable_value(&mut tool.material, Some(ToolMaterial::Hss), "HSS");
                });
            ui.end_row();
        });

        section(ui, "Cutter", &mut |ui| {
            ui.label("Type");
            egui::ComboBox::from_id_salt("kind")
                .selected_text(kind_name(tool.kind))
                .show_ui(ui, |ui| {
                    for k in [ToolKind::Flat, ToolKind::Ball, ToolKind::Bullnose] {
                        ui.selectable_value(&mut tool.kind, k, kind_name(k));
                    }
                });
            ui.end_row();
            length(ui, "Diameter", &mut tool.diameter, &errors, "diameter");
            if tool.kind == ToolKind::Bullnose {
                length(ui, "Corner radius", &mut tool.corner, &errors, "corner");
            }
            ui.label("Flutes");
            ui.horizontal(|ui| {
                let mut on = tool.flutes.is_some();
                if ui.checkbox(&mut on, "").changed() {
                    tool.flutes = on.then_some(2);
                }
                if let Some(f) = tool.flutes.as_mut() {
                    ui.add(egui::DragValue::new(f).range(0..=16));
                } else {
                    ui.weak("not given");
                }
                field_error(ui, &errors, "flutes");
            });
            ui.end_row();
            opt_length(
                ui,
                "Flute length",
                &mut tool.flute_length,
                2.0 * tool.diameter,
                &errors,
                "flute_length",
            );
            opt_length(
                ui,
                "Shoulder length",
                &mut tool.shoulder_length,
                2.5 * tool.diameter,
                &errors,
                "shoulder_length",
            );
        });

        section(ui, "Shank & holder", &mut |ui| {
            opt_length(
                ui,
                "Shank diameter",
                &mut tool.shank_diameter,
                tool.diameter,
                &errors,
                "shank_diameter",
            );
            opt_length(
                ui,
                "Stickout",
                &mut tool.stickout,
                4.0 * tool.diameter,
                &errors,
                "stickout",
            );
            opt_length(
                ui,
                "Overall length",
                &mut tool.overall_length,
                8.0 * tool.diameter,
                &errors,
                "overall_length",
            );
            ui.label("Holder");
            ui.vertical(|ui| {
                let mut on = tool.holder.is_some();
                if ui.checkbox(&mut on, "has a holder").changed() {
                    tool.holder = on.then(|| Holder {
                        name: String::new(),
                        segments: vec![HolderSegment {
                            diameter: 32.0,
                            length: 20.0,
                        }],
                    });
                }
                if let Some(h) = tool.holder.as_mut() {
                    ui.add(egui::TextEdit::singleline(&mut h.name).hint_text("name, e.g. ER20"));
                    let mut remove = None;
                    for (i, seg) in h.segments.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.weak(format!("{}", i + 1));
                            ui.add(
                                egui::DragValue::new(&mut seg.diameter)
                                    .speed(0.1)
                                    .prefix("Ø ")
                                    .suffix(" mm"),
                            );
                            ui.add(
                                egui::DragValue::new(&mut seg.length)
                                    .speed(0.1)
                                    .prefix("L ")
                                    .suffix(" mm"),
                            );
                            if ui.small_button("remove").clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        h.segments.remove(i);
                    }
                    if ui.small_button("add segment").clicked() {
                        h.segments.push(HolderSegment {
                            diameter: 32.0,
                            length: 10.0,
                        });
                    }
                    field_error(ui, &errors, "holder");
                }
            });
            ui.end_row();
        });

        section(ui, "Cutting data", &mut |ui| {
            ui.label("Spindle");
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut tool.rpm)
                        .range(0..=60000)
                        .suffix(" rev/min"),
                );
                field_error(ui, &errors, "rpm");
            });
            ui.end_row();
            let cutting = tool.cutting.get_or_insert_with(CuttingData::default);
            opt_feed(
                ui,
                "Cutting feed",
                &mut cutting.feed,
                &errors,
                "cutting.feed",
            );
            opt_feed(
                ui,
                "Plunge feed",
                &mut cutting.plunge_feed,
                &errors,
                "cutting.plunge_feed",
            );
            opt_feed(
                ui,
                "Ramp feed",
                &mut cutting.ramp_feed,
                &errors,
                "cutting.ramp_feed",
            );
            if *cutting == CuttingData::default() {
                tool.cutting = None;
            }
        });

        section(ui, "Post", &mut |ui| {
            ui.label("Tool number");
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut tool.number)
                        .range(1..=9999)
                        .prefix("T"),
                );
            });
            ui.end_row();
        });
        // A clash only matters between tools the job's operations use.
        if self.source == Source::Job
            && let Some(j) = &self.job
            && j.users.contains_key(&name)
        {
            let clash: Vec<&String> = j
                .set
                .edited
                .iter()
                .filter(|(n, t)| **n != name && t.number == tool.number && j.users.contains_key(*n))
                .map(|(n, _)| n)
                .collect();
            if let Some(other) = clash.first() {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!(
                        "'{other}' is also T{}: the machine would load one and cut with the other's program",
                        tool.number
                    ),
                );
            }
        }

        if tool != original
            && let Some(set) = self.set_mut()
            && let Some(slot) = set.edited.get_mut(&name)
        {
            *slot = tool;
        }
    }
}

fn header(ui: &mut egui::Ui, label: &str, key: Option<Sort>, sort: &mut Sort) {
    match key {
        Some(k) => {
            // The sorted column is underlined; the default font has no arrows.
            let mut text = egui::RichText::new(label).strong();
            if *sort == k {
                text = text.underline();
            }
            if ui
                .add(egui::Button::new(text).frame(false))
                .on_hover_text("sort by this")
                .clicked()
            {
                *sort = k;
            }
        }
        None => {
            ui.strong(label);
        }
    }
}

fn kind_name(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Flat => "flat",
        ToolKind::Ball => "ball",
        ToolKind::Bullnose => "bullnose",
    }
}

fn field_error(ui: &mut egui::Ui, errors: &[ToolFieldError], field: &str) {
    for e in errors.iter().filter(|e| e.field == field) {
        ui.colored_label(ui.visuals().error_fg_color, &e.why);
    }
}

fn length(ui: &mut egui::Ui, label: &str, value: &mut f64, errors: &[ToolFieldError], field: &str) {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.add(
            egui::DragValue::new(value)
                .speed(0.05)
                .range(0.0..=1000.0)
                .suffix(" mm"),
        );
        field_error(ui, errors, field);
    });
    ui.end_row();
}

fn opt_length(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut Option<f64>,
    default: f64,
    errors: &[ToolFieldError],
    field: &str,
) {
    ui.label(label);
    ui.horizontal(|ui| {
        let mut on = value.is_some();
        if ui.checkbox(&mut on, "").changed() {
            *value = on.then_some(default);
        }
        if let Some(v) = value.as_mut() {
            ui.add(
                egui::DragValue::new(v)
                    .speed(0.1)
                    .range(0.0..=1000.0)
                    .suffix(" mm"),
            );
        } else {
            ui.weak("not given");
        }
        field_error(ui, errors, field);
    });
    ui.end_row();
}

fn opt_feed(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut Option<f64>,
    errors: &[ToolFieldError],
    field: &str,
) {
    ui.label(label);
    ui.horizontal(|ui| {
        let mut on = value.is_some();
        if ui.checkbox(&mut on, "").changed() {
            *value = on.then_some(1000.0);
        }
        if let Some(v) = value.as_mut() {
            ui.add(
                egui::DragValue::new(v)
                    .speed(10.0)
                    .range(0.0..=50000.0)
                    .suffix(" mm/min"),
            );
        } else {
            ui.weak("not given");
        }
        field_error(ui, errors, field);
    });
    ui.end_row();
}

fn opt_text(ui: &mut egui::Ui, label: &str, value: &mut Option<String>) {
    ui.label(label);
    let mut text = value.clone().unwrap_or_default();
    if ui.text_edit_singleline(&mut text).changed() {
        *value = (!text.trim().is_empty()).then_some(text);
    }
    ui.end_row();
}

/// The tool, side on and to scale, with its holder face marked.
fn paint_tool(ui: &mut egui::Ui, tool: &ToolSpec) {
    let d = draw(tool);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 220.0),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, theme::egui_color(theme::BG));
    let margin = 16.0_f64;
    let w = f64::from(rect.width()) - 2.0 * margin;
    let h = f64::from(rect.height()) - 2.0 * margin;
    let scale = (w / (2.0 * d.max_radius.max(1e-6))).min(h / d.height.max(1e-6));
    let base = rect.center_bottom() - egui::vec2(0.0, 16.0);
    // Drawing coordinates are millimetres at most a few hundred across.
    #[allow(clippy::cast_possible_truncation)]
    let at = |x: f64, z: f64| base + egui::vec2((x * scale) as f32, -(z * scale) as f32);
    for o in &d.outlines {
        let (fill, stroke) = match o.part {
            Part::Flutes => (theme::AMBER_5, theme::AMBER_3),
            Part::Shank => (theme::GRAY_7, theme::GRAY_8),
            Part::Holder => (theme::GRAY_5, theme::GRAY_7),
        };
        // Right half up, left half down: one closed, convex outline per part.
        let mut points: Vec<egui::Pos2> = o.half.iter().map(|(x, z)| at(*x, *z)).collect();
        points.extend(o.half.iter().rev().map(|(x, z)| at(-x, *z)));
        painter.add(egui::Shape::convex_polygon(
            points,
            theme::egui_color(fill),
            egui::Stroke::new(1.0, theme::egui_color(stroke)),
        ));
    }
    // The holder face: where the stickout is measured to.
    let face = at(0.0, d.holder_face).y;
    painter.line_segment(
        [
            egui::pos2(rect.left() + 8.0, face),
            egui::pos2(rect.right() - 8.0, face),
        ],
        egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
    );
    painter.text(
        egui::pos2(rect.left() + 10.0, face - 4.0),
        egui::Align2::LEFT_BOTTOM,
        format!("stickout {:.1} mm", d.holder_face),
        egui::FontId::monospace(11.0),
        ui.visuals().weak_text_color(),
    );
    painter.text(
        egui::pos2(rect.right() - 10.0, rect.bottom() - 6.0),
        egui::Align2::RIGHT_BOTTOM,
        format!("Ø {:.2} mm", tool.diameter),
        egui::FontId::monospace(11.0),
        ui.visuals().text_color(),
    );
    if !d.assumed.is_empty() {
        ui.weak(format!("assumed, not given: {}", d.assumed.join(", ")));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn tools(names: &[&str]) -> BTreeMap<String, ToolSpec> {
        names
            .iter()
            .map(|n| ((*n).to_string(), ToolSpec::default()))
            .collect()
    }

    /// Renames collapse to one per saved tool, and renaming back undoes it.
    #[test]
    fn renames_are_tracked_from_the_saved_name() {
        let mut set = Set::new(tools(&["a", "b"]));
        set.rename("a", "x").unwrap();
        set.rename("x", "y").unwrap();
        assert_eq!(set.renames, [("a".to_string(), "y".to_string())]);
        set.rename("y", "a").unwrap();
        assert!(set.renames.is_empty());
        assert!(!set.dirty());
    }

    #[test]
    fn a_new_tool_is_renamed_without_a_rename() {
        let mut set = Set::new(tools(&["a"]));
        set.edited.insert("new".into(), ToolSpec::default());
        set.rename("new", "fresh").unwrap();
        assert!(set.renames.is_empty());
        assert!(set.edited.contains_key("fresh"));
    }

    #[test]
    fn a_rename_onto_a_taken_name_or_nothing_is_refused() {
        let mut set = Set::new(tools(&["a", "b"]));
        assert!(set.rename("a", "b").is_err());
        assert!(set.rename("a", "  ").is_err());
        assert_eq!(set.edited.len(), 2);
    }

    fn manager() -> ToolManager {
        let dir = std::env::temp_dir().join(format!("camcraft-tm-{}", std::process::id()));
        ToolManager::with_library(dir.join("never-written.toml"))
    }

    /// A tool the job's operations use cannot be deleted from the job.
    #[test]
    fn a_used_tool_cannot_be_deleted() {
        let mut m = manager();
        let job: Job = toml::from_str(include_str!("../../../examples/rail-jig.toml")).unwrap();
        m.set_job(Path::new("rail-jig.toml"), &job);
        m.delete("rough-6-flat");
        assert!(
            m.job
                .as_ref()
                .unwrap()
                .set
                .edited
                .contains_key("rough-6-flat")
        );
        assert!(matches!(m.message, Some((false, _))));
    }

    /// End to end through the manager's own save: a library tool copied into the
    /// job and the roughing tool renumbered land in the job file as exactly those
    /// lines, comments kept, and a real run then posts the new number.
    #[test]
    fn a_save_reaches_the_file_and_the_program() {
        use camcraft_session::{load_model, load_profile, program, read_job, run};
        let dir = std::env::temp_dir().join(format!("camcraft-tm-e2e-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("job.toml");
        let example = include_str!("../../../examples/rail-jig.toml");
        std::fs::write(&path, example).unwrap();

        let mut m = ToolManager::with_library(dir.join("library.toml"));
        m.set_job(&path, &read_job(&path).unwrap());
        m.source = Source::Library;
        m.library.edited.insert(
            "detail-3-ball".into(),
            ToolSpec {
                kind: ToolKind::Ball,
                diameter: 3.0,
                number: 3,
                flutes: Some(2),
                ..ToolSpec::default()
            },
        );
        m.copy("detail-3-ball", Source::Job);
        m.source = Source::Job;
        m.job
            .as_mut()
            .unwrap()
            .set
            .edited
            .get_mut("rough-6-flat")
            .unwrap()
            .number = 5;
        assert_eq!(m.save(), Some(Event::JobSaved(path.clone())));
        assert!(
            !m.job.as_ref().unwrap().set.dirty(),
            "the saved job is clean"
        );
        assert!(m.library.dirty(), "the library was not saved, and says so");

        let written = std::fs::read_to_string(&path).unwrap();
        let gone: Vec<&str> = example
            .lines()
            .filter(|l| !written.lines().any(|w| w == *l))
            .collect();
        assert_eq!(gone, ["number = 1"], "only the old number went:\n{written}");
        for comment in example.lines().filter(|l| l.starts_with('#')) {
            assert!(written.contains(comment), "lost comment {comment:?}");
        }
        assert!(written.contains("[tools.detail-3-ball]"));

        let job = read_job(&path).unwrap();
        job.validate().unwrap();
        let hinge = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/step/hinge_jig.step");
        let model = load_model(&hinge, None, job.tolerance).unwrap();
        let ran = run(&model, &job).unwrap();
        let rough = ran.result.operations.first().unwrap();
        let (text, _) = program(
            rough,
            job.setup.wcs,
            &load_profile("masso").unwrap(),
            "01-rough",
        )
        .unwrap();
        assert!(
            text.contains("T5 M6"),
            "the program does not load T5:\n{}",
            &text[..400.min(text.len())]
        );
    }

    /// Copying into the job adds a copy under a free name, and the job is then
    /// unsaved; the library is untouched.
    #[test]
    fn copying_to_the_job_adds_a_copy() {
        let mut m = manager();
        let job: Job = toml::from_str(include_str!("../../../examples/rail-jig.toml")).unwrap();
        m.set_job(Path::new("rail-jig.toml"), &job);
        m.source = Source::Library;
        m.library
            .edited
            .insert("rough-6-flat".into(), ToolSpec::default());
        m.copy("rough-6-flat", Source::Job);
        let j = m.job.as_ref().unwrap();
        assert!(
            j.set.edited.contains_key("rough-6-flat-2"),
            "{:?}",
            j.set.edited.keys()
        );
        assert!(j.set.dirty());
    }
}
