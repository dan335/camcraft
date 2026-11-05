//! The desktop's own theme, where there is one to follow: Omarchy's.
//!
//! Omarchy keeps the theme it is in as one palette, `colors.toml`, in
//! `~/.local/state/omarchy/current/theme/` (Omarchy 4), or
//! `~/.config/omarchy/current/theme/` (Omarchy 3), and replaces it whenever
//! the theme is switched. Its guide for apps it does not ship is to read that
//! file and keep watching it, which is what this does: so the window is drawn
//! in the theme as soon as it is switched, light or dark. The keys and how
//! one left out is worked out from the rest are Omarchy's own
//! (`omarchy-theme-color`). Anywhere else, the design system's palette stands
//! ([`Palette::CAMCRAFT`]); see decision 0018 for what is planned there.

use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui;

use crate::ui::theme::{self, Palette, mix};

/// How often the theme file is looked at for a switch: soon enough to seem
/// to follow it, and a few hundred bytes read.
const LOOK_EVERY: Duration = Duration::from_millis(750);

/// Where Omarchy keeps the theme it is in, newest version first, under the
/// home folder `home`.
fn theme_files(home: &Path) -> [PathBuf; 2] {
    [".local/state/omarchy", ".config/omarchy"].map(|dir| {
        home.join(dir)
            .join("current")
            .join("theme")
            .join("colors.toml")
    })
}

/// Draw the window in Omarchy's theme, where there is one, from now on: as
/// it is now, and, from a thread of its own that looks at its file every
/// little while, again whenever it is switched. Nothing is woken for it until
/// the theme changes. Elsewhere the design system's palette stands.
pub fn follow(ctx: &egui::Context) {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return;
    };
    let files = theme_files(&home);
    if !files.iter().any(|f| f.is_file()) {
        return;
    }
    let mut read = look(&files, None, ctx);
    let ctx = ctx.clone();
    // A thread that lives as long as the app: it only ever sleeps and reads.
    // Without one, the theme the app started in stands.
    let _ = std::thread::Builder::new()
        .name("camcraft theme".into())
        .spawn(move || {
            loop {
                std::thread::sleep(LOOK_EVERY);
                read = look(&files, read.take(), &ctx);
            }
        });
}

/// Look at the theme in the first of `files` there is, and draw the window
/// in it if it is not what was `read` before. What it holds now: what was
/// read before while it cannot be read, as mid-switch, when it is briefly
/// not there.
fn look(files: &[PathBuf], read: Option<String>, ctx: &egui::Context) -> Option<String> {
    let Some((file, text)) = files
        .iter()
        .find_map(|f| Some((f, std::fs::read_to_string(f).ok()?)))
    else {
        return read;
    };
    if read.as_deref() != Some(text.as_str()) {
        let light_marker = file.with_file_name("light.mode").is_file();
        if let Some(palette) = palette_of(&text, light_marker) {
            theme::set_palette(ctx, palette);
        }
    }
    Some(text)
}

