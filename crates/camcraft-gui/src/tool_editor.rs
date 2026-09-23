//! The fields of one tool, drawn and edited: shared by the tool manager, which
//! edits the library's tools, and an operation's panel, which edits the job's copy.

use camcraft_core::cutting;
use camcraft_core::job::{
    Coolant, Holder, HolderSegment, Preset, SpindleDirection, StockToLeave, ToolFieldError,
    ToolKind, ToolMaterial, ToolSpec, free_preset_name,
};
use eframe::egui;

use crate::measure::Measure;
use crate::theme;
use crate::tool_drawing::{Part, draw};

/// The tool's picture and every field, edited in place. Lengths and feeds are
/// shown in `m` and kept in millimetres. Call inside `ui.push_id` when more than
/// one tool may be edited in the same window.
pub fn tool_fields(ui: &mut egui::Ui, tool: &mut ToolSpec, m: Measure) {
    let (errors, preset_errors): (Vec<_>, Vec<_>) =
        tool.check().into_iter().partition(|e| e.preset.is_none());
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
                for k in [
                    ToolKind::Flat,
                    ToolKind::Ball,
                    ToolKind::Bullnose,
                    ToolKind::Cone,
                ] {
                    ui.selectable_value(&mut tool.kind, k, kind_name(k));
                }
            });
        ui.end_row();
        if tool.kind == ToolKind::Cone && tool.angle.is_none() {
            tool.angle = Some(90.0);
        }
        length(ui, m, "Diameter", &mut tool.diameter, &errors, "diameter");
        if tool.kind == ToolKind::Bullnose {
            length(ui, m, "Corner radius", &mut tool.corner, &errors, "corner");
        }
        if tool.kind == ToolKind::Cone {
            ui.label("Angle");
            ui.horizontal(|ui| {
                if let Some(a) = tool.angle.as_mut() {
                    ui.add(
                        egui::DragValue::new(a)
                            .speed(0.5)
                            .range(1.0..=179.0)
                            .suffix("° included"),
                    );
                }
                field_error(ui, &errors, "angle");
            });
            ui.end_row();
            length(
                ui,
                m,
                "Tip diameter",
                &mut tool.tip_diameter,
                &errors,
                "tip_diameter",
            );
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

    section(ui, "Speed", &mut |ui| {
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
        ui.label("");
        ui.weak("An operation cuts at this speed unless its preset gives one.");
        ui.end_row();
        let mut rpm = Some(tool.rpm);
        surface_speed_row(ui, &mut rpm, tool.diameter, m);
        if let Some(r) = rpm {
            tool.rpm = r;
        }
    });

    egui::CollapsingHeader::new("Presets")
        .default_open(true)
        .show(ui, |ui| presets(ui, tool, m, &preset_errors));

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
        ui.label("Spindle turns");
        egui::ComboBox::from_id_salt("spindle")
            .selected_text(match tool.spindle {
                SpindleDirection::Clockwise => "clockwise (M3)",
                SpindleDirection::CounterClockwise => "counter-clockwise (M4)",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut tool.spindle,
                    SpindleDirection::Clockwise,
                    "clockwise (M3)",
                );
                ui.selectable_value(
                    &mut tool.spindle,
                    SpindleDirection::CounterClockwise,
                    "counter-clockwise (M4), for a left-hand cutter",
                );
            });
        ui.end_row();
        ui.label("Coolant");
        egui::ComboBox::from_id_salt("coolant")
            .selected_text(match tool.coolant {
                Coolant::Off => "off",
                Coolant::Flood => "flood (M8)",
                Coolant::Mist => "mist (M7)",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut tool.coolant, Coolant::Off, "off");
                ui.selectable_value(&mut tool.coolant, Coolant::Flood, "flood (M8)");
                ui.selectable_value(&mut tool.coolant, Coolant::Mist, "mist (M7)");
            });
        ui.end_row();
    });
}

