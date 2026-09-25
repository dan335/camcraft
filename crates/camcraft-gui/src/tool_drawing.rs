//! A tool drawn side on, to scale: cutter, shank and holder.
//!
//! The drawing is pure geometry in millimetres, with the tip at the origin and +Z
//! up the tool, so the proportions can be tested without a screen; [`paint_tool`]
//! puts it on one. Dimensions the tool does not give are assumed from its diameter
//! and marked as assumed, so the picture never claims more than the tool says.

use camcraft_core::job::{DRILL_POINT, ToolKind, ToolSpec};
use eframe::egui;

use crate::measure::Measure;
use crate::theme;
use crate::widgets;

/// What a region of the drawing is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// The cutting length.
    Flutes,
    /// The body between the flutes and the shank: shoulder and shank together.
    Shank,
    /// The holder, above its face.
    Holder,
}

/// One closed outline, as (radius, z) points, drawn mirrored about the axis.
#[derive(Debug, Clone, PartialEq)]
pub struct Outline {
    /// Which part it is.
    pub part: Part,
    /// The right-hand half of the outline, from bottom to top; the drawing mirrors it.
    pub half: Vec<(f64, f64)>,
}

/// The whole drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawing {
    /// Outlines, from the tip up.
    pub outlines: Vec<Outline>,
    /// Height of the holder's face above the tip: the stickout.
    pub holder_face: f64,
    /// The widest anything is, in radius.
    pub max_radius: f64,
    /// The top of the drawing.
    pub height: f64,
    /// Names of the dimensions that were assumed rather than given.
    pub assumed: Vec<&'static str>,
}

/// Steps along a rounded corner.
const ARC_STEPS: u32 = 8;

/// The cutting end of `tool`, as (radius, z) points from the tip on the axis out
/// to where it reaches full width: a flat bottom and a quarter-round corner for
/// the end mills, a flat tip and a straight flank for a cone. Both the side
/// drawing and the 3D cutter are made from it.
pub fn tip_profile(tool: &ToolSpec) -> Vec<(f64, f64)> {
    let r = if tool.diameter.is_finite() && tool.diameter > 0.0 {
        tool.diameter / 2.0
    } else {
        0.5
    };
    let mut out = vec![(0.0, 0.0)];
    if matches!(tool.kind, ToolKind::Cone | ToolKind::Drill) {
        let (tip, angle) = match tool.kind {
            ToolKind::Drill => (0.0, tool.angle.unwrap_or(DRILL_POINT)),
            _ => (tool.tip_diameter / 2.0, tool.angle.unwrap_or(90.0)),
        };
        let tip = tip.clamp(0.0, r);
        let half = (angle / 2.0).clamp(1.0, 89.0).to_radians();
        if tip > 0.0 {
            out.push((tip, 0.0));
        }
        out.push((r, (r - tip) / half.tan()));
        return out;
    }
    let corner = match tool.kind {
        ToolKind::Ball => r,
        ToolKind::Bullnose => tool.corner.clamp(0.0, r),
        ToolKind::Flat | ToolKind::Cone | ToolKind::Drill => 0.0,
    };
    if corner < r {
        out.push((r - corner, 0.0));
    }
    if corner > 0.0 {
        for i in 1..=ARC_STEPS {
            let t = std::f64::consts::FRAC_PI_2 * f64::from(i) / f64::from(ARC_STEPS);
            out.push((r - corner + corner * t.sin(), corner * (1.0 - t.cos())));
        }
    }
    out
}

