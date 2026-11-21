//! The faces a 3D operation machines and those it keeps clear of, under its
//! boundary on the Geometry tab: what is done to every face in no group, then
//! a card for each group of faces picked in the view, machined or avoided with
//! offsets of its own.

use std::cmp::Ordering;

use camcraft_core::job::{FeatureRef, Operation, SurfaceAction, SurfaceGroup, Surfaces};
use eframe::egui;

use super::geometry::{Swatch, each_alone, legend, pick_list, select_button};
use crate::help::topics::geometry as geo;
use crate::picking::slots::{PickList, Slot};
use crate::properties::{PropAction, ViewPicks};
use crate::ui::defaults::Defaults;
use crate::ui::measure::Measure;
use crate::ui::theme;
use crate::ui::widgets::{self, Icon, Size, Variant, form, hint};

/// A group as Add group starts it, doing `action` to `faces`, with no offsets
/// of its own.
pub(super) fn new_group(action: SurfaceAction, faces: Vec<FeatureRef>) -> SurfaceGroup {
    SurfaceGroup {
        action,
        radial: 0.0,
        axial: 0.0,
        faces,
    }
}

/// What operation `index` of `setup` does to the part's faces, `s`, its
/// settings against `d`: the model's action, a card for each group, and Add
/// group, which starts picking the new group's faces at once.
pub(super) fn surfaces(
    ui: &mut egui::Ui,
    (d, s): (&mut Defaults<Operation>, &mut Surfaces),
    m: Measure,
    (setup, index): (&str, usize),
    picks: ViewPicks<'_>,
    actions: &mut Vec<PropAction>,
) {
    let slot = |group: usize, groups: usize| Slot::GroupFaces {
        setup: setup.to_string(),
        op: index,
        group,
        groups,
    };
    widgets::section(ui, "Surfaces");
    form(ui, "surfaces", |ui| {
        d.row(ui, &geo::SURFACES_MODEL, "surfaces.model.action", |ui| {
            action_choice(ui, &mut s.model.action)
        });
    });

    let groups = s.groups.len();
    let mut remove = None;
    for (k, g) in s.groups.iter_mut().enumerate() {
        ui.add_space(theme::SPACE_3);
        let into = slot(k, groups);
        let found = picks.shown.and_then(|shown| shown.groups.get(k));
        let rows = each_alone(found.map(Vec::as_slice), g.faces.len());
        let mut asked = None;
        widgets::card_helped(
            ui,
            &format!("Group {}", k + 1),
            Some(&geo::SURFACE_GROUP),
            |ui| {
                if widgets::icon_button(ui, Icon::X, "remove the group").clicked() {
                    remove = Some(k);
                }
                asked = select_button(ui, &into, picks.armed);
            },
            |ui| {
                group_form(ui, d, m, k, g);
                ui.add_space(theme::SPACE_2);
                if g.faces.is_empty() {
                    hint(ui, "None picked.");
                }
                pick_list(ui, PickList::Group(k), &mut g.faces, &rows, actions);
            },
        );
        actions.extend(asked);
    }
    if let Some(gone) = remove {
        s.groups.remove(gone);
        // Picking into a group after it goes on into the same group, a place
        // earlier now; into the one taken out, it stops.
        if let Some(Slot::GroupFaces {
            setup: armed,
            op,
            group,
            ..
        }) = picks.armed
            && armed == setup
            && *op == index
        {
            let groups = s.groups.len();
            actions.push(match group.cmp(&gone) {
                Ordering::Less => PropAction::Arm(slot(*group, groups)),
                Ordering::Equal => PropAction::Disarm,
                Ordering::Greater => PropAction::Arm(slot(group - 1, groups)),
            });
        }
    }

    ui.add_space(theme::SPACE_3);
    let add = widgets::button(
        ui,
        Variant::Secondary,
        Size::Sm,
        Some(Icon::Plus),
        "Add group",
    );
    if add.clicked() {
        // Doing what the model does, a group would change nothing until given
        // offsets: it starts doing the other.
        let action = match s.model.action {
            SurfaceAction::Machine => SurfaceAction::Avoid,
            SurfaceAction::Avoid => SurfaceAction::Machine,
        };
        s.groups.push(new_group(action, Vec::new()));
        // A group with no faces is refused, so picking them starts at once,
        // as a boundary's does when it is set to what is picked.
        let groups = s.groups.len();
        actions.push(PropAction::Arm(slot(groups - 1, groups)));
    }

    if !s.is_whole() {
        ui.add_space(theme::SPACE_3);
        let mut items = vec![
            (Swatch::Tint(theme::MACHINED), "Faces machined"),
            (Swatch::Tint(theme::AVOIDED), "Faces avoided"),
        ];
        if matches!(picks.armed, Some(Slot::GroupFaces { .. })) {
            items.push((Swatch::Tint(theme::SELECTED), "The group being picked into"));
        }
        legend(ui, &items);
    }
}

/// Group `k`'s settings, `g`, against `d`: what is done to its faces, and its
/// offsets, named for what they are to a face machined or avoided.
fn group_form(
    ui: &mut egui::Ui,
    d: &mut Defaults<Operation>,
    m: Measure,
    k: usize,
    g: &mut SurfaceGroup,
) {
    let at = |setting: &str| format!("surfaces.group.{k}.{setting}");
    let (radial, axial) = match g.action {
        SurfaceAction::Machine => (&geo::EXTRA_WALLS, &geo::EXTRA_FLOORS),
        SurfaceAction::Avoid => (&geo::CLEARANCE_BESIDE, &geo::CLEARANCE_ABOVE),
    };
    form(ui, ("surface-group", k), |ui| {
        d.row(ui, &geo::SURFACE_ACTION, &at("action"), |ui| {
            action_choice(ui, &mut g.action)
        });
        d.row(ui, radial, &at("radial"), |ui| {
            m.length_field(ui, &mut g.radial, 0.05, 100.0, "")
        });
        d.row(ui, axial, &at("axial"), |ui| {
            m.length_field(ui, &mut g.axial, 0.05, 100.0, "")
        });
    });
}

/// Machine or avoid, side by side.
fn action_choice(ui: &mut egui::Ui, action: &mut SurfaceAction) -> egui::Response {
    widgets::segmented(
        ui,
        action,
        &[
            (SurfaceAction::Machine, "machine"),
            (SurfaceAction::Avoid, "avoid"),
        ],
    )
}
