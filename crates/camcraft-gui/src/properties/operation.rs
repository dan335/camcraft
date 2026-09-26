//! An operation's panel, on Fusion's tabs: the tool it cuts with, the geometry
//! it is kept to, its heights, its passes, and how its passes are linked.

use camcraft_core::job::{
    Boundary, Containment, FeatureRef, Geometry, Height, HeightContext, HeightRef, Operation,
    ProfileSide, StrategySpec,
};
use eframe::egui;
use glam::DVec2;

use super::{Picks, PropAction, cannot_generate, header};
use crate::document::{Document, Level, Selection};
use crate::picking::slots::{Kind, Slot};
use crate::tools::drawing::paint_tool;
use crate::tools::editor::{chip_load_row, surface_speed_row};
use crate::ui::measure::Measure;
use crate::ui::theme;
use crate::ui::widgets::{self, Icon, Size, Tab, Variant, error, form, hint, list_row_background};
use crate::view::viewport::{Plane, PlaneKind};

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
    picks: Picks<'_>,
    actions: &mut Vec<PropAction>,
) -> Option<OperationTab> {
    let Some(s) = doc.setup(setup) else {
        doc.selection = Selection::Setups;
        return None;
    };
    let Some(current) = s.operations.get(index).map(|o| o.name.clone()) else {
        doc.selection = Selection::Setup(setup.to_string());
        return None;
    };
    let taken: Vec<String> = s.operations.iter().map(|o| o.name.clone()).collect();
    let id = ui.id().with(("operation", setup, index));
    if let Some(to) = header(ui, "Operation", id, &current) {
        rename(doc, setup, index, &to, &taken);
    }
    cannot_generate(ui, doc, setup);
    let op = doc.operation_mut(setup, index)?;
    form(ui, id.with("strategy"), |ui| {
        widgets::field_label(ui, "Strategy");
        ui.label(strategy_name(&op.strategy));
        ui.end_row();
    });
    ui.add_space(theme::SPACE_3);

    let tool_name = op.tool.clone();
    let tool_trouble = tool_name
        .as_ref()
        .is_none_or(|t| doc.number_clash(t).is_some() || !doc.job.tools.contains_key(t));
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
    // Picking faces or edges shows the tab that lists them.
    let geometry_tab = TABS.iter().position(|t| *t == OperationTab::Geometry);
    let picking = geometry_tab
        .filter(|_| matches!(picks.armed, Some(Slot::Faces { .. } | Slot::Edges { .. })));
    let page = widgets::remembered_tabs(ui, egui::Id::new(TAB), &tabs, 0, picking);
    let page = TABS.get(page).copied().unwrap_or(OperationTab::Linking);
    let diameter = tool_name
        .as_ref()
        .and_then(|t| doc.job.tools.get(t))
        .map(|t| t.diameter);
    if page == OperationTab::Tool {
        tool(ui, doc, m, setup, index, tool_name.as_deref(), actions);
    } else if let Some(op) = doc.operation_mut(setup, index) {
        match page {
            OperationTab::Tool => {}
            OperationTab::Geometry => {
                geometry(ui, m, setup, index, op, diameter, picks, actions);
            }
            OperationTab::Heights => heights(ui, m, op),
            OperationTab::Passes => passes(ui, m, op),
            OperationTab::Linking => linking(ui, m, op, diameter),
        }
    }
    Some(page)
}

/// Rename operation `index` of `setup` to `to`, if the name is free.
fn rename(doc: &mut Document, setup: &str, index: usize, to: &str, taken: &[String]) {
    let to = to.trim().to_string();
    let Some(op) = doc.operation_mut(setup, index) else {
        return;
    };
    if to.is_empty() {
        doc.note(Level::Warn, "an operation needs a name");
    } else if to != op.name && taken.contains(&to) {
        doc.note(
            Level::Warn,
            format!("there is already an operation called '{to}'"),
        );
    } else {
        op.name = to;
    }
}

