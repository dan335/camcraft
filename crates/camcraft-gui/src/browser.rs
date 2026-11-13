//! The job's tree, after Fusion's browser: Models, Stocks and Setups, each setup
//! with its operations.
//!
//! Clicking selects, and the properties panel edits what is selected. Right-click
//! adds, copies, moves and removes. Each setup shows where it stands: not run,
//! running, verified, with warnings, refused, or out of date since it ran.

use std::cell::RefCell;

use camcraft_core::job::{Operation, Setup, StrategySpec};
use camcraft_core::run::Stage;
use eframe::egui;

use crate::document::{Document, Level, ModelSlot, OpStatus, OpWork, Selection, Status};
use crate::ui::theme;
use crate::ui::widgets::{self, hint_text};
use crate::view::scene::clock;

/// Something the tree asks the app to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserAction {
    /// Pick model files to add.
    AddModels,
    /// Find model `name`'s file.
    Locate(String),
    /// Generate setup `name` again, as asked: whatever stopped it let go.
    Generate(String),
    /// Stop generating setup `name`.
    Stop(String),
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
    DuplicateModel(String),
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
        Status::Stale => "out of date: generating again once editing settles",
        Status::Stopped => "stopped: generates again once it changes",
        Status::Failed => "generating it FAILED: tries again once it changes",
    }
}

/// The colour of nothing to show yet: a status with no result, drawn as a
/// hairline would be.
fn none_yet() -> egui::Color32 {
    theme::egui_color(theme::border_strong())
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
        Status::Stale | Status::Stopped => (widgets::Level::Warn.color(), false),
        Status::Refused => (widgets::Level::Err.color(), true),
        Status::Failed => (widgets::Level::Err.color(), false),
    };
    widgets::status_dot(ui, color, filled).on_hover_text(status_text(status));
}

