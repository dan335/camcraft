//! Where a setup's work zero is, chosen the way a machinist says it: a corner or
//! side of the stock (or of the models' box) from a small top view, a height,
//! and an offset from there; or a point picked on the part in the view.

use camcraft_core::job::{Along, BoxOf, Origin};
use eframe::egui;

use super::{PropAction, ViewPicks};
use crate::help::Help;
use crate::help::topics::setup as topics;
use crate::picking::describe::describe_box_point;
use crate::picking::slots::Slot;
use crate::ui::measure::Measure;
use crate::ui::theme;
use crate::ui::widgets::{self, Icon, Size, Variant, error, form, hint};

/// What the work zero is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum On {
    Stock,
    ModelBox,
    ModelZero,
    Picked,
}

impl On {
    fn of(origin: &Origin) -> Self {
        match origin {
            Origin::BoxPoint {
                of: BoxOf::Stock, ..
            } => On::Stock,
            Origin::BoxPoint {
                of: BoxOf::Model, ..
            } => On::ModelBox,
            Origin::ModelOrigin { .. } => On::ModelZero,
            Origin::Point { .. } => On::Picked,
        }
    }

    fn name(self) -> &'static str {
        match self {
            On::Stock => "the stock",
            On::ModelBox => "the models' box",
            On::ModelZero => "the model's own zero",
            On::Picked => "a point picked on the part",
        }
    }
}

/// The work zero card for setup `setup`.
pub(super) fn show(
    ui: &mut egui::Ui,
    m: Measure,
    setup: &str,
    origin: &mut Origin,
    picks: ViewPicks<'_>,
    actions: &mut Vec<PropAction>,
) {
    let slot = Slot::WorkZero {
        setup: setup.to_string(),
    };
    widgets::card(
        ui,
        Some("Work zero"),
        |ui| {
            if picks.armed == Some(&slot) {
                if widgets::button(ui, Variant::Primary, Size::Sm, None, "Done").clicked() {
                    actions.push(PropAction::Disarm);
                }
            } else if widgets::button(
                ui,
                Variant::Ghost,
                Size::Sm,
                Some(Icon::MousePointerClick),
                "Select in view…",
            )
            .on_hover_text("a corner, a round edge's centre, or a corner or side of the stock")
            .clicked()
            {
                actions.push(PropAction::Arm(slot.clone()));
            }
        },
        |ui| {
            on_row(ui, origin);
            ui.add_space(theme::SPACE_3);
            match origin {
                Origin::BoxPoint { of, x, y, z, .. } => {
                    ui.horizontal(|ui| {
                        box_grid(ui, x, y);
                        ui.add_space(theme::SPACE_4);
                        ui.vertical(|ui| {
                            for (a, label) in [
                                (Along::Max, "Top"),
                                (Along::Center, "Middle"),
                                (Along::Min, "Bottom"),
                            ] {
                                ui.radio_value(z, a, label).help(&topics::WORK_ZERO_HEIGHT);
                            }
                        });
                    });
                    let what = match of {
                        BoxOf::Stock => "stock",
                        BoxOf::Model => "models' box",
                    };
                    hint(
                        ui,
                        &format!("The {what}'s {}.", describe_box_point(*x, *y, *z)),
                    );
                }
                Origin::Point { .. } => picked_row(ui, origin, picks),
                Origin::ModelOrigin { .. } => {
                    hint(ui, "Where the design's own zero is, turned with the part.");
                }
            }
            ui.add_space(theme::SPACE_3);
            offset(ui, m, origin);
        },
    );
}

/// "On [the stock ▾]": what the work zero is measured from. A picked point is
/// chosen by picking, so it is shown but not offered.
fn on_row(ui: &mut egui::Ui, origin: &mut Origin) {
    let on = On::of(origin);
    let offset = origin.offset();
    ui.horizontal(|ui| {
        widgets::field_label(ui, &topics::WORK_ZERO_ON);
        egui::ComboBox::from_id_salt("origin-on")
            .selected_text(on.name())
            .show_ui(ui, |ui| {
                for choice in [On::Stock, On::ModelBox, On::ModelZero] {
                    if ui.selectable_label(on == choice, choice.name()).clicked() && on != choice {
                        *origin = match choice {
                            On::ModelZero => Origin::ModelOrigin { offset },
                            _ => {
                                // Keep the point on the box when only the box changes.
                                let (x, y, z) = match origin {
                                    Origin::BoxPoint { x, y, z, .. } => (*x, *y, *z),
                                    _ => (Along::Min, Along::Min, Along::Max),
                                };
                                let of = if choice == On::Stock {
                                    BoxOf::Stock
                                } else {
                                    BoxOf::Model
                                };
                                Origin::BoxPoint {
                                    of,
                                    x,
                                    y,
                                    z,
                                    offset,
                                }
                            }
                        };
                    }
                }
            })
            .response
            .help(&topics::WORK_ZERO_ON);
    });
}

