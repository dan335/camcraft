//! Figures for how an operation moves between cuts: how high it lifts, how it
//! comes down, and how it enters and leaves a pass.

use camcraft_core::job::ToolKind;
use eframe::egui::{self, Align2};

use super::sketch::{Move, P, Sketch, Tone, arc, faint, hot, quiet};
use super::{Helix, Lead};

/// Two cuts either side of a rise: a minimum retraction goes just over it, a
/// full one up to the retract height.
pub fn retraction(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 46.0]);
    s.ground(
        &[
            [0.0, 12.0],
            [38.0, 12.0],
            [42.0, 24.0],
            [58.0, 24.0],
            [62.0, 12.0],
            [100.0, 12.0],
        ],
        0.0,
        Tone::Stock,
    );
    s.path(&[[4.0, 12.0], [24.0, 12.0]], Move::Cut, false);
    s.path(&[[76.0, 12.0], [96.0, 12.0]], Move::Cut, false);
    s.dashed(&[[0.0, 40.0], [100.0, 40.0]], faint(), 1.0);
    s.dashed(
        &[[24.0, 12.0], [24.0, 40.0], [76.0, 40.0], [76.0, 12.0]],
        quiet(),
        1.0,
    );
    s.text(
        [50.0, 40.0],
        Align2::CENTER_BOTTOM,
        "full: to the retract height",
        false,
    );
    s.path(
        &[[24.0, 12.0], [24.0, 29.0], [76.0, 29.0], [76.0, 12.0]],
        Move::Rapid,
        true,
    );
    s.text([50.0, 29.0], Align2::CENTER_BOTTOM, "minimum", true);
}

/// Rapid down to the safe distance over the material, then feed.
pub fn safe_distance(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let (surface, safe) = (14.0_f32, 22.0_f32);
    s.ground(&[[0.0, surface], [100.0, surface]], 0.0, Tone::Stock);
    s.dashed(&[[16.0, safe], [74.0, safe]], faint(), 1.0);
    s.path(&[[30.0, 42.0], [30.0, safe]], Move::Rapid, false);
    s.path(
        &[[30.0, safe], [30.0, surface], [60.0, surface], [60.0, safe]],
        Move::Cut,
        false,
    );
    s.path(&[[60.0, safe], [60.0, 42.0]], Move::Rapid, false);
    s.dim(
        [72.0, surface],
        [72.0, safe],
        "safe distance",
        Align2::LEFT_CENTER,
        true,
    );
}

/// A short link made staying down, and a long one that lifts.
pub fn stay_down(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let y = 14.0_f32;
    s.ground(&[[0.0, y], [100.0, y]], 0.0, Tone::Stock);
    s.path(&[[4.0, y], [28.0, y]], Move::Cut, false);
    s.path(&[[28.0, y], [46.0, y]], Move::Lead, true);
    s.path(&[[46.0, y], [62.0, y]], Move::Cut, false);
    s.path(
        &[[62.0, y], [62.0, 34.0], [92.0, 34.0], [92.0, y]],
        Move::Rapid,
        false,
    );
    s.text([77.0, 34.0], Align2::CENTER_BOTTOM, "longer: lifts", false);
    s.dim(
        [28.0, 20.0],
        [46.0, 20.0],
        "stay down",
        Align2::CENTER_BOTTOM,
        true,
    );
}

/// A parallel pass rolling in and out on vertical arcs, their radius in amber.
pub fn lead_radius(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let (y, r) = (14.0_f32, 10.0_f32);
    s.ground(&[[0.0, y], [100.0, y]], 0.0, Tone::Stock);
    s.path(&[[10.0, 42.0], [10.0, y + r]], Move::Rapid, false);
    s.path(&arc([20.0, y + r], r, 180.0, 270.0), Move::Lead, true);
    s.path(&[[20.0, y], [80.0, y]], Move::Cut, false);
    s.path(&arc([80.0, y + r], r, 270.0, 360.0), Move::Lead, true);
    radius(&s, [20.0, y + r], r, 225.0);
    s.tool(ToolKind::Ball, [50.0, y], 10.0, 22.0, Tone::Tool);
}

/// A profile's lead: side on, the vertical arc down onto it; from above, the
/// straight and the level arc onto the ring.
pub fn lead(painter: &egui::Painter, rect: egui::Rect, which: Lead) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let lit = |l| which == l || which == Lead::All;
    // Side on.
    let (depth, r) = (20.0_f32, 10.0_f32);
    s.dashed(&[[0.0, depth], [40.0, depth]], faint(), 1.0);
    s.path(&[[6.0, 40.0], [6.0, depth + r]], Move::Rapid, false);
    let c = [16.0, depth + r];
    s.path(&arc(c, r, 180.0, 270.0), Move::Lead, lit(Lead::Vertical));
    s.path(
        &[[16.0, depth], [36.0, depth]],
        Move::Lead,
        lit(Lead::Straight),
    );
    if lit(Lead::Vertical) {
        radius(&s, c, r, 225.0);
    }
    s.text([20.0, 44.0], Align2::CENTER_TOP, "side", false);
    // From above: the part below, the ring along it.
    let ring = 12.0_f32;
    s.rect([50.0, 0.0], [100.0, 8.0], Tone::Part);
    s.path(&[[70.0, ring], [98.0, ring]], Move::Cut, false);
    s.path(
        &[[60.0, 34.0], [60.0, ring + r]],
        Move::Lead,
        lit(Lead::Straight),
    );
    let c = [70.0, ring + r];
    s.path(
        &arc(c, r, 180.0, 270.0),
        Move::Lead,
        lit(Lead::Arc) || lit(Lead::Sweep),
    );
    if which == Lead::Straight {
        s.dim(
            [56.0, 34.0],
            [56.0, ring + r],
            "straight",
            Align2::RIGHT_CENTER,
            true,
        );
    }
    if which == Lead::Arc {
        radius(&s, c, r, 225.0);
    }
    if which == Lead::Sweep {
        s.line(&arc(c, 4.0, 180.0, 270.0), hot(), 1.5);
        s.text([c[0] + 1.0, c[1] + 1.0], Align2::LEFT_BOTTOM, "sweep", true);
    }
    s.text([80.0, 44.0], Align2::CENTER_TOP, "from above", false);
}

