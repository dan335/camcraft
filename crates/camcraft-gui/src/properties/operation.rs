//! An operation's panel, on Fusion's tabs: the tool it cuts with, the geometry
//! it is kept to, its heights, its passes, and how its passes are linked.

use camcraft_core::job::{Containment, FeatureRef, Height, HeightRef, Operation, StrategySpec};
use eframe::egui;

use super::{Picks, PropAction, cannot_run, error, header, quiet, tab_bar};
use crate::document::{Document, Level, Selection};
use crate::measure::Measure;
use crate::slots::{Kind, Slot};
use crate::theme;
use crate::tool_editor::{list_row_background, paint_tool, tool_fields};
use crate::widgets::{self, Icon, Size, Tab, Variant, form};

/// Which tab an operation shows: the same for every operation, so moving between
/// them keeps the place.
const TAB: &str = "operation-tab";

pub(super) fn show(
    ui: &mut egui::Ui,
    doc: &mut Document,
    m: Measure,
    setup: &str,
    index: usize,
    picks: Picks<'_>,
    actions: &mut Vec<PropAction>,
) {
    let Some(si) = doc.setup_index(setup) else {
        doc.selection = Selection::Setups;
        return;
    };
    let Some(taken) = doc.job.setups.get(si).and_then(|s| {
        s.operations.get(index)?;
        Some(
            s.operations
                .iter()
                .map(|o| o.name.clone())
                .collect::<Vec<_>>(),
        )
    }) else {
        doc.selection = Selection::Setup(setup.to_string());
        return;
    };
    let id = ui.id().with(("operation", setup, index));
    let Some(current) = operation_mut(doc, si, index).map(|o| o.name.clone()) else {
        return;
    };
    if let Some(to) = header(ui, "Operation", id, &current) {
        rename(doc, si, index, &to, &taken);
    }
    cannot_run(ui, doc, setup);
    let Some(op) = operation_mut(doc, si, index) else {
        return;
    };
    form(ui, id.with("strategy"), |ui| {
        widgets::field_label(ui, "Strategy");
        strategy_combo(ui, op);
        ui.end_row();
    });
    ui.add_space(theme::SPACE_3);

    let tool_name = op.tool.clone();
    let tool_trouble = tool_name
        .as_ref()
        .is_none_or(|t| doc.number_clash(t).is_some() || !doc.job.tools.contains_key(t));
    let lost = picks.shown.is_some_and(|s| {
        s.faces.iter().chain(&s.edges).any(Result::is_err)
            || s.boundary.as_ref().is_some_and(Result::is_err)
    });
    let tabs = [
        Tab::new("Tool").flagged(tool_trouble),
        Tab::new("Geometry").flagged(lost),
        Tab::new("Heights"),
        Tab::new("Passes"),
        Tab::new("Linking"),
    ];
    let picking = matches!(picks.armed, Some(Slot::Faces { .. } | Slot::Edges { .. })).then_some(1);
    match tab_bar(ui, egui::Id::new(TAB), &tabs, picking) {
        0 => tool(ui, doc, m, setup, si, index, tool_name.as_deref(), actions),
        1 => {
            if let Some(op) = operation_mut(doc, si, index) {
                geometry(ui, m, setup, index, op, picks, actions);
            }
        }
        2 => {
            if let Some(op) = operation_mut(doc, si, index) {
                heights(ui, m, op);
            }
        }
        3 => {
            let tool_rpm = tool_name
                .as_ref()
                .and_then(|t| doc.job.tools.get(t))
                .map(|t| t.rpm);
            if let Some(op) = operation_mut(doc, si, index) {
                passes(ui, m, op, tool_rpm);
            }
        }
        _ => {
            if let Some(op) = operation_mut(doc, si, index) {
                linking(ui, m, op);
            }
        }
    }
}

/// Rename operation `index` of setup `si` to `to`, if the name is free.
fn rename(doc: &mut Document, si: usize, index: usize, to: &str, taken: &[String]) {
    let to = to.trim().to_string();
    let Some(op) = operation_mut(doc, si, index) else {
        return;
    };
    if to.is_empty() {
        doc.note(Level::Warn, "an operation needs a name");
    } else if to != op.name && taken.contains(&to) {
        doc.note(
            Level::Warn,
            format!("there is already an operation called '{to}'"),
        );
    } else {
        op.name = to;
    }
}

