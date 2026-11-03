//! An operation's Passes tab: how its strategy lays its passes out, how far
//! apart they are, the levels it cuts in, the stock it leaves and its tabs.

use camcraft_core::job::{
    ContourOrder, CutDirection, Depths, FaceDirection, Finishing, Leave, Operation, PassOrder,
    Passes, RadialDirection, StockToLeave, StrategySpec, TabShape, Tabs,
};
use eframe::egui;

use crate::browser::strategy_label;
use crate::help::topics::passes as pass;
use crate::help::{Help, Topic};
use crate::ui::defaults::Defaults;
use crate::ui::measure::{Measure, follow_radial};
use crate::ui::theme;
use crate::ui::widgets::{self, form, hint};

pub(super) fn passes(
    ui: &mut egui::Ui,
    m: Measure,
    op: &mut Operation,
    d: &mut Defaults<Operation>,
    least: StockToLeave,
    diameter: Option<f64>,
) {
    let uses = op.strategy.uses();
    form(ui, "passes", |ui| {
        match &mut op.strategy {
            StrategySpec::Parallel { angle } => {
                d.row(ui, &pass::PASS_ANGLE, "angle", |ui| angle_field(ui, angle));
            }
            StrategySpec::Scallop {
                direction,
                order,
                slope_from,
                slope_to,
            } => {
                both_ways_direction(ui, d, direction);
                pass_order(ui, d, &pass::SCALLOP_ORDER, order);
                slopes(ui, d, slope_from, slope_to);
            }
            StrategySpec::Contour {
                direction,
                order,
                slope_from,
                slope_to,
            } => {
                both_ways_direction(ui, d, direction);
                d.row(ui, &pass::CONTOUR_ORDER, "order", |ui| {
                    widgets::segmented(
                        ui,
                        order,
                        &[
                            (ContourOrder::ByDepth, "by depth"),
                            (ContourOrder::ByArea, "by area"),
                        ],
                    );
                });
                slopes(ui, d, slope_from, slope_to);
            }
            StrategySpec::Flat { direction, order } => {
                climb_direction(ui, d, &pass::DIRECTION_2D, direction);
                pass_order(ui, d, &pass::SCALLOP_ORDER, order);
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
                centred(ui, d, m, centre, inner_radius, outer_radius);
                climb_direction(ui, d, &pass::DIRECTION_SPIRAL, direction);
                pass_order(ui, d, &pass::SPIRAL_ORDER, order);
                slopes(ui, d, slope_from, slope_to);
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
                centred(ui, d, m, centre, inner_radius, outer_radius);
                d.row(ui, &pass::RADIAL_ANGLES, "angle_from,angle_to", |ui| {
                    ui.add(
                        widgets::number(angle_from)
                            .speed(1.0)
                            .range(-360.0..=360.0)
                            .prefix("from ")
                            .suffix("°"),
                    );
                    ui.add(
                        widgets::number(angle_to)
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
                d.row(ui, &pass::RADIAL_DIRECTION, "direction", |ui| {
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
                slopes(ui, d, slope_from, slope_to);
            }
            StrategySpec::MorphedSpiral {
                centre,
                direction,
                order,
                slope_from,
                slope_to,
            } => {
                d.row(ui, &pass::MORPH_CENTRE, "centre", |ui| {
                    centre_field(ui, m, centre, "the point furthest inside");
                });
                climb_direction(ui, d, &pass::DIRECTION_SPIRAL, direction);
                pass_order(ui, d, &pass::SPIRAL_ORDER, order);
                slopes(ui, d, slope_from, slope_to);
            }
            StrategySpec::PocketClearing {
                min_ring_length,
                conventional,
                flat_areas,
            } => {
                climb_or_conventional(ui, d, (&pass::DIRECTION_2D, "conventional"), conventional);
                d.row(ui, &pass::FLAT_AREAS, "flat_areas", |ui| {
                    ui.checkbox(flat_areas, "a level on each flat face");
                });
                d.row(ui, &pass::SHORTEST_RING, "min_ring_length", |ui| {
                    m.length_field(ui, min_ring_length, 0.1, 1000.0, "");
                });
            }
            StrategySpec::Adaptive {
                direction,
                flat_areas,
            } => {
                d.row(ui, &pass::DIRECTION_ADAPTIVE, "direction", |ui| {
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
                d.row(ui, &pass::FLAT_AREAS, "flat_areas", |ui| {
                    ui.checkbox(flat_areas, "a level on each flat face");
                });
            }
            StrategySpec::Profile { conventional, .. } | StrategySpec::Pocket { conventional } => {
                climb_or_conventional(ui, d, (&pass::DIRECTION_2D, "conventional"), conventional);
            }
            StrategySpec::Face {
                angle,
                direction,
                pass_extension,
                ..
            } => {
                d.row(ui, &pass::PASS_ANGLE, "angle", |ui| angle_field(ui, angle));
                d.row(ui, &pass::DIRECTION_FACE, "direction", |ui| {
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
                d.row(ui, &pass::PASS_EXTENSION, "pass_extension", |ui| {
                    m.length_field(ui, pass_extension, 0.1, 1000.0, "");
                });
            }
            StrategySpec::Drill { peck, dwell } => {
                d.row(ui, &pass::PECK, "peck", |ui| {
                    widgets::optional(ui, peck, 2.0, "all the way at once", |ui, p| {
                        m.length_field(ui, p, 0.05, 100.0, "");
                    });
                });
                d.row(ui, &pass::DWELL, "dwell", |ui| {
                    widgets::optional(ui, dwell, 0.5, "none", |ui, d| {
                        ui.add(
                            widgets::number(d)
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
                d.row(ui, &pass::OPTIMAL_LOAD, "passes.stepover", |ui| {
                    m.length_field(ui, &mut op.passes.stepover, 0.01, 100.0, "");
                });
            } else {
                d.row(ui, &pass::STEPOVER, "passes.stepover", |ui| {
                    m.length_field(ui, &mut op.passes.stepover, 0.05, 100.0, "");
                });
            }
        }
        d.row(ui, &pass::DEEPEST_STEP, "passes.max_cut_step", |ui| {
            m.length_field(ui, &mut op.passes.max_cut_step, 0.05, 50.0, "");
        });
    });
    if uses.depths != Depths::None {
        ui.add_space(theme::SPACE_4);
        ui.label(widgets::section_label("Multiple depths"));
        ui.add_space(theme::SPACE_2);
        multiple_depths(ui, d, m, op);
    }
    if uses.stock_to_leave != Leave::None {
        ui.add_space(theme::SPACE_4);
        ui.label(widgets::section_label("Stock to leave"));
        ui.add_space(theme::SPACE_2);
        stock_to_leave(ui, d, m, op, least);
    }
    if uses.tabs {
        ui.add_space(theme::SPACE_4);
        ui.label(widgets::section_label("Tabs"));
        ui.add_space(theme::SPACE_2);
        tabs(ui, d, m, &mut op.passes.tabs);
    }
    if uses.finishing {
        ui.add_space(theme::SPACE_4);
        ui.label(widgets::section_label("Finishing passes"));
        ui.add_space(theme::SPACE_2);
        finishing(ui, d, m, &mut op.passes, diameter);
    }
}

/// A pocket's finishing passes along its walls, off until asked for, as
/// Fusion's: how many, how far apart, how fast the last goes, and whether it
/// goes twice. A value left out is Fusion's, and says what it comes to.
fn finishing(
    ui: &mut egui::Ui,
    d: &mut Defaults<Operation>,
    m: Measure,
    passes: &mut Passes,
    diameter: Option<f64>,
) {
    let mut on = passes.finishing.is_some();
    ui.horizontal(|ui| {
        ui.checkbox(&mut on, "Finish the walls with the side of the tool")
            .help(&pass::FINISHING);
        d.reset_group(ui, "passes.finishing");
    });
    match (on, passes.finishing.is_some()) {
        (true, false) => passes.finishing = Some(Finishing::default()),
        (false, true) => passes.finishing = None,
        _ => {}
    }
    let feed = passes.feed;
    let Some(f) = passes.finishing.as_mut() else {
        return;
    };
    let tenth = match diameter {
        Some(d) => format!("a tenth of the tool: {}", m.length(0.1 * d, 2)),
        None => "a tenth of the tool".to_string(),
    };
    ui.add_space(theme::SPACE_2);
    form(ui, "finishing", |ui| {
        d.row(
            ui,
            &pass::FINISHING_PASSES,
            "passes.finishing.passes",
            |ui| {
                let mut n = f.passes.unwrap_or(1);
                ui.add(widgets::number(&mut n).range(1..=20));
                f.passes = (n != 1).then_some(n);
            },
        );
        d.row(
            ui,
            &pass::FINISHING_STEPOVER,
            "passes.finishing.stepover",
            |ui| {
                let start = 0.1 * diameter.unwrap_or(6.0);
                widgets::optional(ui, &mut f.stepover, start, &tenth, |ui, v| {
                    m.length_field(ui, v, 0.01, 100.0, "");
                });
            },
        );
        d.row(ui, &pass::FINISHING_FEED, "passes.finishing.feed", |ui| {
            widgets::optional(ui, &mut f.feed, feed, "the cutting feed", |ui, v| {
                m.feed_field(ui, v, 50_000.0);
            });
        });
        d.row(
            ui,
            &pass::FINISHING_REPEAT,
            "passes.finishing.repeat",
            |ui| {
                ui.checkbox(&mut f.repeat, "cut the last one twice");
            },
        );
    });
}

/// The direction passes run, in degrees from X.
fn angle_field(ui: &mut egui::Ui, angle: &mut f64) {
    ui.add(
        widgets::number(angle)
            .speed(1.0)
            .range(-360.0..=360.0)
            .suffix("°"),
    );
}

/// The stepdown a box ticked to step down in levels starts at: the
/// strategy's own, or 2 mm.
pub(super) fn starting_stepdown(op: &Operation) -> f64 {
    op.strategy.default_stepdown().unwrap_or(2.0)
}

/// Multiple depths, as Fusion's Passes tab has them: ticked to cut in levels,
/// then the deepest each may be. Unticked is the bottom in one pass. A strategy
/// that always cuts in levels, as roughing does, cannot be unticked.
fn multiple_depths(ui: &mut egui::Ui, d: &mut Defaults<Operation>, m: Measure, op: &mut Operation) {
    let required = op.strategy.uses().depths == Depths::Required;
    let mut on = op.passes.stepdown.is_some();
    let r = ui
        .horizontal(|ui| {
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
            // Where it always does, the stepdown's own row has the ↺.
            if !required {
                d.reset_group(ui, "passes.stepdown");
            }
            r
        })
        .inner;
    if r.changed() {
        op.passes.stepdown = on.then(|| starting_stepdown(op));
    }
    let fine = op.strategy.uses().fine_stepdown;
    let Some(stepdown) = &mut op.passes.stepdown else {
        return;
    };
    ui.add_space(theme::SPACE_2);
    form(ui, "multiple-depths", |ui| {
        d.row(ui, &pass::STEPDOWN, "passes.stepdown", |ui| {
            m.length_field(ui, stepdown, 0.05, 100.0, "");
        });
        // Ticked, it starts at a quarter of the stepdown.
        if fine {
            let start = *stepdown / 4.0;
            let most = *stepdown;
            d.row(ui, &pass::FINE_STEPDOWN, "passes.fine_stepdown", |ui| {
                widgets::optional(ui, &mut op.passes.fine_stepdown, start, "none", |ui, f| {
                    m.length_field(ui, f, 0.05, most, "");
                });
            });
        }
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
fn stock_to_leave(
    ui: &mut egui::Ui,
    d: &mut Defaults<Operation>,
    m: Measure,
    op: &mut Operation,
    least: StockToLeave,
) {
    let leave = op.strategy.uses().stock_to_leave;
    let mut on = leaves_stock(op);
    let ticked = ui
        .horizontal(|ui| {
            let r = ui
                .checkbox(&mut on, "Leave stock for a finishing pass")
                .help(&pass::STOCK_TO_LEAVE)
                .changed();
            d.reset(ui, "passes.stock_to_leave");
            r
        })
        .inner;
    if ticked {
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
            d.row(ui, &pass::WALLS, "passes.stock_to_leave.radial", |ui| {
                m.wall_to_leave_field(ui, &mut stock.radial, least.radial, "");
            });
        }
        d.row(ui, &pass::FLOORS, "passes.stock_to_leave.axial", |ui| {
            m.floor_to_leave_field(ui, &mut stock.axial, least.axial, "");
        });
    });
    follow_radial(stock, before);
}

/// Tabs left across the deepest cut to hold what it frees, and their size.
fn tabs(ui: &mut egui::Ui, d: &mut Defaults<Operation>, m: Measure, tabs: &mut Option<Tabs>) {
    let mut on = tabs.is_some();
    ui.horizontal(|ui| {
        ui.checkbox(&mut on, "Leave tabs to hold the part")
            .help(&pass::TABS);
        d.reset_group(ui, "passes.tabs");
    });
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
        d.row(ui, &pass::TAB_SHAPE, "passes.tabs.shape", |ui| {
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
        d.row(ui, &pass::TAB_WIDTH, "passes.tabs.width", |ui| {
            m.length_field(ui, &mut t.width, 0.1, 1000.0, "");
        });
        d.row(ui, &pass::TAB_HEIGHT, "passes.tabs.height", |ui| {
            m.length_field(ui, &mut t.height, 0.1, 1000.0, "");
        });
        d.row(ui, &pass::TAB_SPACING, "passes.tabs.spacing", |ui| {
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
fn both_ways_direction(
    ui: &mut egui::Ui,
    d: &mut Defaults<Operation>,
    direction: &mut CutDirection,
) {
    d.row(ui, &pass::DIRECTION_SCALLOP, "direction", |ui| {
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
fn climb_or_conventional(
    ui: &mut egui::Ui,
    d: &mut Defaults<Operation>,
    (topic, path): (&Topic, &str),
    conventional: &mut bool,
) {
    d.row(ui, topic, path, |ui| {
        widgets::segmented(
            ui,
            conventional,
            &[(false, "climb"), (true, "conventional")],
        );
    });
}

/// Which way a pass cuts, climb or conventional: both ways, which has no
/// meaning for it, reads as climb.
fn climb_direction(
    ui: &mut egui::Ui,
    d: &mut Defaults<Operation>,
    topic: &Topic,
    direction: &mut CutDirection,
) {
    let was = *direction == CutDirection::Conventional;
    let mut conventional = was;
    climb_or_conventional(ui, d, (topic, "direction"), &mut conventional);
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
fn pass_order(
    ui: &mut egui::Ui,
    d: &mut Defaults<Operation>,
    topic: &Topic,
    order: &mut PassOrder,
) {
    d.row(ui, topic, "order", |ui| {
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
fn slopes(
    ui: &mut egui::Ui,
    d: &mut Defaults<Operation>,
    slope_from: &mut f64,
    slope_to: &mut f64,
) {
    d.row(ui, &pass::SLOPES, "slope_from,slope_to", |ui| {
        ui.add(
            widgets::number(slope_from)
                .speed(1.0)
                .range(0.0..=89.0)
                .prefix("from ")
                .suffix("°"),
        );
        ui.add(
            widgets::number(slope_to)
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
    d: &mut Defaults<Operation>,
    m: Measure,
    centre: &mut Option<[f64; 2]>,
    inner: &mut f64,
    outer: &mut f64,
) {
    d.row(ui, &pass::CENTRE, "centre", |ui| {
        centre_field(ui, m, centre, "the boundary's middle");
    });
    d.row(ui, &pass::INNER_RADIUS, "inner_radius", |ui| {
        m.length_field(ui, inner, 0.1, 10_000.0, "");
    });
    // Zero is none given.
    let mut given = (*outer > 0.0).then_some(*outer);
    d.row(ui, &pass::OUTER_RADIUS, "outer_radius", |ui| {
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

#[cfg(all(test, feature = "ui-snapshots"))]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod field_tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;

    /// The fine stepdown, typed into a key at a time, keeps the keyboard from
    /// key to key, and takes what was typed on Enter.
    #[test]
    fn the_fine_stepdown_keeps_the_keyboard_while_typed_into() {
        let mut op = Operation::new("rough", StrategySpec::named("adaptive").unwrap());
        op.passes.stepdown = Some(12.7);
        op.passes.fine_stepdown = Some(3.175);
        let def = op.clone();
        let mut h = Harness::new_ui_state(
            move |ui, op: &mut Operation| {
                let mut d = Defaults::of(&*op, &def);
                multiple_depths(ui, &mut d, Measure::Metric, op);
                d.apply(op);
            },
            op,
        );
        h.run();
        let fields = h.get_all_by_role(egui::accesskit::Role::SpinButton).count();
        assert_eq!(fields, 2, "the stepdown and the fine stepdown");
        h.get_all_by_role(egui::accesskit::Role::SpinButton)
            .nth(1)
            .unwrap()
            .click();
        h.run();
        for c in "2.54".chars() {
            let focused = h
                .get_all_by_role(egui::accesskit::Role::SpinButton)
                .filter(|n| n.is_focused())
                .count();
            assert_eq!(focused, 1, "focus lost before typing {c:?}");
            h.get_all_by_role(egui::accesskit::Role::SpinButton)
                .find(|n| n.is_focused())
                .unwrap()
                .type_text(&c.to_string());
            h.run();
        }
        h.key_press(egui::Key::Enter);
        h.run();
        assert_eq!(h.state().passes.fine_stepdown, Some(2.54));
    }
}
