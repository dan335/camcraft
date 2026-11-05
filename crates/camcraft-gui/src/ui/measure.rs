//! Millimetres or inches, for what the app shows.
//!
//! The engine, job files and programs are all in millimetres, and stay so. This is
//! only how a length, feed or volume is written on screen, and read back from a
//! field. A value is converted when shown, and written back only when the person
//! changes it, so looking at a job in inches never rounds what is in it.

use camcraft_core::job::{Leave, StockToLeave};
use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::ui::widgets;

use camcraft_geom::MM_PER_INCH;

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

    /// Editors for stock to leave, Fusion's radial (wall) and axial (floor), as
    /// much of it as `leave` says is used, each no lower than `least` has it.
    /// The floor follows the wall while the two are the same, as Fusion's does
    /// until the floor is given its own.
    pub fn stock_to_leave_fields(
        self,
        ui: &mut egui::Ui,
        stock: &mut StockToLeave,
        leave: Leave,
        least: StockToLeave,
    ) {
        let before = *stock;
        if leave == Leave::Both {
            self.wall_to_leave_field(ui, &mut stock.radial, least.radial, "wall ");
        }
        if leave != Leave::None {
            self.floor_to_leave_field(ui, &mut stock.axial, least.axial, "floor ");
        }
        follow_radial(stock, before);
    }

    /// An editor for the stock to leave on the walls, no lower than `least`.
    pub fn wall_to_leave_field(
        self,
        ui: &mut egui::Ui,
        mm: &mut f64,
        least: f64,
        prefix: &str,
    ) -> egui::Response {
        // Beside, the tool cannot shrink to nothing: a hundredth of a millimetre short.
        let min = (least + 0.01).max(-50.0).min(*mm);
        self.field(ui, mm, 0.01, min, 50.0, prefix, self.unit(), 4)
    }

    /// An editor for the stock to leave on the floors, no lower than `least`.
    pub fn floor_to_leave_field(
        self,
        ui: &mut egui::Ui,
        mm: &mut f64,
        least: f64,
        prefix: &str,
    ) -> egui::Response {
        let min = least.max(-50.0).min(*mm);
        self.field(ui, mm, 0.01, min, 50.0, prefix, self.unit(), 4)
    }

    /// An editor for a feed kept in mm/min, shown in this unit.
    pub fn feed_field(self, ui: &mut egui::Ui, mm_per_min: &mut f64, max: f64) -> egui::Response {
        self.field(ui, mm_per_min, 10.0, 0.0, max, "", self.feed_unit(), 1)
    }

    /// An editor for `mm`, shown to `decimals` places at most, and to exactly
    /// that many in inches.
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
        decimals: usize,
    ) -> egui::Response {
        let mut shown = self.show(*mm);
        let mut drag = widgets::number(&mut shown)
            .speed(self.show(speed))
            .range(self.show(min)..=self.show(max))
            .prefix(prefix)
            .suffix(format!(" {unit}"))
            .max_decimals(decimals);
        if self == Self::Imperial {
            drag = drag.min_decimals(decimals);
        }
        let response = ui.add(drag);
        // Only a change is written back, so showing a value never rounds it. A
        // field left after being clicked into reads back the text it showed,
        // which holds only so many places: that is the value as shown, and no
        // change, or looking at a job in inches would round what is in it.
        let as_shown = egui::emath::round_to_decimals(self.show(*mm), decimals);
        if response.changed() && shown.to_bits() != as_shown.to_bits() {
            *mm = self.read(shown);
        }
        response
    }
}

