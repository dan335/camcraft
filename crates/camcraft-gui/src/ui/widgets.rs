//! The design system's components, for egui.
//!
//! Each follows a component in `design/system/components/`, named in its doc, and
//! takes its colours and sizes from `theme`. egui's own widgets already wear the
//! system through `theme::apply`; these are the ones egui has no equivalent for, or
//! whose look depends on more than one state (a primary button lightens on hover,
//! a selected tab carries an amber bar).

use eframe::egui;

use crate::help::{self, Help, Topic};
use crate::ui::theme::{self, Tint};

/// The text size of controls at their default size, as `Button.jsx`, `Input.jsx`
/// and `Tabs.jsx` set it: a step under body text, so a row of controls reads
/// quieter than what it edits.
const CONTROL_TEXT: f32 = 13.0;
/// The text size of small controls and tags, as `Button.jsx` (sm) and `Tag.jsx`.
const SMALL_TEXT: f32 = theme::TEXT_SM;
/// The height of a tab, as `Tabs.jsx` gives it at its default size.
const TAB_HEIGHT: f32 = 34.0;
/// The height of a card's header row, as `Card.jsx`.
const CARD_HEADER: f32 = 32.0;
/// The size of a status dot: a square, never a circle, as `readme.md` gives it.
const DOT: f32 = 6.0;
/// The room a status dot takes in a row: a small spinner's, so one can stand in
/// for the other while something runs.
const DOT_SLOT: f32 = 10.0;
/// The size of an icon in a control.
const ICON: f32 = 16.0;

/// A Lucide icon, drawn from the bundled font (`assets/fonts/lucide-subset.ttf`).
/// Only these glyphs are in the font; see `assets/fonts/README.md` to add one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Plus,
    X,
    ChevronUp,
    ChevronDown,
    ChevronRight,
    Copy,
    Trash,
    Search,
    Upload,
    FolderPlus,
    Folder,
    FolderInput,
    ArrowDown,
    Play,
    Pause,
    SkipBack,
    SkipForward,
    StepBack,
    StepForward,
    Rewind,
    ChevronFirst,
    ChevronLast,
    MousePointerClick,
}

impl Icon {
    /// Its codepoint in Lucide's font.
    pub fn glyph(self) -> char {
        match self {
            Icon::Plus => '\u{e13d}',
            Icon::X => '\u{e1b2}',
            Icon::ChevronUp => '\u{e070}',
            Icon::ChevronDown => '\u{e06d}',
            Icon::ChevronRight => '\u{e06f}',
            Icon::Copy => '\u{e09e}',
            Icon::Trash => '\u{e18e}',
            Icon::Search => '\u{e151}',
            Icon::Upload => '\u{e19e}',
            Icon::FolderPlus => '\u{e0d9}',
            Icon::Folder => '\u{e0d7}',
            Icon::FolderInput => '\u{e334}',
            Icon::ArrowDown => '\u{e042}',
            Icon::Play => '\u{e13c}',
            Icon::Pause => '\u{e12e}',
            Icon::SkipBack => '\u{e15f}',
            Icon::SkipForward => '\u{e160}',
            Icon::StepBack => '\u{e3e9}',
            Icon::StepForward => '\u{e3ea}',
            Icon::Rewind => '\u{e147}',
            Icon::ChevronFirst => '\u{e243}',
            Icon::ChevronLast => '\u{e244}',
            Icon::MousePointerClick => '\u{e120}',
        }
    }

    /// The icon as text, at `size`, to lay out or paint.
    pub fn text(self, size: f32) -> egui::RichText {
        egui::RichText::new(self.glyph().to_string()).size(size)
    }
}

/// How serious something is: the system's status colours, which double as the
/// toolpath palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    Err,
}

impl Level {
    pub fn color(self) -> egui::Color32 {
        theme::egui_color(match self {
            Level::Ok => theme::OK,
            Level::Warn => theme::WARN,
            Level::Err => theme::ERR,
        })
    }

    /// The word a diagnostic leads with, in the system's terminal casing.
    fn word(self) -> &'static str {
        match self {
            Level::Ok => "ok",
            Level::Warn => "warn",
            Level::Err => "ERROR",
        }
    }
}

