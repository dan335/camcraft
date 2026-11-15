//! A stock's panel, laid out as the tool library lays out a tool: a drawing of
//! the block around the part, its size and corners in figures, then its fields.
//! For a fixed box, those are one row per axis with the size, the place and the
//! offset side by side. For a box around models, they are one row per margin.
//! An arranged sheet has a panel of its own, in `sheet`.

use camcraft_core::job::{Arranged, FixedBox, Place, SheetPart, StockSpec};
use camcraft_geom::mesh::Aabb;
use eframe::egui;
use glam::DVec3;

use super::parts::{Names, all_model_bounds, names, used_by};
use super::sheet::{self, on_other_sheets};
use super::{ViewPicks, header, renamed};
use crate::document::{Document, Selection};
use crate::help::Help;
use crate::help::topics::stock as topics;
use crate::ui::defaults::Defaults;
use crate::ui::measure::Measure;
use crate::ui::theme;
use crate::ui::widgets::{self, Size, Variant, form, hint};

/// How tall the drawing is, as the tool's.
const PICTURE: f32 = 180.0;
/// How wide a place chooser is, so the rows' offsets line up.
const PLACE_WIDTH: f32 = 116.0;

/// Stock `name`'s panel. The model whose every copy it lights, for the view
/// to light too: a sheet's part under the pointer, or chosen.
pub(super) fn stock(
    ui: &mut egui::Ui,
    doc: &mut Document,
    m: Measure,
    name: &str,
) -> Option<String> {
    let Some(i) = doc.job.stocks.iter().position(|x| x.name == name) else {
        doc.selection = Selection::Stocks;
        return None;
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
        return None;
    }
    let model_names: Vec<String> = doc.job.models.iter().map(|x| x.name.clone()).collect();
    // A sheet as it is laid out before this frame's edits: drawn, then edited.
    let shown = doc.job.stocks.get(i).and_then(|s| match &s.spec {
        StockSpec::Arranged(a) => sheet::Shown::of(doc, name, a),
        StockSpec::AroundModels { .. } | StockSpec::FixedBox(_) => None,
    });
    let elsewhere = on_other_sheets(&doc.job, name);
    let fit = all_model_bounds(doc);
    // What a sheet made of it starts with: the models of the setups using it
    // that no other sheet holds.
    let seatable: Vec<String> = doc
        .job
        .models
        .iter()
        .map(|m| m.name.clone())
        .filter(|m| {
            let used = doc
                .job
                .setups
                .iter()
                .any(|s| s.stocks.iter().any(|x| x == name) && s.models.contains(m));
            used && doc.job.sheet_of(m).is_none_or(|(s, _)| s.name == name)
        })
        .collect();
    // The models put on the sheet by this frame's edits, whose positions it
    // takes over.
    let before: Vec<String> = doc
        .job
        .stocks
        .get(i)
        .map(|s| owned(&s.spec))
        .unwrap_or_default();
    let spec = doc.job.stocks.get_mut(i).map(|s| &mut s.spec)?;
    let mut shape = Defaults::of(&*spec, &StockSpec::default());
    form(ui, "stock", |ui| {
        shape.row(ui, &topics::SHAPE, "kind", |ui| {
            kind_choice(ui, spec, fit, &seatable);
        });
    });
    shape.apply(spec);
    ui.add_space(theme::SPACE_4);

    // The block as it stands before this frame's edits: drawn, then edited. A
    // sheet draws itself.
    if !matches!(spec, StockSpec::Arranged(_)) {
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
    }

    let doc_id = doc.id;
    let spec = doc.job.stocks.get_mut(i).map(|s| &mut s.spec)?;
    let def = match &*spec {
        StockSpec::AroundModels { models, .. } => match StockSpec::default() {
            StockSpec::AroundModels { margin, below, .. } => StockSpec::AroundModels {
                models: models.clone(),
                margin,
                below,
            },
            other => other,
        },
        StockSpec::FixedBox(b) => StockSpec::FixedBox(FixedBox {
            models: b.models.clone(),
            ..FixedBox::centred(fit.map_or([100.0, 100.0, 25.0], |f| f.extent().to_array()))
        }),
        // Its size and which parts it holds are its own, which nothing
        // defaults; each part is one copy, turned as the sheet allows.
        StockSpec::Arranged(a) => StockSpec::Arranged(Arranged {
            parts: a.parts.iter().map(|p| SheetPart::of(&p.model)).collect(),
            ..Arranged::new(a.size)
        }),
    };
    let mut d = Defaults::of(&*spec, &def);
    let mut lit = None;
    match spec {
        StockSpec::Arranged(a) => {
            let cx = sheet::Context {
                doc: doc_id,
                stock: name,
                id: id.with("parts"),
                models: &model_names,
                elsewhere: elsewhere.as_ref(),
                shown,
            };
            lit = sheet::panel(ui, &mut d, m, a, &cx);
        }
        StockSpec::AroundModels {
            models,
            margin,
            below,
        } => {
            margins(ui, &mut d, m, margin, below);
            ui.add_space(theme::SPACE_4);
            names(
                ui,
                &Names {
                    id: id.with("around"),
                    title: "Around",
                    topic: None,
                    all: &model_names,
                    empty: "The models of the setup using it.",
                    pick: None,
                    more: None,
                    withheld: None,
                },
                models,
                ViewPicks::default(),
                &mut Vec::new(),
            );
        }
        StockSpec::FixedBox(b) => {
            axes(ui, &mut d, m, b);
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
                    topic: None,
                    all: &model_names,
                    empty: "The models of the setup using it.",
                    pick: None,
                    more: None,
                    withheld: None,
                },
                &mut b.models,
                ViewPicks::default(),
                &mut Vec::new(),
            );
        }
    }
    d.apply(spec);
    let added: Vec<String> = owned(spec)
        .into_iter()
        .filter(|m| !before.contains(m))
        .collect();
    // Its sheet places a model put on it, with the same edit.
    doc.placed_by_sheets(&added);
    ui.add_space(theme::SPACE_3);
    used_by(
        ui,
        doc,
        |s| s.stocks.iter().any(|x| x == name),
        "No setup uses it.",
        "Used by",
    );
    lit
}

