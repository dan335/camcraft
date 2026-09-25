//! The CamCraft Design System, for egui.
//!
//! Source: claude.ai/design project "CamCraft Design System", copied into
//! `design/system/` beside this crate. The values here mirror its tokens, and the
//! tests read the copied CSS to hold them to it. Re-copy when the design system
//! changes; do not invent values here.
//!
//! [`apply`] turns them into egui's style, fonts included, so every window gets
//! the system's graphite surfaces, hairline borders, sharp corners and amber
//! selection without asking. The components built on top are in `widgets.rs`.

use camcraft_core::toolpath::MoveRole;
use eframe::egui;

/// `--gray-0`, the page background: near-black graphite, like a machine enclosure.
pub const BG: [u8; 3] = [0x0B, 0x0C, 0x0E];
/// `--gray-1`: `--surface-1`, panels.
pub const GRAY_1: [u8; 3] = [0x11, 0x12, 0x16];
/// `--gray-2`: `--surface-2`, cards and windows.
pub const GRAY_2: [u8; 3] = [0x17, 0x18, 0x1D];
/// `--gray-3`: `--surface-3`, controls at rest.
pub const GRAY_3: [u8; 3] = [0x1F, 0x21, 0x27];
/// `--gray-4`: `--surface-raised` and `--border`.
pub const GRAY_4: [u8; 3] = [0x2A, 0x2D, 0x35];
/// `--gray-6`: the part surface, and `--text-4`.
pub const PART: [u8; 3] = [0x58, 0x5D, 0x6A];
/// `--gray-7`: neutral. Retracts, because a safe move should not compete for
/// attention, and the stock outline.
pub const GRAY_7: [u8; 3] = [0x8B, 0x90, 0x9D];
/// `--gray-9`: `--text-1`.
pub const GRAY_9: [u8; 3] = [0xE6, 0xE8, 0xEE];
/// `--gray-10`: near-white. An unresolved link, which is exactly "unclassified".
pub const GRAY_10: [u8; 3] = [0xF7, 0xF8, 0xFA];
/// `--path-cut`.
pub const PATH_CUT: [u8; 3] = [0x5B, 0xE3, 0x7A];
/// `--path-rapid`.
pub const PATH_RAPID: [u8; 3] = [0xFF, 0x5C, 0x5C];
/// `--path-plunge`.
pub const PATH_PLUNGE: [u8; 3] = [0xF5, 0xA5, 0x24];
/// `--path-lead`, also used for ramps.
pub const PATH_LEAD: [u8; 3] = [0x4F, 0xD1, 0xFF];
/// `--amber-5`, the accent: the spindle-warning colour. The cutter is drawn in it,
/// and material left above the part at its thickest.
pub const AMBER_5: [u8; 3] = [0xF5, 0xA5, 0x24];
/// `--amber-3`: highlight on amber.
pub const AMBER_3: [u8; 3] = [0xFF, 0xE1, 0xA3];
/// `--amber-6`, the accent pressed.
pub const AMBER_6: [u8; 3] = [0xD9, 0x8A, 0x0F];
/// `--amber-7`: material only just left above the part.
pub const AMBER_7: [u8; 3] = [0x8A, 0x58, 0x06];
/// `--err`: material cut below the part.
pub const ERR: [u8; 3] = [0xFF, 0x5C, 0x5C];
/// `--ok`.
pub const OK: [u8; 3] = [0x5B, 0xE3, 0x7A];
/// `--warn`.
pub const WARN: [u8; 3] = [0xF5, 0xA5, 0x24];
/// `--gray-5`: the block where the part is not, and `--border-strong`.
pub const GRAY_5: [u8; 3] = [0x3A, 0x3E, 0x48];
/// `--gray-8`: material finished to the part, and `--text-2`.
pub const GRAY_8: [u8; 3] = [0xB9, 0xBD, 0xC7];
/// Material as playback shows it, before it is compared with the part:
/// `--gray-7`, a mid grey that shading turns into machined metal.
pub const MATERIAL: [u8; 3] = GRAY_7;
/// What is picked in the view: the accent, as the design system gives selection.
pub const SELECTED: [u8; 3] = AMBER_5;
/// What is under the pointer, ready to pick: `--accent-hover`.
pub const HOVER: [u8; 3] = AMBER_3;
/// The boundary an operation is kept to: `--accent-press`, a step darker than
/// what is picked, so the outline reads as derived from the picks.
pub const BOUNDARY: [u8; 3] = AMBER_6;
/// What could be picked into a setup but is not in it: its models and stock,
/// drawn faintly while they are being picked.
pub const GHOST: [u8; 3] = GRAY_8;
/// The work zero's X axis. X red, Y green and Z blue, as CAD draws them, taken from
/// the path palette so they sit in the same family.
pub const AXIS_X: [u8; 3] = PATH_RAPID;
/// The work zero's Y axis.
pub const AXIS_Y: [u8; 3] = PATH_CUT;
/// The work zero's Z axis.
pub const AXIS_Z: [u8; 3] = PATH_LEAD;

