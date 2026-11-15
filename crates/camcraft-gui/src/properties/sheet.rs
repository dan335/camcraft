//! An arranged sheet's panel: a drawing of how it lays its parts out, seen
//! from above and to scale, with what it places and leaves out under it; its
//! settings, a row each; and its parts, a card with a row for each that says
//! how many of the part's copies it places.
//!
//! Pointing at a copy in the drawing lights its part's row; pointing at a
//! row, or choosing it, lights the part's copies in the drawing and in the
//! view. Nothing here picks: which part is under the pointer is told from
//! where the copies stand ([`part_at`]), and the view lights the model as it
//! lights one under its own pointer.

use std::sync::Arc;

use camcraft_core::arrange::{Arrangement, Corner, Direction, Laid, PartLaid, SheetLayout};
use camcraft_core::job::{
    Arranged, Job, MOST_COPIES, Rotation, STEP_DEGREES, Seat, SheetPart, StockSpec,
};
use camcraft_geom::contour::Contour;
use camcraft_geom::mesh::distance_to_segment;
use eframe::egui;
use glam::{DVec2, dvec2};

use super::ViewPicks;
use super::parts::{Names, names_with};
use crate::document::{Document, SheetNow};
use crate::help::Help;
use crate::help::topics::stock as topics;
use crate::ui::defaults::Defaults;
use crate::ui::measure::Measure;
use crate::ui::theme;
use crate::ui::widgets::{self, Icon, error_text, form, hint, list_row_background};
use crate::worker::DocId;

/// How far in from the drawing's edges the sheet is drawn: room for the bar
/// beside it that shows how much of it is used.
const PAD: f32 = 16.0;
/// How tall the drawing may be, for a sheet narrow for its length or wide.
const SHORTEST: f32 = 96.0;
const TALLEST: f32 = 240.0;
/// How near a copy's outline, in points, the pointer is taken to be on it: a
/// small part is pointed at as easily as a large one.
const SLACK: f32 = 4.0;
/// How many colours the drawing tells parts apart by before they come round
/// again.
const TONES: usize = 4;
/// How bright what is drawn of an arrangement no longer current is.
const STALE: f32 = 0.35;

/// How a sheet is laid out, as its panel shows it: taken from the job before
/// the panel edits it, so it is drawn as it was laid however the panel's
/// fields change it this frame.
pub(super) struct Shown {
    stock: String,
    bed: Bed,
    /// Its parts' models in its order, which tells each its colour.
    order: Vec<String>,
    now: Now,
}

/// Where a sheet's arrangement stands.
enum Now {
    /// Laid as it is now.
    Laid(Arc<Arrangement>),
    /// Being arranged, or about to be: as it was last laid, if ever.
    Arranging(Option<Arc<Arrangement>>),
    /// Its arrangement ended without an answer, and why.
    Lost(String),
}

impl Shown {
    /// Sheet `stock` of `doc`, laid out as `a`, as it stands now; `None` for
    /// a stock that is not one of the job's sheets.
    pub fn of(doc: &Document, stock: &str, a: &Arranged) -> Option<Self> {
        let now = match doc.sheet_now(stock) {
            SheetNow::NotASheet => return None,
            SheetNow::Laid(arrangement, _) => Now::Laid(Arc::clone(arrangement)),
            SheetNow::Arranging => Now::Arranging(doc.sheet_last_laid(stock)),
            SheetNow::Lost(why) => Now::Lost(why.to_string()),
        };
        Some(Self {
            stock: stock.to_string(),
            bed: Bed::of(a),
            order: a.parts.iter().map(|p| p.model.clone()).collect(),
            now,
        })
    }

    /// The layout drawn: as it is now, or as it was last laid while it is
    /// being arranged again.
    fn drawn(&self) -> Option<&SheetLayout> {
        match &self.now {
            Now::Laid(a) | Now::Arranging(Some(a)) => a.sheet(&self.stock),
            Now::Arranging(None) | Now::Lost(_) => None,
        }
    }

    /// Each part of the layout drawn, where it was laid.
    fn parts(&self) -> &[PartLaid] {
        self.drawn().map_or(&[], SheetLayout::parts)
    }

    /// The layout as it is now, where it was laid: what is said of it is
    /// said only of that.
    fn current(&self) -> Option<&SheetLayout> {
        match &self.now {
            Now::Laid(a) => a
                .sheet(&self.stock)
                .filter(|s| matches!(s.laid, Laid::Done { .. })),
            Now::Arranging(_) | Now::Lost(_) => None,
        }
    }

