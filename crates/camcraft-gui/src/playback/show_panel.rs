//! The setup's panel on the left: what its last run did, what is shown of it, the
//! material and how it is coloured, and the steep descents.

use eframe::egui;
use glam::DVec3;

use super::{jump_to, thousands};
use crate::document::{RanSetup, SetupState, SetupView, StockMode};
use crate::stock::live::LiveStock;
use crate::stock::stock_view::{CLASSES, Class, ColorMode, cell_color, class_name};
use crate::ui::measure::Measure;
use crate::ui::theme;
use crate::ui::widgets::{self, hint_text, section_label};
use crate::view::scene::{Playhead, Scene, Steep};
use crate::view::viewport::StockRef;

/// A checkbox for one bit of a mask.
fn bit(ui: &mut egui::Ui, mask: &mut u32, index: usize, label: impl Into<egui::WidgetText>) {
    let Some(flag) = u32::try_from(index).ok().and_then(|i| 1u32.checked_shl(i)) else {
        return;
    };
    let mut on = *mask & flag != 0;
    if ui.checkbox(&mut on, label).changed() {
        if on {
            *mask |= flag;
        } else {
            *mask &= !flag;
        }
    }
}

/// A square of `color`, as a legend keys what it names.
fn swatch(ui: &mut egui::Ui, color: egui::Color32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    ui.painter().rect_filled(r, 2.0, color);
}

/// One row of a legend: `left`, then at the right its count, `count`, and
/// before that `right`.
fn legend_row(
    ui: &mut egui::Ui,
    count: usize,
    left: impl FnOnce(&mut egui::Ui),
    right: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        left(ui);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            right(ui);
            ui.label(hint_text(thousands(count)));
        });
    });
}

/// What the last run did, and its verdict.
pub fn result(ui: &mut egui::Ui, state: &SetupState, m: Measure) {
    let Some(ran) = &state.ran else {
        return;
    };
    let result = &ran.ran.result;
    egui::Grid::new("result")
        .num_columns(4)
        .striped(true)
        .show(ui, |ui| {
            for heading in ["operation", "passes", "removed", "rapid hits"] {
                ui.label(section_label(heading));
            }
            ui.end_row();
            for op in &result.operations {
                ui.label(&op.name);
                ui.label(op.passes.to_string());
                ui.label(m.volume(op.sim.removed_mm3));
                ui.label(op.sim.rapid_contacts.to_string());
                ui.end_row();
            }
        });
    let v = &result.verify;
    if result.is_clean() {
        ui.label(format!(
            "Verified: {} columns compared, no gouges.",
            v.compared
        ));
    } else {
        ui.colored_label(
            widgets::Level::Err.color(),
            format!(
                "NOT verified: {} gouged column(s), deepest {}.",
                v.gouged,
                m.length(v.deepest_gouge_mm, 3)
            ),
        );
    }
}

/// What to show, and the material. Returns a
/// point to bring the camera in on, when a steep descent is clicked.
pub fn controls(ui: &mut egui::Ui, state: &mut SetupState, m: Measure) -> Option<DVec3> {
    let view = &mut state.view;
    ui.label(section_label("Show"));
    ui.checkbox(&mut view.view.part, "part");
    ui.checkbox(&mut view.view.stock, "stock outline");
    ui.checkbox(&mut view.view.origin, "work zero");
    let Some(ran) = &state.ran else {
        return None;
    };
    toolpaths(ui, view, &ran.scene);
    ui.separator();
    stock(ui, view, state.live.as_ref(), &ran.scene);
    let go = steep(ui, ran, m);
    go.map(|s| jump_to(state, s))
}

/// The cutter and the toolpaths, by role and by operation, each with its count.
fn toolpaths(ui: &mut egui::Ui, view: &mut SetupView, scene: &Scene) {
    ui.checkbox(&mut view.show_cutter, "cutter");
    ui.checkbox(&mut view.show_paths, "toolpaths");
    ui.add_space(4.0);
    for (i, role) in theme::ROLES.iter().enumerate() {
        let n = scene.role_counts.get(i).copied().unwrap_or(0);
        if n == 0 {
            continue;
        }
        legend_row(
            ui,
            n,
            |ui| {
                bit(ui, &mut view.view.roles, i, "");
                swatch(ui, theme::egui_color(theme::role_srgb(*role)));
                ui.label(theme::role_name(*role));
            },
            |_| {},
        );
    }
    let mut jump = None;
    if scene.ops.len() > 1 {
        ui.add_space(4.0);
        for (i, op) in scene.ops.iter().enumerate() {
            legend_row(
                ui,
                op.count,
                |ui| bit(ui, &mut view.view.ops, i, format!("{}. {}", i + 1, op.name)),
                |ui| {
                    let go = widgets::button(
                        ui,
                        widgets::Variant::Ghost,
                        widgets::Size::Sm,
                        None,
                        "go to end",
                    );
                    if go
                        .on_hover_text("show everything up to the end of this operation")
                        .clicked()
                    {
                        jump = Some(op.first + op.count);
                    }
                },
            );
        }
    }
    if let Some(end) = jump {
        view.at = Playhead::at(end);
    }
}

