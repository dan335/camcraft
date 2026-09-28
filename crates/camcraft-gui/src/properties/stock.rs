//! A stock's panel, laid out as the tool library lays out a tool: a drawing of
//! the block around the part, its size and corners in figures, then its fields.
//! For a fixed box, those are one row per axis with the size, the place and the
//! offset side by side. For a box around models, they are one row per margin.

use camcraft_core::job::{FixedBox, Place, StockSpec};
use camcraft_geom::mesh::Aabb;
use eframe::egui;
use glam::DVec3;

use super::parts::{Names, all_model_bounds, names, used_by};
use super::{Picks, header, renamed};
use crate::document::{Document, Selection};
use crate::ui::measure::Measure;
use crate::ui::theme;
use crate::ui::widgets::{self, Size, Variant, form, hint};

/// How tall the drawing is, as the tool's.
const PICTURE: f32 = 180.0;
/// How wide a place chooser is, so the rows' offsets line up.
const PLACE_WIDTH: f32 = 116.0;

pub(super) fn stock(ui: &mut egui::Ui, doc: &mut Document, m: Measure, name: &str) {
    let Some(i) = doc.job.stocks.iter().position(|x| x.name == name) else {
        doc.selection = Selection::Stocks;
        return;
    };
    let id = ui.id().with(("stock", name));
    if let Some(to) = header(
        ui,
        "Stock",
        id,
        name,
        (doc.id, Selection::Stock(name.into())),
    ) {
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
                    let size = fit.map_or([100.0, 100.0, 25.0], |b| b.extent().to_array());
                    *spec = StockSpec::FixedBox(FixedBox::centred(size));
                }
            });
        ui.end_row();
    });
    ui.add_space(theme::SPACE_4);

    // The block as it stands before this frame's edits: drawn, then edited.
    let placed = doc
        .job
        .stocks
        .get(i)
        .map(|s| placements(doc, name, &s.spec))
        .unwrap_or_default();
    if let Some(first) = placed.first() {
        paint_stock(ui, first, m);
        ui.add_space(theme::SPACE_3);
    }
    corners(ui, &placed, m);
    ui.add_space(theme::SPACE_4);

    let Some(spec) = doc.job.stocks.get_mut(i).map(|s| &mut s.spec) else {
        return;
    };
    match spec {
        StockSpec::AroundModels {
            models,
            margin,
            below,
        } => {
            margins(ui, m, margin, below);
            ui.add_space(theme::SPACE_4);
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
        }
        StockSpec::FixedBox(b) => {
            axes(ui, m, b);
            ui.add_space(theme::SPACE_3);
            if ui
                .add_enabled_ui(fit.is_some(), |ui| {
                    widgets::button(ui, Variant::Secondary, Size::Sm, None, "Fit to the models")
                })
                .inner
                .on_hover_text("Size it to the models, centred, its top flush with theirs")
                .clicked()
                && let Some(f) = fit
            {
                *b = FixedBox {
                    models: std::mem::take(&mut b.models),
                    ..FixedBox::centred(f.extent().to_array())
                };
            }
            ui.add_space(theme::SPACE_4);
            names(
                ui,
                &Names {
                    id: id.with("against"),
                    title: "Placed against",
                    all: &model_names,
                    empty: "The models of the setup using it.",
                    pick: None,
                    more: None,
                },
                &mut b.models,
                Picks::default(),
                &mut Vec::new(),
            );
        }
    }
    ui.add_space(theme::SPACE_3);
    used_by(
        ui,
        doc,
        |s| s.stocks.iter().any(|x| x == name),
        "No setup uses it.",
        "Used by",
    );
}

/// One axis of a fixed box, as a person names its sides.
struct Axis {
    letter: &'static str,
    low: &'static str,
    high: &'static str,
    low_hint: &'static str,
    high_hint: &'static str,
}

const AXES: [Axis; 3] = [
    Axis {
        letter: "X",
        low: "from left",
        high: "from right",
        low_hint: "Its left side (−X) this far past the models'",
        high_hint: "Its right side (+X) this far past the models'",
    },
    Axis {
        letter: "Y",
        low: "from front",
        high: "from back",
        low_hint: "Its front (−Y) this far past the models'",
        high_hint: "Its back (+Y) this far past the models'",
    },
    Axis {
        letter: "Z",
        low: "from bottom",
        high: "from top",
        low_hint: "Its bottom this far below the models'",
        high_hint: "Its top this far above the models'",
    },
];