/// `#rrggbb`, as an Omarchy palette writes a colour.
fn hex(text: &str) -> Option<[u8; 3]> {
    let digits = text.trim().strip_prefix('#')?;
    if digits.len() != 6 {
        return None;
    }
    let byte = |k: usize| u8::from_str_radix(digits.get(k..k + 2)?, 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// The window's palette from an Omarchy palette, `text`, with
/// `light_marker` whether its theme has the old `light.mode` file. Keys a
/// theme leaves out are worked out as Omarchy works them out; `None` if it
/// gives neither a background nor a foreground.
pub fn palette_of(text: &str, light_marker: bool) -> Option<Palette> {
    let table: toml::Table = toml::from_str(text).ok()?;
    let given = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| table.get(*k).and_then(toml::Value::as_str).and_then(hex))
    };
    let background = given(&["background", "bg", "color0"])?;
    let foreground = given(&["foreground", "fg", "color7"])?;
    let dark_background =
        given(&["dark_background", "dark_bg"]).unwrap_or_else(|| mix(background, [0; 3], 0.25));
    let darker_background =
        given(&["darker_background", "darker_bg"]).unwrap_or_else(|| mix(background, [0; 3], 0.5));
    let bright_foreground =
        given(&["bright_foreground", "bright_fg", "color15"]).unwrap_or(foreground);
    let dark_foreground = given(&["dark_foreground", "dark_fg", "color8"]).unwrap_or(foreground);
    // A control a step off the card it sits on, where the theme's lighter
    // background is none.
    let lighter_background = given(&["lighter_background", "lighter_bg"])
        .filter(|l| *l != background)
        .unwrap_or_else(|| mix(background, foreground, 0.06));
    let accent = given(&["accent", "blue", "color4"]).unwrap_or(foreground);
    let mode = table
        .get("mode")
        .or_else(|| table.get("theme_type"))
        .and_then(toml::Value::as_str);
    let dark = match mode {
        Some(m) => m != "light",
        None if light_marker => false,
        None => background.iter().map(|c| u32::from(*c)).sum::<u32>() <= 382,
    };
    // What says how something stands keeps the system's colours, which say it
    // whatever a theme calls red: deepened on a light ground to be read.
    let status = |c: [u8; 3]| if dark { c } else { mix(c, [0; 3], 0.3) };
    Some(Palette {
        dark,
        bg: darker_background,
        surface_1: dark_background,
        surface_2: background,
        surface_3: lighter_background,
        surface_raised: mix(lighter_background, foreground, 0.08),
        border: mix(background, foreground, 0.14),
        border_strong: mix(background, foreground, 0.24),
        text_1: bright_foreground,
        text_2: foreground,
        text_3: mix(foreground, dark_foreground, 0.5),
        text_4: dark_foreground,
        accent,
        accent_hover: mix(accent, bright_foreground, 0.35),
        accent_press: mix(accent, darker_background, 0.2),
        ok: status(theme::OK),
        warn: status(theme::WARN),
        err: status(theme::ERR),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const TOKYO_NIGHT: &str = r##"
mode = "dark"
accent = "#7aa2f7"
background = "#1a1b26"
dark_background = "#13141c"
darker_background = "#0e0e14"
lighter_background = "#24283b"
foreground = "#a9b1d6"
dark_foreground = "#565f89"
bright_foreground = "#c0caf5"
red = "#f7768e"
"##;

    /// A theme's palette lays out as the system's does: the darkest
    /// background behind everything, a step lighter for each surface on it,
    /// the brightest text read first and its accent for what is selected.
    #[test]
    fn a_theme_is_drawn_in_its_own_colours() {
        let p = palette_of(TOKYO_NIGHT, false).unwrap();
        assert!(p.dark);
        assert_eq!(p.bg, [0x0e, 0x0e, 0x14]);
        assert_eq!(p.surface_1, [0x13, 0x14, 0x1c]);
        assert_eq!(p.surface_2, [0x1a, 0x1b, 0x26]);
        assert_eq!(p.surface_3, [0x24, 0x28, 0x3b]);
        assert_eq!(p.text_1, [0xc0, 0xca, 0xf5]);
        assert_eq!(p.text_4, [0x56, 0x5f, 0x89]);
        assert_eq!(p.accent, [0x7a, 0xa2, 0xf7]);
        assert_eq!(p.err, theme::ERR, "a gouge is the system's red");
    }

    /// An older theme, with only the terminal's colours, is worked out as
    /// Omarchy works it out: its shades mixed toward black, light or dark by
    /// its marker file or how light its background is.
    #[test]
    fn an_older_theme_is_worked_out_as_omarchy_does() {
        let old = "color0 = \"#ffffff\"\ncolor7 = \"#000000\"\ncolor4 = \"#0000ff\"\n";
        let p = palette_of(old, false).unwrap();
        assert!(!p.dark, "a light background");
        assert_eq!(p.surface_1, mix([255; 3], [0; 3], 0.25));
        assert_eq!(p.accent, [0, 0, 255], "its blue, with no accent");
        assert_ne!(p.err, theme::ERR, "deepened on a light ground");
        let marked = "background = \"#202020\"\nforeground = \"#e0e0e0\"\n";
        assert!(palette_of(marked, false).unwrap().dark);
        assert!(!palette_of(marked, true).unwrap().dark, "light.mode");
        assert!(palette_of("accent = \"#ff0000\"", false).is_none());
        assert!(palette_of("not toml at all [", false).is_none());
    }

    #[test]
    fn colours_are_read_as_omarchy_writes_them() {
        assert_eq!(hex("#7aa2f7"), Some([0x7a, 0xa2, 0xf7]));
        assert_eq!(hex("#7AA2F7"), Some([0x7a, 0xa2, 0xf7]));
        assert_eq!(hex("7aa2f7"), None);
        assert_eq!(hex("#7aa"), None);
        assert_eq!(hex("rgb(1,2,3)"), None);
    }

    /// Omarchy 4's place first, then Omarchy 3's.
    #[test]
    fn the_theme_is_looked_for_where_each_omarchy_keeps_it() {
        let [four, three] = theme_files(Path::new("/home/dan"));
        assert!(four.ends_with(".local/state/omarchy/current/theme/colors.toml"));
        assert!(three.ends_with(".config/omarchy/current/theme/colors.toml"));
    }
}
