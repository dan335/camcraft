//! The tool manager: the user's tool library, after Fusion's Tool Library.
//!
//! It opens in a window inside the app and manages only the library. A job's
//! tools are chosen inside its operations, which copy them into the job, never
//! link them, so a job still cuts what was verified after the library changes.
//!
//! Tools are grouped in folders that nest: one per machine, say. The folders are
//! on the left; a tool moves between them by dragging it onto one, or with
//! "Move to". Tools are edited in place, and the library saves itself whenever an
//! edit is finished and every tool checks clean, to the user's app-data folder
//! (`camcraft_session::library`).
//!
//! The System library, a folder of common bits built in, shows among them. Nothing
//! in it can be changed; a bit is copied out of it into the person's own folders,
//! with a button or by dragging it onto one.

use std::path::{Path, PathBuf};

use camcraft_core::job::ToolSpec;
use camcraft_session::library::system::{SYSTEM, add_system, is_system};
use camcraft_session::library::{
    Folder, FolderPath, Library, folder_label, free_name, fusion_csv, library_path,
};
use eframe::egui;

use crate::tools::drawing::{kind_name, paint_tool};
use crate::tools::editor::tool_fields;
use crate::tools::table::{column_header, dim, name_cell, number, number_cell, tool_table};
use crate::ui::measure::Measure;
use crate::ui::theme;
use crate::ui::widgets::{self, Icon, Level, Size, Variant, list_row_background};

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

    /// A tool number none of the person's own tools uses yet. The System
    /// library's numbers are its own, and do not push theirs up.
    fn next_number(&self) -> u32 {
        self.edited
            .walk()
            .iter()
            .filter(|(f, _, _)| !is_system(f))
            .map(|(_, _, t)| t.number)
            .max()
            .unwrap_or(0)
            + 1
    }
}

/// Why nothing in the System library can be changed, and what to do instead.
const LOCKED: &str = "The System library can't be changed; pick one of your own folders, \
                      or copy a bit into one";

/// What copying a System library bit out is called.
const COPY_OUT: &str = "Copy to my library";