    /// Part `model` as it is laid now, if it is.
    fn laid(&self, model: &str) -> Option<&PartLaid> {
        self.current()?.parts().iter().find(|p| p.model == model)
    }

    /// Which colour part `model`, the `k`th of the layout drawn, is drawn
    /// in: by its place among the sheet's parts now, so its row and its
    /// copies agree.
    fn tone_of(&self, model: &str, k: usize) -> usize {
        self.order.iter().position(|m| m == model).unwrap_or(k)
    }

    /// The box the drawing shows: the sheet, and anything drawn beyond it, as
    /// a layout from before the sheet was made smaller may be.
    fn bounds(&self) -> (DVec2, DVec2) {
        self.parts()
            .iter()
            .flat_map(|p| &p.copies)
            .filter_map(|c| c.outline.as_ref()?.bounds())
            .fold((self.bed.lo, self.bed.hi()), |(lo, hi), (a, b)| {
                (lo.min(a), hi.max(b))
            })
    }
}

/// Where a sheet lays its parts, seen from above.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Bed {
    /// Its front-left corner.
    lo: DVec2,
    /// Its length along X and its width along Y.
    size: DVec2,
    /// How far in from its edges its parts are kept.
    border: f64,
    /// The corner packing starts from.
    from: Corner,
    /// The axis it is used up along.
    along: Direction,
}

impl Bed {
    fn of(a: &Arranged) -> Self {
        let [x, y, _] = a.corner;
        let [length, width, _] = a.size;
        Self {
            lo: dvec2(x, y),
            size: dvec2(length, width),
            border: a.border,
            from: a.from,
            along: a.along,
        }
    }

    /// Its back-right corner.
    fn hi(&self) -> DVec2 {
        self.lo + self.size
    }

    /// Whether packing starts at its left, towards −X.
    fn left(&self) -> bool {
        matches!(self.from, Corner::FrontLeft | Corner::BackLeft)
    }

    /// Whether packing starts at its front, towards −Y.
    fn front(&self) -> bool {
        matches!(self.from, Corner::FrontLeft | Corner::FrontRight)
    }

    /// The corner packing starts from.
    fn start(&self) -> DVec2 {
        let hi = self.hi();
        dvec2(
            if self.left() { self.lo.x } else { hi.x },
            if self.front() { self.lo.y } else { hi.y },
        )
    }

    /// The line across it `used` along the way it is used up from the edge
    /// packing starts at, from the side packing starts at to the other: how
    /// far the parts placed reach.
    fn used_line(&self, used: f64) -> [DVec2; 2] {
        let (start, hi) = (self.start(), self.hi());
        match self.along {
            Direction::X => {
                let x = if self.left() {
                    self.lo.x + used
                } else {
                    hi.x - used
                };
                let other = if self.front() { hi.y } else { self.lo.y };
                [dvec2(x, start.y), dvec2(x, other)]
            }
            Direction::Y => {
                let y = if self.front() {
                    self.lo.y + used
                } else {
                    hi.y - used
                };
                let other = if self.left() { hi.x } else { self.lo.x };
                [dvec2(start.x, y), dvec2(other, y)]
            }
        }
    }

    /// Its edges inset by its border, round from the front-left corner;
    /// none for a border of nothing.
    fn border_line(&self) -> Option<[DVec2; 5]> {
        let inset = DVec2::splat(self.border);
        let (lo, hi) = (self.lo + inset, self.hi() - inset);
        (self.border > 0.0 && lo.cmplt(hi).all())
            .then(|| [lo, dvec2(hi.x, lo.y), hi, dvec2(lo.x, hi.y), lo])
    }
}

/// Points on the sheet onto the drawing: a box of them fitted into a
/// rectangle on the screen at one scale both ways and centred in it, +Y up
/// the screen, as the view has a sheet seen from above.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Fit {
    /// The box's middle on the sheet.
    middle: DVec2,
    /// Where that is on the screen.
    centre: egui::Pos2,
    /// Points on the screen to a millimetre on the sheet.
    scale: f64,
}

impl Fit {
    fn new(lo: DVec2, hi: DVec2, rect: egui::Rect) -> Self {
        let size = (hi - lo).max(DVec2::splat(1e-6));
        let scale = (f64::from(rect.width()) / size.x)
            .min(f64::from(rect.height()) / size.y)
            .max(1e-9);
        Self {
            middle: (lo + hi) / 2.0,
            centre: rect.center(),
            scale,
        }
    }

