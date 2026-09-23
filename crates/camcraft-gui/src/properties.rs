//! The properties of whatever is selected in the job's tree, edited in place.
//!
//! Every field writes straight into the job. Nothing here runs anything: a change
//! only marks a setup's result out of date, and Generate runs it again. Lengths and
//! feeds are shown in the chosen units and kept in millimetres.

use camcraft_core::frame::{Axis, Orientation};
use camcraft_core::job::{
    Along, BoxOf, Height, HeightRef, LengthUnit, Operation, Origin, StockSpec, StrategySpec,
};
use eframe::egui;

use crate::document::{Document, Level, ModelSlot, Selection, Status};
use crate::measure::Measure;
use crate::tool_editor::{kind_name, paint_tool, tool_fields};

/// Something the panel asks the app to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropAction {
    /// Pick model files to add.
    AddModels,
    /// Find model `name`'s file.
    Locate(String),
    /// Pick a machine profile file for setup `setup`.
    ProfileFile(String),
    /// Pick a tool for operation `index` of setup `setup`.
    PickTool(String, usize),
    /// Run setup `setup`.
    Generate(String),
}

/// The panel for `doc`'s selection.
pub fn show(ui: &mut egui::Ui, doc: &mut Document, m: Measure) -> Vec<PropAction> {
    let mut actions = Vec::new();
    match doc.selection.clone() {
        Selection::Job => job(ui, doc, m),
        Selection::Models => {
            ui.heading("Models");
            ui.label("The design files this job machines. Each setup chooses which it includes.");
            if ui.button("Add models…").clicked() {
                actions.push(PropAction::AddModels);
            }
            let names: Vec<String> = doc.job.models.iter().map(|m| m.name.clone()).collect();
            list(ui, names, Selection::Model, doc);
        }
        Selection::Model(name) => model(ui, doc, m, &name, &mut actions),
        Selection::Stocks => {
            ui.heading("Stocks");
            ui.label(
                "Blocks of material, in the models' own coordinates. Setups that turn the \
                 part over see the same block from the other side.",
            );
            if ui.button("New stock").clicked() {
                doc.add_stock();
            }
            let names: Vec<String> = doc.job.stocks.iter().map(|s| s.name.clone()).collect();
            list(ui, names, Selection::Stock, doc);
        }
        Selection::Stock(name) => stock(ui, doc, m, &name),
        Selection::Setups => {
            ui.heading("Setups");
            ui.label(
                "Each setup holds the part one way on the machine, and runs from its own stock.",
            );
            if ui.button("New setup").clicked() {
                doc.add_setup();
            }
            let names: Vec<String> = doc.job.setups.iter().map(|s| s.name.clone()).collect();
            list(ui, names, Selection::Setup, doc);
        }
        Selection::Setup(name) => setup(ui, doc, m, &name, &mut actions),
        Selection::Operation(setup, index) => operation(ui, doc, m, &setup, index, &mut actions),
    }
    actions
}

/// A list of names that select what they name.
fn list(
    ui: &mut egui::Ui,
    names: Vec<String>,
    select: impl Fn(String) -> Selection,
    doc: &mut Document,
) {
    ui.add_space(6.0);
    for n in names {
        if ui.selectable_label(false, &n).clicked() {
            doc.selection = select(n);
        }
    }
}

/// A name, applied when editing finishes: the new one, if it changed.
fn name_field(ui: &mut egui::Ui, id: egui::Id, current: &str) -> Option<String> {
    let mut buffer = ui
        .data(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| current.to_string());
    let r = ui.add(
        egui::TextEdit::singleline(&mut buffer)
            .id(id)
            .desired_width(220.0),
    );
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        return (buffer.trim() != current).then_some(buffer);
    }
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buffer));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    None
}

fn grid(ui: &mut egui::Ui, id: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, body);
}

fn section(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(title)
        .default_open(true)
        .show(ui, body);
}

