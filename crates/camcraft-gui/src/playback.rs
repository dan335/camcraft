//! Looking at a setup's result: what it did, the playhead through its program in
//! machine order, the material at any stage, and the steep descents.
//!
//! All of it is per setup, kept in its [`SetupState`], so each setup of each open
//! job keeps its own place.

use camcraft_session::checks::{Finding, Level};
use eframe::egui;
use glam::DVec3;

use crate::document::{SetupState, StockMode, next_key};
use crate::live::{LiveStock, Target};
use crate::measure::Measure;
use crate::scene::{Playhead, Scene, Steep};
use crate::stock_view::{CLASSES, Class, ColorMode, StockImage, cell_color, class_name};
use crate::theme;
use crate::viewport::{Content, StockChange, StockRef, StockShow, View};

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

/// The fixed stage the stock view shows, if it shows one.
pub fn stage_shown(state: &SetupState) -> Option<usize> {
    match state.view.stock_mode {
        StockMode::Stage(i) => Some(i),
        StockMode::Off | StockMode::Follow => None,
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
    if let (Some(slot @ None), Some(Ok(stock))) = (
        state.view.stock_images.get_mut(stage),
        result.keyframes.get(stage).map(|s| s.unpack()),
    ) {
        let image = StockImage::build(&stock, &result.part, result.verify_budget);
        *slot = Some((next_key(), image));
    }
}

/// Keep the playback material following the playhead, while it is shown.
pub fn follow_stock(ctx: &egui::Context, state: &mut SetupState) {
    if state.view.stock_mode != StockMode::Follow || !paths_fit(state) {
        return;
    }
    let Some(ran) = &state.ran else {
        return;
    };
    if state
        .live
        .as_ref()
        .is_none_or(|l| l.scene_key != ran.scene_key)
    {
        state.live = LiveStock::start(ctx, ran);
    }
    let (at, fraction) = ran.scene.target(state.view.at);
    if let Some(live) = &mut state.live {
        live.seek(Target { at, fraction });
        live.poll();
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
}

/// What to show, and the material. Returns a
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
    ui.checkbox(&mut view.show_cutter, "cutter");
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
                        .small_button("go to end")
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
        view.at = Playhead::at(end);
    }

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
            StockMode::Follow => state
                .live
                .as_ref()
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
                ui.horizontal(|ui| {
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(
                        r,
                        2.0,
                        theme::egui_color(cell_color(mode, class, 0.0)),
                    );
                    ui.label(class_name(class));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.weak(thousands(count(class)));
                    });
                });
            }
            ui.weak("cells of the simulated material, by the verifier's rule");
        }
    }

    let mut go = None;
    if !scene.steep.is_empty() {
        ui.separator();
        ui.heading("Steep descents");
        ui.weak("cutting descents that drop further than allowed while barely advancing");
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
                        .on_hover_text("go there, and show the stock as it stands there")
                        .clicked()
                    {
                        go = Some(*s);
                    }
                }
            });
    }
    go.map(|s| jump_to(state, s))
}

/// What the checks found for this setup's run, failures first; clicking a finding
/// that is at a move goes there. Returns a point to bring the camera in on.
pub fn checks(ui: &mut egui::Ui, state: &mut SetupState) -> Option<DVec3> {
    let ran = state.ran.as_ref()?;
    let checks = &ran.ran.checks;
    let scene = &ran.scene;
    let failed = checks.failures().count();
    if failed > 0 {
        egui::Frame::new()
            .fill(theme::egui_color(theme::ERR).gamma_multiply(0.18))
            .stroke(egui::Stroke::new(1.0, theme::egui_color(theme::ERR)))
            .inner_margin(6.0)
            .corner_radius(4.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.colored_label(
                    theme::egui_color(theme::ERR),
                    egui::RichText::new(format!(
                        "HALT: {failed} check(s) failed. Do not run this program."
                    ))
                    .strong(),
                );
            });
    }
    let mut go = None;
    let mut passed = Vec::new();
    for group in &checks.groups {
        let shown: Vec<_> = group
            .findings
            .iter()
            .filter(|f| f.level != Level::Pass)
            .collect();
        passed.extend(
            group
                .findings
                .iter()
                .filter(|f| f.level == Level::Pass)
                .map(|f| (group, f)),
        );
        if shown.is_empty() {
            continue;
        }
        ui.strong(&group.title);
        for f in shown {
            let segment = finding_segment(scene, group.op, f);
            if finding(ui, f, segment.is_some()) {
                go = segment;
            }
        }
    }
    egui::CollapsingHeader::new(format!("{} passed", passed.len()))
        .id_salt("passed")
        .show(ui, |ui| {
            for (group, f) in passed {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(theme::egui_color(theme::PATH_CUT), "pass");
                    ui.monospace(f.code);
                    ui.label(format!("{}: {}", group.title, f.message));
                });
            }
        });
    let i = go?;
    let at = scene.segments.get(i)?.b;
    go_to(state, i + 1);
    Some(at)
}

