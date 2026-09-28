//! Figures for how an operation cuts: its passes, levels, stock to leave and
//! tabs, and a tool's bite.

use camcraft_core::job::ToolKind;
use eframe::egui::{self, Align2};

use super::sketch::{Move, P, Sketch, Tone, arc, faint, hot, quiet, square};
use super::{Facing, LeaveOn, Tab};
use crate::ui::theme;

/// A ball tool at two passes over the scallops it leaves, the distance between
/// them in amber.
pub fn stepover(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let (r, tip) = (12.0_f32, 10.0_f32);
    let centres = [10.0_f32, 30.0, 50.0, 70.0, 90.0];
    // The surface left: the lowest of the tool's arcs over each point.
    let surface: Vec<P> = (0..=100_u8)
        .map(|i| {
            let x = f32::from(i);
            let y = centres
                .iter()
                .filter(|c| (x - **c).abs() < r)
                .map(|c| tip + r - (r * r - (x - c).powi(2)).sqrt())
                .fold(tip + r, f32::min);
            [x, y]
        })
        .collect();
    s.ground(&surface, 0.0, Tone::Stock);
    s.tool(ToolKind::Ball, [30.0, tip], 2.0 * r, 26.0, Tone::Ghost);
    s.tool(ToolKind::Ball, [50.0, tip], 2.0 * r, 26.0, Tone::Tool);
    for x in [30.0, 50.0] {
        s.dashed(&[[x, tip], [x, 38.0]], faint(), 1.0);
    }
    s.dim(
        [30.0, 38.0],
        [50.0, 38.0],
        "stepover",
        Align2::CENTER_BOTTOM,
        true,
    );
}

/// The tool from above, reaching into the material by the width of cut it
/// holds, in amber.
pub fn optimal_load(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let (c, r, edge) = ([46.0_f32, 20.0_f32], 14.0_f32, 54.0_f32);
    s.rect([edge, 0.0], [100.0, 44.0], Tone::Stock);
    s.circle(c, r, Tone::Tool);
    // The part of the tool in the material: its circle past the edge.
    let reach = ((edge - c[0]) / r).acos().to_degrees();
    let engaged = arc(c, r, -reach, reach);
    let chord: Vec<P> = engaged.iter().map(|p| [edge, p[1]]).collect();
    s.band(&engaged, &chord, Tone::Hot.fill());
    s.line(&engaged, hot(), 1.5);
    for x in [edge, c[0] + r] {
        s.line(&[[x, c[1]], [x, 38.0]], faint(), 1.0);
    }
    s.dim(
        [edge, 37.0],
        [c[0] + r, 37.0],
        "optimal load",
        Align2::CENTER_BOTTOM,
        true,
    );
    s.arrow(&[[24.0, 12.0], [24.0, 28.0]], quiet(), 1.0);
    s.text([2.0, 0.0], Align2::LEFT_BOTTOM, "from above", false);
}

/// A wall cut down in levels, the depth of one in amber.
pub fn stepdown(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    s.ground(
        &[[0.0, 42.0], [30.0, 42.0], [30.0, 24.0], [100.0, 24.0]],
        0.0,
        Tone::Stock,
    );
    for y in [42.0, 33.0, 15.0, 6.0] {
        s.dashed(&[[30.0, y], [100.0, y]], faint(), 1.0);
    }
    s.tool(ToolKind::Flat, [48.0, 24.0], 12.0, 20.0, Tone::Tool);
    s.dim(
        [80.0, 33.0],
        [80.0, 24.0],
        "stepdown",
        Align2::LEFT_CENTER,
        true,
    );
}

/// Parallel passes across the stock from above, and the angle they make with X.
pub fn pass_angle(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let (lo, hi) = ([34.0_f32, 4.0_f32], [96.0_f32, 40.0_f32]);
    s.rect(lo, hi, Tone::Stock);
    let a = 30.0_f32.to_radians();
    let dir = [a.cos(), a.sin()];
    let across = [-a.sin(), a.cos()];
    let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    for k in -6..=6_i8 {
        let o = [
            mid[0] + across[0] * 5.0 * f32::from(k),
            mid[1] + across[1] * 5.0 * f32::from(k),
        ];
        if let Some((p, q)) = clip(o, dir, lo, hi) {
            s.line(&[p, q], Move::Cut.color(), 1.25);
        }
    }
    let o = [6.0_f32, 8.0_f32];
    s.arrow(&[o, [26.0, 8.0]], quiet(), 1.0);
    s.text([26.0, 8.0], Align2::LEFT_CENTER, "X", false);
    s.line(
        &[o, [o[0] + 20.0 * dir[0], o[1] + 20.0 * dir[1]]],
        hot(),
        1.5,
    );
    s.line(&arc(o, 12.0, 0.0, 30.0), hot(), 1.5);
    s.text(
        [o[0] + 13.0, o[1] + 5.0],
        Align2::LEFT_CENTER,
        "angle",
        true,
    );
    s.text([2.0, 44.0], Align2::LEFT_TOP, "from above", false);
}