fn job(ui: &mut egui::Ui, doc: &mut Document, m: Measure) {
    ui.heading(doc.title());
    match &doc.path {
        Some(p) => ui.monospace(p.display().to_string()),
        None => ui.weak("not saved yet"),
    };
    ui.add_space(6.0);
    grid(ui, "job", |ui| {
        ui.label("Tolerance");
        ui.horizontal(|ui| {
            m.length_field(ui, &mut doc.job.tolerance, 0.001, 10.0, "");
        });
        ui.end_row();
        ui.label("");
        ui.weak("How closely paths follow the part; STEP models are meshed to a quarter of it.");
        ui.end_row();
        ui.label("Models");
        ui.label(doc.job.models.len().to_string());
        ui.end_row();
        ui.label("Stocks");
        ui.label(doc.job.stocks.len().to_string());
        ui.end_row();
        ui.label("Setups");
        ui.label(doc.job.setups.len().to_string());
        ui.end_row();
    });
    if doc.job.tolerance < 0.001 {
        doc.job.tolerance = 0.001;
    }

    ui.add_space(8.0);
    ui.strong("Tools in this job");
    ui.weak("Copies of library tools, chosen in each operation.");
    let used = doc.used_tools();
    egui::Grid::new("job-tools")
        .num_columns(4)
        .striped(true)
        .show(ui, |ui| {
            for (name, t) in &doc.job.tools {
                ui.label(format!("T{}", t.number));
                ui.label(name);
                ui.label(format!(
                    "{} Ø{}",
                    kind_name(t.kind),
                    m.length_trimmed(t.diameter)
                ));
                if used.contains(name) {
                    ui.label("");
                } else {
                    ui.weak("unused; dropped on save");
                }
                ui.end_row();
            }
        });
}

fn model(
    ui: &mut egui::Ui,
    doc: &mut Document,
    m: Measure,
    name: &str,
    actions: &mut Vec<PropAction>,
) {
    let Some(i) = doc.job.models.iter().position(|x| x.name == name) else {
        doc.selection = Selection::Models;
        return;
    };
    ui.heading("Model");
    let mut rename = None;
    grid(ui, "model", |ui| {
        ui.label("Name");
        rename = name_field(ui, ui.id().with(("model-name", name)), name);
        ui.end_row();
        let Some(def) = doc.job.models.get_mut(i) else {
            return;
        };
        ui.label("File");
        ui.vertical(|ui| {
            ui.monospace(&def.path);
            if ui.small_button("Find it elsewhere…").clicked() {
                actions.push(PropAction::Locate(name.to_string()));
            }
        });
        ui.end_row();
        let stl = std::path::Path::new(&def.path)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("stl"));
        if stl {
            ui.label("Units");
            ui.horizontal(|ui| {
                ui.radio_value(&mut def.units, Some(LengthUnit::Mm), "mm");
                ui.radio_value(&mut def.units, Some(LengthUnit::Inch), "inch");
            });
            ui.end_row();
            ui.label("");
            ui.weak("An STL does not say; check before cutting.");
            ui.end_row();
        }
    });
    if let Some(to) = rename
        && let Err(e) = doc.rename_model(name, &to)
    {
        doc.note(Level::Warn, e);
    }

    ui.add_space(6.0);
    match doc.models.get(name) {
        Some(ModelSlot::Ready(_, model)) => {
            let e = model.bounds.extent();
            ui.label(format!(
                "{} × {} × {}, {} triangles, {}",
                m.number(e.x, 1),
                m.number(e.y, 1),
                m.length(e.z, 1),
                model.mesh.len(),
                model.units
            ));
        }
        Some(ModelSlot::Loading(_)) => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("loading");
            });
        }
        Some(ModelSlot::Failed { why, .. }) => {
            ui.colored_label(ui.visuals().error_fg_color, why);
        }
        None => {
            ui.weak("not loaded");
        }
    }
    let users: Vec<String> = doc
        .job
        .setups
        .iter()
        .filter(|s| s.models.iter().any(|x| x == name))
        .map(|s| s.name.clone())
        .collect();
    ui.add_space(6.0);
    if users.is_empty() {
        ui.weak("No setup machines it.");
        if ui.button("Remove from the job").clicked()
            && let Err(e) = doc.remove_model(name)
        {
            doc.note(Level::Warn, e);
        }
    } else {
        ui.weak(format!("Machined by {}.", users.join(", ")));
    }
}

