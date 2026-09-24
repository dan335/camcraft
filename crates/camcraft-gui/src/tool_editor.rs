//! The fields of one tool, drawn and edited: shared by the tool manager, which
//! edits the library's tools, and an operation's panel, which edits the job's copy.
//!
//! The fields are on tabs, as Fusion's tool dialog has them, so each shows a
//! short form rather than one long scroll. A tab with a field in error carries a
//! red dot, so a problem out of sight is still seen.

use camcraft_core::cutting;
use camcraft_core::job::{
    Coolant, DRILL_POINT, Holder, HolderSegment, Preset, SpindleDirection, StockToLeave,
    ToolFieldError, ToolKind, ToolMaterial, ToolSpec, free_preset_name,
};
use eframe::egui;

use crate::measure::Measure;
use crate::theme;
use crate::tool_drawing::{Part, draw};
use crate::widgets::{self, Icon, Size, Tab, Variant};

/// The fields each tab edits, as the checks name them, to flag a tab with an
/// error on it.
const CUTTER_FIELDS: [&str; 7] = [
    "diameter",
    "corner",
    "angle",
    "tip_diameter",
    "flutes",
    "flute_length",
    "shoulder_length",
];
const HOLDER_FIELDS: [&str; 4] = ["shank_diameter", "stickout", "overall_length", "holder"];

/// The tabs, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    General,
    Cutter,
    Holder,
    Presets,
    Post,
}

const PAGES: [Page; 5] = [
    Page::General,
    Page::Cutter,
    Page::Holder,
    Page::Presets,
    Page::Post,
];

/// Every field of the tool, on tabs, edited in place. Lengths and feeds are shown
/// in `m` and kept in millimetres. The tab showing is remembered per `ui` id, so
/// call inside `ui.push_id` when more than one tool may be edited in the same
/// window. The tool's picture is [`paint_tool`], drawn by the caller.
pub fn tool_fields(ui: &mut egui::Ui, tool: &mut ToolSpec, m: Measure) {
    let (errors, preset_errors): (Vec<_>, Vec<_>) =
        tool.check().into_iter().partition(|e| e.preset.is_none());
    let flagged = |fields: &[&str]| errors.iter().any(|e| fields.contains(&e.field));

    let page_id = ui.id().with("tool-page");
    let mut page = ui
        .data(|d| d.get_temp::<usize>(page_id))
        .unwrap_or(1)
        .min(PAGES.len() - 1);
    let tabs = [
        Tab::new("General"),
        Tab::new("Cutter").flagged(flagged(&CUTTER_FIELDS)),
        Tab::new("Holder").flagged(flagged(&HOLDER_FIELDS)),
        Tab::new("Presets")
            .count(tool.presets.len())
            .flagged(flagged(&["rpm"]) || !preset_errors.is_empty()),
        Tab::new("Post"),
    ];
    widgets::tabs(ui, &mut page, &tabs);
    ui.data_mut(|d| d.insert_temp(page_id, page));
    ui.add_space(theme::SPACE_4);

    match PAGES.get(page).copied().unwrap_or(Page::Cutter) {
        Page::General => form(ui, "general", |ui| general(ui, tool)),
        Page::Cutter => form(ui, "cutter", |ui| cutter(ui, tool, m, &errors)),
        Page::Holder => holder(ui, tool, m, &errors),
        Page::Presets => speeds(ui, tool, m, &errors, &preset_errors),
        Page::Post => form(ui, "post", |ui| post(ui, tool)),
    }
}

/// A form: labels in a column on the left, their fields lined up on the right.
fn form(ui: &mut egui::Ui, id: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([theme::SPACE_4, 6.0])
        .min_col_width(96.0)
        .show(ui, body);
}

fn general(ui: &mut egui::Ui, tool: &mut ToolSpec) {
    widgets::field_label(ui, "Description");
    ui.add(egui::TextEdit::singleline(&mut tool.comment).desired_width(220.0));
    ui.end_row();
    opt_text(ui, "Vendor", &mut tool.vendor, "e.g. Amana");
    opt_text(ui, "Product", &mut tool.product, "the maker's part number");
    widgets::field_label(ui, "Material");
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
}