fn strategy_combo(ui: &mut egui::Ui, op: &mut Operation) {
    let name = |s: &StrategySpec| match s {
        StrategySpec::Parallel { .. } => "parallel",
        StrategySpec::ZlevelRough { .. } => "z-level rough",
        StrategySpec::Drill { .. } => "drilling",
    };
    egui::ComboBox::from_id_salt("strategy")
        .selected_text(name(&op.strategy))
        .show_ui(ui, |ui| {
            for s in [
                StrategySpec::Parallel { angle: 0.0 },
                StrategySpec::ZlevelRough {
                    stepdown: 2.0,
                    min_ring_length: 3.0,
                },
                StrategySpec::Drill {
                    peck: None,
                    dwell: None,
                },
            ] {
                let on = std::mem::discriminant(&op.strategy) == std::mem::discriminant(&s);
                if ui.selectable_label(on, name(&s)).clicked() {
                    Document::set_strategy(op, s);
                }
            }
        });
}

/// The tool, its preset, then the job's copy of it to adjust.
#[allow(clippy::too_many_arguments)]
fn tool(
    ui: &mut egui::Ui,
    doc: &mut Document,
    m: Measure,
    setup: &str,
    si: usize,
    index: usize,
    tool_name: Option<&str>,
    actions: &mut Vec<PropAction>,
) {
    form(ui, "op-tool", |ui| {
        widgets::field_label(ui, "Tool");
        ui.horizontal(|ui| {
            match tool_name {
                Some(t) => ui.strong(t),
                None => ui.colored_label(widgets::Level::Warn.color(), "no tool yet"),
            };
            if widgets::button(ui, Variant::Secondary, Size::Sm, None, "Select tool…").clicked() {
                actions.push(PropAction::PickTool(setup.to_string(), index));
            }
        });
        ui.end_row();
        if let Some(t) = tool_name {
            preset_row(ui, doc, si, index, t);
        }
    });
    let Some(t) = tool_name else {
        return;
    };
    preset_notes(ui, doc, si, index, t);
    if let Some(clash) = doc.number_clash(t) {
        let number = doc.job.tools.get(t).map_or(0, |x| x.number);
        let mut renumber = false;
        widgets::diagnostic(
            ui,
            widgets::Level::Err,
            &format!("'{clash}' is also T{number}"),
            |ui| {
                renumber =
                    widgets::button(ui, Variant::Secondary, Size::Sm, None, "Renumber").clicked();
            },
        );
        if renumber {
            let free = doc.free_number();
            if let Some(spec) = doc.job.tools.get_mut(t) {
                spec.number = free;
            }
        }
    }
    let users = doc
        .job
        .setups
        .iter()
        .flat_map(|s| &s.operations)
        .filter(|o| o.tool.as_deref() == Some(t))
        .count();
    ui.add_space(theme::SPACE_3);
    match doc.job.tools.get_mut(t) {
        Some(spec) => {
            paint_tool(ui, spec, m);
            ui.add_space(theme::SPACE_2);
            quiet(
                ui,
                &if users > 1 {
                    format!(
                        "This job's copy, used by {users} operations: a change here is a change to all of them."
                    )
                } else {
                    "This job's copy: the library is not changed.".to_string()
                },
            );
            egui::CollapsingHeader::new("Adjust this job's copy")
                .default_open(false)
                .show(ui, |ui| {
                    ui.push_id(("job-tool", t), |ui| tool_fields(ui, spec, m));
                });
        }
        None => {
            error(ui, &format!("'{t}' is not among the job's tools"));
        }
    }
}

/// Which of the tool's presets the operation's values came from: a row of the
/// tool form.
fn preset_row(ui: &mut egui::Ui, doc: &mut Document, si: usize, index: usize, tool: &str) {
    let presets = doc
        .job
        .tools
        .get(tool)
        .map(|t| t.presets.clone())
        .unwrap_or_default();
    let Some(op) = operation_mut(doc, si, index) else {
        return;
    };
    let none = "none";
    widgets::field_label(ui, "Preset");
    egui::ComboBox::from_id_salt(("op-preset", si, index))
        .selected_text(op.preset.as_deref().unwrap_or(none))
        .show_ui(ui, |ui| {
            for p in &presets {
                if ui
                    .selectable_label(op.preset.as_deref() == Some(p.name.as_str()), &p.name)
                    .clicked()
                {
                    op.apply_preset(p);
                }
            }
            if ui.selectable_label(op.preset.is_none(), none).clicked() {
                op.preset = None;
            }
        });
    ui.end_row();
}

