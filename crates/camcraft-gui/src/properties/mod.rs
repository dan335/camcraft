//! The properties of whatever is selected in the job's tree, edited in place.
//!
//! Every field writes straight into the job. Nothing here runs anything: a change
//! only marks a setup's result out of date, and Generate, on the setup's
//! right-click menu or the toolbar, runs it again. Lengths and feeds are shown in
//! the chosen units and kept in millimetres.
//!
//! The panel only edits. What the tree already shows (the setups' operations, where
//! each stands) and what its right-click menus already do (add, generate, remove)
//! is not repeated here. A setup and an operation put their fields on tabs, as the
//! tool library does, so each shows a short form rather than one long scroll.
//!
//! A property filled by picking in the view (a setup's models, stocks and work
//! zero, an operation's faces or edges) has a button that asks the app to arm the
//! view for it ([`crate::slots`]), and lists what is picked as the engine will find
//! it, so a pick that is gone says so here first.

mod operation;
mod parts;
mod setup;
mod work_zero;

pub use operation::height_planes;

use eframe::egui;

use crate::document::{Document, Level, Selection};
use crate::measure::Measure;
use crate::slots::{Kind, Shown, Slot};
use crate::theme;
use crate::tool_editor::kind_name;
use crate::widgets::{self, Icon, Size, Tab, Variant, form};

/// Something the panel asks the app to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropAction {
    /// Pick model files to add, and to add to the named setup too.
    AddModels(Option<String>),
    /// Find model `name`'s file.
    Locate(String),
    /// Pick a machine profile file for setup `setup`.
    ProfileFile(String),
    /// Pick a tool for operation `index` of setup `setup`.
    PickTool(String, usize),
    /// Fill `slot` by picking in the view.
    Arm(Slot),
    /// Stop picking.
    Disarm,
    /// Row `i` of the list of picked faces or edges is under the pointer.
    HoverRow(Kind, usize),
}

/// What the panel needs to know of picking in the view.
#[derive(Clone, Copy, Default)]
pub struct Picks<'a> {
    /// The slot being filled, if any.
    pub armed: Option<&'a Slot>,
    /// What the selection's picks are, as the engine finds them.
    pub shown: Option<&'a Shown>,
}

/// The panel for `doc`'s selection.
pub fn show(
    ui: &mut egui::Ui,
    doc: &mut Document,
    m: Measure,
    picks: Picks<'_>,
) -> Vec<PropAction> {
    let mut actions = Vec::new();
    match doc.selection.clone() {
        Selection::Job => job(ui, doc, m),
        Selection::Models => group(
            ui,
            "Models",
            "The design files this job machines. Each setup chooses which it includes. \
             Right-click Models in the tree to add more.",
            doc.job
                .models
                .is_empty()
                .then_some((Icon::Upload, "No models yet", "Add models…")),
            || actions.push(PropAction::AddModels(None)),
        ),
        Selection::Model(name) => parts::model(ui, doc, m, &name, &mut actions),
        Selection::Stocks => {
            let mut add = false;
            group(
                ui,
                "Stocks",
                "Blocks of material, in the models' own coordinates. Setups that turn the \
                 part over see the same block from the other side.",
                doc.job
                    .stocks
                    .is_empty()
                    .then_some((Icon::Plus, "No stock yet", "New stock")),
                || add = true,
            );
            if add {
                doc.add_stock();
            }
        }
        Selection::Stock(name) => parts::stock(ui, doc, m, &name),
        Selection::Setups => {
            let mut add = false;
            group(
                ui,
                "Setups",
                "Each setup holds the part one way on the machine, and runs from its own \
                 stock. Right-click a setup to generate it or add operations.",
                doc.job
                    .setups
                    .is_empty()
                    .then_some((Icon::Plus, "No setups yet", "New setup")),
                || add = true,
            );
            if add {
                doc.add_setup();
            }
        }
        Selection::Setup(name) => setup::show(ui, doc, m, &name, picks, &mut actions),
        Selection::Operation(setup, index) => {
            operation::show(ui, doc, m, &setup, index, picks, &mut actions);
        }
    }
    actions
}

/// A group in the tree: what it holds, and, when it holds nothing, the way to
/// start it.
fn group(
    ui: &mut egui::Ui,
    title: &str,
    about: &str,
    empty: Option<(Icon, &str, &str)>,
    mut add: impl FnMut(),
) {
    ui.label(widgets::section_label(title));
    ui.add_space(theme::SPACE_2);
    quiet(ui, about);
    if let Some((icon, what, button)) = empty {
        widgets::empty_state(ui, icon, what, "", |ui| {
            if widgets::button(ui, Variant::Secondary, Size::Sm, Some(Icon::Plus), button).clicked()
            {
                add();
            }
        });
    }
}