fn cutter(ui: &mut egui::Ui, tool: &mut ToolSpec, m: Measure, errors: &[ToolFieldError]) {
    widgets::field_label(ui, "Type");
    egui::ComboBox::from_id_salt("kind")
        .selected_text(kind_name(tool.kind))
        .show_ui(ui, |ui| {
            for k in [
                ToolKind::Flat,
                ToolKind::Ball,
                ToolKind::Bullnose,
                ToolKind::Cone,
                ToolKind::Drill,
            ] {
                ui.selectable_value(&mut tool.kind, k, kind_name(k));
            }
        });
    ui.end_row();
    if tool.kind == ToolKind::Cone && tool.angle.is_none() {
        tool.angle = Some(90.0);
    }
    length(ui, m, "Diameter", &mut tool.diameter, errors, "diameter");
    if tool.kind == ToolKind::Bullnose {
        length(ui, m, "Corner radius", &mut tool.corner, errors, "corner");
    }
    if tool.kind == ToolKind::Drill {
        row(ui, "Point angle", errors, "angle", |ui| {
            let mut a = tool.angle.unwrap_or(DRILL_POINT);
            if ui
                .add(
                    egui::DragValue::new(&mut a)
                        .speed(0.5)
                        .range(60.0..=179.0)
                        .suffix("°"),
                )
                .changed()
            {
                tool.angle = ((a - DRILL_POINT).abs() > 1e-9).then_some(a);
            }
        });
    }
    if tool.kind == ToolKind::Cone {
        row(ui, "Angle", errors, "angle", |ui| {
            if let Some(a) = tool.angle.as_mut() {
                ui.add(
                    egui::DragValue::new(a)
                        .speed(0.5)
                        .range(1.0..=179.0)
                        .suffix("° included"),
                );
            }
        });
        length(
            ui,
            m,
            "Tip diameter",
            &mut tool.tip_diameter,
            errors,
            "tip_diameter",
        );
    }
    row(ui, "Flutes", errors, "flutes", |ui| {
        optional(ui, &mut tool.flutes, 2, |ui, f| {
            ui.add(egui::DragValue::new(f).range(0..=16));
        });
    });
    opt_length(
        ui,
        m,
        "Flute length",
        &mut tool.flute_length,
        2.0 * tool.diameter,
        errors,
        "flute_length",
    );
    opt_length(
        ui,
        m,
        "Shoulder length",
        &mut tool.shoulder_length,
        2.5 * tool.diameter,
        errors,
        "shoulder_length",
    );
}

