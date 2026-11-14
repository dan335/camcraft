//! An operation's Passes tab: how its strategy lays its passes out, how far
//! apart they are, the levels it cuts in, the stock it leaves and its tabs.

use camcraft_core::job::{
    Compensation, ContourOrder, CutDirection, Depths, FaceDirection, FeedOptimization, Finishing,
    HolderClearance, HolderMode, Leave, Links, Operation, PassOrder, Passes, RadialDirection,
    StockToLeave, StrategySpec, TabShape, Tabs, ThreadHand,
};
use eframe::egui;

use crate::browser::strategy_label;
use crate::help::topics::passes as pass;
use crate::help::{Help, Topic};
use crate::ui::defaults::Defaults;
use crate::ui::measure::{Measure, follow_radial};
use crate::ui::theme;

use super::{across, left_out};
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
                machine_cavities,
                min_cutting_radius,
                order,
                slot_clearing,
                slot_width,
                min_flat_stepdown,
                ..
            } => {
                min_cutting_radius_row(ui, d, m, min_cutting_radius);
                d.row(ui, &pass::MACHINE_CAVITIES, "machine_cavities", |ui| {
                    ui.checkbox(machine_cavities, "go down into closed pockets");
                });
                widgets::dependent(ui, *machine_cavities, |ui| {
                    slot_clearing_rows(ui, d, m, (slot_clearing, slot_width), diameter);
                });
                adaptive_direction_and_order(ui, d, direction, order);
                d.row(ui, &pass::FLAT_AREAS, "flat_areas", |ui| {
                    ui.checkbox(flat_areas, "a level on each flat face and peak");
                });
                widgets::dependent(ui, *flat_areas, |ui| {
                    d.row(ui, &pass::MIN_FLAT_STEPDOWN, "min_flat_stepdown", |ui| {
                        widgets::optional_positive(ui, min_flat_stepdown, 0.5, "none", |ui, v| {
                            m.length_field(ui, v, 0.01, 100.0, "");
                        });
                    });
                });
            }
            // Fusion's 2D Adaptive: no flats or cavities to choose, a pocket
            // is always gone down into.
            StrategySpec::Adaptive2d {
                direction,
                min_cutting_radius,
                order,
                slot_clearing,
                slot_width,
                ..
            } => {
                min_cutting_radius_row(ui, d, m, min_cutting_radius);
                slot_clearing_rows(ui, d, m, (slot_clearing, slot_width), diameter);
                adaptive_direction_and_order(ui, d, direction, order);
            }
            StrategySpec::Chamfer2d {
                width,
                tip_offset,
                clearance,
                finishing_overlap,
                preserve_order,
            } => {
                d.row(ui, &pass::CHAMFER_WIDTH, "width", |ui| {
                    m.length_field(ui, width, 0.05, 100.0, "");
                });
                d.row(ui, &pass::CHAMFER_TIP_OFFSET, "tip_offset", |ui| {
                    m.length_field(ui, tip_offset, 0.05, 100.0, "");
                });
                d.row(ui, &pass::CHAMFER_CLEARANCE, "clearance", |ui| {
                    m.length_field(ui, clearance, 0.05, 100.0, "");
                });
                d.row(ui, &pass::FINISHING_OVERLAP, "finishing_overlap", |ui| {
                    m.length_field(ui, finishing_overlap, 0.1, 1000.0, "");
                });
                d.row(ui, &pass::PRESERVE_ORDER, "preserve_order", |ui| {
                    ui.checkbox(preserve_order, "in the order picked");
                });
            }
            StrategySpec::Engrave {
                depth,
                sharp_corner_angle,
            } => {
                d.row(ui, &pass::ENGRAVE_DEPTH, "depth", |ui| {
                    // As a 90° V's own: 0.95 of its radius.
                    let start = 0.95 * across(diameter) / 2.0;
                    widgets::optional(ui, depth, start, "0.95 of the flank's", |ui, v| {
                        m.length_field(ui, v, 0.05, 100.0, "");
                    });
                });
                d.row(ui, &pass::SHARP_CORNER_ANGLE, "sharp_corner_angle", |ui| {
                    ui.add(
                        widgets::number(sharp_corner_angle)
                            .speed(1.0)
                            .range(0.0..=180.0)
                            .suffix("°"),
                    );
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
                    let radius = across(diameter) / 2.0;
                    widgets::optional(ui, pass_extension, radius, "just clear", |ui, e| {
                        m.length_field(ui, e, 0.1, 1000.0, "");
                    });
                });
            }
            StrategySpec::Bore {
                use_ramp_angle,
                ramp_angle,
                pitch,
                multiple_passes,
                stepovers,
                conventional,
                repeat,
                ..
            } => {
                d.row(ui, &pass::USE_RAMP_ANGLE, "use_ramp_angle", |ui| {
                    ui.checkbox(use_ramp_angle, "set the helix by its angle");
                });
                widgets::dependent(ui, *use_ramp_angle, |ui| {
                    d.row(ui, &pass::BORE_RAMP_ANGLE, "ramp_angle", |ui| {
                        ui.add(
                            widgets::number(ramp_angle)
                                .speed(0.1)
                                .range(0.1..=89.0)
                                .suffix("°"),
                        );
                    });
                });
                widgets::dependent(ui, !*use_ramp_angle, |ui| {
                    d.row(ui, &pass::PITCH, "pitch", |ui| {
                        m.length_field(ui, pitch, 0.01, 100.0, "");
                    });
                });
                climb_or_conventional(
                    ui,
                    d,
                    (&pass::DIRECTION_ROUND, "conventional"),
                    conventional,
                );
                round_passes(
                    ui,
                    d,
                    m,
                    (multiple_passes, stepovers, repeat),
                    &mut op.passes.stepover,
                );
            }
            StrategySpec::Circular {
                multiple_passes,
                stepovers,
                conventional,
                repeat,
                ..
            } => {
                climb_or_conventional(
                    ui,
                    d,
                    (&pass::DIRECTION_ROUND, "conventional"),
                    conventional,
                );
                round_passes(
                    ui,
                    d,
                    m,
                    (multiple_passes, stepovers, repeat),
                    &mut op.passes.stepover,
                );
            }
            StrategySpec::Thread {
                pitch,
                pitch_diameter_offset,
                hand,
                conventional,
                multiple_passes,
                stepovers,
                ..
            } => {
                d.row(ui, &pass::THREAD_PITCH, "pitch", |ui| {
                    m.length_field(ui, pitch, 0.01, 100.0, "");
                });
                d.row(
                    ui,
                    &pass::PITCH_DIAMETER_OFFSET,
                    "pitch_diameter_offset",
                    |ui| {
                        m.signed_length_field(ui, pitch_diameter_offset, 0.01, 100.0, "");
                    },
                );
                d.row(ui, &pass::THREAD_HAND, "hand", |ui| {
                    widgets::segmented(
                        ui,
                        hand,
                        &[(ThreadHand::Right, "right"), (ThreadHand::Left, "left")],
                    );
                });
                climb_or_conventional(
                    ui,
                    d,
                    (&pass::DIRECTION_THREAD, "conventional"),
                    conventional,
                );
                passes_out(
                    ui,
                    d,
                    m,
                    (&pass::THREAD_PASSES, &pass::THREAD_STEPOVERS, "thread"),
                    (multiple_passes, stepovers),
                    &mut op.passes.stepover,
                );
            }
            StrategySpec::Slot {
                backoff_distance,
                repeat,
                preserve_order,
                ..
            } => {
                d.row(ui, &pass::BACKOFF, "backoff_distance", |ui| {
                    m.length_field(ui, backoff_distance, 0.05, 100.0, "");
                });
                d.row(ui, &pass::SLOT_REPEAT, "repeat", |ui| {
                    ui.checkbox(repeat, "along the floor a second time");
                });
                d.row(ui, &pass::PRESERVE_ORDER, "preserve_order", |ui| {
                    ui.checkbox(preserve_order, "in the order picked");
                });
            }
            StrategySpec::Trace {
                compensation,
                axial_offset,
                pass_extension,
                max_angle,
                repeat,
                preserve_order,
                ..
            } => {
                d.row(ui, &pass::COMPENSATION, "compensation", |ui| {
                    widgets::segmented(
                        ui,
                        compensation,
                        &[
                            (Compensation::Center, "on the curve"),
                            (Compensation::Left, "left"),
                            (Compensation::Right, "right"),
                        ],
                    );
                });
                d.row(ui, &pass::AXIAL_OFFSET, "axial_offset", |ui| {
                    m.signed_length_field(ui, axial_offset, 0.05, 1000.0, "");
                });
                d.row(ui, &pass::TRACE_EXTENSION, "pass_extension", |ui| {
                    m.length_field(ui, pass_extension, 0.1, 1000.0, "");
                });
                d.row(ui, &pass::MAX_ANGLE, "max_angle", |ui| {
                    ui.add(
                        widgets::number(max_angle)
                            .speed(1.0)
                            .range(1.0..=90.0)
                            .suffix("°"),
                    );
                });
                d.row(ui, &pass::TRACE_REPEAT, "repeat", |ui| {
                    ui.checkbox(repeat, "cut the last pass twice");
                });
                d.row(ui, &pass::PRESERVE_ORDER, "preserve_order", |ui| {
                    ui.checkbox(preserve_order, "in the order picked");
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
        // A hole's passes have their stepover with them, above.
        if uses.stepover && uses.holes.is_none() {
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
        if uses.cut_step {
            d.row(ui, &pass::DEEPEST_STEP, "passes.max_cut_step", |ui| {
                m.length_field(ui, &mut op.passes.max_cut_step, 0.05, 50.0, "");
            });
        }
    });
    if uses.depths != Depths::None {
        widgets::section(ui, "Multiple depths");
        multiple_depths(ui, d, m, op);
    }
    if uses.stock_to_leave != Leave::None {
        widgets::section(ui, "Stock to leave");
        stock_to_leave(ui, d, m, op, least);
    }
    if uses.tabs {
        widgets::section(ui, "Tabs");
        tabs(ui, d, m, &mut op.passes.tabs);
    }
    if uses.finishing {
        widgets::section(ui, "Finishing passes");
        // A bore's is one pass, at the cutting feed, its last pass repeated
        // by its own Repeat Passes, above: it has only a stepover.
        let bore = matches!(op.strategy, StrategySpec::Bore { .. });
        finishing(ui, d, m, &mut op.passes, diameter, bore);
    }
    if let Some(modes) = holder_modes(&op.strategy) {
        widgets::section(ui, "Shaft and holder");
        holder(ui, d, m, &mut op.passes.holder, modes);
    }
    if uses.smoothing {
        widgets::section(ui, "Smoothing");
        smoothing(ui, d, m, &mut op.passes);
    }
    if uses.feed_optimization {
        widgets::section(ui, "Feed optimization");
        feed_optimization(ui, d, m, &mut op.passes, diameter);
    }
}

/// Fusion's Feed Optimization: ticked to slow the cuts at their corners,
/// then where and to what. A value left out is camcraft's default, and says
/// what it comes to.
fn feed_optimization(
    ui: &mut egui::Ui,
    d: &mut Defaults<Operation>,
    m: Measure,
    passes: &mut Passes,
    diameter: Option<f64>,
) {
    d.optional_group(
        ui,
        &pass::FEED_OPTIMIZATION,
        "passes.feed_optimization",
        "Slow down in corners",
        &mut passes.feed_optimization,
    );
    let feed = passes.feed;
    let Some(f) = passes.feed_optimization.as_mut() else {
        return;
    };
    // What each value left out comes to, as the engine has it.
    let given = FeedOptimization::default().sizes(across(diameter) / 2.0, feed);
    let twentieth = left_out(m, diameter, given.radius, "a twentieth of the tool");
    let fifth = left_out(m, diameter, given.distance, "a fifth of the tool");
    widgets::sub_form(ui, "feed_optimization", |ui| {
        d.row(
            ui,
            &pass::MAX_DIRECTIONAL_CHANGE,
            "passes.feed_optimization.max_directional_change",
            |ui| {
                ui.add(
                    widgets::number(&mut f.max_directional_change)
                        .speed(1.0)
                        .range(1.0..=179.0)
                        .suffix("°"),
                );
            },
        );
        d.row(
            ui,
            &pass::REDUCED_FEED_RADIUS,
            "passes.feed_optimization.reduced_feed_radius",
            |ui| {
                widgets::optional(
                    ui,
                    &mut f.reduced_feed_radius,
                    given.radius,
                    &twentieth,
                    |ui, v| {
                        m.length_field(ui, v, 0.01, 100.0, "");
                    },
                );
            },
        );
        d.row(
            ui,
            &pass::REDUCED_FEED_DISTANCE,
            "passes.feed_optimization.reduced_feed_distance",
            |ui| {
                widgets::optional(
                    ui,
                    &mut f.reduced_feed_distance,
                    given.distance,
                    &fifth,
                    |ui, v| {
                        m.length_field(ui, v, 0.01, 100.0, "");
                    },
                );
            },
        );
        d.row(
            ui,
            &pass::REDUCED_FEED,
            "passes.feed_optimization.reduced_feed",
            |ui| {
                let half = format!(
                    "half the cutting feed: {} {}",
                    m.feed_number(given.feed),
                    m.feed_unit()
                );
                widgets::optional(ui, &mut f.reduced_feed, given.feed, &half, |ui, v| {
                    m.feed_field(ui, v, 50_000.0);
                });
            },
        );
        d.row(
            ui,
            &pass::ONLY_INNER_CORNERS,
            "passes.feed_optimization.only_inner_corners",
            |ui| {
                ui.checkbox(&mut f.only_inner_corners, "those turning into the material");
            },
        );
    });
}

/// How a strategy may keep its shank and holder clear, each way named as it
/// is for it: `None` for one that does not (facing, 2D cutting).
fn holder_modes(strategy: &StrategySpec) -> Option<&'static [(HolderMode, &'static str)]> {
    const OVER_PART: &[(HolderMode, &str)] = &[
        (HolderMode::Off, "off"),
        (HolderMode::PullAway, "pull away"),
        (HolderMode::Trim, "trim"),
        (HolderMode::Fail, "fail"),
    ];
    // At a fixed height there is nothing to lift to: trimming is all.
    const LEVELS: &[(HolderMode, &str)] = &[
        (HolderMode::Off, "off"),
        (HolderMode::Trim, "trim"),
        (HolderMode::Fail, "fail"),
    ];
    const HOLES: &[(HolderMode, &str)] = &[
        (HolderMode::Off, "off"),
        (HolderMode::Trim, "skip holes"),
        (HolderMode::Fail, "fail"),
    ];
    match strategy {
        // Fusion's Trace has no Shaft and Holder, though it follows its curves
        // as a finish follows the part; nor have its 2D Adaptive, its Engrave
        // and its Thread.
        StrategySpec::Trace { .. }
        | StrategySpec::Adaptive2d { .. }
        | StrategySpec::Engrave { .. }
        | StrategySpec::Thread { .. } => None,
        // Its 2D Chamfer has one, over the part, as a finish's.
        _ if strategy.uses().links == Links::OverPart => Some(OVER_PART),
        StrategySpec::Contour { .. }
        | StrategySpec::Flat { .. }
        | StrategySpec::Adaptive { .. }
        | StrategySpec::PocketClearing { .. } => Some(LEVELS),
        StrategySpec::Drill { .. } => Some(HOLES),
        _ => None,
    }
}

