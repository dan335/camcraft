//! The fields of one tool, drawn and edited: shared by the tool manager, which
//! edits the library's tools, and an operation's panel, which edits the job's copy.
//!
//! The fields are on tabs, as Fusion's tool dialog has them, so each shows a
//! short form rather than one long scroll. A tab with a field in error carries a
//! red dot, so a problem out of sight is still seen.

use camcraft_core::cutting;
use camcraft_core::job::{
    Coolant, DRILL_POINT, Holder, HolderSegment, Leave, Preset, SpindleDirection, StockToLeave,
    ToolFieldError, ToolKind, ToolMaterial, ToolSpec, free_preset_name,
};
use eframe::egui;

use crate::help::topics::{feeds, library as lib, passes as pass};
use crate::help::{Help, Topic};
use crate::tools::drawing::kind_name;
use crate::ui::measure::Measure;
use crate::ui::theme;
use crate::ui::widgets::{self, Icon, Size, Tab, Variant, form};

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

/// What a value left out reads as.
const NOT_GIVEN: &str = "not given";

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
/// window. The tool's picture is [`crate::tools::drawing::paint_tool`], drawn by the
/// caller. A tool that is not `editable` shows its fields greyed, and its tabs and
/// presets can still be looked through.
pub fn tool_fields(ui: &mut egui::Ui, tool: &mut ToolSpec, m: Measure, editable: bool) {
    let (errors, preset_errors): (Vec<_>, Vec<_>) =
        tool.check().into_iter().partition(|e| e.preset.is_none());
    let flagged = |fields: &[&str]| errors.iter().any(|e| fields.contains(&e.field));

    let tabs = [
        Tab::new("General"),
        Tab::new("Cutter").flagged(flagged(&CUTTER_FIELDS)),
        Tab::new("Holder").flagged(flagged(&HOLDER_FIELDS)),
        Tab::new("Presets")
            .count(tool.presets.len())
            .flagged(flagged(&["rpm"]) || !preset_errors.is_empty()),
        Tab::new("Post"),
    ];
    let page = widgets::remembered_tabs(ui, ui.id().with("tool-page"), &tabs, 1, None);

    let page = PAGES.get(page).copied().unwrap_or(Page::Cutter);
    if page == Page::Presets {
        // Its list of presets is picked from even when nothing can be changed.
        speeds(ui, tool, m, &errors, &preset_errors, editable);
        return;
    }
    ui.add_enabled_ui(editable, |ui| match page {
        Page::General => form(ui, "general", |ui| general(ui, tool)),
        Page::Cutter => form(ui, "cutter", |ui| cutter(ui, tool, m, &errors)),
        Page::Holder => holder(ui, tool, m, &errors),
        Page::Presets => {}
        Page::Post => form(ui, "post", |ui| post(ui, tool)),
    });
}

fn general(ui: &mut egui::Ui, tool: &mut ToolSpec) {
    widgets::row(ui, &lib::DESCRIPTION, |ui| {
        ui.add(egui::TextEdit::singleline(&mut tool.comment).desired_width(220.0));
    });
    opt_text(ui, &lib::VENDOR, &mut tool.vendor, "e.g. Amana");
    opt_text(
        ui,
        &lib::PRODUCT,
        &mut tool.product,
        "the maker's part number",
    );
    widgets::row(ui, &lib::MATERIAL, |ui| {
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
    });
}

