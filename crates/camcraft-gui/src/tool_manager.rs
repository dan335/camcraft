//! The tool manager: the user's tool library, after Fusion's Tool Library.
//!
//! It opens in its own OS window and manages only the library. A job's tools are
//! chosen inside its operations, which copy them into the job, never link them,
//! so a job still cuts what was verified after the library changes.
//!
//! Tools are edited in place. The library saves itself whenever an edit is
//! finished and every tool checks clean, to the user's app-data folder
//! (`camcraft_session::library`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use camcraft_core::job::ToolSpec;
use camcraft_session::library::{Library, free_name, library_path};
use eframe::egui;

use crate::measure::Measure;
use crate::tool_editor::{kind_name, tool_fields};

/// The library's tools: as saved, and as edited.
#[derive(Debug, Clone, Default)]
struct Set {
    original: BTreeMap<String, ToolSpec>,
    edited: BTreeMap<String, ToolSpec>,
}

impl Set {
    fn new(tools: BTreeMap<String, ToolSpec>) -> Self {
        Self {
            original: tools.clone(),
            edited: tools,
        }
    }

    fn dirty(&self) -> bool {
        self.edited != self.original
    }

    fn revert(&mut self) {
        self.edited = self.original.clone();
    }

    fn saved(&mut self) {
        self.original = self.edited.clone();
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
        Ok(())
    }

