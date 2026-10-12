//! An operation's Passes tab: how its strategy lays its passes out, how far
//! apart they are, the levels it cuts in, the stock it leaves and its tabs.

use camcraft_core::job::{
    ContourOrder, CutDirection, Depths, FaceDirection, Leave, Operation, PassOrder,
    RadialDirection, StockToLeave, StrategySpec, TabShape, Tabs,
};
use eframe::egui;

use crate::browser::strategy_label;
use crate::help::topics::passes as pass;
use crate::help::{Help, Topic};
use crate::ui::measure::{Measure, follow_radial};
use crate::ui::theme;
use crate::ui::widgets::{self, form, hint};

pub(super) fn passes(ui: &mut egui::Ui, m: Measure, op: &mut Operation, least: StockToLeave) {
    let uses = op.strategy.uses();
    form(ui, "passes", |ui| {
        match &mut op.strategy {
            StrategySpec::Parallel { angle } => {
                widgets::row(ui, &pass::PASS_ANGLE, |ui| angle_field(ui, angle));
            }
            StrategySpec::Scallop {
                direction,
                order,
                slope_from,
                slope_to,
            } => {
                both_ways_direction(ui, direction);
                pass_order(ui, &pass::SCALLOP_ORDER, order);
                slopes(ui, slope_from, slope_to);
            }
            StrategySpec::Contour {
                direction,
                order,
                slope_from,
                slope_to,
            } => {
                both_ways_direction(ui, direction);
                widgets::row(ui, &pass::CONTOUR_ORDER, |ui| {
                    widgets::segmented(
                        ui,
                        order,
                        &[
                            (ContourOrder::ByDepth, "by depth"),
                            (ContourOrder::ByArea, "by area"),
                        ],
                    );
                });
                slopes(ui, slope_from, slope_to);
            }
            StrategySpec::Flat { direction, order } => {
                climb_direction(ui, &pass::DIRECTION_2D, direction);
                pass_order(ui, &pass::SCALLOP_ORDER, order);
            }
            StrategySpec::Spiral {
                centre,
                inner_radius,
                outer_radius,
                direction,
                order,
                slope_from,
                slope_to,
            } => {
                centred(ui, m, centre, inner_radius, outer_radius);
                climb_direction(ui, &pass::DIRECTION_SPIRAL, direction);
                pass_order(ui, &pass::SPIRAL_ORDER, order);
                slopes(ui, slope_from, slope_to);
            }
            StrategySpec::Radial {
                centre,
                inner_radius,
                outer_radius,
                angle_from,
                angle_to,
                direction,
                slope_from,
                slope_to,
            } => {
                centred(ui, m, centre, inner_radius, outer_radius);
                widgets::row(ui, &pass::RADIAL_ANGLES, |ui| {
                    ui.add(
                        egui::DragValue::new(angle_from)
                            .speed(1.0)
                            .range(-360.0..=360.0)
                            .prefix("from ")
                            .suffix("°"),
                    );
                    ui.add(
                        egui::DragValue::new(angle_to)
                            .speed(1.0)
                            .range(-360.0..=720.0)
                            .prefix("to ")
                            .suffix("°"),
                    );
                });
                // A range the job would refuse is put right as it is set.
                if *angle_to <= *angle_from {
                    *angle_to = *angle_from + 1.0;
                }
                if *angle_to > *angle_from + 360.0 {
                    *angle_to = *angle_from + 360.0;
                }
                widgets::row(ui, &pass::RADIAL_DIRECTION, |ui| {
                    widgets::segmented(
                        ui,
                        direction,
                        &[
                            (RadialDirection::BothWays, "both ways"),
                            (RadialDirection::Outward, "outward"),
                            (RadialDirection::Inward, "inward"),
                        ],
                    );
                });
                slopes(ui, slope_from, slope_to);
            }
            StrategySpec::MorphedSpiral {
                centre,
                direction,
                order,
                slope_from,
                slope_to,
            } => {
                widgets::row(ui, &pass::MORPH_CENTRE, |ui| {
                    centre_field(ui, m, centre, "the point furthest inside");
                });
                climb_direction(ui, &pass::DIRECTION_SPIRAL, direction);
                pass_order(ui, &pass::SPIRAL_ORDER, order);
                slopes(ui, slope_from, slope_to);
            }
            StrategySpec::PocketClearing {
                min_ring_length,
                conventional,
                flat_areas,
            } => {
                climb_or_conventional(ui, &pass::DIRECTION_2D, conventional);
                widgets::row(ui, &pass::FLAT_AREAS, |ui| {
                    ui.checkbox(flat_areas, "a level on each flat face");
                });
                widgets::row(ui, &pass::SHORTEST_RING, |ui| {
                    m.length_field(ui, min_ring_length, 0.1, 1000.0, "");
                });
            }
            StrategySpec::Adaptive {
                direction,
                flat_areas,
            } => {
                widgets::row(ui, &pass::DIRECTION_ADAPTIVE, |ui| {
                    widgets::segmented(
                        ui,
                        direction,
                        &[
                            (CutDirection::Climb, "climb"),
                            (CutDirection::Conventional, "conventional"),
                            (CutDirection::BothWays, "both ways"),
                        ],
                    );
                });
                widgets::row(ui, &pass::FLAT_AREAS, |ui| {
                    ui.checkbox(flat_areas, "a level on each flat face");
                });
            }
            StrategySpec::Profile { conventional, .. } | StrategySpec::Pocket { conventional } => {
                climb_or_conventional(ui, &pass::DIRECTION_2D, conventional);
            }
            StrategySpec::Face {
                angle,
                direction,
                pass_extension,
                ..
            } => {
                widgets::row(ui, &pass::PASS_ANGLE, |ui| angle_field(ui, angle));
                widgets::row(ui, &pass::DIRECTION_FACE, |ui| {
                    widgets::segmented(
                        ui,
                        direction,
                        &[
                            (FaceDirection::BothWays, "both ways"),
                            (FaceDirection::Climb, "climb"),
                            (FaceDirection::Conventional, "conventional"),
                        ],
                    );
                });
                widgets::row(ui, &pass::PASS_EXTENSION, |ui| {
                    m.length_field(ui, pass_extension, 0.1, 1000.0, "");
                });
            }
            StrategySpec::Drill { peck, dwell } => {
                widgets::row(ui, &pass::PECK, |ui| {
                    widgets::optional(ui, peck, 2.0, "all the way at once", |ui, p| {
                        m.length_field(ui, p, 0.05, 100.0, "");
                    });
                });
                widgets::row(ui, &pass::DWELL, |ui| {
                    widgets::optional(ui, dwell, 0.5, "none", |ui, d| {
                        ui.add(
                            egui::DragValue::new(d)
                                .speed(0.05)
                                .range(0.0..=60.0)
                                .suffix(" s at the bottom"),
                        );
                    });
                });
            }
        }
        if uses.stepover {
            // Adaptive clearing's stepover is the width of cut it holds.
            if uses.load {
                widgets::row(ui, &pass::OPTIMAL_LOAD, |ui| {
                    m.length_field(ui, &mut op.passes.stepover, 0.01, 100.0, "");
                });
            } else {
                widgets::row(ui, &pass::STEPOVER, |ui| {
                    m.length_field(ui, &mut op.passes.stepover, 0.05, 100.0, "");
                });
            }
        }
        widgets::row(ui, &pass::DEEPEST_STEP, |ui| {
            m.length_field(ui, &mut op.passes.max_cut_step, 0.05, 50.0, "");
        });
    });
    if uses.depths != Depths::None {
        ui.add_space(theme::SPACE_4);
        ui.label(widgets::section_label("Multiple depths"));
        ui.add_space(theme::SPACE_2);
        multiple_depths(ui, m, op);
    }
    if uses.stock_to_leave != Leave::None {
        ui.add_space(theme::SPACE_4);
        ui.label(widgets::section_label("Stock to leave"));
        ui.add_space(theme::SPACE_2);
        stock_to_leave(ui, m, op, least);
    }
    if uses.tabs {
        ui.add_space(theme::SPACE_4);
        ui.label(widgets::section_label("Tabs"));
        ui.add_space(theme::SPACE_2);
        tabs(ui, m, &mut op.passes.tabs);
    }
}