/// The segment a finding in operation `op` is at, if it is at a move: the
/// segment ending there, since move `k` of an operation ends segment `k - 1` of
/// its span.
pub fn finding_segment(scene: &Scene, op: Option<usize>, f: &Finding) -> Option<usize> {
    op.zip(f.at_move)
        .and_then(|(op, k)| Some(scene.ops.get(op)?.first + k.saturating_sub(1)))
        .filter(|&i| i < scene.segments.len())
}

/// Stop and stand after `shown` segments, cutting the stock as it plays.
fn go_to(state: &mut SetupState, shown: usize) {
    state.view.at = Playhead::at(shown);
    state.view.transport.playing = false;
    state.view.stock_mode = StockMode::Follow;
}

/// One finding. Returns whether it was clicked, when it goes somewhere.
fn finding(ui: &mut egui::Ui, f: &Finding, goes: bool) -> bool {
    let (word, color) = match f.level {
        Level::Fail => ("FAIL", theme::ERR),
        Level::Warn => ("warn", theme::AMBER_5),
        Level::Note => ("note", theme::GRAY_7),
        Level::Pass => ("pass", theme::PATH_CUT),
    };
    let mut clicked = false;
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(theme::egui_color(color), egui::RichText::new(word).strong());
        ui.monospace(f.code);
        if goes {
            clicked = ui
                .link(&f.message)
                .on_hover_text("go there, and show the stock as it stands there")
                .clicked();
        } else {
            ui.label(&f.message);
        }
    });
    if !f.detail.is_empty() {
        ui.weak(&f.detail);
    }
    clicked
}

/// Move the playhead to a steep descent, following the stock; where to look.
pub fn jump_to(state: &mut SetupState, steep: Steep) -> DVec3 {
    go_to(state, steep.index + 1);
    steep.at
}

/// What the viewport should draw for this setup.
pub fn content(state: &SetupState) -> Content<'_> {
    let view = &state.view;
    let fits = paths_fit(state);
    let scene = state.ran.as_ref().filter(|_| fits);
    let cutter = scene
        .filter(|_| view.show_cutter)
        .and_then(|r| r.scene.cutter_at(view.at));
    let stock = scene.and_then(|r| match view.stock_mode {
        StockMode::Off => None,
        StockMode::Stage(i) => {
            let (key, image) = view.stock_images.get(i)?.as_ref()?;
            Some(StockShow {
                key: (*key, i),
                image: StockRef::Plain(image),
                changed: StockChange::Nothing,
                mode: view.transport.color,
            })
        }
        StockMode::Follow => {
            let live = state.live.as_ref().filter(|l| l.scene_key == r.scene_key)?;
            let (key, image, changed) = live.shown()?;
            Some(StockShow {
                key: (key, usize::MAX),
                image: StockRef::Shared(image),
                changed,
                mode: view.transport.color,
            })
        }
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
        stock_box: laid.map(|(_, p, _)| p.stock_bounds()).or_else(|| {
            let stocks = &state.ran.as_ref()?.ran.result.stocks;
            stocks.iter().copied().reduce(|a, b| a.union(&b))
        }),
        stock,
        view: View {
            part: show_part,
            roles: if view.show_paths { view.view.roles } else { 0 },
            shown: view.at.shown,
            cutter,
            ..view.view
        },
        selection: None,
        hover: None,
        ghost: None,
        hint: None,
        planes: &[],
    }
}