/// Where an operation stands: a dot filling as far as it has got while it is
/// generated, a hollow dot while it waits or has not been made, a filled one
/// once made (amber with warnings), a hollow amber one once its setup has
/// changed since, a hollow red one where its run failed, and a dash while it
/// is turned off.
fn op_marker(ui: &mut egui::Ui, status: OpStatus) {
    let (color, filled, text) = match status {
        OpStatus::Running { work, done, .. } => {
            widgets::status_gauge(ui, widgets::Level::Ok.color(), done)
                .on_hover_text(work_text(work));
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
        OpStatus::Waiting => (
            theme::egui_color(theme::text_3()),
            false,
            "waiting for the operations before it",
        ),
        OpStatus::Failed => (widgets::Level::Err.color(), false, "generating it failed"),
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

/// What an operation being generated is doing, in a few words.
pub fn work_text(work: OpWork) -> &'static str {
    match work {
        OpWork::Planning => "working out its passes",
        OpWork::Cutting => "linking its passes and cutting them into the material",
    }
}

/// How far an operation being generated has got, and for how long: beside it.
pub fn running_text(done: f64, since: std::time::Instant) -> String {
    format!("{:.0}% · {}", done * 100.0, widgets::elapsed(since))
}

/// What a running setup is doing, in a few words: how far its operations have
/// got together, or the steps either side of them.
fn stage_text(d: &Document, name: &str) -> Option<String> {
    let running = d.setups.get(name)?.running.as_ref()?;
    Some(match running.stage.0 {
        Stage::Verifying => "verifying".into(),
        Stage::Checking => "checking".into(),
        _ => match d.generated_share(name) {
            Some(share) => format!("generating {:.0}%", share * 100.0),
            None => "laying out".into(),
        },
    })
}

/// Text quieter than a row's, and smaller, beside it.
fn beside(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    ui.label(hint_text(text).size(theme::TEXT_SM))
}

/// A row reading `text` that selects `selection`.
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
        egui::Stroke::new(2.0, theme::egui_color(theme::accent())),
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
        egui::Stroke::new(1.5, theme::egui_color(theme::accent())),
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
                item(ui, "Add models…", actions, || BrowserAction::AddModels);
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
                        item(ui, "Duplicate", edits, || {
                            Edit::DuplicateModel(m.name.clone())
                        });
                        item(ui, "Find it elsewhere…", actions, || {
                            BrowserAction::Locate(m.name.clone())
                        });
                        item(ui, "Remove", edits, || Edit::RemoveModel(m.name.clone()));
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
                item(ui, "New stock", edits, || Edit::AddStock);
            });
        },
        |ui| {
            for s in &d.job.stocks {
                leaf(ui, |ui| {
                    let r = row(ui, d, Selection::Stock(s.name.clone()), &s.name, edits);
                    r.context_menu(|ui| {
                        item(ui, "Duplicate", edits, || {
                            Edit::DuplicateStock(s.name.clone())
                        });
                        item(ui, "Remove", edits, || Edit::RemoveStock(s.name.clone()));
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
                item(ui, "New setup", edits, || Edit::AddSetup);
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
            let status = d.status(&name);
            status_dot(ui, status);
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
            if let Some(text) = stage_text(d, &name) {
                beside(ui, text);
            } else if let Some(t) = setup_time(d, &name, status) {
                beside(ui, clock(t)).on_hover_text("machining time, estimated");
            }
        },
        |ui| {
            let statuses = d.op_statuses(&name);
            for (i, op) in s.operations.iter().enumerate() {
                let status = statuses.get(i).copied().unwrap_or(OpStatus::NotRun);
                let time = op_time(d, &name, i, status);
                let line = operation_row(ui, d, (&name, i, status, time), op, edits);
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

/// Setup `name`'s estimated machining time, once its last run is made whole
/// and stands, `status`: as Fusion gives it in its tree.
fn setup_time(d: &Document, name: &str, status: Status) -> Option<f64> {
    let whole = matches!(
        status,
        Status::Verified | Status::Warnings | Status::Refused
    );
    let scene = &d.setups.get(name)?.ran.as_ref()?.scene;
    whole.then(|| scene.total_time())
}

/// Operation `i` of setup `name`'s estimated machining time, where it is made
/// and stands, `status`.
fn op_time(d: &Document, name: &str, i: usize, status: OpStatus) -> Option<f64> {
    if !matches!(status, OpStatus::Done | OpStatus::Warnings) {
        return None;
    }
    d.setups.get(name)?.ran.as_ref()?.scene.op_time(i)
}

/// Operation `i` of setup `name`, `op`: where it stands, `status`, its name, a
/// warning when it has no tool, its machining time, `time`, once it is made,
/// and its menu. It drags to a new place. Returns where its row is.
fn operation_row(
    ui: &mut egui::Ui,
    d: &Document,
    (name, i, status, time): (&str, usize, OpStatus, Option<f64>),
    op: &Operation,
    edits: &RefCell<Vec<Edit>>,
) -> egui::Rect {
    leaf(ui, |ui| {
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
            widgets::badge(ui, widgets::Level::Warn, "no tool");
        }
        if let OpStatus::Running { since, done, .. } = status {
            beside(ui, running_text(done, since));
        } else if let Some(t) = time {
            beside(ui, clock(t)).on_hover_text("machining time, estimated");
        }
        r.context_menu(|ui| operation_menu(ui, name, i, op, edits));
    })
}

/// An operation's menu: turn it off or on, duplicate, move or delete it.
fn operation_menu(
    ui: &mut egui::Ui,
    name: &str,
    i: usize,
    op: &Operation,
    edits: &RefCell<Vec<Edit>>,
) {
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
    item(ui, "Duplicate", edits, || {
        Edit::DuplicateOperation(name.to_string(), i)
    });
    item(ui, "Move up", edits, || {
        Edit::MoveOperation(name.to_string(), i, -1)
    });
    item(ui, "Move down", edits, || {
        Edit::MoveOperation(name.to_string(), i, 1)
    });
    item(ui, "Delete", edits, || {
        Edit::RemoveOperation(name.to_string(), i)
    });
}

/// Stop generating setup `name`, from its menu, while it is generated; or
/// generate it again, as it stands.
fn generate_item(
    ui: &mut egui::Ui,
    doc: &Document,
    name: &str,
    actions: &RefCell<Vec<BrowserAction>>,
) {
    if doc.status(name) == Status::Running {
        if ui
            .button("Stop generating")
            .on_hover_text("what it has made stands; it generates again once it changes")
            .clicked()
        {
            actions
                .borrow_mut()
                .push(BrowserAction::Stop(name.to_string()));
            ui.close();
        }
        return;
    }
    let why_not = doc.cannot_generate(name, None);
    if ui
        .add_enabled(why_not.is_none(), egui::Button::new("Generate again"))
        .on_hover_text(format!(
            "generates setup '{name}' now, keeping what still stands"
        ))
        .on_disabled_hover_text(why_not.unwrap_or_default())
        .clicked()
    {
        actions
            .borrow_mut()
            .push(BrowserAction::Generate(name.to_string()));
        ui.close();
    }
}

/// A strategy's name, as the menu that adds an operation of it and the panel of
/// one read it. A job file names it [`StrategySpec::name`]'s way.
pub fn strategy_label(s: &StrategySpec) -> &'static str {
    match s {
        StrategySpec::Parallel { .. } => "Parallel",
        StrategySpec::Scallop { .. } => "Scallop",
        StrategySpec::Contour { .. } => "Contour",
        StrategySpec::Flat { .. } => "Flat",
        StrategySpec::Spiral { .. } => "Spiral",
        StrategySpec::Radial { .. } => "Radial",
        StrategySpec::MorphedSpiral { .. } => "Morphed spiral",
        StrategySpec::PocketClearing { .. } => "Pocket clearing",
        StrategySpec::Adaptive { .. } => "Adaptive clearing",
        StrategySpec::Profile { .. } => "2D profile",
        StrategySpec::Pocket { .. } => "2D pocket",
        StrategySpec::Face { .. } => "Face",
        StrategySpec::Drill { .. } => "Drill",
        StrategySpec::Bore { .. } => "Bore",
        StrategySpec::Circular { .. } => "Circular",
        StrategySpec::Slot { .. } => "Slot",
        StrategySpec::Trace { .. } => "Trace",
        StrategySpec::Chamfer2d { .. } => "2D chamfer",
        StrategySpec::Adaptive2d { .. } => "2D adaptive clearing",
    }
}

/// A kind of operation to add: its name in the menu and what a new one starts
/// from.
pub type NewOperation = (&'static str, StrategySpec);

/// The kinds of operation there are to add, grouped as Fusion's toolbar groups
/// them: 2D, 3D and drilling. Each group is sections with a line between, 3D's
/// roughing above its finishing. Each starts as a job file that gives none of
/// its strategy's parameters has it, found by the name the file gives it.
pub fn operation_groups() -> [(&'static str, Vec<Vec<NewOperation>>); 3] {
    let section = |names: &[&str]| -> Vec<NewOperation> {
        names
            .iter()
            .filter_map(|name| StrategySpec::named(name))
            .map(|s| (strategy_label(&s), s))
            .collect()
    };
    [
        (
            "2D",
            vec![section(&[
                "face",
                "adaptive-2d",
                "pocket",
                "profile",
                "slot",
                "trace",
                "bore",
                "circular",
                "chamfer-2d",
            ])],
        ),
        (
            "3D",
            vec![
                section(&["adaptive", "pocket-clearing"]),
                section(&[
                    "parallel",
                    "scallop",
                    "contour",
                    "flat",
                    "spiral",
                    "radial",
                    "morphed-spiral",
                ]),
            ],
        ),
        ("Drilling", vec![section(&["drill"])]),
    ]
}

/// Fusion's operations camcraft has not built yet, each by the group of the
/// menus that add operations and the section of it it goes in, and its name
/// there, in the order of Fusion's toolbar. The menus list them after that
/// section's own, greyed, saying so. One that is built comes out of here and
/// into [`operation_groups`], in the same place.
pub const PLANNED: &[(&str, usize, &str)] = &[
    ("2D", 0, "Thread"),
    ("2D", 0, "Engrave"),
    ("3D", 1, "Steep and shallow"),
    ("3D", 1, "Ramp"),
    ("3D", 1, "Pencil"),
    ("3D", 1, "Project"),
    ("3D", 1, "Morph"),
    ("3D", 1, "Blend"),
    ("3D", 1, "Flow"),
    ("3D", 1, "Corner"),
    ("3D", 1, "Geodesic"),
];

/// What a planned operation's item says under the pointer.
pub const NOT_YET: &str = "Not implemented yet";

/// One item of a menu that adds an operation.
#[derive(Debug, Clone, PartialEq)]
pub enum MenuItem {
    /// An operation there is: its name and what a new one starts from.
    Operation(NewOperation),
    /// One planned, by its name: shown, greyed, and not to be clicked.
    Planned(&'static str),
}

/// The menus that add operations, group by group and section by section:
/// [`operation_groups`]' operations, each section's planned ones
/// ([`PLANNED`]) after them.
pub fn operation_menu_items() -> Vec<(&'static str, Vec<Vec<MenuItem>>)> {
    operation_groups()
        .into_iter()
        .map(|(group, sections)| {
            let sections = sections
                .into_iter()
                .enumerate()
                .map(|(i, section)| {
                    let planned = PLANNED
                        .iter()
                        .filter(|(g, s, _)| *g == group && *s == i)
                        .map(|(_, _, label)| MenuItem::Planned(label));
                    section
                        .into_iter()
                        .map(MenuItem::Operation)
                        .chain(planned)
                        .collect()
                })
                .collect();
            (group, sections)
        })
        .collect()
}

/// A submenu for each group of operations, `add` called with what the one
/// picked starts from. The Add menu and a setup's menu both show them, the
/// planned ones greyed.
pub fn operation_menus(ui: &mut egui::Ui, mut add: impl FnMut(StrategySpec)) {
    for (group, sections) in operation_menu_items() {
        ui.menu_button(group, |ui| {
            for (i, section) in sections.into_iter().enumerate() {
                if i > 0 {
                    ui.separator();
                }
                for item in section {
                    match item {
                        MenuItem::Operation((label, strategy)) => {
                            if ui.button(label).clicked() {
                                add(strategy);
                                ui.close();
                            }
                        }
                        MenuItem::Planned(label) => {
                            ui.add_enabled(false, egui::Button::new(label))
                                .on_disabled_hover_text(NOT_YET);
                        }
                    }
                }
            }
        });
    }
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
    operation_menus(ui, |strategy| {
        edits
            .borrow_mut()
            .push(Edit::AddOperation(name.to_string(), strategy));
    });
    ui.separator();
    item(ui, "Duplicate", edits, || {
        Edit::DuplicateSetup(name.to_string())
    });
    item(ui, "Move up", edits, || {
        Edit::MoveSetup(name.to_string(), -1)
    });
    item(ui, "Move down", edits, || {
        Edit::MoveSetup(name.to_string(), 1)
    });
    item(ui, "Delete", edits, || Edit::RemoveSetup(name.to_string()));
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
        Edit::DuplicateModel(n) => {
            doc.duplicate_model(&n);
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
    use egui_kittest::kittest::{NodeT as _, Queryable as _};

    fn names(doc: &Document) -> Vec<String> {
        doc.job.setups[0]
            .operations
            .iter()
            .map(|o| o.name.clone())
            .collect()
    }

    /// Each planned operation is in its menu, greyed, saying so under the
    /// pointer, and clicked adds nothing; one there is, clicked, adds it.
    #[test]
    fn planned_operations_are_greyed_in_the_menus() {
        let mut h = Harness::new_ui_state(
            |ui, added: &mut Vec<StrategySpec>| {
                operation_menus(ui, |s| added.push(s));
            },
            Vec::new(),
        );
        h.run();
        for (group, _, label) in PLANNED {
            h.get_by_label(group).click();
            h.run();
            let item = h.get_by_label(label);
            assert!(item.accesskit_node().is_disabled(), "{label} is not greyed");
            item.hover();
            h.run();
            h.get_by_label(label).click();
            h.run();
            assert!(h.state().is_empty(), "{label} added {:?}", h.state());
            // Shut, for the next.
            h.key_press(egui::Key::Escape);
            h.run();
        }
        h.get_by_label("2D").click();
        h.run();
        h.get_by_label("Bore").click();
        h.run();
        assert_eq!(h.state().len(), 1);
        assert_eq!(h.state()[0].name(), "bore");
        // Built since they were planned: offered, and added.
        for (label, name) in [
            ("2D adaptive clearing", "adaptive-2d"),
            ("2D chamfer", "chamfer-2d"),
        ] {
            h.get_by_label("2D").click();
            h.run();
            let item = h.get_by_label(label);
            assert!(!item.accesskit_node().is_disabled(), "{label} is greyed");
            item.click();
            h.run();
            assert_eq!(h.state().last().map(StrategySpec::name), Some(name));
        }
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

/// A menu item that, clicked, asks for what `then` makes and closes the menu.
fn item<T>(ui: &mut egui::Ui, label: &str, queue: &RefCell<Vec<T>>, then: impl FnOnce() -> T) {
    if ui.button(label).clicked() {
        queue.borrow_mut().push(then());
        ui.close();
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod menu_tests {
    use super::*;

    /// The menu offers every strategy there is, each once: one misnamed in
    /// [`operation_groups`] would drop out of it without a word.
    #[test]
    fn every_strategy_is_offered_once() {
        let mut names: Vec<&str> = operation_groups()
            .into_iter()
            .flat_map(|(_, sections)| sections.into_iter().flatten())
            .map(|(label, s)| {
                assert_eq!(label, strategy_label(&s));
                s.name()
            })
            .collect();
        let offered = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), offered, "one is offered twice: {names:?}");
        assert_eq!(offered, 19, "{names:?}");
    }

    /// The planned operations are listed after their section's own, in the
    /// order given, none of them an operation there is, so one that is built
    /// cannot be listed twice; and every one is greyed.
    #[test]
    fn planned_operations_are_listed_greyed_after_the_rest() {
        let labels: Vec<&str> = operation_groups()
            .into_iter()
            .flat_map(|(_, sections)| sections.into_iter().flatten())
            .map(|(label, _)| label)
            .collect();
        for (group, section, label) in PLANNED {
            assert!(
                !labels.contains(label),
                "{label} is built: take it out of PLANNED"
            );
            let sections = operation_groups()
                .into_iter()
                .find(|(g, _)| g == group)
                .map(|(_, s)| s.len());
            assert!(
                sections.is_some_and(|n| *section < n),
                "{label}: no section {section} of {group}"
            );
        }
        for (group, sections) in operation_menu_items() {
            for (i, section) in sections.iter().enumerate() {
                // Every planned one after every one there is.
                let first_planned = section
                    .iter()
                    .position(|m| matches!(m, MenuItem::Planned(_)))
                    .unwrap_or(section.len());
                assert!(
                    section[first_planned..]
                        .iter()
                        .all(|m| matches!(m, MenuItem::Planned(_))),
                    "{group} {i}: {section:?}"
                );
                let planned: Vec<&str> = section
                    .iter()
                    .filter_map(|m| match m {
                        MenuItem::Planned(l) => Some(*l),
                        MenuItem::Operation(_) => None,
                    })
                    .collect();
                let asked: Vec<&str> = PLANNED
                    .iter()
                    .filter(|(g, s, _)| *g == group && *s == i)
                    .map(|(.., l)| *l)
                    .collect();
                assert_eq!(planned, asked);
            }
        }
        // 2D: Fusion's order, 2D adaptive clearing before the pocket, Slot,
        // Trace, Bore and Circular after the profile, and 2D chamfer last.
        let two_d: Vec<String> = operation_menu_items()[0].1[0]
            .iter()
            .map(|m| match m {
                MenuItem::Operation((l, _)) => (*l).to_string(),
                MenuItem::Planned(l) => format!("({l})"),
            })
            .collect();
        assert_eq!(
            two_d,
            [
                "Face",
                "2D adaptive clearing",
                "2D pocket",
                "2D profile",
                "Slot",
                "Trace",
                "Bore",
                "Circular",
                "2D chamfer",
                "(Thread)",
                "(Engrave)"
            ]
        );
    }
}