fn holder(ui: &mut egui::Ui, tool: &mut ToolSpec, m: Measure, errors: &[ToolFieldError]) {
    form(ui, "shank", |ui| {
        opt_length(
            ui,
            m,
            "Shank diameter",
            &mut tool.shank_diameter,
            tool.diameter,
            errors,
            "shank_diameter",
        );
        opt_length(
            ui,
            m,
            "Stickout",
            &mut tool.stickout,
            4.0 * tool.diameter,
            errors,
            "stickout",
        );
        opt_length(
            ui,
            m,
            "Overall length",
            &mut tool.overall_length,
            8.0 * tool.diameter,
            errors,
            "overall_length",
        );
        row(ui, "Holder", errors, "holder", |ui| {
            let default = Holder {
                name: String::new(),
                segments: vec![HolderSegment {
                    diameter: 32.0,
                    length: 20.0,
                }],
            };
            optional(ui, &mut tool.holder, default, |ui, h| {
                ui.add(
                    egui::TextEdit::singleline(&mut h.name)
                        .hint_text("name, e.g. ER20")
                        .desired_width(160.0),
                );
            });
        });
    });
    let Some(h) = tool.holder.as_mut() else {
        return;
    };
    // The holder's segments, from the collet up: a small table of their own.
    ui.add_space(theme::SPACE_5);
    ui.horizontal(|ui| {
        ui.label(widgets::section_label("Holder segments"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::button(
                ui,
                Variant::Ghost,
                Size::Sm,
                Some(Icon::Plus),
                "Add segment",
            )
            .clicked()
            {
                h.segments.push(HolderSegment {
                    diameter: 32.0,
                    length: 10.0,
                });
            }
        });
    });
    let mut remove = None;
    egui::Grid::new("holder-segments")
        .num_columns(4)
        .spacing([theme::SPACE_3, 6.0])
        .show(ui, |ui| {
            for (i, seg) in h.segments.iter_mut().enumerate() {
                ui.label(
                    widgets::mono(format!("{}", i + 1), theme::TEXT_SM)
                        .color(theme::egui_color(theme::TEXT_4)),
                );
                m.length_field(ui, &mut seg.diameter, 0.1, 1000.0, "Ø ");
                m.length_field(ui, &mut seg.length, 0.1, 1000.0, "L ");
                if widgets::icon_button(ui, Icon::X, "remove this segment").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        h.segments.remove(i);
    }
}

/// The spindle speed the tool cuts at, then its presets: a list to pick one
/// from, and the one picked below it.
fn speeds(
    ui: &mut egui::Ui,
    tool: &mut ToolSpec,
    m: Measure,
    errors: &[ToolFieldError],
    preset_errors: &[ToolFieldError],
) {
    form(ui, "spindle", |ui| {
        row(ui, "Spindle", errors, "rpm", |ui| {
            ui.add(
                egui::DragValue::new(&mut tool.rpm)
                    .range(0..=60000)
                    .suffix(" rev/min"),
            )
            .on_hover_text("An operation cuts at this speed unless its preset gives one.");
        });
        let mut rpm = Some(tool.rpm);
        surface_speed_row(ui, &mut rpm, tool.diameter, m);
        if let Some(r) = rpm {
            tool.rpm = r;
        }
    });
    ui.add_space(theme::SPACE_5);
    presets(ui, tool, m, preset_errors);
}

fn post(ui: &mut egui::Ui, tool: &mut ToolSpec) {
    widgets::field_label(ui, "Tool number");
    ui.add(
        egui::DragValue::new(&mut tool.number)
            .range(1..=9999)
            .prefix("T"),
    );
    ui.end_row();
    widgets::field_label(ui, "Spindle turns");
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
    widgets::field_label(ui, "Coolant");
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
}

/// The tool's presets: a list with a way to add, copy, reorder and remove them,
/// and the one picked shown in full below. The first is the one a new operation
/// gets.
fn presets(ui: &mut egui::Ui, tool: &mut ToolSpec, m: Measure, errors: &[ToolFieldError]) {
    enum Act {
        Add,
        Up(usize),
        Down(usize),
        Copy(usize),
        Remove(usize),
    }
    let mut act = None;
    ui.horizontal(|ui| {
        ui.label(widgets::section_label("Presets"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::button(ui, Variant::Ghost, Size::Sm, Some(Icon::Plus), "Add preset")
                .clicked()
            {
                act = Some(Act::Add);
            }
        });
    });
    if tool.presets.is_empty() {
        ui.label(
            egui::RichText::new(
                "None yet. A preset is a named set of speeds and feeds, \
                 e.g. Roughing and Finishing, that an operation copies.",
            )
            .color(theme::egui_color(theme::TEXT_3)),
        );
    }

    let picked_id = ui.id().with("preset-picked");
    let count = tool.presets.len();
    let mut picked = ui
        .data(|d| d.get_temp::<usize>(picked_id))
        .unwrap_or(0)
        .min(count.saturating_sub(1));
    for (i, p) in tool.presets.iter().enumerate() {
        let bad = errors.iter().any(|e| e.preset == Some(i));
        let on = i == picked;
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), theme::CONTROL_MD),
            egui::Sense::click(),
        );
        if response.clicked() {
            picked = i;
        }
        list_row_background(ui, rect, on, response.hovered());
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(egui::vec2(theme::SPACE_3, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let name = if p.name.trim().is_empty() {
            "(unnamed)"
        } else {
            p.name.as_str()
        };
        let color = if bad {
            theme::ERR
        } else if on {
            theme::TEXT_1
        } else {
            theme::TEXT_2
        };
        row.label(egui::RichText::new(name).color(theme::egui_color(color)));
        if i == 0 {
            widgets::tag(&mut row, "default")
                .on_hover_text("A new operation with this tool gets this preset.");
        }
        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if on {
                if widgets::icon_button(ui, Icon::Trash, "remove this preset").clicked() {
                    act = Some(Act::Remove(i));
                }
                if widgets::icon_button(ui, Icon::Copy, "duplicate this preset").clicked() {
                    act = Some(Act::Copy(i));
                }
                ui.add_enabled_ui(i + 1 < count, |ui| {
                    if widgets::icon_button(ui, Icon::ChevronDown, "move down").clicked() {
                        act = Some(Act::Down(i));
                    }
                });
                ui.add_enabled_ui(i > 0, |ui| {
                    if widgets::icon_button(ui, Icon::ChevronUp, "move up").clicked() {
                        act = Some(Act::Up(i));
                    }
                });
            } else {
                ui.label(
                    widgets::mono(preset_summary(p, m), theme::TEXT_SM)
                        .color(theme::egui_color(theme::TEXT_3)),
                );
            }
        });
    }

    let (diameter, flutes, rpm) = (tool.diameter, tool.flutes, tool.rpm);
    if let Some(p) = tool.presets.get_mut(picked) {
        ui.add_space(theme::SPACE_3);
        let mine: Vec<ToolFieldError> = errors
            .iter()
            .filter(|e| e.preset == Some(picked))
            .cloned()
            .collect();
        ui.push_id(("preset", picked), |ui| {
            widgets::card(
                ui,
                None,
                |_| {},
                |ui| {
                    form(ui, "preset-fields", |ui| {
                        preset_fields(ui, p, (diameter, flutes, rpm), m, &mine);
                    });
                },
            );
        });
    }

    match act {
        Some(Act::Add) => {
            let name = free_preset_name(&tool.presets, "Preset");
            tool.presets.push(Preset {
                name,
                rpm: Some(tool.rpm),
                ..Preset::default()
            });
            picked = tool.presets.len() - 1;
        }
        Some(Act::Up(i)) => {
            tool.presets.swap(i - 1, i);
            picked = i - 1;
        }
        Some(Act::Down(i)) => {
            tool.presets.swap(i, i + 1);
            picked = i + 1;
        }
        Some(Act::Copy(i)) => {
            if let Some(p) = tool.presets.get(i).cloned() {
                let name = free_preset_name(&tool.presets, &p.name);
                tool.presets.insert(i + 1, Preset { name, ..p });
                picked = i + 1;
            }
        }
        Some(Act::Remove(i)) => {
            tool.presets.remove(i);
            picked = i.saturating_sub(1);
        }
        None => {}
    }
    ui.data_mut(|d| d.insert_temp(picked_id, picked));
}

