//! The questions the app stops to ask: close or quit with unsaved changes, find
//! models that are missing, and post programs although something stands in the
//! way. Each is the design system's Dialog: a title, what it is about, and the
//! answers on the right, the one that cannot be undone in red.

use eframe::egui;

use super::App;
use crate::document::Document;
use crate::file_dialog::FileRequest;
use crate::ui::widgets::{self, Size, Variant};
use crate::worker::DocId;

/// What answering a prompt does, once the prompt is drawn.
pub(super) type Answer = Box<dyn FnOnce(&mut App)>;

/// A question the app is waiting on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Prompt {
    /// Close job `doc`, which has unsaved changes?
    Close(DocId),
    /// Quit, with unsaved changes in some jobs?
    Quit,
    /// Job `doc` names models that are not where it says.
    Missing(DocId),
    /// Post the planned programs? Something stands in the way: see `post_plan`.
    Post,
}

/// A dialog's button, at the size its footer has them.
fn answer(ui: &mut egui::Ui, variant: Variant, label: &str) -> egui::Response {
    widgets::button(ui, variant, Size::Md, None, label)
}

impl App {
    /// The prompt waiting, if one is, over everything: what answering it does is
    /// done once it is drawn. Programs rendered while another question waited
    /// are asked about once it is answered.
    pub(super) fn prompts(&mut self, ctx: &egui::Context) {
        if self.prompt.is_none() && self.post_plan.is_some() {
            self.prompt = Some(Prompt::Post);
        }
        let Some(prompt) = self.prompt.clone() else {
            return;
        };
        let mut done = false;
        let mut then: Option<Answer> = None;
        let modal = egui::Modal::new(egui::Id::new("prompt")).show(ctx, |ui| {
            ui.set_width(420.0);
            match &prompt {
                Prompt::Close(id) => self.close_prompt(ui, *id, &mut then, &mut done),
                Prompt::Quit => self.quit_prompt(ui, &mut then, &mut done),
                Prompt::Missing(id) => self.missing_prompt(ui, *id, &mut then, &mut done),
                Prompt::Post => self.post_prompt(ui, ctx, &mut then, &mut done),
            }
        });
        if let Some(f) = then {
            self.prompt = None;
            f(self);
            if prompt == Prompt::Quit && self.docs.is_empty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        } else if done || modal.should_close() {
            self.prompt = None;
            if prompt == Prompt::Post {
                self.post_plan = None;
            }
        }
    }

    /// Close a job with unsaved changes: save it first, or not.
    fn close_prompt(
        &self,
        ui: &mut egui::Ui,
        id: DocId,
        then: &mut Option<Answer>,
        done: &mut bool,
    ) {
        let title = self
            .docs
            .iter()
            .find(|d| d.id == id)
            .map(Document::title)
            .unwrap_or_default();
        widgets::dialog(
            ui,
            &format!("Save changes to {title}?"),
            |ui| {
                ui.label("They are lost if you close without saving.");
            },
            |ui| {
                if answer(ui, Variant::Primary, "Save").clicked() {
                    *then = Some(Box::new(move |app: &mut Self| {
                        app.close_after_save = Some(id);
                        app.save_job(id, None);
                    }));
                }
                if answer(ui, Variant::Danger, "Don't save").clicked() {
                    *then = Some(Box::new(move |app: &mut Self| app.remove(id)));
                }
                if answer(ui, Variant::Ghost, "Cancel").clicked() {
                    *done = true;
                }
            },
        );
    }

    /// Quit with unsaved changes: save them all first, or not.
    fn quit_prompt(&self, ui: &mut egui::Ui, then: &mut Option<Answer>, done: &mut bool) {
        let unsaved: Vec<&Document> = self.docs.iter().filter(|d| d.dirty()).collect();
        let all_have_a_place = unsaved.iter().all(|d| d.path.is_some());
        widgets::dialog(
            ui,
            "Quit with unsaved changes?",
            |ui| {
                for d in &unsaved {
                    ui.label(format!("• {}", d.title()));
                }
            },
            |ui| {
                let save = ui
                    .add_enabled_ui(all_have_a_place, |ui| {
                        answer(ui, Variant::Primary, "Save all and quit")
                    })
                    .inner
                    .on_disabled_hover_text("a new job needs a place first: save it with Save As");
                if save.clicked() {
                    *then = Some(Box::new(|app: &mut Self| {
                        let ids: Vec<DocId> = app
                            .docs
                            .iter()
                            .filter(|d| d.dirty())
                            .map(|d| d.id)
                            .collect();
                        for id in ids {
                            app.save_job(id, None);
                        }
                        if app.docs.iter().all(|d| !d.dirty()) {
                            app.docs.clear();
                        }
                    }));
                }
                if answer(ui, Variant::Danger, "Quit without saving").clicked() {
                    *then = Some(Box::new(|app: &mut Self| app.docs.clear()));
                }
                if answer(ui, Variant::Ghost, "Cancel").clicked() {
                    *done = true;
                }
            },
        );
    }

