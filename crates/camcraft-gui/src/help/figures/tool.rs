//! Figures for the tool library: a tool's dimensions, and the shapes of cutter.

use camcraft_core::job::{ToolKind, ToolSpec};
use eframe::egui::{self, Align2};

use super::ToolDim;
use super::sketch::{Sketch, Tone, arc, faint, hot};

/// A tool in its holder, side on, as the tool library draws it, with its
/// lengths dimensioned down the right: the one the field sets in amber and
/// named. A V-bit for its angle and tip, a bullnose for its corner.
pub fn dimensions(painter: &egui::Painter, rect: egui::Rect, lit: ToolDim) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 46.0]);
    let (x, tip, d) = (30.0_f32, 8.0_f32, 12.0_f32);
    let (flutes, shoulder, face, end) = (22.0_f32, 27.0_f32, 34.0_f32, 44.0_f32);
    let kind = match lit {
        ToolDim::Angle | ToolDim::Tip => ToolKind::Cone,
        ToolDim::Corner => ToolKind::Bullnose,
        _ => ToolKind::Flat,
    };
    let spec = ToolSpec {
        kind,
        diameter: f64::from(d),
        corner: 3.0,
        angle: Some(90.0),
        tip_diameter: if lit == ToolDim::Tip { 4.0 } else { 0.0 },
        ..ToolSpec::default()
    };
    s.cutter(&spec, [x, tip], flutes - tip, Tone::Tool);
    s.rect([x - 4.5, flutes], [x + 4.5, shoulder], Tone::Tool);
    s.rect([x - 5.0, shoulder], [x + 5.0, face], Tone::Tool);
    let holder = if lit == ToolDim::Holder {
        Tone::Hot
    } else {
        Tone::Stock
    };
    s.rect([x - 14.0, face], [x + 14.0, 46.0], holder);
    // The shank runs on inside the holder, out of sight.
    s.dashed(
        &[
            [x - 5.0, face],
            [x - 5.0, end],
            [x + 5.0, end],
            [x + 5.0, face],
        ],
        faint(),
        1.0,
    );

    // The lengths, each from the tip, a column each.
    for y in [tip, flutes, shoulder, face, end] {
        s.line(&[[x + 7.0, y], [66.0, y]], faint(), 1.0);
    }
    for (column, (dim, name, top)) in (0_u8..).zip([
        (ToolDim::Flutes, "flute length", flutes),
        (ToolDim::Shoulder, "shoulder length", shoulder),
        (ToolDim::Stickout, "stickout", face),
        (ToolDim::Overall, "overall length", end),
    ]) {
        let at = 50.0 + 5.0 * f32::from(column);
        let on = dim == lit;
        s.dim([at, tip], [at, top], "", Align2::LEFT_CENTER, on);
        if on {
            s.text([66.0, (tip + top) / 2.0], Align2::LEFT_CENTER, name, true);
        }
    }

    match lit {
        ToolDim::Diameter => {
            for edge in [x - d / 2.0, x + d / 2.0] {
                s.line(&[[edge, tip], [edge, 1.0]], faint(), 1.0);
            }
            s.dim(
                [x - d / 2.0, 3.0],
                [x + d / 2.0, 3.0],
                "",
                Align2::CENTER_BOTTOM,
                true,
            );
            s.text([x + d / 2.0, 3.0], Align2::LEFT_CENTER, "diameter", true);
        }
        ToolDim::Tip => {
            s.dim(
                [x - 2.0, 3.0],
                [x + 2.0, 3.0],
                "",
                Align2::CENTER_BOTTOM,
                true,
            );
            s.text([x + 2.0, 3.0], Align2::LEFT_CENTER, "tip diameter", true);
        }
        ToolDim::Angle => {
            s.line(&arc([x, tip], 9.0, 45.0, 135.0), hot(), 1.5);
            s.text([x + 7.0, tip + 7.0], Align2::LEFT_CENTER, "angle", true);
        }
        ToolDim::Corner => {
            s.ring([x + d / 2.0 - 3.0, tip + 3.0], 3.0, hot(), 1.5);
            s.text(
                [x + d / 2.0, tip + 3.0],
                Align2::LEFT_CENTER,
                "corner radius",
                true,
            );
        }
        ToolDim::Shank => {
            let y = (shoulder + face) / 2.0;
            s.dim([x - 5.0, y], [x + 5.0, y], "", Align2::CENTER_BOTTOM, true);
            s.text([x - 5.0, y], Align2::RIGHT_CENTER, "shank", true);
        }
        ToolDim::Holder => {
            s.text([x + 14.0, 40.0], Align2::LEFT_CENTER, "holder", true);
        }
        ToolDim::Flutes | ToolDim::Shoulder | ToolDim::Stickout | ToolDim::Overall => {}
    }
}

/// The five shapes of cutter, side by side.
pub fn kinds(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    for (cx, kind, name) in [
        (10.0_f32, ToolKind::Flat, "flat"),
        (30.0, ToolKind::Ball, "ball"),
        (50.0, ToolKind::Bullnose, "bullnose"),
        (70.0, ToolKind::Cone, "V-bit"),
        (90.0, ToolKind::Drill, "drill"),
    ] {
        s.tool(kind, [cx, 12.0], 12.0, 30.0, Tone::Tool);
        s.text([cx, 0.0], Align2::CENTER_BOTTOM, name, true);
    }
}
