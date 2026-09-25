//! The tool table the manager and the picker share: its rows, the cells of a
//! tool's number, name and figures, and its column headers.

use eframe::egui;

use crate::ui::theme;
use crate::ui::widgets::{self, Icon};

/// A table of tools, as the library lists them: rows that answer `sense`, the one
/// selected on `--accent-subtle` as the folder selected is, rather than the heavier
/// `--selection`. The columns and rows are the caller's.
pub fn tool_table<'a>(
    ui: &'a mut egui::Ui,
    id: &str,
    sense: egui::Sense,
) -> egui_extras::TableBuilder<'a> {
    ui.visuals_mut().selection.bg_fill = theme::ACCENT_SUBTLE.color();
    egui_extras::TableBuilder::new(ui)
        .id_salt(id)
        .striped(false)
        .sense(sense)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
}

/// A tool's number, in a table's first column: an amber edge on the row when it is
/// `selected`, as a list row has.
pub fn number_cell(ui: &mut egui::Ui, tool_number: u32, selected: bool) {
    if selected {
        let r = ui.max_rect();
        ui.painter().rect_filled(
            egui::Rect::from_min_max(r.left_top(), egui::pos2(r.left() + 2.0, r.bottom())),
            0.0,
            theme::egui_color(theme::ACCENT),
        );
    }
    ui.add_space(theme::SPACE_3);
    ui.add(number(format!("T{tool_number}")));
}

/// A tool's name in a table: brightest when `selected`.
pub fn name_cell(ui: &mut egui::Ui, name: &str, selected: bool) {
    let color = if selected {
        theme::TEXT_1
    } else {
        theme::TEXT_2
    };
    ui.add(
        egui::Label::new(egui::RichText::new(name).color(theme::egui_color(color)))
            .truncate()
            .selectable(false),
    );
}

/// A figure in a table: mono, the value to compare.
pub fn number(text: String) -> egui::Label {
    egui::Label::new(widgets::mono(text, theme::TEXT_SM).color(theme::egui_color(theme::TEXT_1)))
}

/// Secondary text in a table, cut short to its column.
pub fn dim(text: String) -> egui::Label {
    egui::Label::new(egui::RichText::new(text).color(theme::egui_color(theme::TEXT_3))).truncate()
}

/// A column's header: the small uppercase label and its unit, lowercase as
/// units are written. `sorted` is `None` for a column the table does not sort by,
/// and otherwise whether it sorts by it now, which puts an arrow on it. Whether it
/// was clicked.
pub fn column_header(
    ui: &mut egui::Ui,
    label: &str,
    unit: Option<&str>,
    sorted: Option<bool>,
    right: bool,
) -> bool {
    let on = sorted == Some(true);
    let mut text = widgets::section_label(label);
    if on {
        text = text.color(theme::egui_color(theme::TEXT_1));
    }
    let mut parts = vec![text];
    if let Some(u) = unit {
        parts.push(widgets::mono(u, theme::TEXT_XS).color(theme::egui_color(theme::TEXT_4)));
    }
    if on {
        parts.push(
            Icon::ArrowDown
                .text(12.0)
                .color(theme::egui_color(theme::ACCENT)),
        );
    }
    let layout = if right {
        // Laid out from the right, so added last part first.
        parts.reverse();
        egui::Layout::right_to_left(egui::Align::Center)
    } else {
        egui::Layout::left_to_right(egui::Align::Center)
    };
    let sense = if sorted.is_some() {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let clicked = ui
        .with_layout(layout, |ui| {
            ui.spacing_mut().item_spacing.x = theme::SPACE_2;
            let mut clicked = false;
            for part in parts {
                clicked |= ui
                    .add(egui::Label::new(part).selectable(false).sense(sense))
                    .clicked();
            }
            clicked
        })
        .inner;
    // The hairline under the header row.
    let rect = ui.max_rect();
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(1.0, theme::egui_color(theme::BORDER)),
    );
    clicked
}
