//! An operation's panel, on Fusion's tabs: the tool it cuts with, its
//! geometry (what it is kept to, what it cuts round and what is picked), its
//! heights, its passes, and how its passes are linked.

use camcraft_core::features;
use camcraft_core::job::{
    self, Boundary, Containment, ContourOrder, CutDirection, Depths, Entry, FaceDirection,
    FeatureRef, Height, HeightContext, HeightRef, Leads, Leave, MachiningBoundary, Operation,
    Outline, PassOrder, ProfileSide, RadialDirection, Retraction, StockToLeave, StrategySpec,
    TabShape, Tabs,
};
use camcraft_core::setup::Bodies;
use eframe::egui;
use glam::DVec2;

use super::{Picks, PropAction, cannot_generate, header};
use crate::document::{Document, Level, Selection};
use crate::help::topics::{feeds, geometry as geo, heights as ht, linking as link, passes as pass};
use crate::help::{Help, Topic};
use crate::picking::slots::{Kind, Slot};
use crate::tools::drawing::paint_tool;
use crate::tools::editor::{chip_load_row, surface_speed_row};
use crate::ui::measure::{Measure, follow_radial};
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
    let what = (doc.id, Selection::Operation(setup.to_string(), index));
    if let Some(to) = header(ui, "Operation", id, &current, what) {
        rename(doc, setup, index, &to, &taken);
    }
    cannot_generate(ui, doc, setup);
    let op = doc.operation_mut(setup, index)?;
    form(ui, id.with("strategy"), |ui| {
        widgets::key_label(ui, "Strategy");
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
    let diameter = tool_name
        .as_ref()
        .and_then(|t| doc.job.tools.get(t))
        .map(|t| t.diameter);
    // How far below zero its stock to leave may go: the tool's to say.
    let least = tool_name
        .as_ref()
        .and_then(|t| doc.job.tools.get(t))
        .zip(doc.operation(setup, index))
        .map_or(
            StockToLeave {
                radial: f64::NEG_INFINITY,
                axial: f64::NEG_INFINITY,
            },
            |(t, op)| op.least_stock_to_leave(t),
        );
    if page == OperationTab::Tool {
        tool(ui, doc, m, setup, index, tool_name.as_deref(), actions);
    } else if let Some(op) = doc.operation_mut(setup, index) {
        match page {
            OperationTab::Tool => {}
            OperationTab::Geometry => {
                geometry(ui, m, setup, index, op, diameter, picks, actions);
            }
            OperationTab::Heights => heights(ui, m, op),
            OperationTab::Passes => passes(ui, m, op, least),
            OperationTab::Linking => linking(ui, m, op, diameter),
        }
    }
    Some(page)
}

/// Rename operation `index` of `setup` to `to`, if no other of its setup's
/// operations has the name.
pub(super) fn rename_to(doc: &mut Document, setup: &str, index: usize, to: &str) {
    let taken: Vec<String> = doc
        .setup(setup)
        .map(|s| s.operations.iter().map(|o| o.name.clone()).collect())
        .unwrap_or_default();
    rename(doc, setup, index, to, &taken);
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
        StrategySpec::Scallop { .. } => "scallop",
        StrategySpec::Contour { .. } => "contour",
        StrategySpec::Flat { .. } => "flat",
        StrategySpec::Spiral { .. } => "spiral",
        StrategySpec::Radial { .. } => "radial",
        StrategySpec::MorphedSpiral { .. } => "morphed spiral",
        StrategySpec::PocketClearing { .. } => "pocket clearing",
        StrategySpec::Adaptive { .. } => "adaptive clearing",
        StrategySpec::Profile { .. } => "2D profile",
        StrategySpec::Pocket { .. } => "2D pocket",
        StrategySpec::Face { .. } => "face",
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
        widgets::row(ui, &feeds::TOOL, |ui| {
            match tool_name {
                Some(t) => ui.strong(t),
                None => ui.colored_label(widgets::Level::Warn.color(), "no tool yet"),
            };
            if widgets::button(ui, Variant::Secondary, Size::Sm, None, "Select tool…").clicked() {
                actions.push(PropAction::PickTool(setup.to_string(), index));
            }
        });
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
    widgets::row(ui, &feeds::SPINDLE, |ui| {
        let absent = format!("the tool's, {tool_rpm} rev/min");
        widgets::optional(ui, &mut op.passes.rpm, tool_rpm, &absent, |ui, rpm| {
            ui.add(
                egui::DragValue::new(rpm)
                    .range(1..=60000)
                    .suffix(" rev/min"),
            );
        });
    });
    // What the spindle turns at, the operation's own or the tool's.
    let turning = op.passes.rpm.or(Some(tool_rpm));
    let mut rpm = turning;
    surface_speed_row(ui, &mut rpm, diameter, m);
    if rpm != turning {
        op.passes.rpm = rpm;
    }
    // Drilling feeds its holes at the plunge feed: it has no cutting feed.
    if !matches!(op.strategy, StrategySpec::Drill { .. }) {
        widgets::row(ui, &feeds::CUTTING_FEED, |ui| {
            m.feed_field(ui, &mut op.passes.feed, 50_000.0);
        });
        let mut feed = Some(op.passes.feed);
        chip_load_row(ui, &mut feed, op.passes.rpm.or(Some(tool_rpm)), flutes, m);
        if let Some(f) = feed {
            op.passes.feed = f;
        }
    }
    widgets::row(ui, &feeds::PLUNGE_FEED, |ui| {
        m.feed_field(ui, &mut op.linking.plunge_feed, 50_000.0);
    });
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
    widgets::row(ui, &feeds::PRESET, |ui| {
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
    });
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
    if matches!(op.strategy, StrategySpec::Face { .. }) {
        face_form(ui, m, op, picks);
        return;
    }
    let drilling = matches!(op.strategy, StrategySpec::Drill { .. });
    let uses = op.strategy.uses();
    let flat = uses.outline;
    if flat {
        outline_form(ui, op);
        ui.add_space(theme::SPACE_3);
    }
    if drilling {
        hint(
            ui,
            "Pick the round edge at the top of each hole. Pick its bottom edge too to \
             drill to there; otherwise each hole is drilled to the Bottom height.",
        );
        ui.add_space(theme::SPACE_3);
    } else if uses.boundary {
        boundary_form(ui, m, &mut op.boundary, diameter);
        ui.add_space(theme::SPACE_3);
    }
    if op.cuts_selection() {
        pick_cards(ui, setup, index, &mut op.selection, picks, actions);
    }
    if drilling {
        return;
    }
    match picks.shown.and_then(|s| s.boundary.as_ref()) {
        Some(Err(why)) => {
            error(ui, why);
        }
        Some(Ok(())) if flat => {
            let pocket = matches!(op.strategy, StrategySpec::Pocket { .. });
            let outline = if pocket { "The pocket" } else { "The contour" };
            let mut items = vec![(Swatch::Bold(theme::BOUNDARY), outline)];
            if diameter.is_some() {
                items.push((
                    Swatch::Hatch(theme::CHANNEL),
                    if pocket {
                        "What the tool clears"
                    } else {
                        "What the tool cuts away"
                    },
                ));
            }
            legend(ui, &items);
            hint(
                ui,
                if diameter.is_none() {
                    "Select a tool to see what it cuts."
                } else if op.outline == Outline::Selection {
                    "Both are drawn at the height of what is picked."
                } else {
                    "Both are drawn at the stock's top."
                },
            );
        }
        Some(Ok(())) => {
            let mut items = vec![
                (Swatch::Bold(theme::BOUNDARY), "The boundary"),
                (
                    Swatch::Thin(theme::BOUNDARY_LIMIT),
                    "Where the tool's centre may go",
                ),
            ];
            if diameter.is_some() {
                items.push((
                    Swatch::Hatch(theme::CHANNEL),
                    "What is cut outside the part",
                ));
            }
            legend(ui, &items);
            hint(ui, boundary_hint(&op.boundary));
        }
        None => {}
    }
}

/// How a line in the view is drawn, for its legend.
#[derive(Debug, Clone, Copy)]
enum Swatch {
    /// A wide line.
    Bold([u8; 3]),
    /// A thin one.
    Thin([u8; 3]),
    /// Hatching.
    Hatch([u8; 3]),
}

/// What the lines in the view are: each one's look, and its name.
fn legend(ui: &mut egui::Ui, items: &[(Swatch, &str)]) {
    for &(swatch, name) in items {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 14.0), egui::Sense::hover());
            let painter = ui.painter();
            let (y, x0, x1) = (rect.center().y, rect.left() + 2.0, rect.right() - 2.0);
            match swatch {
                Swatch::Bold(c) => {
                    let stroke = egui::Stroke::new(3.0, theme::egui_color(c));
                    painter.line_segment([egui::pos2(x0, y), egui::pos2(x1, y)], stroke);
                }
                Swatch::Thin(c) => {
                    let stroke = egui::Stroke::new(1.0, theme::egui_color(c));
                    painter.line_segment([egui::pos2(x0, y), egui::pos2(x1, y)], stroke);
                }
                Swatch::Hatch(c) => {
                    let stroke = egui::Stroke::new(1.0, theme::egui_color(c));
                    let r = rect.shrink2(egui::vec2(2.0, 1.0));
                    let mut x = r.left() - r.height();
                    while x < r.right() {
                        let a = egui::pos2(x.max(r.left()), r.bottom() - (x.max(r.left()) - x));
                        let right = (x + r.height()).min(r.right());
                        let b = egui::pos2(right, r.bottom() - (right - x));
                        painter.line_segment([a, b], stroke);
                        x += 4.0;
                    }
                }
            }
            ui.label(name);
        });
    }
    ui.add_space(theme::SPACE_2);
}

