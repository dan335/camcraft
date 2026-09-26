//! The job's tree, after Fusion's browser: Models, Stocks and Setups, each setup
//! with its operations.
//!
//! Clicking selects, and the properties panel edits what is selected. Right-click
//! adds, copies, moves and removes. Each setup shows where it stands: not run,
//! running, verified, with warnings, refused, or out of date since it ran.

use std::cell::RefCell;

use camcraft_core::job::{Operation, ProfileSide, Setup, StrategySpec};
use camcraft_core::run::Stage;
use eframe::egui;

use crate::document::{Document, Level, ModelSlot, OpStatus, Selection, Status};
use crate::ui::theme;
use crate::ui::widgets::{self, hint_text};

/// Something the tree asks the app to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserAction {
    /// Pick model files to add.
    AddModels,
    /// Find model `name`'s file.
    Locate(String),
    /// Run setup `name`: all its operations.
    Generate(String),
    /// Run operation `.1` of setup `.0` alone.
    GenerateOperation(String, usize),
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
    /// An operation, by setup and place, to a setup and place.
    MoveOperationTo(String, usize, String, usize),
    /// Turn an operation on or off.
    SetEnabled(String, usize, bool),
    RemoveModel(String),
    DuplicateStock(String),
    RemoveStock(String),
}

/// How a status reads.
pub fn status_text(status: Status) -> &'static str {
    match status {
        Status::NotRun => "not generated",
        Status::Running => "generating",
        Status::Verified => "passed its checks",
        Status::Warnings => "passed its checks, with warnings",
        Status::Refused => "checks FAILED: it would be refused",
        Status::Stale => "out of date",
    }
}

/// The colour of nothing to show yet: a status with no result, drawn as a
/// hairline would be.
fn none_yet() -> egui::Color32 {
    theme::egui_color(theme::BORDER_STRONG)
}

/// Where a setup stands, as a status dot: filled for a result, hollow for none or
/// one out of date; a spinner while it runs.
fn status_dot(ui: &mut egui::Ui, status: Status) {
    let (color, filled) = match status {
        Status::Running => {
            ui.add(egui::Spinner::new().size(10.0));
            return;
        }
        Status::NotRun => (none_yet(), false),
        Status::Verified => (widgets::Level::Ok.color(), true),
        Status::Warnings => (widgets::Level::Warn.color(), true),
        Status::Stale => (widgets::Level::Warn.color(), false),
        Status::Refused => (widgets::Level::Err.color(), true),
    };
    widgets::status_dot(ui, color, filled).on_hover_text(status_text(status));
}

/// Where an operation stands: a spinner while it runs, a hollow dot while it
/// waits or has not been made, a filled one once made (amber with warnings), a
/// hollow amber one once its setup has changed since, and a dash while it is
/// turned off.
fn op_marker(ui: &mut egui::Ui, status: OpStatus) {
    let (color, filled, text) = match status {
        OpStatus::Running(_) => {
            ui.add(egui::Spinner::new().size(10.0))
                .on_hover_text("generating");
            return;
        }
        OpStatus::Disabled => {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().hline(
                rect.center().x - 3.0..=rect.center().x + 3.0,
                rect.center().y,
                egui::Stroke::new(1.5, none_yet()),
            );
            response.on_hover_text("turned off");
            return;
        }
        OpStatus::NotRun => (none_yet(), false, "not generated"),
        OpStatus::Waiting => (theme::egui_color(theme::TEXT_3), false, "waiting its turn"),
        OpStatus::Done => (widgets::Level::Ok.color(), true, "generated"),
        OpStatus::Warnings => (
            widgets::Level::Warn.color(),
            true,
            "generated, with warnings",
        ),
        OpStatus::Stale => (widgets::Level::Warn.color(), false, "out of date"),
    };
    widgets::status_dot(ui, color, filled).on_hover_text(text);
}

/// What a running setup is doing, in a few words: which operation of how many,
/// or the steps either side.
fn stage_text(stage: Stage, operations: usize) -> String {
    match stage {
        Stage::Preparing => "laying out".into(),
        Stage::Operation(i) => format!("{} of {operations}", i + 1),
        Stage::Verifying => "verifying".into(),
        Stage::Checking => "checking".into(),
    }
}