/// A button's role, as `Button.jsx` gives them: one amber primary per view,
/// everything else secondary or ghost, danger only for what cannot be undone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

/// A control's height: `--control-sm` in dense rows, `--control-md` in toolbars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Sm,
    Md,
}

impl Size {
    fn height(self) -> f32 {
        match self {
            Size::Sm => theme::CONTROL_SM,
            Size::Md => theme::CONTROL_MD,
        }
    }

    fn text(self) -> f32 {
        match self {
            Size::Sm => SMALL_TEXT,
            Size::Md => CONTROL_TEXT,
        }
    }

    fn padding(self) -> f32 {
        match self {
            Size::Sm => theme::SPACE_3,
            Size::Md => theme::SPACE_4,
        }
    }
}

/// Text in IBM Plex Sans at 500, the weight of labels, tabs and buttons.
pub fn medium(text: impl Into<String>, size: f32) -> egui::RichText {
    egui::RichText::new(text)
        .size(size)
        .family(egui::FontFamily::Name(theme::MEDIUM.into()))
}

/// Text in Fira Code: for numbers, identifiers, paths and anything a machine
/// reads, as `readme.md` puts it.
pub fn mono(text: impl Into<String>, size: f32) -> egui::RichText {
    egui::RichText::new(text)
        .size(size)
        .family(egui::FontFamily::Monospace)
}

/// The small uppercase label over a card or a table's columns: 11 px, 500,
/// spread by `--tracking-wide`, in `--text-3`. `Card.jsx`'s header.
pub fn section_label(text: &str) -> egui::RichText {
    medium(text.to_uppercase(), theme::TEXT_XS)
        .extra_letter_spacing(theme::TEXT_XS * theme::TRACKING_WIDE)
        .color(theme::egui_color(theme::TEXT_3))
}

/// A key in a form: `--text-3`, as `KeyValue.jsx` sets its keys. For what is
/// only shown; a field that edits is labelled by [`field_label`] or [`row`],
/// which give it its help.
pub fn key_label(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.label(egui::RichText::new(text).color(theme::egui_color(theme::TEXT_3)))
}

/// A field's label in a form, named by `topic`, whose help it shows on hover.
pub fn field_label(ui: &mut egui::Ui, topic: &Topic) -> egui::Response {
    field_label_as(ui, topic.title, topic)
}

/// As [`field_label`], reading `text` where the form names the field its own
/// way, as a row under a column's heading does.
pub fn field_label_as(ui: &mut egui::Ui, text: &str, topic: &Topic) -> egui::Response {
    key_label(ui, text).help(topic)
}

/// A row of a [`form`]: `topic`'s label, then what `add` puts beside it, with
/// the help shown on hovering either, as Fusion shows it.
pub fn row<R>(ui: &mut egui::Ui, topic: &Topic, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    row_as(ui, topic.title, topic, add)
}

/// As [`row`], labelled `text`.
pub fn row_as<R>(
    ui: &mut egui::Ui,
    text: &str,
    topic: &Topic,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let label = key_label(ui, text);
    let field = ui.horizontal(add);
    // Laid over both once they are drawn, so it is hovered over the field's
    // own widgets too.
    let id = ui.id().with(("help", text));
    help::over(ui, label.rect.union(field.response.rect), id, topic);
    ui.end_row();
    field.inner
}

/// Text that explains, quieter than what it explains: `--text-3`, as `readme.md`
/// gives everything but the value the reader compares.
pub fn hint_text(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).color(theme::egui_color(theme::TEXT_3))
}

/// [`hint_text`] on its own line, wrapped to the width there is.
pub fn hint(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Label::new(hint_text(text)).wrap())
}

/// Text that says something is wrong: `--err`.
pub fn error_text(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).color(Level::Err.color())
}

/// [`error_text`] on its own line, wrapped to the width there is.
pub fn error(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Label::new(error_text(text)).wrap())
}

