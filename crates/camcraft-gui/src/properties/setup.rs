//! A setup's panel: what it machines and from what (Parts), how the part sits and
//! where zero is (Position), and what the machine is told (Post).

use camcraft_core::frame::{Axis, Orientation};
use eframe::egui;

use super::parts::{Names, names};
use super::{Picks, PropAction, cannot_generate, header, renamed, work_zero};
use crate::document::{Document, Selection};
use crate::measure::Measure;
use crate::slots::Slot;
use crate::theme;
use crate::widgets::{self, Size, Tab, Variant, form, hint};

/// Which tab a setup shows: the same for every setup, so moving between them
/// keeps the place.
const TAB: &str = "setup-tab";

pub(super) fn show(
    ui: &mut egui::Ui,
    doc: &mut Document,
    m: Measure,
    name: &str,
    picks: Picks<'_>,
    actions: &mut Vec<PropAction>,
) {
    if doc.setup(name).is_none() {
        doc.selection = Selection::Setups;
        return;
    }
    let id = ui.id().with(("setup", name));
    if let Some(to) = header(ui, "Setup", id, name) {
        let r = doc.rename_setup(name, &to);
        renamed(doc, r);
        return;
    }
    cannot_generate(ui, doc, name);

    // Picking in the view needs the setup laid out: a model and a stock to lay out.
    let laid_out = doc
        .setups
        .get(name)
        .and_then(|s| s.layout.as_ref())
        .is_some_and(|l| l.prepared.is_ok());
    let why_not = (!laid_out).then_some("give the setup a model and a stock first");
    let model_names: Vec<String> = doc.job.models.iter().map(|x| x.name.clone()).collect();
    let stock_names: Vec<String> = doc.job.stocks.iter().map(|x| x.name.clone()).collect();
    let origin_lost = picks
        .shown
        .and_then(|s| s.origin.as_ref())
        .is_some_and(Result::is_err);
    let Some(s) = doc.setup_mut(name) else {
        return;
    };

    let tabs = [
        Tab::new("Parts")
            .count(s.models.len())
            .flagged(s.models.is_empty() || s.stocks.is_empty()),
        Tab::new("Position").flagged(origin_lost),
        Tab::new("Post"),
    ];
    let picking = match picks.armed {
        Some(Slot::Models { .. } | Slot::Stocks { .. }) => Some(0),
        Some(Slot::WorkZero { .. }) => Some(1),
        _ => None,
    };
    match widgets::remembered_tabs(ui, egui::Id::new(TAB), &tabs, 0, picking) {
        0 => {
            names(
                ui,
                &Names {
                    id: id.with("models"),
                    title: "Models",
                    all: &model_names,
                    empty: "None yet: add some to machine them.",
                    pick: Some((
                        Slot::Models {
                            setup: name.to_string(),
                        },
                        why_not,
                    )),
                    more: Some((
                        "Add model files…",
                        PropAction::AddModels(Some(name.to_string())),
                    )),
                },
                &mut s.models,
                picks,
                actions,
            );
            ui.add_space(theme::SPACE_4);
            names(
                ui,
                &Names {
                    id: id.with("stocks"),
                    title: "Stock",
                    all: &stock_names,
                    empty: "None yet: the setup needs material to cut.",
                    pick: Some((
                        Slot::Stocks {
                            setup: name.to_string(),
                        },
                        why_not,
                    )),
                    more: None,
                },
                &mut s.stocks,
                picks,
                actions,
            );
            ui.add_space(theme::SPACE_3);
            hint(
                ui,
                "Each setup starts from its own stock: what one leaves is not carried to the next.",
            );
        }
        1 => {
            orientation(ui, &mut s.orientation);
            ui.add_space(theme::SPACE_4);
            work_zero::show(ui, m, name, &mut s.origin, picks, actions);
        }
        _ => {
            form(ui, "setup-post", |ui| {
                widgets::field_label(ui, "Work offset");
                egui::ComboBox::from_id_salt("wcs")
                    .selected_text(format!("G{}", s.wcs))
                    .show_ui(ui, |ui| {
                        for w in 54..=59 {
                            ui.selectable_value(&mut s.wcs, w, format!("G{w}"));
                        }
                    });
                ui.end_row();
                widgets::field_label(ui, "Post");
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("post")
                        .selected_text(&s.post)
                        .show_ui(ui, |ui| {
                            for p in camcraft_session::builtin_posts() {
                                ui.selectable_value(&mut s.post, (*p).to_string(), *p);
                            }
                        });
                    if widgets::button(ui, Variant::Secondary, Size::Sm, None, "File…")
                        .on_hover_text("a machine profile of your own")
                        .clicked()
                    {
                        actions.push(PropAction::ProfileFile(name.to_string()));
                    }
                });
                ui.end_row();
            });
        }
    }
}

/// Which way the part sits: the model directions that become the machine's Z and X.
fn orientation(ui: &mut egui::Ui, o: &mut Orientation) {
    let mut turned = None;
    let flipped = o.flipped();
    widgets::card(
        ui,
        Some("Orientation"),
        |ui| {
            if widgets::button(ui, Variant::Ghost, Size::Sm, None, "As modelled").clicked() {
                turned = Some(Orientation::default());
            }
            if widgets::button(ui, Variant::Ghost, Size::Sm, None, "Turn over")
                .on_hover_text("machine the other side: turn the part over about X")
                .clicked()
            {
                turned = Some(flipped);
            }
        },
        |ui| {
            form(ui, "orientation", |ui| {
                widgets::field_label(ui, "Up (Z)");
                axis_combo(ui, "orient-z", &mut o.z, None);
                ui.end_row();
                widgets::field_label(ui, "Along X");
                let z = o.z;
                axis_combo(ui, "orient-x", &mut o.x, Some(z));
                ui.end_row();
            });
        },
    );
    if let Some(t) = turned {
        *o = t;
    }
    if o.check().is_err() {
        // The Z choice took X's axis: move X to the next square one.
        if let Some(x) = Axis::ALL
            .into_iter()
            .find(|a| Orientation { z: o.z, x: *a }.check().is_ok())
        {
            o.x = x;
        }
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