/// Fusion's Shaft and Holder: what generating does where the rest of the
/// tool would meet the material, and how near each part of it may come.
fn holder(
    ui: &mut egui::Ui,
    d: &mut Defaults<Operation>,
    m: Measure,
    h: &mut HolderClearance,
    modes: &[(HolderMode, &str)],
) {
    form(ui, "holder", |ui| {
        d.row(ui, &pass::HOLDER_MODE, "passes.holder.mode", |ui| {
            // A mode this strategy has no use for, from another it was, is
            // trimming, as the run takes it; shown so, and kept until changed.
            let known = modes.iter().any(|(mode, _)| *mode == h.mode);
            let mut shown = if known { h.mode } else { HolderMode::Trim };
            widgets::segmented(ui, &mut shown, modes);
            if shown != h.mode && (known || shown != HolderMode::Trim) {
                h.mode = shown;
            }
        });
        widgets::dependent(ui, h.mode != HolderMode::Off, |ui| {
            d.row(
                ui,
                &pass::SHANK_CLEARANCE,
                "passes.holder.shank_clearance",
                |ui| {
                    m.length_field(ui, &mut h.shank_clearance, 0.0, 100.0, "");
                },
            );
            d.row(
                ui,
                &pass::HOLDER_CLEARANCE,
                "passes.holder.holder_clearance",
                |ui| {
                    m.length_field(ui, &mut h.holder_clearance, 0.0, 100.0, "");
                },
            );
        });
    });
}