/// Text quieter than a row's, and smaller, beside it.
fn beside(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(hint_text(text).size(theme::TEXT_SM));
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

/// A row without children, lined up with those that have them. Returns where
/// it is, across the whole width of the tree.
fn leaf(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) -> egui::Rect {
    let line = ui
        .horizontal(|ui| {
            ui.add_space(ui.spacing().indent);
            body(ui);
        })
        .response
        .rect;
    egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), line.y_range())
}

/// An operation being dragged to a new place, in its setup or another.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DraggedOp {
    setup: String,
    index: usize,
    /// What it is called, shown by the pointer.
    label: String,
}

/// Operation `index` of `setup`, whose row is `line`, as somewhere to drop one:
/// over its top half before it, its bottom half after it, with a line where it
/// would go. Returns the move, once let go.
fn drop_target(ui: &egui::Ui, setup: &str, index: usize, line: egui::Rect) -> Option<Edit> {
    let ctx = ui.ctx();
    egui::DragAndDrop::payload::<DraggedOp>(ctx)?;
    let pointer = ctx.pointer_interact_pos()?;
    if !line.contains(pointer) {
        return None;
    }
    let after = pointer.y > line.center().y;
    let y = if after { line.bottom() } else { line.top() };
    ui.painter().hline(
        line.x_range(),
        y,
        egui::Stroke::new(2.0, theme::egui_color(theme::ACCENT)),
    );
    let dragged = dropped(ui)?;
    // The gap it goes into, counted, in its own setup, before it is taken out.
    let gap = index + usize::from(after);
    let to = if dragged.setup == setup && gap > dragged.index {
        gap - 1
    } else {
        gap
    };
    (dragged.setup != setup || to != dragged.index)
        .then(|| Edit::MoveOperationTo(dragged.setup.clone(), dragged.index, setup.to_string(), to))
}

/// Setup `setup`, whose row is `line` and which has `count` operations, as
/// somewhere to drop one: at its end, outlined while held over it.
fn setup_drop(ui: &egui::Ui, setup: &str, count: usize, line: egui::Rect) -> Option<Edit> {
    let ctx = ui.ctx();
    egui::DragAndDrop::payload::<DraggedOp>(ctx)?;
    if !ctx.pointer_interact_pos().is_some_and(|p| line.contains(p)) {
        return None;
    }
    ui.painter().rect_stroke(
        line,
        theme::RADIUS_SM,
        egui::Stroke::new(1.5, theme::egui_color(theme::ACCENT)),
        egui::StrokeKind::Inside,
    );
    let dragged = dropped(ui)?;
    Some(Edit::MoveOperationTo(
        dragged.setup.clone(),
        dragged.index,
        setup.to_string(),
        count,
    ))
}

/// What was let go this frame, if an operation was being dragged.
fn dropped(ui: &egui::Ui) -> Option<std::sync::Arc<DraggedOp>> {
    let ctx = ui.ctx();
    if !ctx.input(|i| i.pointer.any_released()) {
        return None;
    }
    egui::DragAndDrop::take_payload::<DraggedOp>(ctx)
}

/// The operation being dragged, named by the pointer, over everything.
fn drag_label(ui: &egui::Ui) {
    if let Some(dragged) = egui::DragAndDrop::payload::<DraggedOp>(ui.ctx()) {
        let font = theme::font(ui.style(), egui::TextStyle::Body);
        widgets::drag_label(ui.ctx(), &dragged.label, font);
    }
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
            models(ui, d, &edits, &actions);
            stocks(ui, d, &edits);
            setups(ui, d, &edits, &actions);
        },
    );

    drag_label(ui);
    for edit in edits.into_inner() {
        apply(doc, edit);
    }
    actions.into_inner()
}

