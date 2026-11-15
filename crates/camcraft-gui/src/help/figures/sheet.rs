//! Figures for a sheet that lays its parts out on itself: the gaps it keeps,
//! and where it starts and which way it is used up.

use eframe::egui::{self, Align2};

use super::sketch::{Sketch, Tone, faint, hot};
use super::{Gap, Packing};

/// A sheet from above with two parts on it, the gap between them and the
/// border inside its edge dimensioned, the one the field sets in amber.
pub fn gaps(painter: &egui::Painter, rect: egui::Rect, lit: Gap) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 48.0]);
    let (lo, hi) = ([2.0_f32, 4.0_f32], [98.0_f32, 44.0_f32]);
    let border = 6.0_f32;
    let (inside_lo, inside_hi) = (
        [lo[0] + border, lo[1] + border],
        [hi[0] - border, hi[1] - border],
    );
    s.rect(lo, hi, Tone::Stock);
    let edge = lit == Gap::Border;
    let line = [
        inside_lo,
        [inside_hi[0], inside_lo[1]],
        inside_hi,
        [inside_lo[0], inside_hi[1]],
        inside_lo,
    ];
    s.dashed(&line, if edge { hot() } else { faint() }, 1.0);
    // Two parts against the border, a gap between them.
    let (first, second) = (
        ([inside_lo[0], inside_lo[1]], [40.0, inside_hi[1]]),
        ([54.0_f32, inside_lo[1]], [78.0, 28.0]),
    );
    s.rect(first.0, first.1, Tone::Part);
    s.rect(second.0, second.1, Tone::Part);

    let spacing = lit == Gap::Spacing;
    s.dim(
        [first.1[0], 20.0],
        [second.0[0], 20.0],
        "",
        Align2::CENTER_BOTTOM,
        spacing,
    );
    s.text([50.0, 30.0], Align2::CENTER_BOTTOM, "spacing", spacing);
    s.dim(
        [inside_hi[0], 33.0],
        [hi[0], 33.0],
        "",
        Align2::CENTER_BOTTOM,
        edge,
    );
    s.text([inside_hi[0], 33.0], Align2::RIGHT_CENTER, "border", edge);
}

/// A sheet from above with parts laid from its front-left corner, filling
/// across it first and so along it, the offcut left whole at the far end:
/// the corner marked, the way it is used up arrowed, the one the field sets
/// in amber.
pub fn packing(painter: &egui::Painter, rect: egui::Rect, lit: Packing) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 48.0]);
    let (lo, hi) = ([4.0_f32, 10.0_f32], [96.0_f32, 46.0_f32]);
    s.rect(lo, hi, Tone::Stock);
    // Across first, then along: a column at a time from the corner.
    for (a, b) in [
        ([8.0, 14.0], [22.0, 26.0]),
        ([8.0, 30.0], [22.0, 42.0]),
        ([26.0, 14.0], [40.0, 24.0]),
        ([26.0, 28.0], [40.0, 36.0]),
        ([44.0, 14.0], [54.0, 28.0]),
    ] {
        s.rect(a, b, Tone::Part);
    }
    let from = lit == Packing::From;
    s.polygon(
        &[lo, [lo[0] + 6.0, lo[1]], [lo[0], lo[1] + 6.0]],
        if from { Tone::Hot } else { Tone::Ghost },
    );
    s.text(lo, Align2::LEFT_TOP, "pack from", from);

    let along = lit == Packing::Along;
    let color = if along { hot() } else { faint() };
    s.arrow(
        &[[60.0, 30.0], [90.0, 30.0]],
        color,
        if along { 1.5 } else { 1.0 },
    );
    s.text([75.0, 30.0], Align2::CENTER_BOTTOM, "along", along);
    s.text([75.0, 26.0], Align2::CENTER_TOP, "offcut", false);
}