fn stock(ui: &mut egui::Ui, doc: &mut Document, m: Measure, name: &str) {
    let Some(i) = doc.job.stocks.iter().position(|x| x.name == name) else {
        doc.selection = Selection::Stocks;
        return;
    };
    ui.heading("Stock");
    let mut rename = None;
    let model_names: Vec<String> = doc.job.models.iter().map(|x| x.name.clone()).collect();
    let fit = all_model_bounds(doc);
    grid(ui, "stock", |ui| {
        ui.label("Name");
        rename = name_field(ui, ui.id().with(("stock-name", name)), name);
        ui.end_row();
        let Some(spec) = doc.job.stocks.get_mut(i).map(|s| &mut s.spec) else {
            return;
        };
        ui.label("Shape");
        let around = matches!(spec, StockSpec::AroundModels { .. });
        egui::ComboBox::from_id_salt("stock-kind")
            .selected_text(if around { "around models" } else { "fixed box" })
            .show_ui(ui, |ui| {
                if ui.selectable_label(around, "around models").clicked() && !around {
                    *spec = StockSpec::default();
                }
                if ui.selectable_label(!around, "fixed box").clicked() && around {
                    let (corner, size) = fit.map_or(([0.0; 3], [100.0, 100.0, 25.0]), |b| {
                        (b.min.to_array(), b.extent().to_array())
                    });
                    *spec = StockSpec::FixedBox { corner, size };
                }
            });
        ui.end_row();
        match spec {
            StockSpec::AroundModels {
                models,
                margin,
                below,
            } => {
                ui.label("Around");
                ui.vertical(|ui| {
                    if models.is_empty() {
                        ui.weak("the models of the setup using it");
                    }
                    for n in &model_names {
                        let mut on = models.contains(n);
                        if ui.checkbox(&mut on, n).changed() {
                            if on {
                                models.push(n.clone());
                            } else {
                                models.retain(|x| x != n);
                            }
                        }
                    }
                });
                ui.end_row();
                ui.label("Each side, X");
                m.length_field(ui, &mut margin[0], 0.1, 1000.0, "");
                ui.end_row();
                ui.label("Each side, Y");
                m.length_field(ui, &mut margin[1], 0.1, 1000.0, "");
                ui.end_row();
                ui.label("On top");
                m.length_field(ui, &mut margin[2], 0.1, 1000.0, "");
                ui.end_row();
                ui.label("Underneath");
                m.length_field(ui, below, 0.1, 1000.0, "");
                ui.end_row();
            }
            StockSpec::FixedBox { corner, size } => {
                for (label, v) in ["Corner X", "Corner Y", "Corner Z"]
                    .into_iter()
                    .zip(corner.iter_mut())
                {
                    ui.label(label);
                    m.signed_length_field(ui, v, 0.1, 10_000.0, "");
                    ui.end_row();
                }
                for (label, v) in ["Size X", "Size Y", "Size Z"]
                    .into_iter()
                    .zip(size.iter_mut())
                {
                    ui.label(label);
                    m.length_field(ui, v, 0.1, 10_000.0, "");
                    ui.end_row();
                }
                ui.label("");
                if ui
                    .add_enabled(fit.is_some(), egui::Button::new("Fit to the models"))
                    .clicked()
                    && let Some(b) = fit
                {
                    *corner = b.min.to_array();
                    *size = b.extent().to_array();
                }
                ui.end_row();
            }
        }
    });
    if let Some(to) = rename
        && let Err(e) = doc.rename_stock(name, &to)
    {
        doc.note(Level::Warn, e);
    }
    ui.add_space(6.0);
    let users: Vec<String> = doc
        .job
        .setups
        .iter()
        .filter(|s| s.stocks.iter().any(|x| x == name))
        .map(|s| s.name.clone())
        .collect();
    if users.is_empty() {
        ui.weak("No setup uses it.");
        if ui.button("Remove from the job").clicked()
            && let Err(e) = doc.remove_stock(name)
        {
            doc.note(Level::Warn, e);
        }
    } else {
        ui.weak(format!("Used by {}.", users.join(", ")));
    }
}