/// The direction passes run, in degrees from X.
fn angle_field(ui: &mut egui::Ui, angle: &mut f64) {
    ui.add(
        egui::DragValue::new(angle)
            .speed(1.0)
            .range(-360.0..=360.0)
            .suffix("°"),
    );
}

/// Multiple depths, as Fusion's Passes tab has them: ticked to cut in levels,
/// then the deepest each may be. Unticked is the bottom in one pass. A strategy
/// that always cuts in levels, as roughing does, cannot be unticked.
fn multiple_depths(ui: &mut egui::Ui, m: Measure, op: &mut Operation) {
    let required = op.strategy.uses().depths == Depths::Required;
    let mut on = op.passes.stepdown.is_some();
    let r = ui
        .add_enabled(
            !(required && on),
            egui::Checkbox::new(&mut on, "Step down in levels"),
        )
        .help(&pass::MULTIPLE_DEPTHS)
        .on_disabled_hover_text(format!(
            "{} always cuts in levels",
            strategy_label(&op.strategy)
        ));
    if r.changed() {
        op.passes.stepdown = on.then(|| op.strategy.default_stepdown().unwrap_or(2.0));
    }
    let Some(stepdown) = &mut op.passes.stepdown else {
        return;
    };
    ui.add_space(theme::SPACE_2);
    form(ui, "multiple-depths", |ui| {
        widgets::row(ui, &pass::STEPDOWN, |ui| {
            m.length_field(ui, stepdown, 0.05, 100.0, "");
        });
    });
}