fn cutter(ui: &mut egui::Ui, tool: &mut ToolSpec, m: Measure, errors: &[ToolFieldError]) {
    widgets::row(ui, &lib::KIND, |ui| {
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
    });
    if tool.kind == ToolKind::Cone && tool.angle.is_none() {
        tool.angle = Some(90.0);
    }
    length(
        ui,
        m,
        &lib::DIAMETER,
        &mut tool.diameter,
        errors,
        "diameter",
    );
    if tool.kind == ToolKind::Bullnose {
        length(ui, m, &lib::CORNER, &mut tool.corner, errors, "corner");
    }
    if tool.kind == ToolKind::Drill {
        row(ui, &lib::POINT_ANGLE, errors, "angle", |ui| {
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
        row(ui, &lib::ANGLE, errors, "angle", |ui| {
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
            &lib::TIP_DIAMETER,
            &mut tool.tip_diameter,
            errors,
            "tip_diameter",
        );
    }
    row(ui, &lib::FLUTES, errors, "flutes", |ui| {
        widgets::optional(ui, &mut tool.flutes, 2, NOT_GIVEN, |ui, f| {
            ui.add(egui::DragValue::new(f).range(0..=16));
        });
    });
    opt_length(
        ui,
        m,
        &lib::FLUTE_LENGTH,
        &mut tool.flute_length,
        2.0 * tool.diameter,
        errors,
        "flute_length",
    );
    opt_length(
        ui,
        m,
        &lib::SHOULDER,
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
            &lib::SHANK,
            &mut tool.shank_diameter,
            tool.diameter,
            errors,
            "shank_diameter",
        );
        opt_length(
            ui,
            m,
            &lib::STICKOUT,
            &mut tool.stickout,
            4.0 * tool.diameter,
            errors,
            "stickout",
        );
        opt_length(
            ui,
            m,
            &lib::OVERALL,
            &mut tool.overall_length,
            8.0 * tool.diameter,
            errors,
            "overall_length",
        );
        row(ui, &lib::HOLDER, errors, "holder", |ui| {
            let default = Holder {
                name: String::new(),
                segments: vec![HolderSegment {
                    diameter: 32.0,
                    length: 20.0,
                }],
            };
            widgets::optional(ui, &mut tool.holder, default, NOT_GIVEN, |ui, h| {
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
        ui.label(widgets::section_label("Holder segments"))
            .help(&lib::HOLDER_SEGMENT);
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
                m.length_field(ui, &mut seg.diameter, 0.1, 1000.0, "Ø ")
                    .help(&lib::HOLDER_SEGMENT);
                m.length_field(ui, &mut seg.length, 0.1, 1000.0, "L ")
                    .help(&lib::HOLDER_SEGMENT);
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
    editable: bool,
) {
    ui.add_enabled_ui(editable, |ui| {
        form(ui, "spindle", |ui| {
            row(ui, &lib::TOOL_SPINDLE, errors, "rpm", |ui| {
                ui.add(
                    egui::DragValue::new(&mut tool.rpm)
                        .range(0..=60000)
                        .suffix(" rev/min"),
                );
            });
            let mut rpm = Some(tool.rpm);
            surface_speed_row(ui, &mut rpm, tool.diameter, m);
            if let Some(r) = rpm {
                tool.rpm = r;
            }
        })
    });
    ui.add_space(theme::SPACE_5);
    presets(ui, tool, m, preset_errors, editable);
}

fn post(ui: &mut egui::Ui, tool: &mut ToolSpec) {
    widgets::row(ui, &lib::NUMBER, |ui| {
        ui.add(
            egui::DragValue::new(&mut tool.number)
                .range(1..=9999)
                .prefix("T"),
        );
    });
    widgets::row(ui, &lib::SPINDLE_TURNS, |ui| {
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
    });
    widgets::row(ui, &lib::COOLANT, |ui| {
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
    });
}

/// The tool's presets: a list with a way to add, copy, reorder and remove them,
/// and the one picked shown in full below. The first is the one a new operation
/// gets.
fn presets(
    ui: &mut egui::Ui,
    tool: &mut ToolSpec,
    m: Measure,
    errors: &[ToolFieldError],
    editable: bool,
) {
    let mut act = None;
    ui.horizontal(|ui| {
        ui.label(widgets::section_label("Presets"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if editable
                && widgets::button(ui, Variant::Ghost, Size::Sm, Some(Icon::Plus), "Add preset")
                    .clicked()
            {
                act = Some(PresetAction::Add);
            }
        });
    });
    if tool.presets.is_empty() {
        ui.label(widgets::hint_text(
            "None yet. A preset is a named set of speeds and feeds, \
             e.g. Roughing and Finishing, that an operation copies.",
        ));
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
        widgets::list_row_background(ui, rect, on, response.hovered());
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
            widgets::Level::Err.color()
        } else if on {
            theme::egui_color(theme::TEXT_1)
        } else {
            theme::egui_color(theme::TEXT_2)
        };
        row.label(egui::RichText::new(name).color(color));
        if i == 0 {
            widgets::tag(&mut row, "default")
                .on_hover_text("A new operation with this tool gets this preset.");
        }
        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if on && editable {
                if widgets::icon_button(ui, Icon::Trash, "remove this preset").clicked() {
                    act = Some(PresetAction::Remove(i));
                }
                if widgets::icon_button(ui, Icon::Copy, "duplicate this preset").clicked() {
                    act = Some(PresetAction::Copy(i));
                }
                ui.add_enabled_ui(i + 1 < count, |ui| {
                    if widgets::icon_button(ui, Icon::ChevronDown, "move down").clicked() {
                        act = Some(PresetAction::Down(i));
                    }
                });
                ui.add_enabled_ui(i > 0, |ui| {
                    if widgets::icon_button(ui, Icon::ChevronUp, "move up").clicked() {
                        act = Some(PresetAction::Up(i));
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

    ui.add_enabled_ui(editable, |ui| preset_card(ui, tool, picked, m, errors));

    if let Some(act) = act {
        apply_preset_action(tool, act, &mut picked);
    }
    ui.data_mut(|d| d.insert_temp(picked_id, picked));
}

/// Something done to the list of presets, applied once it is drawn.
enum PresetAction {
    Add,
    Up(usize),
    Down(usize),
    Copy(usize),
    Remove(usize),
}

/// The preset picked, `picked`, in full, in a card below the list.
fn preset_card(
    ui: &mut egui::Ui,
    tool: &mut ToolSpec,
    picked: usize,
    m: Measure,
    errors: &[ToolFieldError],
) {
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
}

/// Do `action` to `tool`'s presets, and pick the one it leaves in hand.
fn apply_preset_action(tool: &mut ToolSpec, action: PresetAction, picked: &mut usize) {
    match action {
        PresetAction::Add => {
            let name = free_preset_name(&tool.presets, "Preset");
            tool.presets.push(Preset {
                name,
                rpm: Some(tool.rpm),
                ..Preset::default()
            });
            *picked = tool.presets.len() - 1;
        }
        PresetAction::Up(i) => {
            tool.presets.swap(i - 1, i);
            *picked = i - 1;
        }
        PresetAction::Down(i) => {
            tool.presets.swap(i, i + 1);
            *picked = i + 1;
        }
        PresetAction::Copy(i) => {
            if let Some(p) = tool.presets.get(i).cloned() {
                let name = free_preset_name(&tool.presets, &p.name);
                tool.presets.insert(i + 1, Preset { name, ..p });
                *picked = i + 1;
            }
        }
        PresetAction::Remove(i) => {
            tool.presets.remove(i);
            *picked = i.saturating_sub(1);
        }
    }
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

fn preset_fields(
    ui: &mut egui::Ui,
    p: &mut Preset,
    (diameter, flutes, tool_rpm): (f64, Option<u32>, u32),
    m: Measure,
    errors: &[ToolFieldError],
) {
    row(ui, &lib::PRESET_NAME, errors, "presets.name", |ui| {
        ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(180.0));
    });
    widgets::row(ui, &lib::NOTE, |ui| {
        ui.add(
            egui::TextEdit::singleline(&mut p.comment)
                .hint_text("e.g. from the maker's chart")
                .desired_width(180.0),
        );
    });
    row(ui, &lib::PRESET_SPINDLE, errors, "presets.rpm", |ui| {
        widgets::optional(ui, &mut p.rpm, tool_rpm, NOT_GIVEN, |ui, rpm| {
            ui.add(
                egui::DragValue::new(rpm)
                    .range(0..=60000)
                    .suffix(" rev/min"),
            );
        });
    });
    surface_speed_row(ui, &mut p.rpm, diameter, m);
    opt_feed(
        ui,
        m,
        &feeds::CUTTING_FEED,
        &mut p.feed,
        errors,
        "presets.feed",
    );
    chip_load_row(ui, &mut p.feed, p.rpm, flutes, m);
    opt_feed(
        ui,
        m,
        &feeds::PLUNGE_FEED,
        &mut p.plunge_feed,
        errors,
        "presets.plunge_feed",
    );
    opt_feed(
        ui,
        m,
        &lib::PRESET_RAMP_FEED,
        &mut p.ramp_feed,
        errors,
        "presets.ramp_feed",
    );
    opt_length(
        ui,
        m,
        &pass::STEPDOWN,
        &mut p.stepdown,
        0.5 * diameter,
        errors,
        "presets.stepdown",
    );
    opt_length(
        ui,
        m,
        &pass::STEPOVER,
        &mut p.stepover,
        0.4 * diameter,
        errors,
        "presets.stepover",
    );
    row(
        ui,
        &lib::PRESET_STOCK_TO_LEAVE,
        errors,
        "presets.stock_to_leave",
        |ui| {
            widgets::optional(
                ui,
                &mut p.stock_to_leave,
                StockToLeave::default(),
                NOT_GIVEN,
                |ui, s| {
                    // Whichever operation it is applied to checks it with its tool.
                    let least = StockToLeave {
                        radial: -diameter / 2.0,
                        axial: f64::NEG_INFINITY,
                    };
                    m.stock_to_leave_fields(ui, s, Leave::Both, least);
                },
            );
        },
    );
}

/// The surface speed at `rpm`, which sets `rpm` when edited. Makers chart
/// cutters in this, and it holds across diameters.
pub fn surface_speed_row(ui: &mut egui::Ui, rpm: &mut Option<u32>, diameter: f64, m: Measure) {
    widgets::row(ui, &feeds::SURFACE_SPEED, |ui| match *rpm {
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
            widgets::hint(ui, "needs a spindle speed");
        }
    });
}

/// The chip load at `feed` and `rpm`, which sets the feed when edited.
pub fn chip_load_row(
    ui: &mut egui::Ui,
    feed: &mut Option<f64>,
    rpm: Option<u32>,
    flutes: Option<u32>,
    m: Measure,
) {
    widgets::row(ui, &feeds::CHIP_LOAD, |ui| match (*feed, rpm, flutes) {
        (Some(now), Some(rpm), Some(z)) if rpm > 0 && z > 0 => {
            let fz = cutting::chip_load(now, f64::from(rpm), z);
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
                *feed = Some(cutting::feed_for(m.read(shown), f64::from(rpm), z));
            }
        }
        (_, _, None) | (_, _, Some(0)) => {
            widgets::hint(ui, "needs the tool's flutes");
        }
        _ => {
            widgets::hint(ui, "needs a spindle speed and cutting feed");
        }
    });
}

/// A spindle speed worked out from a surface speed, to the nearest rev/min and
/// within what a field takes.
fn whole_rpm(rpm: f64) -> u32 {
    // Rounded and clamped to 1..=60000 first, so it fits.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let whole = rpm.round().clamp(1.0, 60000.0) as u32;
    whole
}

/// One row of a form, [`widgets::row`]: the label, then what `add` puts in,
/// bordered in red when the checks find `field` wrong, with why on the line
/// under it.
fn row(
    ui: &mut egui::Ui,
    topic: &Topic,
    errors: &[ToolFieldError],
    field: &str,
    add: impl FnOnce(&mut egui::Ui),
) {
    let why: Vec<&str> = errors
        .iter()
        .filter(|e| e.field == field)
        .map(|e| e.why.as_str())
        .collect();
    widgets::row(ui, topic, |ui| {
        if !why.is_empty() {
            // The system's `invalid` field: an `--err` border, nothing louder.
            let err = egui::Stroke::new(1.0, widgets::Level::Err.color());
            let w = &mut ui.visuals_mut().widgets;
            w.inactive.bg_stroke = err;
            w.hovered.bg_stroke = err;
            w.active.bg_stroke = err;
        }
        add(ui);
    });
    for why in why {
        ui.label("");
        ui.add(egui::Label::new(widgets::error_text(why).size(theme::TEXT_SM)).wrap());
        ui.end_row();
    }
}

fn length(
    ui: &mut egui::Ui,
    m: Measure,
    topic: &Topic,
    value: &mut f64,
    errors: &[ToolFieldError],
    field: &str,
) {
    row(ui, topic, errors, field, |ui| {
        m.length_field(ui, value, 0.05, 1000.0, "");
    });
}

fn opt_length(
    ui: &mut egui::Ui,
    m: Measure,
    topic: &Topic,
    value: &mut Option<f64>,
    default: f64,
    errors: &[ToolFieldError],
    field: &str,
) {
    row(ui, topic, errors, field, |ui| {
        widgets::optional(ui, value, default, NOT_GIVEN, |ui, v| {
            m.length_field(ui, v, 0.1, 1000.0, "");
        });
    });
}

fn opt_feed(
    ui: &mut egui::Ui,
    m: Measure,
    topic: &Topic,
    value: &mut Option<f64>,
    errors: &[ToolFieldError],
    field: &str,
) {
    row(ui, topic, errors, field, |ui| {
        widgets::optional(ui, value, 1000.0, NOT_GIVEN, |ui, v| {
            m.feed_field(ui, v, 50000.0);
        });
    });
}

fn opt_text(ui: &mut egui::Ui, topic: &Topic, value: &mut Option<String>, hint: &str) {
    widgets::row(ui, topic, |ui| {
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
    });
}
