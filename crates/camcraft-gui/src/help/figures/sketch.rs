//! What the help's figures are drawn with: a section or a plan in its own units,
//! x to the right and y up, fitted to the figure, and the few things every
//! figure draws: material, the part, a tool, toolpaths, and dimensions.
//!
//! Colours are the design system's: material and part grey as the view has
//! them, toolpaths in the toolpath palette, and what the field sets in amber.

use camcraft_core::job::{ToolKind, ToolSpec};
use eframe::egui::{self, Align, Align2, Color32, Pos2, Stroke};

use crate::tools::drawing::tip_profile;
use crate::ui::theme;

/// A point in a figure's own units, x to the right and y up.
pub type P = [f32; 2];

/// How a region is filled and edged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Material to be cut.
    Stock,
    /// The part, which is kept.
    Part,
    /// A cutter.
    Tool,
    /// Only an outline: where something was, or would be.
    Ghost,
    /// Cleared: the figure's background, edged.
    Air,
    /// What the field sets.
    Hot,
}

impl Tone {
    /// What it is filled with.
    pub fn fill(self) -> Color32 {
        match self {
            Tone::Stock => theme::egui_color(theme::GRAY_4),
            Tone::Part => theme::egui_color(theme::PART),
            Tone::Tool => theme::egui_color(theme::GRAY_7),
            Tone::Ghost => Color32::TRANSPARENT,
            Tone::Air => theme::egui_color(theme::BG),
            Tone::Hot => theme::egui_color(theme::AMBER_7),
        }
    }

    fn stroke(self) -> Stroke {
        let c = match self {
            Tone::Stock => theme::GRAY_5,
            Tone::Part => theme::GRAY_7,
            Tone::Tool => theme::GRAY_8,
            Tone::Ghost | Tone::Air => theme::TEXT_4,
            Tone::Hot => theme::ACCENT,
        };
        Stroke::new(1.0, theme::egui_color(c))
    }
}

/// A move of a toolpath, coloured as the view colours it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    /// Cutting.
    Cut,
    /// Rapid, through the air: dashed.
    Rapid,
    /// A lead in or out.
    Lead,
}

impl Move {
    /// Its colour in the toolpath palette.
    pub fn color(self) -> Color32 {
        theme::egui_color(match self {
            Move::Cut => theme::PATH_CUT,
            Move::Rapid => theme::PATH_RAPID,
            Move::Lead => theme::PATH_LEAD,
        })
    }
}

/// The amber of what the field sets.
pub fn hot() -> Color32 {
    theme::egui_color(theme::ACCENT)
}

/// The quiet grey of everything around it.
pub fn quiet() -> Color32 {
    theme::egui_color(theme::TEXT_3)
}

/// The quietest grey, for lines that only show where a dimension is taken.
pub fn faint() -> Color32 {
    theme::egui_color(theme::TEXT_4)
}

/// The size of a figure's words.
fn font() -> egui::FontId {
    egui::FontId::proportional(theme::TEXT_XS)
}

/// Points round an arc about `c` of radius `r`, from `from` to `to` degrees
/// (anticlockwise from +x), a point every few degrees.
pub fn arc(c: P, r: f32, from: f32, to: f32) -> Vec<P> {
    let steps: u16 = 24;
    (0..=steps)
        .map(|i| {
            let a = (from + (to - from) * f32::from(i) / f32::from(steps)).to_radians();
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

/// The corners of a box, round and back to the first.
pub fn square(lo: P, hi: P) -> [P; 5] {
    [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]], lo]
}

/// A figure being drawn: its units mapped onto the screen.
pub struct Sketch<'a> {
    painter: &'a egui::Painter,
    /// Where (0, 0) is on the screen.
    origin: Pos2,
    /// Points per unit.
    scale: f32,
}

