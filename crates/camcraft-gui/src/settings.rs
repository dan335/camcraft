//! The person's settings: how the app shows things, remembered between runs.
//!
//! They live in eframe's storage with the rest of what the app remembers, not in
//! any job, so opening someone else's job never changes them.

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::help::topics::app;
use crate::ui::defaults::Defaults;
use crate::ui::measure::Measure;
use crate::ui::themes::{self, Choice};
use crate::ui::widgets;

/// What is remembered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Millimetres or inches, everywhere a length, feed or volume is shown.
    pub measure: Measure,
    /// Generate each setup as soon as editing it settles, rather than only
    /// when asked: on unless turned off.
    pub generate: bool,
    /// The window's colours: the desktop's, the design system's, or a
    /// palette file's.
    pub theme: Choice,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            measure: Measure::default(),
            generate: true,
            theme: Choice::default(),
        }
    }
}

impl Settings {
    /// The settings window, while `open`.
    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool) {
        egui::Window::new("Settings")
            .open(open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                let mut d = Defaults::of(&*self, &Settings::default());
                widgets::form(ui, "settings", |ui| {
                    d.row(ui, &app::UNITS, "measure", |ui| {
                        ui.vertical(|ui| {
                            for m in Measure::ALL {
                                ui.radio_value(&mut self.measure, m, m.name());
                            }
                        });
                    });
                    d.row(ui, &app::GENERATE, "generate", |ui| {
                        ui.checkbox(&mut self.generate, "as soon as editing settles");
                    });
                    d.row(ui, &app::THEME, "theme", |ui| self.theme_choice(ui));
                });
                d.apply(self);
                ui.label(widgets::hint_text(
                    "How the app shows lengths and feeds. Jobs and programs stay in millimetres.",
                ));
            });
    }

    /// The Theme setting: the desktop's, the design system's, Omarchy's, and
    /// each palette in the themes folder, with a way to that folder.
    fn theme_choice(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            egui::ComboBox::from_id_salt("theme")
                .selected_text(self.theme.title())
                .height(480.0)
                .show_ui(ui, |ui| themes::pick(ui, &mut self.theme));
            if ui
                .button("Themes folder")
                .on_hover_text(themes::folder().map_or_else(String::new, |f| {
                    format!("{}: palette files, and Omarchy theme folders", f.display())
                }))
                .clicked()
            {
                // Where it cannot be opened, its place is in the hover.
                let _ = themes::open_folder();
            }
        });
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// A choice survives being stored, and storage from before a setting existed
    /// reads as the defaults.
    #[test]
    fn settings_are_remembered_and_old_storage_still_reads() {
        let imperial = Settings {
            measure: Measure::Imperial,
            generate: false,
            theme: Choice::File("tokyo-night".into()),
        };
        let stored = toml::to_string(&imperial).unwrap();
        assert_eq!(toml::from_str::<Settings>(&stored).unwrap(), imperial);
        assert_eq!(toml::from_str::<Settings>("").unwrap(), Settings::default());
    }
}