/// The box around every loaded model, in model coordinates.
fn all_model_bounds(doc: &Document) -> Option<camcraft_geom::mesh::Aabb> {
    doc.models
        .values()
        .filter_map(|s| s.model().map(|m| m.bounds))
        .reduce(|a, b| a.union(&b))
}

fn setup(
    ui: &mut egui::Ui,
    doc: &mut Document,
    m: Measure,
    name: &str,
    actions: &mut Vec<PropAction>,
) {
    let Some(i) = doc.setup_index(name) else {
        doc.selection = Selection::Setups;
        return;
    };
    ui.heading("Setup");
    status_line(ui, doc, name, actions);

    let mut rename = None;
    let model_names: Vec<String> = doc.job.models.iter().map(|x| x.name.clone()).collect();
    let stock_names: Vec<String> = doc.job.stocks.iter().map(|x| x.name.clone()).collect();
    grid(ui, "setup", |ui| {
        ui.label("Name");
        rename = name_field(ui, ui.id().with(("setup-name", name)), name);
        ui.end_row();
        let Some(s) = doc.job.setups.get_mut(i) else {
            return;
        };
        ui.label("Models");
        ui.vertical(|ui| {
            if model_names.is_empty() {
                ui.weak("none in the job yet");
            }
            for n in &model_names {
                let mut on = s.models.contains(n);
                if ui.checkbox(&mut on, n).changed() {
                    if on {
                        s.models.push(n.clone());
                    } else {
                        s.models.retain(|x| x != n);
                    }
                }
            }
        });
        ui.end_row();
        ui.label("Stock");
        ui.vertical(|ui| {
            for n in &stock_names {
                let mut on = s.stocks.contains(n);
                if ui.checkbox(&mut on, n).changed() {
                    if on {
                        s.stocks.push(n.clone());
                    } else {
                        s.stocks.retain(|x| x != n);
                    }
                }
            }
        });
        ui.end_row();
        ui.label("Work offset");
        egui::ComboBox::from_id_salt("wcs")
            .selected_text(format!("G{}", s.wcs))
            .show_ui(ui, |ui| {
                for w in 54..=59 {
                    ui.selectable_value(&mut s.wcs, w, format!("G{w}"));
                }
            });
        ui.end_row();
        ui.label("Post");
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("post")
                .selected_text(&s.post)
                .show_ui(ui, |ui| {
                    for p in camcraft_session::builtin_posts() {
                        ui.selectable_value(&mut s.post, (*p).to_string(), *p);
                    }
                });
            if ui.small_button("File…").clicked() {
                actions.push(PropAction::ProfileFile(name.to_string()));
            }
        });
        ui.end_row();
    });
    if let Some(to) = rename
        && let Err(e) = doc.rename_setup(name, &to)
    {
        doc.note(Level::Warn, e);
        return;
    }
    let Some(s) = doc.job.setups.get_mut(i) else {
        return;
    };

    section(ui, "Orientation", |ui| {
        ui.weak("Which way the part sits: the model directions that become the machine's Z and X.");
        grid(ui, "orientation", |ui| {
            ui.label("Up (Z)");
            axis_combo(ui, "orient-z", &mut s.orientation.z, None);
            ui.end_row();
            ui.label("Along X");
            let z = s.orientation.z;
            axis_combo(ui, "orient-x", &mut s.orientation.x, Some(z));
            ui.end_row();
        });
        if s.orientation.check().is_err() {
            // The Z choice took X's axis: move X to the next square one.
            if let Some(x) = Axis::ALL.into_iter().find(|a| {
                Orientation {
                    z: s.orientation.z,
                    x: *a,
                }
                .check()
                .is_ok()
            }) {
                s.orientation.x = x;
            }
        }
        ui.horizontal(|ui| {
            if ui
                .button("Turn over")
                .on_hover_text("machine the other side: turn the part over about X")
                .clicked()
            {
                s.orientation = s.orientation.flipped();
            }
            if ui.button("As modelled").clicked() {
                s.orientation = Orientation::default();
            }
        });
    });

    section(ui, "Work zero", |ui| origin(ui, m, &mut s.origin));

    ui.add_space(6.0);
    ui.weak("Each setup starts from its own stock: what one leaves is not carried to the next.");

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.strong("Operations");
        if ui.small_button("+ parallel").clicked() {
            doc.add_operation(name, StrategySpec::Parallel { angle: 0.0 });
        }
        if ui.small_button("+ z-level rough").clicked() {
            doc.add_operation(
                name,
                StrategySpec::ZlevelRough {
                    stepdown: 2.0,
                    min_ring_length: 3.0,
                },
            );
        }
    });
    let ops: Vec<String> = doc
        .job
        .setups
        .get(i)
        .map(|s| s.operations.iter().map(|o| o.name.clone()).collect())
        .unwrap_or_default();
    for (k, op) in ops.iter().enumerate() {
        if ui
            .selectable_label(false, format!("{}. {op}", k + 1))
            .clicked()
        {
            doc.selection = Selection::Operation(name.to_string(), k);
        }
    }
}

