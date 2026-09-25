//! The checks window's body: what the checks found for a setup's run, failures
//! first, each finding at a move a link that goes there.

use camcraft_session::checks::{Finding, Level};
use eframe::egui;
use glam::DVec3;

use super::{finding_segment, go_to};
use crate::document::SetupState;
use crate::ui::theme;
use crate::ui::widgets::{self, hint_text, section_label};

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
        ui.label(section_label(&group.title));
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
                    ui.colored_label(widgets::Level::Ok.color(), "pass");
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

/// One finding. Returns whether it was clicked, when it goes somewhere.
fn finding(ui: &mut egui::Ui, f: &Finding, goes: bool) -> bool {
    let (word, color) = match f.level {
        Level::Fail => ("FAIL", widgets::Level::Err.color()),
        Level::Warn => ("warn", widgets::Level::Warn.color()),
        Level::Note => ("note", theme::egui_color(theme::TEXT_3)),
        Level::Pass => ("pass", widgets::Level::Ok.color()),
    };
    let mut clicked = false;
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(color, egui::RichText::new(word).strong());
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
        ui.label(hint_text(&f.detail));
    }
    clicked
}