/// The tool's presets, each a block of its own with its values, and a way to add,
/// copy, reorder and remove them. The first is the one a new operation gets.
fn presets(ui: &mut egui::Ui, tool: &mut ToolSpec, m: Measure, errors: &[ToolFieldError]) {
    if tool.presets.is_empty() {
        ui.weak(
            "No presets. A preset is a named set of speeds and feeds, \
             e.g. Roughing and Finishing, that an operation copies.",
        );
    }
    enum Act {
        Up(usize),
        Down(usize),
        Copy(usize),
        Remove(usize),
    }
    let mut act = None;
    let count = tool.presets.len();
    let (diameter, flutes) = (tool.diameter, tool.flutes);
    for (i, p) in tool.presets.iter_mut().enumerate() {
        let mine: Vec<ToolFieldError> = errors
            .iter()
            .filter(|e| e.preset == Some(i))
            .cloned()
            .collect();
        let title = if p.name.trim().is_empty() {
            "(unnamed)".to_string()
        } else if i == 0 {
            format!("{} (default)", p.name)
        } else {
            p.name.clone()
        };
        let title = if mine.is_empty() {
            egui::RichText::new(title)
        } else {
            egui::RichText::new(title).color(ui.visuals().error_fg_color)
        };
        ui.push_id(("preset", i), |ui| {
            egui::CollapsingHeader::new(title)
                .id_salt(("preset", i))
                .default_open(count <= 2)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(i > 0, egui::Button::new("up").small())
                            .clicked()
                        {
                            act = Some(Act::Up(i));
                        }
                        if ui
                            .add_enabled(i + 1 < count, egui::Button::new("down").small())
                            .clicked()
                        {
                            act = Some(Act::Down(i));
                        }
                        if ui.small_button("duplicate").clicked() {
                            act = Some(Act::Copy(i));
                        }
                        if ui.small_button("remove").clicked() {
                            act = Some(Act::Remove(i));
                        }
                    });
                    egui::Grid::new("preset-fields")
                        .num_columns(2)
                        .spacing([12.0, 6.0])
                        .show(ui, |ui| preset_fields(ui, p, (diameter, flutes), m, &mine));
                });
        });
    }
    match act {
        Some(Act::Up(i)) => tool.presets.swap(i - 1, i),
        Some(Act::Down(i)) => tool.presets.swap(i, i + 1),
        Some(Act::Copy(i)) => {
            if let Some(p) = tool.presets.get(i).cloned() {
                let name = free_preset_name(&tool.presets, &p.name);
                tool.presets.insert(i + 1, Preset { name, ..p });
            }
        }
        Some(Act::Remove(i)) => {
            tool.presets.remove(i);
        }
        None => {}
    }
    if ui.button("Add preset").clicked() {
        let name = free_preset_name(&tool.presets, "Preset");
        tool.presets.push(Preset {
            name,
            rpm: Some(tool.rpm),
            ..Preset::default()
        });
    }
}

fn preset_fields(
    ui: &mut egui::Ui,
    p: &mut Preset,
    (diameter, flutes): (f64, Option<u32>),
    m: Measure,
    errors: &[ToolFieldError],
) {
    ui.label("Name");
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut p.name);
        field_error(ui, errors, "presets.name");
    });
    ui.end_row();
    ui.label("Note");
    ui.add(egui::TextEdit::singleline(&mut p.comment).hint_text("e.g. from the maker's chart"));
    ui.end_row();
    ui.label("Spindle");
    ui.horizontal(|ui| {
        let mut on = p.rpm.is_some();
        if ui.checkbox(&mut on, "").changed() {
            p.rpm = on.then_some(18000);
        }
        if let Some(rpm) = p.rpm.as_mut() {
            ui.add(
                egui::DragValue::new(rpm)
                    .range(0..=60000)
                    .suffix(" rev/min"),
            );
        } else {
            ui.weak("not given");
        }
        field_error(ui, errors, "presets.rpm");
    });
    ui.end_row();
    surface_speed_row(ui, &mut p.rpm, diameter, m);
    opt_feed(ui, m, "Cutting feed", &mut p.feed, errors, "presets.feed");
    chip_load_row(ui, p, flutes, m);
    opt_feed(
        ui,
        m,
        "Plunge feed",
        &mut p.plunge_feed,
        errors,
        "presets.plunge_feed",
    );
    opt_feed(
        ui,
        m,
        "Ramp feed",
        &mut p.ramp_feed,
        errors,
        "presets.ramp_feed",
    );
    opt_length(
        ui,
        m,
        "Stepdown",
        &mut p.stepdown,
        0.5 * diameter,
        errors,
        "presets.stepdown",
    );
    opt_length(
        ui,
        m,
        "Stepover",
        &mut p.stepover,
        0.4 * diameter,
        errors,
        "presets.stepover",
    );
    ui.label("Stock to leave");
    ui.horizontal(|ui| {
        let mut on = p.stock_to_leave.is_some();
        if ui.checkbox(&mut on, "").changed() {
            p.stock_to_leave = on.then(StockToLeave::default);
        }
        if let Some(s) = p.stock_to_leave.as_mut() {
            m.length_field(ui, &mut s.radial, 0.01, 100.0, "radial ");
            m.length_field(ui, &mut s.axial, 0.01, 100.0, "axial ");
        } else {
            ui.weak("not given");
        }
        field_error(ui, errors, "presets.stock_to_leave");
    });
    ui.end_row();
}