/// What has been changed since the preset was applied, and a way to keep the
/// operation's values as a new preset of the job's copy of the tool.
fn preset_notes(ui: &mut egui::Ui, doc: &mut Document, si: usize, index: usize, tool: &str) {
    let presets = doc
        .job
        .tools
        .get(tool)
        .map(|t| t.presets.clone())
        .unwrap_or_default();
    let Some(op) = operation_mut(doc, si, index) else {
        return;
    };
    if presets.is_empty() {
        quiet(
            ui,
            "The tool has no presets. Save this operation's values as one.",
        );
    }
    if let Some(name) = op.preset.clone() {
        match presets.iter().find(|p| p.name == name) {
            Some(p) => {
                let changed = op.differs_from(p);
                if !changed.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(
                            widgets::Level::Warn.color(),
                            format!("Changed from '{name}': {}", changed.join(", ")),
                        );
                        if widgets::button(ui, Variant::Ghost, Size::Sm, None, "Reset to preset")
                            .clicked()
                        {
                            op.apply_preset(p);
                        }
                    });
                }
            }
            None => {
                quiet(
                    ui,
                    &format!("The tool no longer has a preset called '{name}'."),
                );
            }
        }
    }

    let id = ui.id().with(("save-preset", si, index));
    let mut draft: Option<String> = ui.data(|d| d.get_temp(id));
    let mut saved = None;
    match draft.as_mut() {
        None => {
            if widgets::button(
                ui,
                Variant::Ghost,
                Size::Sm,
                Some(Icon::Plus),
                "Save as preset",
            )
            .clicked()
            {
                draft = Some(String::new());
            }
        }
        Some(text) => {
            let mut done = false;
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(text).hint_text("preset name, e.g. Finishing"));
                let name = text.trim().to_string();
                let taken = presets.iter().any(|p| p.name == name);
                if ui
                    .add_enabled_ui(!name.is_empty() && !taken, |ui| {
                        widgets::button(ui, Variant::Primary, Size::Sm, None, "Save")
                    })
                    .inner
                    .clicked()
                {
                    saved = Some(op.as_preset(&name));
                    op.preset = Some(name);
                    done = true;
                }
                if widgets::button(ui, Variant::Ghost, Size::Sm, None, "Cancel").clicked() {
                    done = true;
                }
                if taken {
                    ui.colored_label(widgets::Level::Warn.color(), "that name is taken");
                }
            });
            if done {
                draft = None;
            }
        }
    }
    if let (Some(preset), Some(spec)) = (saved, doc.job.tools.get_mut(tool)) {
        spec.presets.push(preset);
    }
    ui.data_mut(|d| match draft {
        Some(text) => {
            d.insert_temp(id, text);
        }
        None => d.remove::<String>(id),
    });
}

fn geometry(
    ui: &mut egui::Ui,
    m: Measure,
    setup: &str,
    index: usize,
    op: &mut Operation,
    picks: Picks<'_>,
    actions: &mut Vec<PropAction>,
) {
    let drilling = matches!(op.strategy, StrategySpec::Drill { .. });
    quiet(
        ui,
        if drilling {
            "Pick the round edge at the top of each hole. Pick its bottom edge too to \
             drill to there; otherwise each hole is drilled to the Bottom height."
        } else {
            "With nothing picked, the whole part within the stock is machined. Picked faces \
             and edges keep the tool to their outline seen from above."
        },
    );
    ui.add_space(theme::SPACE_3);
    let g = &mut op.geometry;
    for (kind, title) in [(Kind::Face, "Faces"), (Kind::Edge, "Edges")] {
        let slot = match kind {
            Kind::Face => Slot::Faces {
                setup: setup.to_string(),
                op: index,
            },
            Kind::Edge => Slot::Edges {
                setup: setup.to_string(),
                op: index,
            },
        };
        let (list, found) = match kind {
            Kind::Face => (&mut g.faces, picks.shown.map(|s| s.faces.as_slice())),
            Kind::Edge => (&mut g.edges, picks.shown.map(|s| s.edges.as_slice())),
        };
        let mut clear = false;
        let mut asked = None;
        let any = !list.is_empty();
        widgets::card(
            ui,
            Some(title),
            |ui| {
                if picks.armed == Some(&slot) {
                    if widgets::button(ui, Variant::Primary, Size::Sm, None, "Done").clicked() {
                        asked = Some(PropAction::Disarm);
                    }
                } else if widgets::button(
                    ui,
                    Variant::Ghost,
                    Size::Sm,
                    Some(Icon::MousePointerClick),
                    "Select in view",
                )
                .clicked()
                {
                    asked = Some(PropAction::Arm(slot.clone()));
                }
                if any && widgets::button(ui, Variant::Ghost, Size::Sm, None, "Clear").clicked() {
                    clear = true;
                }
            },
            |ui| {
                if list.is_empty() {
                    quiet(ui, "None picked.");
                }
                pick_list(ui, kind, list, found.unwrap_or_default(), actions);
            },
        );
        actions.extend(asked);
        if clear {
            list.clear();
        }
        ui.add_space(theme::SPACE_3);
    }
    form(ui, "geometry", |ui| {
        if g.picked() && !drilling {
            widgets::field_label(ui, "Tool");
            let name = |c: Containment| match c {
                Containment::Center => "centre inside the boundary",
                Containment::Inside => "all of it inside",
                Containment::Outside => "reaching in from outside",
            };
            egui::ComboBox::from_id_salt("containment")
                .selected_text(name(g.containment))
                .show_ui(ui, |ui| {
                    for c in [
                        Containment::Center,
                        Containment::Inside,
                        Containment::Outside,
                    ] {
                        ui.selectable_value(&mut g.containment, c, name(c));
                    }
                });
            ui.end_row();
        }
        widgets::field_label(ui, "Margin");
        m.length_field(ui, &mut g.margin, 0.1, 1000.0, "");
        ui.end_row();
    });
    if g.picked() && !drilling {
        match picks.shown.and_then(|s| s.boundary.as_ref()) {
            Some(Err(why)) => {
                error(ui, why);
            }
            Some(Ok(())) => {
                quiet(ui, "The boundary is drawn in the view at the stock's top.");
            }
            None => {}
        }
    }
}

