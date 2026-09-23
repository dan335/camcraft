//! Colours from the CamCraft Design System.
//!
//! Source: claude.ai/design project "CamCraft Design System", `tokens/colors.css`,
//! vendored for the web inspector in `viewer/tokens.css`. Re-copy when the design
//! system changes; do not invent values here.
//!
//! Colours and the type scale so far. The rest of the system (surfaces, the amber
//! accent as the selection colour, IBM Plex Sans and Fira Code) joins them here.

use camcraft_core::toolpath::MoveRole;

/// `--gray-0`, the page background: near-black graphite, like a machine enclosure.
pub const BG: [u8; 3] = [0x0B, 0x0C, 0x0E];
/// `--gray-6`: the part surface.
pub const PART: [u8; 3] = [0x58, 0x5D, 0x6A];
/// `--gray-7`: neutral. Retracts, because a safe move should not compete for
/// attention, and the stock outline.
pub const GRAY_7: [u8; 3] = [0x8B, 0x90, 0x9D];
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
/// `--amber-7`: material only just left above the part.
pub const AMBER_7: [u8; 3] = [0x8A, 0x58, 0x06];
/// `--err`: material cut below the part.
pub const ERR: [u8; 3] = [0xFF, 0x5C, 0x5C];
/// `--gray-5`: the block where the part is not.
pub const GRAY_5: [u8; 3] = [0x3A, 0x3E, 0x48];
/// `--gray-8`: material finished to the part.
pub const GRAY_8: [u8; 3] = [0xB9, 0xBD, 0xC7];
/// The work zero's X axis. X red, Y green and Z blue, as CAD draws them, taken from
/// the path palette so they sit in the same family.
pub const AXIS_X: [u8; 3] = PATH_RAPID;
/// The work zero's Y axis.
pub const AXIS_Y: [u8; 3] = PATH_CUT;
/// The work zero's Z axis.
pub const AXIS_Z: [u8; 3] = PATH_LEAD;

/// `--text-xs`, from `tokens/typography.css`: captions on drawings and the view.
pub const TEXT_XS: f32 = 11.0;
/// `--text-sm`: small print.
pub const TEXT_SM: f32 = 12.0;
/// `--text-base`: body text, buttons, fields.
pub const TEXT_BASE: f32 = 14.0;
/// `--text-md`: section headings.
pub const TEXT_MD: f32 = 16.0;

/// A named text style for captions: monospace at `--text-xs`, as the web
/// inspector set its figures and hints.
pub const CAPTION: &str = "caption";

/// Set the design system's type scale on every theme egui has, so it holds in dark
/// and light alike. Called once, when the app starts.
pub fn apply(ctx: &eframe::egui::Context) {
    use eframe::egui::{FontFamily, FontId, TextStyle};
    ctx.all_styles_mut(|style| {
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
                FontId::new(TEXT_MD, FontFamily::Proportional),
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

    /// The palette is the design system's, read from the vendored tokens rather
    /// than trusted to have been copied correctly.
    #[test]
    fn colours_match_the_vendored_tokens() {
        let css = include_str!("../../../viewer/tokens.css");
        let hex = |c: [u8; 3]| format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
        for (token, value) in [
            ("--gray-0:", BG),
            ("--gray-6:", PART),
            ("--gray-7:", GRAY_7),
            ("--gray-10:", GRAY_10),
            ("--path-cut:", PATH_CUT),
            ("--path-rapid:", PATH_RAPID),
            ("--path-plunge:", PATH_PLUNGE),
            ("--path-lead:", PATH_LEAD),
            ("--amber-5:", AMBER_5),
            ("--amber-7:", AMBER_7),
            ("--amber-3:", AMBER_3),
            ("--err:", ERR),
            ("--gray-5:", GRAY_5),
            ("--gray-8:", GRAY_8),
        ] {
            let want = format!("{token}{}", hex(value));
            assert!(css.contains(&want), "tokens.css has no {want}");
        }
    }

    /// The type scale is the design system's too.
    #[test]
    fn type_sizes_match_the_vendored_tokens() {
        let css = include_str!("../../../viewer/tokens.css");
        for (token, size) in [
            ("--text-xs:", TEXT_XS),
            ("--text-sm:", TEXT_SM),
            ("--text-base:", TEXT_BASE),
            ("--text-md:", TEXT_MD),
        ] {
            let want = format!("{token}{size}px");
            assert!(css.contains(&want), "tokens.css has no {want}");
        }
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