impl<'a> Sketch<'a> {
    /// `lo` to `hi`, in the figure's units, fitted into `rect` at one scale both
    /// ways, and centred.
    pub fn new(painter: &'a egui::Painter, rect: egui::Rect, lo: P, hi: P) -> Self {
        let w = (hi[0] - lo[0]).max(1e-3);
        let h = (hi[1] - lo[1]).max(1e-3);
        let scale = (rect.width() / w).min(rect.height() / h);
        let corner = rect.center() - egui::vec2(w, h) * scale / 2.0;
        Self {
            painter,
            origin: egui::pos2(corner.x - lo[0] * scale, corner.y + hi[1] * scale),
            scale,
        }
    }

    /// Where `p` is on the screen.
    pub fn at(&self, p: P) -> Pos2 {
        egui::pos2(
            self.origin.x + p[0] * self.scale,
            self.origin.y - p[1] * self.scale,
        )
    }

    fn all(&self, ps: &[P]) -> Vec<Pos2> {
        ps.iter().map(|p| self.at(*p)).collect()
    }

    /// A box from `lo` to `hi`.
    pub fn rect(&self, lo: P, hi: P, tone: Tone) {
        self.painter.rect(
            egui::Rect::from_two_pos(self.at(lo), self.at(hi)),
            0.0,
            tone.fill(),
            tone.stroke(),
            egui::StrokeKind::Inside,
        );
    }

    /// A convex region.
    pub fn polygon(&self, ps: &[P], tone: Tone) {
        self.painter.add(egui::Shape::convex_polygon(
            self.all(ps),
            tone.fill(),
            tone.stroke(),
        ));
    }

    /// The region between two lines of as many points each, filled with `color`,
    /// whatever its shape: a crescent, a band, the ground under a profile.
    pub fn band(&self, a: &[P], b: &[P], color: Color32) {
        let mut mesh = egui::Mesh::default();
        for (p, q) in a.iter().zip(b) {
            mesh.colored_vertex(self.at(*p), color);
            mesh.colored_vertex(self.at(*q), color);
        }
        let pairs = u32::try_from(a.len().min(b.len())).unwrap_or(0);
        for i in 1..pairs {
            let (p0, q0, p1, q1) = (2 * i - 2, 2 * i - 1, 2 * i, 2 * i + 1);
            mesh.add_triangle(p0, q0, p1);
            mesh.add_triangle(q0, q1, p1);
        }
        self.painter.add(egui::Shape::mesh(mesh));
    }

    /// Material or part in section: everything from its surface, `top`, down to
    /// `base`, edged along the surface.
    pub fn ground(&self, top: &[P], base: f32, tone: Tone) {
        let floor: Vec<P> = top.iter().map(|p| [p[0], base]).collect();
        self.band(top, &floor, tone.fill());
        let s = tone.stroke();
        self.line(top, s.color, s.width);
    }

    /// A line through `ps`.
    pub fn line(&self, ps: &[P], color: Color32, width: f32) {
        self.painter
            .add(egui::Shape::line(self.all(ps), Stroke::new(width, color)));
    }

    /// A dashed line through `ps`.
    pub fn dashed(&self, ps: &[P], color: Color32, width: f32) {
        self.painter.extend(egui::Shape::dashed_line(
            &self.all(ps),
            Stroke::new(width, color),
            4.0,
            3.0,
        ));
    }

    /// A toolpath through `ps`, heavier where it is what the field sets, with
    /// an arrowhead at its end.
    pub fn path(&self, ps: &[P], mv: Move, hot: bool) {
        let width = if hot { 2.0 } else { 1.25 };
        if mv == Move::Rapid {
            self.dashed(ps, mv.color(), width);
        } else {
            self.line(ps, mv.color(), width);
        }
        if let [.., a, b] = ps {
            self.head(*a, *b, mv.color());
        }
    }

    /// A line through `ps` with an arrowhead at its end.
    pub fn arrow(&self, ps: &[P], color: Color32, width: f32) {
        self.line(ps, color, width);
        if let [.., a, b] = ps {
            self.head(*a, *b, color);
        }
    }