/// `--surface-1`: panels, and the inside of fields.
pub const SURFACE_1: [u8; 3] = GRAY_1;
/// `--surface-2`: cards and windows, a step up from the panel they sit on.
pub const SURFACE_2: [u8; 3] = GRAY_2;
/// `--surface-3`: controls at rest.
pub const SURFACE_3: [u8; 3] = GRAY_3;
/// `--surface-raised`: open menus.
pub const SURFACE_RAISED: [u8; 3] = GRAY_4;
/// `--border`: the hairlines that do the structural work.
pub const BORDER: [u8; 3] = GRAY_4;
/// `--border-strong`: around controls.
pub const BORDER_STRONG: [u8; 3] = GRAY_5;
/// `--text-1`: the value the reader should compare.
pub const TEXT_1: [u8; 3] = GRAY_9;
/// `--text-2`: body text.
pub const TEXT_2: [u8; 3] = GRAY_8;
/// `--text-3`: labels, units, everything secondary.
pub const TEXT_3: [u8; 3] = GRAY_7;
/// `--text-4`: the quietest text: counts, paths, "not given".
pub const TEXT_4: [u8; 3] = PART;
/// `--accent`.
pub const ACCENT: [u8; 3] = AMBER_5;

/// A colour at some opacity, as the tokens give the subtle tints and overlays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tint {
    pub rgb: [u8; 3],
    pub alpha: f32,
}

/// `--accent-subtle`: amber at 12%, behind the folder that is selected.
pub const ACCENT_SUBTLE: Tint = Tint {
    rgb: AMBER_5,
    alpha: 0.12,
};
/// `--selection`: amber at 25%, behind what is selected.
pub const SELECTION: Tint = Tint {
    rgb: AMBER_5,
    alpha: 0.25,
};
/// `--hover-overlay`: white at 5%, over a neutral control under the pointer.
pub const HOVER_OVERLAY: Tint = Tint {
    rgb: [255, 255, 255],
    alpha: 0.05,
};
/// `--press-overlay`: white at 9%, over a control being pressed.
pub const PRESS_OVERLAY: Tint = Tint {
    rgb: [255, 255, 255],
    alpha: 0.09,
};
/// `--err-subtle`: behind a destructive button.
pub const ERR_SUBTLE: Tint = Tint {
    rgb: ERR,
    alpha: 0.12,
};
/// `--grid-line`: the 24 px CAD grid behind viewport-like areas.
pub const GRID_LINE: Tint = Tint {
    rgb: [255, 255, 255],
    alpha: 0.04,
};
/// `--grid-line-major`: every fourth line of the grid.
pub const GRID_LINE_MAJOR: Tint = Tint {
    rgb: [255, 255, 255],
    alpha: 0.08,
};

impl Tint {
    /// For egui, which blends it over whatever is under it.
    pub fn color(self) -> egui::Color32 {
        // A fraction of 255, so it fits.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let a = (self.alpha * 255.0).round() as u8;
        let [r, g, b] = self.rgb;
        egui::Color32::from_rgba_unmultiplied(r, g, b, a)
    }

    /// Laid over `base`, as one solid colour: for egui's fills that are not blended.
    pub fn over(self, base: [u8; 3]) -> [u8; 3] {
        let mut out = base;
        for (o, (b, t)) in out.iter_mut().zip(base.iter().zip(self.rgb)) {
            let mixed = f32::from(*b) + (f32::from(t) - f32::from(*b)) * self.alpha;
            // Between two bytes, so it fits.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let byte = mixed.round() as u8;
            *o = byte;
        }
        out
    }
}

