//! The fields of one tool, drawn and edited: shared by the tool manager, which
//! edits the library's tools, and an operation's panel, which edits the job's copy.

use camcraft_core::job::{
    CuttingData, Holder, HolderSegment, ToolFieldError, ToolKind, ToolMaterial, ToolSpec,
};
use eframe::egui;

use crate::measure::Measure;
use crate::theme;
use crate::tool_drawing::{Part, draw};

/// The tool's picture and every field, edited in place. Lengths and feeds are
/// shown in `m` and kept in millimetres. Call inside `ui.push_id` when more than
/// one tool may be edited in the same window.
pub fn tool_fields(ui: &mut egui::Ui, tool: &mut ToolSpec, m: Measure) {
    let errors = tool.check();
    paint_tool(ui, tool, m);

    let section = |ui: &mut egui::Ui, title: &str, body: &mut dyn FnMut(&mut egui::Ui)| {
        egui::CollapsingHeader::new(title)
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new(title)
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| body(ui));
            });
    };

    section(ui, "General", &mut |ui| {
        ui.label("Description");
        ui.text_edit_singleline(&mut tool.comment);
        ui.end_row();
        opt_text(ui, "Vendor", &mut tool.vendor);
        opt_text(ui, "Product", &mut tool.product);
        ui.label("Material");
        egui::ComboBox::from_id_salt("material")
            .selected_text(match tool.material {
                None => "not given",
                Some(ToolMaterial::Carbide) => "carbide",
                Some(ToolMaterial::Hss) => "HSS",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut tool.material, None, "not given");
                ui.selectable_value(&mut tool.material, Some(ToolMaterial::Carbide), "carbide");
                ui.selectable_value(&mut tool.material, Some(ToolMaterial::Hss), "HSS");
            });
        ui.end_row();
    });

    section(ui, "Cutter", &mut |ui| {
        ui.label("Type");
        egui::ComboBox::from_id_salt("kind")
            .selected_text(kind_name(tool.kind))
            .show_ui(ui, |ui| {
                for k in [ToolKind::Flat, ToolKind::Ball, ToolKind::Bullnose] {
                    ui.selectable_value(&mut tool.kind, k, kind_name(k));
                }
            });
        ui.end_row();
        length(ui, m, "Diameter", &mut tool.diameter, &errors, "diameter");
        if tool.kind == ToolKind::Bullnose {
            length(ui, m, "Corner radius", &mut tool.corner, &errors, "corner");
        }
        ui.label("Flutes");
        ui.horizontal(|ui| {
            let mut on = tool.flutes.is_some();
            if ui.checkbox(&mut on, "").changed() {
                tool.flutes = on.then_some(2);
            }
            if let Some(f) = tool.flutes.as_mut() {
                ui.add(egui::DragValue::new(f).range(0..=16));
            } else {
                ui.weak("not given");
            }
            field_error(ui, &errors, "flutes");
        });
        ui.end_row();
        opt_length(
            ui,
            m,
            "Flute length",
            &mut tool.flute_length,
            2.0 * tool.diameter,
            &errors,
            "flute_length",
        );
        opt_length(
            ui,
            m,
            "Shoulder length",
            &mut tool.shoulder_length,
            2.5 * tool.diameter,
            &errors,
            "shoulder_length",
        );
    });

    section(ui, "Shank & holder", &mut |ui| {
        opt_length(
            ui,
            m,
            "Shank diameter",
            &mut tool.shank_diameter,
            tool.diameter,
            &errors,
            "shank_diameter",
        );
        opt_length(
            ui,
            m,
            "Stickout",
            &mut tool.stickout,
            4.0 * tool.diameter,
            &errors,
            "stickout",
        );
        opt_length(
            ui,
            m,
            "Overall length",
            &mut tool.overall_length,
            8.0 * tool.diameter,
            &errors,
            "overall_length",
        );
        ui.label("Holder");
        ui.vertical(|ui| {
            let mut on = tool.holder.is_some();
            if ui.checkbox(&mut on, "has a holder").changed() {
                tool.holder = on.then(|| Holder {
                    name: String::new(),
                    segments: vec![HolderSegment {
                        diameter: 32.0,
                        length: 20.0,
                    }],
                });
            }
            if let Some(h) = tool.holder.as_mut() {
                ui.add(egui::TextEdit::singleline(&mut h.name).hint_text("name, e.g. ER20"));
                let mut remove = None;
                for (i, seg) in h.segments.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.weak(format!("{}", i + 1));
                        m.length_field(ui, &mut seg.diameter, 0.1, 1000.0, "Ø ");
                        m.length_field(ui, &mut seg.length, 0.1, 1000.0, "L ");
                        if ui.small_button("remove").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    h.segments.remove(i);
                }
                if ui.small_button("add segment").clicked() {
                    h.segments.push(HolderSegment {
                        diameter: 32.0,
                        length: 10.0,
                    });
                }
                field_error(ui, &errors, "holder");
            }
        });
        ui.end_row();
    });

    section(ui, "Cutting data", &mut |ui| {
        ui.label("Spindle");
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut tool.rpm)
                    .range(0..=60000)
                    .suffix(" rev/min"),
            );
            field_error(ui, &errors, "rpm");
        });
        ui.end_row();
        let cutting = tool.cutting.get_or_insert_with(CuttingData::default);
        opt_feed(
            ui,
            m,
            "Cutting feed",
            &mut cutting.feed,
            &errors,
            "cutting.feed",
        );
        opt_feed(
            ui,
            m,
            "Plunge feed",
            &mut cutting.plunge_feed,
            &errors,
            "cutting.plunge_feed",
        );
        opt_feed(
            ui,
            m,
            "Ramp feed",
            &mut cutting.ramp_feed,
            &errors,
            "cutting.ramp_feed",
        );
        if *cutting == CuttingData::default() {
            tool.cutting = None;
        }
    });

    section(ui, "Post", &mut |ui| {
        ui.label("Tool number");
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut tool.number)
                    .range(1..=9999)
                    .prefix("T"),
            );
        });
        ui.end_row();
    });
}

