//! What a setup is made of: the model and stock panels, and the list of names a
//! setup (or a stock around models) includes, with a way to add more from a
//! filtered list or by clicking them in the view.

use camcraft_core::job::{LengthUnit, StockSpec};
use eframe::egui;

use super::{Picks, PropAction, error, header, quiet, renamed};
use crate::document::{Document, ModelSlot, Selection};
use crate::measure::Measure;
use crate::slots::Slot;
use crate::theme;
use crate::tool_editor::list_row_background;
use crate::widgets::{self, Icon, Size, Variant, form};

/// A list of names included from a larger one: a card with each included name
/// and a way to take it out, and in its header a way to add more.
pub(super) struct Names<'a> {
    /// Where its popup's state is kept.
    pub id: egui::Id,
    /// The card's title.
    pub title: &'a str,
    /// Every name that could be included, in order.
    pub all: &'a [String],
    /// What an empty list means.
    pub empty: &'a str,
    /// The slot that picks them in the view, and why it cannot, if it cannot.
    pub pick: Option<(Slot, Option<&'a str>)>,
    /// What the popup's last row asks for: files to add, for models.
    pub more: Option<(&'a str, PropAction)>,
}

/// `names` as a card, editing `included`.
pub(super) fn names(
    ui: &mut egui::Ui,
    n: &Names<'_>,
    included: &mut Vec<String>,
    picks: Picks<'_>,
    actions: &mut Vec<PropAction>,
) {
    let others: Vec<&String> = n.all.iter().filter(|x| !included.contains(x)).collect();
    let mut add = None;
    let mut remove = None;
    widgets::card(
        ui,
        Some(n.title),
        |ui| {
            // Right to left: picking in the view, then adding from a list.
            if let Some((slot, why_not)) = &n.pick {
                if picks.armed == Some(slot) {
                    if widgets::button(ui, Variant::Primary, Size::Sm, None, "Done").clicked() {
                        actions.push(PropAction::Disarm);
                    }
                } else {
                    let r = ui
                        .add_enabled_ui(why_not.is_none(), |ui| {
                            widgets::icon_button(ui, Icon::MousePointerClick, "Select in the view")
                        })
                        .inner;
                    if r.clicked() {
                        actions.push(PropAction::Arm(slot.clone()));
                    }
                    if let Some(why) = why_not {
                        r.on_disabled_hover_text(*why);
                    }
                }
            }
            let button = widgets::button(ui, Variant::Ghost, Size::Sm, Some(Icon::Plus), "Add");
            egui::Popup::from_toggle_button_response(&button)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .align(egui::RectAlign::BOTTOM_END)
                .width(220.0)
                .show(|ui| {
                    add = add_list(ui, n, &others, actions);
                });
        },
        |ui| {
            if included.is_empty() {
                quiet(ui, n.empty);
            }
            for (i, name) in included.iter().enumerate() {
                if row(ui, name, !n.all.contains(name)) {
                    remove = Some(i);
                }
            }
        },
    );
    if let Some(name) = add {
        included.push(name);
    }
    if let Some(i) = remove {
        included.remove(i);
    }
}

/// The popup's list: the names not yet included, filtered by what is typed.
/// The one clicked, if any.
fn add_list(
    ui: &mut egui::Ui,
    n: &Names<'_>,
    others: &[&String],
    actions: &mut Vec<PropAction>,
) -> Option<String> {
    let filter_id = n.id.with("filter");
    let mut filter: String = ui.data(|d| d.get_temp(filter_id)).unwrap_or_default();
    let mut picked = None;
    if others.len() > 6 {
        ui.add(
            egui::TextEdit::singleline(&mut filter)
                .hint_text("filter")
                .desired_width(f32::INFINITY),
        )
        .request_focus();
    }
    let wanted = filter.to_lowercase();
    let shown: Vec<&&String> = others
        .iter()
        .filter(|x| x.to_lowercase().contains(&wanted))
        .collect();
    egui::ScrollArea::vertical()
        .max_height(240.0)
        .show(ui, |ui| {
            if shown.is_empty() {
                quiet(
                    ui,
                    if others.is_empty() {
                        "All of them are in already."
                    } else {
                        "None match."
                    },
                );
            }
            for name in shown {
                if ui
                    .add(egui::Button::new(name.as_str()).frame(false))
                    .clicked()
                {
                    picked = Some((*name).clone());
                }
            }
        });
    if let Some((label, action)) = &n.more {
        ui.separator();
        if ui.add(egui::Button::new(*label).frame(false)).clicked() {
            actions.push(action.clone());
            ui.close();
        }
    }
    ui.data_mut(|d| d.insert_temp(filter_id, filter));
    picked
}

/// One included name, with a way to take it out at its right: whether it was.
fn row(ui: &mut egui::Ui, name: &str, missing: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme::CONTROL_SM),
        egui::Sense::hover(),
    );
    let hovered = ui.rect_contains_pointer(rect);
    list_row_background(ui, rect, false, hovered);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(theme::SPACE_3, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let color = if missing {
        widgets::Level::Err.color()
    } else {
        theme::egui_color(theme::TEXT_1)
    };
    child.label(egui::RichText::new(name).color(color));
    if missing {
        response.on_hover_text("not in the job");
    }
    let mut out = false;
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if widgets::icon_button(ui, Icon::X, "Remove").clicked() {
            out = true;
        }
    });
    out
}