/// A value that may be left out: `absent`, quiet, saying what is used without it,
/// to click to give it `default`; or its field, from `add`, and a button to take
/// it out again. In place of a checkbox beside every field.
pub fn optional<T>(
    ui: &mut egui::Ui,
    value: &mut Option<T>,
    default: T,
    absent: &str,
    add: impl FnOnce(&mut egui::Ui, &mut T),
) {
    let clear = if let Some(v) = value.as_mut() {
        add(ui, v);
        icon_button(ui, Icon::X, &format!("clear: {absent}")).clicked()
    } else {
        // `--text-4`, the quietest, as the system gives "not given".
        let given = ui
            .add(
                egui::Button::new(
                    egui::RichText::new(absent).color(theme::egui_color(theme::TEXT_4)),
                )
                .frame(false),
            )
            .on_hover_text("Give it")
            .clicked();
        if given {
            *value = Some(default);
        }
        false
    };
    if clear {
        *value = None;
    }
}

/// One of `options`, as `Select.jsx`: the one chosen, and the rest in a list
/// that drops from it, each read as `name` gives it. The response is the
/// control's, changed when another is chosen.
pub fn select<T: Clone + PartialEq, W: Into<egui::WidgetText>>(
    ui: &mut egui::Ui,
    id: impl egui::AsIdSalt,
    value: &mut T,
    options: &[T],
    name: impl Fn(&T) -> W,
) -> egui::Response {
    let mut changed = false;
    let mut response = egui::ComboBox::from_id_salt(id)
        .selected_text(name(value))
        .show_ui(ui, |ui| {
            for option in options {
                changed |= ui
                    .selectable_value(value, option.clone(), name(option))
                    .changed();
            }
        })
        .response;
    if changed {
        response.mark_changed();
    }
    response
}

/// One of a few, all in sight side by side: a segmented control, the one chosen
/// lit. Each of `options` is a value and what it reads as. The response covers
/// them all, changed when another is chosen.
pub fn segmented<T: Clone + PartialEq>(
    ui: &mut egui::Ui,
    value: &mut T,
    options: &[(T, &str)],
) -> egui::Response {
    let mut changed = false;
    let mut response = ui
        .horizontal(|ui| {
            for (option, name) in options {
                changed |= ui.selectable_value(value, option.clone(), *name).changed();
            }
        })
        .response;
    if changed {
        response.mark_changed();
    }
    response
}

/// What a dialog holds, as `Dialog.jsx`, in the window that holds it (an
/// `egui::Modal` wears the system's): its title over a hairline, `body`, and
/// under another hairline `footer`'s buttons on the right. They are laid out
/// from the right, so the answer goes in first and Cancel after it, on its left.
pub fn dialog<R>(
    ui: &mut egui::Ui,
    title: &str,
    body: impl FnOnce(&mut egui::Ui),
    footer: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.label(
        medium(title, theme::TEXT_BASE)
            .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
            .color(theme::egui_color(theme::TEXT_1)),
    );
    ui.add_space(theme::SPACE_3);
    ui.separator();
    ui.add_space(theme::SPACE_3);
    body(ui);
    ui.add_space(theme::SPACE_4);
    ui.separator();
    ui.add_space(theme::SPACE_3);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), footer)
            .inner
    })
    .inner
}

/// A form: labels in a column on the left, their fields lined up on the right.
pub fn form(ui: &mut egui::Ui, id: impl egui::AsIdSalt, body: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([theme::SPACE_4, 6.0])
        .min_col_width(96.0)
        .show(ui, body);
}