    /// Where `p` on the sheet is on the screen.
    fn at(&self, p: DVec2) -> egui::Pos2 {
        let d = (p - self.middle) * self.scale;
        // Within the panel, a few hundred points at most.
        #[allow(clippy::cast_possible_truncation)]
        egui::pos2(self.centre.x + d.x as f32, self.centre.y - d.y as f32)
    }

    /// Where `p` on the screen is on the sheet: [`at`](Self::at) undone.
    fn back(&self, p: egui::Pos2) -> DVec2 {
        let d = p - self.centre;
        self.middle + dvec2(f64::from(d.x), -f64::from(d.y)) / self.scale
    }

    /// How far `points` on the screen are on the sheet.
    fn length(&self, points: f32) -> f64 {
        f64::from(points) / self.scale
    }
}

/// Which of `parts` has a copy at `p`, by its place among them; or, where
/// none has, the one whose copy's outline passes nearest within `slack` of
/// it. A part standing in another's hole is the one at a point in the hole,
/// as the other's outline has a hole there.
fn part_at(parts: &[PartLaid], p: DVec2, slack: f64) -> Option<usize> {
    let copies = || {
        parts.iter().enumerate().flat_map(|(k, part)| {
            part.copies
                .iter()
                .filter_map(move |c| Some((k, c.outline.as_ref()?)))
        })
    };
    let near = |(lo, hi): (DVec2, DVec2)| {
        let reach = DVec2::splat(slack);
        p.cmpge(lo - reach).all() && p.cmple(hi + reach).all()
    };
    let nearby = || copies().filter(|(_, o)| o.bounds().is_some_and(near));
    if let Some((k, _)) = nearby().find(|(_, o)| o.contains(p)) {
        return Some(k);
    }
    nearby()
        .filter_map(|(k, o)| {
            let d = o
                .rings()
                .iter()
                .flat_map(Contour::edges)
                .map(|(a, b)| distance_to_segment(p, a, b))
                .fold(f64::INFINITY, f64::min);
            (d <= slack).then_some((k, d))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(k, _)| k)
}

/// What the line under the drawing says of `sheet`, where it was laid: how
/// many copies it places of how many its parts ask for, and how far along it
/// they reach of how long it is, in `m`'s units.
fn caption(sheet: &SheetLayout, m: Measure) -> Option<String> {
    let Laid::Done { used, length, .. } = &sheet.laid else {
        return None;
    };
    let (placed, copies) = sheet.placed();
    Some(format!(
        "{placed} of {copies} placed · {} of {} used",
        m.number(*used, 0),
        m.length(*length, 0)
    ))
}

/// How many of part `p`'s copies are placed, of how many it asks for.
fn placed_text(p: &PartLaid) -> String {
    format!("{} of {}", p.placed(), p.copies.len())
}

/// Why any of part `p`'s copies are left out, as the checks say it: a line
/// for each reason, and what the checks add to it.
fn left_out(p: &PartLaid) -> Vec<(String, &'static str)> {
    let n = p.copies.len();
    let mut out = Vec::new();
    let too_large = p.too_large();
    if too_large > 0 {
        let why = if too_large == n {
            "too large for the sheet".to_string()
        } else {
            format!("{too_large} of {n} too large for the sheet")
        };
        out.push((
            why,
            "it does not fit inside the border at any turn it may take",
        ));
    }
    let no_room = p.no_room();
    if no_room > 0 {
        out.push(if n == 1 {
            (
                "no room beside the others".to_string(),
                "it fits alone, but not beside the others",
            )
        } else {
            (
                format!("no room for {no_room} of {n}"),
                "each fits alone, but not beside the others",
            )
        });
    }
    out
}

/// The models on the job's sheets other than `stock`, which a model on one
/// keeps off this one, and what the parts card's popup says when they are
/// all it has left to offer; `None` where no other sheet holds any.
pub(super) fn on_other_sheets(job: &Job, stock: &str) -> Option<(Vec<String>, String)> {
    let mut models: Vec<String> = Vec::new();
    let mut sheets: Vec<String> = Vec::new();
    for s in job.stocks.iter().filter(|s| s.name != stock) {
        let StockSpec::Arranged(a) = &s.spec else {
            continue;
        };
        for p in &a.parts {
            if !models.contains(&p.model) {
                models.push(p.model.clone());
                if !sheets.contains(&s.name) {
                    sheets.push(s.name.clone());
                }
            }
        }
    }
    let named: Vec<String> = sheets.iter().map(|s| format!("'{s}'")).collect();
    let why = match named.as_slice() {
        [] => return None,
        [one] => format!("The others are on sheet {one}: a model goes on one sheet only."),
        _ => format!(
            "The others are on sheets {}: a model goes on one sheet only.",
            named.join(", ")
        ),
    };
    Some((models, why))
}

/// The colour part `k`'s outlines are drawn in: the theme's text, and
/// blue and green sunk halfway into it, so parts are told apart without any
/// shouting, none the amber of the sheet and of what is lit, nor the red of
/// what is left out.
fn tone(k: usize) -> egui::Color32 {
    let text = theme::text_2();
    theme::egui_color(match k % TONES {
        0 => text,
        1 => theme::mix(text, theme::PATH_LEAD, 0.5),
        2 => theme::mix(text, theme::ok(), 0.5),
        _ => theme::text_3(),
    })
}

/// The id a sheet's chosen part is kept under, from frame to frame: its
/// model's name.
pub(crate) fn chosen_id(doc: DocId, stock: &str) -> egui::Id {
    egui::Id::new(("sheet-part", doc, stock))
}

/// What a sheet's panel needs of the job besides the sheet.
pub(super) struct Context<'a> {
    /// The job.
    pub doc: DocId,
    /// The sheet's name in it.
    pub stock: &'a str,
    /// Where its parts card keeps its popup's state.
    pub id: egui::Id,
    /// Every model of the job, in order.
    pub models: &'a [String],
    /// The models on other sheets, and why they are not offered, if any are
    /// ([`on_other_sheets`]).
    pub elsewhere: Option<&'a (Vec<String>, String)>,
    /// How it is laid out now.
    pub shown: Option<Shown>,
}