/// A strategy's name, as the person reads it. It is chosen when the operation
/// is made, from the setup's menu in the tree, and not changed after.
fn strategy_name(s: &StrategySpec) -> &'static str {
    match s {
        StrategySpec::Parallel { .. } => "parallel",
        StrategySpec::ZlevelRough { .. } => "z-level rough",
        StrategySpec::Profile { .. } => "2D profile",
        StrategySpec::Pocket { .. } => "2D pocket",
        StrategySpec::Drill { .. } => "drilling",
    }
}

/// The tool and its preset, then the speeds and feeds it cuts at. The tool itself
/// is edited in the tool library.
#[allow(clippy::too_many_arguments)]
fn tool(
    ui: &mut egui::Ui,
    doc: &mut Document,
    m: Measure,
    setup: &str,
    index: usize,
    tool_name: Option<&str>,
    actions: &mut Vec<PropAction>,
) {
    form(ui, "op-tool", |ui| {
        widgets::field_label(ui, "Tool");
        ui.horizontal(|ui| {
            match tool_name {
                Some(t) => ui.strong(t),
                None => ui.colored_label(widgets::Level::Warn.color(), "no tool yet"),
            };
            if widgets::button(ui, Variant::Secondary, Size::Sm, None, "Select tool…").clicked() {
                actions.push(PropAction::PickTool(setup.to_string(), index));
            }
        });
        ui.end_row();
        if let Some(t) = tool_name {
            preset_row(ui, doc, setup, index, t);
        }
    });
    let Some(t) = tool_name else {
        return;
    };
    if let Some(clash) = doc.number_clash(t) {
        let number = doc.job.tools.get(t).map_or(0, |x| x.number);
        let mut renumber = false;
        widgets::diagnostic(
            ui,
            widgets::Level::Err,
            &format!("'{clash}' is also T{number}"),
            |ui| {
                renumber =
                    widgets::button(ui, Variant::Secondary, Size::Sm, None, "Renumber").clicked();
            },
        );
        if renumber {
            let free = doc.free_number();
            if let Some(spec) = doc.job.tools.get_mut(t) {
                spec.number = free;
            }
        }
    }
    ui.add_space(theme::SPACE_3);
    let Some(spec) = doc.job.tools.get(t) else {
        error(ui, &format!("'{t}' is not among the job's tools"));
        return;
    };
    paint_tool(ui, spec, m);
    let cutter = (spec.rpm, spec.diameter, spec.flutes);
    ui.add_space(theme::SPACE_4);
    ui.label(widgets::section_label("Feed and speed"));
    ui.add_space(theme::SPACE_2);
    if let Some(op) = doc.operation_mut(setup, index) {
        form(ui, "feed-speed", |ui| feed_speed(ui, m, op, cutter));
    }
}

/// The spindle speed and the feeds the operation cuts at, with the surface speed
/// and chip load they come to, which set them when edited.
fn feed_speed(
    ui: &mut egui::Ui,
    m: Measure,
    op: &mut Operation,
    (tool_rpm, diameter, flutes): (u32, f64, Option<u32>),
) {
    widgets::field_label(ui, "Spindle");
    ui.horizontal(|ui| {
        let absent = format!("the tool's, {tool_rpm} rev/min");
        widgets::optional(ui, &mut op.passes.rpm, tool_rpm, &absent, |ui, rpm| {
            ui.add(
                egui::DragValue::new(rpm)
                    .range(1..=60000)
                    .suffix(" rev/min"),
            );
        });
    });
    ui.end_row();
    // What the spindle turns at, the operation's own or the tool's.
    let turning = op.passes.rpm.or(Some(tool_rpm));
    let mut rpm = turning;
    surface_speed_row(ui, &mut rpm, diameter, m);
    if rpm != turning {
        op.passes.rpm = rpm;
    }
    // Drilling feeds its holes at the plunge feed: it has no cutting feed.
    if !matches!(op.strategy, StrategySpec::Drill { .. }) {
        widgets::field_label(ui, "Cutting feed");
        m.feed_field(ui, &mut op.passes.feed, 50_000.0);
        ui.end_row();
        let mut feed = Some(op.passes.feed);
        chip_load_row(ui, &mut feed, op.passes.rpm.or(Some(tool_rpm)), flutes, m);
        if let Some(f) = feed {
            op.passes.feed = f;
        }
    }
    widgets::field_label(ui, "Plunge feed");
    m.feed_field(ui, &mut op.linking.plunge_feed, 50_000.0);
    ui.end_row();
}

