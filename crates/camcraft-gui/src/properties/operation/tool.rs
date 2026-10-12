//! An operation's Tool tab: the tool it cuts with and its preset, then the
//! speeds and feeds it cuts at.

use camcraft_core::job::Operation;
use eframe::egui;

use crate::document::Document;
use crate::help::topics::feeds;
use crate::properties::PropAction;
use crate::tools::drawing::paint_tool;
use crate::tools::editor::{chip_load_row, surface_speed_row};
use crate::ui::measure::Measure;
use crate::ui::theme;
use crate::ui::widgets::{self, Size, Variant, error, form};

/// The tool of operation `index` of `setup`, `tool_name`, and its preset, then
/// the speeds and feeds it cuts at. The tool itself is edited in the tool
/// library.
pub(super) fn tool(
    ui: &mut egui::Ui,
    doc: &mut Document,
    m: Measure,
    (setup, index): (&str, usize),
    tool_name: Option<&str>,
    actions: &mut Vec<PropAction>,
) {
    form(ui, "op-tool", |ui| {
        widgets::row(ui, &feeds::TOOL, |ui| {
            if let Some(t) = tool_name {
                ui.strong(t);
            }
            if widgets::button(ui, Variant::Secondary, Size::Sm, None, "Select tool…").clicked() {
                actions.push(PropAction::PickTool(setup.to_string(), index));
            }
        });
        if let Some(t) = tool_name {
            preset_row(ui, doc, setup, index, t);
        }
    });
    let Some(t) = tool_name else {
        ui.add_space(theme::SPACE_3);
        widgets::diagnostic(ui, widgets::Level::Warn, "no tool yet", |_| {});
        return;
    };
    if let Some(clash) = doc.number_clash(t) {
        let number = doc.job.tools.get(t).map_or(0, |x| x.number);
        let mut renumber = false;
        widgets::diagnostic(
            ui,
            widgets::Level::Err,
            &format!("'{clash}' is also T{number}"),
            |ui| {
                renumber =
                    widgets::button(ui, Variant::Secondary, Size::Sm, None, "Renumber").clicked();
            },
        );
        if renumber {
            let free = doc.free_number();
            if let Some(spec) = doc.job.tools.get_mut(t) {
                spec.number = free;
            }
        }
    }
    ui.add_space(theme::SPACE_3);
    let Some(spec) = doc.job.tools.get(t) else {
        error(ui, &format!("'{t}' is not among the job's tools"));
        return;
    };
    paint_tool(ui, spec, m);
    let cutter = (spec.rpm, spec.diameter, spec.flutes);
    ui.add_space(theme::SPACE_4);
    ui.label(widgets::section_label("Feed and speed"));
    ui.add_space(theme::SPACE_2);
    if let Some(op) = doc.operation_mut(setup, index) {
        form(ui, "feed-speed", |ui| feed_speed(ui, m, op, cutter));
    }
}

/// The spindle speed and the feeds the operation cuts at, with the surface speed
/// and chip load they come to, which set them when edited.
fn feed_speed(
    ui: &mut egui::Ui,
    m: Measure,
    op: &mut Operation,
    (tool_rpm, diameter, flutes): (u32, f64, Option<u32>),
) {
    widgets::row(ui, &feeds::SPINDLE, |ui| {
        let absent = format!("the tool's, {tool_rpm} rev/min");
        widgets::optional(ui, &mut op.passes.rpm, tool_rpm, &absent, |ui, rpm| {
            ui.add(
                egui::DragValue::new(rpm)
                    .range(1..=60000)
                    .suffix(" rev/min"),
            );
        });
    });
    // What the spindle turns at, the operation's own or the tool's.
    let turning = op.passes.rpm.or(Some(tool_rpm));
    let mut rpm = turning;
    surface_speed_row(ui, &mut rpm, diameter, m);
    if rpm != turning {
        op.passes.rpm = rpm;
    }
    // Drilling feeds its holes at the plunge feed: it has no cutting feed.
    if op.strategy.uses().feed {
        widgets::row(ui, &feeds::CUTTING_FEED, |ui| {
            m.feed_field(ui, &mut op.passes.feed, 50_000.0);
        });
        let mut feed = Some(op.passes.feed);
        chip_load_row(ui, &mut feed, op.passes.rpm.or(Some(tool_rpm)), flutes, m);
        if let Some(f) = feed {
            op.passes.feed = f;
        }
    }
    widgets::row(ui, &feeds::PLUNGE_FEED, |ui| {
        m.feed_field(ui, &mut op.linking.plunge_feed, 50_000.0);
    });
}

/// Which of the tool's presets the operation's values came from: a row of the
/// tool form.
fn preset_row(ui: &mut egui::Ui, doc: &mut Document, setup: &str, index: usize, tool: &str) {
    let presets = doc
        .job
        .tools
        .get(tool)
        .map(|t| t.presets.clone())
        .unwrap_or_default();
    let Some(op) = doc.operation_mut(setup, index) else {
        return;
    };
    let none = "none";
    widgets::row(ui, &feeds::PRESET, |ui| {
        egui::ComboBox::from_id_salt(("op-preset", setup, index))
            .selected_text(op.preset.as_deref().unwrap_or(none))
            .show_ui(ui, |ui| {
                for p in &presets {
                    if ui
                        .selectable_label(op.preset.as_deref() == Some(p.name.as_str()), &p.name)
                        .clicked()
                    {
                        op.apply_preset(p);
                    }
                }
                if ui.selectable_label(op.preset.is_none(), none).clicked() {
                    op.preset = None;
                }
            });
    });
}