/// Whether `op` leaves any stock: of what its strategy cuts with, so a wall's
/// that it has no walls to leave on is none.
fn leaves_stock(op: &Operation) -> bool {
    op.stock_to_leave() != StockToLeave::default()
}

/// The stock to leave ticking "Leave stock" starts `op` at: roughing's half a
/// millimetre on what its strategy leaves stock on, and nothing on the rest.
fn stock_to_start(op: &Operation) -> StockToLeave {
    let half = 0.5;
    match op.strategy.uses().stock_to_leave {
        Leave::Both => StockToLeave {
            radial: half,
            axial: half,
        },
        Leave::Axial => StockToLeave {
            radial: 0.0,
            axial: half,
        },
        Leave::None => StockToLeave::default(),
    }
}

/// Stock to leave, as Fusion's Passes tab has it: ticked to leave any, then how
/// much on the walls and floors, as far as the strategy leaves any on either.
/// Unticked is none, and ticked starts at roughing's half a millimetre; set back
/// to none, it unticks.
fn stock_to_leave(ui: &mut egui::Ui, m: Measure, op: &mut Operation, least: StockToLeave) {
    let leave = op.strategy.uses().stock_to_leave;
    let mut on = leaves_stock(op);
    if ui
        .checkbox(&mut on, "Leave stock for a finishing pass")
        .help(&pass::STOCK_TO_LEAVE)
        .changed()
    {
        op.passes.stock_to_leave = if on {
            stock_to_start(op)
        } else {
            StockToLeave::default()
        };
    }
    if !on {
        return;
    }
    ui.add_space(theme::SPACE_2);
    let stock = &mut op.passes.stock_to_leave;
    let before = *stock;
    form(ui, "stock-to-leave", |ui| {
        if leave == Leave::Both {
            widgets::row(ui, &pass::WALLS, |ui| {
                m.wall_to_leave_field(ui, &mut stock.radial, least.radial, "");
            });
        }
        widgets::row(ui, &pass::FLOORS, |ui| {
            m.floor_to_leave_field(ui, &mut stock.axial, least.axial, "");
        });
    });
    follow_radial(stock, before);
}

/// Tabs left across the deepest cut to hold what it frees, and their size.
fn tabs(ui: &mut egui::Ui, m: Measure, tabs: &mut Option<Tabs>) {
    let mut on = tabs.is_some();
    ui.checkbox(&mut on, "Leave tabs to hold the part")
        .help(&pass::TABS);
    match (on, tabs.is_some()) {
        (true, false) => *tabs = Some(Tabs::default()),
        (false, true) => *tabs = None,
        _ => {}
    }
    let Some(t) = tabs else {
        return;
    };
    ui.add_space(theme::SPACE_2);
    form(ui, "tabs", |ui| {
        widgets::row(ui, &pass::TAB_SHAPE, |ui| {
            widgets::select(
                ui,
                "tab-shape",
                &mut t.shape,
                &[TabShape::Rectangular, TabShape::Triangular],
                |s| match s {
                    TabShape::Rectangular => "rectangular",
                    TabShape::Triangular => "triangular: ramped, no plunge",
                },
            );
        });
        widgets::row(ui, &pass::TAB_WIDTH, |ui| {
            m.length_field(ui, &mut t.width, 0.1, 1000.0, "");
        });
        widgets::row(ui, &pass::TAB_HEIGHT, |ui| {
            m.length_field(ui, &mut t.height, 0.1, 1000.0, "");
        });
        widgets::row(ui, &pass::TAB_SPACING, |ui| {
            m.length_field(ui, &mut t.spacing, 1.0, 100_000.0, "about ");
        });
    });
    hint(
        ui,
        "Tabs go evenly round each loop cut at the deepest level, and every cut \
         over one rides over its top.",
    );
}

/// Which way a finishing pass cuts, both ways among them.
fn both_ways_direction(ui: &mut egui::Ui, direction: &mut CutDirection) {
    widgets::row(ui, &pass::DIRECTION_SCALLOP, |ui| {
        widgets::segmented(
            ui,
            direction,
            &[
                (CutDirection::BothWays, "both ways"),
                (CutDirection::Climb, "climb"),
                (CutDirection::Conventional, "conventional"),
            ],
        );
    });
}