pub(super) fn model(
    ui: &mut egui::Ui,
    doc: &mut Document,
    m: Measure,
    name: &str,
    actions: &mut Vec<PropAction>,
) {
    let Some(i) = doc.job.models.iter().position(|x| x.name == name) else {
        doc.selection = Selection::Models;
        return;
    };
    if let Some(to) = header(ui, "Model", ui.id().with(("model", name)), name) {
        let r = doc.rename_model(name, &to);
        renamed(doc, r);
        return;
    }
    let failed = matches!(doc.models.get(name), Some(ModelSlot::Failed { .. }));
    if let Some(def) = doc.job.models.get_mut(i) {
        form(ui, "model", |ui| {
            widgets::field_label(ui, "File");
            ui.add(egui::Label::new(widgets::mono(&def.path, theme::TEXT_SM)).wrap());
            ui.end_row();
            let stl = std::path::Path::new(&def.path)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("stl"));
            if stl {
                widgets::field_label(ui, "Units");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut def.units, Some(LengthUnit::Mm), "mm");
                    ui.radio_value(&mut def.units, Some(LengthUnit::Inch), "inch")
                        .on_hover_text("An STL does not say; check before cutting.");
                });
                ui.end_row();
            }
        });
    }

    ui.add_space(theme::SPACE_3);
    match doc.models.get(name) {
        Some(ModelSlot::Ready(_, model, _)) => {
            let e = model.bounds.extent();
            ui.label(
                widgets::mono(
                    format!(
                        "{} × {} × {} · {} triangles · {}",
                        m.number(e.x, 1),
                        m.number(e.y, 1),
                        m.length(e.z, 1),
                        model.mesh().len(),
                        model.units
                    ),
                    theme::TEXT_SM,
                )
                .color(theme::egui_color(theme::TEXT_2)),
            );
        }
        Some(ModelSlot::Loading(_)) => {
            ui.horizontal(|ui| {
                ui.spinner();
                quiet(ui, "loading");
            });
        }
        Some(ModelSlot::Failed { why, .. }) => {
            error(ui, why);
        }
        None => {
            quiet(ui, "not loaded");
        }
    }
    if failed
        && widgets::button(ui, Variant::Secondary, Size::Sm, None, "Find it elsewhere…").clicked()
    {
        actions.push(PropAction::Locate(name.to_string()));
    }
    let users: Vec<String> = doc
        .job
        .setups
        .iter()
        .filter(|s| s.models.iter().any(|x| x == name))
        .map(|s| s.name.clone())
        .collect();
    ui.add_space(theme::SPACE_2);
    quiet(
        ui,
        &if users.is_empty() {
            "No setup machines it.".to_string()
        } else {
            format!("Machined by {}.", users.join(", "))
        },
    );
}

