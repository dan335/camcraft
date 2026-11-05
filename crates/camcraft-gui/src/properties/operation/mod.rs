//! An operation's panel, on Fusion's tabs: the tool it cuts with ([`tool`]), its
//! geometry, what it is kept to, what it cuts round and what is picked
//! ([`geometry`]), its heights ([`heights`]), its passes ([`passes`]), and how
//! its passes are linked ([`linking`]).

mod geometry;
mod heights;
mod linking;
mod passes;
mod tool;

pub use heights::HeightPlanes;

use camcraft_core::job::{Finishing, Leads, Operation, StockToLeave, Tabs};
use eframe::egui;

use super::{PropAction, ViewPicks, cannot_generate, header, renamed};
use crate::browser::strategy_label;
use crate::document::{Document, Selection};
use crate::picking::slots::Slot;
use crate::ui::defaults::Defaults;
use crate::ui::measure::Measure;
use crate::ui::theme;
use crate::ui::widgets::{self, Tab, form};

/// Which tab an operation shows: the same for every operation, so moving between
/// them keeps the place.
const TAB: &str = "operation-tab";

/// The diameter a value of the tool's stands for before a tool is chosen: a
/// usual router bit's, 6 mm.
const ASSUMED_DIAMETER: f64 = 6.0;

/// The tool's diameter, or [`ASSUMED_DIAMETER`] before one is chosen.
fn across(diameter: Option<f64>) -> f64 {
    diameter.unwrap_or(ASSUMED_DIAMETER)
}

/// What a value left out comes to, as said beside its field: `what` and,
/// with a tool chosen, the `length` it is for that tool.
fn left_out(m: Measure, diameter: Option<f64>, length: f64, what: &str) -> String {
    match diameter {
        Some(_) => format!("{what}: {}", m.length(length, 2)),
        None => what.to_string(),
    }
}

/// An operation's tabs, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationTab {
    /// The tool it cuts with, and its speeds and feeds.
    Tool,
    /// What it is kept to.
    Geometry,
    /// Its heights: while this shows, the view draws them as planes.
    Heights,
    /// Its passes.
    Passes,
    /// How its passes are linked.
    Linking,
}

const TABS: [OperationTab; 5] = [
    OperationTab::Tool,
    OperationTab::Geometry,
    OperationTab::Heights,
    OperationTab::Passes,
    OperationTab::Linking,
];

/// The tab an operation's setting is on, by its place (`passes.stepover`):
/// the feeds and speeds on Tool, what it is kept to on Geometry, and so on.
pub fn tab_of(setting: &str) -> OperationTab {
    let section = setting.split('.').next().unwrap_or(setting);
    match (section, setting) {
        (_, "tool" | "preset" | "passes.feed" | "passes.rpm" | "linking.plunge_feed") => {
            OperationTab::Tool
        }
        ("selection" | "boundary" | "outline" | "rest_machining" | "ignore_stock_less_than", _) => {
            OperationTab::Geometry
        }
        ("heights", _) => OperationTab::Heights,
        ("linking" | "stay_down_level" | "stay_down_clearance" | "lift_height", _) => {
            OperationTab::Linking
        }
        _ => OperationTab::Passes,
    }
}

/// Show the tab of an operation's that has `setting` on it ([`tab_of`]),
/// the next time an operation's panel is drawn.
pub fn show_tab_of(ctx: &egui::Context, setting: &str) {
    let tab = tab_of(setting);
    if let Some(page) = TABS.iter().position(|t| *t == tab) {
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(TAB), page));
    }
}

/// Operation `index` of `setup`'s panel. Which of its tabs shows, if it has one to
/// show.
/// `def`, an operation's default, with each optional group `op` has given as
/// ticking it starts it: tabs, a pocket's finishing passes, leads and
/// levels, each as its box makes it.
fn started(op: &Operation, def: &Operation) -> Operation {
    let mut started = def.clone();
    let (passes, linking) = (&mut started.passes, &mut started.linking);
    if op.passes.tabs.is_some() && passes.tabs.is_none() {
        passes.tabs = Some(Tabs::default());
    }
    if op.passes.finishing.is_some() && passes.finishing.is_none() {
        passes.finishing = Some(Finishing::default());
    }
    if op.linking.leads.is_some() && linking.leads.is_none() {
        linking.leads = Some(Leads::default());
    }
    if op.passes.stepdown.is_some() && passes.stepdown.is_none() {
        passes.stepdown = Some(passes::starting_stepdown(op));
    }
    started
}