/// The top of an entity's panel: what it is, and its name, applied when editing
/// finishes. The new name, if it changed.
fn header(ui: &mut egui::Ui, kind: &str, id: egui::Id, current: &str) -> Option<String> {
    ui.label(widgets::section_label(kind));
    ui.add_space(theme::SPACE_2);
    let mut rename = None;
    form(ui, id.with("header"), |ui| {
        widgets::field_label(ui, "Name");
        rename = name_field(ui, id.with("name"), current);
        ui.end_row();
    });
    rename
}

/// A name, applied when editing finishes: the new one, if it changed.
fn name_field(ui: &mut egui::Ui, id: egui::Id, current: &str) -> Option<String> {
    let mut buffer = ui
        .data(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| current.to_string());
    let r = ui.add(
        egui::TextEdit::singleline(&mut buffer)
            .id(id)
            .desired_width(200.0),
    );
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        return (buffer.trim() != current).then_some(buffer);
    }
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buffer));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    None
}

/// Underline tabs whose choice is remembered under `id`: the one showing. While
/// something is being picked in the view, the tab that lists it is `picking`, and
/// shows, so what a click adds is in sight.
fn tab_bar(ui: &mut egui::Ui, id: egui::Id, tabs: &[Tab<'_>], picking: Option<usize>) -> usize {
    let mut page = picking
        .or_else(|| ui.data(|d| d.get_temp::<usize>(id)))
        .unwrap_or(0)
        .min(tabs.len().saturating_sub(1));
    widgets::tabs(ui, &mut page, tabs);
    ui.data_mut(|d| d.insert_temp(id, page));
    ui.add_space(theme::SPACE_4);
    page
}

/// Why setup `name` cannot be generated, if it cannot: the tree's dot says it
/// cannot, and this says why.
fn cannot_run(ui: &mut egui::Ui, doc: &Document, name: &str) {
    if let Some(why) = doc.setup_index(name).and_then(|i| doc.cannot_run(i)) {
        widgets::diagnostic(ui, widgets::Level::Warn, &why, |_| {});
        ui.add_space(theme::SPACE_3);
    }
}

/// Explaining text, quieter than what it explains.
fn quiet(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Label::new(egui::RichText::new(text).color(theme::egui_color(theme::TEXT_3))).wrap(),
    )
}

/// Text that says something is wrong.
fn error(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Label::new(egui::RichText::new(text).color(widgets::Level::Err.color())).wrap())
}

fn job(ui: &mut egui::Ui, doc: &mut Document, m: Measure) {
    ui.label(widgets::section_label("Job"));
    ui.add_space(theme::SPACE_2);
    form(ui, "job", |ui| {
        widgets::field_label(ui, "File");
        match &doc.path {
            Some(p) => ui.label(widgets::mono(p.display().to_string(), theme::TEXT_SM)),
            None => quiet(ui, "not saved yet"),
        };
        ui.end_row();
        widgets::field_label(ui, "Tolerance");
        m.length_field(ui, &mut doc.job.tolerance, 0.001, 10.0, "")
            .on_hover_text(
                "How closely paths follow the part; STEP models are meshed to a quarter of it.",
            );
        ui.end_row();
    });
    if doc.job.tolerance < 0.001 {
        doc.job.tolerance = 0.001;
    }

    ui.add_space(theme::SPACE_4);
    let used = doc.used_tools();
    widgets::card(
        ui,
        Some("Tools in this job"),
        |_| {},
        |ui| {
            if doc.job.tools.is_empty() {
                quiet(ui, "None yet: an operation's Select tool brings one in.");
            }
            egui::Grid::new("job-tools")
                .num_columns(4)
                .spacing([theme::SPACE_3, 6.0])
                .show(ui, |ui| {
                    for (name, t) in &doc.job.tools {
                        widgets::tag(ui, &format!("T{}", t.number));
                        ui.label(name);
                        ui.label(
                            widgets::mono(
                                format!("{} Ø{}", kind_name(t.kind), m.length_trimmed(t.diameter)),
                                theme::TEXT_SM,
                            )
                            .color(theme::egui_color(theme::TEXT_3)),
                        );
                        if used.contains(name) {
                            ui.label("");
                        } else {
                            quiet(ui, "unused; dropped on save");
                        }
                        ui.end_row();
                    }
                });
        },
    );
}

/// Warn of a rename that could not be made.
fn renamed(doc: &mut Document, result: Result<(), String>) -> bool {
    match result {
        Ok(()) => true,
        Err(e) => {
            doc.note(Level::Warn, e);
            false
        }
    }
}