    /// A tool number no tool here uses yet.
    fn next_number(&self) -> u32 {
        self.edited.values().map(|t| t.number).max().unwrap_or(0) + 1
    }
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
    /// How lengths and feeds are shown; the tools themselves stay in millimetres.
    pub measure: Measure,
    library_path: Option<PathBuf>,
    library: Set,
    library_error: Option<String>,
    /// Why the library's edits are not saved yet, and the edits that got that
    /// answer, so a failure is not retried every frame.
    library_unsaved: Option<(String, BTreeMap<String, ToolSpec>)>,
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
            measure: Measure::default(),
            library_path: Some(path),
            library: Set::new(library.tools),
            library_error,
            library_unsaved: None,
            selected: None,
            name_buffer: String::new(),
            filter: String::new(),
            sort: Sort::Number,
            message: None,
            closing: false,
        }
    }

    /// Show `name` selected, after `tool` is put in the library under it. Only
    /// the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn show_for_snapshot(&mut self, name: &str, tool: Option<ToolSpec>) {
        if let Some(tool) = tool {
            self.library.edited.insert(name.to_string(), tool);
        }
        self.select(Some(name.to_string()));
    }

    /// Save the library if it has edits and they can be kept. The manager does
    /// this itself as edits finish; the app calls it too, on the way out.
    pub fn save_library(&mut self) {
        if !self.library.dirty() {
            self.library_unsaved = None;
            return;
        }
        if self
            .library_unsaved
            .as_ref()
            .is_some_and(|(_, tried)| *tried == self.library.edited)
        {
            return;
        }
        let why = if self.library_error.is_some() {
            // Writing now would replace the file that failed to load with only
            // what has been added since.
            Some("the library file could not be read, so it is not written over".into())
        } else if let Some(path) = self.library_path.clone() {
            let lib = Library {
                tools: self.library.edited.clone(),
            };
            lib.save(&path).err().map(|e| e.to_string())
        } else {
            Some("there is nowhere to keep the library".into())
        };
        match why {
            None => {
                self.library.saved();
                self.library_unsaved = None;
            }
            Some(why) => self.library_unsaved = Some((why, self.library.edited.clone())),
        }
    }

    /// Whether anything is unsaved.
    pub fn dirty(&self) -> bool {
        self.library.dirty()
    }

    fn select(&mut self, name: Option<String>) {
        self.name_buffer = name.clone().unwrap_or_default();
        self.selected = name;
    }

    /// Show the window, if open.
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("camcraft-tool-manager"),
            egui::ViewportBuilder::default()
                .with_title("camcraft — tool library")
                .with_inner_size([1180.0, 720.0])
                .with_min_inner_size([860.0, 480.0]),
            |ui, _class| {
                if ui.ctx().input(|i| i.viewport().close_requested()) {
                    self.save_library();
                    if self.dirty() {
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                        self.closing = true;
                    } else {
                        self.open = false;
                    }
                }
                self.ui(ui);
            },
        );
    }

    /// The whole manager, in `ui`.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("tools-top").show(ui, |ui| self.top_bar(ui));
        egui::Panel::right("tools-editor")
            .resizable(true)
            .default_size(420.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.editor(ui));
            });
        egui::CentralPanel::default().show(ui, |ui| self.table(ui));
        // An edit is finished once no field is being typed in or dragged.
        let ctx = ui.ctx();
        if ctx.memory(|m| m.focused().is_none()) && ctx.dragged_id().is_none() {
            self.save_library();
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        if self.closing {
            ui.horizontal(|ui| {
                ui.colored_label(ui.visuals().warn_fg_color, "There are unsaved changes.");
                if ui.button("Discard and close").clicked() {
                    self.library.revert();
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
            ui.strong(format!("Library ({})", self.library.edited.len()));
            if let Some((ok, text)) = &self.message {
                let color = if *ok {
                    ui.visuals().text_color()
                } else {
                    ui.visuals().error_fg_color
                };
                ui.colored_label(color, text);
            }
        });
        if let Some(e) = &self.library_error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        } else if let Some(p) = &self.library_path {
            ui.weak(format!("saved as you go, in {}", p.display()));
        }
        if let Some((why, _)) = &self.library_unsaved {
            ui.colored_label(ui.visuals().error_fg_color, format!("not saved: {why}"));
        }
    }

    fn table(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("filter"));
            if ui.button("New").clicked() {
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
        });
        ui.separator();

        let filter = self.filter.to_lowercase();
        let mut rows: Vec<(&String, &ToolSpec)> = self
            .library
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
        let m = self.measure;
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
                    header(
                        ui,
                        &format!("Ø {}", m.unit()),
                        Some(Sort::Diameter),
                        &mut sort,
                    );
                    ui.strong("flutes");
                    ui.strong(format!("stickout {}", m.unit()));
                    ui.strong("description");
                    ui.end_row();
                    for (name, tool) in rows {
                        ui.label(format!("T{}", tool.number));
                        let is = self.selected.as_deref() == Some(name.as_str());
                        if ui.selectable_label(is, name.as_str()).clicked() {
                            clicked = Some(name.clone());
                        }
                        ui.label(kind_name(tool.kind));
                        ui.label(m.number(tool.diameter, 2));
                        ui.label(tool.flutes.map_or_else(|| "–".into(), |f| f.to_string()));
                        ui.label(tool.stickout.map_or_else(|| "–".into(), |s| m.number(s, 1)));
                        ui.label(&tool.comment);
                        ui.end_row();
                    }
                });
        });
        self.sort = sort;
        if let Some(name) = clicked {
            self.select(Some(name));
        }
    }

    fn new_tool(&mut self) {
        let set = &mut self.library;
        let name = free_name("tool", &set.edited);
        let tool = ToolSpec {
            number: set.next_number(),
            ..ToolSpec::default()
        };
        set.edited.insert(name.clone(), tool);
        self.select(Some(name));
    }

    fn duplicate(&mut self, name: &str) {
        let set = &mut self.library;
        let Some(tool) = set.edited.get(name).cloned() else {
            return;
        };
        let copy = free_name(&format!("{name}-copy"), &set.edited);
        let number = set.next_number();
        set.edited.insert(copy.clone(), ToolSpec { number, ..tool });
        self.select(Some(copy));
    }

    fn delete(&mut self, name: &str) {
        self.library.edited.remove(name);
        self.select(None);
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let Some(name) = self.selected.clone() else {
            ui.weak("Select a tool to edit it.");
            return;
        };
        let Some(original) = self.library.edited.get(&name).cloned() else {
            self.select(None);
            return;
        };

        // The name, applied when editing finishes.
        ui.horizontal(|ui| {
            ui.label("Name");
            let r = ui.text_edit_singleline(&mut self.name_buffer);
            if r.lost_focus() && self.name_buffer != name {
                let new = self.name_buffer.trim().to_string();
                match self.library.rename(&name, &new) {
                    Ok(()) => {
                        self.selected = Some(new.clone());
                        self.name_buffer = new;
                    }
                    Err(e) => {
                        self.message = Some((false, e));
                        self.name_buffer = name.clone();
                    }
                }
            }
        });

        let mut tool = original.clone();
        tool_fields(ui, &mut tool, self.measure);
        // The edit lands under whatever name the tool has now.
        let key = self.selected.clone().unwrap_or(name);
        if tool != original
            && let Some(slot) = self.library.edited.get_mut(&key)
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

    #[test]
    fn a_rename_onto_a_taken_name_or_nothing_is_refused() {
        let mut set = Set::new(tools(&["a", "b"]));
        assert!(set.rename("a", "b").is_err());
        assert!(set.rename("a", "  ").is_err());
        assert_eq!(set.edited.len(), 2);
    }

    /// A library edit is written as soon as it is kept, with nothing to press.
    #[test]
    fn the_library_saves_itself() {
        let dir = std::env::temp_dir().join(format!("camcraft-tm-lib-{}", std::process::id()));
        let path = dir.join("library.toml");
        let _ = std::fs::remove_file(&path);
        let mut m = ToolManager::with_library(path.clone());
        m.library
            .edited
            .insert("flat-6".into(), ToolSpec::default());
        m.save_library();
        assert!(!m.library.dirty());
        let again = ToolManager::with_library(path);
        assert!(again.library.edited.contains_key("flat-6"));
    }

    /// A tool that does not check clean holds the save back, and says why, until
    /// it is fixed.
    #[test]
    fn a_bad_library_tool_is_not_saved() {
        let dir = std::env::temp_dir().join(format!("camcraft-tm-bad-{}", std::process::id()));
        let path = dir.join("library.toml");
        let _ = std::fs::remove_file(&path);
        let mut m = ToolManager::with_library(path.clone());
        let bad = ToolSpec {
            diameter: -1.0,
            ..ToolSpec::default()
        };
        m.library.edited.insert("bad".into(), bad);
        m.save_library();
        assert!(m.library.dirty());
        assert!(m.library_unsaved.is_some());
        assert!(!path.exists());
        m.library.edited.get_mut("bad").unwrap().diameter = 6.0;
        m.save_library();
        assert!(!m.library.dirty());
        assert!(m.library_unsaved.is_none());
    }

    /// A library file that could not be read is never written over.
    #[test]
    fn an_unreadable_library_is_not_written_over() {
        let dir = std::env::temp_dir().join(format!("camcraft-tm-unread-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("library.toml");
        std::fs::write(&path, "this is not toml [").unwrap();
        let mut m = ToolManager::with_library(path.clone());
        assert!(m.library_error.is_some());
        m.library
            .edited
            .insert("flat-6".into(), ToolSpec::default());
        m.save_library();
        assert!(m.library.dirty());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "this is not toml ["
        );
    }
}