/// The Models group: each model, a spinner while it loads, and why not when it
/// failed.
fn models(
    ui: &mut egui::Ui,
    d: &Document,
    edits: &RefCell<Vec<Edit>>,
    actions: &RefCell<Vec<BrowserAction>>,
) {
    branch(
        ui,
        ("models", d.id),
        |ui| {
            let r = row(ui, d, Selection::Models, "Models", edits);
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
                        Some(ModelSlot::Failed { .. }) => widgets::error_text(&m.name),
                        _ => egui::RichText::new(&m.name),
                    };
                    let r = row(ui, d, Selection::Model(m.name.clone()), text, edits);
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
}

/// The Stocks group: each stock.
fn stocks(ui: &mut egui::Ui, d: &Document, edits: &RefCell<Vec<Edit>>) {
    branch(
        ui,
        ("stocks", d.id),
        |ui| {
            let r = row(ui, d, Selection::Stocks, "Stocks", edits);
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
                    let r = row(ui, d, Selection::Stock(s.name.clone()), &s.name, edits);
                    r.context_menu(|ui| {
                        if ui.button("Duplicate").clicked() {
                            edits
                                .borrow_mut()
                                .push(Edit::DuplicateStock(s.name.clone()));
                            ui.close();
                        }
                        if ui.button("Remove").clicked() {
                            edits.borrow_mut().push(Edit::RemoveStock(s.name.clone()));
                            ui.close();
                        }
                    });
                });
            }
        },
    );
}

/// The Setups group: each setup with its operations.
fn setups(
    ui: &mut egui::Ui,
    d: &Document,
    edits: &RefCell<Vec<Edit>>,
    actions: &RefCell<Vec<BrowserAction>>,
) {
    branch(
        ui,
        ("setups", d.id),
        |ui| {
            let r = row(ui, d, Selection::Setups, "Setups", edits);
            r.context_menu(|ui| {
                if ui.button("New setup").clicked() {
                    edits.borrow_mut().push(Edit::AddSetup);
                    ui.close();
                }
            });
        },
        |ui| {
            for s in &d.job.setups {
                setup(ui, d, s, edits, actions);
            }
        },
    );
}

/// One setup: where it stands and how far a run has got, then its operations,
/// each somewhere to drop one being dragged.
fn setup(
    ui: &mut egui::Ui,
    d: &Document,
    s: &Setup,
    edits: &RefCell<Vec<Edit>>,
    actions: &RefCell<Vec<BrowserAction>>,
) {
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
            let r = row(ui, d, Selection::Setup(name.clone()), text, edits);
            r.context_menu(|ui| setup_menu(ui, d, &name, edits, actions));
            let line = egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), r.rect.y_range());
            if let Some(edit) = setup_drop(ui, &name, s.operations.len(), line) {
                edits.borrow_mut().push(edit);
            }
            let progress = d.setups.get(&name).and_then(|st| st.progress);
            if let Some((stage, _)) = progress {
                beside(ui, stage_text(stage, s.operations.len()));
            }
        },
        |ui| {
            for (i, op) in s.operations.iter().enumerate() {
                let line = operation_row(ui, d, &name, i, op, edits, actions);
                if let Some(edit) = drop_target(ui, &name, i, line) {
                    edits.borrow_mut().push(edit);
                }
            }
            if s.operations.is_empty() {
                leaf(ui, |ui| {
                    ui.label(hint_text("no operations: right-click the setup"));
                });
            }
        },
    );
}

/// Operation `i` of setup `name`: where it stands, its name, a warning when it has
/// no tool, and its menu. It drags to a new place. Returns where its row is.
fn operation_row(
    ui: &mut egui::Ui,
    d: &Document,
    name: &str,
    i: usize,
    op: &Operation,
    edits: &RefCell<Vec<Edit>>,
    actions: &RefCell<Vec<BrowserAction>>,
) -> egui::Rect {
    leaf(ui, |ui| {
        let status = d.op_status(name, i);
        op_marker(ui, status);
        let tool = op.tool.as_deref().unwrap_or("no tool");
        let text = format!("{}. {}", i + 1, op.name);
        // Turned off: dim and struck through.
        let shown = if op.enabled {
            egui::RichText::new(&text)
        } else {
            hint_text(&text).strikethrough()
        };
        let r = row(
            ui,
            d,
            Selection::Operation(name.to_string(), i),
            shown,
            edits,
        )
        .interact(egui::Sense::drag())
        .on_hover_text(format!("{tool} · drag to reorder"));
        r.dnd_set_drag_payload(DraggedOp {
            setup: name.to_string(),
            index: i,
            label: text,
        });
        if op.tool.is_none() {
            ui.colored_label(widgets::Level::Warn.color(), "no tool");
        }
        if let OpStatus::Running(since) = status {
            beside(ui, format!("{} s", since.elapsed().as_secs()));
        }
        r.context_menu(|ui| operation_menu(ui, d, name, i, op, edits, actions));
    })
}

