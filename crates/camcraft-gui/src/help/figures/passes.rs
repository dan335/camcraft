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

/// A slope in section, roughed in levels: the terraces a stepdown apart leave,
/// dashed, the fine levels between in amber, and the smaller steps they leave.
pub fn fine_stepdown(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let (step, fine) = (12.0_f32, 6.0_f32);
    // Where the slope is at height `y`: it rises to the right, the part under it.
    let at = |y: f32| 14.0 + 2.0 * (y - 4.0);
    s.ground(
        &[[0.0, 4.0], [at(4.0), 4.0], [at(40.0), 40.0], [100.0, 40.0]],
        0.0,
        Tone::Part,
    );
    // What is left over the slope: a step above each level, fine or not.
    for z in [4.0_f32, 10.0, 16.0, 22.0, 28.0, 34.0] {
        s.polygon(
            &[[at(z), z], [at(z), z + fine], [at(z + fine), z + fine]],
            Tone::Stock,
        );
    }
    // What the stepdown's levels alone would leave.
    let mut stair = Vec::new();
    for z in [4.0_f32, 16.0, 28.0] {
        stair.extend([[at(z), z], [at(z), z + step]]);
    }
    stair.push([at(40.0), 40.0]);
    s.dashed(&stair, quiet(), 1.0);
    // The fine levels, each across the terrace above a level.
    for z in [10.0_f32, 22.0, 34.0] {
        s.line(&[[at(z - fine), z], [at(z), z]], hot(), 1.5);
    }
    s.dashed(&[[6.0, 28.0], [at(28.0), 28.0]], faint(), 1.0);
    s.dashed(&[[6.0, 40.0], [at(28.0), 40.0]], faint(), 1.0);
    s.dim(
        [9.0, 28.0],
        [9.0, 40.0],
        "stepdown",
        Align2::LEFT_CENTER,
        false,
    );
    s.dim(
        [at(16.0) - 3.0, 16.0],
        [at(16.0) - 3.0, 22.0],
        "fine stepdown",
        Align2::RIGHT_CENTER,
        true,
    );
}

/// A pass over the part in section: a small drop it cuts down, within the
/// deepest step, in amber, and a taller one it lifts over and plunges down.
pub fn cut_step(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let (high, mid, low) = (30.0_f32, 22.0_f32, 4.0_f32);
    s.ground(
        &[
            [0.0, high],
            [20.0, high],
            [20.0, mid],
            [66.0, mid],
            [70.0, low],
            [100.0, low],
        ],
        0.0,
        Tone::Part,
    );
    let lift = 38.0_f32;
    s.path(
        &[[2.0, high], [20.0, high], [20.0, mid], [66.0, mid]],
        Move::Cut,
        false,
    );
    s.path(
        &[[66.0, mid], [66.0, lift], [72.0, lift]],
        Move::Rapid,
        false,
    );
    s.path(&[[72.0, lift], [72.0, low]], Move::Lead, false);
    s.path(&[[72.0, low], [98.0, low]], Move::Cut, false);
    s.dashed(&[[20.0, high], [30.0, high]], faint(), 1.0);
    s.dim(
        [26.0, mid],
        [26.0, high],
        "deepest step cut",
        Align2::LEFT_CENTER,
        true,
    );
    s.text([72.0, lift], Align2::LEFT_CENTER, "taller: lifts", false);
    s.text([72.0, 14.0], Align2::LEFT_CENTER, "plunge feed", false);
}

/// A shelf between two levels in section: the terrace the levels alone leave
/// on it, and the level flat area detection adds on it, in amber.
pub fn flat_areas(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let (shelf, above, below) = (22.0_f32, 34.0_f32, 10.0_f32);
    let (from, to) = (32.0_f32, 66.0_f32);
    s.ground(
        &[
            [0.0, below],
            [from, below],
            [from, shelf],
            [to, shelf],
            [to, 42.0],
            [100.0, 42.0],
        ],
        0.0,
        Tone::Part,
    );
    s.dashed(&[[0.0, above], [to, above]], faint(), 1.0);
    s.text([0.0, above], Align2::LEFT_BOTTOM, "levels", false);
    // Left on the shelf by the levels alone: the one below cannot reach it.
    s.rect([from, shelf], [to, above], Tone::Stock);
    s.text(
        [(from + to) / 2.0, (shelf + above) / 2.0],
        Align2::CENTER_CENTER,
        "terrace",
        false,
    );
    s.line(&[[from, shelf], [to, shelf]], hot(), 2.0);
    s.text([from, shelf], Align2::RIGHT_CENTER, "flat level", true);
}