/// The part of the line through `o` along `dir` inside the box from `lo` to
/// `hi`, if it crosses it.
fn clip(o: P, dir: P, lo: P, hi: P) -> Option<(P, P)> {
    let (mut t0, mut t1) = (f32::NEG_INFINITY, f32::INFINITY);
    for ((o, d), (lo, hi)) in o.iter().zip(dir).zip(lo.iter().zip(hi.iter())) {
        if d.abs() < 1e-6 {
            if *o < *lo || *o > *hi {
                return None;
            }
        } else {
            let (a, b) = ((lo - o) / d, (hi - o) / d);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
    }
    (t0 < t1).then(|| {
        (
            [o[0] + dir[0] * t0, o[1] + dir[1] * t0],
            [o[0] + dir[0] * t1, o[1] + dir[1] * t1],
        )
    })
}

/// Climb and conventional from above: the same clockwise tool, the material on
/// its right or its left as it goes.
pub fn climb(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    for (cx, right, name) in [(25.0_f32, true, "climb"), (75.0, false, "conventional")] {
        let (x0, x1) = if right {
            (cx + 5.0, cx + 20.0)
        } else {
            (cx - 20.0, cx - 5.0)
        };
        s.rect([x0, 8.0], [x1, 44.0], Tone::Stock);
        s.path(&[[cx, 8.0], [cx, 42.0]], Move::Cut, false);
        s.circle([cx, 24.0], 8.0, Tone::Tool);
        // Clockwise, as the spindle turns seen from above.
        s.arrow(
            &arc([cx, 24.0], 5.0, 160.0, 20.0),
            theme::egui_color(theme::BG),
            1.25,
        );
        s.text([cx, 1.0], Align2::CENTER_BOTTOM, name, true);
    }
}

/// The tool from above, and where it is one tooth later: the crescent between
/// is the chip.
pub fn chip_load(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let (c1, c2, r) = ([40.0_f32, 22.0_f32], [46.0_f32, 22.0_f32], 14.0_f32);
    s.circle(c1, r, Tone::Ghost);
    s.band(
        &arc(c2, r, -90.0, 90.0),
        &arc(c1, r, -90.0, 90.0),
        Tone::Hot.fill(),
    );
    s.ring(c2, r, theme::egui_color(theme::GRAY_8), 1.25);
    s.dim(
        [c1[0] + r, 22.0],
        [c2[0] + r, 22.0],
        "",
        Align2::CENTER_BOTTOM,
        true,
    );
    s.text([c2[0] + r, 22.0], Align2::LEFT_CENTER, "chip load", true);
    s.arrow(&[[6.0, 22.0], [20.0, 22.0]], quiet(), 1.0);
    s.text([13.0, 22.0], Align2::CENTER_BOTTOM, "feed", false);
    s.text([2.0, 0.0], Align2::LEFT_BOTTOM, "from above", false);
}

/// A hole drilled in pecks, backing out between, the depth of one in amber.
pub fn peck(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    s.ground(
        &[
            [0.0, 36.0],
            [42.0, 36.0],
            [42.0, 6.0],
            [58.0, 6.0],
            [58.0, 36.0],
            [100.0, 36.0],
        ],
        0.0,
        Tone::Stock,
    );
    let depths = [26.0_f32, 16.0, 6.0];
    for (i, depth) in (0_u8..).zip(depths) {
        let x = 45.0 + 4.0 * f32::from(i);
        s.path(&[[x, 40.0], [x, depth]], Move::Cut, false);
        if depth > 6.0 {
            s.path(&[[x + 2.0, depth], [x + 2.0, 40.0]], Move::Rapid, false);
        }
    }
    s.dashed(&[[58.0, 26.0], [68.0, 26.0]], faint(), 1.0);
    s.dim(
        [66.0, 36.0],
        [66.0, 26.0],
        "peck",
        Align2::LEFT_CENTER,
        true,
    );
}

/// Facing from above: passes run past the stock by the offset, then on by the
/// extension.
pub fn facing(painter: &egui::Painter, rect: egui::Rect, which: Facing) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 48.0]);
    let (offset, extension) = (5.0_f32, 6.0_f32);
    let (lo, hi) = ([26.0_f32, 10.0_f32], [74.0_f32, 34.0_f32]);
    s.rect(lo, hi, Tone::Stock);
    let faced = square(
        [lo[0] - offset, lo[1] - offset],
        [hi[0] + offset, hi[1] + offset],
    );
    let off_hot = which == Facing::StockOffset;
    s.dashed(&faced, if off_hot { hot() } else { faint() }, 1.0);
    let (start, end) = (lo[0] - offset - extension, hi[0] + offset + extension);
    let mut zigzag = Vec::new();
    for (i, y) in (0_u8..).zip([8.0_f32, 15.0, 22.0, 29.0, 36.0]) {
        if i % 2 == 0 {
            zigzag.extend([[start, y], [end, y]]);
        } else {
            zigzag.extend([[end, y], [start, y]]);
        }
    }
    s.path(&zigzag, Move::Cut, false);
    for (x, from) in [
        (hi[0], hi[1]),
        (hi[0] + offset, hi[1] + offset),
        (end, 36.0),
    ] {
        s.line(&[[x, from], [x, 44.0]], faint(), 1.0);
    }
    s.dim(
        [hi[0], 42.0],
        [hi[0] + offset, 42.0],
        if off_hot { "stock offset" } else { "" },
        Align2::CENTER_BOTTOM,
        off_hot,
    );
    s.dim(
        [hi[0] + offset, 42.0],
        [end, 42.0],
        if off_hot { "" } else { "pass extension" },
        Align2::CENTER_BOTTOM,
        !off_hot,
    );
}

