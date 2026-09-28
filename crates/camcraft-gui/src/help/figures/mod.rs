//! The sketches in the help's popups, painted rather than pictures: sharp at any
//! scale, in the theme's colours, and drawn with the few pieces in [`sketch`].
//!
//! A figure that serves several fields says which one it is for, and that one's
//! dimension is drawn in amber: one sketch of a tool lights its stickout for
//! Stickout, its diameter for Diameter.

mod geometry;
mod linking;
mod passes;
mod sketch;
mod tool;

use eframe::egui;

use crate::ui::theme;
use crate::view::viewport::PlaneKind;

/// A sketch for a topic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Figure {
    /// Side-by-side passes and the distance between them.
    Stepover,
    /// The width of cut adaptive clearing holds, from above.
    OptimalLoad,
    /// Levels down a wall and the depth of each.
    Stepdown,
    /// Passes at an angle to X, from above.
    PassAngle,
    /// Climb and conventional milling, from above.
    Climb,
    /// The bite each tooth takes, from above.
    ChipLoad,
    /// A drill pecking down a hole.
    Peck,
    /// Facing passes past the stock.
    Facing(Facing),
    /// Stock left on walls and floors.
    Leave(LeaveOn),
    /// Tabs left across a cut.
    Tab(Tab),
    /// Lifting only clear of the material, or up to the retract height.
    Retraction,
    /// Where rapid down ends and feed begins.
    SafeDistance,
    /// A short link made without lifting, and a long one that lifts.
    StayDown,
    /// The vertical arcs a parallel pass rolls in and out on.
    LeadRadius,
    /// A profile's lead onto its ring.
    Lead(Lead),
    /// A ramp down into material.
    RampAngle,
    /// Helix, plunge and ramp entries.
    Entry,
    /// A helical entry.
    Helix(Helix),
    /// An operation's heights as planes through the stock.
    Height(PlaneKind),
    /// Where the tool may go against a boundary.
    Containment,
    /// A boundary moved out by an offset.
    BoundaryOffset,
    /// A profile cut outside, inside, or on its contour.
    ProfileSide,
    /// A tool's dimensions.
    ToolDim(ToolDim),
    /// The shapes of cutter.
    ToolKinds,
    /// A stock box's margins round the models.
    Margin(Margin),
}

/// Which of facing's distances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facing {
    /// How far past the stock is faced.
    StockOffset,
    /// How far each pass runs on past that.
    PassExtension,
}

/// Where stock is left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaveOn {
    /// Walls and floors.
    Both,
    /// Walls.
    Walls,
    /// Floors.
    Floors,
}

/// Which of a tab's settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// Tabs in general.
    All,
    /// Their two shapes.
    Shape,
    /// Their width.
    Width,
    /// Their height.
    Height,
    /// How far apart they are.
    Spacing,
}

/// Which part of a profile's lead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lead {
    /// All of it.
    All,
    /// The arc down onto it.
    Vertical,
    /// The straight before the level arc.
    Straight,
    /// The level arc's radius.
    Arc,
    /// How far round the level arc turns.
    Sweep,
}

/// Which of a helix's sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Helix {
    /// Its diameter.
    Diameter,
    /// How far it goes down a lap.
    PerLap,
}

/// Which of a tool's dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolDim {
    /// The cutting diameter.
    Diameter,
    /// A bullnose's corner radius.
    Corner,
    /// A V-bit's or drill's included angle.
    Angle,
    /// A V-bit's flat tip.
    Tip,
    /// The cutting length.
    Flutes,
    /// Tip to full shank diameter.
    Shoulder,
    /// The shank's diameter.
    Shank,
    /// Out of the holder.
    Stickout,
    /// The whole tool.
    Overall,
    /// The holder.
    Holder,
}

/// Which of a stock box's margins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Margin {
    /// Along X, each side.
    X,
    /// Along Y, each side.
    Y,
    /// On top.
    Top,
    /// Underneath.
    Under,
}

/// How tall a figure is in its popup.
const HEIGHT: f32 = 120.0;