/// A button, as `Button.jsx`: sharp, flat, and shaded by state rather than
/// resized. Disabled, it fades to 40% with the rest of a disabled `Ui`.
pub fn button(
    ui: &mut egui::Ui,
    variant: Variant,
    size: Size,
    icon: Option<Icon>,
    label: &str,
) -> egui::Response {
    let (fill, text, stroke) = button_colors(variant);
    let icon_galley = icon.map(|i| galley(ui, i.text(ICON).color(text)));
    let label_galley =
        (!label.is_empty()).then(|| galley(ui, medium(label, size.text()).color(text)));
    let gap = if icon_galley.is_some() && label_galley.is_some() {
        6.0
    } else {
        0.0
    };
    let content = icon_galley.as_ref().map_or(0.0, |g| g.size().x)
        + gap
        + label_galley.as_ref().map_or(0.0, |g| g.size().x);
    let width = if label_galley.is_none() {
        size.height()
    } else {
        content + 2.0 * size.padding()
    };
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, size.height()), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    if ui.is_rect_visible(rect) {
        let pressed = response.is_pointer_button_down_on();
        let hovered = response.hovered();
        let fill = match variant {
            // The accent lightens and darkens; everything else takes an overlay.
            Variant::Primary if pressed => theme::egui_color(theme::AMBER_6),
            Variant::Primary if hovered => theme::egui_color(theme::AMBER_3),
            _ if pressed => overlay(fill, theme::PRESS_OVERLAY),
            _ if hovered => overlay(fill, theme::HOVER_OVERLAY),
            _ => fill,
        };
        let radius = egui::CornerRadius::same(theme::RADIUS_SM);
        let painter = ui.painter();
        painter.rect(rect, radius, fill, stroke, egui::StrokeKind::Inside);
        let mut x = rect.center().x - content / 2.0;
        for galley in [icon_galley, label_galley].into_iter().flatten() {
            let at = egui::pos2(x, rect.center().y - galley.size().y / 2.0);
            x += galley.size().x + gap;
            painter.galley(at, galley, text);
        }
    }
    response
}

/// A square, icon-only ghost button, as `IconButton.jsx`, with its label as
/// the tooltip.
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, label: &str) -> egui::Response {
    let response = button(ui, Variant::Ghost, Size::Sm, Some(icon), "");
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    response.on_hover_text(label)
}

fn button_colors(variant: Variant) -> (egui::Color32, egui::Color32, egui::Stroke) {
    let hairline = |c| egui::Stroke::new(1.0, c);
    match variant {
        Variant::Primary => (
            theme::egui_color(theme::ACCENT),
            theme::egui_color(theme::BG),
            egui::Stroke::NONE,
        ),
        Variant::Secondary => (
            theme::egui_color(theme::SURFACE_3),
            theme::egui_color(theme::TEXT_1),
            hairline(theme::egui_color(theme::BORDER_STRONG)),
        ),
        Variant::Ghost => (
            egui::Color32::TRANSPARENT,
            theme::egui_color(theme::TEXT_2),
            egui::Stroke::NONE,
        ),
        Variant::Danger => (
            theme::ERR_SUBTLE.color(),
            theme::egui_color(theme::ERR),
            hairline(
                Tint {
                    rgb: theme::ERR,
                    alpha: 0.3,
                }
                .color(),
            ),
        ),
    }
}

/// `tint` over `fill`, which may itself be see-through.
fn overlay(fill: egui::Color32, tint: Tint) -> egui::Color32 {
    if fill.a() == 0 {
        tint.color()
    } else {
        let [r, g, b, _] = fill.to_srgba_unmultiplied();
        let [r, g, b] = tint.over([r, g, b]);
        egui::Color32::from_rgba_unmultiplied(r, g, b, fill.a())
    }
}

