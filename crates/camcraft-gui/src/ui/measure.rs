//! Millimetres or inches, for what the app shows.
//!
//! The engine, job files and programs are all in millimetres, and stay so. This is
//! only how a length, feed or volume is written on screen, and read back from a
//! field. A value is converted when shown, and written back only when the person
//! changes it, so looking at a job in inches never rounds what is in it.

use eframe::egui;
use serde::{Deserialize, Serialize};

const MM_PER_INCH: f64 = 25.4;

/// How lengths are shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Measure {
    /// Millimetres, mm/min and cm³.
    #[default]
    Metric,
    /// Inches, in/min and in³.
    Imperial,
}

impl Measure {
    /// Both, in the order a chooser lists them.
    pub const ALL: [Self; 2] = [Self::Metric, Self::Imperial];

    /// What a person calls it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Metric => "metric (mm)",
            Self::Imperial => "imperial (inch)",
        }
    }

    /// The length unit's symbol.
    pub fn unit(self) -> &'static str {
        match self {
            Self::Metric => "mm",
            Self::Imperial => "in",
        }
    }

    /// The feed unit's symbol.
    pub fn feed_unit(self) -> &'static str {
        match self {
            Self::Metric => "mm/min",
            Self::Imperial => "in/min",
        }
    }

    /// The surface speed unit's symbol.
    pub fn surface_unit(self) -> &'static str {
        match self {
            Self::Metric => "m/min",
            Self::Imperial => "ft/min",
        }
    }

    /// Surface speed in metres per minute, in the shown unit.
    pub fn show_surface(self, m_per_min: f64) -> f64 {
        m_per_min * 1000.0 / self.scale() / self.surface_per_length()
    }

    /// A shown surface speed back in metres per minute.
    pub fn read_surface(self, shown: f64) -> f64 {
        shown * self.scale() * self.surface_per_length() / 1000.0
    }

    /// Shown lengths per shown surface-speed length: 1000 mm to the metre, 12
    /// inches to the foot.
    fn surface_per_length(self) -> f64 {
        match self {
            Self::Metric => 1000.0,
            Self::Imperial => 12.0,
        }
    }

    /// Millimetres per shown unit.
    fn scale(self) -> f64 {
        match self {
            Self::Metric => 1.0,
            Self::Imperial => MM_PER_INCH,
        }
    }

    /// `mm` in the shown unit.
    pub fn show(self, mm: f64) -> f64 {
        mm / self.scale()
    }

    /// A shown value back in millimetres.
    pub fn read(self, shown: f64) -> f64 {
        shown * self.scale()
    }

    /// Decimals for a length that would take `metric` in millimetres. An inch is
    /// 25.4 mm, so two more keep about the same resolution.
    pub fn decimals(self, metric: usize) -> usize {
        match self {
            Self::Metric => metric,
            Self::Imperial => metric + 2,
        }
    }

    /// A length, without its unit: for lists of coordinates.
    pub fn number(self, mm: f64, metric_decimals: usize) -> String {
        format!("{:.*}", self.decimals(metric_decimals), self.show(mm))
    }

    /// A length with its unit, as `12.35 mm` or `0.4862 in`.
    pub fn length(self, mm: f64, metric_decimals: usize) -> String {
        format!("{} {}", self.number(mm, metric_decimals), self.unit())
    }

    /// A length with its unit and no trailing zeros, for values a person typed,
    /// such as a tolerance: `0.01 mm`, `0.0003937 in`. Millimetres keep up to four
    /// decimals; inches, which seldom come out round, four significant figures.
    pub fn length_trimmed(self, mm: f64) -> String {
        let shown = self.show(mm);
        let decimals = match self {
            Self::Metric => 4,
            Self::Imperial => {
                // A small whole number of decimals; the float is a log of a length.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let places = (3.0 - shown.abs().log10().floor()).clamp(0.0, 8.0) as usize;
                places
            }
        };
        let text = format!("{shown:.decimals$}");
        let text = if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.')
        } else {
            &text
        };
        format!("{text} {}", self.unit())
    }

    /// A feed's number, without its unit, as a program's `F` word would read.
    pub fn feed_number(self, mm_per_min: f64) -> String {
        match self {
            Self::Metric => format!("{mm_per_min:.0}"),
            Self::Imperial => format!("{:.1}", self.show(mm_per_min)),
        }
    }

    /// An area, from square millimetres.
    pub fn area(self, mm2: f64) -> String {
        match self {
            Self::Metric => format!("{mm2:.1} mm²"),
            Self::Imperial => format!("{:.3} in²", mm2 / MM_PER_INCH.powi(2)),
        }
    }

    /// A volume of material, from cubic millimetres.
    pub fn volume(self, mm3: f64) -> String {
        match self {
            Self::Metric => format!("{:.1} cm³", mm3 / 1000.0),
            Self::Imperial => format!("{:.2} in³", mm3 / MM_PER_INCH.powi(3)),
        }
    }

    /// An editor for a length kept in millimetres, shown in this unit. `speed` and
    /// `max` are in millimetres.
    pub fn length_field(
        self,
        ui: &mut egui::Ui,
        mm: &mut f64,
        speed: f64,
        max: f64,
        prefix: &str,
    ) -> egui::Response {
        self.field(ui, mm, speed, 0.0, max, prefix, self.unit(), 4)
    }

    /// As [`length_field`](Self::length_field), for a length that may be negative:
    /// an offset or a position.
    pub fn signed_length_field(
        self,
        ui: &mut egui::Ui,
        mm: &mut f64,
        speed: f64,
        limit: f64,
        prefix: &str,
    ) -> egui::Response {
        self.field(ui, mm, speed, -limit, limit, prefix, self.unit(), 4)
    }

    /// An editor for a feed kept in mm/min, shown in this unit.
    pub fn feed_field(self, ui: &mut egui::Ui, mm_per_min: &mut f64, max: f64) -> egui::Response {
        self.field(ui, mm_per_min, 10.0, 0.0, max, "", self.feed_unit(), 1)
    }

    #[allow(clippy::too_many_arguments)]
    fn field(
        self,
        ui: &mut egui::Ui,
        mm: &mut f64,
        speed: f64,
        min: f64,
        max: f64,
        prefix: &str,
        unit: &str,
        imperial_decimals: usize,
    ) -> egui::Response {
        let mut shown = self.show(*mm);
        let mut drag = egui::DragValue::new(&mut shown)
            .speed(self.show(speed))
            .range(self.show(min)..=self.show(max))
            .prefix(prefix)
            .suffix(format!(" {unit}"));
        // Metric keeps egui's own choice, as before there was a choice.
        if self == Self::Imperial {
            drag = drag
                .min_decimals(imperial_decimals)
                .max_decimals(imperial_decimals);
        }
        let response = ui.add(drag);
        // Only a change is written back, so showing a value never rounds it.
        if response.changed() {
            *mm = self.read(shown);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_read_in_either_unit() {
        assert_eq!(Measure::Metric.length(12.345, 2), "12.35 mm");
        assert_eq!(Measure::Imperial.length(25.4, 2), "1.0000 in");
        assert_eq!(Measure::Imperial.number(12.7, 1), "0.500");
    }

    #[test]
    fn a_typed_length_keeps_no_trailing_zeros() {
        assert_eq!(Measure::Metric.length_trimmed(0.01), "0.01 mm");
        assert_eq!(Measure::Metric.length_trimmed(3.0), "3 mm");
        assert_eq!(Measure::Imperial.length_trimmed(0.0254), "0.001 in");
        assert_eq!(Measure::Imperial.length_trimmed(0.01), "0.0003937 in");
        assert_eq!(Measure::Imperial.length_trimmed(6.0), "0.2362 in");
        assert_eq!(Measure::Imperial.length_trimmed(254.0), "10 in");
        assert_eq!(Measure::Imperial.length_trimmed(0.0), "0 in");
    }

    #[test]
    fn feeds_and_volumes_convert() {
        assert_eq!(Measure::Metric.feed_number(1200.0), "1200");
        assert_eq!(Measure::Imperial.feed_number(254.0), "10.0");
        assert_eq!(Measure::Metric.volume(12_300.0), "12.3 cm³");
        assert_eq!(Measure::Imperial.volume(MM_PER_INCH.powi(3)), "1.00 in³");
    }

    #[test]
    fn surface_speeds_convert() {
        assert!((Measure::Metric.show_surface(100.0) - 100.0).abs() < 1e-9);
        assert!((Measure::Imperial.show_surface(0.3048) - 1.0).abs() < 1e-9);
        for m in Measure::ALL {
            assert!((m.read_surface(m.show_surface(319.2)) - 319.2).abs() < 1e-9);
        }
    }

    #[test]
    fn shown_values_read_back_to_millimetres() {
        for m in Measure::ALL {
            let mm = 6.35;
            assert!((m.read(m.show(mm)) - mm).abs() < 1e-12);
        }
    }
}
