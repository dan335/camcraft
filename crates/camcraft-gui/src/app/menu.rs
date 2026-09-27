//! The top of the window and the window with no job: the menu bar, the job tabs,
//! and the start screen.

use eframe::egui;

use super::post::post_blocker;
use super::{App, log_color};
use crate::browser;
use crate::document::Selection;
use crate::file_dialog::FileRequest;
use crate::ui::theme;
use crate::ui::widgets;

impl App {
    pub(super) fn menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let id = self.doc().map(|d| d.id);
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New").clicked() {
                    self.new_job();
                    ui.close();
                }
                if ui.button("Open…").clicked() {
                    self.dialog.open(FileRequest::OpenJobs, "");
                    ui.close();
                }
                ui.add_enabled_ui(!self.recent.is_empty(), |ui| {
                    ui.menu_button("Open Recent", |ui| {
                        for path in self.recent.clone() {
                            if ui.button(path.display().to_string()).clicked() {
                                self.open_job(&ctx, path);
                                ui.close();
                            }
                        }
                    });
                });
                ui.separator();
                ui.add_enabled_ui(id.is_some(), |ui| {
                    if ui.button("Save").clicked()
                        && let Some(id) = id
                    {
                        self.save_job(id, None);
                        ui.close();
                    }
                    if ui.button("Save As…").clicked()
                        && let Some(id) = id
                    {
                        self.save_as(id);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Close").clicked()
                        && let Some(id) = id
                    {
                        self.close(id);
                        ui.close();
                    }
                });
            });
            ui.menu_button("Edit", |ui| {
                let (undo, redo) = self
                    .doc()
                    .map_or((false, false), |d| (d.can_undo(), d.can_redo()));
                if ui.add_enabled(undo, egui::Button::new("Undo")).clicked() {
                    if let Some(d) = self.doc_mut() {
                        d.undo();
                    }
                    ui.close();
                }
                if ui.add_enabled(redo, egui::Button::new("Redo")).clicked() {
                    if let Some(d) = self.doc_mut() {
                        d.redo();
                    }
                    ui.close();
                }
            });
            ui.menu_button("Setup", |ui| {
                let active = self.doc().and_then(|d| d.active.clone());
                if ui
                    .add_enabled(active.is_some(), egui::Button::new("Generate"))
                    .clicked()
                    && let Some(name) = active
                {
                    self.generate(&ctx, &name, None);
                    ui.close();
                }
                if ui
                    .add_enabled(id.is_some(), egui::Button::new("Generate all"))
                    .clicked()
                {
                    self.generate_all(&ctx);
                    ui.close();
                }
                ui.separator();
                let Some(doc) = self.docs.get(self.active) else {
                    return;
                };
                let mut post = None;
                if let Some(name) = doc.active.clone() {
                    let why = post_blocker(doc, std::slice::from_ref(&name));
                    if ui
                        .add_enabled(why.is_none(), egui::Button::new("Post…"))
                        .on_hover_text("write its programs, one per operation, into a folder")
                        .on_disabled_hover_text(why.unwrap_or_default())
                        .clicked()
                    {
                        post = Some((doc.id, Some(name)));
                    }
                }
                let names: Vec<String> = doc.job.setups.iter().map(|s| s.name.clone()).collect();
                let why = post_blocker(doc, &names);
                if ui
                    .add_enabled(why.is_none(), egui::Button::new("Post all…"))
                    .on_hover_text("every setup's programs, or none")
                    .on_disabled_hover_text(why.unwrap_or_default())
                    .clicked()
                {
                    post = Some((doc.id, None));
                }
                if let Some((id, scope)) = post {
                    self.dialog.open(FileRequest::PostTo(id, scope), "");
                    ui.close();
                }
            });
            ui.menu_button("Add", |ui| self.add_menu(ui));
            ui.menu_button("View", |ui| {
                if ui.checkbox(&mut self.tools.open, "Tool library").clicked() {
                    ui.close();
                }
                if ui.checkbox(&mut self.checks_open, "Checks").clicked() {
                    ui.close();
                }
                if ui.checkbox(&mut self.log_open, "Log").clicked() {
                    ui.close();
                }
                if ui.checkbox(&mut self.settings_open, "Settings").clicked() {
                    ui.close();
                }
            });
        });
    }

    /// Add to the showing job: models, a stock, a setup, or an operation to the
    /// selected setup (or the one showing), from its group: 2D, 3D or drilling.
    fn add_menu(&mut self, ui: &mut egui::Ui) {
        let Some(doc) = self.docs.get_mut(self.active) else {
            ui.label(widgets::hint_text("Open or start a job first."));
            return;
        };
        let id = doc.id;
        if ui.button("Model…").clicked() {
            self.dialog.open(FileRequest::AddModels(id, None), "");
            ui.close();
            return;
        }
        if ui.button("Stock").clicked() {
            doc.add_stock();
            ui.close();
        }
        if ui.button("Setup").clicked() {
            doc.add_setup();
            ui.close();
        }
        let target = match &doc.selection {
            Selection::Setup(s) | Selection::Operation(s, _) => Some(s.clone()),
            _ => doc.active.clone(),
        };
        ui.separator();
        let menu = ui.add_enabled_ui(target.is_some(), |ui| {
            browser::operation_menus(ui, |strategy| {
                if let Some(setup) = &target {
                    doc.add_operation(setup, strategy);
                }
            });
        });
        match &target {
            Some(setup) => menu.response.on_hover_text(format!("to setup '{setup}'")),
            None => menu.response.on_disabled_hover_text("the job has no setup"),
        };
    }

    pub(super) fn tabs(&mut self, ui: &mut egui::Ui) {
        let mut close = None;
        ui.horizontal(|ui| {
            for (i, doc) in self.docs.iter().enumerate() {
                let mut title = doc.title();
                if doc.dirty() {
                    title.push_str(" •");
                }
                let r = ui.selectable_label(i == self.active, title);
                let r = match &doc.path {
                    Some(p) => r.on_hover_text(p.display().to_string()),
                    None => r.on_hover_text("not saved yet"),
                };
                if r.clicked() {
                    self.active = i;
                }
                if widgets::icon_button(ui, widgets::Icon::X, "close this job").clicked() {
                    close = Some(doc.id);
                }
                ui.separator();
            }
            if widgets::icon_button(ui, widgets::Icon::Plus, "new job").clicked() {
                self.new_job();
            }
        });
        if let Some(id) = close {
            self.close(id);
        }
    }

    /// With no job open: what can be done.
    pub(super) fn start_screen(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.25);
            // The wordmark: Fira Code at 500, as the design system sets it where a
            // logo would go.
            ui.label(
                egui::RichText::new("camcraft")
                    .family(egui::FontFamily::Name(theme::MONO_MEDIUM.into()))
                    .size(theme::TEXT_LG)
                    .color(theme::egui_color(theme::TEXT_1)),
            );
            ui.add_space(12.0);
            if ui.button("New job").clicked() {
                self.new_job();
            }
            if ui.button("Open job…").clicked() {
                self.dialog.open(FileRequest::OpenJobs, "");
            }
            if !self.worker.idle() {
                ui.add_space(8.0);
                ui.spinner();
            }
            if !self.recent.is_empty() {
                ui.add_space(16.0);
                ui.label(widgets::section_label("Recent"));
                for path in self.recent.clone() {
                    let name = path.file_name().map_or_else(
                        || path.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    if ui
                        .link(name)
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        self.open_job(&ctx, path);
                    }
                }
            }
            for (level, text) in &self.log {
                ui.colored_label(log_color(*level), text);
            }
        });
    }
}