/// Which of the tool's presets the operation's values came from: a row of the
/// tool form.
fn preset_row(ui: &mut egui::Ui, doc: &mut Document, setup: &str, index: usize, tool: &str) {
    let presets = doc
        .job
        .tools
        .get(tool)
        .map(|t| t.presets.clone())
        .unwrap_or_default();
    let Some(op) = doc.operation_mut(setup, index) else {
        return;
    };
    let none = "none";
    widgets::field_label(ui, "Preset");
    egui::ComboBox::from_id_salt(("op-preset", setup, index))
        .selected_text(op.preset.as_deref().unwrap_or(none))
        .show_ui(ui, |ui| {
            for p in &presets {
                if ui
                    .selectable_label(op.preset.as_deref() == Some(p.name.as_str()), &p.name)
                    .clicked()
                {
                    op.apply_preset(p);
                }
            }
            if ui.selectable_label(op.preset.is_none(), none).clicked() {
                op.preset = None;
            }
        });
    ui.end_row();
}

#[allow(clippy::too_many_arguments)]
fn geometry(
    ui: &mut egui::Ui,
    m: Measure,
    setup: &str,
    index: usize,
    op: &mut Operation,
    diameter: Option<f64>,
    picks: Picks<'_>,
    actions: &mut Vec<PropAction>,
) {
    let drilling = matches!(op.strategy, StrategySpec::Drill { .. });
    let flat = op.strategy.is_2d();
    if flat {
        outline_form(ui, op);
        ui.add_space(theme::SPACE_3);
    }
    let g = &mut op.geometry;
    if drilling {
        hint(
            ui,
            "Pick the round edge at the top of each hole. Pick its bottom edge too to \
             drill to there; otherwise each hole is drilled to the Bottom height.",
        );
        ui.add_space(theme::SPACE_3);
    } else if !flat {
        boundary_form(ui, m, g, diameter);
        ui.add_space(theme::SPACE_3);
    }
    let selection = if flat { g.boundary_2d() } else { g.boundary() };
    if drilling || selection == Boundary::Selection {
        pick_cards(ui, setup, index, g, picks, actions);
    }
    if flat {
        match picks.shown.and_then(|s| s.boundary.as_ref()) {
            Some(Err(why)) => {
                error(ui, why);
            }
            Some(Ok(())) => {
                hint(
                    ui,
                    "In the view at the stock's top: the boundary, and where the tool's \
                     centre goes.",
                );
            }
            None => {}
        }
    } else if !drilling {
        match picks.shown.and_then(|s| s.boundary.as_ref()) {
            Some(Err(why)) => {
                error(ui, why);
            }
            Some(Ok(())) => {
                hint(ui, boundary_hint(g));
            }
            None => {}
        }
    }
}