/// A fixed box's size and place, a row an axis: the size, where it sits against
/// the models, and by how much.
fn axes(ui: &mut egui::Ui, m: Measure, b: &mut FixedBox) {
    egui::Grid::new("stock-axes")
        .num_columns(4)
        .spacing([theme::SPACE_3, 6.0])
        .show(ui, |ui| {
            ui.label("");
            ui.label(widgets::section_label("Size"));
            ui.label(widgets::section_label("Position"));
            ui.label(widgets::section_label("Offset"));
            ui.end_row();
            for ((axis, size), place) in AXES.iter().zip(&mut b.size).zip(&mut b.place) {
                ui.label(widgets::mono(axis.letter, theme::TEXT_SM));
                m.length_field(ui, size, 0.1, 10_000.0, "");
                place_choice(ui, axis, place);
                match place {
                    Place::Center {} => {
                        ui.label("");
                    }
                    Place::Min { offset: v } | Place::Max { offset: v } => {
                        m.signed_length_field(ui, v, 0.1, 10_000.0, "");
                    }
                    Place::At { coordinate: v } => {
                        m.signed_length_field(ui, v, 0.1, 10_000.0, "");
                    }
                }
                ui.end_row();
            }
        });
}

/// Where a fixed box sits on one axis. A new choice keeps the number already
/// there, so a person switching sides does not lose it.
fn place_choice(ui: &mut egui::Ui, axis: &Axis, p: &mut Place) {
    let at = format!("at {}", axis.letter);
    let text = |p: &Place| match p {
        Place::Center {} => "centred",
        Place::Min { .. } => axis.low,
        Place::Max { .. } => axis.high,
        Place::At { .. } => at.as_str(),
    };
    let keep = p.value().unwrap_or(0.0);
    egui::ComboBox::from_id_salt(("stock-place", axis.letter))
        .width(PLACE_WIDTH)
        .selected_text(text(p))
        .show_ui(ui, |ui| {
            for (choice, why) in [
                (Place::Center {}, "Centred on the models"),
                (Place::Min { offset: keep }, axis.low_hint),
                (Place::Max { offset: keep }, axis.high_hint),
                (
                    Place::At { coordinate: keep },
                    "Its low side at this model coordinate, wherever the models are",
                ),
            ] {
                let on = std::mem::discriminant(p) == std::mem::discriminant(&choice);
                if ui
                    .selectable_label(on, text(&choice))
                    .on_hover_text(why)
                    .clicked()
                    && !on
                {
                    *p = choice;
                }
            }
        });
}

/// A box around models' margins, a row each, by the axis it grows along.
fn margins(ui: &mut egui::Ui, m: Measure, margin: &mut [f64; 3], below: &mut f64) {
    egui::Grid::new("stock-margins")
        .num_columns(3)
        .spacing([theme::SPACE_3, 6.0])
        .show(ui, |ui| {
            ui.label("");
            ui.label(widgets::section_label("Margin"));
            ui.label("");
            ui.end_row();
            let [x, y, z] = margin;
            for (letter, side, v) in [
                ("X", "each side", x),
                ("Y", "each side", y),
                ("Z", "on top", z),
                ("", "underneath", below),
            ] {
                ui.label(widgets::mono(letter, theme::TEXT_SM));
                widgets::field_label(ui, side);
                m.length_field(ui, v, 0.1, 1000.0, "");
                ui.end_row();
            }
        });
}

/// Where a stock is for one setup using it: the block and the models it is
/// around or placed against, in model coordinates.
struct Placement {
    /// The setup, or empty when no setup uses it.
    setup: String,
    stock: Aabb,
    /// The models, when the block depends on them.
    part: Option<Aabb>,
}

/// Stock `name`, shaped `spec`, for each setup using it. Setups that give the
/// same block share one; a stock no setup uses is placed only when it names its
/// own models. One around a model not loaded is left out.
fn placements(doc: &Document, name: &str, spec: &StockSpec) -> Vec<Placement> {
    let bodies: camcraft_core::setup::Bodies<'_> = doc
        .models
        .iter()
        .filter_map(|(n, slot)| Some((n.as_str(), &slot.model()?.body)))
        .collect();
    let bounds = |models: &[String]| {
        models
            .iter()
            .map(|n| doc.models.get(n)?.model().map(|m| m.bounds))
            .collect::<Option<Vec<_>>>()?
            .into_iter()
            .reduce(|a, b| a.union(&b))
    };
    let own = match spec {
        StockSpec::AroundModels { models, .. } | StockSpec::FixedBox(FixedBox { models, .. }) => {
            models
        }
    };
    let place = |setup: &str, models: &[String]| {
        let stock = camcraft_core::setup::stock_block(spec, models, &bodies).ok()?;
        let part = Some(bounds(if own.is_empty() { models } else { own })?);
        Some(Placement {
            setup: setup.to_string(),
            stock,
            part,
        })
    };
    let mut out: Vec<Placement> = Vec::new();
    for s in doc
        .job
        .setups
        .iter()
        .filter(|s| s.stocks.iter().any(|x| x == name))
    {
        if let Some(p) = place(&s.name, &s.models)
            && !out.iter().any(|o| o.stock == p.stock)
        {
            out.push(p);
        }
    }
    if out.is_empty() && !own.is_empty() {
        out.extend(place("", own));
    }
    // A box at a corner is where it is whatever the models: any loaded one
    // places it.
    if out.is_empty()
        && let StockSpec::FixedBox(b) = spec
        && !b.placed()
        && let Some(any) = bodies.keys().next()
        && let Ok(stock) = camcraft_core::setup::stock_block(spec, &[(*any).to_string()], &bodies)
    {
        out.push(Placement {
            setup: String::new(),
            stock,
            part: None,
        });
    }
    out
}