/// What a 2D operation cuts, its outline: the part's silhouette or picks, and a
/// profile's side of it. Fusion's 2D Contour and 2D Pocket Geometry tabs, which
/// select what is cut rather than fence the tool in. The tool's centre goes its radius and the
/// stock to leave beside it, so there is no containment or additional offset.
fn outline_form(ui: &mut egui::Ui, op: &mut Operation) {
    let pocket = matches!(op.strategy, StrategySpec::Pocket { .. });
    form(ui, "outline", |ui| {
        let name = |o: Outline| match o {
            Outline::Silhouette => "the part's silhouette",
            Outline::Selection => "picked faces and edges",
        };
        widgets::row(
            ui,
            if pocket { &geo::POCKET } else { &geo::CONTOUR },
            |ui| {
                egui::ComboBox::from_id_salt("outline")
                    .selected_text(name(op.outline))
                    .show_ui(ui, |ui| {
                        for o in [Outline::Silhouette, Outline::Selection] {
                            ui.selectable_value(&mut op.outline, o, name(o));
                        }
                    });
            },
        );
        if let StrategySpec::Profile { side, .. } = &mut op.strategy {
            let name = |s: ProfileSide| match s {
                ProfileSide::Outside => "outside the contour",
                ProfileSide::Inside => "inside the contour",
                ProfileSide::On => "on the contour",
            };
            widgets::row(ui, &geo::SIDE, |ui| {
                egui::ComboBox::from_id_salt("side")
                    .selected_text(name(*side))
                    .show_ui(ui, |ui| {
                        for s in [ProfileSide::Outside, ProfileSide::Inside, ProfileSide::On] {
                            ui.selectable_value(side, s, name(s));
                        }
                    });
            });
        }
    });
    // The side is the area's, not each loop's: a part's holes are cut from
    // inside them, a hole's islands from outside them.
    if let StrategySpec::Profile { side, .. } = op.strategy {
        hint(
            ui,
            match side {
                ProfileSide::Outside => {
                    "Cuts a part out: the tool runs round outside the contour, and \
                     inside any hole in it."
                }
                ProfileSide::Inside => {
                    "Cuts a hole out: the tool runs round inside the contour, and \
                     outside anything left standing in it."
                }
                ProfileSide::On => "Cuts a groove: the tool's centre runs on the contour.",
            },
        );
    }
}

