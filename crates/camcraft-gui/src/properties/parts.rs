//! What a setup is made of: the model panel, and the list of names a setup (or a
//! stock) includes, with a way to add more from a filtered list or by clicking
//! them in the view. The stock panel is in `stock`.

use camcraft_core::job::{LengthUnit, Setup, Transform};
use eframe::egui;

use super::{PropAction, ViewPicks, header, renamed};
use crate::document::{Document, ModelSlot, Selection, is_stl};
use crate::help::Help;
use crate::help::topics::model as topics;
use crate::picking::slots::Slot;
use crate::ui::measure::Measure;
use crate::ui::theme;
use crate::ui::widgets::{self, Icon, Size, Variant, error, form, hint, list_row_background};

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
    picks: ViewPicks<'_>,
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
                            widgets::icon_button(ui, Icon::MousePointerClick, "Select in view…")
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
                hint(ui, n.empty);
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
                hint(
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
    let what = (doc.id, Selection::Model(name.to_string()));
    if let Some(to) = header(ui, "Model", ui.id().with(("model", name)), name, what) {
        let r = doc.rename_model(name, &to);
        renamed(doc, r);
        return;
    }
    let failed = matches!(doc.models.get(name), Some(ModelSlot::Failed { .. }));
    if let Some(def) = doc.job.models.get_mut(i) {
        form(ui, "model", |ui| {
            widgets::key_label(ui, "File");
            ui.add(egui::Label::new(widgets::mono(&def.path, theme::TEXT_SM)).wrap());
            ui.end_row();
            if is_stl(&def.path) {
                widgets::row(ui, &topics::UNITS, |ui| {
                    widgets::segmented(
                        ui,
                        &mut def.units,
                        &[
                            (Some(LengthUnit::Mm), "mm"),
                            (Some(LengthUnit::Inch), "inch"),
                        ],
                    );
                });
            }
        });
        ui.add_space(theme::SPACE_3);
        transform(ui, m, name, &mut def.transform);
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
                hint(ui, "loading");
            });
        }
        Some(ModelSlot::Failed { why, .. }) => {
            error(ui, why);
        }
        None => {
            hint(ui, "not loaded");
        }
    }
    if failed
        && widgets::button(ui, Variant::Secondary, Size::Sm, None, "Find it elsewhere…").clicked()
    {
        actions.push(PropAction::Locate(name.to_string()));
    }
    ui.add_space(theme::SPACE_2);
    used_by(
        ui,
        doc,
        |s| s.models.iter().any(|x| x == name),
        "No setup machines it.",
        "Machined by",
    );
}

/// Where a model sits in the job: its position, its rotation about the job's X,
/// then Y, then Z, and its scale, uniform unless unlocked, as Fusion's.
fn transform(ui: &mut egui::Ui, m: Measure, name: &str, t: &mut Transform) {
    ui.horizontal(|ui| {
        ui.label(widgets::section_label("Transform"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let moved = *t != Transform::default();
            let reset = ui.add_enabled(
                moved,
                egui::Button::new(widgets::hint_text("reset")).frame(false),
            );
            if reset.on_hover_text("back where its file puts it").clicked() {
                *t = Transform::default();
            }
        });
    });
    let id = ui.id().with(("uniform-scale", name));
    let even = t.scale.iter().all(|&s| s.to_bits() == t.scale[0].to_bits());
    let mut uniform = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(even);
    form(ui, "transform", |ui| {
        widgets::row(ui, &topics::POSITION, |ui| {
            for (axis, v) in ["X ", "Y ", "Z "].into_iter().zip(t.position.iter_mut()) {
                m.signed_length_field(ui, v, 0.1, 100_000.0, axis);
            }
        });
        widgets::row(ui, &topics::ROTATION, |ui| {
            for (axis, v) in ["X ", "Y ", "Z "].into_iter().zip(t.rotation.iter_mut()) {
                ui.add(
                    egui::DragValue::new(v)
                        .speed(1.0)
                        .range(-360.0..=360.0)
                        .prefix(axis)
                        .suffix("°"),
                );
            }
        });
        widgets::row(ui, &topics::SCALE, |ui| {
            let scale = |v: &mut f64, prefix: &str, ui: &mut egui::Ui| {
                ui.add(
                    egui::DragValue::new(v)
                        .speed(0.01)
                        .range(0.001..=1000.0)
                        .prefix(prefix)
                        .suffix("×")
                        .max_decimals(4),
                )
                .changed()
            };
            if uniform {
                let mut s = t.scale[0];
                if scale(&mut s, "", ui) {
                    t.scale = [s; 3];
                }
            } else {
                for (axis, v) in ["X ", "Y ", "Z "].into_iter().zip(t.scale.iter_mut()) {
                    scale(v, axis, ui);
                }
            }
            if ui
                .checkbox(&mut uniform, "uniform")
                .help(&topics::UNIFORM)
                .changed()
                && uniform
            {
                t.scale = [t.scale[0]; 3];
            }
        });
    });
    ui.data_mut(|d| d.insert_temp(id, uniform));
}

/// Which setups use a model or stock, those `uses` holds for: "`some` A, B.", or
/// `none` when no setup does.
pub(super) fn used_by(
    ui: &mut egui::Ui,
    doc: &Document,
    uses: impl Fn(&Setup) -> bool,
    none: &str,
    some: &str,
) {
    let users: Vec<&str> = doc
        .job
        .setups
        .iter()
        .filter(|s| uses(s))
        .map(|s| s.name.as_str())
        .collect();
    if users.is_empty() {
        hint(ui, none);
    } else {
        hint(ui, &format!("{some} {}.", users.join(", ")));
    }
}

/// The box around every loaded model, in model coordinates.
pub(super) fn all_model_bounds(doc: &Document) -> Option<camcraft_geom::mesh::Aabb> {
    // The job's models: one undone out of it may still be loaded, for a redo.
    doc.job
        .models
        .iter()
        .filter_map(|d| doc.models.get(&d.name)?.model().map(|m| m.bounds))
        .reduce(|a, b| a.union(&b))
}
