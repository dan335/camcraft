//! The job's tree, after Fusion's browser: Models, Stocks and Setups, each setup
//! with its operations.
//!
//! Clicking selects, and the properties panel edits what is selected. Right-click
//! adds, copies, moves and removes. Each setup shows where it stands: not run,
//! running, verified, with warnings, refused, or out of date since it ran.

use std::cell::RefCell;

use camcraft_core::job::StrategySpec;
use eframe::egui;

use crate::document::{Document, Level, ModelSlot, Selection, Status};
use crate::theme;

/// Something the tree asks the app to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserAction {
    /// Pick model files to add.
    AddModels,
    /// Find model `name`'s file.
    Locate(String),
    /// Run setup `name`.
    Generate(String),
}

/// A change to the job, applied once the tree is drawn.
enum Edit {
    Select(Selection),
    AddStock,
    AddSetup,
    DuplicateSetup(String),
    RemoveSetup(String),
    MoveSetup(String, isize),
    AddOperation(String, StrategySpec),
    DuplicateOperation(String, usize),
    RemoveOperation(String, usize),
    MoveOperation(String, usize, isize),
    RemoveModel(String),
    RemoveStock(String),
}

/// How a status reads.
pub fn status_text(status: Status) -> &'static str {
    match status {
        Status::NotRun => "not generated",
        Status::Running => "generating",
        Status::Verified => "verified",
        Status::Warnings => "verified, with warnings",
        Status::Refused => "NOT verified",
        Status::Stale => "out of date",
    }
}

fn status_color(status: Status) -> [u8; 3] {
    match status {
        Status::NotRun | Status::Running => theme::GRAY_5,
        Status::Verified => theme::PATH_CUT,
        Status::Warnings | Status::Stale => theme::AMBER_5,
        Status::Refused => theme::ERR,
    }
}

/// A dot in the status's colour: filled for a result, a ring for none or one out
/// of date.
fn status_dot(ui: &mut egui::Ui, status: Status) {
    if status == Status::Running {
        ui.add(egui::Spinner::new().size(10.0));
        return;
    }
    let (rect, response) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    let color = theme::egui_color(status_color(status));
    let centre = rect.center();
    if matches!(status, Status::NotRun | Status::Stale) {
        ui.painter()
            .circle_stroke(centre, 3.5, egui::Stroke::new(1.5, color));
    } else {
        ui.painter().circle_filled(centre, 4.0, color);
    }
    response.on_hover_text(status_text(status));
}

/// A row that selects `selection`, with `extra` beside it.
fn row(
    ui: &mut egui::Ui,
    doc: &Document,
    selection: Selection,
    text: impl Into<egui::WidgetText>,
    edits: &RefCell<Vec<Edit>>,
) -> egui::Response {
    let r = ui.selectable_label(doc.selection == selection, text);
    if r.clicked() {
        edits.borrow_mut().push(Edit::Select(selection));
    }
    r
}

/// A group or item with children, open by default.
fn branch(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    header: impl FnOnce(&mut egui::Ui),
    body: impl FnOnce(&mut egui::Ui),
) {
    let id = ui.make_persistent_id(id);
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
        .show_header(ui, header)
        .body(body);
}

/// A row without children, lined up with those that have them.
fn leaf(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.add_space(ui.spacing().indent);
        body(ui);
    });
}