/// What facing faces: the stock's footprint, and how far past it, Fusion's stock
/// offset. It has nothing to pick.
fn face_form(ui: &mut egui::Ui, m: Measure, op: &mut Operation, picks: Picks<'_>) {
    if let StrategySpec::Face { stock_offset, .. } = &mut op.strategy {
        form(ui, "face", |ui| {
            widgets::row(ui, &geo::STOCK_OFFSET, |ui| {
                m.signed_length_field(ui, stock_offset, 0.1, 1000.0, "");
            });
        });
    }
    ui.add_space(theme::SPACE_3);
    match picks.shown.and_then(|s| s.boundary.as_ref()) {
        Some(Err(why)) => {
            error(ui, why);
        }
        Some(Ok(())) => legend(ui, &[(Swatch::Bold(theme::BOUNDARY), "What is faced")]),
        None => {}
    }
    hint(
        ui,
        "Faces the stock's top flat, down to the Bottom height. The tool reaches a \
         radius past what is faced, and every pass runs off the stock.",
    );
}

/// The machining boundary, where the tool may go against it, and how far past it:
/// Fusion's Geometry tab.
fn boundary_form(ui: &mut egui::Ui, m: Measure, g: &mut MachiningBoundary, diameter: Option<f64>) {
    let before = (g.kind, g.containment);
    form(ui, "geometry", |ui| {
        let name = |b: Boundary| match b {
            Boundary::Stock => "the stock",
            Boundary::BoundingBox => "the part's bounding box",
            Boundary::Silhouette => "the part's silhouette",
            Boundary::Selection => "picked faces and edges",
        };
        widgets::row(ui, &geo::BOUNDARY, |ui| {
            egui::ComboBox::from_id_salt("boundary")
                .selected_text(name(g.kind))
                .show_ui(ui, |ui| {
                    for b in [
                        Boundary::Stock,
                        Boundary::BoundingBox,
                        Boundary::Silhouette,
                        Boundary::Selection,
                    ] {
                        ui.selectable_value(&mut g.kind, b, name(b));
                    }
                });
        });
        if g.kind != Boundary::Stock {
            let name = |c: Containment| match c {
                Containment::Center => "centre inside the boundary",
                Containment::Inside => "all of it inside",
                Containment::Outside => "reaching in from outside",
            };
            widgets::row(ui, &geo::CONTAINMENT, |ui| {
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
            });
        }
        widgets::row(ui, &geo::ADDITIONAL_OFFSET, |ui| {
            m.signed_length_field(ui, &mut g.margin, 0.1, 1000.0, "");
        });
    });
    // Reaching in from outside the part's own outline, the tool only touches its
    // walls: it needs room past them to clear anything. A tool's width is a
    // channel two wide, room for a finishing tool to follow. Set once, as a
    // number, so changing the tool later does not move the boundary.
    let around_part = matches!(g.kind, Boundary::BoundingBox | Boundary::Silhouette);
    if (g.kind, g.containment) != before
        && around_part
        && g.containment == Containment::Outside
        && g.margin == 0.0
        && let Some(d) = diameter
    {
        g.margin = d;
    }
}

/// What the view shows for an operation's boundary.
fn boundary_hint(g: &MachiningBoundary) -> &'static str {
    let around_part = matches!(g.kind, Boundary::BoundingBox | Boundary::Silhouette);
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
    selection: &mut job::Selection,
    picks: Picks<'_>,
    actions: &mut Vec<PropAction>,
) {
    for (kind, topic) in [(Kind::Face, &geo::FACES), (Kind::Edge, &geo::EDGES)] {
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
        let list = match kind {
            Kind::Face => &mut selection.faces,
            Kind::Edge => &mut selection.edges,
        };
        // A row for each face, and for each loop of edges; each pick alone, as the
        // job names it, until the view has found them.
        let rows: Vec<PickRow> = match (kind, picks.shown) {
            (Kind::Face, Some(s)) => s
                .faces
                .iter()
                .enumerate()
                .map(|(i, f)| (vec![i], Some(f.clone())))
                .collect(),
            (Kind::Edge, Some(s)) => s
                .edge_loops
                .iter()
                .map(|(picks, what)| (picks.clone(), Some(what.clone())))
                .collect(),
            (_, None) => (0..list.len()).map(|i| (vec![i], None)).collect(),
        };
        let mut clear = false;
        let mut asked = None;
        let any = !list.is_empty();
        widgets::topic_card(
            ui,
            topic,
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
                pick_list(ui, kind, list, &rows, actions);
            },
        );
        actions.extend(asked);
        if clear {
            list.clear();
        }
        ui.add_space(theme::SPACE_3);
    }
}

/// A row of a list of picks: the places in the job's list of the picks it stands
/// for, and what they are or why they cannot be found, once the view has looked.
type PickRow = (Vec<usize>, Option<Result<String, String>>);

/// A list of picks, as the engine finds them, a row for each of `rows`: the picks
/// it stands for, and what they are or why they cannot be found. Each row can be
/// taken out, and a row under the pointer shows its picks in the view.
fn pick_list(
    ui: &mut egui::Ui,
    kind: Kind,
    list: &mut Vec<FeatureRef>,
    rows: &[PickRow],
    actions: &mut Vec<PropAction>,
) {
    let mut remove = None;
    for (picks, found) in rows {
        let Some((&i, r)) = picks.first().and_then(|i| Some((i, list.get(*i)?))) else {
            continue;
        };
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
        match found {
            Some(Ok(what)) => row.label(what),
            Some(Err(why)) => row.colored_label(widgets::Level::Err.color(), why),
            None => row.label(
                egui::RichText::new(format!("{} on '{}'", r.index, r.model))
                    .color(theme::egui_color(theme::TEXT_3)),
            ),
        };
        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if hovered && widgets::icon_button(ui, Icon::X, "take it out").clicked() {
                remove = Some(picks);
            }
        });
    }
    if let Some(picks) = remove {
        let mut i = 0;
        list.retain(|_| {
            i += 1;
            !picks.contains(&(i - 1))
        });
    }
}

/// Where the strategy cuts down to without a Bottom: facing to the part's top,
/// the rest to its bottom.
fn default_bottom(strategy: &StrategySpec) -> HeightRef {
    match strategy {
        StrategySpec::Face { .. } => HeightRef::ModelTop,
        _ => HeightRef::ModelBottom,
    }
}