/// A tool type's name, as the user reads it.
pub fn kind_name(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Flat => "flat",
        ToolKind::Ball => "ball",
        ToolKind::Bullnose => "bullnose",
    }
}

fn field_error(ui: &mut egui::Ui, errors: &[ToolFieldError], field: &str) {
    for e in errors.iter().filter(|e| e.field == field) {
        ui.colored_label(ui.visuals().error_fg_color, &e.why);
    }
}

fn length(
    ui: &mut egui::Ui,
    m: Measure,
    label: &str,
    value: &mut f64,
    errors: &[ToolFieldError],
    field: &str,
) {
    ui.label(label);
    ui.horizontal(|ui| {
        m.length_field(ui, value, 0.05, 1000.0, "");
        field_error(ui, errors, field);
    });
    ui.end_row();
}

fn opt_length(
    ui: &mut egui::Ui,
    m: Measure,
    label: &str,
    value: &mut Option<f64>,
    default: f64,
    errors: &[ToolFieldError],
    field: &str,
) {
    ui.label(label);
    ui.horizontal(|ui| {
        let mut on = value.is_some();
        if ui.checkbox(&mut on, "").changed() {
            *value = on.then_some(default);
        }
        if let Some(v) = value.as_mut() {
            m.length_field(ui, v, 0.1, 1000.0, "");
        } else {
            ui.weak("not given");
        }
        field_error(ui, errors, field);
    });
    ui.end_row();
}

fn opt_feed(
    ui: &mut egui::Ui,
    m: Measure,
    label: &str,
    value: &mut Option<f64>,
    errors: &[ToolFieldError],
    field: &str,
) {
    ui.label(label);
    ui.horizontal(|ui| {
        let mut on = value.is_some();
        if ui.checkbox(&mut on, "").changed() {
            *value = on.then_some(1000.0);
        }
        if let Some(v) = value.as_mut() {
            m.feed_field(ui, v, 50000.0);
        } else {
            ui.weak("not given");
        }
        field_error(ui, errors, field);
    });
    ui.end_row();
}

fn opt_text(ui: &mut egui::Ui, label: &str, value: &mut Option<String>) {
    ui.label(label);
    let mut text = value.clone().unwrap_or_default();
    if ui.text_edit_singleline(&mut text).changed() {
        *value = (!text.trim().is_empty()).then_some(text);
    }
    ui.end_row();
}

/// The tool, side on and to scale, with its holder face marked.
pub fn paint_tool(ui: &mut egui::Ui, tool: &ToolSpec, m: Measure) {
    let d = draw(tool);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 220.0),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, theme::egui_color(theme::BG));
    let margin = 16.0_f64;
    let w = f64::from(rect.width()) - 2.0 * margin;
    let h = f64::from(rect.height()) - 2.0 * margin;
    let scale = (w / (2.0 * d.max_radius.max(1e-6))).min(h / d.height.max(1e-6));
    let base = rect.center_bottom() - egui::vec2(0.0, 16.0);
    // Drawing coordinates are millimetres at most a few hundred across.
    #[allow(clippy::cast_possible_truncation)]
    let at = |x: f64, z: f64| base + egui::vec2((x * scale) as f32, -(z * scale) as f32);
    for o in &d.outlines {
        let (fill, stroke) = match o.part {
            Part::Flutes => (theme::AMBER_5, theme::AMBER_3),
            Part::Shank => (theme::GRAY_7, theme::GRAY_8),
            Part::Holder => (theme::GRAY_5, theme::GRAY_7),
        };
        // Right half up, left half down: one closed, convex outline per part.
        let mut points: Vec<egui::Pos2> = o.half.iter().map(|(x, z)| at(*x, *z)).collect();
        points.extend(o.half.iter().rev().map(|(x, z)| at(-x, *z)));
        painter.add(egui::Shape::convex_polygon(
            points,
            theme::egui_color(fill),
            egui::Stroke::new(1.0, theme::egui_color(stroke)),
        ));
    }
    // The holder face: where the stickout is measured to.
    let face = at(0.0, d.holder_face).y;
    painter.line_segment(
        [
            egui::pos2(rect.left() + 8.0, face),
            egui::pos2(rect.right() - 8.0, face),
        ],
        egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
    );
    // Just below the face line, at the right edge: above it the holder is at its
    // widest, and below it only the narrower cutter, centred. Anywhere nearer the
    // middle, dim text lands on the grey shank and disappears.
    painter.text(
        egui::pos2(rect.right() - 10.0, face + 4.0),
        egui::Align2::RIGHT_TOP,
        format!("stickout {}", m.length(d.holder_face, 1)),
        theme::caption(ui.style()),
        ui.visuals().weak_text_color(),
    );
    painter.text(
        egui::pos2(rect.right() - 10.0, rect.bottom() - 6.0),
        egui::Align2::RIGHT_BOTTOM,
        format!("Ø {}", m.length(tool.diameter, 2)),
        theme::caption(ui.style()),
        ui.visuals().text_color(),
    );
    if !d.assumed.is_empty() {
        ui.weak(format!("assumed, not given: {}", d.assumed.join(", ")));
    }
}