/// Sheet `a`'s panel, its settings against `d`: the drawing of it with what
/// it places and leaves out, its settings and its parts card. The model whose
/// copies it lights, if any, for the view to light too.
pub(super) fn panel(
    ui: &mut egui::Ui,
    d: &mut Defaults<StockSpec>,
    m: Measure,
    a: &mut Arranged,
    cx: &Context<'_>,
) -> Option<String> {
    let key = chosen_id(cx.doc, cx.stock);
    // Only a part it has can be chosen.
    let mut chosen = ui
        .data(|data| data.get_temp::<String>(key))
        .filter(|c| a.holds(c));

    let drawing = cx.shown.as_ref().map(|s| (Drawing::allocate(ui, s), s));
    let pointed: Option<String> = drawing
        .as_ref()
        .and_then(|(dr, s)| dr.pointed(s))
        .map(str::to_string);
    if drawing
        .as_ref()
        .is_some_and(|(dr, _)| dr.response.clicked())
    {
        chosen.clone_from(&pointed);
    }
    if let Some(s) = &cx.shown {
        ui.add_space(theme::SPACE_3);
        status(ui, s, m, &mut chosen);
    }
    ui.add_space(theme::SPACE_4);
    sheet_fields(ui, d, m, a);
    ui.add_space(theme::SPACE_4);

    let mut row_hovered = None;
    let mut parts: Vec<String> = a.parts.iter().map(|p| p.model.clone()).collect();
    let sheet_turns = a.rotation;
    names_with(
        ui,
        &Names {
            id: cx.id,
            title: topics::PARTS.title,
            topic: Some(&topics::PARTS),
            all: cx.models,
            empty: "None yet: add the models it lays out.",
            pick: None,
            more: None,
            withheld: cx
                .elsewhere
                .map(|(models, why)| (models.as_slice(), why.as_str())),
        },
        &mut parts,
        ViewPicks::default(),
        &mut Vec::new(),
        |ui, k, name| {
            let Some(p) = a.parts.get_mut(k) else {
                return false;
            };
            let shown = cx.shown.as_ref();
            let row = PartRow {
                k,
                laid: shown.and_then(|s| s.laid(name)),
                missing: !cx.models.iter().any(|x| x == name),
                lit: pointed.as_deref() == Some(name),
                chosen: chosen.as_deref() == Some(name),
            };
            let out = part_row(ui, d, p, sheet_turns, &row);
            if out.hovered {
                row_hovered = Some(name.to_string());
            }
            if out.clicked {
                chosen = (!row.chosen).then(|| name.to_string());
            }
            out.remove
        },
    );
    // Each part kept as it is, those added after.
    a.parts.retain(|p| parts.contains(&p.model));
    for p in &parts {
        if !a.holds(p) {
            a.parts.push(SheetPart::of(p));
        }
    }

    let lit = pointed.or(row_hovered).or_else(|| chosen.clone());
    if let Some((dr, s)) = &drawing {
        dr.paint(ui, s, lit.as_deref());
    }
    ui.data_mut(|data| match chosen {
        Some(c) => {
            data.insert_temp(key, c);
        }
        None => data.remove::<String>(key),
    });
    lit
}

