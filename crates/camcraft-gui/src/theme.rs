//! Colours from the CamCraft Design System.
//!
//! Source: claude.ai/design project "CamCraft Design System", `tokens/colors.css`,
//! vendored for the web inspector in `viewer/tokens.css`. Re-copy when the design
//! system changes; do not invent values here.
//!
//! Only what the viewport needs for now. The rest of the system (surfaces, text,
//! the amber accent, IBM Plex Sans and Fira Code) becomes an egui theme here later.

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
/// `--amber-7`: material only just left above the part.
pub const AMBER_7: [u8; 3] = [0x8A, 0x58, 0x06];
/// `--err`: material cut below the part.
pub const ERR: [u8; 3] = [0xFF, 0x5C, 0x5C];
/// `--gray-5`: the block where the part is not.
pub const GRAY_5: [u8; 3] = [0x3A, 0x3E, 0x48];
/// `--gray-8`: material finished to the part.
pub const GRAY_8: [u8; 3] = [0xB9, 0xBD, 0xC7];

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
            ("--err:", ERR),
            ("--gray-5:", GRAY_5),
            ("--gray-8:", GRAY_8),
        ] {
            let want = format!("{token}{}", hex(value));
            assert!(css.contains(&want), "tokens.css has no {want}");
        }
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
