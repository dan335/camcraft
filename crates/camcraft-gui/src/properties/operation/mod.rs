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

use camcraft_core::job::StockToLeave;
use eframe::egui;

use super::{PropAction, ViewPicks, cannot_generate, header, renamed};
use crate::browser::strategy_label;
use crate::document::{Document, Selection};
use crate::picking::slots::Slot;
use crate::ui::measure::Measure;
use crate::ui::theme;
use crate::ui::widgets::{self, Tab, form};

/// Which tab an operation shows: the same for every operation, so moving between
/// them keeps the place.
const TAB: &str = "operation-tab";

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

/// Operation `index` of `setup`'s panel. Which of its tabs shows, if it has one to
/// show.
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
    match page {
        OperationTab::Tool => {
            tool::tool(ui, doc, m, (setup, index), tool_name.as_deref(), actions);
        }
        OperationTab::Geometry => {
            let op = doc.operation_mut(setup, index)?;
            geometry::geometry(ui, m, (setup, index), op, diameter, picks, actions);
        }
        OperationTab::Heights => heights::heights(ui, m, doc.operation_mut(setup, index)?),
        OperationTab::Passes => passes::passes(ui, m, doc.operation_mut(setup, index)?, least),
        OperationTab::Linking => {
            linking::linking(ui, m, doc.operation_mut(setup, index)?, diameter);
        }
    }
    Some(page)
}