/// `--text-xs`, from `tokens/typography.css`: captions, and the uppercase labels
/// over cards and table columns.
pub const TEXT_XS: f32 = 11.0;
/// `--text-sm`: small print.
pub const TEXT_SM: f32 = 12.0;
/// `--text-base`: body text, buttons, fields.
pub const TEXT_BASE: f32 = 14.0;
/// `--text-md`: section headings.
pub const TEXT_MD: f32 = 16.0;
/// `--text-lg`: the wordmark, where a logo would go.
pub const TEXT_LG: f32 = 20.0;
/// `--tracking-wide`, in ems: the spread of the uppercase labels.
pub const TRACKING_WIDE: f32 = 0.06;

/// `--space-1`, from `tokens/spacing.css`.
pub const SPACE_1: f32 = 2.0;
/// `--space-2`.
pub const SPACE_2: f32 = 4.0;
/// `--space-3`: between related controls.
pub const SPACE_3: f32 = 8.0;
/// `--space-4`.
pub const SPACE_4: f32 = 12.0;
/// `--space-5`: inside a card.
pub const SPACE_5: f32 = 16.0;
/// `--space-6`: between groups.
pub const SPACE_6: f32 = 24.0;
/// `--control-sm`: dense controls, as in an inspector's rows.
pub const CONTROL_SM: f32 = 24.0;
/// `--control-md`: toolbar buttons.
pub const CONTROL_MD: f32 = 28.0;
/// `--sidebar`: the left sidebar.
pub const SIDEBAR: f32 = 260.0;
/// `--panel`: the right inspector.
pub const PANEL: f32 = 320.0;
/// `--grid-size`: the CAD grid's spacing.
pub const GRID_SIZE: f32 = 24.0;

/// `--radius-sm`, from `tokens/effects.css`: everything, by default.
pub const RADIUS_SM: u8 = 2;
/// `--radius-lg`: dialogs and windows, the most the system rounds anything.
pub const RADIUS_LG: u8 = 4;

/// A named text style for captions: monospace at `--text-xs`, for figures and
/// notes drawn small, as over the view and on a tool's picture.
pub const CAPTION: &str = "caption";

/// IBM Plex Sans at 500, for labels and tabs: egui has no font weights, so each
/// weight is a family of its own.
pub const MEDIUM: &str = "medium";
/// IBM Plex Sans at 600, for headings.
pub const SEMIBOLD: &str = "semibold";
/// Fira Code at 500.
pub const MONO_MEDIUM: &str = "mono-medium";

/// The system's typefaces, bundled so nothing is fetched and the app looks the
/// same everywhere. Lucide comes last in every family, so an icon's codepoint
/// draws wherever text does.
fn fonts() -> egui::FontDefinitions {
    use egui::{FontData, FontFamily};
    use std::sync::Arc;
    let mut fonts = egui::FontDefinitions::default();
    for (name, bytes) in [
        (
            "plex",
            &include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf")[..],
        ),
        (
            "plex-medium",
            include_bytes!("../assets/fonts/IBMPlexSans-Medium.ttf"),
        ),
        (
            "plex-semibold",
            include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf"),
        ),
        (
            "fira",
            include_bytes!("../assets/fonts/FiraCode-Regular.ttf"),
        ),
        (
            "fira-medium",
            include_bytes!("../assets/fonts/FiraCode-Medium.ttf"),
        ),
        (
            "lucide",
            include_bytes!("../assets/fonts/lucide-subset.ttf"),
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    // egui's own fonts stay behind ours, for the symbols Plex and Fira lack.
    let fallback = |first: &str, family: &FontFamily, fonts: &egui::FontDefinitions| {
        let mut list = vec![first.to_string()];
        list.extend(fonts.families.get(family).cloned().unwrap_or_default());
        list.push("lucide".into());
        list
    };
    let sans = fallback("plex", &FontFamily::Proportional, &fonts);
    let mono = fallback("fira", &FontFamily::Monospace, &fonts);
    let medium = fallback("plex-medium", &FontFamily::Proportional, &fonts);
    let semibold = fallback("plex-semibold", &FontFamily::Proportional, &fonts);
    let mono_medium = fallback("fira-medium", &FontFamily::Monospace, &fonts);
    fonts.families.insert(FontFamily::Proportional, sans);
    fonts.families.insert(FontFamily::Monospace, mono);
    fonts
        .families
        .insert(FontFamily::Name(MEDIUM.into()), medium);
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), semibold);
    fonts
        .families
        .insert(FontFamily::Name(MONO_MEDIUM.into()), mono_medium);
    fonts
}