/// An operation's menu: generate it alone, turn it off or on, duplicate, move or
/// delete it.
fn operation_menu(
    ui: &mut egui::Ui,
    d: &Document,
    name: &str,
    i: usize,
    op: &Operation,
    edits: &RefCell<Vec<Edit>>,
    actions: &RefCell<Vec<BrowserAction>>,
) {
    generate_op_item(ui, d, name, i, actions);
    let label = if op.enabled { "Turn off" } else { "Turn on" };
    if ui
        .button(label)
        .on_hover_text(
            "a turned-off operation stays in the job, \
             but is not generated, cut or posted",
        )
        .clicked()
    {
        edits
            .borrow_mut()
            .push(Edit::SetEnabled(name.to_string(), i, !op.enabled));
        ui.close();
    }
    ui.separator();
    if ui.button("Duplicate").clicked() {
        edits
            .borrow_mut()
            .push(Edit::DuplicateOperation(name.to_string(), i));
        ui.close();
    }
    if ui.button("Move up").clicked() {
        edits
            .borrow_mut()
            .push(Edit::MoveOperation(name.to_string(), i, -1));
        ui.close();
    }
    if ui.button("Move down").clicked() {
        edits
            .borrow_mut()
            .push(Edit::MoveOperation(name.to_string(), i, 1));
        ui.close();
    }
    if ui.button("Delete").clicked() {
        edits
            .borrow_mut()
            .push(Edit::RemoveOperation(name.to_string(), i));
        ui.close();
    }
}

/// Generate operation `index` of setup `name` alone: the others keep what they
/// have.
fn generate_op_item(
    ui: &mut egui::Ui,
    doc: &Document,
    name: &str,
    index: usize,
    actions: &RefCell<Vec<BrowserAction>>,
) {
    let why_not = doc.cannot_generate(name, Some(index));
    if ui
        .add_enabled(why_not.is_none(), egui::Button::new("Generate"))
        .on_hover_text("generates this operation alone: the others keep what they have")
        .on_disabled_hover_text(why_not.unwrap_or_default())
        .clicked()
    {
        actions
            .borrow_mut()
            .push(BrowserAction::GenerateOperation(name.to_string(), index));
        ui.close();
    }
}

/// Generate setup `name`, from its menu: all its operations.
fn generate_item(
    ui: &mut egui::Ui,
    doc: &Document,
    name: &str,
    actions: &RefCell<Vec<BrowserAction>>,
) {
    let why_not = doc.cannot_generate(name, None);
    if ui
        .add_enabled(why_not.is_none(), egui::Button::new("Generate"))
        .on_hover_text(format!("generates setup '{name}', all its operations"))
        .on_disabled_hover_text(why_not.unwrap_or_default())
        .clicked()
    {
        actions
            .borrow_mut()
            .push(BrowserAction::Generate(name.to_string()));
        ui.close();
    }
}

