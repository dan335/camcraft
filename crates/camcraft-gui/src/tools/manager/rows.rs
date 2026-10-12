//! The manager's table of tools: where it is, what can be done to the tool
//! selected, and a row for each tool, sorted by the column chosen.

use camcraft_core::job::ToolSpec;
use camcraft_session::library::system::is_system;
use camcraft_session::library::{FolderPath, folder_label};
use eframe::egui;

use super::{COPY_OUT, ToolDrag, ToolManager, breadcrumb, copy_out_hint};
use crate::tools::drawing::kind_name;
use crate::tools::table::{column_header, dim, name_cell, number, number_cell, tool_table};
use crate::ui::theme;
use crate::ui::widgets::{self, Icon, Size, Variant};

/// How the table is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Sort {
    Number,
    Name,
    Diameter,
}

/// Something done to a tool's row, applied once the table has been drawn.
enum RowAction {
    Select(FolderPath, String),
    Move(FolderPath, String, FolderPath),
    Duplicate(FolderPath, String),
    Delete(FolderPath, String),
}

impl ToolManager {
    pub(super) fn table(&mut self, ui: &mut egui::Ui) {
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
        // Compared in place: a key would copy a folder path per comparison, every
        // frame.
        match self.sort {
            Sort::Number => rows.sort_by(|(fa, na, ta), (fb, nb, tb)| {
                (ta.number, na, fa).cmp(&(tb.number, nb, fb))
            }),
            Sort::Name => rows.sort_by(|(fa, na, _), (fb, nb, _)| (na, fa).cmp(&(nb, fb))),
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