/// One state of the controls: its fill and text over the system's hairline.
fn widget(fill: [u8; 3], stroke: [u8; 3], text: [u8; 3]) -> egui::style::WidgetVisuals {
    egui::style::WidgetVisuals {
        bg_fill: egui_color(fill),
        weak_bg_fill: egui_color(fill),
        bg_stroke: egui::Stroke::new(1.0, egui_color(stroke)),
        corner_radius: egui::CornerRadius::same(RADIUS_SM),
        fg_stroke: egui::Stroke::new(1.0, egui_color(text)),
        // The system changes a control's shade on hover and press, never its size.
        expansion: 0.0,
    }
}

/// The system's look, as egui's visuals. It has one theme, dark.
fn visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.widgets.noninteractive = widget(SURFACE_1, BORDER, TEXT_2);
    v.widgets.inactive = widget(SURFACE_3, BORDER_STRONG, TEXT_1);
    v.widgets.hovered = widget(HOVER_OVERLAY.over(SURFACE_3), BORDER_STRONG, TEXT_1);
    v.widgets.active = widget(PRESS_OVERLAY.over(SURFACE_3), BORDER_STRONG, TEXT_1);
    v.widgets.open = widget(SURFACE_RAISED, BORDER_STRONG, TEXT_1);
    v.weak_text_color = Some(egui_color(TEXT_3));
    // Amber behind what is selected, and around the field being typed in: the
    // system's `--selection` and `--border-focus`.
    v.selection.bg_fill = SELECTION.color();
    v.selection.stroke = egui::Stroke::new(1.0, egui_color(ACCENT));
    v.hyperlink_color = egui_color(ACCENT);
    v.faint_bg_color = egui_color(SURFACE_2);
    v.extreme_bg_color = egui_color(SURFACE_1);
    v.text_edit_bg_color = Some(egui_color(SURFACE_1));
    v.code_bg_color = egui_color(SURFACE_2);
    v.warn_fg_color = egui_color(WARN);
    v.error_fg_color = egui_color(ERR);
    v.panel_fill = egui_color(SURFACE_1);
    v.window_fill = egui_color(SURFACE_2);
    v.window_stroke = egui::Stroke::new(1.0, egui_color(BORDER_STRONG));
    v.window_corner_radius = egui::CornerRadius::same(RADIUS_LG);
    v.menu_corner_radius = egui::CornerRadius::same(RADIUS_SM);
    // `--shadow-dialog` and `--shadow-popover`; their 1 px ring is the stroke.
    v.window_shadow = egui::Shadow {
        offset: [0, 24],
        blur: 64,
        spread: 0,
        color: egui::Color32::from_black_alpha(179),
    };
    v.popup_shadow = egui::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: egui::Color32::from_black_alpha(153),
    };
    v.window_highlight_topmost = false;
    v.collapsing_header_frame = false;
    v.disabled_alpha = 0.4;
    v
}

/// The system's spacing, as egui's: dense, a desktop tool rather than a website.
fn spacing(s: &mut egui::style::Spacing) {
    s.item_spacing = egui::vec2(SPACE_3, 6.0);
    s.button_padding = egui::vec2(SPACE_3, SPACE_1);
    s.interact_size = egui::vec2(40.0, CONTROL_SM);
    s.indent = SPACE_5;
    s.window_margin = egui::Margin::same(12);
    s.menu_margin = egui::Margin::same(4);
    s.icon_width = 14.0;
    s.icon_width_inner = 8.0;
}