/// Fusion's Smoothing: ticked to fit the passes with arcs and longer lines,
/// then how far they may stray, the job's tolerance unless given.
fn smoothing(ui: &mut egui::Ui, d: &mut Defaults<Operation>, m: Measure, passes: &mut Passes) {
    form(ui, "smoothing", |ui| {
        d.row(ui, &pass::SMOOTHING, "passes.smoothing", |ui| {
            ui.checkbox(&mut passes.smoothing, "fit arcs and longer lines");
        });
        widgets::dependent(ui, passes.smoothing, |ui| {
            d.row(
                ui,
                &pass::SMOOTHING_TOLERANCE,
                "passes.smoothing_tolerance",
                |ui| {
                    widgets::optional(
                        ui,
                        &mut passes.smoothing_tolerance,
                        0.01,
                        "the job's tolerance",
                        |ui, t| {
                            m.length_field(ui, t, 0.001, 1.0, "");
                        },
                    );
                },
            );
        });
    });
}

/// A pocket's finishing passes along its walls, off until asked for, as
/// Fusion's: how many, how far apart, how fast the last goes, and whether it
/// goes twice; a `bore`'s, how far in from the wall its one is. A value left
/// out is Fusion's, and says what it comes to.
fn finishing(
    ui: &mut egui::Ui,
    d: &mut Defaults<Operation>,
    m: Measure,
    passes: &mut Passes,
    diameter: Option<f64>,
    bore: bool,
) {
    d.optional_group(
        ui,
        &pass::FINISHING,
        "passes.finishing",
        "Finish the walls with the side of the tool",
        &mut passes.finishing,
    );
    let feed = passes.feed;
    let Some(f) = passes.finishing.as_mut() else {
        return;
    };
    // What each value left out comes to, as the engine has it.
    let given = Finishing::default().sizes(across(diameter), feed);
    let tenth = left_out(m, diameter, given.stepover, "a tenth of the tool");
    widgets::sub_form(ui, "finishing", |ui| {
        if !bore {
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
        }
        d.row(
            ui,
            &pass::FINISHING_STEPOVER,
            "passes.finishing.stepover",
            |ui| {
                widgets::optional(ui, &mut f.stepover, given.stepover, &tenth, |ui, v| {
                    m.length_field(ui, v, 0.01, 100.0, "");
                });
            },
        );
        if bore {
            return;
        }
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

/// A bore's or a circular's passes out to the wall ([`passes_out`]), and
/// whether the last is cut twice.
fn round_passes(
    ui: &mut egui::Ui,
    d: &mut Defaults<Operation>,
    m: Measure,
    (multiple, stepovers, repeat): (&mut bool, &mut u32, &mut bool),
    stepover: &mut f64,
) {
    passes_out(
        ui,
        d,
        m,
        (&pass::MULTIPLE_PASSES, &pass::STEPOVERS, "wall"),
        (multiple, stepovers),
        stepover,
    );
    d.row(ui, &pass::REPEAT_PASS, "repeat", |ui| {
        ui.checkbox(repeat, "cut the last one twice");
    });
}

/// Passes out to a hole's wall or its thread, `to`, as Fusion's Multiple
/// Passes: ticked, how many, the last at it, and how far apart, each row with
/// its help, `topics`.
fn passes_out(
    ui: &mut egui::Ui,
    d: &mut Defaults<Operation>,
    m: Measure,
    (multiple_topic, stepovers_topic, to): (&Topic, &Topic, &str),
    (multiple, stepovers): (&mut bool, &mut u32),
    stepover: &mut f64,
) {
    d.row(ui, multiple_topic, "multiple_passes", |ui| {
        ui.checkbox(multiple, format!("more than one out to the {to}"));
    });
    widgets::dependent(ui, *multiple, |ui| {
        d.row(ui, stepovers_topic, "stepovers", |ui| {
            ui.add(widgets::number(stepovers).range(1..=1000));
        });
        d.row(ui, &pass::STEPOVER, "passes.stepover", |ui| {
            m.length_field(ui, stepover, 0.05, 100.0, "");
        });
    });
}

/// Adaptive clearing's tightest turn into an inside corner, 3D or 2D: none, or
/// a radius.
fn min_cutting_radius_row(ui: &mut egui::Ui, d: &mut Defaults<Operation>, m: Measure, r: &mut f64) {
    d.row(ui, &pass::MIN_CUTTING_RADIUS, "min_cutting_radius", |ui| {
        widgets::optional_positive(ui, r, 1.0, "none: into every corner", |ui, r| {
            m.length_field(ui, r, 0.05, 100.0, "");
        });
    });
}

/// Adaptive clearing's slot along a closed pocket's middle first, 3D or 2D:
/// ticked, how wide.
fn slot_clearing_rows(
    ui: &mut egui::Ui,
    d: &mut Defaults<Operation>,
    m: Measure,
    (slot_clearing, slot_width): (&mut bool, &mut f64),
    diameter: Option<f64>,
) {
    d.row(ui, &pass::SLOT_CLEARING, "slot_clearing", |ui| {
        ui.checkbox(slot_clearing, "a slot along the middle first");
    });
    widgets::dependent(ui, *slot_clearing, |ui| {
        d.row(ui, &pass::SLOT_WIDTH, "slot_width", |ui| {
            let start = 1.5 * across(diameter);
            widgets::optional_positive(ui, slot_width, start, "the tool's width", |ui, w| {
                m.length_field(ui, w, 0.05, 1000.0, "");
            });
        });
    });
}

/// Which way adaptive clearing's passes cut, 3D or 2D, and whether it clears
/// level by level or area by area.
fn adaptive_direction_and_order(
    ui: &mut egui::Ui,
    d: &mut Defaults<Operation>,
    direction: &mut CutDirection,
    order: &mut ContourOrder,
) {
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
    d.row(ui, &pass::ADAPTIVE_ORDER, "order", |ui| {
        widgets::segmented(
            ui,
            order,
            &[
                (ContourOrder::ByDepth, "by depth"),
                (ContourOrder::ByArea, "by area"),
            ],
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
/// strategy's own, or a slot's, a trace's and an engraving's Maximum Stepdown
/// as Fusion's starts it, a millimetre, or 2 mm.
pub(super) fn starting_stepdown(op: &Operation) -> f64 {
    let fusions = match op.strategy {
        StrategySpec::Slot { .. } | StrategySpec::Trace { .. } | StrategySpec::Engrave { .. } => {
            1.0
        }
        _ => 2.0,
    };
    op.strategy.default_stepdown().unwrap_or(fusions)
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
        // Its stepdown's expression comes and goes with the levels: left on,
        // it would put them back.
        let worked_out = op
            .strategy
            .default_expressions()
            .into_iter()
            .find(|(setting, _)| *setting == "passes.stepdown");
        match worked_out.filter(|_| on) {
            Some((setting, text)) => {
                op.expressions.insert(setting.into(), text.into());
            }
            None => {
                op.expressions.remove("passes.stepdown");
            }
        }
    }
    let fine = op.strategy.uses().fine_stepdown;
    let Some(stepdown) = &mut op.passes.stepdown else {
        return;
    };
    ui.add_space(theme::SPACE_2);
    widgets::sub_form(ui, "multiple-depths", |ui| {
        d.row(ui, &pass::STEPDOWN, "passes.stepdown", |ui| {
            m.length_field(ui, stepdown, 0.05, 100.0, "");
        });
        if let StrategySpec::Circular { even_stepdowns, .. }
        | StrategySpec::Adaptive2d { even_stepdowns, .. } = &mut op.strategy
        {
            d.row(ui, &pass::EVEN_STEPDOWNS, "even_stepdowns", |ui| {
                ui.checkbox(even_stepdowns, "all the same depth");
            });
        }
        // A trace's levels are above its curves, so many of them.
        if let StrategySpec::Trace {
            stepdowns,
            order,
            both_ways,
            ..
        } = &mut op.strategy
        {
            d.row(ui, &pass::STEPDOWNS, "stepdowns", |ui| {
                ui.add(widgets::number(stepdowns).range(1..=1000));
            });
            d.row(ui, &pass::TRACE_ORDER, "order", |ui| {
                widgets::segmented(
                    ui,
                    order,
                    &[
                        (ContourOrder::ByDepth, "by depth"),
                        (ContourOrder::ByArea, "by curve"),
                    ],
                );
            });
            d.row(ui, &pass::BOTH_WAYS, "both_ways", |ui| {
                ui.checkbox(both_ways, "back and forth along open curves");
            });
        }
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
        if let StrategySpec::Adaptive {
            shallow_areas,
            min_shallow_stepdown,
            max_shallow_stepover,
            min_axial_engagement,
            ..
        } = &mut op.strategy
        {
            shallow(
                ui,
                d,
                m,
                *stepdown,
                (shallow_areas, min_shallow_stepdown, max_shallow_stepover),
            );
            d.row(
                ui,
                &pass::MIN_AXIAL_ENGAGEMENT,
                "min_axial_engagement",
                |ui| {
                    widgets::optional_positive(ui, min_axial_engagement, 0.5, "none", |ui, v| {
                        m.length_field(ui, v, 0.01, 100.0, "");
                    });
                },
            );
        }
    });
}

/// Adaptive clearing's shallow areas, as Fusion's Passes tab has them: ticked
/// to add levels where the terraces are wide, then the closest they come and
/// the widest a terrace may be.
fn shallow(
    ui: &mut egui::Ui,
    d: &mut Defaults<Operation>,
    m: Measure,
    stepdown: f64,
    (on, min_stepdown, max_stepover): (&mut bool, &mut f64, &mut f64),
) {
    d.row(ui, &pass::SHALLOW_AREAS, "shallow_areas", |ui| {
        ui.checkbox(on, "levels between where the slope is shallow");
    });
    widgets::dependent(ui, *on, |ui| {
        d.row(
            ui,
            &pass::MIN_SHALLOW_STEPDOWN,
            "min_shallow_stepdown",
            |ui| {
                m.length_field(ui, min_stepdown, 0.01, stepdown.max(0.01), "");
            },
        );
        d.row(
            ui,
            &pass::MAX_SHALLOW_STEPOVER,
            "max_shallow_stepover",
            |ui| {
                m.length_field(ui, max_stepover, 0.01, 1000.0, "");
            },
        );
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
    widgets::sub_form(ui, "stock-to-leave", |ui| {
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
    d.optional_group(
        ui,
        &pass::TABS,
        "passes.tabs",
        "Leave tabs to hold the part",
        tabs,
    );
    let Some(t) = tabs else {
        return;
    };
    widgets::sub_form(ui, "tabs", |ui| {
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
