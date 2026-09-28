//! Figures for where an operation cuts: its heights, its boundary, a profile's
//! side, and the stock's margins.

use eframe::egui::{self, Align2};

use super::Margin;
use super::sketch::{Move, Sketch, Tone, hot, square};
use crate::ui::theme;
use crate::view::viewport::PlaneKind;

/// The stock and part in section with the operation's four heights across
/// them, each in its colour in the view, the one the field sets drawn solid.
pub fn heights(painter: &egui::Painter, rect: egui::Rect, lit: PlaneKind) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 46.0]);
    s.rect([22.0, 4.0], [78.0, 24.0], Tone::Stock);
    s.rect([30.0, 4.0], [70.0, 18.0], Tone::Part);
    for (kind, name, y) in [
        (PlaneKind::Clearance, "Clearance", 39.0),
        (PlaneKind::Retract, "Retract", 32.0),
        (PlaneKind::Top, "Top", 24.0),
        (PlaneKind::Bottom, "Bottom", 6.0),
    ] {
        let color = theme::egui_color(kind.color());
        if kind == lit {
            s.line(&[[4.0, y], [96.0, y]], color, 2.0);
            s.text_in([4.0, y], Align2::LEFT_BOTTOM, name, color);
        } else {
            s.dashed(&[[4.0, y], [96.0, y]], color.gamma_multiply(0.5), 1.0);
            s.text([4.0, y], Align2::LEFT_BOTTOM, name, false);
        }
    }
}

/// The three ways a tool may sit against a boundary, from above: its centre on
/// it, all of it inside, or reaching in from outside. The thin line is where
/// its centre goes.
pub fn containment(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let (half, r, y) = (8.0_f32, 4.0_f32, 24.0_f32);
    for (cx, name, shift) in [
        (17.0_f32, "centre on it", 0.0_f32),
        (50.0, "all inside", -r),
        (83.0, "reaching in", r),
    ] {
        s.line(
            &square([cx - half, y - half], [cx + half, y + half]),
            theme::egui_color(theme::BOUNDARY),
            2.0,
        );
        let path = half + shift;
        s.line(
            &square([cx - path, y - path], [cx + path, y + path]),
            theme::egui_color(theme::BOUNDARY_LIMIT),
            1.0,
        );
        s.circle([cx + path, y], r, Tone::Tool);
        s.text([cx, 0.0], Align2::CENTER_BOTTOM, name, true);
    }
}

/// A boundary round the part, and the same moved out by the offset.
pub fn boundary_offset(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let offset = 6.0_f32;
    s.rect([38.0, 16.0], [62.0, 28.0], Tone::Part);
    let (lo, hi) = ([34.0_f32, 12.0_f32], [66.0_f32, 32.0_f32]);
    s.line(&square(lo, hi), theme::egui_color(theme::BOUNDARY), 2.0);
    s.line(
        &square(
            [lo[0] - offset, lo[1] - offset],
            [hi[0] + offset, hi[1] + offset],
        ),
        hot(),
        1.25,
    );
    s.dim(
        [hi[0], 22.0],
        [hi[0] + offset, 22.0],
        "",
        Align2::CENTER_BOTTOM,
        true,
    );
    s.text([hi[0] + offset, 22.0], Align2::LEFT_CENTER, "offset", true);
    s.text([2.0, 0.0], Align2::LEFT_BOTTOM, "from above", false);
}

/// A profile from above: round a part outside it, round a hole inside it, or
/// along a groove on it.
pub fn profile_side(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let (half, r, y) = (9.0_f32, 4.0_f32, 26.0_f32);
    for (cx, name, shift) in [
        (17.0_f32, "outside", r),
        (50.0, "inside", -r),
        (83.0, "on", 0.0_f32),
    ] {
        let (lo, hi) = ([cx - half, y - half], [cx + half, y + half]);
        if shift > 0.0 {
            s.rect(lo, hi, Tone::Part);
        } else if shift < 0.0 {
            s.rect([cx - 15.0, y - 15.0], [cx + 15.0, y + 15.0], Tone::Part);
            s.rect(lo, hi, Tone::Air);
        } else {
            s.line(&square(lo, hi), theme::egui_color(theme::BOUNDARY), 1.0);
        }
        let path = half + shift;
        s.line(
            &square([cx - path, y - path], [cx + path, y + path]),
            Move::Cut.color(),
            1.25,
        );
        s.circle([cx + path, y], r, Tone::Tool);
        s.text([cx, 0.0], Align2::CENTER_BOTTOM, name, true);
    }
}

/// A stock box round the part, from the front, or from above for Y, its
/// margins dimensioned and the one the field sets in amber.
pub fn margins(painter: &egui::Painter, rect: egui::Rect, lit: Margin) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let (x, lo, hi) = (10.0_f32, [32.0_f32, 12.0_f32], [68.0_f32, 30.0_f32]);
    let from_above = lit == Margin::Y;
    let (below, above) = if from_above { (7.0, 7.0) } else { (7.0, 8.0) };
    s.rect(
        [lo[0] - x, lo[1] - below],
        [hi[0] + x, hi[1] + above],
        Tone::Stock,
    );
    s.rect(lo, hi, Tone::Part);
    let mid = [(lo[1] + hi[1]) / 2.0, (lo[0] + hi[0]) / 2.0];
    s.dim(
        [hi[0], mid[0]],
        [hi[0] + x, mid[0]],
        "",
        Align2::CENTER_BOTTOM,
        lit == Margin::X,
    );
    s.text(
        [hi[0] + x, mid[0]],
        Align2::LEFT_CENTER,
        "X",
        lit == Margin::X,
    );
    if from_above {
        s.dim(
            [mid[1], hi[1]],
            [mid[1], hi[1] + above],
            "",
            Align2::CENTER_BOTTOM,
            true,
        );
        s.text(
            [mid[1], hi[1] + above / 2.0],
            Align2::LEFT_CENTER,
            "Y",
            true,
        );
        s.text([2.0, 44.0], Align2::LEFT_TOP, "from above", false);
    } else {
        let top = lit == Margin::Top;
        let under = lit == Margin::Under;
        s.dim(
            [mid[1], hi[1]],
            [mid[1], hi[1] + above],
            "",
            Align2::CENTER_BOTTOM,
            top,
        );
        s.text(
            [mid[1], hi[1] + above / 2.0],
            Align2::LEFT_CENTER,
            "top",
            top,
        );
        s.dim(
            [mid[1], lo[1] - below],
            [mid[1], lo[1]],
            "",
            Align2::CENTER_BOTTOM,
            under,
        );
        s.text(
            [mid[1], lo[1] - below / 2.0],
            Align2::LEFT_CENTER,
            "underneath",
            under,
        );
        s.text([2.0, 44.0], Align2::LEFT_TOP, "front", false);
    }
}
