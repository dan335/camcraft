//! An operation's Linking tab: how it moves between cuts, and into and out of
//! them.

use camcraft_core::job::{Entry, HelixSize, Leads, Operation, Reach, Retraction, StrategySpec};

use crate::ui::defaults::Defaults;
use eframe::egui;

use crate::help::topics::linking as link;
use crate::ui::measure::Measure;
use crate::ui::widgets::{self, form};

/// How the operation moves between cuts. A value left out is the strategy's own,
/// as Fusion's, and says what that comes to for the tool chosen, whose corners
/// are of radius `corner`.
pub(super) fn linking(
    ui: &mut egui::Ui,
    m: Measure,
    op: &mut Operation,
    d: &mut Defaults<Operation>,
    diameter: Option<f64>,
    corner: f64,
) {
    let uses = op.strategy.uses();
    // Where a value is left out, the strategy's own for the tool, or for a
    // 6 mm one before a tool is chosen.
    let across = diameter.unwrap_or(6.0);
    let of_tool = |part: f64, what: &str| match diameter {
        Some(d) => format!("{what}: {}", m.length(part * d, 2)),
        None => what.to_string(),
    };
    form(ui, "linking", |ui| {
        d.row(ui, &link::RETRACT_FEED, "linking.retract_feed", |ui| {
            m.feed_field(ui, &mut op.linking.retract_feed, 50_000.0);
        });
        if uses.stay_down {
            d.row(ui, &link::RETRACTION, "linking.retraction", |ui| {
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
        d.row(ui, &link::SAFE_DISTANCE, "linking.safe_distance", |ui| {
            m.length_field(ui, &mut op.linking.safe_distance, 0.1, 1_000.0, "");
        });
        if uses.stay_down {
            // Fusion's for the strategy: a length, or a multiple of the tool.
            let absent = match uses.reach {
                Reach::Mm(mm) => format!("up to {}", m.length(f64::from(mm), 0)),
                Reach::Tools(k) => of_tool(f64::from(k), &format!("up to {k} × the tool")),
            };
            let start = op.stay_down(across);
            d.row(ui, &link::STAY_DOWN, "linking.stay_down", |ui| {
                widgets::optional(ui, &mut op.linking.stay_down, start, &absent, |ui, v| {
                    m.length_field(ui, v, 0.5, 10_000.0, "up to ");
                });
            });
        }
        if let StrategySpec::Adaptive {
            stay_down_level,
            stay_down_clearance,
            lift_height,
            ..
        } = &mut op.strategy
        {
            d.row(ui, &link::STAY_DOWN_LEVEL, "stay_down_level", |ui| {
                ui.add(
                    egui::Slider::new(stay_down_level, 0.0..=100.0)
                        .step_by(10.0)
                        .suffix("%")
                        .custom_formatter(|v, _| match v {
                            v if v <= 0.0 => "least".into(),
                            v if v >= 100.0 => "most".into(),
                            v => format!("{v:.0}"),
                        }),
                );
            });
            d.row(
                ui,
                &link::STAY_DOWN_CLEARANCE,
                "stay_down_clearance",
                |ui| {
                    m.length_field(ui, stay_down_clearance, 0.05, 100.0, "");
                },
            );
            d.row(ui, &link::LIFT_HEIGHT, "lift_height", |ui| {
                m.length_field(ui, lift_height, 0.05, 100.0, "");
            });
        }
        if uses.lead_radius {
            let absent = of_tool(0.1, "a tenth of the tool");
            let start = op.lead_radius(across);
            d.row(ui, &link::LEAD_RADIUS, "linking.lead_radius", |ui| {
                widgets::optional(ui, &mut op.linking.lead_radius, start, &absent, |ui, v| {
                    m.length_field(ui, v, 0.05, 100.0, "");
                });
            });
        }
        if uses.ramp {
            d.row(ui, &link::RAMP_ANGLE, "linking.ramp_angle", |ui| {
                ui.add(
                    widgets::number(&mut op.linking.ramp_angle)
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
            d.row(ui, &link::RAMP_FEED, "linking.ramp_feed", |ui| {
                widgets::optional(ui, &mut op.linking.ramp_feed, start, &absent, |ui, v| {
                    m.feed_field(ui, v, 50_000.0);
                });
            });
            let stepdown = op.stepdown();
            let absent = match (stepdown, diameter) {
                (Some(d), _) => format!("the stepdown: {}", m.length(d, 2)),
                (None, _) => of_tool(1.0, "the tool's diameter"),
            };
            let start = op.ramp_stepdown(across);
            d.row(ui, &link::MOST_PER_LAP, "linking.ramp_stepdown", |ui| {
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
            d.row(ui, &link::ENTRY, "linking.entry", |ui| {
                // A profile goes down beside its lead; a pocket where its
                // first ring starts.
                widgets::select(ui, "entry", &mut entry, uses.entries, |e| {
                    match (e, uses.leads) {
                        (Entry::Helix, true) => "helix beside the lead",
                        (Entry::Plunge, true) => "plunge beside the lead",
                        (Entry::Helix, false) => "helix",
                        (Entry::Plunge, false) => "plunge",
                        (Entry::Ramp, _) => "ramp along the ring",
                    }
                });
            });
            // The default is written only when changed.
            op.linking.entry = (entry != Entry::default()).then_some(entry);
            if entry == Entry::Helix {
                let absent = match (uses.helix, diameter) {
                    (HelixSize::Tool, _) => of_tool(0.9, "0.9 × the tool"),
                    (HelixSize::Flat, Some(_)) => format!(
                        "0.95 × the tool less its corners: {}",
                        m.length(op.helix_diameter(across, corner), 2)
                    ),
                    (HelixSize::Flat, None) => "0.95 × the tool less its corners".to_string(),
                };
                let start = op.helix_diameter(across, corner);
                d.row(ui, &link::HELIX_DIAMETER, "linking.helix_diameter", |ui| {
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
        // A pocket's leads take the tool onto its finishing passes and off.
        let pocket_leads = uses.finishing && op.passes.finishing.is_some();
        if uses.leads || pocket_leads {
            let mut on = op.linking.leads.is_some();
            d.group_row(ui, &link::LEADS, "linking.leads", |ui| {
                let what = if uses.leads {
                    "on and off each ring"
                } else {
                    "onto the finishing passes and off"
                };
                ui.checkbox(&mut on, what);
            });
            match (on, op.linking.leads.is_some()) {
                (true, false) => op.linking.leads = Some(Leads::default()),
                (false, true) => op.linking.leads = None,
                _ => {}
            }
            if let Some(l) = &mut op.linking.leads {
                let start = 0.1 * diameter.unwrap_or(6.0);
                let tenth = of_tool(0.1, "a tenth of the tool");
                // A pocket's finishing passes are entered level, beside the
                // wall: their leads have no vertical arc.
                let vertical = (uses.leads).then_some((
                    &link::LEAD_VERTICAL,
                    "linking.leads.vertical",
                    &mut l.vertical,
                ));
                for (topic, path, v) in vertical.into_iter().chain([
                    (
                        &link::LEAD_STRAIGHT,
                        "linking.leads.straight",
                        &mut l.straight,
                    ),
                    (&link::LEAD_ARC, "linking.leads.radius", &mut l.radius),
                ]) {
                    d.row(ui, topic, path, |ui| {
                        widgets::optional(ui, v, start, &tenth, |ui, v| {
                            m.length_field(ui, v, 0.05, 1000.0, "");
                        });
                    });
                }
                d.row(ui, &link::LEAD_SWEEP, "linking.leads.sweep", |ui| {
                    ui.add(
                        widgets::number(&mut l.sweep)
                            .speed(1.0)
                            .range(1.0..=180.0)
                            .suffix("°"),
                    );
                });
            }
        }
        if uses.reorder {
            d.row(ui, &link::REORDER, "linking.reorder", |ui| {
                ui.checkbox(&mut op.linking.reorder, "to shorten travel");
            });
            d.row(ui, &link::REVERSE, "linking.allow_reverse", |ui| {
                ui.checkbox(
                    &mut op.linking.allow_reverse,
                    "while reordering (changes climb)",
                );
            });
        }
    });
}