/// An outlined identifier in mono, as `Tag.jsx`: a tool number, a file, a
/// preset's role.
pub fn tag(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let galley = galley(
        ui,
        mono(text, SMALL_TEXT).color(theme::egui_color(theme::TEXT_2)),
    );
    let size = egui::vec2(galley.size().x + 12.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_stroke(
        rect,
        theme::RADIUS_SM,
        egui::Stroke::new(1.0, theme::egui_color(theme::BORDER_STRONG)),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        rect.center() - galley.size() / 2.0,
        galley,
        theme::egui_color(theme::TEXT_2),
    );
    response
}

/// A status in a word, as `Badge.jsx`: the level's colour on its 12% tint, small
/// and set in a row beside what it is about.
pub fn badge(ui: &mut egui::Ui, level: Level, text: &str) -> egui::Response {
    let color = level.color();
    let galley = galley(ui, medium(text, theme::TEXT_XS).color(color));
    let size = egui::vec2(galley.size().x + 12.0, 18.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let rgb = match level {
        Level::Ok => theme::OK,
        Level::Warn => theme::WARN,
        Level::Err => theme::ERR,
    };
    let painter = ui.painter();
    painter.rect_filled(rect, theme::RADIUS_SM, Tint { rgb, alpha: 0.12 }.color());
    painter.galley(rect.center() - galley.size() / 2.0, galley, color);
    response
}

/// A status dot, as `readme.md` gives it: a 6 px square in a status colour,
/// hollow when there is no result to show or the one there is out of date.
pub fn status_dot(ui: &mut egui::Ui, color: egui::Color32, filled: bool) -> egui::Response {
    let (slot, response) =
        ui.allocate_exact_size(egui::vec2(DOT_SLOT, DOT_SLOT), egui::Sense::hover());
    let rect = egui::Rect::from_center_size(slot.center(), egui::vec2(DOT, DOT));
    if filled {
        ui.painter().rect_filled(rect, 0.0, color);
    } else {
        ui.painter().rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.25, color),
            egui::StrokeKind::Inside,
        );
    }
    response
}

/// One structured finding, as `Diagnostic.jsx`: a 2 px edge in the level's
/// colour on a card, the level in mono, then the message. `actions` go on the
/// right: the buttons that answer it, when it asks something.
pub fn diagnostic(
    ui: &mut egui::Ui,
    level: Level,
    message: &str,
    actions: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let frame = egui::Frame::new()
        .fill(theme::egui_color(theme::SURFACE_2))
        .stroke(egui::Stroke::new(1.0, theme::egui_color(theme::BORDER)))
        .corner_radius(theme::RADIUS_SM)
        .inner_margin(egui::Margin::symmetric(12, 6));
    let response = frame
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [48.0, 18.0],
                    egui::Label::new(mono(level.word(), theme::TEXT_XS).color(level.color())),
                );
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(message).color(theme::egui_color(theme::TEXT_1)),
                    )
                    .wrap(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), actions);
            });
        })
        .response;
    let r = response.rect;
    ui.painter().rect_filled(
        egui::Rect::from_min_max(r.min, egui::pos2(r.min.x + 2.0, r.max.y)),
        egui::CornerRadius {
            nw: theme::RADIUS_SM,
            sw: theme::RADIUS_SM,
            ne: 0,
            se: 0,
        },
        level.color(),
    );
    response
}

/// A row of a list: the selected one on `--accent-subtle` with an amber edge,
/// the one under the pointer on `--hover-overlay`.
pub fn list_row_background(ui: &egui::Ui, rect: egui::Rect, selected: bool, hovered: bool) {
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, theme::RADIUS_SM, theme::ACCENT_SUBTLE.color());
        painter.rect_filled(
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + 2.0, rect.max.y)),
            0.0,
            theme::egui_color(theme::ACCENT),
        );
    } else if hovered {
        painter.rect_filled(rect, theme::RADIUS_SM, theme::HOVER_OVERLAY.color());
    }
}

/// What is being dragged, named in `font` by the pointer and over everything, on
/// a raised surface as an open menu is.
pub fn drag_label(ctx: &egui::Context, text: &str, font: egui::FontId) {
    let Some(at) = ctx.pointer_interact_pos() else {
        return;
    };
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("drag label"),
    ));
    let galley = painter.layout_no_wrap(text.to_string(), font, theme::egui_color(theme::TEXT_1));
    let rect = egui::Rect::from_min_size(
        at + egui::vec2(12.0, 4.0),
        galley.size() + egui::vec2(12.0, 6.0),
    );
    painter.rect(
        rect,
        theme::RADIUS_SM,
        theme::egui_color(theme::SURFACE_RAISED),
        egui::Stroke::new(1.0, theme::egui_color(theme::BORDER_STRONG)),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        rect.min + egui::vec2(6.0, 3.0),
        galley,
        theme::egui_color(theme::TEXT_1),
    );
}

/// One tab in [`tabs`].
pub struct Tab<'a> {
    pub label: &'a str,
    /// Shown after the label in mono, as `Tabs.jsx` shows a count.
    pub count: Option<usize>,
    /// Something inside needs attention: an error dot after the label, so a
    /// problem on a tab not showing is still seen.
    pub flagged: bool,
}