/// Where a System library bit's copy goes, said on the button.
fn copy_out_hint(to: &[String]) -> String {
    format!("A copy to change, in {}", folder_label(to))
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

/// Something done to a tool's row, applied once the table has been drawn.
enum RowAction {
    Select(FolderPath, String),
    Move(FolderPath, String, FolderPath),
    Duplicate(FolderPath, String),
    Delete(FolderPath, String),
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
    /// The last of the person's own folders shown: where a System library bit is
    /// copied to.
    own_folder: FolderPath,
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
    /// The person asked to import a Fusion tool library; the app opens the dialog.
    pub import_requested: bool,
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

    /// A manager for the library at `path`, with the System library in it.
    pub fn with_library(path: PathBuf) -> Self {
        let (library, library_error) = match Library::load(&path) {
            Ok(lib) => (lib, None),
            Err(e) => (Library::default(), Some(e.to_string())),
        };
        let mut root = library.root;
        add_system(&mut root);
        Self {
            open: false,
            measure: Measure::default(),
            library_path: Some(path),
            library: Set::new(root),
            library_error,
            library_unsaved: None,
            folder: Vec::new(),
            own_folder: Vec::new(),
            include_subfolders: true,
            selected: None,
            name_buffer: String::new(),
            renaming: None,
            confirm_delete: None,
            filter: String::new(),
            sort: Sort::Number,
            message: None,
            closing: false,
            import_requested: false,
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

    /// Show the System library with one of its bits selected. Only the snapshot
    /// harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn system_for_snapshot(&mut self) {
        self.folder = vec![SYSTEM.into()];
        self.select(Some((
            vec![SYSTEM.into(), "End mills".into()],
            "1/4 upcut".into(),
        )));
    }

    /// Show a new, empty folder, with nothing selected. Only the snapshot harness
    /// asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn empty_folder_for_snapshot(&mut self) {
        if let Ok(path) = self.library.edited.create(&[], "empty") {
            self.folder = path;
        }
        self.select(None);
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

    /// Bring the tools in the Fusion tool library CSV at `path` into the folder
    /// showing. A tool already there gets the file's presets beside its own.
    pub fn import_fusion(&mut self, path: &Path) {
        if is_system(&self.folder) {
            self.message = Some((false, LOCKED.into()));
            return;
        }
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                self.message = Some((false, format!("{}: {e}", path.display())));
                return;
            }
        };
        let imported = match fusion_csv::import(&text) {
            Ok(i) => i,
            Err(e) => {
                self.message = Some((false, format!("{}: {e}", path.display())));
                return;
            }
        };
        let Some(folder) = self.library.edited.at_mut(&self.folder) else {
            return;
        };
        let merged = fusion_csv::merge(folder, imported.tools);
        let mut said = Vec::new();
        if !merged.added.is_empty() {
            said.push(format!("added {}", merged.added.join(", ")));
        }
        for (name, n) in &merged.extended {
            said.push(match n {
                0 => format!("'{name}' already has these presets"),
                1 => format!("added 1 preset to '{name}'"),
                n => format!("added {n} presets to '{name}'"),
            });
        }
        if !imported.skipped.is_empty() {
            said.push(format!(
                "skipped {}: {}",
                imported.skipped.len(),
                imported.skipped.join("; ")
            ));
        }
        if said.is_empty() {
            said.push("no tools in it".into());
        }
        let file = path.file_name().map_or_else(
            || path.display().to_string(),
            |f| f.to_string_lossy().into_owned(),
        );
        self.message = Some((
            imported.skipped.is_empty(),
            format!("{file}: {}", said.join("; ")),
        ));
        let first = merged
            .added
            .first()
            .or_else(|| merged.extended.first().map(|(n, _)| n))
            .cloned();
        if let Some(name) = first {
            self.select(Some((self.folder.clone(), name)));
        }
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
        // A window inside the app's own, as Settings is, rather than an OS window:
        // a tiling window manager would tile an OS window beside the app.
        let screen = ctx.content_rect().shrink(16.0);
        let mut open = true;
        egui::Window::new("Tool library")
            .id(egui::Id::new("camcraft-tool-manager"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2(1280.0, 720.0).min(screen.size()))
            .min_size(egui::vec2(940.0, 480.0).min(screen.size()))
            .max_size(screen.size())
            .constrain_to(screen)
            .show(ctx, |ui| self.ui(ui));
        if !open {
            self.save_library();
            if self.dirty() {
                self.closing = true;
            } else {
                self.open = false;
            }
        }
    }

    /// The whole manager, in `ui`.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let bar = egui::Frame::new()
            .fill(theme::egui_color(theme::SURFACE_1))
            .inner_margin(egui::Margin::symmetric(12, 8));
        egui::Panel::top("tools-top")
            .frame(bar)
            .show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("tools-status")
            .frame(bar.inner_margin(egui::Margin::symmetric(12, 4)))
            .show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("tools-folders")
            .resizable(true)
            .default_size(theme::SIDEBAR - 40.0)
            .min_size(160.0)
            .frame(bar.inner_margin(egui::Margin::symmetric(8, 8)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.folders(ui));
            });
        egui::Panel::right("tools-editor")
            .resizable(true)
            .default_size(theme::PANEL + 60.0)
            .min_size(theme::PANEL + 40.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::egui_color(theme::SURFACE_1))
                    .inner_margin(egui::Margin::symmetric(16, 12)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.editor(ui));
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::egui_color(theme::BG))
                    .inner_margin(egui::Margin::symmetric(12, 10)),
            )
            .show(ui, |ui| self.table(ui));
        // An edit is finished once no field is being typed in or dragged.
        let ctx = ui.ctx();
        if ctx.memory(|m| m.focused().is_none()) && ctx.dragged_id().is_none() {
            self.save_library();
        }
    }

    /// The library's name and count, the filter, and the two ways to add tools.
    /// A question waiting on an answer takes the row over, so it is not missed.
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        if self.closing {
            widgets::diagnostic(ui, Level::Warn, "There are unsaved changes.", |ui| {
                if widgets::button(ui, Variant::Danger, Size::Sm, None, "Discard and close")
                    .clicked()
                {
                    self.library.revert();
                    self.closing = false;
                    self.open = false;
                }
                if widgets::button(ui, Variant::Secondary, Size::Sm, None, "Keep editing").clicked()
                {
                    self.closing = false;
                }
            });
            ui.add_space(theme::SPACE_3);
        }
        if let Some(path) = self.confirm_delete.clone() {
            let n = self.library.edited.at(&path).map_or(0, Folder::count);
            let tools = if n == 1 { "tool" } else { "tools" };
            let question = format!(
                "Delete the folder {} and the {n} {tools} in it?",
                folder_label(&path)
            );
            widgets::diagnostic(ui, Level::Warn, &question, |ui| {
                if widgets::button(ui, Variant::Danger, Size::Sm, Some(Icon::Trash), "Delete")
                    .clicked()
                {
                    self.delete_folder(&path);
                    self.confirm_delete = None;
                }
                if widgets::button(ui, Variant::Secondary, Size::Sm, None, "Keep it").clicked() {
                    self.confirm_delete = None;
                }
            });
            ui.add_space(theme::SPACE_3);
        }
        ui.horizontal(|ui| {
            ui.label(
                widgets::medium("Tool library", theme::TEXT_MD)
                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                    .color(theme::egui_color(theme::TEXT_1)),
            );
            widgets::tag(ui, &self.library.edited.count().to_string())
                .on_hover_text("tools in the library");
            ui.add_space(theme::SPACE_5);
            ui.label(
                Icon::Search
                    .text(16.0)
                    .color(theme::egui_color(theme::TEXT_3)),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Filter by name or description")
                    .desired_width(260.0),
            );
            if !self.filter.is_empty()
                && widgets::icon_button(ui, Icon::X, "clear the filter").clicked()
            {
                self.filter.clear();
            }
            let locked = is_system(&self.folder);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(!locked, |ui| {
                    if widgets::button(ui, Variant::Primary, Size::Md, Some(Icon::Plus), "New tool")
                        .on_hover_text("a new tool in the folder showing")
                        .on_disabled_hover_text(LOCKED)
                        .clicked()
                    {
                        self.new_tool();
                    }
                    if widgets::button(
                        ui,
                        Variant::Secondary,
                        Size::Md,
                        Some(Icon::Upload),
                        "Import…",
                    )
                    .on_hover_text(
                        "Import a Fusion tool library exported as CSV, as makers publish them, \
                             into the folder showing",
                    )
                    .on_disabled_hover_text(LOCKED)
                    .clicked()
                    {
                        self.import_requested = true;
                    }
                });
            });
        });
    }

    /// Where the library is kept and whether it is saved, and the last thing that
    /// happened, on one quiet line at the bottom.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (level, text) = if let Some(e) = &self.library_error {
                (Level::Err, format!("not read: {e}"))
            } else if let Some((why, _)) = &self.library_unsaved {
                (Level::Err, format!("not saved: {why}"))
            } else {
                (Level::Ok, "saved as you go".to_string())
            };
            widgets::status_dot(ui, level.color(), true);
            let color = if level == Level::Ok {
                theme::egui_color(theme::TEXT_3)
            } else {
                Level::Err.color()
            };
            ui.add(egui::Label::new(egui::RichText::new(text).color(color)).truncate());
            if let Some(p) = &self.library_path {
                ui.label(
                    widgets::mono(short_path(p), theme::TEXT_XS)
                        .color(theme::egui_color(theme::TEXT_4)),
                )
                .on_hover_text(p.display().to_string());
            }
            if let Some((ok, text)) = &self.message {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let color = if *ok {
                        theme::egui_color(theme::TEXT_2)
                    } else {
                        Level::Err.color()
                    };
                    ui.add(egui::Label::new(egui::RichText::new(text).color(color)).truncate());
                });
            }
        });
    }

    /// The folder tree, with the top of the library first.
    fn folders(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(theme::SPACE_2);
            ui.label(widgets::section_label("Folders")).on_hover_text(
                "Drag a tool onto a folder to move it there. Right-click a folder for more.",
            );
            let locked = is_system(&self.folder);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r = ui.add_enabled_ui(!locked, |ui| {
                    widgets::icon_button(
                        ui,
                        Icon::FolderPlus,
                        "a new folder inside the one selected",
                    )
                    .on_disabled_hover_text(LOCKED)
                });
                if r.inner.clicked() {
                    let parent = self.folder.clone();
                    self.new_folder(&parent);
                }
            });
        });
        ui.add_space(theme::SPACE_2);
        let mut actions = Vec::new();
        ui.spacing_mut().item_spacing.y = 0.0;
        folder_row(
            ui,
            &self.library.edited,
            &mut Vec::new(),
            &self.folder,
            &mut self.renaming,
            &mut actions,
        );
        for action in actions {
            self.apply(action);
        }
    }

    fn apply(&mut self, action: TreeAction) {
        match &action {
            TreeAction::NewIn(p)
            | TreeAction::StartRename(p)
            | TreeAction::Rename(p, _)
            | TreeAction::Delete(p)
                if is_system(p) =>
            {
                self.message = Some((false, LOCKED.into()));
                return;
            }
            _ => {}
        }
        match action {
            TreeAction::Select(path) => {
                if !is_system(&path) {
                    self.own_folder.clone_from(&path);
                }
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
                        moved(&mut self.own_folder);
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
                self.own_folder = path.clone();
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

    /// Move a tool to folder `to`; a System library bit is copied there instead.
    fn move_tool(&mut self, from: &[String], name: &str, to: &[String]) {
        if from == to {
            return;
        }
        if is_system(to) {
            self.message = Some((false, LOCKED.into()));
            return;
        }
        if is_system(from) {
            self.copy_tool(from, name, to);
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
        let all_folders = self.library.edited.folder_paths();
        // Where we are, and whether the folders inside show too.
        ui.horizontal(|ui| {
            breadcrumb(ui, &self.folder);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.checkbox(&mut self.include_subfolders, "Include subfolders");
            });
        });
        self.selection_bar(ui, &all_folders);

        let Some(here) = self.library.edited.at(&self.folder) else {
            self.folder = Vec::new();
            return;
        };
        let mut rows = here.find(self.include_subfolders, &self.filter);
        if rows.is_empty() {
            self.empty_table(ui);
            return;
        }
        match self.sort {
            Sort::Number => rows.sort_by_key(|(f, n, t)| (t.number, *n, f.clone())),
            Sort::Name => rows.sort_by_key(|(f, n, _)| (*n, f.clone())),
            Sort::Diameter => rows.sort_by(|a, b| a.2.diameter.total_cmp(&b.2.diameter)),
        }
        let mut sort = self.sort;
        let actions = self.tool_rows(ui, rows, &all_folders, &mut sort);
        self.sort = sort;
        for action in actions {
            self.apply_row(action);
        }
    }

    /// What can be done to the tool selected, only while one is.
    fn selection_bar(&mut self, ui: &mut egui::Ui, all_folders: &[FolderPath]) {
        let selected = self.selected.clone();
        let has_tools = self
            .library
            .edited
            .at(&self.folder)
            .is_some_and(|f| f.count() > 0);
        ui.horizontal(|ui| {
            ui.set_min_height(theme::CONTROL_MD);
            let Some((folder, name)) = &selected else {
                if !has_tools {
                    return;
                }
                ui.label(
                    egui::RichText::new(
                        "Select a tool to edit it; drag it onto a folder to move it.",
                    )
                    .color(theme::egui_color(theme::TEXT_4)),
                );
                return;
            };
            ui.label(
                widgets::mono(name.as_str(), theme::TEXT_SM)
                    .color(theme::egui_color(theme::TEXT_1)),
            );
            ui.add_space(theme::SPACE_3);
            if is_system(folder) {
                if widgets::button(ui, Variant::Ghost, Size::Sm, Some(Icon::Copy), COPY_OUT)
                    .on_hover_text(copy_out_hint(&self.own_folder()))
                    .clicked()
                {
                    self.duplicate(folder, name);
                }
                return;
            }
            if widgets::button(ui, Variant::Ghost, Size::Sm, Some(Icon::Copy), "Duplicate")
                .clicked()
            {
                self.duplicate(folder, name);
            }
            let moving = widgets::button(
                ui,
                Variant::Ghost,
                Size::Sm,
                Some(Icon::FolderInput),
                "Move to…",
            );
            egui::Popup::menu(&moving).show(|ui| {
                for path in all_folders {
                    if path != folder && !is_system(path) && ui.button(folder_label(path)).clicked()
                    {
                        self.move_tool(folder, name, path);
                        ui.close();
                    }
                }
            });
            if widgets::button(ui, Variant::Danger, Size::Sm, Some(Icon::Trash), "Delete").clicked()
            {
                self.delete(folder, name);
            }
        });
        ui.add_space(theme::SPACE_2);
    }

    /// What shows in place of the table when no tool is listed: why, and the ways
    /// to add one.
    fn empty_table(&mut self, ui: &mut egui::Ui) {
        let (title, detail) = if self.filter.is_empty() {
            (
                "No tools here",
                "Add one, or import a maker's Fusion library.",
            )
        } else {
            (
                "Nothing matches",
                "No tool here has that in its name or description.",
            )
        };
        let mut new = false;
        let mut import = false;
        let locked = is_system(&self.folder);
        widgets::empty_state(ui, Icon::Folder, title, detail, |ui| {
            if locked {
                return;
            }
            new = widgets::button(
                ui,
                Variant::Secondary,
                Size::Md,
                Some(Icon::Plus),
                "New tool",
            )
            .clicked();
            import = widgets::button(
                ui,
                Variant::Secondary,
                Size::Md,
                Some(Icon::Upload),
                "Import…",
            )
            .clicked();
        });
        if new {
            self.new_tool();
        }
        self.import_requested |= import;
    }

    /// The table of `rows`, whose headers set `sort` when clicked, and what was
    /// done to the rows.
    fn tool_rows(
        &self,
        ui: &mut egui::Ui,
        rows: Vec<(FolderPath, &str, &ToolSpec)>,
        all_folders: &[FolderPath],
        sort: &mut Sort,
    ) -> Vec<RowAction> {
        let m = self.measure;
        // Which folder a tool is in, only when some are in folders further down.
        let with_folder = rows.iter().any(|(sub, _, _)| !sub.is_empty());
        let mut actions = Vec::new();

        let mut table = tool_table(ui, "tools-table", egui::Sense::click_and_drag())
            .column(egui_extras::Column::exact(44.0))
            .column(
                egui_extras::Column::initial(116.0)
                    .at_least(72.0)
                    .clip(true),
            );
        if with_folder {
            table = table.column(egui_extras::Column::initial(64.0).at_least(40.0).clip(true));
        }
        table
            .column(egui_extras::Column::initial(68.0).at_least(56.0).clip(true))
            .column(egui_extras::Column::exact(56.0))
            .column(egui_extras::Column::exact(48.0))
            .column(egui_extras::Column::exact(56.0))
            .column(egui_extras::Column::exact(84.0))
            .column(egui_extras::Column::remainder().at_least(60.0).clip(true))
            .header(theme::CONTROL_MD, |mut header| {
                header.col(|ui| {
                    ui.add_space(theme::SPACE_3);
                    sort_header(ui, "T", None, Some(Sort::Number), sort, false);
                });
                header.col(|ui| sort_header(ui, "Name", None, Some(Sort::Name), sort, false));
                if with_folder {
                    header.col(|ui| sort_header(ui, "In", None, None, sort, false));
                }
                header.col(|ui| sort_header(ui, "Type", None, None, sort, false));
                header.col(|ui| {
                    sort_header(ui, "Ø", Some(m.unit()), Some(Sort::Diameter), sort, true);
                });
                header.col(|ui| sort_header(ui, "Flutes", None, None, sort, true));
                header.col(|ui| sort_header(ui, "Presets", None, None, sort, true));
                header.col(|ui| {
                    sort_header(ui, "Stickout", Some(m.unit()), None, sort, true);
                });
                header.col(|ui| sort_header(ui, "Description", None, None, sort, false));
            })
            .body(|mut body| {
                for tool in rows {
                    self.tool_row(&mut body, tool, with_folder, all_folders, &mut actions);
                }
            });
        actions
    }

    /// One tool's row: its cells, then what a click, a drag or its menu does.
    fn tool_row(
        &self,
        body: &mut egui_extras::TableBody<'_>,
        (sub, name, tool): (FolderPath, &str, &ToolSpec),
        with_folder: bool,
        all_folders: &[FolderPath],
        actions: &mut Vec<RowAction>,
    ) {
        let m = self.measure;
        let right = egui::Layout::right_to_left(egui::Align::Center);
        let mut folder = self.folder.clone();
        folder.extend(sub.iter().cloned());
        let is = self
            .selected
            .as_ref()
            .is_some_and(|(f, n)| *f == folder && n == name);
        body.row(theme::CONTROL_MD, |mut row| {
            row.set_selected(is);
            row.col(|ui| number_cell(ui, tool.number, is));
            row.col(|ui| name_cell(ui, name, is));
            if with_folder {
                row.col(|ui| {
                    ui.add(dim(sub.join("/")).selectable(false));
                });
            }
            row.col(|ui| {
                ui.add(dim(kind_name(tool.kind).to_string()).selectable(false));
            });
            row.col(|ui| {
                ui.with_layout(right, |ui| ui.add(number(m.number(tool.diameter, 2))));
            });
            row.col(|ui| {
                ui.with_layout(right, |ui| {
                    ui.add(number(
                        tool.flutes.map_or_else(|| "–".into(), |f| f.to_string()),
                    ))
                });
            });
            row.col(|ui| {
                ui.with_layout(right, |ui| match tool.presets.len() {
                    0 => ui.add(dim("–".into())),
                    n => ui.add(number(n.to_string())).on_hover_text(
                        tool.presets
                            .iter()
                            .map(|p| p.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
                });
            });
            row.col(|ui| {
                ui.with_layout(right, |ui| {
                    ui.add(number(
                        tool.stickout.map_or_else(|| "–".into(), |s| m.number(s, 1)),
                    ))
                });
            });
            row.col(|ui| {
                ui.add(dim(tool.comment.clone()).selectable(false));
            });

            // The row both clicks and drags onto a folder.
            let r = row.response();
            let drag = ToolDrag {
                folder: folder.clone(),
                name: name.to_string(),
            };
            r.dnd_set_drag_payload(drag);
            if r.dragged() {
                widgets::drag_label(&r.ctx, name, egui::FontId::monospace(theme::TEXT_SM));
            }
            if r.clicked() || r.secondary_clicked() {
                actions.push(RowAction::Select(folder.clone(), name.to_string()));
            }
            r.context_menu(|ui| {
                if is_system(&folder) {
                    if ui.button(COPY_OUT).clicked() {
                        actions.push(RowAction::Duplicate(folder.clone(), name.to_string()));
                        ui.close();
                    }
                    return;
                }
                if ui.button("Duplicate").clicked() {
                    actions.push(RowAction::Duplicate(folder.clone(), name.to_string()));
                    ui.close();
                }
                ui.menu_button("Move to", |ui| {
                    for path in all_folders {
                        if *path != folder
                            && !is_system(path)
                            && ui.button(folder_label(path)).clicked()
                        {
                            actions.push(RowAction::Move(
                                folder.clone(),
                                name.to_string(),
                                path.clone(),
                            ));
                            ui.close();
                        }
                    }
                });
                ui.separator();
                if ui.button(widgets::error_text("Delete")).clicked() {
                    actions.push(RowAction::Delete(folder.clone(), name.to_string()));
                    ui.close();
                }
            });
        });
    }

    fn apply_row(&mut self, action: RowAction) {
        match action {
            RowAction::Select(folder, name) => self.select(Some((folder, name))),
            RowAction::Move(from, name, to) => self.move_tool(&from, &name, &to),
            RowAction::Duplicate(folder, name) => self.duplicate(&folder, &name),
            RowAction::Delete(folder, name) => self.delete(&folder, &name),
        }
    }

    fn new_tool(&mut self) {
        if is_system(&self.folder) {
            self.message = Some((false, LOCKED.into()));
            return;
        }
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

    /// A copy of a tool beside it; a System library bit's copy goes to the
    /// person's own folder instead.
    fn duplicate(&mut self, folder: &[String], name: &str) {
        if is_system(folder) {
            let to = self.own_folder();
            self.copy_tool(folder, name, &to);
            return;
        }
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

    /// The folder a System library bit is copied to: the last of the person's own
    /// shown, while it is still there, or else the top of the library.
    fn own_folder(&self) -> FolderPath {
        if !is_system(&self.own_folder) && self.library.edited.at(&self.own_folder).is_some() {
            self.own_folder.clone()
        } else {
            Vec::new()
        }
    }

    /// Copy tool `name` of `from` into folder `to` under a free name and the next
    /// tool number, and show it there.
    fn copy_tool(&mut self, from: &[String], name: &str, to: &[String]) {
        let Some(tool) = self
            .library
            .edited
            .at(from)
            .and_then(|f| f.tools.get(name))
            .cloned()
        else {
            return;
        };
        let number = self.library.next_number();
        let Some(there) = self.library.edited.at_mut(to) else {
            return;
        };
        let as_name = free_name(name, &there.tools);
        there
            .tools
            .insert(as_name.clone(), ToolSpec { number, ..tool });
        let said = if as_name == name {
            format!("copied '{name}' to {} as T{number}", folder_label(to))
        } else {
            format!(
                "copied '{name}' to {} as '{as_name}', T{number}",
                folder_label(to)
            )
        };
        self.message = Some((true, said));
        self.folder = to.to_vec();
        self.own_folder = to.to_vec();
        self.select(Some((to.to_vec(), as_name)));
    }

    fn delete(&mut self, folder: &[String], name: &str) {
        if is_system(folder) {
            self.message = Some((false, LOCKED.into()));
            return;
        }
        if let Some(here) = self.library.edited.at_mut(folder) {
            here.tools.remove(name);
        }
        self.select(None);
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let Some((folder, name)) = self.selected.clone() else {
            widgets::empty_state(
                ui,
                Icon::Search,
                "No tool selected",
                "Pick one from the list to see and edit it.",
                |_| {},
            );
            return;
        };
        let Some(original) = self.selected_tool().cloned() else {
            self.select(None);
            return;
        };

        // Which tool this is: its folder, then its name, edited in place and
        // applied when editing finishes, beside its number.
        let locked = is_system(&folder);
        breadcrumb(ui, &folder);
        ui.horizontal(|ui| {
            let r = ui.add_enabled(
                !locked,
                egui::TextEdit::singleline(&mut self.name_buffer)
                    .font(egui::FontId::new(
                        theme::TEXT_MD,
                        egui::FontFamily::Name(theme::SEMIBOLD.into()),
                    ))
                    .desired_width(ui.available_width() - 64.0),
            );
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
            widgets::tag(ui, &format!("T{}", original.number)).on_hover_text("its tool number");
        });
        if locked {
            ui.add_space(theme::SPACE_2);
            ui.horizontal(|ui| {
                ui.label(widgets::hint_text(
                    "Part of the System library, which can't be changed.",
                ));
                if widgets::button(ui, Variant::Secondary, Size::Sm, Some(Icon::Copy), COPY_OUT)
                    .on_hover_text(copy_out_hint(&self.own_folder()))
                    .clicked()
                {
                    self.duplicate(&folder, &name);
                }
            });
        }
        ui.add_space(theme::SPACE_3);
        paint_tool(ui, &original, self.measure);
        ui.add_space(theme::SPACE_3);

        let mut tool = original.clone();
        tool_fields(ui, &mut tool, self.measure, !locked);
        // The edit lands under whatever name the tool has now.
        if tool != original
            && !locked
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
    let depth = path.len();
    let open_id = ui.make_persistent_id(("tool-folder", &*path));
    let mut open = ui
        .data_mut(|d| d.get_persisted::<bool>(open_id))
        .unwrap_or(true);

    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme::CONTROL_SM + 2.0),
        egui::Sense::click(),
    );
    let is = *selected == *path;
    let dropping = !is_system(path) && response.dnd_hover_payload::<ToolDrag>().is_some();
    list_row_background(ui, rect, is, response.hovered());
    if dropping {
        ui.painter().rect_stroke(
            rect,
            theme::RADIUS_SM,
            egui::Stroke::new(1.5, theme::egui_color(theme::ACCENT)),
            egui::StrokeKind::Inside,
        );
    }
    // Truncating a float of a few folders deep to pixels.
    #[allow(clippy::cast_precision_loss)]
    let indent = theme::SPACE_3 + depth as f32 * theme::SPACE_5;
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(
                rect.with_min_x(rect.left() + indent)
                    .shrink2(egui::vec2(0.0, 1.0)),
            )
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row.spacing_mut().item_spacing.x = theme::SPACE_2;
    // The open-close chevron, where there is something to open.
    if folder.folders.is_empty() {
        row.add_space(14.0 + theme::SPACE_2);
    } else {
        let chevron = if open {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        };
        let r = row.add(
            egui::Label::new(chevron.text(14.0).color(theme::egui_color(theme::TEXT_3)))
                .sense(egui::Sense::click()),
        );
        if r.clicked() {
            open = !open;
            ui.data_mut(|d| d.insert_persisted(open_id, open));
        }
    }
    folder_name(&mut row, folder, path, is, renaming, actions);
    folder_actions(&response, path, actions);

    if open {
        // The System library first, above the person's own folders.
        let mut subs: Vec<_> = folder.folders.iter().collect();
        if path.is_empty() {
            subs.sort_by_key(|(name, _)| name.as_str() != SYSTEM);
        }
        for (name, sub) in subs {
            path.push(name.clone());
            folder_row(ui, sub, path, selected, renaming, actions);
            path.pop();
        }
    }
}

/// A folder's name and how many tools it holds, or the field renaming it.
fn folder_name(
    row: &mut egui::Ui,
    folder: &Folder,
    path: &FolderPath,
    is: bool,
    renaming: &mut Option<(FolderPath, String)>,
    actions: &mut Vec<TreeAction>,
) {
    let editing = renaming.as_ref().is_some_and(|(at, _)| at == path);
    if editing {
        if let Some((_, buffer)) = renaming.as_mut() {
            let r = row.add(egui::TextEdit::singleline(buffer).desired_width(140.0));
            if !r.has_focus() && !r.lost_focus() {
                r.request_focus();
            }
            if r.lost_focus() {
                if row.input(|i| i.key_pressed(egui::Key::Escape)) {
                    actions.push(TreeAction::CancelRename);
                } else {
                    actions.push(TreeAction::Rename(path.clone(), buffer.clone()));
                }
            }
        }
    } else {
        let name = path.last().map_or("Library", String::as_str);
        let color = if is { theme::TEXT_1 } else { theme::TEXT_2 };
        row.add(
            egui::Label::new(egui::RichText::new(name).color(theme::egui_color(color)))
                .truncate()
                .selectable(false),
        );
        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(theme::SPACE_3);
            ui.label(
                widgets::mono(folder.count().to_string(), theme::TEXT_XS)
                    .color(theme::egui_color(theme::TEXT_4)),
            );
        });
    }
}

/// What dropping a tool on a folder's row, clicking it, and its menu do.
fn folder_actions(response: &egui::Response, path: &FolderPath, actions: &mut Vec<TreeAction>) {
    if response.clicked() {
        actions.push(TreeAction::Select(path.clone()));
    }
    // The System library takes no tools, and has no names or folders to change.
    if is_system(path) {
        if path.len() == 1 {
            response.clone().on_hover_text(
                "Common bits, built in and read-only. Copy one into your own folders                  to change it.",
            );
        }
        return;
    }
    if let Some(drag) = response.dnd_release_payload::<ToolDrag>() {
        actions.push(TreeAction::Drop((*drag).clone(), path.clone()));
    }
    if response.double_clicked() && !path.is_empty() {
        actions.push(TreeAction::StartRename(path.clone()));
    }
    response.context_menu(|ui| {
        if ui.button("New folder inside").clicked() {
            actions.push(TreeAction::NewIn(path.clone()));
            ui.close();
        }
        if !path.is_empty() {
            if ui.button("Rename").clicked() {
                actions.push(TreeAction::StartRename(path.clone()));
                ui.close();
            }
            ui.separator();
            if ui
                .button(egui::RichText::new("Delete").color(theme::egui_color(theme::ERR)))
                .clicked()
            {
                actions.push(TreeAction::Delete(path.clone()));
                ui.close();
            }
        }
    });
}

/// Where a folder is: `Library / machine_01 / flat`, the last part brightest.
fn breadcrumb(ui: &mut egui::Ui, path: &[String]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SPACE_2;
        let parts = std::iter::once("Library").chain(path.iter().map(String::as_str));
        let last = path.len();
        for (i, part) in parts.enumerate() {
            if i > 0 {
                ui.label(
                    widgets::mono("/", theme::TEXT_SM).color(theme::egui_color(theme::TEXT_4)),
                );
            }
            let color = if i == last {
                theme::TEXT_2
            } else {
                theme::TEXT_3
            };
            ui.label(widgets::mono(part, theme::TEXT_SM).color(theme::egui_color(color)));
        }
    });
}