/// A part's wall and floor in section with a skin left on them, the side the
/// field sets in amber, and a finishing tool beside it.
pub fn leave(painter: &egui::Painter, rect: egui::Rect, on: LeaveOn) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let walls = matches!(on, LeaveOn::Both | LeaveOn::Walls);
    let floors = matches!(on, LeaveOn::Both | LeaveOn::Floors);
    let tone = |hot| if hot { Tone::Hot } else { Tone::Stock };
    s.ground(
        &[[0.0, 40.0], [40.0, 40.0], [40.0, 10.0], [100.0, 10.0]],
        0.0,
        Tone::Part,
    );
    s.rect([40.0, 14.0], [44.0, 40.0], tone(walls));
    s.rect([40.0, 10.0], [100.0, 14.0], tone(floors));
    s.tool(ToolKind::Flat, [54.0, 14.0], 16.0, 28.0, Tone::Tool);
    s.dim([40.0, 30.0], [44.0, 30.0], "", Align2::CENTER_BOTTOM, walls);
    s.text([40.0, 30.0], Align2::RIGHT_CENTER, "walls", walls);
    s.dim(
        [84.0, 10.0],
        [84.0, 14.0],
        "",
        Align2::CENTER_BOTTOM,
        floors,
    );
    s.text([84.0, 14.0], Align2::CENTER_BOTTOM, "floors", floors);
}

/// Along a cut through the sheet: the tabs it leaves, and the path riding over
/// them.
pub fn tabs(painter: &egui::Painter, rect: egui::Rect, which: Tab) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let (floor, top) = (6.0_f32, 16.0_f32);
    s.rect([0.0, floor], [100.0, 34.0], Tone::Stock);
    let triangular = which == Tab::Shape;
    // The first tab square-sided, the second square too or ramped at 45°.
    let first = [[22.0, floor], [22.0, top], [32.0, top], [32.0, floor]];
    let second: Vec<P> = if triangular {
        vec![[58.0, floor], [68.0, top], [78.0, top], [88.0, floor]]
    } else {
        vec![[68.0, floor], [68.0, top], [78.0, top], [78.0, floor]]
    };
    s.polygon(&first, Tone::Hot);
    s.polygon(&second, Tone::Hot);
    let mut path = vec![[2.0, floor]];
    path.extend(first);
    path.extend(second);
    path.push([98.0, floor]);
    s.path(&path, Move::Cut, false);
    s.line(&[[0.0, floor], [100.0, floor]], quiet(), 1.0);
    if triangular {
        s.text([27.0, 0.0], Align2::CENTER_BOTTOM, "rectangular", true);
        s.text([73.0, 0.0], Align2::CENTER_BOTTOM, "triangular", true);
        return;
    }
    let lit = |t| which == t || which == Tab::All;
    s.dim(
        [22.0, 20.0],
        [32.0, 20.0],
        "width",
        Align2::CENTER_BOTTOM,
        lit(Tab::Width),
    );
    s.dim(
        [36.0, floor],
        [36.0, top],
        "height",
        Align2::LEFT_CENTER,
        lit(Tab::Height),
    );
    s.dim(
        [27.0, 27.0],
        [73.0, 27.0],
        "spacing",
        Align2::CENTER_BOTTOM,
        lit(Tab::Spacing),
    );
}