/// The block's corners in model coordinates, a pair for each distinct
/// placement, named by setup when there is more than one.
fn corners(ui: &mut egui::Ui, placed: &[Placement], m: Measure) {
    if placed.is_empty() {
        hint(ui, "Its size shows once a setup uses it.");
        return;
    }
    let point = |p: DVec3| {
        format!(
            "{}, {}, {}",
            m.number(p.x, 2),
            m.number(p.y, 2),
            m.number(p.z, 2)
        )
    };
    let value = |text: String| widgets::mono(text, theme::TEXT_SM);
    form(ui, "stock-corners", |ui| {
        for p in placed {
            if placed.len() > 1 {
                widgets::field_label(ui, "In");
                ui.label(&p.setup);
                ui.end_row();
            }
            widgets::field_label(ui, "From");
            ui.label(value(point(p.stock.min)));
            ui.end_row();
            widgets::field_label(ui, "To");
            ui.label(value(point(p.stock.max)));
            ui.end_row();
        }
    });
}

/// The block and the models it is around, to scale: from the top on the left,
/// from the front on the right, on the grid the tool's drawing is on, with the
/// block's size under each. The block is amber, as the cutter is in the tool's:
/// it is what the panel edits.
fn paint_stock(ui: &mut egui::Ui, p: &Placement, m: Measure) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), PICTURE),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, theme::RADIUS_SM, theme::egui_color(theme::BG));
    widgets::grid_background(&painter, rect);
    painter.rect_stroke(
        rect,
        theme::RADIUS_SM,
        egui::Stroke::new(1.0, theme::egui_color(theme::BORDER)),
        egui::StrokeKind::Inside,
    );
    let caption = theme::caption(ui.style());
    let quiet = theme::egui_color(theme::TEXT_3);
    let s = p.stock;
    let all = p.part.map_or(s, |part| s.union(&part));
    let e = all.extent();

    // Two views side by side, one scale for both, a label over each and the
    // block's size under it.
    let pad = 16.0_f32;
    let (above, below) = (26.0_f32, 30.0_f32);
    let half = (rect.width() - 3.0 * pad) / 2.0;
    let tall = rect.height() - above - below;
    let scale = (f64::from(half) / e.x.max(1e-6))
        .min(f64::from(tall) / e.y.max(1e-6))
        .min(f64::from(tall) / e.z.max(1e-6));
    // Millimetres at most a few metres across, at a scale that fits a panel.
    #[allow(clippy::cast_possible_truncation)]
    let px = |v: f64| (v * scale) as f32;
    let view_at = |left: f32| {
        egui::Rect::from_min_size(
            egui::pos2(rect.left() + left, rect.top() + above),
            egui::vec2(half, tall),
        )
    };
    let (top_view, front_view) = (view_at(pad), view_at(2.0 * pad + half));

    let block = egui::Stroke::new(1.5, theme::egui_color(theme::AMBER_5));
    let block_fill = theme::egui_color(theme::AMBER_5).gamma_multiply(0.22);
    let model = egui::Stroke::new(1.0, theme::egui_color(theme::GRAY_7));
    let model_fill = theme::egui_color(theme::PART);
    for (view, [u, v], label) in [
        (top_view, [0_usize, 1], "top"),
        (front_view, [0, 2], "front"),
    ] {
        // Centred in the view, `v` up.
        let origin = view.center() + egui::vec2(-px(e[u]), px(e[v])) / 2.0;
        let r = |b: &Aabb| {
            egui::Rect::from_two_pos(
                origin + egui::vec2(px(b.min[u] - all.min[u]), -px(b.min[v] - all.min[v])),
                origin + egui::vec2(px(b.max[u] - all.min[u]), -px(b.max[v] - all.min[v])),
            )
        };
        if let Some(part) = &p.part {
            painter.rect_filled(r(part), 0.0, model_fill);
        }
        painter.rect_filled(r(&s), 0.0, block_fill);
        if let Some(part) = &p.part {
            painter.rect_stroke(r(part), 0.0, model, egui::StrokeKind::Inside);
        }
        painter.rect_stroke(r(&s), 0.0, block, egui::StrokeKind::Inside);
        painter.text(
            egui::pos2(view.left(), rect.top() + 8.0),
            egui::Align2::LEFT_TOP,
            label,
            caption.clone(),
            quiet,
        );
    }
    let size = s.extent();
    let under = |view: egui::Rect| egui::pos2(view.center().x, rect.bottom() - 8.0);
    painter.text(
        under(top_view),
        egui::Align2::CENTER_BOTTOM,
        format!("{} × {}", m.number(size.x, 2), m.length(size.y, 2)),
        caption.clone(),
        ui.visuals().text_color(),
    );
    painter.text(
        under(front_view),
        egui::Align2::CENTER_BOTTOM,
        format!("{} tall", m.length(size.z, 2)),
        caption,
        ui.visuals().text_color(),
    );
}