/// A library path, with the home folder as `~`, to fit on the status line.
fn short_path(path: &Path) -> String {
    let full = path.display().to_string();
    match std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        Some(home) => {
            let home = home.to_string_lossy();
            match full.strip_prefix(home.as_ref()) {
                Some(rest) if !home.is_empty() => format!("~{rest}"),
                _ => full,
            }
        }
        None => full,
    }
}

/// A header that sorts the table by `key`, when it has one, on a click.
fn sort_header(
    ui: &mut egui::Ui,
    label: &str,
    unit: Option<&str>,
    key: Option<Sort>,
    sort: &mut Sort,
    right: bool,
) {
    if column_header(ui, label, unit, key.map(|k| k == *sort), right)
        && let Some(k) = key
    {
        *sort = k;
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

    /// A maker's Fusion library lands in the folder showing, selected, and said
    /// so; a file that is not one is said to be not one.
    #[test]
    fn a_fusion_library_is_imported_into_the_folder_showing() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/tools");
        let mut m = manager("import");
        let folder = m.library.edited.create(&[], "onefinity").unwrap();
        m.folder = folder.clone();
        m.import_fusion(&fixtures.join("srf4-250up.csv"));
        let tool = &m.library.edited.at(&folder).unwrap().tools["SRF4-250UP"];
        assert_eq!(tool.presets.len(), 4);
        assert_eq!(m.selected, Some((folder, "SRF4-250UP".to_string())));
        assert!(matches!(&m.message, Some((true, t)) if t.contains("added SRF4-250UP")));

        m.import_fusion(&fixtures.join("../step/sled_base.step"));
        assert!(matches!(&m.message, Some((false, _))), "{:?}", m.message);
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
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains(SYSTEM),
            "the System library is not saved: {text}"
        );
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
        assert_eq!(
            m.library.edited.at(&[SYSTEM.into()]).map(Folder::count),
            Some(20),
            "the System library is there all the same"
        );
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

    fn system(sub: &str) -> FolderPath {
        vec![SYSTEM.to_string(), sub.to_string()]
    }

    /// A library with nothing in it yet has the System library, and nothing to
    /// save.
    #[test]
    fn a_new_library_has_the_system_bits() {
        let m = manager("fresh");
        assert_eq!(m.library.edited.count(), 20);
        assert!(!m.dirty());
        assert_eq!(
            m.library.next_number(),
            1,
            "its numbers are not the person's"
        );
    }

    /// Nothing in the System library can be deleted, renamed, moved or added to;
    /// a bit moved out of it is copied.
    #[test]
    fn the_system_library_cannot_be_changed() {
        let mut m = manager("locked");
        let mine = m.library.edited.create(&[], "machine_01").unwrap();
        let ends = system("End mills");
        let before = m.library.edited.at(&[SYSTEM.into()]).unwrap().clone();

        m.delete(&ends, "1/4 upcut");
        m.apply(TreeAction::Delete(ends.clone()));
        m.apply(TreeAction::Rename(ends.clone(), "mine".into()));
        m.apply(TreeAction::NewIn(ends.clone()));
        m.folder = ends.clone();
        m.new_tool();
        m.move_tool(&mine, "nothing", &ends);
        m.move_tool(&ends, "1/4 upcut", &mine);
        assert!(
            matches!(&m.message, Some((true, t)) if t.contains("copied")),
            "{:?}",
            m.message
        );

        assert_eq!(m.library.edited.at(&[SYSTEM.into()]).unwrap(), &before);
        assert!(
            m.library
                .edited
                .at(&mine)
                .unwrap()
                .tools
                .contains_key("1/4 upcut")
        );
    }

    /// Copying a bit out puts it, numbered as the person's next, in the folder of
    /// theirs last shown, and shows it there to be changed; and it is saved.
    #[test]
    fn a_system_bit_is_copied_to_the_folder_last_shown() {
        let dir = std::env::temp_dir().join(format!("camcraft-tm-copy-{}", std::process::id()));
        let path = dir.join("library.toml");
        let _ = std::fs::remove_file(&path);
        let mut m = ToolManager::with_library(path.clone());
        m.library.edited.tools.insert(
            "mine".into(),
            ToolSpec {
                number: 4,
                ..ToolSpec::default()
            },
        );
        let folder = m.library.edited.create(&[], "machine_01").unwrap();
        m.apply(TreeAction::Select(folder.clone()));
        let balls = system("Ball");
        m.apply(TreeAction::Select(balls.clone()));
        m.duplicate(&balls, "1/8 ball");
        m.duplicate(&balls, "1/8 ball");

        let here = &m.library.edited.at(&folder).unwrap().tools;
        assert_eq!(here["1/8 ball"].number, 5);
        assert_eq!(here["1/8 ball-2"].number, 6);
        assert_eq!(here["1/8 ball"].presets.len(), 4);
        assert_eq!(m.folder, folder);
        assert_eq!(m.selected, Some((folder.clone(), "1/8 ball-2".into())));

        m.save_library();
        assert!(!m.dirty());
        let saved = Library::load(&path).unwrap();
        assert_eq!(saved.root.at(&folder).unwrap().tools.len(), 2);
        assert!(saved.root.at(&[SYSTEM.into()]).is_none());
    }
}