/// What is said under the drawing: how much the sheet places and how much of
/// it is used, then each part it leaves copies of out, in red with why, a
/// click on which chooses the part; or that it is being arranged.
fn status(ui: &mut egui::Ui, s: &Shown, m: Measure, chosen: &mut Option<String>) {
    if let Now::Arranging(Some(_)) = s.now {
        hint(ui, "Being arranged: drawn as it was last laid.");
        return;
    }
    let Some(sheet) = s.current() else {
        return;
    };
    if let Some(text) = caption(sheet, m) {
        ui.label(widgets::mono(text, theme::TEXT_SM).color(theme::egui_color(theme::text_2())));
    }
    for p in sheet.left_out() {
        for (why, detail) in left_out(p) {
            let text = error_text(format!("'{}': {why}", p.model));
            let r = ui
                .add(egui::Label::new(text).wrap().sense(egui::Sense::click()))
                .on_hover_text(detail)
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if r.clicked() {
                *chosen = Some(p.model.clone());
            }
        }
    }
}

/// The drawing's place in the panel, and the sheet's on it.
struct Drawing {
    rect: egui::Rect,
    fit: Fit,
    response: egui::Response,
}

impl Drawing {
    /// Room for `s` across the panel, as tall as its sheet is for its
    /// length, within reason.
    fn allocate(ui: &mut egui::Ui, s: &Shown) -> Self {
        let width = ui.available_width();
        let (lo, hi) = s.bounds();
        let e = hi - lo;
        // A ratio of lengths, near one.
        #[allow(clippy::cast_possible_truncation)]
        let aspect = (e.y / e.x.max(1e-6)) as f32;
        let tall = ((width - 2.0 * PAD) * aspect).clamp(SHORTEST, TALLEST) + 2.0 * PAD;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, tall), egui::Sense::click());
        Self {
            rect,
            fit: Fit::new(lo, hi, rect.shrink(PAD)),
            response,
        }
    }

    /// The part of `s` under the pointer, by its model.
    fn pointed<'a>(&self, s: &'a Shown) -> Option<&'a str> {
        let at = self.response.hover_pos()?;
        let parts = s.parts();
        let k = part_at(parts, self.fit.back(at), self.fit.length(SLACK))?;
        parts.get(k).map(|p| p.model.as_str())
    }

    /// `s` drawn, part `lit`'s copies lit: the sheet in amber with its
    /// border dashed inside it, the corner packing starts from marked, each
    /// copy's outline as it stands in its part's colour, and beside the
    /// sheet how far along it they reach. A layout no longer current is
    /// drawn faint, with a spinner; where there is none, what there is
    /// instead is said in its place.
    fn paint(&self, ui: &egui::Ui, s: &Shown, lit: Option<&str>) {
        let painter = ui.painter_at(self.rect);
        painter.rect_filled(self.rect, theme::RADIUS_SM, theme::egui_color(theme::bg()));
        widgets::grid_background(&painter, self.rect);
        painter.rect_stroke(
            self.rect,
            theme::RADIUS_SM,
            egui::Stroke::new(1.0, theme::egui_color(theme::border())),
            egui::StrokeKind::Inside,
        );
        let current = matches!(s.now, Now::Laid(_));
        let shade = |c: egui::Color32| {
            if current { c } else { c.gamma_multiply(STALE) }
        };
        let (fit, bed) = (&self.fit, &s.bed);
        let accent = theme::egui_color(theme::accent());
        let sheet = egui::Rect::from_two_pos(fit.at(bed.lo), fit.at(bed.hi()));
        painter.rect(
            sheet,
            0.0,
            accent.gamma_multiply(0.08),
            egui::Stroke::new(1.5, accent),
            egui::StrokeKind::Inside,
        );
        if let Some(line) = bed.border_line() {
            let points: Vec<egui::Pos2> = line.iter().map(|p| fit.at(*p)).collect();
            painter.extend(egui::Shape::dashed_line(
                &points,
                egui::Stroke::new(1.0, accent.gamma_multiply(0.5)),
                4.0,
                3.0,
            ));
        }

        for (k, part) in s.parts().iter().enumerate() {
            let color = tone(s.tone_of(&part.model, k));
            let stroke = match lit {
                Some(l) if l == part.model => {
                    egui::Stroke::new(2.0, theme::egui_color(theme::accent_hover()))
                }
                Some(_) => egui::Stroke::new(1.0, color.gamma_multiply(0.5)),
                None => egui::Stroke::new(1.25, color),
            };
            let stroke = egui::Stroke::new(stroke.width, shade(stroke.color));
            for ring in part
                .copies
                .iter()
                .filter_map(|c| c.outline.as_ref())
                .flat_map(|o| o.rings())
            {
                let points = ring.points().iter().map(|p| fit.at(*p)).collect();
                painter.add(egui::Shape::closed_line(points, stroke));
            }
        }

        if let Some(Laid::Done { used, .. }) = s.drawn().map(|l| &l.laid)
            && *used > 0.0
        {
            self.paint_used(&painter, bed, *used, shade);
        }
        // The corner packing starts from: a small amber triangle in it.
        let corner = fit.at(bed.start());
        let inward = egui::vec2(
            if bed.left() { 9.0 } else { -9.0 },
            if bed.front() { -9.0 } else { 9.0 },
        );
        painter.add(egui::Shape::convex_polygon(
            vec![
                corner,
                corner + egui::vec2(inward.x, 0.0),
                corner + egui::vec2(0.0, inward.y),
            ],
            accent,
            egui::Stroke::NONE,
        ));

        // In the drawing's corner, clear of the sheet.
        if matches!(s.now, Now::Arranging(_)) {
            let at = egui::Rect::from_min_size(
                self.rect.right_top() + egui::vec2(-PAD + 1.0, 2.0),
                egui::vec2(PAD - 4.0, PAD - 4.0),
            );
            egui::Spinner::new().paint_at(ui, at);
        }
        if let Some((text, color)) = said_instead(s) {
            let galley = painter.layout(
                text,
                egui::FontId::proportional(theme::TEXT_SM),
                color,
                (sheet.width() - 2.0 * theme::SPACE_4).max(40.0),
            );
            let at = sheet.center() - galley.size() / 2.0;
            painter.galley(at, galley, color);
        }
    }

    /// How far along `bed` its parts reach, `used`: a dashed line across it
    /// there, and an amber bar beside the edge packing runs along, from the
    /// corner it starts at to the line.
    fn paint_used(
        &self,
        painter: &egui::Painter,
        bed: &Bed,
        used: f64,
        shade: impl Fn(egui::Color32) -> egui::Color32,
    ) {
        let fit = &self.fit;
        let [a, b] = bed.used_line(used.min(match bed.along {
            Direction::X => bed.size.x,
            Direction::Y => bed.size.y,
        }));
        painter.extend(egui::Shape::dashed_line(
            &[fit.at(a), fit.at(b)],
            egui::Stroke::new(1.0, shade(theme::egui_color(theme::text_3()))),
            3.0,
            3.0,
        ));
        let out = match bed.along {
            Direction::X if bed.front() => egui::vec2(0.0, 5.0),
            Direction::X => egui::vec2(0.0, -5.0),
            Direction::Y if bed.left() => egui::vec2(-5.0, 0.0),
            Direction::Y => egui::vec2(5.0, 0.0),
        };
        painter.line_segment(
            [fit.at(bed.start()) + out, fit.at(a) + out],
            egui::Stroke::new(3.0, shade(theme::egui_color(theme::accent()))),
        );
    }
}