/// An arc's radius: an arrow from its centre out to it at `at` degrees.
fn radius(s: &Sketch<'_>, c: P, r: f32, at: f32) {
    if let Some(p) = arc(c, r, at, at).first() {
        s.arrow(&[c, *p], hot(), 1.5);
    }
    s.text(c, Align2::LEFT_BOTTOM, "radius", true);
}

/// A ramp down into material along the pass, its angle in amber.
pub fn ramp_angle(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let (top, floor) = (34.0_f32, 16.0_f32);
    s.ground(
        &[
            [0.0, top],
            [8.0, top],
            [58.0, floor],
            [96.0, floor],
            [96.0, top],
            [100.0, top],
        ],
        0.0,
        Tone::Stock,
    );
    s.path(&[[8.0, 44.0], [8.0, top]], Move::Rapid, false);
    s.path(&[[8.0, top], [58.0, floor], [84.0, floor]], Move::Cut, true);
    let angle = ((top - floor) / 50.0).atan().to_degrees();
    s.line(&arc([8.0, top], 18.0, -angle, 0.0), hot(), 1.5);
    s.text([27.0, top - 3.0], Align2::LEFT_CENTER, "ramp angle", true);
    s.tool(ToolKind::Flat, [84.0, floor], 10.0, 24.0, Tone::Tool);
}

/// The three ways into material, side on: a helix, a plunge and a ramp.
pub fn entry(painter: &egui::Painter, rect: egui::Rect) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let (top, bottom) = (40.0_f32, 10.0_f32);
    s.ground(&[[0.0, 30.0], [100.0, 30.0]], 0.0, Tone::Stock);
    s.path(&descent(17.0, 7.0, top, bottom, 3.0), Move::Cut, false);
    s.path(&[[50.0, top], [50.0, bottom]], Move::Cut, false);
    s.path(&[[74.0, top], [92.0, bottom]], Move::Cut, false);
    for (x, name) in [(17.0, "helix"), (50.0, "plunge"), (83.0, "ramp")] {
        s.text([x, 0.0], Align2::CENTER_BOTTOM, name, true);
    }
}

/// A helix side on, about `x` with radius `r`, from `top` to `bottom` in
/// `laps` turns.
fn descent(x: f32, r: f32, top: f32, bottom: f32, laps: f32) -> Vec<P> {
    let steps: u16 = 96;
    (0..=steps)
        .map(|i| {
            let t = f32::from(i) / f32::from(steps);
            let a = t * laps * std::f32::consts::TAU;
            [x + r * a.sin(), top - (top - bottom) * t]
        })
        .collect()
}

/// A helical entry: from above, the circle its centre follows; side on, the
/// drop of each lap.
pub fn helix(painter: &egui::Painter, rect: egui::Rect, which: Helix) {
    let s = Sketch::new(painter, rect, [0.0, 0.0], [100.0, 44.0]);
    let (c, r) = ([22.0_f32, 24.0_f32], 12.0_f32);
    let diameter = which == Helix::Diameter;
    s.ring(c, r, Move::Cut.color(), 1.25);
    s.circle([c[0], c[1] - r], 5.0, Tone::Tool);
    s.dim(
        [c[0] - r, c[1]],
        [c[0] + r, c[1]],
        if diameter { "diameter" } else { "" },
        Align2::CENTER_BOTTOM,
        diameter,
    );
    s.text([22.0, 0.0], Align2::CENTER_BOTTOM, "from above", false);
    let (x, top, bottom, laps) = (66.0_f32, 40.0_f32, 8.0_f32, 2.5_f32);
    s.path(&descent(x, r, top, bottom, laps), Move::Cut, false);
    // Two crests a lap apart, at the helix's right.
    let lap = (top - bottom) / laps;
    let first = top - lap / 4.0;
    s.dim(
        [x + r + 6.0, first],
        [x + r + 6.0, first - lap],
        if diameter { "" } else { "per lap" },
        Align2::LEFT_CENTER,
        !diameter,
    );
    for y in [first, first - lap] {
        s.line(&[[x + r, y], [x + r + 8.0, y]], faint(), 1.0);
    }
    s.text([x, 0.0], Align2::CENTER_BOTTOM, "side", false);
}