/// The tree for `doc`.
pub fn show(ui: &mut egui::Ui, doc: &mut Document) -> Vec<BrowserAction> {
    // Both halves of each branch add to these, so they are shared, not borrowed.
    let actions = RefCell::new(Vec::new());
    let edits = RefCell::new(Vec::new());
    let d: &Document = doc;

    branch(
        ui,
        ("job", d.id),
        |ui| {
            row(
                ui,
                d,
                Selection::Job,
                egui::RichText::new(d.title()).strong(),
                &edits,
            );
        },
        |ui| {
            // Models.
            branch(
                ui,
                ("models", d.id),
                |ui| {
                    let r = row(
                        ui,
                        d,
                        Selection::Models,
                        format!("Models ({})", d.job.models.len()),
                        &edits,
                    );
                    r.context_menu(|ui| {
                        if ui.button("Add models…").clicked() {
                            actions.borrow_mut().push(BrowserAction::AddModels);
                            ui.close();
                        }
                    });
                },
                |ui| {
                    for m in &d.job.models {
                        leaf(ui, |ui| {
                            let slot = d.models.get(&m.name);
                            let text = match slot {
                                Some(ModelSlot::Failed { .. }) => {
                                    egui::RichText::new(&m.name).color(ui.visuals().error_fg_color)
                                }
                                _ => egui::RichText::new(&m.name),
                            };
                            let r = row(ui, d, Selection::Model(m.name.clone()), text, &edits);
                            if let Some(ModelSlot::Loading(_)) = slot {
                                ui.add(egui::Spinner::new().size(10.0));
                            }
                            let r = match slot {
                                Some(ModelSlot::Failed { why, .. }) => r.on_hover_text(why),
                                _ => r.on_hover_text(&m.path),
                            };
                            r.context_menu(|ui| {
                                if ui.button("Find it elsewhere…").clicked() {
                                    actions
                                        .borrow_mut()
                                        .push(BrowserAction::Locate(m.name.clone()));
                                    ui.close();
                                }
                                if ui.button("Remove").clicked() {
                                    edits.borrow_mut().push(Edit::RemoveModel(m.name.clone()));
                                    ui.close();
                                }
                            });
                        });
                    }
                },
            );

            // Stocks.
            branch(
                ui,
                ("stocks", d.id),
                |ui| {
                    let r = row(
                        ui,
                        d,
                        Selection::Stocks,
                        format!("Stocks ({})", d.job.stocks.len()),
                        &edits,
                    );
                    r.context_menu(|ui| {
                        if ui.button("New stock").clicked() {
                            edits.borrow_mut().push(Edit::AddStock);
                            ui.close();
                        }
                    });
                },
                |ui| {
                    for s in &d.job.stocks {
                        leaf(ui, |ui| {
                            let r = row(ui, d, Selection::Stock(s.name.clone()), &s.name, &edits);
                            r.context_menu(|ui| {
                                if ui.button("Remove").clicked() {
                                    edits.borrow_mut().push(Edit::RemoveStock(s.name.clone()));
                                    ui.close();
                                }
                            });
                        });
                    }
                },
            );

            // Setups.
            branch(
                ui,
                ("setups", d.id),
                |ui| {
                    let r = row(
                        ui,
                        d,
                        Selection::Setups,
                        format!("Setups ({})", d.job.setups.len()),
                        &edits,
                    );
                    r.context_menu(|ui| {
                        if ui.button("New setup").clicked() {
                            edits.borrow_mut().push(Edit::AddSetup);
                            ui.close();
                        }
                    });
                },
                |ui| {
                    for s in &d.job.setups {
                        let name = s.name.clone();
                        branch(
                            ui,
                            ("setup", d.id, &name),
                            |ui| {
                                status_dot(ui, d.status(&name));
                                let shown = d.active.as_deref() == Some(name.as_str());
                                let mut text = egui::RichText::new(&name);
                                if shown {
                                    text = text.strong();
                                }
                                let r = row(ui, d, Selection::Setup(name.clone()), text, &edits);
                                r.context_menu(|ui| setup_menu(ui, d, &name, &edits, &actions));
                            },
                            |ui| {
                                for (i, op) in s.operations.iter().enumerate() {
                                    leaf(ui, |ui| {
                                        let tool = op.tool.as_deref().unwrap_or("no tool");
                                        let text = format!("{}. {}", i + 1, op.name);
                                        let r = row(
                                            ui,
                                            d,
                                            Selection::Operation(name.clone(), i),
                                            text,
                                            &edits,
                                        )
                                        .on_hover_text(tool);
                                        if op.tool.is_none() {
                                            ui.colored_label(ui.visuals().warn_fg_color, "no tool");
                                        }
                                        r.context_menu(|ui| {
                                            if ui.button("Duplicate").clicked() {
                                                edits.borrow_mut().push(Edit::DuplicateOperation(
                                                    name.clone(),
                                                    i,
                                                ));
                                                ui.close();
                                            }
                                            if ui.button("Move up").clicked() {
                                                edits.borrow_mut().push(Edit::MoveOperation(
                                                    name.clone(),
                                                    i,
                                                    -1,
                                                ));
                                                ui.close();
                                            }
                                            if ui.button("Move down").clicked() {
                                                edits.borrow_mut().push(Edit::MoveOperation(
                                                    name.clone(),
                                                    i,
                                                    1,
                                                ));
                                                ui.close();
                                            }
                                            if ui.button("Delete").clicked() {
                                                edits
                                                    .borrow_mut()
                                                    .push(Edit::RemoveOperation(name.clone(), i));
                                                ui.close();
                                            }
                                        });
                                    });
                                }
                                if s.operations.is_empty() {
                                    leaf(ui, |ui| {
                                        ui.weak("no operations: right-click the setup");
                                    });
                                }
                            },
                        );
                    }
                },
            );
        },
    );

    for edit in edits.into_inner() {
        apply(doc, edit);
    }
    actions.into_inner()
}