/// What the drawing says in place of a layout, where it has none to show,
/// and in what colour: the parts it waits for, why it cannot be arranged,
/// or that it is being.
fn said_instead(s: &Shown) -> Option<(String, egui::Color32)> {
    let quiet = theme::egui_color(theme::text_3());
    let err = widgets::Level::Err.color();
    match &s.now {
        Now::Arranging(None) => Some(("Arranging…".into(), quiet)),
        Now::Arranging(Some(_)) => None,
        Now::Lost(why) => Some((format!("It could not be arranged: {why}."), err)),
        Now::Laid(_) => match s.drawn().map(|l| &l.laid) {
            Some(Laid::Waiting(missing)) => {
                let names: Vec<String> = missing.iter().map(|n| format!("'{n}'")).collect();
                Some((format!("It waits for {} to load.", names.join(", ")), quiet))
            }
            Some(Laid::Failed(e)) => Some((format!("It cannot be arranged: {e}."), err)),
            Some(Laid::Done { .. }) | None => None,
        },
    }
}

/// A sheet's settings, a row each: its size and corner, the gaps it keeps,
/// the turns its parts may take, where packing starts and which way it goes,
/// and the face its parts stand on.
fn sheet_fields(ui: &mut egui::Ui, d: &mut Defaults<StockSpec>, m: Measure, a: &mut Arranged) {
    form(ui, "sheet", |ui| {
        d.row(ui, &topics::SHEET_SIZE, "size", |ui| {
            for (axis, v) in ["X ", "Y ", "Z "].into_iter().zip(a.size.iter_mut()) {
                m.length_field(ui, v, 1.0, 100_000.0, axis);
            }
        });
        d.row(ui, &topics::CORNER, "corner", |ui| {
            for (axis, v) in ["X ", "Y ", "Z "].into_iter().zip(a.corner.iter_mut()) {
                m.signed_length_field(ui, v, 1.0, 100_000.0, axis);
            }
        });
        d.row(ui, &topics::SPACING, "spacing", |ui| {
            m.length_field(ui, &mut a.spacing, 0.1, 1000.0, "")
        });
        d.row(ui, &topics::BORDER, "border", |ui| {
            m.length_field(ui, &mut a.border, 0.1, 1000.0, "")
        });
        d.row(ui, &topics::ROTATION, "rotation", |ui| {
            rotation_choice(ui, "sheet-rotation", &mut a.rotation);
        });
        if let Rotation::Step(step) = &mut a.rotation {
            d.row(ui, &topics::STEP, "rotation.step", |ui| {
                ui.add(step_field(step))
            });
        }
        d.row(ui, &topics::FROM, "from", |ui| {
            egui::ComboBox::from_id_salt("sheet-from")
                .selected_text(corner_text(a.from))
                .show_ui(ui, |ui| {
                    for c in [
                        Corner::FrontLeft,
                        Corner::FrontRight,
                        Corner::BackLeft,
                        Corner::BackRight,
                    ] {
                        ui.selectable_value(&mut a.from, c, corner_text(c));
                    }
                })
                .response
        });
        d.row(ui, &topics::ALONG, "along", |ui| {
            widgets::segmented(
                ui,
                &mut a.along,
                &[(Direction::X, "X"), (Direction::Y, "Y")],
            )
        });
        d.row(ui, &topics::SEAT, "seat", |ui| {
            widgets::segmented(
                ui,
                &mut a.seat,
                &[(Seat::Bottom, "bottom"), (Seat::Top, "top")],
            )
        });
    });
}

