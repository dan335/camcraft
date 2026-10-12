//! The manager's folder tree: a row for each folder, nested, that selects it,
//! takes a tool dropped on it, and renames, adds to or deletes it.

use camcraft_session::library::system::{SYSTEM, is_system};
use camcraft_session::library::{Folder, FolderPath};
use eframe::egui;

use super::{LOCKED, ToolDrag, ToolManager};
use crate::ui::theme;
use crate::ui::widgets::{self, Icon, list_row_background};

/// Something done in the folder tree, applied once it has been drawn.
pub(super) enum TreeAction {
    Select(FolderPath),
    NewIn(FolderPath),
    StartRename(FolderPath),
    Rename(FolderPath, String),
    CancelRename,
    Delete(FolderPath),
    Drop(ToolDrag, FolderPath),
}

impl ToolManager {
    /// The folder tree, with the top of the library first.
    pub(super) fn folders(&mut self, ui: &mut egui::Ui) {
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

    pub(super) fn apply(&mut self, action: TreeAction) {
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
                match self.library.edit().rename(&path, &to) {
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
                "Common bits, built in and read-only. Copy one into your own folders \
                 to change it.",
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