/// Set the design system on every theme egui has, fonts and all, so it holds
/// whatever the OS prefers: the system has one theme. Called once, when the app
/// starts.
pub fn apply(ctx: &egui::Context) {
    use egui::{FontFamily, FontId, TextStyle};
    ctx.set_fonts(fonts());
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|style| {
        style.visuals = visuals();
        spacing(&mut style.spacing);
        style.text_styles = [
            (
                TextStyle::Small,
                FontId::new(TEXT_SM, FontFamily::Proportional),
            ),
            (
                TextStyle::Body,
                FontId::new(TEXT_BASE, FontFamily::Proportional),
            ),
            (
                TextStyle::Button,
                FontId::new(TEXT_BASE, FontFamily::Proportional),
            ),
            (
                TextStyle::Heading,
                FontId::new(TEXT_MD, FontFamily::Name(SEMIBOLD.into())),
            ),
            (
                TextStyle::Monospace,
                FontId::new(TEXT_BASE, FontFamily::Monospace),
            ),
            (
                TextStyle::Name(CAPTION.into()),
                FontId::new(TEXT_XS, FontFamily::Monospace),
            ),
        ]
        .into();
    });
}

/// The font for a named or built-in text style, as the current style sets it.
///
/// A style that has not been set, as before [`apply`] has run, falls back to the
/// scale rather than failing: egui's own lookup panics on a missing style.
pub fn font(style: &eframe::egui::Style, text: eframe::egui::TextStyle) -> eframe::egui::FontId {
    use eframe::egui::{FontFamily, FontId, TextStyle};
    if let Some(f) = style.text_styles.get(&text) {
        return f.clone();
    }
    match text {
        TextStyle::Name(ref n) if n.as_ref() == CAPTION => {
            FontId::new(TEXT_XS, FontFamily::Monospace)
        }
        TextStyle::Monospace => FontId::new(TEXT_BASE, FontFamily::Monospace),
        TextStyle::Small => FontId::new(TEXT_SM, FontFamily::Proportional),
        TextStyle::Heading => FontId::new(TEXT_MD, FontFamily::Proportional),
        _ => FontId::new(TEXT_BASE, FontFamily::Proportional),
    }
}

/// The caption font, as the current style sets it.
pub fn caption(style: &eframe::egui::Style) -> eframe::egui::FontId {
    font(style, eframe::egui::TextStyle::Name(CAPTION.into()))
}

/// Every role, in the order the controls list them.
pub const ROLES: [MoveRole; 7] = [
    MoveRole::Cut,
    MoveRole::Rapid,
    MoveRole::Plunge,
    MoveRole::Retract,
    MoveRole::Lead,
    MoveRole::Ramp,
    MoveRole::Link,
];

/// A role's colour, as the design system gives it.
pub fn role_srgb(role: MoveRole) -> [u8; 3] {
    match role {
        MoveRole::Cut => PATH_CUT,
        MoveRole::Rapid => PATH_RAPID,
        MoveRole::Plunge => PATH_PLUNGE,
        MoveRole::Lead | MoveRole::Ramp => PATH_LEAD,
        MoveRole::Retract => GRAY_7,
        MoveRole::Link => GRAY_10,
    }
}

/// A role's name, as a person would say it.
pub fn role_name(role: MoveRole) -> &'static str {
    match role {
        MoveRole::Cut => "cut",
        MoveRole::Rapid => "rapid",
        MoveRole::Plunge => "plunge",
        MoveRole::Retract => "retract",
        MoveRole::Lead => "lead",
        MoveRole::Ramp => "ramp",
        MoveRole::Link => "link",
    }
}

/// The role's bit in a visibility mask, and its index in [`ROLES`].
pub fn role_index(role: MoveRole) -> u32 {
    match role {
        MoveRole::Cut => 0,
        MoveRole::Rapid => 1,
        MoveRole::Plunge => 2,
        MoveRole::Retract => 3,
        MoveRole::Lead => 4,
        MoveRole::Ramp => 5,
        MoveRole::Link => 6,
    }
}

/// An sRGB colour in linear light, for shading on the GPU.
pub fn linear(c: [u8; 3]) -> [f32; 3] {
    c.map(|v| {
        let s = f32::from(v) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    })
}