/// A 2D operation's boundary, what it profiles round or pockets, and a profile's
/// side of it: Fusion's 2D Contour and 2D Pocket Geometry tabs. The tool's centre
/// goes its radius beside the boundary, so there is no containment or offset.
fn outline_form(ui: &mut egui::Ui, op: &mut Operation) {
    form(ui, "outline", |ui| {
        let g = &mut op.geometry;
        widgets::field_label(ui, "Boundary");
        let name = |b: Boundary| match b {
            Boundary::Stock => "the stock",
            Boundary::BoundingBox => "the part's bounding box",
            Boundary::Silhouette => "the part's silhouette",
            Boundary::Selection => "picked faces and edges",
        };
        let mut chosen = g.boundary_2d();
        egui::ComboBox::from_id_salt("outline")
            .selected_text(name(chosen))
            .show_ui(ui, |ui| {
                for b in [
                    Boundary::Stock,
                    Boundary::BoundingBox,
                    Boundary::Silhouette,
                    Boundary::Selection,
                ] {
                    ui.selectable_value(&mut chosen, b, name(b));
                }
            });
        if chosen != g.boundary_2d() {
            g.boundary_kind = Some(chosen);
        }
        ui.end_row();
        if let StrategySpec::Profile { side, .. } = &mut op.strategy {
            widgets::field_label(ui, "Side");
            let name = |s: ProfileSide| match s {
                ProfileSide::Outside => "outside: keep what it encloses",
                ProfileSide::Inside => "inside: cut what it encloses",
                ProfileSide::On => "on it: the tool's centre",
            };
            egui::ComboBox::from_id_salt("side")
                .selected_text(name(*side))
                .show_ui(ui, |ui| {
                    for s in [ProfileSide::Outside, ProfileSide::Inside, ProfileSide::On] {
                        ui.selectable_value(side, s, name(s));
                    }
                });
            ui.end_row();
        }
    });
}

/// The machining boundary, where the tool may go against it, and how far past it:
/// Fusion's Geometry tab.
fn boundary_form(ui: &mut egui::Ui, m: Measure, g: &mut Geometry, diameter: Option<f64>) {
    let before = (g.boundary(), g.containment);
    form(ui, "geometry", |ui| {
        widgets::field_label(ui, "Boundary");
        let name = |b: Boundary| match b {
            Boundary::Stock => "the stock",
            Boundary::BoundingBox => "the part's bounding box",
            Boundary::Silhouette => "the part's silhouette",
            Boundary::Selection => "picked faces and edges",
        };
        let mut chosen = g.boundary();
        egui::ComboBox::from_id_salt("boundary")
            .selected_text(name(chosen))
            .show_ui(ui, |ui| {
                for b in [
                    Boundary::Stock,
                    Boundary::BoundingBox,
                    Boundary::Silhouette,
                    Boundary::Selection,
                ] {
                    ui.selectable_value(&mut chosen, b, name(b));
                }
            });
        if chosen != g.boundary() {
            g.boundary_kind = Some(chosen);
        }
        ui.end_row();
        if g.boundary() != Boundary::Stock {
            widgets::field_label(ui, "Tool");
            let name = |c: Containment| match c {
                Containment::Center => "centre inside the boundary",
                Containment::Inside => "all of it inside",
                Containment::Outside => "reaching in from outside",
            };
            egui::ComboBox::from_id_salt("containment")
                .selected_text(name(g.containment))
                .show_ui(ui, |ui| {
                    for c in [
                        Containment::Center,
                        Containment::Inside,
                        Containment::Outside,
                    ] {
                        ui.selectable_value(&mut g.containment, c, name(c));
                    }
                });
            ui.end_row();
        }
        widgets::field_label(ui, "Additional offset");
        m.signed_length_field(ui, &mut g.margin, 0.1, 1000.0, "");
        ui.end_row();
    });
    // Reaching in from outside the part's own outline, the tool only touches its
    // walls: it needs room past them to clear anything. A tool's width is a
    // channel two wide, room for a finishing tool to follow. Set once, as a
    // number, so changing the tool later does not move the boundary.
    let around_part = matches!(g.boundary(), Boundary::BoundingBox | Boundary::Silhouette);
    if (g.boundary(), g.containment) != before
        && around_part
        && g.containment == Containment::Outside
        && g.margin == 0.0
        && let Some(d) = diameter
    {
        g.margin = d;
    }
}

/// What the view shows for an operation's boundary.
fn boundary_hint(g: &Geometry) -> &'static str {
    let around_part = matches!(g.boundary(), Boundary::BoundingBox | Boundary::Silhouette);
    if around_part && g.containment == Containment::Outside && g.margin <= 0.0 {
        "The tool only touches the part's walls: nothing outside them is cut. An \
         additional offset makes room for a channel round the part."
    } else {
        "In the view at the stock's top: the boundary, where the tool's centre may go, \
         and hatched, what is cut outside the part."
    }
}

