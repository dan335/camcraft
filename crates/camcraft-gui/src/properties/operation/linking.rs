//! An operation's Linking tab: how it moves between cuts, and into and out of
//! them.

use camcraft_core::job::{Entry, Leads, Links, Operation, Retraction};
use eframe::egui;

use crate::help::topics::linking as link;
use crate::ui::measure::Measure;
use crate::ui::widgets::{self, form};

/// How the operation moves between cuts. A value left out is the strategy's own,
/// as Fusion's, and says what that comes to for the tool chosen.
pub(super) fn linking(ui: &mut egui::Ui, m: Measure, op: &mut Operation, diameter: Option<f64>) {
    let uses = op.strategy.uses();
    // Where a value is left out, the strategy's own for the tool, or for a
    // 6 mm one before a tool is chosen.
    let d = diameter.unwrap_or(6.0);
    let of_tool = |part: f64, what: &str| match diameter {
        Some(d) => format!("{what}: {}", m.length(part * d, 2)),
        None => what.to_string(),
    };
    form(ui, "linking", |ui| {
        widgets::row(ui, &link::RETRACT_FEED, |ui| {
            m.feed_field(ui, &mut op.linking.retract_feed, 50_000.0);
        });
        if uses.stay_down {
            widgets::row(ui, &link::RETRACTION, |ui| {
                widgets::select(
                    ui,
                    "retraction",
                    &mut op.linking.retraction,
                    &[Retraction::Minimum, Retraction::Full],
                    |r| match r {
                        Retraction::Minimum => "minimum, clear of the material",
                        Retraction::Full => "full, to the retract height",
                    },
                );
            });
        }
        widgets::row(ui, &link::SAFE_DISTANCE, |ui| {
            m.length_field(ui, &mut op.linking.safe_distance, 0.1, 1_000.0, "");
        });
        if uses.stay_down {
            // Finishing that follows the part stays down over it; the rest at
            // the level, a multiple of the tool.
            let absent = match uses.links {
                Links::OverPart => format!("up to {}", m.length(uses.links.stay_down(d), 0)),
                Links::Level | Links::Own => {
                    let k = uses.links.stay_down(1.0);
                    of_tool(k, &format!("up to {k} × the tool"))
                }
            };
            let start = op.stay_down(d);
            widgets::row(ui, &link::STAY_DOWN, |ui| {
                widgets::optional(ui, &mut op.linking.stay_down, start, &absent, |ui, v| {
                    m.length_field(ui, v, 0.5, 10_000.0, "up to ");
                });
            });
        }
        if uses.lead_radius {
            let absent = of_tool(0.1, "a tenth of the tool");
            let start = op.lead_radius(d);
            widgets::row(ui, &link::LEAD_RADIUS, |ui| {
                widgets::optional(ui, &mut op.linking.lead_radius, start, &absent, |ui, v| {
                    m.length_field(ui, v, 0.05, 100.0, "");
                });
            });
        }
        if uses.ramp {
            widgets::row(ui, &link::RAMP_ANGLE, |ui| {
                ui.add(
                    egui::DragValue::new(&mut op.linking.ramp_angle)
                        .speed(0.1)
                        .range(0.1..=45.0)
                        .suffix("°"),
                );
            });
            let absent = format!(
                "60% of cutting: {} {}",
                m.feed_number(op.ramp_feed()),
                m.feed_unit()
            );
            let start = op.ramp_feed();
            widgets::row(ui, &link::RAMP_FEED, |ui| {
                widgets::optional(ui, &mut op.linking.ramp_feed, start, &absent, |ui, v| {
                    m.feed_field(ui, v, 50_000.0);
                });
            });
            let stepdown = op.stepdown();
            let absent = match (stepdown, diameter) {
                (Some(d), _) => format!("the stepdown: {}", m.length(d, 2)),
                (None, _) => of_tool(1.0, "the tool's diameter"),
            };
            let start = op.ramp_stepdown(d);
            widgets::row(ui, &link::MOST_PER_LAP, |ui| {
                widgets::optional(
                    ui,
                    &mut op.linking.ramp_stepdown,
                    start,
                    &absent,
                    |ui, v| {
                        m.length_field(ui, v, 0.1, 1000.0, "");
                    },
                );
            });
        }
        if !uses.entries.is_empty() {
            let mut entry = op.entry().unwrap_or_default();
            widgets::row(ui, &link::ENTRY, |ui| {
                widgets::select(ui, "entry", &mut entry, uses.entries, |e| match e {
                    Entry::Helix => "helix beside the lead",
                    Entry::Plunge => "plunge beside the lead",
                    Entry::Ramp => "ramp along the ring",
                });
            });
            // The default is written only when changed.
            op.linking.entry = (entry != Entry::default()).then_some(entry);
            if entry == Entry::Helix {
                let absent = of_tool(0.9, "0.9 × the tool");
                let start = op.helix_diameter(d);
                widgets::row(ui, &link::HELIX_DIAMETER, |ui| {
                    widgets::optional(
                        ui,
                        &mut op.linking.helix_diameter,
                        start,
                        &absent,
                        |ui, v| {
                            m.length_field(ui, v, 0.1, 1000.0, "");
                        },
                    );
                });
            }
        }
        if uses.leads {
            let mut on = op.linking.leads.is_some();
            widgets::row(ui, &link::LEADS, |ui| {
                ui.checkbox(&mut on, "on and off each ring");
            });
            match (on, op.linking.leads.is_some()) {
                (true, false) => op.linking.leads = Some(Leads::default()),
                (false, true) => op.linking.leads = None,
                _ => {}
            }
            if let Some(l) = &mut op.linking.leads {
                let start = 0.1 * diameter.unwrap_or(6.0);
                let tenth = of_tool(0.1, "a tenth of the tool");
                for (topic, v) in [
                    (&link::LEAD_VERTICAL, &mut l.vertical),
                    (&link::LEAD_STRAIGHT, &mut l.straight),
                    (&link::LEAD_ARC, &mut l.radius),
                ] {
                    widgets::row(ui, topic, |ui| {
                        widgets::optional(ui, v, start, &tenth, |ui, v| {
                            m.length_field(ui, v, 0.05, 1000.0, "");
                        });
                    });
                }
                widgets::row(ui, &link::LEAD_SWEEP, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut l.sweep)
                            .speed(1.0)
                            .range(1.0..=180.0)
                            .suffix("°"),
                    );
                });
            }
        }
        if uses.reorder {
            widgets::row(ui, &link::REORDER, |ui| {
                ui.checkbox(&mut op.linking.reorder, "to shorten travel");
            });
            widgets::row(ui, &link::REVERSE, |ui| {
                ui.checkbox(
                    &mut op.linking.allow_reverse,
                    "while reordering (changes climb)",
                );
            });
        }
    });
}