/// The kinds of operation there are to add, each with what a new one starts
/// from.
pub fn new_operations() -> [(&'static str, StrategySpec); 5] {
    [
        ("parallel finish", StrategySpec::Parallel { angle: 0.0 }),
        (
            "z-level rough",
            StrategySpec::ZlevelRough {
                stepdown: 2.0,
                min_ring_length: 3.0,
            },
        ),
        (
            "2D pocket",
            StrategySpec::Pocket {
                stepdown: 2.0,
                conventional: false,
            },
        ),
        (
            "2D profile",
            StrategySpec::Profile {
                side: ProfileSide::Outside,
                stepdown: 2.0,
                conventional: false,
            },
        ),
        (
            "drilling",
            StrategySpec::Drill {
                peck: None,
                dwell: None,
            },
        ),
    ]
}

fn setup_menu(
    ui: &mut egui::Ui,
    doc: &Document,
    name: &str,
    edits: &RefCell<Vec<Edit>>,
    actions: &RefCell<Vec<BrowserAction>>,
) {
    generate_item(ui, doc, name, actions);
    ui.separator();
    for (label, strategy) in new_operations() {
        if ui.button(format!("New {label}")).clicked() {
            edits
                .borrow_mut()
                .push(Edit::AddOperation(name.to_string(), strategy));
            ui.close();
        }
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
        Edit::DuplicateStock(n) => {
            doc.duplicate_stock(&n);
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
        Edit::SetEnabled(s, i, on) => {
            doc.set_enabled(&s, i, on);
            Ok(())
        }
        Edit::MoveOperationTo(from_setup, from, to_setup, to) => {
            doc.move_operation_to(&from_setup, from, &to_setup, to);
            Ok(())
        }
        Edit::RemoveModel(n) => doc.remove_model(&n),
        Edit::RemoveStock(n) => doc.remove_stock(&n),
    };
    if let Err(e) = result {
        doc.note(Level::Warn, e);
    }
}

#[cfg(all(test, feature = "ui-snapshots"))]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;

    fn names(doc: &Document) -> Vec<String> {
        doc.job.setups[0]
            .operations
            .iter()
            .map(|o| o.name.clone())
            .collect()
    }

    /// Dragging an operation's row onto the top half of another's puts it
    /// before that one, and letting go anywhere else leaves the order alone.
    #[test]
    fn an_operation_is_dragged_to_a_new_place() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/rail-jig.camcraft");
        let job = camcraft_session::read_job(&path).unwrap();
        let mut doc = Document::opened(1, Some(path), job);
        doc.duplicate_operation("top", 1);
        assert_eq!(names(&doc), ["rough", "finish", "finish copy"]);
        let mut h = Harness::new_ui_state(
            |ui, doc: &mut Document| {
                show(ui, doc);
            },
            doc,
        );
        h.run();

        let drag = |h: &mut Harness<'_, Document>, from: egui::Pos2, to: egui::Pos2| {
            h.hover_at(from);
            h.run();
            h.drag_at(from);
            h.run();
            for step in 1..=6 {
                h.hover_at(from.lerp(to, step as f32 / 6.0));
                h.run();
            }
            // Let go where it is, without the pointer leaving.
            h.input_mut().events.push(egui::Event::PointerButton {
                pos: to,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            });
            h.run();
        };

        let from = h.get_by_label("3. finish copy").rect().center();
        let onto = h.get_by_label("1. rough").rect();
        drag(&mut h, from, egui::pos2(onto.center().x, onto.top() + 2.0));
        assert_eq!(names(h.state()), ["finish copy", "rough", "finish"]);

        // Dropped on the models, nothing moves.
        let from = h.get_by_label("1. finish copy").rect().center();
        let away = h.get_by_label("sled_base").rect().center();
        drag(&mut h, from, away);
        assert_eq!(names(h.state()), ["finish copy", "rough", "finish"]);
        // Onto another setup's row: to the end of that setup.
        h.state_mut().duplicate_setup("top").unwrap();
        h.run();
        let from = h
            .get_all_by_label("2. rough")
            .next()
            .unwrap()
            .rect()
            .center();
        let onto = h.get_by_label("top copy").rect().center();
        drag(&mut h, from, onto);
        let other: Vec<&str> = h.state().job.setups[1]
            .operations
            .iter()
            .map(|o| o.name.as_str())
            .collect();
        assert_eq!(names(h.state()), ["finish copy", "finish"]);
        assert_eq!(other, ["finish copy", "rough", "finish", "rough 2"]);
    }
}