/// Move the floor's stock to leave with the wall's, when the wall's changed from
/// `before` and the floor's was the same as it: Fusion's axial follows radial
/// until given its own.
pub fn follow_radial(stock: &mut StockToLeave, before: StockToLeave) {
    // Exactly the same: a value typed, not one worked out.
    let same = |a: f64, b: f64| a.to_bits() == b.to_bits();
    if !same(stock.radial, before.radial)
        && same(before.axial, before.radial)
        && same(stock.axial, before.axial)
    {
        stock.axial = stock.radial;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::float_cmp)]
    fn the_floor_follows_the_wall_until_given_its_own() {
        let even = StockToLeave {
            radial: 0.5,
            axial: 0.5,
        };
        let mut s = StockToLeave {
            radial: 0.3,
            ..even
        };
        follow_radial(&mut s, even);
        assert_eq!(s.axial, 0.3);
        let apart = StockToLeave {
            radial: 0.5,
            axial: 0.2,
        };
        let mut s = StockToLeave {
            radial: 0.3,
            ..apart
        };
        follow_radial(&mut s, apart);
        assert_eq!(s.axial, 0.2, "a floor given its own stays");
    }

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
    fn feeds_convert() {
        assert_eq!(Measure::Metric.feed_number(1200.0), "1200");
        assert_eq!(Measure::Imperial.feed_number(254.0), "10.0");
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

#[cfg(all(test, feature = "ui-snapshots"))]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod field_tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;

    fn click(h: &mut Harness<'_, f64>, at: egui::Pos2) {
        for pressed in [true, false] {
            h.input_mut().events.push(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            h.run();
        }
    }

    /// Clicking into a length and out again, in inches, leaves it as it was:
    /// the text it re-reads holds only four places, and a hundredth of a
    /// millimetre is not 0.0004 in.
    #[test]
    fn looking_at_a_length_in_inches_does_not_change_it() {
        let mut h = Harness::new_ui_state(
            |ui, mm: &mut f64| {
                Measure::Imperial.length_field(ui, mm, 0.01, 10.0, "");
            },
            0.01,
        );
        h.run();
        click(&mut h, egui::pos2(20.0, 12.0));
        h.hover_at(egui::pos2(400.0, 400.0));
        click(&mut h, egui::pos2(400.0, 400.0));
        assert_eq!(*h.state(), 0.01);
    }

    /// A field of a form bound to its defaults, changed from them, offers ↺,
    /// which puts the default back, and then offers it no more.
    #[test]
    fn a_changed_field_resets_to_its_default() {
        use crate::ui::defaults::Defaults;
        use camcraft_core::job::{Operation, StrategySpec};
        let def = Operation::new("rough", StrategySpec::named("adaptive").unwrap());
        let mut op = def.clone();
        op.passes.stepover = def.passes.stepover + 2.25;
        let stepover = def.passes.stepover;
        let mut h = Harness::new_ui_state(
            move |ui, op: &mut Operation| {
                let mut d = Defaults::of(&*op, &def);
                egui::Grid::new("form").show(ui, |ui| {
                    d.row(
                        ui,
                        &crate::help::topics::passes::STEPOVER,
                        "passes.stepover",
                        |ui| {
                            Measure::Metric.length_field(
                                ui,
                                &mut op.passes.stepover,
                                0.01,
                                100.0,
                                "",
                            );
                        },
                    );
                });
                d.apply(op);
            },
            op,
        );
        h.run();
        h.get_by_role(egui::accesskit::Role::Button).click();
        h.run();
        assert_eq!(h.state().passes.stepover, stepover);
        assert!(h.query_by_role(egui::accesskit::Role::Button).is_none());
    }

    /// Type `text` into the field at the left of the window a key at a time,
    /// a frame each, as a person does, with what the field holds after each
    /// key, and then press Enter.
    fn type_into(h: &mut Harness<'_, f64>, text: &str) -> Vec<f64> {
        h.get_by_role(egui::accesskit::Role::SpinButton).click();
        h.run();
        let mut seen = Vec::new();
        for c in text.chars() {
            // Being typed into, it keeps its role.
            h.get_by_role(egui::accesskit::Role::SpinButton)
                .type_text(&c.to_string());
            h.run();
            seen.push(*h.state());
        }
        h.key_press(egui::Key::Enter);
        h.run();
        seen
    }

    /// ".07" typed into an optimal load in inches is 0.07 in once Enter is
    /// pressed, and nothing before: ".0" on the way is not clamped up to the
    /// field's least and written back, which threw the text being typed away
    /// for the number it had become.
    #[test]
    fn a_number_part_typed_is_not_taken_until_enter() {
        let mut h = Harness::new_ui_state(
            |ui, mm: &mut f64| {
                Measure::Imperial.length_field(ui, mm, 0.01, 100.0, "");
            },
            2.54,
        );
        h.run();
        let seen = type_into(&mut h, ".07");
        assert_eq!(seen, [2.54, 2.54, 2.54], "nothing taken while typing");
        assert!(
            (*h.state() - 0.07 * MM_PER_INCH).abs() < 1e-9,
            "{}",
            h.state()
        );
    }

    /// A wall's stock to leave that may go to nought, typed down from half a
    /// millimetre to 0.07: it never passes through nought on the way, which
    /// unticked "Leave stock" and took the field away mid-number.
    #[test]
    fn a_stock_to_leave_typed_never_passes_through_nought() {
        let mut h = Harness::new_ui_state(
            |ui, mm: &mut f64| {
                Measure::Metric.wall_to_leave_field(ui, mm, -3.0, "");
            },
            0.5,
        );
        h.run();
        let seen = type_into(&mut h, ".07");
        assert!(seen.iter().all(|&v| v == 0.5), "{seen:?}");
        assert_eq!(*h.state(), 0.07);
    }
}