/// Generate, and where the setup stands.
fn status_line(ui: &mut egui::Ui, doc: &Document, name: &str, actions: &mut Vec<PropAction>) {
    let index = doc.setup_index(name);
    let why_not = index.and_then(|i| doc.cannot_run(i));
    ui.horizontal(|ui| {
        let status = doc.status(name);
        let running = status == Status::Running;
        if ui
            .add_enabled(why_not.is_none() && !running, egui::Button::new("Generate"))
            .on_disabled_hover_text(why_not.clone().unwrap_or_default())
            .clicked()
        {
            actions.push(PropAction::Generate(name.to_string()));
        }
        if running {
            ui.spinner();
        }
        ui.label(crate::browser::status_text(status));
    });
    if let Some(why) = why_not {
        ui.colored_label(ui.visuals().warn_fg_color, why);
    }
}

fn axis_combo(ui: &mut egui::Ui, id: &str, value: &mut Axis, square_to: Option<Axis>) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(value.label())
        .show_ui(ui, |ui| {
            for a in Axis::ALL {
                let ok = square_to.is_none_or(|z| Orientation { z, x: a }.check().is_ok());
                ui.add_enabled_ui(ok, |ui| {
                    ui.selectable_value(value, a, a.label());
                });
            }
        });
}

fn along_combo(ui: &mut egui::Ui, id: &str, value: &mut Along, names: [&str; 3]) {
    let label = |a: Along| match a {
        Along::Min => names[0],
        Along::Center => names[1],
        Along::Max => names[2],
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(label(*value))
        .show_ui(ui, |ui| {
            for a in [Along::Min, Along::Center, Along::Max] {
                ui.selectable_value(value, a, label(a));
            }
        });
}

fn origin(ui: &mut egui::Ui, m: Measure, origin: &mut Origin) {
    let kind = match origin {
        Origin::ModelOrigin { .. } => "the model's own zero",
        Origin::BoxPoint {
            of: BoxOf::Stock, ..
        } => "a point on the stock",
        Origin::BoxPoint {
            of: BoxOf::Model, ..
        } => "a point on the model's box",
    };
    let offset = match origin {
        Origin::ModelOrigin { offset } | Origin::BoxPoint { offset, .. } => *offset,
    };
    egui::ComboBox::from_id_salt("origin-kind")
        .selected_text(kind)
        .show_ui(ui, |ui| {
            if ui.selectable_label(false, "the model's own zero").clicked() {
                *origin = Origin::ModelOrigin { offset };
            }
            for (of, text) in [
                (BoxOf::Stock, "a point on the stock"),
                (BoxOf::Model, "a point on the model's box"),
            ] {
                if ui.selectable_label(false, text).clicked() {
                    *origin = Origin::BoxPoint {
                        of,
                        x: Along::Center,
                        y: Along::Center,
                        z: Along::Max,
                        offset,
                    };
                }
            }
        });
    grid(ui, "origin", |ui| {
        if let Origin::BoxPoint { x, y, z, .. } = origin {
            ui.label("X");
            along_combo(ui, "origin-x", x, ["left", "middle", "right"]);
            ui.end_row();
            ui.label("Y");
            along_combo(ui, "origin-y", y, ["front", "middle", "back"]);
            ui.end_row();
            ui.label("Z");
            along_combo(ui, "origin-z", z, ["bottom", "middle", "top"]);
            ui.end_row();
        }
        let offset = match origin {
            Origin::ModelOrigin { offset } | Origin::BoxPoint { offset, .. } => offset,
        };
        for (label, v) in ["Offset X", "Offset Y", "Offset Z"]
            .into_iter()
            .zip(offset.iter_mut())
        {
            ui.label(label);
            m.signed_length_field(ui, v, 0.1, 10_000.0, "");
            ui.end_row();
        }
    });
    ui.weak("Drawn in the view as red X, green Y and blue Z.");
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
    ui.vertical(|ui| {
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
                ui.weak("the whole stock");
            }
        }
    });
}

