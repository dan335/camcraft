//! Looking at a setup's result: what it did, the scrub through its program in
//! machine order, the material at any stage, and the steep descents.
//!
//! All of it is per setup, kept in its [`SetupState`], so each setup of each open
//! job keeps its own place.

use eframe::egui;
use glam::DVec3;

use crate::document::{SetupState, StockMode};
use crate::measure::Measure;
use crate::scene::{Steep, clock};
use crate::stock_view::{CLASSES, StockImage, class_color, class_name};
use crate::theme;
use crate::viewport::{Content, View};

/// `12345` as `12,345`, as the inspector printed counts.
pub fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

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

/// Whether the last run was laid out as the setup is now, so its paths belong on
/// the part as drawn.
pub fn paths_fit(state: &SetupState) -> bool {
    match (&state.ran, &state.layout) {
        (Some(ran), Some(layout)) => layout
            .prepared
            .as_ref()
            .is_ok_and(|p| p.frame == ran.ran.result.frame && p.stocks == ran.ran.result.stocks),
        (Some(_), None) => true,
        _ => false,
    }
}

/// The stage the stock view shows, if any.
pub fn stage_shown(state: &SetupState) -> Option<usize> {
    let view = &state.view;
    match view.stock_mode {
        StockMode::Off => None,
        StockMode::Stage(i) => Some(i),
        StockMode::Follow => {
            let scene = &state.ran.as_ref()?.scene;
            let at = scene.position(view.scrub);
            let moves = scene.ops.get(at.op).map_or(0, |o| o.count + 1);
            Some(if at.moves >= moves { at.op + 1 } else { at.op })
        }
    }
}

/// Classify the shown stage's material, the first time it is shown.
pub fn ensure_stock_image(state: &mut SetupState) {
    let Some(stage) = stage_shown(state) else {
        return;
    };
    let Some(ran) = &state.ran else {
        return;
    };
    let result = &ran.ran.result;
    if let (Some(slot @ None), Some(stock)) = (
        state.view.stock_images.get_mut(stage),
        result.stages.get(stage),
    ) {
        *slot = Some(StockImage::build(stock, &result.part, result.verify_budget));
    }
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
            ui.strong("operation");
            ui.strong("passes");
            ui.strong("removed");
            ui.strong("rapid hits");
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
            ui.visuals().error_fg_color,
            format!(
                "NOT verified: {} gouged column(s), deepest {}.",
                v.gouged,
                m.length(v.deepest_gouge_mm, 3)
            ),
        );
    }
    for w in &ran.ran.warnings {
        ui.colored_label(ui.visuals().warn_fg_color, w);
    }
}

