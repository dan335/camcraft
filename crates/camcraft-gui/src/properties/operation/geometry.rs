//! An operation's Geometry tab: what it is kept to or cuts round, and what is
//! picked for it.

use camcraft_core::job::{
    self, Boundary, Containment, FeatureRef, MachiningBoundary, Operation, Outline, ProfileSide,
    StrategySpec,
};
use eframe::egui;

use crate::document::needs_room;
use crate::help::topics::geometry as geo;
use crate::picking::slots::{Kind, Slot};
use crate::properties::{PropAction, ViewPicks};
use crate::ui::measure::Measure;
use crate::ui::theme;
use crate::ui::widgets::{self, Icon, Size, Variant, error, form, hint, list_row_background};

/// Operation `index` of `setup`, `op`, as far as what it cuts goes, with a tool
/// `diameter` across when it has one.
pub(super) fn geometry(
    ui: &mut egui::Ui,
    m: Measure,
    (setup, index): (&str, usize),
    op: &mut Operation,
    diameter: Option<f64>,
    picks: ViewPicks<'_>,
    actions: &mut Vec<PropAction>,
) {
    if matches!(op.strategy, StrategySpec::Face { .. }) {
        face_form(ui, m, op, picks);
        return;
    }
    let uses = op.strategy.uses();
    let drilling = uses.holes;
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
        widgets::row(
            ui,
            if pocket { &geo::POCKET } else { &geo::CONTOUR },
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
            widgets::row(ui, &geo::SIDE, |ui| {
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
fn face_form(ui: &mut egui::Ui, m: Measure, op: &mut Operation, picks: ViewPicks<'_>) {
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
        widgets::row(ui, &geo::BOUNDARY, |ui| {
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
            widgets::row(ui, &geo::CONTAINMENT, |ui| {
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
        widgets::row(ui, &geo::ADDITIONAL_OFFSET, |ui| {
            m.signed_length_field(ui, &mut g.margin, 0.1, 1000.0, "");
        });
    });
    // Chosen just now, a boundary with no room round the part is given it, as a
    // new operation's is: set once, as a number, so changing the tool later
    // does not move it.
    if (g.kind, g.containment) != before
        && needs_room(g)
        && let Some(d) = diameter
    {
        g.margin = d;
    }
}

/// What the view shows for an operation's boundary.
fn boundary_hint(g: &MachiningBoundary) -> &'static str {
    if needs_room(g) {
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
    picks: ViewPicks<'_>,
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
            Some(Err(why)) => row.label(widgets::error_text(why)),
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