/// Where the strategy cuts down from without a Top: finishing in levels from the
/// part's top, the rest from the stock's.
fn default_top(strategy: &StrategySpec) -> HeightRef {
    match strategy {
        StrategySpec::Contour { .. } | StrategySpec::Flat { .. } => HeightRef::ModelTop,
        _ => HeightRef::StockTop,
    }
}

/// Which of Top and Bottom the strategy cuts to: the others are not asked for.
fn uses_top_bottom(strategy: &StrategySpec) -> (bool, bool) {
    match strategy {
        StrategySpec::PocketClearing { .. }
        | StrategySpec::Adaptive { .. }
        | StrategySpec::Contour { .. }
        | StrategySpec::Flat { .. }
        | StrategySpec::Profile { .. }
        | StrategySpec::Pocket { .. }
        | StrategySpec::Face { .. } => (true, true),
        StrategySpec::Drill { .. } => (false, true),
        StrategySpec::Parallel { .. }
        | StrategySpec::Scallop { .. }
        | StrategySpec::Spiral { .. }
        | StrategySpec::Radial { .. }
        | StrategySpec::MorphedSpiral { .. } => (false, false),
    }
}

/// Whether a 2D operation goes down to its picks without a Bottom: it has some,
/// and they are what it cuts round.
fn down_to_picks(op: &Operation) -> bool {
    op.strategy.uses().outline && !op.selection.is_empty() && op.outline == Outline::Selection
}