/// What to show, and the scrub through the program in machine order. Returns a
/// point to bring the camera in on, when a steep descent is clicked.
pub fn controls(ui: &mut egui::Ui, state: &mut SetupState, m: Measure) -> Option<DVec3> {
    let view = &mut state.view;
    ui.heading("Show");
    ui.checkbox(&mut view.view.part, "part");
    ui.checkbox(&mut view.view.stock, "stock outline");
    ui.checkbox(&mut view.view.origin, "work zero");
    let Some(ran) = &state.ran else {
        return None;
    };
    let scene = &ran.scene;
    ui.checkbox(&mut view.show_cutter, "cutter at the scrub");
    ui.checkbox(&mut view.show_paths, "toolpaths");
    ui.add_space(4.0);
    for (i, role) in theme::ROLES.iter().enumerate() {
        let n = scene.role_counts.get(i).copied().unwrap_or(0);
        if n == 0 {
            continue;
        }
        ui.horizontal(|ui| {
            bit(ui, &mut view.view.roles, i, "");
            let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(r, 2.0, theme::egui_color(theme::role_srgb(*role)));
            ui.label(theme::role_name(*role));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(thousands(n));
            });
        });
    }
    let mut jump = None;
    if scene.ops.len() > 1 {
        ui.add_space(4.0);
        for (i, op) in scene.ops.iter().enumerate() {
            ui.horizontal(|ui| {
                bit(ui, &mut view.view.ops, i, format!("{}. {}", i + 1, op.name));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("scrub to end")
                        .on_hover_text("show everything up to the end of this operation")
                        .clicked()
                    {
                        jump = Some(op.first + op.count);
                    }
                    ui.weak(thousands(op.count));
                });
            });
        }
    }
    if let Some(end) = jump {
        view.scrub = end;
    }

    ui.separator();
    ui.heading("Move order");
    let total = scene.segments.len();
    ui.spacing_mut().slider_width = ui.available_width() - 8.0;
    ui.add(egui::Slider::new(&mut view.scrub, 0..=total).show_value(false));
    ui.label(format!(
        "showing {} / {}",
        thousands(view.scrub),
        thousands(total)
    ));
    if let Some(at) = scene.at(view.scrub) {
        let s = at.segment;
        ui.label(format!("{}. {}", at.op + 1, at.op_name));
        let feed = s
            .feed
            .map_or_else(String::new, |f| format!("  F{}", m.feed_number(f)));
        ui.monospace(format!(
            "{} to {}, {}, {} {}{feed}",
            theme::role_name(s.role),
            m.number(s.b.x, 2),
            m.number(s.b.y, 2),
            m.number(s.b.z, 2),
            m.unit()
        ));
        ui.label(format!(
            "{} of {}, estimated",
            clock(at.segment.ends_at),
            clock(scene.total_time())
        ));
    }
    ui.weak("Left and Right arrows step one move; with Shift, a hundred.");

    ui.separator();
    ui.heading("Stock");
    let stage_name = |i: usize| match i {
        0 => "the block, before anything".to_string(),
        n => scene
            .ops
            .get(n - 1)
            .map_or_else(String::new, |o| format!("after {n}. {}", o.name)),
    };
    let selected = match view.stock_mode {
        StockMode::Off => "off".to_string(),
        StockMode::Follow => "follow the scrub".to_string(),
        StockMode::Stage(i) => stage_name(i),
    };
    egui::ComboBox::from_id_salt("stock")
        .selected_text(selected)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut view.stock_mode, StockMode::Off, "off");
            ui.selectable_value(&mut view.stock_mode, StockMode::Follow, "follow the scrub");
            for i in 0..=scene.ops.len() {
                ui.selectable_value(&mut view.stock_mode, StockMode::Stage(i), stage_name(i));
            }
        });
    ui.checkbox(
        &mut view.hide_part_with_stock,
        "hide the part while showing it",
    );
    let shown = match view.stock_mode {
        StockMode::Off => None,
        StockMode::Stage(i) => Some(i),
        StockMode::Follow => {
            let at = scene.position(view.scrub);
            let moves = scene.ops.get(at.op).map_or(0, |o| o.count + 1);
            Some(if at.moves >= moves { at.op + 1 } else { at.op })
        }
    };
    let image = shown
        .and_then(|i| view.stock_images.get(i))
        .and_then(Option::as_ref);
    if let Some(image) = image {
        for class in CLASSES {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(r, 2.0, theme::egui_color(class_color(class)));
                ui.label(class_name(class));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(thousands(image.count(class)));
                });
            });
        }
        ui.weak("cells of the simulated material, by the verifier's rule");
    }

    let mut go = None;
    if !scene.steep.is_empty() {
        ui.separator();
        ui.heading("Steep descents");
        ui.weak("cutting moves that drop further than allowed while barely advancing");
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
                "{}. {}: {} move(s), deepest {} of {} allowed",
                i + 1,
                op.name,
                mine.len(),
                m.length(deepest, 2),
                m.length(allowance, 2)
            ));
        }
        // Deepest first: the worst is the one to look at.
        let mut worst: Vec<Steep> = scene.steep.clone();
        worst.sort_by(|a, b| b.drop.total_cmp(&a.drop));
        ui.weak(format!("drop, operation and position ({})", m.unit()));
        egui::ScrollArea::vertical()
            .id_salt("steep")
            .max_height(160.0)
            .show(ui, |ui| {
                for s in worst.iter().take(200) {
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
                        .on_hover_text("go there: scrub to it and follow the stock")
                        .clicked()
                    {
                        go = Some(*s);
                    }
                }
            });
    }
    go.map(|s| jump_to(state, s))
}

/// Move the scrub to a steep descent, following the stock; where to look.
pub fn jump_to(state: &mut SetupState, steep: Steep) -> DVec3 {
    state.view.scrub = steep.index + 1;
    state.view.stock_mode = StockMode::Follow;
    steep.at
}

/// Arrow keys step the scrub, unless a widget has the keyboard.
pub fn step_scrub(ctx: &egui::Context, state: &mut SetupState) {
    let Some(total) = state.ran.as_ref().map(|r| r.scene.segments.len()) else {
        return;
    };
    if ctx.memory(|m| m.focused().is_some()) {
        return;
    }
    let (left, right, shift) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowLeft),
            i.key_pressed(egui::Key::ArrowRight),
            i.modifiers.shift,
        )
    });
    let step = if shift { 100 } else { 1 };
    let scrub = &mut state.view.scrub;
    if right {
        *scrub = (*scrub + step).min(total);
    }
    if left {
        *scrub = scrub.saturating_sub(step);
    }
}

/// What the viewport should draw for this setup.
pub fn content(state: &SetupState) -> Content<'_> {
    let view = &state.view;
    let fits = paths_fit(state);
    let scene = state.ran.as_ref().filter(|_| fits);
    let total = scene.map_or(0, |r| r.scene.segments.len());
    let cutter = scene
        .filter(|_| view.show_cutter && view.scrub < total)
        .and_then(|r| r.scene.at(view.scrub))
        .map(|at| (at.op, at.segment.b));
    let stock = scene.and_then(|r| {
        let i = stage_shown(state)?;
        let image = view.stock_images.get(i)?.as_ref()?;
        Some(((r.scene_key, i), image))
    });
    let laid = state
        .layout
        .as_ref()
        .and_then(|l| Some((l.key, l.prepared.as_ref().ok()?, &l.outline)));
    let part = laid
        .map(|(key, p, _)| (key, &p.part))
        .or_else(|| state.ran.as_ref().map(|r| (r.scene_key, &r.ran.part)));
    let show_part = view.view.part && !(stock.is_some() && view.hide_part_with_stock);
    Content {
        part,
        framing: view.framing,
        scene: scene.map(|r| (r.scene_key, &r.scene)),
        outline: laid.map(|(key, _, o)| (key, o.as_slice())),
        stock,
        view: View {
            part: show_part,
            roles: if view.show_paths { view.view.roles } else { 0 },
            shown: view.scrub,
            cutter,
            ..view.view
        },
    }
}
