//! Help for every input, as Fusion gives it: hovering a field's label or the
//! field itself pops up its name, a sentence or two on what it does, and often a
//! small sketch.
//!
//! Every field label in the app is made from a [`Topic`]
//! ([`widgets::field_label`], [`widgets::row`]), so none can be left without
//! one; anything else that edits takes one with [`Help::help`]. The topics are
//! in [`topics`], one file per panel, and the sketches in [`figures`].

pub mod figures;
pub mod topics;

use eframe::egui;

pub use figures::Figure;

use crate::ui::theme;
use crate::ui::widgets;

/// What a field is, for its popup.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Topic {
    /// The field's name: its label, and the popup's heading.
    pub title: &'static str,
    /// What it does, in a sentence or two.
    pub text: &'static str,
    /// A sketch, where one says more than the words.
    pub figure: Option<Figure>,
}

/// How wide a popup is: wide enough for a sketch, narrow enough to read at a
/// glance.
pub const WIDTH: f32 = 280.0;

/// A topic's popup, shown on hovering what it is attached to.
pub trait Help {
    /// Show `topic` while this is hovered, whether it is enabled or not: a field
    /// that cannot be changed is still worth explaining.
    fn help(self, topic: &Topic) -> Self;
}

impl Help for egui::Response {
    fn help(self, topic: &Topic) -> Self {
        let mut tip = if self.enabled() {
            egui::Tooltip::for_enabled(&self)
        } else {
            egui::Tooltip::for_disabled(&self)
        };
        tip.popup = tip.popup.frame(frame(&self.ctx)).width(WIDTH);
        tip.show(|ui| popup(ui, topic));
        self
    }
}

/// Show `topic` while the pointer is anywhere over `rect`, over the widgets
/// already there too: a label and its field hovered as one, as Fusion's are.
/// It senses only hover, so what is under it still takes clicks and drags.
pub fn over(ui: &mut egui::Ui, rect: egui::Rect, id: egui::Id, topic: &Topic) {
    ui.interact(rect, id, egui::Sense::hover()).help(topic);
}

/// `topic`'s popup drawn where `ui` is, rather than on hover: for laying every
/// one out to look at.
#[cfg(all(test, feature = "ui-snapshots"))]
pub fn show_in_place(ui: &mut egui::Ui, topic: &Topic) {
    frame(ui.ctx()).show(ui, |ui| {
        ui.set_width(WIDTH - 20.0);
        popup(ui, topic);
    });
}

/// The popup's frame, as `Tooltip.jsx` draws it: raised grey, a hairline ring,
/// small corners and the popover's shadow.
fn frame(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::new()
        .fill(theme::egui_color(theme::SURFACE_RAISED))
        .stroke(egui::Stroke::new(
            1.0,
            theme::egui_color(theme::BORDER_STRONG),
        ))
        .corner_radius(theme::RADIUS_SM)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .shadow(ctx.global_style().visuals.popup_shadow)
}

/// What a popup says: the name, what it does, and the sketch.
fn popup(ui: &mut egui::Ui, topic: &Topic) {
    ui.spacing_mut().item_spacing.y = theme::SPACE_2;
    ui.label(
        widgets::medium(topic.title, theme::TEXT_BASE).color(theme::egui_color(theme::TEXT_1)),
    );
    ui.add(
        egui::Label::new(
            egui::RichText::new(topic.text)
                .size(theme::TEXT_SM)
                .color(theme::egui_color(theme::TEXT_2)),
        )
        .wrap(),
    );
    if let Some(figure) = topic.figure {
        ui.add_space(theme::SPACE_2);
        figure.show(ui);
    }
}