fn heights(ui: &mut egui::Ui, m: Measure, op: &mut Operation) {
    let (top, bottom) = uses_top_bottom(&op.strategy);
    // Only a top or a bottom is measured from the picks, with some picked, and
    // never a drill's, whose holes each go to their own.
    let picks = !op.selection.is_empty() && !matches!(op.strategy, StrategySpec::Drill { .. });
    let unset = if down_to_picks(op) {
        HeightRef::Selection
    } else {
        default_bottom(&op.strategy)
    };
    form(ui, "heights", |ui| {
        widgets::row(ui, &ht::CLEARANCE, |ui| {
            height_field(ui, m, "clearance", &mut op.heights.clearance, false);
        });
        if op.strategy.uses().stay_down {
            widgets::row(ui, &ht::RETRACT, |ui| {
                height_field(ui, m, "retract", &mut op.heights.retract, false);
            });
        }
        if top {
            widgets::row(ui, &ht::TOP, |ui| {
                optional_height(
                    ui,
                    m,
                    "geo-top",
                    &mut op.heights.top,
                    default_top(&op.strategy),
                    picks,
                );
            });
        }
        if bottom {
            widgets::row(ui, &ht::BOTTOM, |ui| {
                optional_height(ui, m, "geo-bottom", &mut op.heights.bottom, unset, picks);
            });
        }
    });
    if let Some(why) = op.cannot_measure_from_picks() {
        error(ui, why);
    }
    ui.add_space(theme::SPACE_3);
    match op.strategy {
        StrategySpec::Parallel { .. }
        | StrategySpec::Scallop { .. }
        | StrategySpec::Spiral { .. }
        | StrategySpec::Radial { .. }
        | StrategySpec::MorphedSpiral { .. } => {
            hint(
                ui,
                "It follows the part all the way down, so it has no Top or Bottom.",
            );
        }
        StrategySpec::Contour { .. } => {
            hint(
                ui,
                "Without a Top or Bottom, its levels run from the part's top down to its \
                 bottom.",
            );
        }
        StrategySpec::Flat { .. } => {
            hint(
                ui,
                "Only the flats between the Top and Bottom are finished; without them, \
                 every flat of the part.",
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
                 it, or to the part's bottom when nothing is picked. From the selection, \
                 each area's Top and Bottom are the highest and lowest points picked in \
                 it, plus the offset.",
            );
        }
        StrategySpec::Face { .. } => {
            hint(
                ui,
                "Without a Bottom, the stock is faced down to the part's top.",
            );
        }
        StrategySpec::PocketClearing { .. } | StrategySpec::Adaptive { .. } => {}
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
    // How high the picks go, when a height is measured from them. One that
    // cannot be found leaves the height where it would be with none.
    let picked = if op.heights.measured_from_selection() || down_to_picks(op) {
        let bodies: Bodies<'_> = doc
            .models
            .iter()
            .filter_map(|(n, slot)| Some((n.as_str(), &slot.model()?.body)))
            .collect();
        features::picked_points(&op.selection, prepared, &bodies, doc.job.tolerance)
            .ok()
            .and_then(|points| job::Picks::of(points.iter().map(|p| p.z)))
    } else {
        None
    };
    let ctx = HeightContext {
        stock_top: stock.max.z,
        stock_bottom: stock.min.z,
        model_top: prepared.model_bounds.max.z,
        model_bottom: prepared.model_bounds.min.z,
        picked,
    };
    // A little past the stock, so a plane on its top does not hide its edges.
    let grow = 0.04 * (stock.max.x - stock.min.x).max(stock.max.y - stock.min.y);
    let (lo, hi) = (
        stock.min.truncate() - DVec2::splat(grow),
        stock.max.truncate() + DVec2::splat(grow),
    );
    let (top, bottom) = uses_top_bottom(&op.strategy);
    // A 2D operation's, over all its areas: each area's own are drawn with its
    // outline.
    let h = &op.heights;
    let (top_z, bottom_z) = if op.strategy.uses().outline {
        op.levels_2d(&ctx, ctx.picked)
    } else {
        let unset = match default_bottom(&op.strategy) {
            HeightRef::ModelTop => ctx.model_top,
            _ => ctx.model_bottom,
        };
        let top = match default_top(&op.strategy) {
            HeightRef::ModelTop => ctx.model_top,
            _ => ctx.stock_top,
        };
        (
            h.top.map_or(top, |h| h.resolve_top(&ctx)),
            h.bottom.map_or(unset, |h| h.resolve_bottom(&ctx)),
        )
    };
    [
        (
            PlaneKind::Clearance,
            "Clearance",
            Some(op.heights.clearance.resolve(&ctx)),
        ),
        (
            PlaneKind::Retract,
            "Retract",
            op.strategy
                .uses()
                .stay_down
                .then(|| op.heights.retract.resolve(&ctx)),
        ),
        (PlaneKind::Top, "Top", top.then_some(top_z)),
        (PlaneKind::Bottom, "Bottom", bottom.then_some(bottom_z)),
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

fn passes(ui: &mut egui::Ui, m: Measure, op: &mut Operation, least: StockToLeave) {
    let uses = op.strategy.uses();
    form(ui, "passes", |ui| {
        match &mut op.strategy {
            StrategySpec::Parallel { angle } => {
                widgets::row(ui, &pass::PASS_ANGLE, |ui| angle_field(ui, angle));
            }
            StrategySpec::Scallop {
                direction,
                order,
                slope_from,
                slope_to,
            } => {
                both_ways_direction(ui, direction);
                pass_order(ui, &pass::SCALLOP_ORDER, order);
                slopes(ui, slope_from, slope_to);
            }
            StrategySpec::Contour {
                direction,
                order,
                slope_from,
                slope_to,
            } => {
                both_ways_direction(ui, direction);
                widgets::row(ui, &pass::CONTOUR_ORDER, |ui| {
                    ui.selectable_value(order, ContourOrder::ByDepth, "by depth");
                    ui.selectable_value(order, ContourOrder::ByArea, "by area");
                });
                slopes(ui, slope_from, slope_to);
            }
            StrategySpec::Flat { direction, order } => {
                climb_direction(ui, &pass::DIRECTION_2D, direction);
                pass_order(ui, &pass::SCALLOP_ORDER, order);
            }
            StrategySpec::Spiral {
                centre,
                inner_radius,
                outer_radius,
                direction,
                order,
                slope_from,
                slope_to,
            } => {
                centred(ui, m, centre, inner_radius, outer_radius, &pass::CENTRE);
                climb_direction(ui, &pass::DIRECTION_SPIRAL, direction);
                pass_order(ui, &pass::SPIRAL_ORDER, order);
                slopes(ui, slope_from, slope_to);
            }
            StrategySpec::Radial {
                centre,
                inner_radius,
                outer_radius,
                angle_from,
                angle_to,
                direction,
                slope_from,
                slope_to,
            } => {
                centred(ui, m, centre, inner_radius, outer_radius, &pass::CENTRE);
                widgets::row(ui, &pass::RADIAL_ANGLES, |ui| {
                    ui.add(
                        egui::DragValue::new(angle_from)
                            .speed(1.0)
                            .range(-360.0..=360.0)
                            .prefix("from ")
                            .suffix("°"),
                    );
                    ui.add(
                        egui::DragValue::new(angle_to)
                            .speed(1.0)
                            .range(-360.0..=720.0)
                            .prefix("to ")
                            .suffix("°"),
                    );
                });
                // A range the job would refuse is put right as it is set.
                if *angle_to <= *angle_from {
                    *angle_to = *angle_from + 1.0;
                }
                if *angle_to > *angle_from + 360.0 {
                    *angle_to = *angle_from + 360.0;
                }
                widgets::row(ui, &pass::RADIAL_DIRECTION, |ui| {
                    for (d, name) in [
                        (RadialDirection::BothWays, "both ways"),
                        (RadialDirection::Outward, "outward"),
                        (RadialDirection::Inward, "inward"),
                    ] {
                        ui.selectable_value(direction, d, name);
                    }
                });
                slopes(ui, slope_from, slope_to);
            }
            StrategySpec::MorphedSpiral {
                centre,
                direction,
                order,
                slope_from,
                slope_to,
            } => {
                widgets::row(ui, &pass::MORPH_CENTRE, |ui| {
                    centre_field(ui, m, centre, "the point furthest inside");
                });
                climb_direction(ui, &pass::DIRECTION_SPIRAL, direction);
                pass_order(ui, &pass::SPIRAL_ORDER, order);
                slopes(ui, slope_from, slope_to);
            }
            StrategySpec::PocketClearing {
                min_ring_length,
                conventional,
                flat_areas,
            } => {
                let mut climb = !*conventional;
                widgets::row(ui, &pass::DIRECTION_2D, |ui| {
                    ui.selectable_value(&mut climb, true, "climb");
                    ui.selectable_value(&mut climb, false, "conventional");
                });
                *conventional = !climb;
                widgets::row(ui, &pass::FLAT_AREAS, |ui| {
                    ui.checkbox(flat_areas, "a level on each flat face");
                });
                widgets::row(ui, &pass::SHORTEST_RING, |ui| {
                    m.length_field(ui, min_ring_length, 0.1, 1000.0, "");
                });
            }
            StrategySpec::Adaptive {
                direction,
                flat_areas,
            } => {
                widgets::row(ui, &pass::DIRECTION_ADAPTIVE, |ui| {
                    for (d, name) in [
                        (CutDirection::Climb, "climb"),
                        (CutDirection::Conventional, "conventional"),
                        (CutDirection::BothWays, "both ways"),
                    ] {
                        ui.selectable_value(direction, d, name);
                    }
                });
                widgets::row(ui, &pass::FLAT_AREAS, |ui| {
                    ui.checkbox(flat_areas, "a level on each flat face");
                });
            }
            StrategySpec::Profile { conventional, .. } | StrategySpec::Pocket { conventional } => {
                let mut climb = !*conventional;
                widgets::row(ui, &pass::DIRECTION_2D, |ui| {
                    ui.selectable_value(&mut climb, true, "climb");
                    ui.selectable_value(&mut climb, false, "conventional");
                });
                *conventional = !climb;
            }
            StrategySpec::Face {
                angle,
                direction,
                pass_extension,
                ..
            } => {
                widgets::row(ui, &pass::PASS_ANGLE, |ui| angle_field(ui, angle));
                widgets::row(ui, &pass::DIRECTION_FACE, |ui| {
                    for (d, name) in [
                        (FaceDirection::BothWays, "both ways"),
                        (FaceDirection::Climb, "climb"),
                        (FaceDirection::Conventional, "conventional"),
                    ] {
                        ui.selectable_value(direction, d, name);
                    }
                });
                widgets::row(ui, &pass::PASS_EXTENSION, |ui| {
                    m.length_field(ui, pass_extension, 0.1, 1000.0, "");
                });
            }
            StrategySpec::Drill { peck, dwell } => {
                widgets::row(ui, &pass::PECK, |ui| {
                    widgets::optional(ui, peck, 2.0, "all the way at once", |ui, p| {
                        m.length_field(ui, p, 0.05, 100.0, "");
                    });
                });
                widgets::row(ui, &pass::DWELL, |ui| {
                    widgets::optional(ui, dwell, 0.5, "none", |ui, d| {
                        ui.add(
                            egui::DragValue::new(d)
                                .speed(0.05)
                                .range(0.0..=60.0)
                                .suffix(" s at the bottom"),
                        );
                    });
                });
            }
        }
        if uses.stepover {
            // Adaptive clearing's stepover is the width of cut it holds.
            if matches!(op.strategy, StrategySpec::Adaptive { .. }) {
                widgets::row(ui, &pass::OPTIMAL_LOAD, |ui| {
                    m.length_field(ui, &mut op.passes.stepover, 0.01, 100.0, "");
                });
            } else {
                widgets::row(ui, &pass::STEPOVER, |ui| {
                    m.length_field(ui, &mut op.passes.stepover, 0.05, 100.0, "");
                });
            }
        }
        widgets::row(ui, &pass::DEEPEST_STEP, |ui| {
            m.length_field(ui, &mut op.passes.max_cut_step, 0.05, 50.0, "");
        });
    });
    if uses.depths != Depths::None {
        ui.add_space(theme::SPACE_4);
        ui.label(widgets::section_label("Multiple depths"));
        ui.add_space(theme::SPACE_2);
        multiple_depths(ui, m, op);
    }
    if uses.stock_to_leave != Leave::None {
        ui.add_space(theme::SPACE_4);
        ui.label(widgets::section_label("Stock to leave"));
        ui.add_space(theme::SPACE_2);
        stock_to_leave(
            ui,
            m,
            &mut op.passes.stock_to_leave,
            uses.stock_to_leave,
            least,
        );
    }
    if uses.tabs {
        ui.add_space(theme::SPACE_4);
        ui.label(widgets::section_label("Tabs"));
        ui.add_space(theme::SPACE_2);
        tabs(ui, m, &mut op.passes.tabs);
    }
}

/// The direction passes run, in degrees from X.
fn angle_field(ui: &mut egui::Ui, angle: &mut f64) {
    ui.add(
        egui::DragValue::new(angle)
            .speed(1.0)
            .range(-360.0..=360.0)
            .suffix("°"),
    );
}

/// Multiple depths, as Fusion's Passes tab has them: ticked to cut in levels,
/// then the deepest each may be. Unticked is the bottom in one pass. A strategy
/// that always cuts in levels, as roughing does, cannot be unticked.
fn multiple_depths(ui: &mut egui::Ui, m: Measure, op: &mut Operation) {
    let required = op.strategy.uses().depths == Depths::Required;
    let mut on = op.passes.stepdown.is_some();
    let r = ui
        .add_enabled(
            !(required && on),
            egui::Checkbox::new(&mut on, "Step down in levels"),
        )
        .help(&pass::MULTIPLE_DEPTHS)
        .on_disabled_hover_text(format!(
            "{} always cuts in levels",
            strategy_name(&op.strategy)
        ));
    if r.changed() {
        op.passes.stepdown = on.then(|| op.strategy.default_stepdown().unwrap_or(2.0));
    }
    let Some(stepdown) = &mut op.passes.stepdown else {
        return;
    };
    ui.add_space(theme::SPACE_2);
    form(ui, "multiple-depths", |ui| {
        widgets::row(ui, &pass::STEPDOWN, |ui| {
            m.length_field(ui, stepdown, 0.05, 100.0, "");
        });
    });
}

/// Stock to leave, as Fusion's Passes tab has it: ticked to leave any, then how
/// much on the walls and floors. Unticked is none, and ticked starts at
/// roughing's half a millimetre each way; set back to none, it unticks.
fn stock_to_leave(
    ui: &mut egui::Ui,
    m: Measure,
    stock: &mut StockToLeave,
    leave: Leave,
    least: StockToLeave,
) {
    let mut on = *stock != StockToLeave::default();
    if ui
        .checkbox(&mut on, "Leave stock for a finishing pass")
        .help(&pass::STOCK_TO_LEAVE)
        .changed()
    {
        *stock = if on {
            StockToLeave {
                radial: 0.5,
                axial: 0.5,
            }
        } else {
            StockToLeave::default()
        };
    }
    if !on {
        return;
    }
    ui.add_space(theme::SPACE_2);
    let before = *stock;
    form(ui, "stock-to-leave", |ui| {
        if leave == Leave::Both {
            widgets::row(ui, &pass::WALLS, |ui| {
                m.wall_to_leave_field(ui, &mut stock.radial, least.radial, "");
            });
        }
        widgets::row(ui, &pass::FLOORS, |ui| {
            m.floor_to_leave_field(ui, &mut stock.axial, least.axial, "");
        });
    });
    follow_radial(stock, before);
}

/// Tabs left across the deepest cut to hold what it frees, and their size.
fn tabs(ui: &mut egui::Ui, m: Measure, tabs: &mut Option<Tabs>) {
    let mut on = tabs.is_some();
    ui.checkbox(&mut on, "Leave tabs to hold the part")
        .help(&pass::TABS);
    match (on, tabs.is_some()) {
        (true, false) => *tabs = Some(Tabs::default()),
        (false, true) => *tabs = None,
        _ => {}
    }
    let Some(t) = tabs else {
        return;
    };
    ui.add_space(theme::SPACE_2);
    form(ui, "tabs", |ui| {
        let name = |s: TabShape| match s {
            TabShape::Rectangular => "rectangular",
            TabShape::Triangular => "triangular: ramped, no plunge",
        };
        widgets::row(ui, &pass::TAB_SHAPE, |ui| {
            egui::ComboBox::from_id_salt("tab-shape")
                .selected_text(name(t.shape))
                .show_ui(ui, |ui| {
                    for s in [TabShape::Rectangular, TabShape::Triangular] {
                        ui.selectable_value(&mut t.shape, s, name(s));
                    }
                });
        });
        widgets::row(ui, &pass::TAB_WIDTH, |ui| {
            m.length_field(ui, &mut t.width, 0.1, 1000.0, "");
        });
        widgets::row(ui, &pass::TAB_HEIGHT, |ui| {
            m.length_field(ui, &mut t.height, 0.1, 1000.0, "");
        });
        widgets::row(ui, &pass::TAB_SPACING, |ui| {
            m.length_field(ui, &mut t.spacing, 1.0, 100_000.0, "about ");
        });
    });
    hint(
        ui,
        "Tabs go evenly round each loop cut at the deepest level, and every cut \
         over one rides over its top.",
    );
}

/// Which way a finishing pass cuts, both ways among them.
fn both_ways_direction(ui: &mut egui::Ui, direction: &mut CutDirection) {
    widgets::row(ui, &pass::DIRECTION_SCALLOP, |ui| {
        for (d, name) in [
            (CutDirection::BothWays, "both ways"),
            (CutDirection::Climb, "climb"),
            (CutDirection::Conventional, "conventional"),
        ] {
            ui.selectable_value(direction, d, name);
        }
    });
}

/// Which way a pass cuts, climb or conventional: both ways, which has no
/// meaning for it, reads as climb.
fn climb_direction(ui: &mut egui::Ui, topic: &Topic, direction: &mut CutDirection) {
    let mut climb = *direction != CutDirection::Conventional;
    widgets::row(ui, topic, |ui| {
        ui.selectable_value(&mut climb, true, "climb");
        ui.selectable_value(&mut climb, false, "conventional");
    });
    let chosen = if climb {
        CutDirection::Climb
    } else {
        CutDirection::Conventional
    };
    // Left as it was unless changed, so a job's both ways is kept.
    if (*direction == CutDirection::Conventional) == climb {
        *direction = chosen;
    }
}

/// Outside in or inside out.
fn pass_order(ui: &mut egui::Ui, topic: &Topic, order: &mut PassOrder) {
    widgets::row(ui, topic, |ui| {
        ui.selectable_value(order, PassOrder::OutsideIn, "outside in");
        ui.selectable_value(order, PassOrder::InsideOut, "inside out");
    });
}

/// The range of slopes a finishing pass keeps to, put right as it is set if the
/// job would refuse it.
fn slopes(ui: &mut egui::Ui, slope_from: &mut f64, slope_to: &mut f64) {
    widgets::row(ui, &pass::SLOPES, |ui| {
        ui.add(
            egui::DragValue::new(slope_from)
                .speed(1.0)
                .range(0.0..=89.0)
                .prefix("from ")
                .suffix("°"),
        );
        ui.add(
            egui::DragValue::new(slope_to)
                .speed(1.0)
                .range(1.0..=90.0)
                .prefix("to ")
                .suffix("°"),
        );
    });
    if *slope_from >= *slope_to {
        *slope_to = (*slope_from + 1.0).min(90.0);
        *slope_from = slope_from.min(*slope_to - 1.0);
    }
}

/// A pattern's centre, given or `absent`, as X and Y from the work zero.
fn centre_field(ui: &mut egui::Ui, m: Measure, centre: &mut Option<[f64; 2]>, absent: &str) {
    widgets::optional(ui, centre, [0.0, 0.0], absent, |ui, c| {
        let [x, y] = c;
        ui.label("X");
        m.signed_length_field(ui, x, 0.1, 10_000.0, "");
        ui.label("Y");
        m.signed_length_field(ui, y, 0.1, 10_000.0, "");
    });
}

/// A pattern about a centre's centre and radii, the inner kept inside the outer.
fn centred(
    ui: &mut egui::Ui,
    m: Measure,
    centre: &mut Option<[f64; 2]>,
    inner: &mut f64,
    outer: &mut f64,
    topic: &Topic,
) {
    widgets::row(ui, topic, |ui| {
        centre_field(ui, m, centre, "the boundary's middle");
    });
    widgets::row(ui, &pass::INNER_RADIUS, |ui| {
        m.length_field(ui, inner, 0.1, 10_000.0, "");
    });
    // Zero is none given.
    let mut given = (*outer > 0.0).then_some(*outer);
    widgets::row(ui, &pass::OUTER_RADIUS, |ui| {
        widgets::optional(
            ui,
            &mut given,
            *inner + 10.0,
            "all of the boundary",
            |ui, r| {
                m.length_field(ui, r, 0.1, 10_000.0, "");
            },
        );
    });
    *outer = given.unwrap_or(0.0);
    if *outer > 0.0 && *inner >= *outer {
        *outer = *inner + 1.0;
    }
}

/// How the operation moves between cuts. A value left out is the strategy's own,
/// as Fusion's, and says what that comes to for the tool chosen.
fn linking(ui: &mut egui::Ui, m: Measure, op: &mut Operation, diameter: Option<f64>) {
    let uses = op.strategy.uses();
    // Finishing that follows the part stays down over it; the rest level by
    // level.
    let parallel = op.strategy.follows_part();
    let of_tool = |part: f64, what: &str| match diameter {
        Some(d) => format!("{what}: {}", m.length(part * d, 2)),
        None => what.to_string(),
    };
    form(ui, "linking", |ui| {
        widgets::row(ui, &link::RETRACT_FEED, |ui| {
            m.feed_field(ui, &mut op.linking.retract_feed, 50_000.0);
        });
        if uses.stay_down {
            let name = |r: Retraction| match r {
                Retraction::Minimum => "minimum, clear of the material",
                Retraction::Full => "full, to the retract height",
            };
            widgets::row(ui, &link::RETRACTION, |ui| {
                egui::ComboBox::from_id_salt("retraction")
                    .selected_text(name(op.linking.retraction))
                    .show_ui(ui, |ui| {
                        for r in [Retraction::Minimum, Retraction::Full] {
                            ui.selectable_value(&mut op.linking.retraction, r, name(r));
                        }
                    });
            });
        }
        widgets::row(ui, &link::SAFE_DISTANCE, |ui| {
            m.length_field(ui, &mut op.linking.safe_distance, 0.1, 1_000.0, "");
        });
        if uses.stay_down {
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
            widgets::row(ui, &link::STAY_DOWN, |ui| {
                widgets::optional(ui, &mut op.linking.stay_down, start, &absent, |ui, v| {
                    m.length_field(ui, v, 0.5, 10_000.0, "up to ");
                });
            });
        }
        if uses.lead_radius {
            let absent = of_tool(0.1, "a tenth of the tool");
            let start = 0.1 * diameter.unwrap_or(6.0);
            widgets::row(ui, &link::LEAD_RADIUS, |ui| {
                widgets::optional(ui, &mut op.linking.lead_radius, start, &absent, |ui, v| {
                    m.length_field(ui, v, 0.05, 100.0, "");
                });
            });
        }
        if uses.ramp {
            widgets::row(ui, &link::RAMP_ANGLE, |ui| {
                ui.add(
                    egui::DragValue::new(&mut op.linking.ramp_angle)
                        .speed(0.1)
                        .range(0.1..=45.0)
                        .suffix("°"),
                );
            });
            let absent = format!(
                "60% of cutting: {} {}",
                m.feed_number(op.ramp_feed()),
                m.feed_unit()
            );
            let start = op.ramp_feed();
            widgets::row(ui, &link::RAMP_FEED, |ui| {
                widgets::optional(ui, &mut op.linking.ramp_feed, start, &absent, |ui, v| {
                    m.feed_field(ui, v, 50_000.0);
                });
            });
            let stepdown = op.stepdown();
            let absent = match (stepdown, diameter) {
                (Some(d), _) => format!("the stepdown: {}", m.length(d, 2)),
                (None, _) => of_tool(1.0, "the tool's diameter"),
            };
            let start = stepdown.or(diameter).unwrap_or(6.0);
            widgets::row(ui, &link::MOST_PER_LAP, |ui| {
                widgets::optional(
                    ui,
                    &mut op.linking.ramp_stepdown,
                    start,
                    &absent,
                    |ui, v| {
                        m.length_field(ui, v, 0.1, 1000.0, "");
                    },
                );
            });
        }
        if uses.entry {
            let name = |e: Entry| match e {
                Entry::Helix => "helix beside the lead",
                Entry::Plunge => "plunge beside the lead",
                Entry::Ramp => "ramp along the ring",
            };
            let mut entry = op.linking.entry.unwrap_or_default();
            widgets::row(ui, &link::ENTRY, |ui| {
                egui::ComboBox::from_id_salt("entry")
                    .selected_text(name(entry))
                    .show_ui(ui, |ui| {
                        for e in [Entry::Helix, Entry::Plunge, Entry::Ramp] {
                            ui.selectable_value(&mut entry, e, name(e));
                        }
                    });
            });
            // The default is written only when changed.
            op.linking.entry = (entry != Entry::default()).then_some(entry);
            if entry == Entry::Helix {
                let absent = of_tool(0.9, "0.9 × the tool");
                let start = 0.9 * diameter.unwrap_or(6.0);
                widgets::row(ui, &link::HELIX_DIAMETER, |ui| {
                    widgets::optional(
                        ui,
                        &mut op.linking.helix_diameter,
                        start,
                        &absent,
                        |ui, v| {
                            m.length_field(ui, v, 0.1, 1000.0, "");
                        },
                    );
                });
            }
        }
        if uses.leads {
            let mut on = op.linking.leads.is_some();
            widgets::row(ui, &link::LEADS, |ui| {
                ui.checkbox(&mut on, "on and off each ring");
            });
            match (on, op.linking.leads.is_some()) {
                (true, false) => op.linking.leads = Some(Leads::default()),
                (false, true) => op.linking.leads = None,
                _ => {}
            }
            if let Some(l) = &mut op.linking.leads {
                let start = 0.1 * diameter.unwrap_or(6.0);
                let tenth = of_tool(0.1, "a tenth of the tool");
                for (topic, v) in [
                    (&link::LEAD_VERTICAL, &mut l.vertical),
                    (&link::LEAD_STRAIGHT, &mut l.straight),
                    (&link::LEAD_ARC, &mut l.radius),
                ] {
                    widgets::row(ui, topic, |ui| {
                        widgets::optional(ui, v, start, &tenth, |ui, v| {
                            m.length_field(ui, v, 0.05, 1000.0, "");
                        });
                    });
                }
                widgets::row(ui, &link::LEAD_SWEEP, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut l.sweep)
                            .speed(1.0)
                            .range(1.0..=180.0)
                            .suffix("°"),
                    );
                });
            }
        }
        if uses.reorder {
            widgets::row(ui, &link::REORDER, |ui| {
                ui.checkbox(&mut op.linking.reorder, "to shorten travel");
            });
            widgets::row(ui, &link::REVERSE, |ui| {
                ui.checkbox(
                    &mut op.linking.allow_reverse,
                    "while reordering (changes climb)",
                );
            });
        }
    });
}

