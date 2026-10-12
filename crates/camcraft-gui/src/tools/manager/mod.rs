//! The tool manager: the user's tool library, after Fusion's Tool Library.
//!
//! It opens in a window inside the app and manages only the library. A job's
//! tools are chosen inside its operations, which copy them into the job, never
//! link them, so a job still cuts what was verified after the library changes.
//!
//! Tools are grouped in folders that nest: one per machine, say. The folders are
//! on the left ([`tree`]); a tool moves between them by dragging it onto one, or
//! with "Move to". Tools are listed in a table ([`rows`]) and edited in place,
//! and the library saves itself whenever an edit is finished and every tool
//! checks clean, to the user's app-data folder (`camcraft_session::library`).
//! What an edit does to the library, apart from how it is drawn, is in
//! [`library`].
//!
//! The System library, a folder of common bits built in, shows among them. Nothing
//! in it can be changed; a bit is copied out of it into the person's own folders,
//! with a button or by dragging it onto one.

mod library;
mod rows;
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests;
mod tree;

use std::path::{Path, PathBuf};

use camcraft_core::job::ToolSpec;
#[cfg(all(test, feature = "ui-snapshots"))]
use camcraft_session::library::system::SYSTEM;
use camcraft_session::library::system::add_system;
use camcraft_session::library::system::is_system;
use camcraft_session::library::{Folder, FolderPath, Library, folder_label, library_path};
use eframe::egui;

use crate::tools::drawing::paint_tool;
use crate::tools::editor::tool_fields;
use crate::ui::measure::Measure;
use crate::ui::theme;
use crate::ui::widgets::{self, Icon, Level, Size, Variant};

use library::Set;
use rows::Sort;

/// Why nothing in the System library can be changed, and what to do instead.
const LOCKED: &str = "The System library can't be changed; pick one of your own folders, \
                      or copy a bit into one";

/// What copying a System library bit out is called.
const COPY_OUT: &str = "Copy to my library";

/// Where a System library bit's copy goes, said on the button.
fn copy_out_hint(to: &[String]) -> String {
    format!("A copy to change, in {}", folder_label(to))
}

/// A tool being dragged onto a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolDrag {
    folder: FolderPath,
    name: String,
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
    /// Why the library's last edits could not be saved: tried again once
    /// another edit lands, not every frame.
    library_unsaved: Option<String>,
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
        if let (Some(tool), Some(folder)) = (tool, self.library.edit().at_mut(&self.folder)) {
            folder.tools.insert(name.to_string(), tool);
        }
        self.select(Some((self.folder.clone(), name.to_string())));
    }

    /// Fill the library with folders for two machines and show the first. Only the
    /// snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn folders_for_snapshot(&mut self) {
        library::fill_for_snapshot(self.library.edit());
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
        if let Ok(path) = self.library.edit().create(&[], "empty") {
            self.folder = path;
        }
        self.select(None);
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
            } else if let Some(why) = &self.library_unsaved {
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
                .edit()
                .at_mut(&folder)
                .and_then(|f| f.tools.get_mut(&key))
        {
            *slot = tool;
        }
    }
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