/// A list of picks, as the engine finds them: each can be taken out, and a row under
/// the pointer shows its pick in the view.
fn pick_list(
    ui: &mut egui::Ui,
    kind: Kind,
    list: &mut Vec<FeatureRef>,
    found: &[Result<String, String>],
    actions: &mut Vec<PropAction>,
) {
    let mut remove = None;
    for (i, r) in list.iter().enumerate() {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), theme::CONTROL_SM),
            egui::Sense::hover(),
        );
        let hovered = ui.rect_contains_pointer(rect);
        list_row_background(ui, rect, false, hovered);
        if hovered {
            actions.push(PropAction::HoverRow(kind, i));
        }
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(egui::vec2(theme::SPACE_3, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        match found.get(i) {
            Some(Ok(what)) => row.label(what),
            Some(Err(why)) => row.colored_label(widgets::Level::Err.color(), why),
            None => row.label(
                egui::RichText::new(format!("{} on '{}'", r.index, r.model))
                    .color(theme::egui_color(theme::TEXT_3)),
            ),
        };
        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if hovered && widgets::icon_button(ui, Icon::X, "take it out").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        list.remove(i);
    }
}

fn heights(ui: &mut egui::Ui, m: Measure, op: &mut Operation) {
    form(ui, "heights", |ui| {
        widgets::field_label(ui, "Clearance");
        height_field(ui, m, "clearance", &mut op.heights.clearance);
        ui.end_row();
        widgets::field_label(ui, "Top");
        optional_height(ui, m, "geo-top", &mut op.geometry.top, HeightRef::StockTop);
        ui.end_row();
        widgets::field_label(ui, "Bottom");
        optional_height(
            ui,
            m,
            "geo-bottom",
            &mut op.geometry.bottom,
            HeightRef::ModelBottom,
        );
        ui.end_row();
    });
}