/// The surface speed at `rpm`, which sets `rpm` when edited. Makers chart
/// cutters in this, and it holds across diameters.
fn surface_speed_row(ui: &mut egui::Ui, rpm: &mut Option<u32>, diameter: f64, m: Measure) {
    ui.label("Surface speed");
    ui.horizontal(|ui| match *rpm {
        Some(r) if diameter > 0.0 => {
            let vc = cutting::surface_speed(diameter, f64::from(r));
            let mut shown = m.show_surface(vc);
            let response = ui.add(
                egui::DragValue::new(&mut shown)
                    .speed(1.0)
                    .range(0.0..=m.show_surface(cutting::surface_speed(diameter, 60000.0)))
                    .max_decimals(1)
                    .suffix(format!(" {}", m.surface_unit())),
            );
            if response.changed() {
                *rpm = Some(whole_rpm(cutting::rpm_for(diameter, m.read_surface(shown))));
            }
            ui.weak("sets the spindle speed");
        }
        _ => {
            ui.weak("needs a spindle speed");
        }
    });
    ui.end_row();
}

/// The chip load at the preset's feed and speed, which sets the feed when
/// edited.
fn chip_load_row(ui: &mut egui::Ui, p: &mut Preset, flutes: Option<u32>, m: Measure) {
    ui.label("Chip load");
    ui.horizontal(|ui| match (p.feed, p.rpm, flutes) {
        (Some(feed), Some(rpm), Some(z)) if rpm > 0 && z > 0 => {
            let fz = cutting::chip_load(feed, f64::from(rpm), z);
            let mut shown = m.show(fz);
            let response = ui.add(
                egui::DragValue::new(&mut shown)
                    .speed(m.show(0.001))
                    .range(0.0..=m.show(2.0))
                    .min_decimals(m.decimals(3))
                    .max_decimals(m.decimals(4))
                    .suffix(format!(" {}/tooth", m.unit())),
            );
            if response.changed() {
                p.feed = Some(cutting::feed_for(m.read(shown), f64::from(rpm), z));
            }
            ui.weak("sets the cutting feed");
        }
        (_, _, None) | (_, _, Some(0)) => {
            ui.weak("needs the tool's flutes");
        }
        _ => {
            ui.weak("needs a spindle speed and cutting feed");
        }
    });
    ui.end_row();
}

/// A spindle speed worked out from a surface speed, to the nearest rev/min and
/// within what a field takes.
fn whole_rpm(rpm: f64) -> u32 {
    // Rounded and clamped to 1..=60000 first, so it fits.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let whole = rpm.round().clamp(1.0, 60000.0) as u32;
    whole
}

/// A tool type's name, as the user reads it.
pub fn kind_name(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Flat => "flat",
        ToolKind::Ball => "ball",
        ToolKind::Bullnose => "bullnose",
        ToolKind::Cone => "V-bit / chamfer",
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