/// Adaptive's three directions from above, a clockwise tool beside the
/// material: climbing, conventional, and both ways, up climbing and back down
/// conventional rather than going round.
pub fn both_ways(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let r = 6.0_f32;
    for (cx, right, name) in [(14.0_f32, true, "climb"), (48.0, false, "conventional")] {
        let (x0, x1) = if right {
            (cx + 4.0, cx + 14.0)
        } else {
            (cx - 14.0, cx - 4.0)
        };
        s.rect([x0, 8.0], [x1, 44.0], Tone::Stock);
        s.path(&[[cx, 8.0], [cx, 42.0]], Move::Cut, false);
        s.circle([cx, 24.0], r, Tone::Tool);
        s.arrow(
            &arc([cx, 24.0], 3.8, 160.0, 20.0),
            theme::egui_color(theme::BG),
            1.25,
        );
        s.text([cx, 1.0], Align2::CENTER_BOTTOM, name, true);
    }
    // Both ways: up beside the material, climbing, then round and back down
    // into it, the material now on its left: conventional.
    let cx = 76.0_f32;
    s.rect([cx + 4.0, 8.0], [cx + 14.0, 44.0], Tone::Stock);
    let mut way = vec![[cx, 8.0], [cx, 36.0]];
    way.extend(arc([cx + 3.0, 36.0], 3.0, 180.0, 0.0));
    way.push([cx + 6.0, 8.0]);
    s.path(&way, Move::Cut, false);
    s.circle([cx, 20.0], r, Tone::Tool);
    s.arrow(
        &arc([cx, 20.0], 3.8, 160.0, 20.0),
        theme::egui_color(theme::BG),
        1.25,
    );
    s.text([cx + 3.0, 1.0], Align2::CENTER_BOTTOM, "both ways", true);
}

/// An inside corner of a pocket from above: the tool's centre blended round it
/// at the minimum cutting radius, in amber, rather than into it, dashed, and
/// the material that leaves in the corner.
pub fn min_cutting_radius(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let (r, blend) = (6.0_f32, 14.0_f32);
    let (wall_x, wall_y) = (10.0_f32, 6.0_f32);
    let (x, y) = (wall_x + r, wall_y + r);
    let c = [x + blend, y + blend];
    // What the tool reaches round the blend, and the corner it leaves.
    let reach = arc(c, blend + r, 180.0, 270.0);
    let corner: Vec<P> = reach.iter().map(|_| [wall_x, wall_y]).collect();
    s.band(&reach, &corner, Tone::Stock.fill());
    s.rect([0.0, 0.0], [100.0, wall_y], Tone::Part);
    s.rect([0.0, 0.0], [wall_x, 44.0], Tone::Part);
    s.dashed(&[[x, c[1]], [x, y], [c[0], y]], faint(), 1.0);
    let mut way = vec![[x, 42.0]];
    way.extend(arc(c, blend, 180.0, 270.0));
    way.push([96.0, y]);
    s.path(&way, Move::Cut, false);
    let mid = arc(c, blend, 225.0, 225.0);
    if let Some(&m) = mid.first() {
        s.circle(m, r, Tone::Ghost);
        s.dim(c, m, "", Align2::CENTER_BOTTOM, true);
    }
    s.text(
        [c[0] + 1.0, c[1]],
        Align2::LEFT_CENTER,
        "minimum radius",
        true,
    );
    s.text([100.0, 44.0], Align2::RIGHT_TOP, "from above", false);
}

/// A closed pocket from above, twice: cleared, gone down into by a helix,
/// and left, as it is with machine cavities unticked.
pub fn cavities(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    for (x0, cleared, name) in [(4.0_f32, true, "ticked"), (54.0, false, "unticked")] {
        let (lo, hi) = ([x0, 8.0], [x0 + 42.0, 42.0]);
        s.rect(lo, hi, Tone::Part);
        let (plo, phi) = ([x0 + 8.0, 15.0], [x0 + 34.0, 35.0]);
        if cleared {
            s.rect(plo, phi, Tone::Air);
            let c = [x0 + 21.0, 25.0];
            s.path(&arc(c, 3.0, 0.0, 330.0), Move::Lead, false);
            for k in 1..=2_u8 {
                let w = 3.0 + 3.0 * f32::from(k);
                s.line(
                    &square(
                        [c[0] - w - 2.0, c[1] - w + 1.0],
                        [c[0] + w + 2.0, c[1] + w - 1.0],
                    ),
                    Move::Cut.color(),
                    1.25,
                );
            }
        } else {
            s.rect(plo, phi, Tone::Stock);
            s.text([x0 + 21.0, 25.0], Align2::CENTER_CENTER, "left", false);
        }
        s.text([x0 + 21.0, 1.0], Align2::CENTER_BOTTOM, name, true);
    }
}