/// Which material shows, how it is coloured, and a legend of its cells.
fn stock(ui: &mut egui::Ui, view: &mut SetupView, live: Option<&LiveStock>, scene: &Scene) {
    ui.label(section_label("Stock"));
    let stage_name = |i: usize| match i {
        0 => "the block, before anything".to_string(),
        n => scene
            .ops
            .get(n - 1)
            .map_or_else(String::new, |o| format!("after {n}. {}", o.name)),
    };
    let selected = match view.stock_mode {
        StockMode::Off => "off".to_string(),
        StockMode::Follow => "cut as it plays".to_string(),
        StockMode::Stage(i) => stage_name(i),
    };
    egui::ComboBox::from_id_salt("stock")
        .selected_text(selected)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut view.stock_mode, StockMode::Off, "off");
            ui.selectable_value(&mut view.stock_mode, StockMode::Follow, "cut as it plays");
            for i in 0..=scene.ops.len() {
                ui.selectable_value(&mut view.stock_mode, StockMode::Stage(i), stage_name(i));
            }
        });
    ui.checkbox(
        &mut view.hide_part_with_stock,
        "hide the part while showing it",
    );
    ui.horizontal(|ui| {
        ui.label("colours");
        ui.selectable_value(&mut view.transport.color, ColorMode::Material, "material")
            .on_hover_text("the shape being cut; red where it goes below the part");
        ui.selectable_value(&mut view.transport.color, ColorMode::Compare, "compare")
            .on_hover_text("every column against the part, by the verifier's rule");
    });
    // In a block, so the image (playback's is locked, shared with its thread) is
    // held no longer than the legend needs it.
    {
        let image = match view.stock_mode {
            StockMode::Off => None,
            StockMode::Stage(i) => view
                .stock_images
                .get(i)
                .and_then(Option::as_ref)
                .map(|(_, image)| StockRef::Plain(image)),
            StockMode::Follow => live
                .and_then(|l| l.shown())
                .map(|(_, image, _)| StockRef::Shared(image)),
        };
        let mode = view.transport.color;
        if let Some(image) = image {
            let counts: Vec<usize> = image.with(|i| CLASSES.iter().map(|c| i.count(*c)).collect());
            let count = |class: Class| {
                CLASSES
                    .iter()
                    .position(|c| *c == class)
                    .and_then(|i| counts.get(i))
                    .copied()
                    .unwrap_or(0)
            };
            let classes: &[Class] = match mode {
                ColorMode::Compare => &CLASSES,
                ColorMode::Material => &[Class::Gouge],
            };
            for &class in classes {
                legend_row(
                    ui,
                    count(class),
                    |ui| {
                        swatch(ui, theme::egui_color(cell_color(mode, class, 0.0)));
                        ui.label(class_name(class));
                    },
                    |_| {},
                );
            }
            ui.label(hint_text(
                "cells of the simulated material, by the verifier's rule",
            ));
        }
    }
}

/// The steep descents, by operation and then deepest first. The one clicked.
fn steep(ui: &mut egui::Ui, ran: &RanSetup, m: Measure) -> Option<Steep> {
    let scene = &ran.scene;
    let mut go = None;
    if !scene.steep.is_empty() {
        ui.separator();
        ui.label(section_label("Steep descents"));
        ui.label(hint_text(
            "cutting descents that drop further than allowed while barely advancing",
        ));
        let setup = ran
            .job
            .setups
            .iter()
            .find(|s| s.name == ran.ran.result.setup);
        for (i, op) in scene.ops.iter().enumerate() {
            let mine: Vec<&Steep> = scene.steep.iter().filter(|s| s.op == i).collect();
            if mine.is_empty() {
                continue;
            }
            let deepest = mine.iter().map(|s| s.drop).fold(0.0, f64::max);
            let allowance = setup
                .and_then(|s| s.operations.get(i))
                .map_or(0.0, |o| o.passes.max_cut_step);
            ui.label(format!(
                "{}. {}: {} descent(s), deepest {} of {} allowed",
                i + 1,
                op.name,
                mine.len(),
                m.length(deepest, 2),
                m.length(allowance, 2)
            ));
        }
        // Deepest first, as the scene keeps them: the worst is the one to look at.
        ui.label(hint_text(format!(
            "drop, operation and position ({})",
            m.unit()
        )));
        egui::ScrollArea::vertical()
            .id_salt("steep")
            .max_height(160.0)
            .show(ui, |ui| {
                for s in scene.steep.iter().take(200) {
                    let label = format!(
                        "{}  {}. at {}, {}, {}",
                        m.number(s.drop, 2),
                        s.op + 1,
                        m.number(s.at.x, 1),
                        m.number(s.at.y, 1),
                        m.number(s.at.z, 1)
                    );
                    if ui
                        .selectable_label(false, egui::RichText::new(label).monospace())
                        .on_hover_text("go there, and show the stock as it stands there")
                        .clicked()
                    {
                        go = Some(*s);
                    }
                }
            });
    }
    go
}