/// Whether a pass cuts conventional rather than climb.
fn climb_or_conventional(ui: &mut egui::Ui, topic: &Topic, conventional: &mut bool) {
    widgets::row(ui, topic, |ui| {
        widgets::segmented(
            ui,
            conventional,
            &[(false, "climb"), (true, "conventional")],
        );
    });
}

/// Which way a pass cuts, climb or conventional: both ways, which has no
/// meaning for it, reads as climb.
fn climb_direction(ui: &mut egui::Ui, topic: &Topic, direction: &mut CutDirection) {
    let was = *direction == CutDirection::Conventional;
    let mut conventional = was;
    climb_or_conventional(ui, topic, &mut conventional);
    // Left as it was unless changed, so a job's both ways is kept.
    if conventional != was {
        *direction = if conventional {
            CutDirection::Conventional
        } else {
            CutDirection::Climb
        };
    }
}

/// Outside in or inside out.
fn pass_order(ui: &mut egui::Ui, topic: &Topic, order: &mut PassOrder) {
    widgets::row(ui, topic, |ui| {
        widgets::segmented(
            ui,
            order,
            &[
                (PassOrder::OutsideIn, "outside in"),
                (PassOrder::InsideOut, "inside out"),
            ],
        );
    });
}

/// The range of slopes a finishing pass keeps to, put right as it is set if the
/// job would refuse it.
fn slopes(ui: &mut egui::Ui, slope_from: &mut f64, slope_to: &mut f64) {
    widgets::row(ui, &pass::SLOPES, |ui| {
        ui.add(
            egui::DragValue::new(slope_from)
                .speed(1.0)
                .range(0.0..=89.0)
                .prefix("from ")
                .suffix("°"),
        );
        ui.add(
            egui::DragValue::new(slope_to)
                .speed(1.0)
                .range(1.0..=90.0)
                .prefix("to ")
                .suffix("°"),
        );
    });
    if *slope_from >= *slope_to {
        *slope_to = (*slope_from + 1.0).min(90.0);
        *slope_from = slope_from.min(*slope_to - 1.0);
    }
}

/// A pattern's centre, given or `absent`, as X and Y from the work zero.
fn centre_field(ui: &mut egui::Ui, m: Measure, centre: &mut Option<[f64; 2]>, absent: &str) {
    widgets::optional(ui, centre, [0.0, 0.0], absent, |ui, c| {
        let [x, y] = c;
        ui.label("X");
        m.signed_length_field(ui, x, 0.1, 10_000.0, "");
        ui.label("Y");
        m.signed_length_field(ui, y, 0.1, 10_000.0, "");
    });
}

/// A pattern about a centre's centre and radii, the inner kept inside the outer.
fn centred(
    ui: &mut egui::Ui,
    m: Measure,
    centre: &mut Option<[f64; 2]>,
    inner: &mut f64,
    outer: &mut f64,
) {
    widgets::row(ui, &pass::CENTRE, |ui| {
        centre_field(ui, m, centre, "the boundary's middle");
    });
    widgets::row(ui, &pass::INNER_RADIUS, |ui| {
        m.length_field(ui, inner, 0.1, 10_000.0, "");
    });
    // Zero is none given.
    let mut given = (*outer > 0.0).then_some(*outer);
    widgets::row(ui, &pass::OUTER_RADIUS, |ui| {
        widgets::optional(
            ui,
            &mut given,
            *inner + 10.0,
            "all of the boundary",
            |ui, r| {
                m.length_field(ui, r, 0.1, 10_000.0, "");
            },
        );
    });
    *outer = given.unwrap_or(0.0);
    if *outer > 0.0 && *inner >= *outer {
        *outer = *inner + 1.0;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    fn op(strategy: &str) -> Operation {
        let text = format!("name = \"op\"\nstrategy = \"{strategy}\"\n");
        toml::from_str(&text).unwrap()
    }

    /// A strategy that leaves stock only on the floor is ticked by its floor
    /// alone, and ticking it starts only the floor: a wall's left over from
    /// another strategy, which it neither shows nor cuts with, does not count,
    /// and none is written that it cannot show.
    #[test]
    fn leaving_stock_is_what_the_strategy_leaves() {
        let mut face = op("face");
        face.passes.stock_to_leave = StockToLeave {
            radial: 0.5,
            axial: 0.0,
        };
        assert!(!leaves_stock(&face), "a wall it has none of is no stock");
        assert_eq!(
            stock_to_start(&face),
            StockToLeave {
                radial: 0.0,
                axial: 0.5
            }
        );
        let rough = op("pocket-clearing");
        assert_eq!(
            stock_to_start(&rough),
            StockToLeave {
                radial: 0.5,
                axial: 0.5
            }
        );
        assert_eq!(stock_to_start(&op("drill")), StockToLeave::default());
    }
}