/// A height, an offset from what it is measured from; from the picks too when
/// `picks`. One measured from them already keeps it on the list either way, so
/// it reads as what it is until changed.
fn height_field(ui: &mut egui::Ui, m: Measure, id: &str, h: &mut Height, picks: bool) {
    let name = |r: HeightRef| match r {
        HeightRef::StockTop => "stock top",
        HeightRef::StockBottom => "stock bottom",
        HeightRef::ModelTop => "model top",
        HeightRef::ModelBottom => "model bottom",
        HeightRef::Absolute => "work zero",
        HeightRef::Selection => "selection",
    };
    let picks = picks || h.from_selection();
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
                ]
                .into_iter()
                .chain(picks.then_some(HeightRef::Selection))
                {
                    ui.selectable_value(&mut h.reference, r, name(r));
                }
            });
    });
}

/// A top or bottom that need not be given: `default` when it is not, which it
/// starts from when given. `picks` as [`height_field`]'s.
fn optional_height(
    ui: &mut egui::Ui,
    m: Measure,
    id: &str,
    h: &mut Option<Height>,
    default: HeightRef,
    picks: bool,
) {
    let absent = match default {
        HeightRef::StockTop => "the stock's top",
        HeightRef::ModelTop => "the part's top",
        HeightRef::Selection => "the picks",
        _ => "the part's bottom",
    };
    let start = Height {
        reference: default,
        offset: 0.0,
    };
    ui.horizontal(|ui| {
        widgets::optional(ui, h, start, absent, |ui, h| {
            height_field(ui, m, id, h, picks);
        });
    });
}