/// A field for a step between turns, in degrees.
fn step_field(step: &mut f64) -> egui::DragValue<'_> {
    widgets::number(step)
        .speed(1.0)
        .range(*STEP_DEGREES.start()..=*STEP_DEGREES.end())
        .suffix("°")
}

/// A corner of a sheet, as a person names it.
fn corner_text(c: Corner) -> &'static str {
    match c {
        Corner::FrontLeft => "front left",
        Corner::FrontRight => "front right",
        Corner::BackLeft => "back left",
        Corner::BackRight => "back right",
    }
}

/// The turns a sheet's parts may take. A step chosen starts at 15 degrees; one
/// chosen again keeps the step it had.
fn rotation_choice(ui: &mut egui::Ui, id: impl egui::AsIdSalt, r: &mut Rotation) {
    let mut given = Some(*r);
    turns_choice(ui, id, &mut given, None);
    if let Some(chosen) = given {
        *r = chosen;
    }
}

/// What a person calls a way of turning.
fn rotation_text(r: &Rotation) -> &'static str {
    match r {
        Rotation::None => "none",
        Rotation::HalfTurn => "half turn",
        Rotation::QuarterTurns => "quarter turns",
        Rotation::Step(_) => "by a step",
    }
}

/// The turns something may take, where `sheet` is the sheet's, for a part
/// that may take the sheet's instead, `None`: what `r` is then. A step chosen
/// starts at 15 degrees; one chosen again keeps the step it had. The choice's
/// response.
fn turns_choice(
    ui: &mut egui::Ui,
    id: impl egui::AsIdSalt,
    r: &mut Option<Rotation>,
    sheet: Option<Rotation>,
) -> egui::Response {
    let text = |r: &Option<Rotation>| match (r, sheet) {
        (Some(r), _) => rotation_text(r).to_string(),
        (None, Some(s)) => format!("as the sheet ({})", rotation_text(&s)),
        (None, None) => String::new(),
    };
    let step = match *r {
        Some(Rotation::Step(s)) => s,
        _ => 15.0,
    };
    let mut choices = Vec::new();
    if sheet.is_some() {
        choices.push((None, "Turned as the sheet allows its parts"));
    }
    choices.extend([
        (Some(Rotation::None), "Each part as it stands"),
        (
            Some(Rotation::HalfTurn),
            "As it stands, or turned half round",
        ),
        (
            Some(Rotation::QuarterTurns),
            "By any number of quarter turns",
        ),
        (Some(Rotation::Step(step)), "By any whole number of a step"),
    ]);
    let kind = |r: &Option<Rotation>| r.as_ref().map(std::mem::discriminant);
    egui::ComboBox::from_id_salt(id)
        .selected_text(text(r))
        .show_ui(ui, |ui| {
            for (choice, why) in choices {
                let on = kind(r) == kind(&choice);
                if ui
                    .selectable_label(on, text(&choice))
                    .on_hover_text(why)
                    .clicked()
                    && !on
                {
                    *r = choice;
                }
            }
        })
        .response
}