fn operation(
    ui: &mut egui::Ui,
    doc: &mut Document,
    m: Measure,
    setup: &str,
    index: usize,
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
    ui.heading("Operation");
    ui.weak(format!("in setup '{setup}'"));
    status_line(ui, doc, setup, actions);

    let Some(op) = operation_mut(doc, si, index) else {
        return;
    };
    let mut new_name = None;
    grid(ui, "operation", |ui| {
        ui.label("Name");
        new_name = name_field(ui, ui.id().with(("op-name", setup, index)), &op.name);
        ui.end_row();
        ui.label("Strategy");
        let parallel = matches!(op.strategy, StrategySpec::Parallel { .. });
        egui::ComboBox::from_id_salt("strategy")
            .selected_text(if parallel {
                "parallel"
            } else {
                "z-level rough"
            })
            .show_ui(ui, |ui| {
                if ui.selectable_label(parallel, "parallel").clicked() {
                    Document::set_strategy(op, StrategySpec::Parallel { angle: 0.0 });
                }
                if ui.selectable_label(!parallel, "z-level rough").clicked() {
                    Document::set_strategy(
                        op,
                        StrategySpec::ZlevelRough {
                            stepdown: 2.0,
                            min_ring_length: 3.0,
                        },
                    );
                }
            });
        ui.end_row();
        match &mut op.strategy {
            StrategySpec::Parallel { angle } => {
                ui.label("Pass angle");
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
                ui.label("Stepdown");
                m.length_field(ui, stepdown, 0.05, 100.0, "");
                ui.end_row();
                ui.label("Shortest ring");
                m.length_field(ui, min_ring_length, 0.1, 1000.0, "");
                ui.end_row();
            }
        }
    });
    if let Some(to) = new_name {
        let to = to.trim().to_string();
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

    // The tool, then the job's copy of it to adjust.
    let tool_name = operation_mut(doc, si, index).and_then(|o| o.tool.clone());
    section(ui, "Tool", |ui| {
        ui.horizontal(|ui| {
            match &tool_name {
                Some(t) => ui.strong(t),
                None => ui.colored_label(ui.visuals().warn_fg_color, "no tool yet"),
            };
            if ui.button("Select tool…").clicked() {
                actions.push(PropAction::PickTool(setup.to_string(), index));
            }
        });
        if let Some(t) = &tool_name {
            if let Some(clash) = doc.number_clash(t) {
                ui.horizontal(|ui| {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!(
                            "'{clash}' is also T{}",
                            doc.job.tools.get(t).map_or(0, |x| x.number)
                        ),
                    );
                    if ui.small_button("Renumber").clicked() {
                        let free = doc.free_number();
                        if let Some(spec) = doc.job.tools.get_mut(t) {
                            spec.number = free;
                        }
                    }
                });
            }
            let users = doc
                .job
                .setups
                .iter()
                .flat_map(|s| &s.operations)
                .filter(|o| o.tool.as_deref() == Some(t.as_str()))
                .count();
            match doc.job.tools.get_mut(t) {
                Some(spec) => {
                    if users > 1 {
                        ui.weak(format!(
                            "This job's copy, used by {users} operations: a change here is a change to all of them."
                        ));
                    } else {
                        ui.weak("This job's copy: the library is not changed.");
                    }
                    paint_tool(ui, spec, m);
                    egui::CollapsingHeader::new("Adjust this job's copy")
                        .default_open(false)
                        .show(ui, |ui| {
                            ui.push_id(("job-tool", t), |ui| tool_fields(ui, spec, m));
                        });
                }
                None => {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("'{t}' is not among the job's tools"),
                    );
                }
            }
        }
    });

    let Some(op) = operation_mut(doc, si, index) else {
        return;
    };
    section(ui, "Geometry", |ui| {
        grid(ui, "geometry", |ui| {
            ui.label("Margin");
            m.length_field(ui, &mut op.geometry.margin, 0.1, 1000.0, "");
            ui.end_row();
            ui.label("Top");
            optional_height(ui, m, "geo-top", &mut op.geometry.top, HeightRef::StockTop);
            ui.end_row();
            ui.label("Bottom");
            optional_height(
                ui,
                m,
                "geo-bottom",
                &mut op.geometry.bottom,
                HeightRef::ModelBottom,
            );
            ui.end_row();
        });
    });
    section(ui, "Heights", |ui| {
        grid(ui, "heights", |ui| {
            ui.label("Clearance");
            height_field(ui, m, "clearance", &mut op.heights.clearance);
            ui.end_row();
        });
    });
    section(ui, "Passes", |ui| {
        grid(ui, "passes", |ui| {
            ui.label("Stepover");
            m.length_field(ui, &mut op.passes.stepover, 0.05, 100.0, "");
            ui.end_row();
            ui.label("Feed");
            m.feed_field(ui, &mut op.passes.feed, 50_000.0);
            ui.end_row();
            ui.label("Leave, sideways");
            m.length_field(ui, &mut op.passes.stock_to_leave.radial, 0.05, 50.0, "");
            ui.end_row();
            ui.label("Leave, down");
            m.length_field(ui, &mut op.passes.stock_to_leave.axial, 0.05, 50.0, "");
            ui.end_row();
            ui.label("Deepest step cut");
            m.length_field(ui, &mut op.passes.max_cut_step, 0.05, 50.0, "");
            ui.end_row();
        });
    });
    section(ui, "Linking", |ui| {
        grid(ui, "linking", |ui| {
            ui.label("Retract feed");
            m.feed_field(ui, &mut op.linking.retract_feed, 50_000.0);
            ui.end_row();
            ui.label("Plunge feed");
            m.feed_field(ui, &mut op.linking.plunge_feed, 50_000.0);
            ui.end_row();
            ui.label("Reorder passes");
            ui.checkbox(&mut op.linking.reorder, "to shorten travel");
            ui.end_row();
            ui.label("Reverse passes");
            ui.checkbox(
                &mut op.linking.allow_reverse,
                "while reordering (changes climb)",
            );
            ui.end_row();
        });
    });
}

fn operation_mut(doc: &mut Document, setup: usize, index: usize) -> Option<&mut Operation> {
    doc.job.setups.get_mut(setup)?.operations.get_mut(index)
}