/// The size a new sheet starts at: a whole sheet of board as it is sold,
/// 2440 by 1220, 18 thick.
const NEW_SHEET: [f64; 3] = [2440.0, 1220.0, 18.0];

/// The models `spec` arranges, if it is a sheet.
fn owned(spec: &StockSpec) -> Vec<String> {
    match spec {
        StockSpec::Arranged(a) => a.parts.iter().map(|p| p.model.clone()).collect(),
        StockSpec::AroundModels { .. } | StockSpec::FixedBox(_) => Vec::new(),
    }
}

/// The choice of a stock's kind. A new choice starts as a new one of its kind
/// starts: a fixed box sized to the models `fit`, a sheet holding
/// `seatable`, the models of the setups using it that no other sheet holds.
fn kind_choice(ui: &mut egui::Ui, spec: &mut StockSpec, fit: Option<Aabb>, seatable: &[String]) {
    let text = |s: &StockSpec| match s {
        StockSpec::AroundModels { .. } => "around models",
        StockSpec::FixedBox(_) => "fixed box",
        StockSpec::Arranged(_) => "arranged sheet",
    };
    egui::ComboBox::from_id_salt("stock-kind")
        .selected_text(text(spec))
        .show_ui(ui, |ui| {
            let size = fit.map_or([100.0, 100.0, 25.0], |b| b.extent().to_array());
            let choices = [
                (
                    StockSpec::default(),
                    "A box round the models, grown by margins",
                ),
                (
                    StockSpec::FixedBox(FixedBox::centred(size)),
                    "A box of a size you give, placed against the models",
                ),
                (
                    StockSpec::Arranged(Arranged {
                        parts: seatable.iter().map(|m| SheetPart::of(m)).collect(),
                        ..Arranged::new(NEW_SHEET)
                    }),
                    "A sheet that lays its parts out on itself",
                ),
            ];
            for (choice, why) in choices {
                let on = std::mem::discriminant(spec) == std::mem::discriminant(&choice);
                if ui
                    .selectable_label(on, text(&choice))
                    .on_hover_text(why)
                    .clicked()
                    && !on
                {
                    *spec = choice;
                }
            }
        });
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
fn axes(ui: &mut egui::Ui, d: &mut Defaults<StockSpec>, m: Measure, b: &mut FixedBox) {
    egui::Grid::new("stock-axes")
        .num_columns(4)
        .spacing([theme::SPACE_3, 6.0])
        .show(ui, |ui| {
            ui.label("");
            for topic in [&topics::SIZE, &topics::POSITION, &topics::OFFSET] {
                ui.label(widgets::section_label(topic.title)).help(topic);
            }
            ui.end_row();
            for (k, ((axis, size), place)) in
                AXES.iter().zip(&mut b.size).zip(&mut b.place).enumerate()
            {
                let path = format!("size.{k},{}", axis.letter.to_lowercase());
                ui.label(d.tint(widgets::mono(axis.letter, theme::TEXT_SM), &path));
                m.length_field(ui, size, 0.1, 10_000.0, "")
                    .help(&topics::SIZE);
                place_choice(ui, axis, place);
                ui.horizontal(|ui| {
                    match place {
                        Place::Center {} => {}
                        Place::Min { offset: v } | Place::Max { offset: v } => {
                            m.signed_length_field(ui, v, 0.1, 10_000.0, "")
                                .help(&topics::OFFSET);
                        }
                        Place::At { coordinate: v } => {
                            m.signed_length_field(ui, v, 0.1, 10_000.0, "")
                                .help(&topics::OFFSET);
                        }
                    }
                    d.reset(ui, &path);
                });
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
        })
        .response
        .help(&topics::POSITION);
}

/// A box around models' margins, a row each, by the axis it grows along.
fn margins(
    ui: &mut egui::Ui,
    d: &mut Defaults<StockSpec>,
    m: Measure,
    margin: &mut [f64; 3],
    below: &mut f64,
) {
    egui::Grid::new("stock-margins")
        .num_columns(3)
        .spacing([theme::SPACE_3, 6.0])
        .show(ui, |ui| {
            ui.label("");
            ui.label(widgets::section_label("Margin"));
            ui.label("");
            ui.end_row();
            let [x, y, z] = margin;
            for (letter, side, v, topic, path) in [
                ("X", "each side", x, &topics::MARGIN_X, "margin.0"),
                ("Y", "each side", y, &topics::MARGIN_Y, "margin.1"),
                ("Z", "on top", z, &topics::MARGIN_TOP, "margin.2"),
                ("", "underneath", below, &topics::MARGIN_UNDER, "below"),
            ] {
                ui.label(widgets::mono(letter, theme::TEXT_SM));
                let label = egui::RichText::new(side).color(theme::egui_color(theme::text_3()));
                ui.label(d.tint(label, path)).help(topic);
                ui.horizontal(|ui| {
                    m.length_field(ui, v, 0.1, 1000.0, "").help(topic);
                    d.reset(ui, path);
                });
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

/// Stock `name`, a box shaped `spec`, for each setup using it. Setups that
/// give the same block share one; a stock no setup uses is placed only when
/// it names its own models. One around a model not loaded is left out.
fn placements(doc: &Document, name: &str, spec: &StockSpec) -> Vec<Placement> {
    let bodies = doc.bodies();
    // Around every copy of the models, as the block is.
    let bounds = |models: &[String]| camcraft_core::setup::bounds_of(models, &bodies).ok();
    let own: Vec<String> = spec.models().into_iter().map(String::from).collect();
    let place = |setup: &str, models: &[String]| {
        let stock = camcraft_core::setup::stock_block(spec, models, &bodies).ok()?;
        let part = Some(bounds(if own.is_empty() { models } else { &own })?);
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
        out.extend(place("", &own));
    }
    // A box at a corner is where it is whatever the models: any loaded one
    // places it.
    if out.is_empty()
        && let StockSpec::FixedBox(b) = spec
        && !b.placed()
        && let Some(any) = bodies.names().next()
        && let Ok(stock) = camcraft_core::setup::stock_block(spec, &[any.to_string()], &bodies)
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
                widgets::key_label(ui, "In");
                ui.label(&p.setup);
                ui.end_row();
            }
            widgets::key_label(ui, "From");
            ui.label(value(point(p.stock.min)));
            ui.end_row();
            widgets::key_label(ui, "To");
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
    painter.rect_filled(rect, theme::RADIUS_SM, theme::egui_color(theme::bg()));
    widgets::grid_background(&painter, rect);
    painter.rect_stroke(
        rect,
        theme::RADIUS_SM,
        egui::Stroke::new(1.0, theme::egui_color(theme::border())),
        egui::StrokeKind::Inside,
    );
    let caption = theme::caption(ui.style());
    let quiet = theme::egui_color(theme::text_3());
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

    let block = egui::Stroke::new(1.5, theme::egui_color(theme::accent()));
    let block_fill = theme::egui_color(theme::accent()).gamma_multiply(0.22);
    let model = egui::Stroke::new(1.0, theme::egui_color(theme::text_3()));
    let model_fill = theme::egui_color(theme::text_4());
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
