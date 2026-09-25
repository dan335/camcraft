//! The person's settings: how the app shows things, remembered between runs.
//!
//! They live in eframe's storage with the rest of what the app remembers, not in
//! any job, so opening someone else's job never changes them.

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::measure::Measure;
use crate::widgets;

/// What is remembered.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Millimetres or inches, everywhere a length, feed or volume is shown.
    pub measure: Measure,
}

impl Settings {
    /// The settings window, while `open`.
    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool) {
        egui::Window::new("Settings")
            .open(open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                widgets::form(ui, "settings", |ui| {
                    widgets::field_label(ui, "Units");
                    ui.vertical(|ui| {
                        for m in Measure::ALL {
                            ui.radio_value(&mut self.measure, m, m.name());
                        }
                    });
                    ui.end_row();
                });
                ui.label(widgets::hint_text(
                    "How the app shows lengths and feeds. Jobs and programs stay in millimetres.",
                ));
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
        };
        let stored = toml::to_string(&imperial).unwrap();
        assert_eq!(toml::from_str::<Settings>(&stored).unwrap(), imperial);
        assert_eq!(toml::from_str::<Settings>("").unwrap(), Settings::default());
    }
}