    /// An arrowhead at `to`, pointing away from `from`.
    fn head(&self, from: P, to: P, color: Color32) {
        let (a, b) = (self.at(from), self.at(to));
        let dir = (b - a).normalized();
        if !dir.x.is_finite() || !dir.y.is_finite() {
            return;
        }
        let side = dir.rot90() * 2.5;
        let back = b - dir * 6.0;
        self.painter.add(egui::Shape::convex_polygon(
            vec![b, back + side, back - side],
            color,
            Stroke::NONE,
        ));
    }

    /// A disc: a tool seen from above.
    pub fn circle(&self, c: P, r: f32, tone: Tone) {
        self.painter
            .circle(self.at(c), r * self.scale, tone.fill(), tone.stroke());
    }

    /// A circle's outline.
    pub fn ring(&self, c: P, r: f32, color: Color32, width: f32) {
        self.painter
            .circle_stroke(self.at(c), r * self.scale, Stroke::new(width, color));
    }

    /// A cutter of `kind` side on, its tip at `tip` and `d` wide, standing
    /// `height` tall.
    pub fn tool(&self, kind: ToolKind, tip: P, d: f32, height: f32, tone: Tone) {
        let spec = ToolSpec {
            kind,
            diameter: f64::from(d),
            corner: f64::from(d) / 4.0,
            angle: (kind == ToolKind::Cone).then_some(90.0),
            ..ToolSpec::default()
        };
        self.cutter(&spec, tip, height, tone);
    }

    /// The cutting end of `spec` side on, in the figure's units, shaped as the
    /// tool library draws it: its tip at `tip`, standing `height` tall.
    pub fn cutter(&self, spec: &ToolSpec, tip: P, height: f32, tone: Tone) {
        // The profile is in the figure's own units, a few dozen at most.
        #[allow(clippy::cast_possible_truncation)]
        let mut half: Vec<P> = tip_profile(spec)
            .into_iter()
            .map(|(r, z)| [r as f32, z as f32])
            .collect();
        #[allow(clippy::cast_possible_truncation)]
        let r = (spec.diameter / 2.0) as f32;
        half.push([r, height]);
        half.push([0.0, height]);
        let mut outline: Vec<P> = half
            .iter()
            .map(|p| [tip[0] + p[0], tip[1] + p[1]])
            .collect();
        outline.extend(half.iter().rev().map(|p| [tip[0] - p[0], tip[1] + p[1]]));
        self.polygon(&outline, tone);
    }

    /// A dimension from `a` to `b`, arrowed both ends, `label` beside its middle
    /// on the side `side` says (as text is anchored: `CENTER_BOTTOM` puts it
    /// above). Amber when it is what the field sets.
    pub fn dim(&self, a: P, b: P, label: &str, side: Align2, hot: bool) {
        let color = if hot { self::hot() } else { faint() };
        self.line(&[a, b], color, if hot { 1.5 } else { 1.0 });
        self.head(b, a, color);
        self.head(a, b, color);
        if !label.is_empty() {
            self.text([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0], side, label, hot);
        }
    }

    /// Words at `p`, anchored at `align` and kept a few points off it: amber
    /// when they name what the field sets.
    pub fn text(&self, p: P, align: Align2, text: &str, hot: bool) {
        self.text_in(p, align, text, if hot { self::hot() } else { quiet() });
    }

    /// As [`text`](Self::text), in `color`.
    pub fn text_in(&self, p: P, align: Align2, text: &str, color: Color32) {
        let nudge = |a: Align, by: f32| match a {
            Align::Min => by,
            Align::Center => 0.0,
            Align::Max => -by,
        };
        let at = self.at(p) + egui::vec2(nudge(align.x(), 4.0), nudge(align.y(), 3.0));
        self.painter.text(at, align, text, font(), color);
    }
}