/// The cards of picked faces and edges, each with a way to pick more in the view.
fn pick_cards(
    ui: &mut egui::Ui,
    setup: &str,
    index: usize,
    g: &mut Geometry,
    picks: Picks<'_>,
    actions: &mut Vec<PropAction>,
) {
    for (kind, title) in [(Kind::Face, "Faces"), (Kind::Edge, "Edges")] {
        let slot = match kind {
            Kind::Face => Slot::Faces {
                setup: setup.to_string(),
                op: index,
            },
            Kind::Edge => Slot::Edges {
                setup: setup.to_string(),
                op: index,
            },
        };
        let (list, found) = match kind {
            Kind::Face => (&mut g.faces, picks.shown.map(|s| s.faces.as_slice())),
            Kind::Edge => (&mut g.edges, picks.shown.map(|s| s.edges.as_slice())),
        };
        let mut clear = false;
        let mut asked = None;
        let any = !list.is_empty();
        widgets::card(
            ui,
            Some(title),
            |ui| {
                if picks.armed == Some(&slot) {
                    if widgets::button(ui, Variant::Primary, Size::Sm, None, "Done").clicked() {
                        asked = Some(PropAction::Disarm);
                    }
                } else if widgets::button(
                    ui,
                    Variant::Ghost,
                    Size::Sm,
                    Some(Icon::MousePointerClick),
                    "Select in view…",
                )
                .clicked()
                {
                    asked = Some(PropAction::Arm(slot.clone()));
                }
                if any && widgets::button(ui, Variant::Ghost, Size::Sm, None, "Clear").clicked() {
                    clear = true;
                }
            },
            |ui| {
                if list.is_empty() {
                    hint(ui, "None picked.");
                }
                pick_list(ui, kind, list, found.unwrap_or_default(), actions);
            },
        );
        actions.extend(asked);
        if clear {
            list.clear();
        }
        ui.add_space(theme::SPACE_3);
    }
}

/// A list of picks, as the engine finds them: each can be taken out, and a row under
/// the pointer shows its pick in the view.
fn pick_list(
    ui: &mut egui::Ui,
    kind: Kind,
    list: &mut Vec<FeatureRef>,
    found: &[Result<String, String>],
    actions: &mut Vec<PropAction>,
) {
    let mut remove = None;
    for (i, r) in list.iter().enumerate() {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), theme::CONTROL_SM),
            egui::Sense::hover(),
        );
        let hovered = ui.rect_contains_pointer(rect);
        list_row_background(ui, rect, false, hovered);
        if hovered {
            actions.push(PropAction::HoverRow(kind, i));
        }
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(egui::vec2(theme::SPACE_3, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        match found.get(i) {
            Some(Ok(what)) => row.label(what),
            Some(Err(why)) => row.colored_label(widgets::Level::Err.color(), why),
            None => row.label(
                egui::RichText::new(format!("{} on '{}'", r.index, r.model))
                    .color(theme::egui_color(theme::TEXT_3)),
            ),
        };
        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if hovered && widgets::icon_button(ui, Icon::X, "take it out").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        list.remove(i);
    }
}

/// Which of Top and Bottom the strategy cuts to: the others are not asked for.
fn uses_top_bottom(strategy: &StrategySpec) -> (bool, bool) {
    match strategy {
        StrategySpec::ZlevelRough { .. }
        | StrategySpec::Profile { .. }
        | StrategySpec::Pocket { .. } => (true, true),
        StrategySpec::Drill { .. } => (false, true),
        StrategySpec::Parallel { .. } => (false, false),
    }
}