fn passes(ui: &mut egui::Ui, m: Measure, op: &mut Operation, tool_rpm: Option<u32>) {
    form(ui, "passes", |ui| {
        match &mut op.strategy {
            StrategySpec::Parallel { angle } => {
                widgets::field_label(ui, "Pass angle");
                ui.add(
                    egui::DragValue::new(angle)
                        .speed(1.0)
                        .range(-360.0..=360.0)
                        .suffix("°"),
                );
                ui.end_row();
            }
            StrategySpec::ZlevelRough {
                stepdown,
                min_ring_length,
            } => {
                widgets::field_label(ui, "Stepdown");
                m.length_field(ui, stepdown, 0.05, 100.0, "");
                ui.end_row();
                widgets::field_label(ui, "Shortest ring");
                m.length_field(ui, min_ring_length, 0.1, 1000.0, "");
                ui.end_row();
            }
            StrategySpec::Drill { peck, dwell } => {
                widgets::field_label(ui, "Peck");
                ui.horizontal(|ui| {
                    let mut on = peck.is_some();
                    if ui.checkbox(&mut on, "").changed() {
                        *peck = on.then_some(2.0);
                    }
                    match peck.as_mut() {
                        Some(p) => {
                            m.length_field(ui, p, 0.05, 100.0, "");
                        }
                        None => {
                            quiet(ui, "all the way at once");
                        }
                    }
                });
                ui.end_row();
                widgets::field_label(ui, "Dwell");
                ui.horizontal(|ui| {
                    let mut on = dwell.is_some();
                    if ui.checkbox(&mut on, "").changed() {
                        *dwell = on.then_some(0.5);
                    }
                    match dwell.as_mut() {
                        Some(d) => {
                            ui.add(
                                egui::DragValue::new(d)
                                    .speed(0.05)
                                    .range(0.0..=60.0)
                                    .suffix(" s at the bottom"),
                            );
                        }
                        None => {
                            quiet(ui, "none");
                        }
                    }
                });
                ui.end_row();
            }
        }
        widgets::field_label(ui, "Stepover");
        m.length_field(ui, &mut op.passes.stepover, 0.05, 100.0, "");
        ui.end_row();
        widgets::field_label(ui, "Feed");
        m.feed_field(ui, &mut op.passes.feed, 50_000.0);
        ui.end_row();
        widgets::field_label(ui, "Spindle");
        ui.horizontal(|ui| {
            let mut own = op.passes.rpm.is_some();
            if ui.checkbox(&mut own, "").changed() {
                op.passes.rpm = own.then(|| tool_rpm.unwrap_or(18000));
            }
            match op.passes.rpm.as_mut() {
                Some(rpm) => {
                    ui.add(
                        egui::DragValue::new(rpm)
                            .range(1..=60000)
                            .suffix(" rev/min"),
                    );
                }
                None => {
                    quiet(
                        ui,
                        &match tool_rpm {
                            Some(r) => format!("the tool's, {r} rev/min"),
                            None => "the tool's".to_string(),
                        },
                    );
                }
            }
        });
        ui.end_row();
        widgets::field_label(ui, "Leave, sideways");
        m.length_field(ui, &mut op.passes.stock_to_leave.radial, 0.05, 50.0, "");
        ui.end_row();
        widgets::field_label(ui, "Leave, down");
        m.length_field(ui, &mut op.passes.stock_to_leave.axial, 0.05, 50.0, "");
        ui.end_row();
        widgets::field_label(ui, "Deepest step cut");
        m.length_field(ui, &mut op.passes.max_cut_step, 0.05, 50.0, "");
        ui.end_row();
    });
}

fn linking(ui: &mut egui::Ui, m: Measure, op: &mut Operation) {
    form(ui, "linking", |ui| {
        widgets::field_label(ui, "Retract feed");
        m.feed_field(ui, &mut op.linking.retract_feed, 50_000.0);
        ui.end_row();
        widgets::field_label(ui, "Plunge feed");
        m.feed_field(ui, &mut op.linking.plunge_feed, 50_000.0);
        ui.end_row();
        widgets::field_label(ui, "Reorder passes");
        ui.checkbox(&mut op.linking.reorder, "to shorten travel");
        ui.end_row();
        widgets::field_label(ui, "Reverse passes");
        ui.checkbox(
            &mut op.linking.allow_reverse,
            "while reordering (changes climb)",
        );
        ui.end_row();
    });
}

fn height_field(ui: &mut egui::Ui, m: Measure, id: &str, h: &mut Height) {
    let name = |r: HeightRef| match r {
        HeightRef::StockTop => "stock top",
        HeightRef::StockBottom => "stock bottom",
        HeightRef::ModelTop => "model top",
        HeightRef::ModelBottom => "model bottom",
        HeightRef::Absolute => "work zero",
    };
    ui.horizontal(|ui| {
        m.signed_length_field(ui, &mut h.offset, 0.1, 10_000.0, "");
        ui.label("from");
        egui::ComboBox::from_id_salt(id)
            .selected_text(name(h.reference))
            .show_ui(ui, |ui| {
                for r in [
                    HeightRef::StockTop,
                    HeightRef::StockBottom,
                    HeightRef::ModelTop,
                    HeightRef::ModelBottom,
                    HeightRef::Absolute,
                ] {
                    ui.selectable_value(&mut h.reference, r, name(r));
                }
            });
    });
}

fn optional_height(
    ui: &mut egui::Ui,
    m: Measure,
    id: &str,
    h: &mut Option<Height>,
    default: HeightRef,
) {
    ui.horizontal(|ui| {
        let mut on = h.is_some();
        if ui.checkbox(&mut on, "").changed() {
            *h = on.then_some(Height {
                reference: default,
                offset: 0.0,
            });
        }
        match h {
            Some(h) => height_field(ui, m, id, h),
            None => {
                quiet(ui, "the whole stock");
            }
        }
    });
}

fn operation_mut(doc: &mut Document, setup: usize, index: usize) -> Option<&mut Operation> {
    doc.job.setups.get_mut(setup)?.operations.get_mut(index)
}