/// An sRGB colour for egui widgets.
pub fn egui_color(c: [u8; 3]) -> eframe::egui::Color32 {
    eframe::egui::Color32::from_rgb(c[0], c[1], c[2])
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    const COLORS: &str = include_str!("../design/system/tokens/colors.css");
    const TYPOGRAPHY: &str = include_str!("../design/system/tokens/typography.css");
    const SPACING: &str = include_str!("../design/system/tokens/spacing.css");
    const EFFECTS: &str = include_str!("../design/system/tokens/effects.css");

    /// The palette is the design system's, read from the copied tokens rather
    /// than trusted to have been copied correctly.
    #[test]
    fn colours_match_the_design_system() {
        let hex = |c: [u8; 3]| format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
        for (token, value) in [
            ("--gray-0:", BG),
            ("--gray-1:", GRAY_1),
            ("--gray-2:", GRAY_2),
            ("--gray-3:", GRAY_3),
            ("--gray-4:", GRAY_4),
            ("--gray-5:", GRAY_5),
            ("--gray-6:", PART),
            ("--gray-7:", GRAY_7),
            ("--gray-8:", GRAY_8),
            ("--gray-9:", GRAY_9),
            ("--gray-10:", GRAY_10),
            ("--path-cut:", PATH_CUT),
            ("--path-rapid:", PATH_RAPID),
            ("--path-plunge:", PATH_PLUNGE),
            ("--path-lead:", PATH_LEAD),
            ("--amber-5:", AMBER_5),
            ("--amber-6:", AMBER_6),
            ("--amber-7:", AMBER_7),
            ("--amber-3:", AMBER_3),
            ("--ok:", OK),
            ("--warn:", WARN),
            ("--err:", ERR),
        ] {
            let want = format!("{token}{}", hex(value));
            assert!(COLORS.contains(&want), "colors.css has no {want}");
        }
    }

    /// The surfaces, borders and text are the tokens the system names for them.
    #[test]
    fn semantic_colours_are_the_ones_the_system_names() {
        for (alias, gray, ours, theirs) in [
            ("--surface-1", "--gray-1", SURFACE_1, GRAY_1),
            ("--surface-2", "--gray-2", SURFACE_2, GRAY_2),
            ("--surface-3", "--gray-3", SURFACE_3, GRAY_3),
            ("--surface-raised", "--gray-4", SURFACE_RAISED, GRAY_4),
            ("--border", "--gray-4", BORDER, GRAY_4),
            ("--border-strong", "--gray-5", BORDER_STRONG, GRAY_5),
            ("--text-1", "--gray-9", TEXT_1, GRAY_9),
            ("--text-2", "--gray-8", TEXT_2, GRAY_8),
            ("--text-3", "--gray-7", TEXT_3, GRAY_7),
            ("--text-4", "--gray-6", TEXT_4, PART),
            ("--accent", "--amber-5", ACCENT, AMBER_5),
        ] {
            let want = format!("{alias}:var({gray})");
            assert!(COLORS.contains(&want), "colors.css has no {want}");
            assert_eq!(ours, theirs, "{alias}");
        }
    }

    /// The tints and overlays are the system's colours at the system's opacities.
    #[test]
    fn tints_match_the_design_system() {
        for (token, tint, css) in [
            ("--accent-subtle:", ACCENT_SUBTLE, COLORS),
            ("--selection:", SELECTION, COLORS),
            ("--hover-overlay:", HOVER_OVERLAY, COLORS),
            ("--press-overlay:", PRESS_OVERLAY, COLORS),
            ("--err-subtle:", ERR_SUBTLE, COLORS),
            ("--grid-line:", GRID_LINE, EFFECTS),
            ("--grid-line-major:", GRID_LINE_MAJOR, EFFECTS),
        ] {
            let [r, g, b] = tint.rgb;
            let alpha = format!("{:.2}", tint.alpha);
            let want = format!("{token}rgba({r},{g},{b},{})", &alpha[1..]);
            assert!(css.contains(&want), "the tokens have no {want}");
        }
    }

    /// The type scale is the design system's too.
    #[test]
    fn type_sizes_match_the_design_system() {
        for (token, size) in [
            ("--text-xs:", TEXT_XS),
            ("--text-sm:", TEXT_SM),
            ("--text-base:", TEXT_BASE),
            ("--text-md:", TEXT_MD),
            ("--text-lg:", TEXT_LG),
        ] {
            let want = format!("{token}{size}px");
            assert!(TYPOGRAPHY.contains(&want), "typography.css has no {want}");
        }
        let want = format!("--tracking-wide:{TRACKING_WIDE}em");
        assert!(TYPOGRAPHY.contains(&want), "typography.css has no {want}");
    }

    /// So are the spacing, the control heights, the layout widths and the radii.
    #[test]
    fn spacing_and_radii_match_the_design_system() {
        for (token, px, css) in [
            ("--space-1:", SPACE_1, SPACING),
            ("--space-2:", SPACE_2, SPACING),
            ("--space-3:", SPACE_3, SPACING),
            ("--space-4:", SPACE_4, SPACING),
            ("--space-5:", SPACE_5, SPACING),
            ("--space-6:", SPACE_6, SPACING),
            ("--control-sm:", CONTROL_SM, SPACING),
            ("--control-md:", CONTROL_MD, SPACING),
            ("--sidebar:", SIDEBAR, SPACING),
            ("--panel:", PANEL, SPACING),
            ("--grid-size:", GRID_SIZE, EFFECTS),
            ("--radius-sm:", f32::from(RADIUS_SM), EFFECTS),
            ("--radius-lg:", f32::from(RADIUS_LG), EFFECTS),
        ] {
            let want = format!("{token}{px}px");
            assert!(css.contains(&want), "the tokens have no {want}");
        }
    }

    /// A tint laid over a colour lands between the two, by its opacity.
    #[test]
    fn a_tint_over_a_colour_blends_by_its_opacity() {
        assert_eq!(HOVER_OVERLAY.over([0, 0, 0]), [13, 13, 13]);
        assert_eq!(SELECTION.over(AMBER_5), AMBER_5);
        assert_eq!(SELECTION.color().a(), 64);
    }

    /// Applied, every text style takes its size from the scale, and the caption
    /// style exists for the labels that used to fix their own size.
    #[test]
    fn applying_sets_every_text_style() {
        use eframe::egui::{Context, TextStyle, Theme};
        let ctx = Context::default();
        apply(&ctx);
        for theme in [Theme::Dark, Theme::Light] {
            let style = ctx.style_of(theme);
            assert_eq!(font(&style, TextStyle::Body).size, TEXT_BASE);
            assert_eq!(font(&style, TextStyle::Button).size, TEXT_BASE);
            assert_eq!(font(&style, TextStyle::Heading).size, TEXT_MD);
            assert_eq!(font(&style, TextStyle::Small).size, TEXT_SM);
            assert_eq!(caption(&style).size, TEXT_XS);
        }
    }

    /// Applied, the system's surfaces and amber selection hold in both of egui's
    /// themes: the system has one, so an OS set to light does not undo it.
    #[test]
    fn applying_sets_the_look_in_every_theme() {
        use eframe::egui::{Context, Theme};
        let ctx = Context::default();
        apply(&ctx);
        for theme in [Theme::Dark, Theme::Light] {
            let v = &ctx.style_of(theme).visuals;
            assert_eq!(v.panel_fill, egui_color(SURFACE_1));
            assert_eq!(v.window_fill, egui_color(SURFACE_2));
            assert_eq!(v.selection.bg_fill, SELECTION.color());
            assert_eq!(v.widgets.inactive.corner_radius.nw, RADIUS_SM);
            assert_eq!(v.window_corner_radius.nw, RADIUS_LG);
        }
    }

    /// Before the theme is applied, looking a style up falls back to the scale
    /// instead of panicking, as egui's own lookup would.
    #[test]
    fn a_missing_style_falls_back_to_the_scale() {
        let bare = eframe::egui::Style::default();
        assert_eq!(caption(&bare).size, TEXT_XS);
    }

    #[test]
    fn every_role_has_its_own_bit() {
        for (i, role) in ROLES.iter().enumerate() {
            assert_eq!(role_index(*role) as usize, i);
        }
    }

    #[test]
    fn linear_light_endpoints() {
        assert_eq!(linear([0, 0, 0]), [0.0; 3]);
        assert!((linear([255, 255, 255])[0] - 1.0).abs() < 1e-6);
        // Mid-grey sRGB is about a fifth of the light.
        assert!((linear([128, 128, 128])[0] - 0.2158).abs() < 1e-3);
    }
}