/// A preset in a line: its speed and feed, the two a person compares.
fn preset_summary(p: &Preset, m: Measure) -> String {
    let rpm = p.rpm.map(|r| format!("{r} rev/min"));
    let feed = p
        .feed
        .map(|f| format!("{} {}", m.feed_number(f), m.feed_unit()));
    let parts: Vec<String> = [rpm, feed].into_iter().flatten().collect();
    if parts.is_empty() {
        "no speeds".into()
    } else {
        parts.join(" · ")
    }
}

/// A row of a list: the selected one on `--accent-subtle` with an amber edge,
/// the one under the pointer on `--hover-overlay`.
pub fn list_row_background(ui: &egui::Ui, rect: egui::Rect, selected: bool, hovered: bool) {
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, theme::RADIUS_SM, theme::ACCENT_SUBTLE.color());
        painter.rect_filled(
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + 2.0, rect.max.y)),
            0.0,
            theme::egui_color(theme::ACCENT),
        );
    } else if hovered {
        painter.rect_filled(rect, theme::RADIUS_SM, theme::HOVER_OVERLAY.color());
    }
}

fn preset_fields(
    ui: &mut egui::Ui,
    p: &mut Preset,
    (diameter, flutes, tool_rpm): (f64, Option<u32>, u32),
    m: Measure,
    errors: &[ToolFieldError],
) {
    row(ui, "Name", errors, "presets.name", |ui| {
        ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(180.0));
    });
    widgets::field_label(ui, "Note");
    ui.add(
        egui::TextEdit::singleline(&mut p.comment)
            .hint_text("e.g. from the maker's chart")
            .desired_width(180.0),
    );
    ui.end_row();
    row(ui, "Spindle", errors, "presets.rpm", |ui| {
        optional(ui, &mut p.rpm, tool_rpm, |ui, rpm| {
            ui.add(
                egui::DragValue::new(rpm)
                    .range(0..=60000)
                    .suffix(" rev/min"),
            );
        });
    });
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
    row(
        ui,
        "Stock to leave",
        errors,
        "presets.stock_to_leave",
        |ui| {
            optional(
                ui,
                &mut p.stock_to_leave,
                StockToLeave::default(),
                |ui, s| {
                    m.length_field(ui, &mut s.radial, 0.01, 100.0, "radial ");
                    m.length_field(ui, &mut s.axial, 0.01, 100.0, "axial ");
                },
            );
        },
    );
}

/// The surface speed at `rpm`, which sets `rpm` when edited. Makers chart
/// cutters in this, and it holds across diameters.
fn surface_speed_row(ui: &mut egui::Ui, rpm: &mut Option<u32>, diameter: f64, m: Measure) {
    widgets::field_label(ui, "Surface speed").on_hover_text("Setting it sets the spindle speed.");
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
        }
        _ => {
            quiet(ui, "needs a spindle speed");
        }
    });
    ui.end_row();
}

