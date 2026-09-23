//! The tool manager: the user's tool library, after Fusion's Tool Library.
//!
//! It opens in its own OS window and manages only the library. A job's tools are
//! chosen inside its operations, which copy them into the job, never link them,
//! so a job still cuts what was verified after the library changes.
//!
//! Tools are grouped in folders that nest: one per machine, say. The folders are
//! on the left; a tool moves between them by dragging it onto one, or with
//! "Move to". Tools are edited in place, and the library saves itself whenever an
//! edit is finished and every tool checks clean, to the user's app-data folder
//! (`camcraft_session::library`).

use std::path::PathBuf;

use camcraft_core::job::ToolSpec;
use camcraft_session::library::{
    Folder, FolderPath, Library, folder_label, free_name, library_path,
};
use eframe::egui;

use crate::measure::Measure;
use crate::tool_editor::{kind_name, tool_fields};

/// The library: as saved, and as edited.
#[derive(Debug, Clone, Default)]
struct Set {
    original: Folder,
    edited: Folder,
}

impl Set {
    fn new(root: Folder) -> Self {
        Self {
            original: root.clone(),
            edited: root,
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

    /// Rename a tool within its folder.
    fn rename(&mut self, folder: &[String], from: &str, to: &str) -> Result<(), String> {
        let to = to.trim();
        if to.is_empty() {
            return Err("a tool needs a name".into());
        }
        if to == from {
            return Ok(());
        }
        let tools = &mut self
            .edited
            .at_mut(folder)
            .ok_or_else(|| format!("there is no folder {}", folder_label(folder)))?
            .tools;
        if tools.contains_key(to) {
            return Err(format!("there is already a tool called '{to}' here"));
        }
        let tool = tools
            .remove(from)
            .ok_or_else(|| format!("no tool called '{from}'"))?;
        tools.insert(to.to_string(), tool);
        Ok(())
    }

    /// A tool number no tool in the library uses yet.
    fn next_number(&self) -> u32 {
        self.edited
            .walk()
            .iter()
            .map(|(_, _, t)| t.number)
            .max()
            .unwrap_or(0)
            + 1
    }
}

/// How the table is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sort {
    Number,
    Name,
    Diameter,
}

/// A tool being dragged onto a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolDrag {
    folder: FolderPath,
    name: String,
}

