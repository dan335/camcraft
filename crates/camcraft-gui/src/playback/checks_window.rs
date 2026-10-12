//! The checks window's body: what the checks found for a setup's run, failures
//! first, each as the design system's diagnostic, with a button that goes there
//! when it is at a move.

use camcraft_session::checks::{Finding, Level};
use eframe::egui;
use glam::DVec3;

use super::{finding_segment, go_to};
use crate::document::SetupState;
use crate::ui::theme;
use crate::ui::widgets::{self, hint_text, section_label};

/// What the checks found for this setup's run, failures first; going to a finding
/// that is at a move goes there. Returns a point to bring the camera in on.
pub fn checks(ui: &mut egui::Ui, state: &mut SetupState) -> Option<DVec3> {
    let ran = state.ran.as_ref()?;
    let checks = &ran.ran.checks;
    let scene = &ran.scene;
    let failed = checks.failures().count();
    if failed > 0 {
        // On `--err-subtle`, edged in `--err`, as the system's danger is.
        let err = theme::egui_color(theme::ERR);
        egui::Frame::new()
            .fill(theme::ERR_SUBTLE.color())
            .stroke(egui::Stroke::new(1.0, err))
            .inner_margin(egui::Margin::symmetric(12, 6))
            .corner_radius(theme::RADIUS_SM)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.colored_label(
                    err,
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

/// One finding, as a diagnostic, with a button to go there when it is at a move.
/// Returns whether that was clicked.
fn finding(ui: &mut egui::Ui, f: &Finding, goes: bool) -> bool {
    let message = format!("{}  {}", f.code, f.message);
    let mut clicked = false;
    let mut go = |ui: &mut egui::Ui| {
        if goes {
            clicked = widgets::button(
                ui,
                widgets::Variant::Ghost,
                widgets::Size::Sm,
                None,
                "go there",
            )
            .on_hover_text("go there, and show the stock as it stands there")
            .clicked();
        }
    };
    match f.level {
        Level::Fail => {
            widgets::diagnostic(ui, widgets::Level::Err, &message, go);
        }
        Level::Warn => {
            widgets::diagnostic(ui, widgets::Level::Warn, &message, go);
        }
        Level::Pass => {
            widgets::diagnostic(ui, widgets::Level::Ok, &message, go);
        }
        // The system's diagnostics have no level for a note: said quietly.
        Level::Note => {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    widgets::mono("note", theme::TEXT_XS).color(theme::egui_color(theme::TEXT_3)),
                );
                ui.label(message);
                go(ui);
            });
        }
    }
    if !f.detail.is_empty() {
        ui.label(hint_text(&f.detail));
    }
    clicked
}