/// The chip load at the preset's feed and speed, which sets the feed when
/// edited.
fn chip_load_row(ui: &mut egui::Ui, p: &mut Preset, flutes: Option<u32>, m: Measure) {
    widgets::field_label(ui, "Chip load").on_hover_text("Setting it sets the cutting feed.");
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
        }
        (_, _, None) | (_, _, Some(0)) => {
            quiet(ui, "needs the tool's flutes");
        }
        _ => {
            quiet(ui, "needs a spindle speed and cutting feed");
        }
    });
    ui.end_row();
}

/// Text that says why a field is not there: `--text-4`, the quietest.
fn quiet(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.label(egui::RichText::new(text).color(theme::egui_color(theme::TEXT_4)))
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
        ToolKind::Drill => "drill",
    }
}

/// One row of a form: the label, then what `add` puts in, bordered in red when
/// the checks find `field` wrong, with why on the line under it.
fn row(
    ui: &mut egui::Ui,
    label: &str,
    errors: &[ToolFieldError],
    field: &str,
    add: impl FnOnce(&mut egui::Ui),
) {
    widgets::field_label(ui, label);
    let why: Vec<&str> = errors
        .iter()
        .filter(|e| e.field == field)
        .map(|e| e.why.as_str())
        .collect();
    ui.horizontal(|ui| {
        if !why.is_empty() {
            // The system's `invalid` field: an `--err` border, nothing louder.
            let err = egui::Stroke::new(1.0, theme::egui_color(theme::ERR));
            let w = &mut ui.visuals_mut().widgets;
            w.inactive.bg_stroke = err;
            w.hovered.bg_stroke = err;
            w.active.bg_stroke = err;
        }
        add(ui);
    });
    ui.end_row();
    for why in why {
        ui.label("");
        ui.add(
            egui::Label::new(
                egui::RichText::new(why)
                    .size(theme::TEXT_SM)
                    .color(theme::egui_color(theme::ERR)),
            )
            .wrap(),
        );
        ui.end_row();
    }
}

/// A value that may be left out: a quiet "not given" to give it, or its field
/// and a button to take it out again. Replaces a checkbox beside every field.
fn optional<T>(
    ui: &mut egui::Ui,
    value: &mut Option<T>,
    default: T,
    add: impl FnOnce(&mut egui::Ui, &mut T),
) {
    let clear = if let Some(v) = value.as_mut() {
        add(ui, v);
        widgets::icon_button(ui, Icon::X, "clear: not given").clicked()
    } else {
        let given = ui
            .add(
                egui::Button::new(
                    egui::RichText::new("not given").color(theme::egui_color(theme::TEXT_4)),
                )
                .frame(false),
            )
            .on_hover_text("Give it")
            .clicked();
        if given {
            *value = Some(default);
        }
        false
    };
    if clear {
        *value = None;
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
    row(ui, label, errors, field, |ui| {
        m.length_field(ui, value, 0.05, 1000.0, "");
    });
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
    row(ui, label, errors, field, |ui| {
        optional(ui, value, default, |ui, v| {
            m.length_field(ui, v, 0.1, 1000.0, "");
        });
    });
}

fn opt_feed(
    ui: &mut egui::Ui,
    m: Measure,
    label: &str,
    value: &mut Option<f64>,
    errors: &[ToolFieldError],
    field: &str,
) {
    row(ui, label, errors, field, |ui| {
        optional(ui, value, 1000.0, |ui, v| {
            m.feed_field(ui, v, 50000.0);
        });
    });
}

fn opt_text(ui: &mut egui::Ui, label: &str, value: &mut Option<String>, hint: &str) {
    widgets::field_label(ui, label);
    let mut text = value.clone().unwrap_or_default();
    if ui
        .add(
            egui::TextEdit::singleline(&mut text)
                .hint_text(hint)
                .desired_width(220.0),
        )
        .changed()
    {
        *value = (!text.trim().is_empty()).then_some(text);
    }
    ui.end_row();
}

/// The height of a tool's picture.
const PICTURE: f32 = 180.0;

/// The tool, side on and to scale, with its holder face marked, on the CAD grid.
pub fn paint_tool(ui: &mut egui::Ui, tool: &ToolSpec, m: Measure) {
    let d = draw(tool);
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
        ui.label(
            egui::RichText::new(format!("assumed, not given: {}", d.assumed.join(", ")))
                .size(theme::TEXT_SM)
                .color(theme::egui_color(theme::TEXT_4)),
        );
    }
}