    /// A job's models that are not where it says: find each, or leave it for now.
    fn missing_prompt(
        &self,
        ui: &mut egui::Ui,
        id: DocId,
        then: &mut Option<Answer>,
        done: &mut bool,
    ) {
        let (title, missing) = self
            .docs
            .iter()
            .find(|d| d.id == id)
            .map(|d| (d.title(), d.missing_models()))
            .unwrap_or_default();
        widgets::dialog(
            ui,
            &format!("{title}: models not found"),
            |ui| {
                ui.label("These are not where the job says. Find each, or leave it for now.");
                for name in missing {
                    ui.horizontal(|ui| {
                        ui.label(&name);
                        if widgets::button(ui, Variant::Secondary, Size::Sm, None, "Find…")
                            .clicked()
                        {
                            *then = Some(Box::new(move |app: &mut Self| {
                                app.dialog.open(FileRequest::Locate(id, name), "");
                            }));
                        }
                    });
                }
            },
            |ui| {
                if answer(ui, Variant::Ghost, "Later").clicked() {
                    *done = true;
                }
            },
        );
    }

    /// Post programs although something stands in the way: setups that did not
    /// pass their checks, or files that would be replaced.
    fn post_prompt(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        then: &mut Option<Answer>,
        done: &mut bool,
    ) {
        let Some((id, plan)) = self.post_plan.as_ref() else {
            *done = true;
            return;
        };
        let title = format!(
            "Post {} program(s) to {}?",
            plan.programs.len(),
            plan.dir.display()
        );
        let go = if plan.refused.is_empty() {
            "Replace and post"
        } else {
            "Post anyway"
        };
        let id = *id;
        // Ticked in the body, read in the footer.
        let anyway = std::cell::Cell::new(self.post_anyway);
        widgets::dialog(
            ui,
            &title,
            |ui| {
                if !plan.refused.is_empty() {
                    ui.label(widgets::error_text(
                        "These setups did NOT pass their checks. Their programs are \
                         not safe to run as they stand:",
                    ));
                    egui::ScrollArea::vertical()
                        .id_salt("refused")
                        .max_height(180.0)
                        .show(ui, |ui| {
                            for (setup, why) in &plan.refused {
                                ui.strong(setup);
                                for w in why {
                                    ui.label(format!("• {w}"));
                                }
                            }
                        });
                    let mut ticked = anyway.get();
                    ui.checkbox(&mut ticked, "I know why, and want these programs anyway");
                    anyway.set(ticked);
                }
                if !plan.overwrites.is_empty() {
                    ui.label("These files are there already, and will be replaced:");
                    for p in plan.overwrites.iter().take(8) {
                        let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
                        ui.monospace(name.unwrap_or_default());
                    }
                    if plan.overwrites.len() > 8 {
                        ui.label(widgets::hint_text(format!(
                            "and {} more",
                            plan.overwrites.len() - 8
                        )));
                    }
                }
            },
            |ui| {
                let allowed = plan.refused.is_empty() || anyway.get();
                let go = ui
                    .add_enabled_ui(allowed, |ui| answer(ui, Variant::Danger, go))
                    .inner;
                if go.clicked() {
                    let ctx = ctx.clone();
                    *then = Some(Box::new(move |app: &mut Self| {
                        if let Some((_, plan)) = app.post_plan.take() {
                            app.write_post(&ctx, id, plan);
                        }
                    }));
                }
                if answer(ui, Variant::Ghost, "Cancel").clicked() {
                    *done = true;
                }
            },
        );
        self.post_anyway = anyway.get();
    }
}