/// Something done in the folder tree, applied once it has been drawn.
enum TreeAction {
    Select(FolderPath),
    NewIn(FolderPath),
    StartRename(FolderPath),
    Rename(FolderPath, String),
    CancelRename,
    Delete(FolderPath),
    Drop(ToolDrag, FolderPath),
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
    library_unsaved: Option<(String, Folder)>,
    /// The folder showing.
    folder: FolderPath,
    /// Tools in the folders inside it show too.
    include_subfolders: bool,
    /// The tool being edited: its folder and name.
    selected: Option<(FolderPath, String)>,
    name_buffer: String,
    /// A folder being renamed, and the name so far.
    renaming: Option<(FolderPath, String)>,
    /// A folder with tools in it, to be deleted once confirmed.
    confirm_delete: Option<FolderPath>,
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
            library: Set::new(library.root),
            library_error,
            library_unsaved: None,
            folder: Vec::new(),
            include_subfolders: true,
            selected: None,
            name_buffer: String::new(),
            renaming: None,
            confirm_delete: None,
            filter: String::new(),
            sort: Sort::Number,
            message: None,
            closing: false,
        }
    }

    /// The library as it stands, edits included: what a tool is picked from.
    pub fn library(&self) -> &Folder {
        &self.library.edited
    }

    /// Show `name` selected in the folder showing, after `tool` is put there under
    /// it. Only the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn show_for_snapshot(&mut self, name: &str, tool: Option<ToolSpec>) {
        if let (Some(tool), Some(folder)) = (tool, self.library.edited.at_mut(&self.folder)) {
            folder.tools.insert(name.to_string(), tool);
        }
        self.select(Some((self.folder.clone(), name.to_string())));
    }

    /// Fill the library with folders for two machines and show the first. Only the
    /// snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn folders_for_snapshot(&mut self) {
        fill_for_snapshot(&mut self.library.edited);
        self.folder = vec!["machine_01".into()];
        self.include_subfolders = true;
        self.select(Some((
            vec!["machine_01".into(), "flat".into()],
            "flat-6".into(),
        )));
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
                root: self.library.edited.clone(),
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

    fn select(&mut self, tool: Option<(FolderPath, String)>) {
        self.name_buffer = tool.as_ref().map(|(_, n)| n.clone()).unwrap_or_default();
        self.selected = tool;
    }

    fn selected_tool(&self) -> Option<&ToolSpec> {
        let (folder, name) = self.selected.as_ref()?;
        self.library.edited.at(folder)?.tools.get(name)
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
                .with_inner_size([1280.0, 720.0])
                .with_min_inner_size([940.0, 480.0]),
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
        egui::Panel::left("tools-folders")
            .resizable(true)
            .default_size(220.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.folders(ui));
            });
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
        if let Some(path) = self.confirm_delete.clone() {
            let n = self.library.edited.at(&path).map_or(0, Folder::count);
            ui.horizontal(|ui| {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    format!(
                        "Delete the folder {} and the {n} tool(s) in it?",
                        folder_label(&path)
                    ),
                );
                if ui.button("Delete").clicked() {
                    self.delete_folder(&path);
                    self.confirm_delete = None;
                }
                if ui.button("Keep it").clicked() {
                    self.confirm_delete = None;
                }
            });
            ui.separator();
        }
        ui.horizontal(|ui| {
            ui.strong(format!("Library ({})", self.library.edited.count()));
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

    /// The folder tree, with the top of the library first.
    fn folders(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("Folders");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("New folder")
                    .on_hover_text("a folder inside the one selected")
                    .clicked()
                {
                    let parent = self.folder.clone();
                    self.new_folder(&parent);
                }
            });
        });
        ui.separator();
        let mut actions = Vec::new();
        folder_row(
            ui,
            &self.library.edited,
            &mut Vec::new(),
            &self.folder,
            &mut self.renaming,
            &mut actions,
        );
        ui.add_space(8.0);
        ui.weak("Drag a tool onto a folder to move it there. Right-click a folder for more.");
        for action in actions {
            self.apply(action);
        }
    }

    fn apply(&mut self, action: TreeAction) {
        match action {
            TreeAction::Select(path) => {
                if self.folder != path {
                    self.folder = path;
                    self.select(None);
                }
            }
            TreeAction::NewIn(path) => self.new_folder(&path),
            TreeAction::StartRename(path) => {
                let name = path.last().cloned().unwrap_or_default();
                self.renaming = Some((path, name));
            }
            TreeAction::Rename(path, to) => {
                self.renaming = None;
                match self.library.edited.rename(&path, &to) {
                    Ok(new) => {
                        let moved = |p: &mut FolderPath| {
                            if p.starts_with(&path) {
                                p.splice(..path.len(), new.iter().cloned());
                            }
                        };
                        moved(&mut self.folder);
                        if let Some((f, _)) = self.selected.as_mut() {
                            moved(f);
                        }
                    }
                    Err(e) => self.message = Some((false, e.to_string())),
                }
            }
            TreeAction::CancelRename => self.renaming = None,
            TreeAction::Delete(path) => {
                if self.library.edited.at(&path).is_some_and(|f| f.count() > 0) {
                    self.confirm_delete = Some(path);
                } else {
                    self.delete_folder(&path);
                }
            }
            TreeAction::Drop(drag, to) => self.move_tool(&drag.folder, &drag.name, &to),
        }
    }

    fn new_folder(&mut self, parent: &[String]) {
        let taken: Vec<String> = self
            .library
            .edited
            .at(parent)
            .map(|f| f.folders.keys().cloned().collect())
            .unwrap_or_default();
        let name = (1..)
            .map(|n| {
                if n == 1 {
                    "New folder".to_string()
                } else {
                    format!("New folder {n}")
                }
            })
            .find(|n| !taken.contains(n))
            .unwrap_or_default();
        match self.library.edited.create(parent, &name) {
            Ok(path) => {
                self.folder = path.clone();
                self.select(None);
                self.renaming = Some((path, name));
            }
            Err(e) => self.message = Some((false, e.to_string())),
        }
    }

    fn delete_folder(&mut self, path: &[String]) {
        match self.library.edited.remove(path) {
            Ok(_) => {
                if self.folder.starts_with(path) {
                    self.folder = path
                        .split_last()
                        .map(|(_, p)| p.to_vec())
                        .unwrap_or_default();
                }
                if self
                    .selected
                    .as_ref()
                    .is_some_and(|(f, _)| f.starts_with(path))
                {
                    self.select(None);
                }
            }
            Err(e) => self.message = Some((false, e.to_string())),
        }
    }

    fn move_tool(&mut self, from: &[String], name: &str, to: &[String]) {
        if from == to {
            return;
        }
        match self.library.edited.move_tool(from, name, to) {
            Ok(as_name) => {
                let was = self
                    .selected
                    .as_ref()
                    .is_some_and(|(f, n)| f.as_slice() == from && n == name);
                if was {
                    self.select(Some((to.to_vec(), as_name.clone())));
                }
                self.message = Some((
                    true,
                    format!("moved '{name}' to {} as '{as_name}'", folder_label(to)),
                ));
            }
            Err(e) => self.message = Some((false, e.to_string())),
        }
    }

    fn table(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong(folder_label(&self.folder));
            ui.checkbox(&mut self.include_subfolders, "include the folders inside");
        });
        let all_folders = self.library.edited.folder_paths();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("filter"));
            if ui.button("New").clicked() {
                self.new_tool();
            }
            let selected = self.selected.clone();
            if ui
                .add_enabled(selected.is_some(), egui::Button::new("Duplicate"))
                .clicked()
                && let Some((folder, name)) = &selected
            {
                self.duplicate(folder, name);
            }
            if ui
                .add_enabled(selected.is_some(), egui::Button::new("Delete"))
                .clicked()
                && let Some((folder, name)) = &selected
            {
                self.delete(folder, name);
            }
            ui.add_enabled_ui(selected.is_some(), |ui| {
                ui.menu_button("Move to", |ui| {
                    for path in &all_folders {
                        if ui.button(folder_label(path)).clicked()
                            && let Some((folder, name)) = &selected
                        {
                            self.move_tool(folder, name, path);
                            ui.close();
                        }
                    }
                });
            });
        });
        ui.separator();

        let filter = self.filter.to_lowercase();
        let Some(here) = self.library.edited.at(&self.folder) else {
            self.folder = Vec::new();
            return;
        };
        let subs = if self.include_subfolders {
            here.folder_paths()
        } else {
            vec![Vec::new()]
        };
        let mut rows: Vec<(FolderPath, &String, &ToolSpec)> = subs
            .into_iter()
            .filter_map(|sub| here.at(&sub).map(|f| (sub, f)))
            .flat_map(|(sub, f)| f.tools.iter().map(move |(n, t)| (sub.clone(), n, t)))
            .filter(|(_, n, t)| {
                filter.is_empty()
                    || n.to_lowercase().contains(&filter)
                    || t.comment.to_lowercase().contains(&filter)
            })
            .collect();
        match self.sort {
            Sort::Number => rows.sort_by_key(|(f, n, t)| (t.number, (*n).clone(), f.clone())),
            Sort::Name => rows.sort_by_key(|(f, n, _)| ((*n).clone(), f.clone())),
            Sort::Diameter => rows.sort_by(|a, b| a.2.diameter.total_cmp(&b.2.diameter)),
        }
        let m = self.measure;
        let base = self.folder.clone();
        let with_folder = self.include_subfolders;
        let mut clicked = None;
        let mut moves = Vec::new();
        let mut sort = self.sort;
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("tools-table")
                .striped(true)
                .num_columns(if with_folder { 8 } else { 7 })
                .min_col_width(40.0)
                .show(ui, |ui| {
                    for (label, key) in [("T", Some(Sort::Number)), ("name", Some(Sort::Name))] {
                        header(ui, label, key, &mut sort);
                    }
                    if with_folder {
                        ui.strong("in");
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
                    for (sub, name, tool) in rows {
                        let mut folder = base.clone();
                        folder.extend(sub.iter().cloned());
                        ui.label(format!("T{}", tool.number));
                        let is = self
                            .selected
                            .as_ref()
                            .is_some_and(|(f, n)| *f == folder && n == name);
                        let drag = ToolDrag {
                            folder: folder.clone(),
                            name: name.clone(),
                        };
                        let id = egui::Id::new(("tool-row", &folder, name));
                        let r = ui
                            .dnd_drag_source(id, drag, |ui| ui.selectable_label(is, name.as_str()));
                        if r.inner.clicked() {
                            clicked = Some((folder.clone(), name.clone()));
                        }
                        r.inner.context_menu(|ui| {
                            ui.weak("Move to");
                            for path in &all_folders {
                                if *path != folder && ui.button(folder_label(path)).clicked() {
                                    moves.push((folder.clone(), name.clone(), path.clone()));
                                    ui.close();
                                }
                            }
                        });
                        if with_folder {
                            ui.weak(sub.join("/"));
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
        if let Some(tool) = clicked {
            self.select(Some(tool));
        }
        for (from, name, to) in moves {
            self.move_tool(&from, &name, &to);
        }
    }

    fn new_tool(&mut self) {
        let number = self.library.next_number();
        let Some(here) = self.library.edited.at_mut(&self.folder) else {
            return;
        };
        let name = free_name("tool", &here.tools);
        here.tools.insert(
            name.clone(),
            ToolSpec {
                number,
                ..ToolSpec::default()
            },
        );
        self.select(Some((self.folder.clone(), name)));
    }

    fn duplicate(&mut self, folder: &[String], name: &str) {
        let number = self.library.next_number();
        let Some(here) = self.library.edited.at_mut(folder) else {
            return;
        };
        let Some(tool) = here.tools.get(name).cloned() else {
            return;
        };
        let copy = free_name(&format!("{name}-copy"), &here.tools);
        here.tools.insert(copy.clone(), ToolSpec { number, ..tool });
        self.select(Some((folder.to_vec(), copy)));
    }

    fn delete(&mut self, folder: &[String], name: &str) {
        if let Some(here) = self.library.edited.at_mut(folder) {
            here.tools.remove(name);
        }
        self.select(None);
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let Some((folder, name)) = self.selected.clone() else {
            ui.weak("Select a tool to edit it.");
            return;
        };
        let Some(original) = self.selected_tool().cloned() else {
            self.select(None);
            return;
        };
        ui.weak(folder_label(&folder));

        // The name, applied when editing finishes.
        ui.horizontal(|ui| {
            ui.label("Name");
            let r = ui.text_edit_singleline(&mut self.name_buffer);
            if r.lost_focus() && self.name_buffer != name {
                let new = self.name_buffer.trim().to_string();
                match self.library.rename(&folder, &name, &new) {
                    Ok(()) => {
                        self.selected = Some((folder.clone(), new.clone()));
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
        if tool != original
            && let Some((folder, key)) = self.selected.clone()
            && let Some(slot) = self
                .library
                .edited
                .at_mut(&folder)
                .and_then(|f| f.tools.get_mut(&key))
        {
            *slot = tool;
        }
    }
}

/// Two machines' folders of tools, for pictures and tests.
#[cfg(test)]
fn fill_for_snapshot(root: &mut Folder) {
    use camcraft_core::job::ToolKind;
    let tool = |kind, diameter: f64, number, comment: &str| ToolSpec {
        kind,
        diameter,
        number,
        comment: comment.into(),
        flutes: Some(2),
        stickout: Some(diameter * 4.0),
        ..ToolSpec::default()
    };
    for (machine, first) in [("machine_01", 1), ("machine_02", 11)] {
        let m = root.create(&[], machine).unwrap_or_default();
        let flat = root.create(&m, "flat").unwrap_or_default();
        let ball = root.create(&m, "ball").unwrap_or_default();
        if let Some(f) = root.at_mut(&flat) {
            f.tools.insert(
                "flat-6".into(),
                tool(ToolKind::Flat, 6.0, first, "6mm flat"),
            );
            f.tools.insert(
                "flat-3".into(),
                tool(ToolKind::Flat, 3.0, first + 1, "3mm flat"),
            );
        }
        if let Some(f) = root.at_mut(&ball) {
            f.tools.insert(
                "ball-6".into(),
                tool(ToolKind::Ball, 6.0, first + 2, "6mm ball"),
            );
        }
    }
}

/// One folder's row in the tree, then the folders inside it.
fn folder_row(
    ui: &mut egui::Ui,
    folder: &Folder,
    path: &mut FolderPath,
    selected: &FolderPath,
    renaming: &mut Option<(FolderPath, String)>,
    actions: &mut Vec<TreeAction>,
) {
    let row = |ui: &mut egui::Ui,
               path: &FolderPath,
               renaming: &mut Option<(FolderPath, String)>,
               actions: &mut Vec<TreeAction>| {
        if let Some((at, buffer)) = renaming.as_mut()
            && at == path
        {
            let r = ui.add(egui::TextEdit::singleline(buffer).desired_width(140.0));
            if !r.has_focus() && !r.lost_focus() {
                r.request_focus();
            }
            if r.lost_focus() {
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    actions.push(TreeAction::CancelRename);
                } else {
                    actions.push(TreeAction::Rename(path.clone(), buffer.clone()));
                }
            }
            return;
        }
        let name = path.last().map_or("Library", String::as_str);
        let r = ui.selectable_label(*selected == *path, name);
        ui.weak(folder.count().to_string());
        if r.dnd_hover_payload::<ToolDrag>().is_some() {
            ui.painter().rect_stroke(
                r.rect.expand(2.0),
                3.0,
                egui::Stroke::new(1.5, ui.visuals().selection.stroke.color),
                egui::StrokeKind::Outside,
            );
        }
        if let Some(drag) = r.dnd_release_payload::<ToolDrag>() {
            actions.push(TreeAction::Drop((*drag).clone(), path.clone()));
        }
        if r.clicked() {
            actions.push(TreeAction::Select(path.clone()));
        }
        r.context_menu(|ui| {
            if ui.button("New folder inside").clicked() {
                actions.push(TreeAction::NewIn(path.clone()));
                ui.close();
            }
            if !path.is_empty() {
                if ui.button("Rename").clicked() {
                    actions.push(TreeAction::StartRename(path.clone()));
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    actions.push(TreeAction::Delete(path.clone()));
                    ui.close();
                }
            }
        });
    };

    if folder.folders.is_empty() {
        ui.horizontal(|ui| {
            // Line up with the rows that have an open-close arrow.
            ui.add_space(ui.spacing().indent);
            row(ui, path, renaming, actions);
        });
        return;
    }
    let id = ui.make_persistent_id(("tool-folder", &*path));
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
        .show_header(ui, |ui| row(ui, path, renaming, actions))
        .body(|ui| {
            for (name, sub) in &folder.folders {
                path.push(name.clone());
                folder_row(ui, sub, path, selected, renaming, actions);
                path.pop();
            }
        });
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
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn tools(names: &[&str]) -> Folder {
        Folder {
            tools: names
                .iter()
                .map(|n| ((*n).to_string(), ToolSpec::default()))
                .collect(),
            ..Folder::default()
        }
    }

    #[test]
    fn a_rename_onto_a_taken_name_or_nothing_is_refused() {
        let mut set = Set::new(tools(&["a", "b"]));
        assert!(set.rename(&[], "a", "b").is_err());
        assert!(set.rename(&[], "a", "  ").is_err());
        assert_eq!(set.edited.tools.len(), 2);
        set.rename(&[], "a", "c").unwrap();
        assert!(set.edited.tools.contains_key("c"));
    }

    fn manager(name: &str) -> ToolManager {
        let dir = std::env::temp_dir().join(format!("camcraft-tm-{name}-{}", std::process::id()));
        ToolManager::with_library(dir.join("never-written.toml"))
    }

    /// Moving the tool being edited keeps it selected where it went, and a folder
    /// renamed under the selection carries it along.
    #[test]
    fn the_selection_follows_moves_and_renames() {
        let mut m = manager("follow");
        fill_for_snapshot(&mut m.library.edited);
        let flat = vec!["machine_01".to_string(), "flat".to_string()];
        m.select(Some((flat.clone(), "flat-6".into())));
        let ball = vec!["machine_01".to_string(), "ball".to_string()];
        m.move_tool(&flat, "flat-6", &ball);
        assert_eq!(m.selected, Some((ball.clone(), "flat-6".into())));
        m.folder = ball.clone();
        m.apply(TreeAction::Rename(
            vec!["machine_01".into()],
            "router".into(),
        ));
        let moved = vec!["router".to_string(), "ball".to_string()];
        assert_eq!(m.folder, moved);
        assert_eq!(m.selected, Some((moved.clone(), "flat-6".into())));
        assert!(m.selected_tool().is_some());
    }

    /// A folder with tools in it is deleted only once confirmed; an empty one at
    /// once.
    #[test]
    fn deleting_a_full_folder_asks_first() {
        let mut m = manager("delete");
        fill_for_snapshot(&mut m.library.edited);
        m.apply(TreeAction::Delete(vec!["machine_02".into()]));
        assert!(m.confirm_delete.is_some());
        assert!(m.library.edited.at(&["machine_02".into()]).is_some());
        let empty = m.library.edited.create(&[], "empty").unwrap();
        m.apply(TreeAction::Delete(empty.clone()));
        assert!(m.library.edited.at(&empty).is_none());
    }

    /// A library edit is written as soon as it is kept, with nothing to press.
    #[test]
    fn the_library_saves_itself() {
        let dir = std::env::temp_dir().join(format!("camcraft-tm-lib-{}", std::process::id()));
        let path = dir.join("library.toml");
        let _ = std::fs::remove_file(&path);
        let mut m = ToolManager::with_library(path.clone());
        let folder = m.library.edited.create(&[], "machine_01").unwrap();
        m.library
            .edited
            .at_mut(&folder)
            .unwrap()
            .tools
            .insert("flat-6".into(), ToolSpec::default());
        m.save_library();
        assert!(!m.library.dirty());
        let again = ToolManager::with_library(path);
        assert!(
            again
                .library
                .edited
                .at(&folder)
                .unwrap()
                .tools
                .contains_key("flat-6")
        );
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
        m.library.edited.tools.insert("bad".into(), bad);
        m.save_library();
        assert!(m.library.dirty());
        assert!(m.library_unsaved.is_some());
        assert!(!path.exists());
        m.library.edited.tools.get_mut("bad").unwrap().diameter = 6.0;
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
            .tools
            .insert("flat-6".into(), ToolSpec::default());
        m.save_library();
        assert!(m.library.dirty());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "this is not toml ["
        );
    }
}
