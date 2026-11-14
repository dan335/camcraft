//! An operation's Geometry tab: what it is kept to or cuts round, and what is
//! picked for it.

use camcraft_core::job::{
    self, Boundary, Containment, Curves, FeatureRef, Holes, MachiningBoundary, Operation, Outline,
    ProfileSide, StrategySpec,
};
use eframe::egui;

use crate::help::Help;
use crate::help::topics::geometry as geo;
use crate::picking::slots::{Kind, Slot};
use crate::properties::{PropAction, ViewPicks};
use crate::ui::defaults::Defaults;
use crate::ui::measure::Measure;
use crate::ui::theme;
use crate::ui::widgets::{self, Icon, Size, Variant, error, form, hint, list_row_background};

/// Operation `index` of `setup`, `op`, as far as what it cuts goes, with a tool
/// `diameter` across when it has one.
pub(super) fn geometry(
    ui: &mut egui::Ui,
    (d, op): (&mut Defaults<Operation>, &mut Operation),
    m: Measure,
    (setup, index): (&str, usize),
    diameter: Option<f64>,
    picks: ViewPicks<'_>,
    actions: &mut Vec<PropAction>,
) {
    if matches!(op.strategy, StrategySpec::Face { .. }) {
        face_form(ui, d, m, op, picks);
        return;
    }
    let uses = op.strategy.uses();
    let drilling = uses.holes.is_some();
    let flat = uses.outline;
    if flat {
        outline_form(ui, d, op);
        ui.add_space(theme::SPACE_3);
    }
    if let Some(what) = what_to_pick(&op.strategy) {
        hint(ui, what);
        ui.add_space(theme::SPACE_3);
    } else if uses.boundary {
        boundary_form(ui, d, m, &mut op.boundary, diameter);
        ui.add_space(theme::SPACE_3);
        if rest_form(ui, d, m, op) {
            ui.add_space(theme::SPACE_3);
        }
    }
    if op.cuts_selection() {
        // Round walls are faces, as Fusion's Bore and Circular pick them; a
        // trace's curves are edges, as Fusion's Trace picks them.
        let kinds: &[Kind] = if uses.holes == Some(Holes::Walls) {
            &[Kind::Face]
        } else if uses.curves == Some(Curves::Traced) {
            &[Kind::Edge]
        } else {
            &[Kind::Face, Kind::Edge]
        };
        pick_cards(
            ui,
            (setup, index),
            (kinds, uses.curves),
            &mut op.selection,
            picks,
            actions,
        );
    }
    if drilling || uses.curves.is_some() {
        return;
    }
    if picks.shown.is_some_and(|s| s.waiting) {
        hint(
            ui,
            "Working out the part's silhouette: the boundary is drawn once it is.",
        );
        return;
    }
    match picks.shown.and_then(|s| s.boundary.as_ref()) {
        Some(Err(why)) => {
            error(ui, why);
        }
        Some(Ok(())) if flat => {
            let pocket = matches!(
                op.strategy,
                StrategySpec::Pocket { .. } | StrategySpec::Adaptive2d { .. }
            );
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

/// What to pick for a strategy whose picks are what it cuts, its holes or what
/// it runs along, rather than an area: `None` for one that cuts an area.
fn what_to_pick(strategy: &StrategySpec) -> Option<&'static str> {
    if matches!(strategy, StrategySpec::Thread { .. }) {
        return Some(
            "Pick the round wall of each hole to thread, bored to size first: it is \
             threaded from its own bottom to its own top. Only holes with straight walls \
             are threaded; a boss is not yet.",
        );
    }
    let uses = strategy.uses();
    Some(match (uses.holes, uses.curves) {
        (Some(Holes::Walls), _) => {
            "Pick the round wall of each hole or boss: it is cut from its own top down \
             to its own bottom, from inside a hole and round the outside of a boss. \
             Only straight walls are cut."
        }
        (Some(Holes::Rims), _) => {
            "Pick the round edge at the top of each hole: it is drilled down to its \
             own bottom, where its wall ends. Pick its bottom edge too to drill to \
             there instead."
        }
        (None, Some(Curves::Slots)) => {
            "Pick the edges round each slot's floor, or its floor: it is cut along its \
             middle from the Top down to the floor. A slot has two sides a constant \
             width apart and round ends; the tool may be no wider."
        }
        (None, Some(Curves::Traced)) => {
            "Pick the edges to follow, open or closed: the tool follows each at its own \
             height, on it or its radius beside it. Edges joined end to end are \
             followed as one."
        }
        (None, Some(Curves::Chamfered)) => {
            "Pick the edges to chamfer, or faces to chamfer all their edges: a sharp \
             edge, or the lower edge of a chamfer the model has. A click takes the loop \
             an edge goes round, or the edges running on from it."
        }
        (None, Some(Curves::Engraved)) => {
            "Pick the rim of each shape to engrave, and of any island in it, or its floor: \
             the V follows its middle, as deep as its flank touches the walls. A shape \
             sunk less deep than the V goes is cut into."
        }
        (None, None) => return None,
    })
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
fn outline_form(ui: &mut egui::Ui, d: &mut Defaults<Operation>, op: &mut Operation) {
    let pocket = matches!(
        op.strategy,
        StrategySpec::Pocket { .. } | StrategySpec::Adaptive2d { .. }
    );
    form(ui, "outline", |ui| {
        d.row(
            ui,
            if pocket { &geo::POCKET } else { &geo::CONTOUR },
            "outline",
            |ui| {
                widgets::select(
                    ui,
                    "outline",
                    &mut op.outline,
                    &[Outline::Silhouette, Outline::Selection],
                    |o| match o {
                        Outline::Silhouette => "the part's silhouette",
                        Outline::Selection => "picked faces and edges",
                    },
                );
            },
        );
        if let StrategySpec::Profile { side, .. } = &mut op.strategy {
            d.row(ui, &geo::SIDE, "side", |ui| {
                widgets::select(
                    ui,
                    "side",
                    side,
                    &[ProfileSide::Outside, ProfileSide::Inside, ProfileSide::On],
                    |s| match s {
                        ProfileSide::Outside => "outside the contour",
                        ProfileSide::Inside => "inside the contour",
                        ProfileSide::On => "on the contour",
                    },
                );
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
fn face_form(
    ui: &mut egui::Ui,
    d: &mut Defaults<Operation>,
    m: Measure,
    op: &mut Operation,
    picks: ViewPicks<'_>,
) {
    if let StrategySpec::Face { stock_offset, .. } = &mut op.strategy {
        form(ui, "face", |ui| {
            d.row(ui, &geo::STOCK_OFFSET, "stock_offset", |ui| {
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

/// Rest machining, as Fusion's Geometry tab has it, for a strategy that has
/// it: ticked to clear only what the operations before left, then how thin a
/// skin of it is let be. Whether it was shown.
fn rest_form(
    ui: &mut egui::Ui,
    d: &mut Defaults<Operation>,
    m: Measure,
    op: &mut Operation,
) -> bool {
    let StrategySpec::Adaptive {
        rest_machining,
        ignore_stock_less_than,
        ..
    } = &mut op.strategy
    else {
        return false;
    };
    ui.horizontal(|ui| {
        ui.checkbox(rest_machining, "Rest machining: only what was left")
            .help(&geo::REST_MACHINING);
        d.reset(ui, "rest_machining");
    });
    if *rest_machining {
        ui.add_space(theme::SPACE_2);
        widgets::sub_form(ui, "rest", |ui| {
            d.row(ui, &geo::IGNORE_LESS_THAN, "ignore_stock_less_than", |ui| {
                m.length_field(ui, ignore_stock_less_than, 0.01, 100.0, "");
            });
        });
        hint(
            ui,
            "What the operations above this one cut is not material: it waits for them.",
        );
    }
    true
}

/// The machining boundary, where the tool may go against it, and how far past it:
/// Fusion's Geometry tab.
fn boundary_form(
    ui: &mut egui::Ui,
    d: &mut Defaults<Operation>,
    m: Measure,
    g: &mut MachiningBoundary,
    diameter: Option<f64>,
) {
    let before = (g.kind, g.containment);
    form(ui, "geometry", |ui| {
        d.row(ui, &geo::BOUNDARY, "boundary.kind", |ui| {
            widgets::select(
                ui,
                "boundary",
                &mut g.kind,
                &[
                    Boundary::Stock,
                    Boundary::BoundingBox,
                    Boundary::Silhouette,
                    Boundary::Selection,
                ],
                |b| match b {
                    Boundary::Stock => "the stock",
                    Boundary::BoundingBox => "the part's bounding box",
                    Boundary::Silhouette => "the part's silhouette",
                    Boundary::Selection => "picked faces and edges",
                },
            );
        });
        if g.kind != Boundary::Stock {
            d.row(ui, &geo::CONTAINMENT, "boundary.containment", |ui| {
                widgets::select(
                    ui,
                    "containment",
                    &mut g.containment,
                    &[
                        Containment::Center,
                        Containment::Inside,
                        Containment::Outside,
                    ],
                    |c| match c {
                        Containment::Center => "centre inside the boundary",
                        Containment::Inside => "all of it inside",
                        Containment::Outside => "reaching in from outside",
                    },
                );
            });
        }
        d.row(ui, &geo::ADDITIONAL_OFFSET, "boundary.margin", |ui| {
            m.signed_length_field(ui, &mut g.margin, 0.1, 1000.0, "");
        });
    });
    // Chosen just now, a boundary with no room round the part is given it, as a
    // new operation's is: set once, as a number, so changing the tool later
    // does not move it.
    if (g.kind, g.containment) != before
        && g.needs_room()
        && let Some(d) = diameter
    {
        g.margin = d;
    }
}

/// What the view shows for an operation's boundary.
fn boundary_hint(g: &MachiningBoundary) -> &'static str {
    if g.needs_room() {
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
    (setup, index): (&str, usize),
    (kinds, curves): (&[Kind], Option<Curves>),
    selection: &mut job::Selection,
    picks: ViewPicks<'_>,
    actions: &mut Vec<PropAction>,
) {
    for &kind in kinds {
        let topic = match (kind, curves) {
            (Kind::Face, Some(Curves::Chamfered)) => &geo::CHAMFER_FACES,
            (Kind::Face, Some(Curves::Engraved)) => &geo::ENGRAVE_FACES,
            (Kind::Face, _) => &geo::FACES,
            (Kind::Edge, Some(Curves::Traced)) => &geo::CURVES,
            (Kind::Edge, Some(Curves::Chamfered)) => &geo::CHAMFER_EDGES,
            (Kind::Edge, Some(Curves::Engraved)) => &geo::ENGRAVE_EDGES,
            (Kind::Edge, _) => &geo::EDGES,
        };
        let slot = match (kind, curves) {
            (Kind::Face, _) => Slot::Faces {
                setup: setup.to_string(),
                op: index,
            },
            // A trace's edges run on along smooth joints, open or closed.
            (Kind::Edge, Some(Curves::Traced)) => Slot::Curves {
                setup: setup.to_string(),
                op: index,
            },
            // A chamfer's, round their loop, or along their chain where
            // they close none.
            (Kind::Edge, Some(Curves::Chamfered)) => Slot::Chains {
                setup: setup.to_string(),
                op: index,
            },
            (Kind::Edge, _) => Slot::Edges {
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
            Some(Err(why)) => row.label(widgets::error_text(why)),
            None => row.label(
                egui::RichText::new(format!("{} on '{}'", r.index, r.model))
                    .color(theme::egui_color(theme::text_3())),
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