/// The box seen from above: nine points, X to the right and Y away. Click one to
/// put the work zero over it.
fn box_grid(ui: &mut egui::Ui, x: &mut Along, y: &mut Along) {
    const SIDE: f32 = 104.0;
    const INSET: f32 = 16.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(SIDE, SIDE), egui::Sense::click());
    let inner = rect.shrink(INSET);
    let along = [Along::Min, Along::Center, Along::Max];
    let at = |ax: Along, ay: Along| {
        let f = |a: Along| match a {
            Along::Min => 0.0,
            Along::Center => 0.5,
            Along::Max => 1.0,
        };
        egui::pos2(
            inner.left() + inner.width() * f(ax),
            inner.bottom() - inner.height() * f(ay),
        )
    };
    let nearest = response.hover_pos().and_then(|p| {
        along
            .iter()
            .flat_map(|ax| along.iter().map(move |ay| (*ax, *ay)))
            .map(|(ax, ay)| ((ax, ay), at(ax, ay).distance(p)))
            .filter(|(_, d)| *d < INSET * 1.5)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(xy, _)| xy)
    });
    if response.clicked()
        && let Some((ax, ay)) = nearest
    {
        *x = ax;
        *y = ay;
    }
    let painter = ui.painter();
    painter.rect(
        inner,
        theme::RADIUS_SM,
        theme::egui_color(theme::SURFACE_1),
        egui::Stroke::new(1.0, theme::egui_color(theme::BORDER_STRONG)),
        egui::StrokeKind::Middle,
    );
    for ax in along {
        for ay in along {
            let p = at(ax, ay);
            if (ax, ay) == (*x, *y) {
                painter.circle_filled(p, 5.0, theme::egui_color(theme::ACCENT));
            } else if nearest == Some((ax, ay)) {
                painter.circle_filled(p, 4.0, theme::egui_color(theme::HOVER));
            } else {
                painter.circle_stroke(
                    p,
                    3.5,
                    egui::Stroke::new(1.0, theme::egui_color(theme::TEXT_3)),
                );
            }
        }
    }
    // Which way is which: X to the right, Y away from the operator.
    let small = egui::FontId::monospace(theme::TEXT_XS);
    let faint = theme::egui_color(theme::TEXT_4);
    painter.text(
        egui::pos2(inner.right() + 6.0, inner.bottom() + 4.0),
        egui::Align2::LEFT_TOP,
        "X",
        small.clone(),
        faint,
    );
    painter.text(
        egui::pos2(inner.left() - 6.0, inner.top() - 4.0),
        egui::Align2::RIGHT_BOTTOM,
        "Y",
        small,
        faint,
    );
    response.help(&topics::WORK_ZERO_POINT);
}

/// A point picked on the part: what it is, or why it cannot be found, and a way
/// back to the stock's corner.
fn picked_row(ui: &mut egui::Ui, origin: &mut Origin, picks: ViewPicks<'_>) {
    let mut back = false;
    ui.horizontal(|ui| {
        match picks.shown.and_then(|s| s.origin.as_ref()) {
            Some(Ok(what)) => {
                ui.label(what);
            }
            Some(Err(why)) => {
                error(ui, why);
            }
            None => {
                hint(ui, "the model is not loaded yet");
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            back = widgets::icon_button(ui, Icon::X, "back to a corner of the stock").clicked();
        });
    });
    if back {
        *origin = Origin::stock_corner().with_offset(origin.offset());
    }
}

/// The offset from the chosen point, folded away while it is nothing.
fn offset(ui: &mut egui::Ui, m: Measure, origin: &mut Origin) {
    let offset = match origin {
        Origin::ModelOrigin { offset }
        | Origin::BoxPoint { offset, .. }
        | Origin::Point { offset, .. } => offset,
    };
    let summary = format!(
        "Offset  {}, {}, {}",
        m.number(offset[0], 2),
        m.number(offset[1], 2),
        m.length(offset[2], 2)
    );
    let id = ui.make_persistent_id("work-zero-offset");
    let moved = offset.iter().any(|v| *v != 0.0);
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, moved)
        .show_header(ui, |ui| {
            ui.label(egui::RichText::new(summary).color(theme::egui_color(theme::TEXT_2)));
        })
        .body(|ui| {
            form(ui, "work-zero-offset-fields", |ui| {
                for (label, v) in ["X", "Y", "Z"].into_iter().zip(offset.iter_mut()) {
                    widgets::row_as(ui, label, &topics::WORK_ZERO_OFFSET, |ui| {
                        m.signed_length_field(ui, v, 0.1, 10_000.0, "");
                    });
                }
            });
        });
}