fn heights(ui: &mut egui::Ui, m: Measure, op: &mut Operation) {
    let (top, bottom) = uses_top_bottom(&op.strategy);
    form(ui, "heights", |ui| {
        widgets::field_label(ui, "Clearance");
        height_field(ui, m, "clearance", &mut op.heights.clearance);
        ui.end_row();
        if top {
            widgets::field_label(ui, "Top");
            optional_height(ui, m, "geo-top", &mut op.geometry.top, HeightRef::StockTop);
            ui.end_row();
        }
        if bottom {
            widgets::field_label(ui, "Bottom");
            optional_height(
                ui,
                m,
                "geo-bottom",
                &mut op.geometry.bottom,
                HeightRef::ModelBottom,
            );
            ui.end_row();
        }
    });
    ui.add_space(theme::SPACE_3);
    match op.strategy {
        StrategySpec::Parallel { .. } => {
            hint(
                ui,
                "Parallel follows the part all the way down, so it has no Top or Bottom.",
            );
        }
        StrategySpec::Drill { .. } => {
            hint(
                ui,
                "Each hole starts at its picked rim. Bottom is how deep a hole goes when \
                 its bottom edge is not picked.",
            );
        }
        StrategySpec::Profile { .. } | StrategySpec::Pocket { .. } => {
            hint(
                ui,
                "Without a Bottom, each area is cut down to the lowest point picked in \
                 it, or to the part's bottom when nothing is picked.",
            );
        }
        StrategySpec::ZlevelRough { .. } => {}
    }
    hint(
        ui,
        "The view shows each height as a plane across the stock.",
    );
}

/// The selected operation's heights as planes across the stock, while `tab`, the
/// tab of it showing, is its Heights, as the run will resolve them. None otherwise.
pub fn height_planes(doc: &Document, m: Measure, tab: Option<OperationTab>) -> Vec<Plane> {
    let showing = tab == Some(OperationTab::Heights);
    let Selection::Operation(setup, index) = &doc.selection else {
        return Vec::new();
    };
    let op = doc.operation(setup, *index);
    let prepared = doc
        .setups
        .get(setup)
        .and_then(|s| s.layout.as_ref()?.prepared.as_ref().ok());
    let (true, Some(op), Some(prepared), true) =
        (showing, op, prepared, doc.active.as_ref() == Some(setup))
    else {
        return Vec::new();
    };
    let stock = prepared.stock_bounds();
    let ctx = HeightContext {
        stock_top: stock.max.z,
        stock_bottom: stock.min.z,
        model_top: prepared.model_bounds.max.z,
        model_bottom: prepared.model_bounds.min.z,
    };
    // A little past the stock, so a plane on its top does not hide its edges.
    let grow = 0.04 * (stock.max.x - stock.min.x).max(stock.max.y - stock.min.y);
    let (lo, hi) = (
        stock.min.truncate() - DVec2::splat(grow),
        stock.max.truncate() + DVec2::splat(grow),
    );
    let (top, bottom) = uses_top_bottom(&op.strategy);
    let g = &op.geometry;
    [
        (
            PlaneKind::Clearance,
            "Clearance",
            Some(op.heights.clearance.resolve(&ctx)),
        ),
        (
            PlaneKind::Top,
            "Top",
            top.then(|| g.top.map_or(ctx.stock_top, |h| h.resolve(&ctx))),
        ),
        (
            PlaneKind::Bottom,
            "Bottom",
            bottom.then(|| g.bottom.map_or(ctx.model_bottom, |h| h.resolve(&ctx))),
        ),
    ]
    .into_iter()
    .filter_map(|(kind, name, z)| {
        let z = z?;
        Some(Plane {
            kind,
            z,
            lo,
            hi,
            label: format!("{name}  Z {}", m.length(z, 2)),
        })
    })
    .collect()
}