fn setup_menu(
    ui: &mut egui::Ui,
    doc: &Document,
    name: &str,
    edits: &RefCell<Vec<Edit>>,
    actions: &RefCell<Vec<BrowserAction>>,
) {
    let why_not = doc.setup_index(name).and_then(|i| doc.cannot_run(i));
    if ui
        .add_enabled(why_not.is_none(), egui::Button::new("Generate"))
        .on_disabled_hover_text(why_not.unwrap_or_default())
        .clicked()
    {
        actions
            .borrow_mut()
            .push(BrowserAction::Generate(name.to_string()));
        ui.close();
    }
    ui.separator();
    if ui.button("New parallel finish").clicked() {
        edits.borrow_mut().push(Edit::AddOperation(
            name.to_string(),
            StrategySpec::Parallel { angle: 0.0 },
        ));
        ui.close();
    }
    if ui.button("New z-level rough").clicked() {
        edits.borrow_mut().push(Edit::AddOperation(
            name.to_string(),
            StrategySpec::ZlevelRough {
                stepdown: 2.0,
                min_ring_length: 3.0,
            },
        ));
        ui.close();
    }
    ui.separator();
    if ui.button("Duplicate").clicked() {
        edits
            .borrow_mut()
            .push(Edit::DuplicateSetup(name.to_string()));
        ui.close();
    }
    if ui.button("Move up").clicked() {
        edits
            .borrow_mut()
            .push(Edit::MoveSetup(name.to_string(), -1));
        ui.close();
    }
    if ui.button("Move down").clicked() {
        edits
            .borrow_mut()
            .push(Edit::MoveSetup(name.to_string(), 1));
        ui.close();
    }
    if ui.button("Delete").clicked() {
        edits.borrow_mut().push(Edit::RemoveSetup(name.to_string()));
        ui.close();
    }
}

fn apply(doc: &mut Document, edit: Edit) {
    let result = match edit {
        Edit::Select(s) => {
            match &s {
                Selection::Setup(n) | Selection::Operation(n, _) => doc.active = Some(n.clone()),
                _ => {}
            }
            doc.selection = s;
            Ok(())
        }
        Edit::AddStock => {
            doc.add_stock();
            Ok(())
        }
        Edit::AddSetup => {
            doc.add_setup();
            Ok(())
        }
        Edit::DuplicateSetup(n) => {
            doc.duplicate_setup(&n);
            Ok(())
        }
        Edit::RemoveSetup(n) => {
            doc.remove_setup(&n);
            Ok(())
        }
        Edit::MoveSetup(n, by) => {
            doc.move_setup(&n, by);
            Ok(())
        }
        Edit::AddOperation(s, strategy) => {
            doc.add_operation(&s, strategy);
            Ok(())
        }
        Edit::DuplicateOperation(s, i) => {
            doc.duplicate_operation(&s, i);
            Ok(())
        }
        Edit::RemoveOperation(s, i) => {
            doc.remove_operation(&s, i);
            Ok(())
        }
        Edit::MoveOperation(s, i, by) => {
            doc.move_operation(&s, i, by);
            Ok(())
        }
        Edit::RemoveModel(n) => doc.remove_model(&n),
        Edit::RemoveStock(n) => doc.remove_stock(&n),
    };
    if let Err(e) = result {
        doc.note(Level::Warn, e);
    }
}