/// A wall, then a shallow slope, in section: the stepdown's levels leave a
/// narrow step on the wall and a wide terrace on the slope; shallow areas add
/// levels, in amber, over the slope only, until its terraces are no wider
/// than the most.
pub fn shallow_areas(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    // Where the part stands at height `y`: steep up to 22, then shallow to 34.
    let at = |y: f32| {
        if y <= 22.0 {
            18.0 + (y - 4.0) * (4.0 / 18.0)
        } else {
            22.0 + (y - 22.0) * (74.0 / 12.0)
        }
    };
    s.ground(
        &[
            [0.0, 4.0],
            [at(4.0), 4.0],
            [at(22.0), 22.0],
            [96.0, 34.0],
            [100.0, 34.0],
        ],
        0.0,
        Tone::Part,
    );
    // What is left over it: a step above each level, the wall's narrow, the
    // slope's cut down by the levels added.
    s.polygon(
        &[[at(4.0), 4.0], [at(4.0), 14.0], [at(14.0), 14.0]],
        Tone::Stock,
    );
    s.polygon(
        &[
            [at(14.0), 14.0],
            [at(14.0), 24.0],
            [at(24.0), 24.0],
            [at(22.0), 22.0],
        ],
        Tone::Stock,
    );
    for z in [24.0_f32, 26.5, 29.0, 31.5] {
        s.polygon(
            &[[at(z), z], [at(z), z + 2.5], [at(z + 2.5), z + 2.5]],
            Tone::Stock,
        );
    }
    // What the stepdown's levels alone would leave.
    s.dashed(
        &[
            [at(4.0), 4.0],
            [at(4.0), 14.0],
            [at(14.0), 14.0],
            [at(14.0), 24.0],
            [at(24.0), 24.0],
            [at(24.0), 34.0],
            [96.0, 34.0],
        ],
        quiet(),
        1.0,
    );
    // The levels added over the slope, each across its terrace.
    for z in [26.5_f32, 29.0, 31.5] {
        s.line(&[[at(z - 2.5), z], [at(z), z]], hot(), 1.5);
    }
    s.text(
        [at(14.0) + 4.0, 10.0],
        Align2::LEFT_CENTER,
        "steep: none",
        false,
    );
    s.dashed(&[[at(29.0), 29.0], [at(29.0), 39.0]], faint(), 1.0);
    s.dashed(&[[at(31.5), 31.5], [at(31.5), 39.0]], faint(), 1.0);
    s.dim(
        [at(29.0), 39.0],
        [at(31.5), 39.0],
        "max stepover",
        Align2::RIGHT_CENTER,
        true,
    );
}

/// A closed pocket from above: a slot down its middle first, the tool's
/// width, then rings out to the slot's width, before clearing out from it.
pub fn slot_clearing(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    s.rect([2.0, 2.0], [72.0, 42.0], Tone::Part);
    s.rect([8.0, 8.0], [66.0, 36.0], Tone::Stock);
    let y = 22.0_f32;
    let (a, b) = (20.0_f32, 54.0_f32);
    // The slot cut, as wide as asked.
    s.rect([a - 5.0, y - 5.0], [b + 5.0, y + 5.0], Tone::Air);
    s.path(&arc([a, y], 2.5, 0.0, 330.0), Move::Lead, false);
    s.path(&[[a, y], [b, y]], Move::Cut, true);
    let mut ring = arc([b, y], 3.0, -90.0, 90.0);
    ring.extend(arc([a, y], 3.0, 90.0, 270.0));
    if let Some(&first) = ring.first() {
        ring.push(first);
    }
    s.path(&ring, Move::Cut, false);
    s.dim(
        [75.0, y - 5.0],
        [75.0, y + 5.0],
        "slot width",
        Align2::LEFT_CENTER,
        true,
    );
    s.dashed(&[[b + 5.0, y - 5.0], [75.0, y - 5.0]], faint(), 1.0);
    s.dashed(&[[b + 5.0, y + 5.0], [75.0, y + 5.0]], faint(), 1.0);
    s.text([76.0, 40.0], Align2::LEFT_TOP, "from above", false);
}