fn passes(ui: &mut egui::Ui, m: Measure, op: &mut Operation) {
    form(ui, "passes", |ui| {
        match &mut op.strategy {
            StrategySpec::Parallel { angle } => {
                widgets::field_label(ui, "Pass angle");
                ui.add(
                    egui::DragValue::new(angle)
                        .speed(1.0)
                        .range(-360.0..=360.0)
                        .suffix("°"),
                );
                ui.end_row();
            }
            StrategySpec::ZlevelRough {
                stepdown,
                min_ring_length,
            } => {
                widgets::field_label(ui, "Stepdown");
                m.length_field(ui, stepdown, 0.05, 100.0, "");
                ui.end_row();
                widgets::field_label(ui, "Shortest ring");
                m.length_field(ui, min_ring_length, 0.1, 1000.0, "");
                ui.end_row();
            }
            StrategySpec::Profile {
                stepdown,
                conventional,
                ..
            }
            | StrategySpec::Pocket {
                stepdown,
                conventional,
            } => {
                widgets::field_label(ui, "Stepdown");
                m.length_field(ui, stepdown, 0.05, 100.0, "")
                    .on_hover_text("The deepest each level may be: they are spaced evenly");
                ui.end_row();
                widgets::field_label(ui, "Direction");
                let mut climb = !*conventional;
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut climb, true, "climb");
                    ui.selectable_value(&mut climb, false, "conventional");
                });
                *conventional = !climb;
                ui.end_row();
            }
            StrategySpec::Drill { peck, dwell } => {
                widgets::field_label(ui, "Peck");
                ui.horizontal(|ui| {
                    widgets::optional(ui, peck, 2.0, "all the way at once", |ui, p| {
                        m.length_field(ui, p, 0.05, 100.0, "");
                    });
                });
                ui.end_row();
                widgets::field_label(ui, "Dwell");
                ui.horizontal(|ui| {
                    widgets::optional(ui, dwell, 0.5, "none", |ui, d| {
                        ui.add(
                            egui::DragValue::new(d)
                                .speed(0.05)
                                .range(0.0..=60.0)
                                .suffix(" s at the bottom"),
                        );
                    });
                });
                ui.end_row();
            }
        }
        // A profile is one ring round: it has no stepover.
        if !matches!(op.strategy, StrategySpec::Profile { .. }) {
            widgets::field_label(ui, "Stepover");
            m.length_field(ui, &mut op.passes.stepover, 0.05, 100.0, "");
            ui.end_row();
        }
        widgets::field_label(ui, "Leave, sideways");
        m.length_field(ui, &mut op.passes.stock_to_leave.radial, 0.05, 50.0, "");
        ui.end_row();
        widgets::field_label(ui, "Leave, down");
        m.length_field(ui, &mut op.passes.stock_to_leave.axial, 0.05, 50.0, "");
        ui.end_row();
        widgets::field_label(ui, "Deepest step cut");
        m.length_field(ui, &mut op.passes.max_cut_step, 0.05, 50.0, "");
        ui.end_row();
    });
}