impl<'a> Tab<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            count: None,
            flagged: false,
        }
    }

    pub fn count(mut self, n: usize) -> Self {
        self.count = Some(n);
        self
    }

    pub fn flagged(mut self, flagged: bool) -> Self {
        self.flagged = flagged;
        self
    }
}

/// Underline tabs, as `Tabs.jsx`: a hairline under the row, and a 2 px amber
/// bar under the tab showing. Returns whether the selection changed.
pub fn tabs(ui: &mut egui::Ui, selected: &mut usize, tabs: &[Tab<'_>]) -> bool {
    let mut changed = false;
    let top = ui.cursor().min.y;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (i, tab) in tabs.iter().enumerate() {
            let on = *selected == i;
            let label = galley(ui, medium(tab.label, CONTROL_TEXT));
            let count = tab
                .count
                .map(|n| galley(ui, mono(n.to_string(), theme::TEXT_XS)));
            let extra = count.as_ref().map_or(0.0, |g| 6.0 + g.size().x)
                + if tab.flagged { 6.0 + DOT } else { 0.0 };
            let width = label.size().x + extra + 2.0 * theme::SPACE_4;
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(width, TAB_HEIGHT), egui::Sense::click());
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, tab.label)
            });
            if response.clicked() && !on {
                *selected = i;
                changed = true;
            }
            let text = theme::egui_color(if on {
                theme::TEXT_1
            } else if response.hovered() {
                theme::TEXT_2
            } else {
                theme::TEXT_3
            });
            let painter = ui.painter();
            let mut x = rect.left() + theme::SPACE_4;
            let mid = rect.center().y;
            let label_width = label.size().x;
            painter.galley(egui::pos2(x, mid - label.size().y / 2.0), label, text);
            x += label_width;
            if let Some(g) = count {
                x += 6.0;
                let w = g.size().x;
                painter.galley(
                    egui::pos2(x, mid - g.size().y / 2.0),
                    g,
                    theme::egui_color(theme::TEXT_4),
                );
                x += w;
            }
            if tab.flagged {
                x += 6.0;
                painter.rect_filled(
                    egui::Rect::from_min_size(egui::pos2(x, mid - DOT / 2.0), egui::vec2(DOT, DOT)),
                    0.0,
                    Level::Err.color(),
                );
            }
            if on {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(rect.left(), rect.bottom() - 2.0),
                        rect.right_bottom(),
                    ),
                    0.0,
                    theme::egui_color(theme::ACCENT),
                );
            }
        }
    });
    // The hairline under the whole row, under the amber bar.
    let rect = ui.max_rect();
    let y = top + TAB_HEIGHT - 0.5;
    ui.painter().with_clip_rect(ui.clip_rect()).line_segment(
        [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
        egui::Stroke::new(1.0, theme::egui_color(theme::BORDER)),
    );
    changed
}