/// Draw `tool`.
pub fn draw(tool: &ToolSpec) -> Drawing {
    let d = if tool.diameter.is_finite() && tool.diameter > 0.0 {
        tool.diameter
    } else {
        1.0
    };
    let r = d / 2.0;
    let mut assumed = Vec::new();
    let mut or_assume = |value: Option<f64>, name, default: f64| match value {
        Some(v) if v.is_finite() && v > 0.0 => v,
        _ => {
            assumed.push(name);
            default
        }
    };
    let flutes = or_assume(tool.flute_length, "flute length", 2.0 * d);
    let stickout = or_assume(tool.stickout, "stickout", flutes.max(3.0 * d));
    let shank_r = or_assume(tool.shank_diameter, "shank diameter", d) / 2.0;
    let overall = or_assume(tool.overall_length, "overall length", stickout + 2.0 * d);

    // The cutting end, then the side up to the end of the flutes; a long V-bit's
    // flank can outrun them.
    let mut cutter = tip_profile(tool);
    let end = cutter.last().map_or(0.0, |p| p.1);
    let flutes = flutes.max(end);
    cutter.push((r, flutes));
    cutter.push((0.0, flutes));

    // Above the flutes, the body steps to the shank diameter and runs up into the
    // holder, and on to its end, hidden inside the holder.
    let shank = vec![
        (0.0, flutes),
        (shank_r, flutes),
        (shank_r, overall),
        (0.0, overall),
    ];

    let mut outlines = vec![
        Outline {
            part: Part::Flutes,
            half: cutter,
        },
        Outline {
            part: Part::Shank,
            half: shank,
        },
    ];
    let mut top = overall;
    let mut max_radius = r.max(shank_r);
    if let Some(holder) = &tool.holder {
        let mut z = stickout;
        for seg in &holder.segments {
            if !(seg.diameter.is_finite() && seg.length.is_finite()) {
                continue;
            }
            let hr = seg.diameter / 2.0;
            outlines.push(Outline {
                part: Part::Holder,
                half: vec![
                    (0.0, z),
                    (hr, z),
                    (hr, z + seg.length),
                    (0.0, z + seg.length),
                ],
            });
            z += seg.length;
            max_radius = max_radius.max(hr);
        }
        top = top.max(z);
    }
    Drawing {
        outlines,
        holder_face: stickout,
        max_radius,
        height: top,
        assumed,
    }
}

/// A tool type's name, as the user reads it.
pub fn kind_name(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Flat => "flat",
        ToolKind::Ball => "ball",
        ToolKind::Bullnose => "bullnose",
        ToolKind::Cone => "V-bit / chamfer",
        ToolKind::Drill => "drill",
    }
}

/// The height of a tool's picture.
const PICTURE: f32 = 180.0;