/// How the operation moves between cuts. A value left out is the strategy's own,
/// as Fusion's, and says what that comes to for the tool chosen.
fn linking(ui: &mut egui::Ui, m: Measure, op: &mut Operation, diameter: Option<f64>) {
    // A 2D operation links as Z-level roughing does: level by level, ramping in.
    let flat = op.strategy.is_2d();
    let (parallel, zlevel) = (
        matches!(op.strategy, StrategySpec::Parallel { .. }),
        matches!(op.strategy, StrategySpec::ZlevelRough { .. }) || flat,
    );
    let of_tool = |part: f64, what: &str| match diameter {
        Some(d) => format!("{what}: {}", m.length(part * d, 2)),
        None => what.to_string(),
    };
    form(ui, "linking", |ui| {
        widgets::field_label(ui, "Retract feed");
        m.feed_field(ui, &mut op.linking.retract_feed, 50_000.0);
        ui.end_row();
        widgets::field_label(ui, "Safe distance");
        m.length_field(ui, &mut op.linking.safe_distance, 0.1, 1_000.0, "")
            .on_hover_text(
                "How far above the material the tool stops rapiding down and feeds, \
                 and feeds up to on the way out",
            );
        ui.end_row();
        if parallel || zlevel {
            widgets::field_label(ui, "Stay down");
            let absent = if parallel {
                format!("up to {}", m.length(50.0, 0))
            } else {
                of_tool(5.0, "up to 5 × the tool")
            };
            let start = if parallel {
                50.0
            } else {
                5.0 * diameter.unwrap_or(6.0)
            };
            ui.horizontal(|ui| {
                widgets::optional(ui, &mut op.linking.stay_down, start, &absent, |ui, v| {
                    m.length_field(ui, v, 0.5, 10_000.0, "up to ")
                        .on_hover_text(
                            "Links this long or shorter are made without lifting; 0 always lifts",
                        );
                });
            });
            ui.end_row();
        }
        if parallel {
            widgets::field_label(ui, "Lead radius");
            let absent = of_tool(0.1, "a tenth of the tool");
            let start = 0.1 * diameter.unwrap_or(6.0);
            ui.horizontal(|ui| {
                widgets::optional(ui, &mut op.linking.lead_radius, start, &absent, |ui, v| {
                    m.length_field(ui, v, 0.05, 100.0, "").on_hover_text(
                        "The vertical arcs each pass is entered and left along; 0 for none",
                    );
                });
            });
            ui.end_row();
        }
        if zlevel {
            widgets::field_label(ui, "Ramp angle");
            ui.add(
                egui::DragValue::new(&mut op.linking.ramp_angle)
                    .speed(0.1)
                    .range(0.1..=45.0)
                    .suffix("°"),
            )
            .on_hover_text("How steeply the tool ramps down into material, along the pass");
            ui.end_row();
            widgets::field_label(ui, "Ramp feed");
            let absent = format!(
                "a third of cutting: {} {}",
                m.feed_number(op.passes.feed / 3.0),
                m.feed_unit()
            );
            let start = op.passes.feed / 3.0;
            ui.horizontal(|ui| {
                widgets::optional(ui, &mut op.linking.ramp_feed, start, &absent, |ui, v| {
                    m.feed_field(ui, v, 50_000.0);
                });
            });
            ui.end_row();
        }
        // A 2D operation's order is its own: what is inside before what frees
        // it, the middle of a pocket before its walls.
        if !flat {
            widgets::field_label(ui, "Reorder passes");
            ui.checkbox(&mut op.linking.reorder, "to shorten travel");
            ui.end_row();
            widgets::field_label(ui, "Reverse passes");
            ui.checkbox(
                &mut op.linking.allow_reverse,
                "while reordering (changes climb)",
            );
            ui.end_row();
        }
    });
}

fn height_field(ui: &mut egui::Ui, m: Measure, id: &str, h: &mut Height) {
    let name = |r: HeightRef| match r {
        HeightRef::StockTop => "stock top",
        HeightRef::StockBottom => "stock bottom",
        HeightRef::ModelTop => "model top",
        HeightRef::ModelBottom => "model bottom",
        HeightRef::Absolute => "work zero",
    };
    ui.horizontal(|ui| {
        m.signed_length_field(ui, &mut h.offset, 0.1, 10_000.0, "");
        ui.label("from");
        egui::ComboBox::from_id_salt(id)
            .selected_text(name(h.reference))
            .show_ui(ui, |ui| {
                for r in [
                    HeightRef::StockTop,
                    HeightRef::StockBottom,
                    HeightRef::ModelTop,
                    HeightRef::ModelBottom,
                    HeightRef::Absolute,
                ] {
                    ui.selectable_value(&mut h.reference, r, name(r));
                }
            });
    });
}

fn optional_height(
    ui: &mut egui::Ui,
    m: Measure,
    id: &str,
    h: &mut Option<Height>,
    default: HeightRef,
) {
    let absent = match default {
        HeightRef::StockTop => "the stock's top",
        _ => "the part's bottom",
    };
    let start = Height {
        reference: default,
        offset: 0.0,
    };
    ui.horizontal(|ui| {
        widgets::optional(ui, h, start, absent, |ui, h| height_field(ui, m, id, h));
    });
}