impl Figure {
    /// The figure, the popup's width and [`HEIGHT`] tall, on the view's dark
    /// ground.
    pub fn show(self, ui: &mut egui::Ui) {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), HEIGHT),
            egui::Sense::hover(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, theme::RADIUS_SM, theme::egui_color(theme::BG));
        self.paint(&painter, rect.shrink(8.0));
    }

    /// The figure, fitted into `rect`.
    pub fn paint(self, painter: &egui::Painter, rect: egui::Rect) {
        match self {
            Figure::Stepover => passes::stepover(painter, rect),
            Figure::OptimalLoad => passes::optimal_load(painter, rect),
            Figure::Stepdown => passes::stepdown(painter, rect),
            Figure::PassAngle => passes::pass_angle(painter, rect),
            Figure::Climb => passes::climb(painter, rect),
            Figure::ChipLoad => passes::chip_load(painter, rect),
            Figure::Peck => passes::peck(painter, rect),
            Figure::Facing(f) => passes::facing(painter, rect, f),
            Figure::Leave(on) => passes::leave(painter, rect, on),
            Figure::Tab(t) => passes::tabs(painter, rect, t),
            Figure::Retraction => linking::retraction(painter, rect),
            Figure::SafeDistance => linking::safe_distance(painter, rect),
            Figure::StayDown => linking::stay_down(painter, rect),
            Figure::LeadRadius => linking::lead_radius(painter, rect),
            Figure::Lead(l) => linking::lead(painter, rect, l),
            Figure::RampAngle => linking::ramp_angle(painter, rect),
            Figure::Entry => linking::entry(painter, rect),
            Figure::Helix(h) => linking::helix(painter, rect, h),
            Figure::Height(k) => geometry::heights(painter, rect, k),
            Figure::Containment => geometry::containment(painter, rect),
            Figure::BoundaryOffset => geometry::boundary_offset(painter, rect),
            Figure::ProfileSide => geometry::profile_side(painter, rect),
            Figure::ToolDim(d) => tool::dimensions(painter, rect, d),
            Figure::ToolKinds => tool::kinds(painter, rect),
            Figure::Margin(m) => geometry::margins(painter, rect, m),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every figure there is, each way it can be lit.
    fn every() -> Vec<Figure> {
        let mut all = vec![
            Figure::Stepover,
            Figure::OptimalLoad,
            Figure::Stepdown,
            Figure::PassAngle,
            Figure::Climb,
            Figure::ChipLoad,
            Figure::Peck,
            Figure::Retraction,
            Figure::SafeDistance,
            Figure::StayDown,
            Figure::LeadRadius,
            Figure::RampAngle,
            Figure::Entry,
            Figure::Containment,
            Figure::BoundaryOffset,
            Figure::ProfileSide,
            Figure::ToolKinds,
        ];
        all.extend([Facing::StockOffset, Facing::PassExtension].map(Figure::Facing));
        all.extend([LeaveOn::Both, LeaveOn::Walls, LeaveOn::Floors].map(Figure::Leave));
        all.extend([Tab::All, Tab::Shape, Tab::Width, Tab::Height, Tab::Spacing].map(Figure::Tab));
        all.extend(
            [
                Lead::All,
                Lead::Vertical,
                Lead::Straight,
                Lead::Arc,
                Lead::Sweep,
            ]
            .map(Figure::Lead),
        );
        all.extend([Helix::Diameter, Helix::PerLap].map(Figure::Helix));
        all.extend(
            [
                PlaneKind::Clearance,
                PlaneKind::Retract,
                PlaneKind::Top,
                PlaneKind::Bottom,
            ]
            .map(Figure::Height),
        );
        all.extend(
            [
                ToolDim::Diameter,
                ToolDim::Corner,
                ToolDim::Angle,
                ToolDim::Tip,
                ToolDim::Flutes,
                ToolDim::Shoulder,
                ToolDim::Shank,
                ToolDim::Stickout,
                ToolDim::Overall,
                ToolDim::Holder,
            ]
            .map(Figure::ToolDim),
        );
        all.extend([Margin::X, Margin::Y, Margin::Top, Margin::Under].map(Figure::Margin));
        all
    }

    /// Each figure draws something, and all of it inside its rect: a sketch
    /// that spills out would be cut off at the popup's edge. Words may overhang
    /// by a point or two where their glyphs' bearings reach.
    #[test]
    fn every_figure_draws_inside_its_rect() {
        let rect = egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(244.0, 104.0));
        let mut outside = Vec::new();
        for figure in every() {
            let ctx = egui::Context::default();
            // A layer of its own, so nothing else drawn is counted.
            let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("figure"));
            let mut shapes = Vec::new();
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                let painter = egui::Painter::new(ui.ctx().clone(), layer, egui::Rect::EVERYTHING);
                figure.paint(&painter, rect);
                shapes = ui.ctx().graphics(|g| {
                    g.get(layer)
                        .map(|l| {
                            l.all_entries()
                                .map(|s| s.shape.visual_bounding_rect())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                });
            });
            // The fonts it laid out, which no renderer is here to take.
            out.textures_delta.clear();
            assert!(!shapes.is_empty(), "{figure:?} draws nothing");
            outside.extend(
                shapes
                    .iter()
                    .filter(|b| b.is_positive() && !rect.expand(3.0).contains_rect(**b))
                    .map(|b| format!("{figure:?} draws {b:?}")),
            );
        }
        assert!(
            outside.is_empty(),
            "outside {rect:?}:\n{}",
            outside.join("\n")
        );
    }
}