pub(super) fn show(
    ui: &mut egui::Ui,
    doc: &mut Document,
    m: Measure,
    setup: &str,
    index: usize,
    picks: ViewPicks<'_>,
    actions: &mut Vec<PropAction>,
) -> Option<OperationTab> {
    if doc.setup(setup).is_none() {
        doc.selection = Selection::Setups;
        return None;
    }
    let Some(current) = doc.operation(setup, index).map(|o| o.name.clone()) else {
        doc.selection = Selection::Setup(setup.to_string());
        return None;
    };
    let id = ui.id().with(("operation", setup, index));
    let what = (doc.id, Selection::Operation(setup.to_string(), index));
    if let Some(to) = header(ui, "Operation", id, &current, what) {
        let r = doc.rename_operation(setup, index, &to);
        renamed(doc, r);
    }
    cannot_generate(ui, doc, setup);
    let op = doc.operation(setup, index)?;
    form(ui, id.with("strategy"), |ui| {
        widgets::key_label(ui, "Strategy");
        ui.label(strategy_label(&op.strategy));
        ui.end_row();
    });
    ui.add_space(theme::SPACE_3);

    let tool_name = op.tool.clone();
    let spec = doc.job.tool_for(op);
    let tool_trouble = spec.is_none()
        || tool_name
            .as_ref()
            .is_some_and(|t| doc.number_clash(t).is_some());
    let diameter = spec.map(|t| t.diameter);
    let corner = spec.map_or(0.0, |t| t.corner_radius());
    // How far below zero its stock to leave may go: the tool's to say.
    let least = spec.map_or(
        StockToLeave {
            radial: f64::NEG_INFINITY,
            axial: f64::NEG_INFINITY,
        },
        |t| op.least_stock_to_leave(t),
    );
    let lost = picks.shown.is_some_and(|s| {
        s.faces.iter().chain(&s.edges).any(Result::is_err)
            || s.boundary.as_ref().is_some_and(Result::is_err)
    });
    let tabs = [
        Tab::new("Tool").flagged(tool_trouble),
        Tab::new("Geometry").flagged(lost),
        Tab::new("Heights"),
        Tab::new("Passes"),
        Tab::new("Linking"),
    ];
    // Picking faces or edges starts on the tab that lists them.
    let geometry_tab = TABS.iter().position(|t| *t == OperationTab::Geometry);
    let picking = geometry_tab
        .filter(|_| matches!(picks.armed, Some(Slot::Faces { .. } | Slot::Edges { .. })));
    let page = widgets::remembered_tabs(ui, egui::Id::new(TAB), &tabs, 0, picking);
    // Another tab chosen while picking stops it: what is picked is out of sight.
    if picking.is_some_and(|p| p != page) {
        actions.push(PropAction::Disarm);
    }
    let page = TABS.get(page).copied().unwrap_or(OperationTab::Linking);
    // What each setting shows as changed from, and goes back to: a new
    // operation's, and inside each optional group it has given, what giving
    // it starts it as.
    let def = doc.operation_defaults(setup, index)?;
    let started = doc.operation(setup, index).map(|op| started(op, &def))?;
    // Each tab's form: its settings against those, and an ƒx on each number.
    let faults = doc.expression_faults(setup, index);
    let form = |op: &Operation| Defaults::of(op, &def).started(&started).expressive(faults);
    match page {
        OperationTab::Tool => {
            tool::tool(ui, doc, m, (setup, index), tool_name.as_deref(), actions);
        }
        OperationTab::Geometry => {
            let op = doc.operation_mut(setup, index)?;
            let mut d = form(op);
            geometry::geometry(
                ui,
                (&mut d, op),
                m,
                (setup, index),
                diameter,
                picks,
                actions,
            );
            d.apply(op);
        }
        OperationTab::Heights => {
            let op = doc.operation_mut(setup, index)?;
            let mut d = form(op);
            heights::heights(ui, &mut d, m, op);
            d.apply(op);
        }
        OperationTab::Passes => {
            let op = doc.operation_mut(setup, index)?;
            let mut d = form(op);
            passes::passes(ui, m, op, &mut d, least, diameter);
            d.apply(op);
        }
        OperationTab::Linking => {
            let op = doc.operation_mut(setup, index)?;
            let mut d = form(op);
            linking::linking(ui, m, op, &mut d, diameter, corner);
            d.apply(op);
        }
    }
    Some(page)
}