/// [`tabs`] whose choice is remembered under `id` from frame to frame, starting
/// on tab `first`, with space after them: the one showing. `wanted` is shown the
/// frame it is first given, as when picking in the view starts and the tab that
/// lists what is picked must be in sight; after that the person may still choose
/// another, and the caller stops what wanted it.
pub fn remembered_tabs(
    ui: &mut egui::Ui,
    id: egui::Id,
    list: &[Tab<'_>],
    first: usize,
    wanted: Option<usize>,
) -> usize {
    let asked = id.with("wanted");
    let before = ui.data(|d| d.get_temp::<Option<usize>>(asked)).flatten();
    let mut page = wanted
        .filter(|w| before != Some(*w))
        .or_else(|| ui.data(|d| d.get_temp::<usize>(id)))
        .unwrap_or(first)
        .min(list.len().saturating_sub(1));
    tabs(ui, &mut page, list);
    ui.data_mut(|d| {
        d.insert_temp(id, page);
        d.insert_temp(asked, wanted);
    });
    ui.add_space(theme::SPACE_4);
    page
}

/// A card, as `Card.jsx`: `--surface-2` on the panel, a hairline border, and an
/// optional header row with an uppercase title and actions on the right.
pub fn card<R>(
    ui: &mut egui::Ui,
    title: Option<&str>,
    actions: impl FnOnce(&mut egui::Ui),
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    card_titled(ui, title.map(|t| (t, None)), actions, body)
}

/// A [`card`] for something the person edits, titled by `topic`, whose help
/// its title shows on hover.
pub fn topic_card<R>(
    ui: &mut egui::Ui,
    topic: &Topic,
    actions: impl FnOnce(&mut egui::Ui),
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    card_titled(ui, Some((topic.title, Some(topic))), actions, body)
}

fn card_titled<R>(
    ui: &mut egui::Ui,
    title: Option<(&str, Option<&Topic>)>,
    actions: impl FnOnce(&mut egui::Ui),
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Frame::new()
        .fill(theme::egui_color(theme::SURFACE_2))
        .stroke(egui::Stroke::new(1.0, theme::egui_color(theme::BORDER)))
        .corner_radius(theme::RADIUS_SM)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if let Some((title, topic)) = title {
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), CARD_HEADER),
                    egui::Sense::hover(),
                );
                let mut header = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect.shrink2(egui::vec2(12.0, 0.0)))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                let label = header.label(section_label(title));
                if let Some(topic) = topic {
                    label.help(topic);
                }
                header.with_layout(egui::Layout::right_to_left(egui::Align::Center), actions);
                ui.painter().hline(
                    rect.x_range(),
                    rect.bottom() - 0.5,
                    egui::Stroke::new(1.0, theme::egui_color(theme::BORDER)),
                );
            }
            egui::Frame::new()
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    body(ui)
                })
                .inner
        })
        .inner
}

/// The CAD grid, the only texture the system allows: 24 px lines at 4% white,
/// every fourth at 8%, behind viewport-like areas.
pub fn grid_background(painter: &egui::Painter, rect: egui::Rect) {
    let minor = egui::Stroke::new(1.0, theme::GRID_LINE.color());
    let major = egui::Stroke::new(1.0, theme::GRID_LINE_MAJOR.color());
    let step = theme::GRID_SIZE;
    let mut i = 0_u32;
    let mut x = rect.left();
    while x <= rect.right() {
        painter.vline(
            x,
            rect.y_range(),
            if i.is_multiple_of(4) { major } else { minor },
        );
        x += step;
        i += 1;
    }
    i = 0;
    let mut y = rect.top();
    while y <= rect.bottom() {
        painter.hline(
            rect.x_range(),
            y,
            if i.is_multiple_of(4) { major } else { minor },
        );
        y += step;
        i += 1;
    }
}

/// What an empty list shows instead of nothing: a 20 px icon, what is missing,
/// and the buttons that fill it.
pub fn empty_state(
    ui: &mut egui::Ui,
    icon: Icon,
    title: &str,
    detail: &str,
    actions: impl FnOnce(&mut egui::Ui),
) {
    ui.vertical_centered(|ui| {
        ui.add_space(theme::SPACE_6 * 2.0);
        ui.label(icon.text(20.0).color(theme::egui_color(theme::TEXT_4)));
        ui.add_space(theme::SPACE_3);
        ui.label(medium(title, theme::TEXT_BASE).color(theme::egui_color(theme::TEXT_1)));
        ui.label(egui::RichText::new(detail).color(theme::egui_color(theme::TEXT_3)));
        ui.add_space(theme::SPACE_4);
        ui.horizontal(|ui| {
            // Centre the buttons: egui cannot centre a horizontal row itself.
            let id = ui.id().with("empty-actions-width");
            let width: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(0.0);
            ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
            let start = ui.cursor().min.x;
            actions(ui);
            let used = ui.min_rect().right() - start;
            ui.ctx().data_mut(|d| d.insert_temp(id, used));
        });
    });
}

/// `text` laid out on one line, to paint.
fn galley(ui: &egui::Ui, text: egui::RichText) -> std::sync::Arc<egui::Galley> {
    egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    )
}