/// The tool, side on and to scale, with its holder face marked, on the CAD grid.
pub fn paint_tool(ui: &mut egui::Ui, tool: &ToolSpec, m: Measure) {
    let d = draw(tool);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), PICTURE),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, theme::RADIUS_SM, theme::egui_color(theme::BG));
    widgets::grid_background(&painter, rect);
    painter.rect_stroke(
        rect,
        theme::RADIUS_SM,
        egui::Stroke::new(1.0, theme::egui_color(theme::BORDER)),
        egui::StrokeKind::Inside,
    );
    let margin = 16.0_f64;
    let w = f64::from(rect.width()) - 2.0 * margin;
    let h = f64::from(rect.height()) - 2.0 * margin;
    let scale = (w / (2.0 * d.max_radius.max(1e-6))).min(h / d.height.max(1e-6));
    let base = rect.center_bottom() - egui::vec2(0.0, 16.0);
    // Drawing coordinates are millimetres at most a few hundred across.
    #[allow(clippy::cast_possible_truncation)]
    let at = |x: f64, z: f64| base + egui::vec2((x * scale) as f32, -(z * scale) as f32);
    for o in &d.outlines {
        let (fill, stroke) = match o.part {
            Part::Flutes => (theme::AMBER_5, theme::AMBER_3),
            Part::Shank => (theme::GRAY_7, theme::GRAY_8),
            Part::Holder => (theme::GRAY_5, theme::GRAY_7),
        };
        // Right half up, left half down: one closed, convex outline per part.
        let mut points: Vec<egui::Pos2> = o.half.iter().map(|(x, z)| at(*x, *z)).collect();
        points.extend(o.half.iter().rev().map(|(x, z)| at(-x, *z)));
        painter.add(egui::Shape::convex_polygon(
            points,
            theme::egui_color(fill),
            egui::Stroke::new(1.0, theme::egui_color(stroke)),
        ));
    }
    // The holder face: where the stickout is measured to.
    let face = at(0.0, d.holder_face).y;
    painter.line_segment(
        [
            egui::pos2(rect.left() + 8.0, face),
            egui::pos2(rect.right() - 8.0, face),
        ],
        egui::Stroke::new(1.0, theme::egui_color(theme::TEXT_3)),
    );
    // Just below the face line, at the right edge: above it the holder is at its
    // widest, and below it only the narrower cutter, centred. Anywhere nearer the
    // middle, dim text lands on the grey shank and disappears.
    painter.text(
        egui::pos2(rect.right() - 10.0, face + 4.0),
        egui::Align2::RIGHT_TOP,
        format!("stickout {}", m.length(d.holder_face, 1)),
        theme::caption(ui.style()),
        theme::egui_color(theme::TEXT_3),
    );
    painter.text(
        egui::pos2(rect.right() - 10.0, rect.bottom() - 6.0),
        egui::Align2::RIGHT_BOTTOM,
        format!("Ø {}", m.length(tool.diameter, 2)),
        theme::caption(ui.style()),
        ui.visuals().text_color(),
    );
    if !d.assumed.is_empty() {
        ui.label(
            egui::RichText::new(format!("assumed, not given: {}", d.assumed.join(", ")))
                .size(theme::TEXT_SM)
                .color(theme::egui_color(theme::TEXT_4)),
        );
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use camcraft_core::job::{Holder, HolderSegment};

    fn widest(d: &Drawing, part: Part) -> f64 {
        d.outlines
            .iter()
            .filter(|o| o.part == part)
            .flat_map(|o| o.half.iter().map(|p| p.0))
            .fold(0.0, f64::max)
    }

    /// Drawn to scale: the cutter is as wide as the tool, the tip is at zero, and
    /// the holder starts exactly at the stickout.
    #[test]
    fn a_tool_is_drawn_to_its_own_dimensions() {
        let tool = ToolSpec {
            kind: ToolKind::Ball,
            diameter: 6.0,
            flute_length: Some(12.0),
            stickout: Some(25.0),
            shank_diameter: Some(6.0),
            overall_length: Some(50.0),
            holder: Some(Holder {
                name: "ER20".into(),
                segments: vec![HolderSegment {
                    diameter: 32.0,
                    length: 20.0,
                }],
            }),
            ..ToolSpec::default()
        };
        let d = draw(&tool);
        assert!((widest(&d, Part::Flutes) - 3.0).abs() < 1e-9);
        assert_eq!(d.holder_face, 25.0);
        let holder_bottom = d
            .outlines
            .iter()
            .filter(|o| o.part == Part::Holder)
            .flat_map(|o| o.half.iter().map(|p| p.1))
            .fold(f64::MAX, f64::min);
        assert_eq!(holder_bottom, 25.0);
        assert_eq!(widest(&d, Part::Holder), 16.0);
        let lowest = d
            .outlines
            .iter()
            .flat_map(|o| o.half.iter().map(|p| p.1))
            .fold(f64::MAX, f64::min);
        assert_eq!(lowest, 0.0, "the tip is at the origin");
        assert!(d.assumed.is_empty());
        assert_eq!(d.height, 50.0_f64.max(45.0));
    }

    /// What the tool does not say is assumed, and named as assumed.
    #[test]
    fn missing_dimensions_are_assumed_out_loud() {
        let d = draw(&ToolSpec::default());
        assert!(d.assumed.contains(&"stickout"));
        assert!(d.assumed.contains(&"flute length"));
        assert!((widest(&d, Part::Flutes) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn a_flat_is_square_and_a_ball_is_round() {
        let flat = draw(&ToolSpec::default());
        let flutes = &flat.outlines[0].half;
        assert!(flutes.contains(&(3.0, 0.0)), "square corner at the tip");
        let ball = draw(&ToolSpec {
            kind: ToolKind::Ball,
            ..ToolSpec::default()
        });
        assert!(
            !ball.outlines[0]
                .half
                .iter()
                .any(|p| p.1 == 0.0 && p.0 > 0.0)
        );
    }
}