pub(super) fn stock(ui: &mut egui::Ui, doc: &mut Document, m: Measure, name: &str) {
    let Some(i) = doc.job.stocks.iter().position(|x| x.name == name) else {
        doc.selection = Selection::Stocks;
        return;
    };
    let id = ui.id().with(("stock", name));
    if let Some(to) = header(ui, "Stock", id, name) {
        let r = doc.rename_stock(name, &to);
        renamed(doc, r);
        return;
    }
    let model_names: Vec<String> = doc.job.models.iter().map(|x| x.name.clone()).collect();
    let fit = all_model_bounds(doc);
    let Some(spec) = doc.job.stocks.get_mut(i).map(|s| &mut s.spec) else {
        return;
    };
    form(ui, "stock", |ui| {
        widgets::field_label(ui, "Shape");
        let around = matches!(spec, StockSpec::AroundModels { .. });
        egui::ComboBox::from_id_salt("stock-kind")
            .selected_text(if around { "around models" } else { "fixed box" })
            .show_ui(ui, |ui| {
                if ui.selectable_label(around, "around models").clicked() && !around {
                    *spec = StockSpec::default();
                }
                if ui.selectable_label(!around, "fixed box").clicked() && around {
                    let (corner, size) = fit.map_or(([0.0; 3], [100.0, 100.0, 25.0]), |b| {
                        (b.min.to_array(), b.extent().to_array())
                    });
                    *spec = StockSpec::FixedBox { corner, size };
                }
            });
        ui.end_row();
    });
    ui.add_space(theme::SPACE_4);
    match spec {
        StockSpec::AroundModels {
            models,
            margin,
            below,
        } => {
            names(
                ui,
                &Names {
                    id: id.with("around"),
                    title: "Around",
                    all: &model_names,
                    empty: "The models of the setup using it.",
                    pick: None,
                    more: None,
                },
                models,
                Picks::default(),
                &mut Vec::new(),
            );
            ui.add_space(theme::SPACE_4);
            form(ui, "stock-margins", |ui| {
                widgets::field_label(ui, "Each side, X");
                m.length_field(ui, &mut margin[0], 0.1, 1000.0, "");
                ui.end_row();
                widgets::field_label(ui, "Each side, Y");
                m.length_field(ui, &mut margin[1], 0.1, 1000.0, "");
                ui.end_row();
                widgets::field_label(ui, "On top");
                m.length_field(ui, &mut margin[2], 0.1, 1000.0, "");
                ui.end_row();
                widgets::field_label(ui, "Underneath");
                m.length_field(ui, below, 0.1, 1000.0, "");
                ui.end_row();
            });
        }
        StockSpec::FixedBox { corner, size } => {
            form(ui, "stock-box", |ui| {
                widgets::field_label(ui, "Corner");
                ui.horizontal(|ui| {
                    for (axis, v) in ["X ", "Y ", "Z "].into_iter().zip(corner.iter_mut()) {
                        m.signed_length_field(ui, v, 0.1, 10_000.0, axis);
                    }
                });
                ui.end_row();
                widgets::field_label(ui, "Size");
                ui.horizontal(|ui| {
                    for (axis, v) in ["X ", "Y ", "Z "].into_iter().zip(size.iter_mut()) {
                        m.length_field(ui, v, 0.1, 10_000.0, axis);
                    }
                });
                ui.end_row();
                ui.label("");
                if ui
                    .add_enabled_ui(fit.is_some(), |ui| {
                        widgets::button(ui, Variant::Secondary, Size::Sm, None, "Fit to the models")
                    })
                    .inner
                    .clicked()
                    && let Some(b) = fit
                {
                    *corner = b.min.to_array();
                    *size = b.extent().to_array();
                }
                ui.end_row();
            });
        }
    }
    ui.add_space(theme::SPACE_3);
    let users: Vec<String> = doc
        .job
        .setups
        .iter()
        .filter(|s| s.stocks.iter().any(|x| x == name))
        .map(|s| s.name.clone())
        .collect();
    quiet(
        ui,
        &if users.is_empty() {
            "No setup uses it.".to_string()
        } else {
            format!("Used by {}.", users.join(", "))
        },
    );
}

/// The box around every loaded model, in model coordinates.
fn all_model_bounds(doc: &Document) -> Option<camcraft_geom::mesh::Aabb> {
    doc.models
        .values()
        .filter_map(|s| s.model().map(|m| m.bounds))
        .reduce(|a, b| a.union(&b))
}