/// What one part's row of the parts card shows of it besides its settings.
struct PartRow<'a> {
    /// Its place among the sheet's parts, which names its settings.
    k: usize,
    /// How it is laid now, if it is.
    laid: Option<&'a PartLaid>,
    /// Whether its model is gone from the job.
    missing: bool,
    /// Whether one of its copies is under the pointer in the drawing.
    lit: bool,
    /// Whether it is the part chosen.
    chosen: bool,
}

/// What was done to a part's row.
struct RowDone {
    remove: bool,
    hovered: bool,
    clicked: bool,
}

/// How tall a part's row is: two lines of controls.
const ROW: f32 = 2.0 * theme::CONTROL_SM + theme::SPACE_2;

/// Part `p`'s row of the parts card, its settings against `d`: a swatch of
/// its colour in the drawing, its model, how many of its copies are placed
/// and a way to take it out; under them, how many copies it asks for and the
/// turns they may take, the sheet's, `sheet`, or its own. On `--hover-overlay`
/// while it or one of its copies is under the pointer, and marked as chosen
/// while it is; a click on it chooses it, or no part if it was.
fn part_row(
    ui: &mut egui::Ui,
    d: &mut Defaults<StockSpec>,
    p: &mut SheetPart,
    sheet: Rotation,
    row: &PartRow<'_>,
) -> RowDone {
    let k = row.k;
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW), egui::Sense::hover());
    // Under the row's own controls, so they take their clicks.
    let click = ui.interact(rect, ui.id().with(("part-row", k)), egui::Sense::click());
    let hovered = ui.rect_contains_pointer(rect);
    list_row_background(ui, rect, row.chosen, hovered || row.lit);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(theme::SPACE_3, 0.0)))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.spacing_mut().item_spacing.y = theme::SPACE_2;
    let line = egui::vec2(child.available_width(), theme::CONTROL_SM);
    let mut remove = false;
    child.allocate_ui_with_layout(
        line,
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            let (swatch, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().rect_stroke(
                swatch,
                0.0,
                egui::Stroke::new(1.5, tone(k)),
                egui::StrokeKind::Inside,
            );
            let name = egui::RichText::new(&p.model).color(if row.missing {
                widgets::Level::Err.color()
            } else {
                theme::egui_color(theme::text_1())
            });
            let label = ui.label(name);
            if row.missing {
                label.on_hover_text("not in the job");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                remove = widgets::icon_button(ui, Icon::X, "Remove").clicked();
                if let Some(laid) = row.laid {
                    let short = laid.placed() < laid.copies.len();
                    let color = if short {
                        widgets::Level::Err.color()
                    } else {
                        theme::egui_color(theme::text_3())
                    };
                    ui.label(widgets::mono(placed_text(laid), theme::TEXT_SM).color(color))
                        .on_hover_text("of its copies, how many the sheet places");
                }
            });
        },
    );
    child.allocate_ui_with_layout(
        line,
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.add_space(10.0 + ui.spacing().item_spacing.x);
            let unit = if p.copies == 1 { " copy" } else { " copies" };
            ui.add(
                widgets::number(&mut p.copies)
                    .speed(0.1)
                    .range(1..=MOST_COPIES)
                    .suffix(unit),
            )
            .help(&topics::COPIES);
            d.reset(ui, &format!("parts.{k}.copies"));
            turns_choice(ui, ("part-grain", k), &mut p.rotation, Some(sheet)).help(&topics::GRAIN);
            d.reset(ui, &format!("parts.{k}.rotation"));
            if let Some(Rotation::Step(step)) = &mut p.rotation {
                ui.add(step_field(step)).help(&topics::GRAIN_STEP);
                d.reset(ui, &format!("parts.{k}.rotation.step"));
            }
        },
    );
    RowDone {
        remove,
        hovered,
        clicked: click.clicked(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::indexing_slicing)]
mod tests;
