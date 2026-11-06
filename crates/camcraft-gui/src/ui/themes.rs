//! The Theme setting: which palette the window is drawn in, and keeping it
//! drawn in it as its file changes.
//!
//! *System* follows the desktop's theme where there is one to follow
//! (Omarchy's, [`crate::ui::desktop`]) and is the design system's elsewhere;
//! *camcraft* is the design system's whatever the desktop; then the themes
//! Omarchy ships, bundled ([`BUNDLED`]), as Omarchy keeps its own beside the
//! person's; and the person's: palette files in camcraft's themes folder, in
//! Omarchy's `colors.toml` format, so an Omarchy theme can be dropped in on
//! any system: a `name.toml`, or a theme's folder as it is,
//! `name/colors.toml`. A file chosen is watched as Omarchy's is, so a palette
//! being written is seen as it is saved. See decision 0018.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::ui::desktop;
use crate::ui::theme::{self, Palette};
use crate::ui::widgets;

/// How often the theme's file is looked at for a change: soon enough to seem
/// to follow it, and a few hundred bytes read.
const LOOK_EVERY: Duration = Duration::from_millis(750);

/// Omarchy's themes, by name, each its `colors.toml` as Omarchy ships it,
/// in `assets/themes/` (MIT, `LICENSE-Omarchy.txt` there).
macro_rules! bundled {
    ($($name:literal),* $(,)?) => {
        /// The themes Omarchy ships, by name, with what each `colors.toml`
        /// holds.
        pub const BUNDLED: &[(&str, &str)] = &[$(
            ($name, include_str!(concat!("../../assets/themes/", $name, ".toml"))),
        )*];
    };
}

bundled! {
    "catppuccin",
    "catppuccin-latte",
    "ethereal",
    "everforest",
    "flexoki-light",
    "gruvbox",
    "hackerman",
    "jade-horse",
    "kanagawa",
    "last-horizon",
    "lumon",
    "lupine",
    "matte-black",
    "miasma",
    "nord",
    "retro-82",
    "ristretto",
    "rose-pine",
    "solitude",
    "starship",
    "tokyo-night",
    "vantablack",
    "white",
}

/// Which theme the window is drawn in.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Choice {
    /// The desktop's theme where there is one to follow, Omarchy's; the
    /// design system's elsewhere.
    #[default]
    System,
    /// The design system's own, graphite and amber, whatever the desktop's.
    Camcraft,
    /// The theme Omarchy ships of this name ([`BUNDLED`]).
    Bundled(String),
    /// The palette file of this name in the themes folder ([`folder`]).
    File(String),
}

/// Where a theme's palette comes from.
enum Source {
    /// A palette that does not change.
    Fixed(Palette),
    /// The first of these files there is, watched.
    Files(Vec<PathBuf>),
}

impl Choice {
    /// What it is called in the Theme setting.
    pub fn title(&self) -> String {
        match self {
            Self::System if desktop::omarchy_files().is_some() => "System (Omarchy)".into(),
            Self::System => "System".into(),
            Self::Camcraft => "camcraft".into(),
            Self::Bundled(name) | Self::File(name) => title(name),
        }
    }

    /// Where its palette comes from: the design system's where it has none,
    /// as off Omarchy for *System*, or a bundled theme no longer bundled.
    fn source(&self) -> Source {
        let files = match self {
            Self::System => desktop::omarchy_files(),
            Self::Camcraft => None,
            Self::Bundled(name) => {
                let palette = BUNDLED
                    .iter()
                    .find(|(n, _)| n == name)
                    .and_then(|(_, text)| desktop::palette_of(text, false));
                return Source::Fixed(palette.unwrap_or(Palette::CAMCRAFT));
            }
            Self::File(name) => folder().map(|f| files_in(&f, name)),
        };
        files.map_or(Source::Fixed(Palette::CAMCRAFT), Source::Files)
    }
}

/// The themes there are to choose, `choice` set to the one clicked: the
/// desktop's, the design system's, Omarchy's, and the person's own, each
/// under its heading. Whether it changed.
pub fn pick(ui: &mut egui::Ui, choice: &mut Choice) -> bool {
    let was = choice.clone();
    let mut offer = |ui: &mut egui::Ui, c: Choice| {
        let title = c.title();
        ui.selectable_value(choice, c, title);
    };
    offer(ui, Choice::System);
    offer(ui, Choice::Camcraft);
    ui.separator();
    ui.label(widgets::hint_text("Omarchy's"));
    for (name, _) in BUNDLED {
        offer(ui, Choice::Bundled((*name).to_owned()));
    }
    // Looked at only while the list is open: a few small files.
    let own = folder().map(|f| listed(&f)).unwrap_or_default();
    if !own.is_empty() {
        ui.separator();
        ui.label(widgets::hint_text("Yours"));
        for name in own {
            offer(ui, Choice::File(name));
        }
    }
    *choice != was
}

/// A palette file's name as a title: `tokyo-night` as *Tokyo Night*.
fn title(name: &str) -> String {
    name.split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            chars.next().map_or_else(String::new, |c| {
                c.to_uppercase().chain(chars).collect::<String>()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// camcraft's themes folder, in its config folder.
pub fn folder() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "camcraft").map(|d| d.config_dir().join("themes"))
}

/// Where the palette `name` is in `folder`: a file of its own first, then a
/// theme's folder.
fn files_in(folder: &Path, name: &str) -> Vec<PathBuf> {
    vec![
        folder.join(format!("{name}.toml")),
        folder.join(name).join("colors.toml"),
    ]
}

/// The palettes in `folder` that read as one, by name, in order.
pub fn listed(folder: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let path = e.path();
            let name = if path.is_dir() {
                path.file_name()?
            } else if path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("toml"))
            {
                path.file_stem()?
            } else {
                return None;
            };
            name.to_str().map(str::to_owned)
        })
        .filter(|name| read(&files_in(folder, name)).is_some_and(|(_, _, p)| p.is_some()))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    names
}

/// The first of `files` there is: where it is, what it holds, and its
/// palette, if it reads as one.
fn read(files: &[PathBuf]) -> Option<(&Path, String, Option<Palette>)> {
    files.iter().find_map(|f| {
        let text = std::fs::read_to_string(f).ok()?;
        let palette = desktop::palette_in(f, &text);
        Some((f.as_path(), text, palette))
    })
}

/// The theme being followed: its files, and what was read from them last.
#[derive(Default)]
struct Watch {
    files: Vec<PathBuf>,
    read: Option<String>,
}

impl Watch {
    /// Look at the theme's file: the palette to draw the window in, if it
    /// is not what was read before. None while it cannot be read, as
    /// mid-switch, when it is briefly not there, nor if what it holds is not
    /// a palette; then what was drawn before stands.
    fn look(&mut self) -> Option<Palette> {
        let (_, text, palette) = read(&self.files)?;
        if self.read.as_deref() == Some(text.as_str()) {
            return None;
        }
        self.read = Some(text);
        palette
    }
}

/// Keeps the window drawn in the theme chosen: as it is when chosen, and,
/// from a thread of its own that looks at its file every little while, again
/// whenever it changes. Nothing is woken for it until it does.
#[derive(Default)]
pub struct Follower {
    watch: Arc<Mutex<Watch>>,
    /// The theme the window is drawn in, once one has been.
    shown: Option<Choice>,
    /// Whether the thread that watches has been started: not until there is
    /// a file to watch.
    watching: bool,
}

impl Follower {
    /// Draw the window in `choice`, if it is not drawn in it already: in the
    /// design system's palette where it has no file, or its file does not
    /// read as one.
    pub fn show(&mut self, ctx: &egui::Context, choice: &Choice) {
        if self.shown.as_ref() == Some(choice) {
            return;
        }
        self.shown = Some(choice.clone());
        let (files, fixed) = match choice.source() {
            Source::Fixed(palette) => (Vec::new(), Some(palette)),
            Source::Files(files) => (files, None),
        };
        let watch = !files.is_empty();
        let palette = {
            let mut w = self.watch.lock().unwrap_or_else(PoisonError::into_inner);
            *w = Watch { files, read: None };
            fixed.or_else(|| w.look()).unwrap_or(Palette::CAMCRAFT)
        };
        theme::set_palette(ctx, palette);
        if watch && !self.watching {
            self.watching = true;
            spawn(Arc::downgrade(&self.watch), ctx.clone());
        }
    }
}

/// The thread that watches the theme's file, for as long as `watch` is.
fn spawn(watch: Weak<Mutex<Watch>>, ctx: egui::Context) {
    // Without it, the theme stands as it was when chosen.
    let _ = std::thread::Builder::new()
        .name("camcraft theme".into())
        .spawn(move || {
            loop {
                std::thread::sleep(LOOK_EVERY);
                let Some(watch) = watch.upgrade() else {
                    return;
                };
                let palette = watch.lock().unwrap_or_else(PoisonError::into_inner).look();
                if let Some(palette) = palette {
                    theme::set_palette(&ctx, palette);
                }
            }
        });
}

/// Open the themes folder where the person can see it and drop a palette in,
/// making it first if it is not there yet.
pub fn open_folder() -> std::io::Result<()> {
    let Some(folder) = folder() else {
        return Err(std::io::Error::other("no config folder"));
    };
    std::fs::create_dir_all(&folder)?;
    let opener = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener).arg(&folder).spawn()?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    const DARK: &str = "background = \"#1a1b26\"\nforeground = \"#a9b1d6\"\naccent = \"#7aa2f7\"\n";
    const LIGHT: &str = "background = \"#ffffff\"\nforeground = \"#202020\"\n";

    /// A folder of the test's own.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("camcraft-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A palette is listed whether it is a file of its own or an Omarchy
    /// theme's folder dropped in as it is; what is not a palette is not.
    #[test]
    fn palettes_are_listed_as_files_or_theme_folders() {
        let dir = scratch("themes-listed");
        std::fs::write(dir.join("tokyo-night.toml"), DARK).unwrap();
        std::fs::create_dir(dir.join("Catppuccin-Latte")).unwrap();
        std::fs::write(dir.join("Catppuccin-Latte/colors.toml"), LIGHT).unwrap();
        std::fs::create_dir(dir.join("empty")).unwrap();
        std::fs::write(dir.join("broken.toml"), "accent = \"#ff0000\"").unwrap();
        std::fs::write(dir.join("readme.md"), "not a palette").unwrap();
        assert_eq!(listed(&dir), ["Catppuccin-Latte", "tokyo-night"]);
        assert!(listed(&dir.join("not-there")).is_empty());
    }

    /// A theme folder is light by its marker, as Omarchy's are; a file of its
    /// own by what it says or how light its background is.
    #[test]
    fn a_theme_folders_marker_makes_it_light() {
        let dir = scratch("themes-marker");
        std::fs::create_dir(dir.join("marked")).unwrap();
        std::fs::write(dir.join("marked/colors.toml"), DARK).unwrap();
        std::fs::write(dir.join("marked/light.mode"), "").unwrap();
        std::fs::write(dir.join("light.mode"), "").unwrap();
        std::fs::write(dir.join("plain.toml"), DARK).unwrap();
        let (_, _, marked) = read(&files_in(&dir, "marked")).unwrap();
        assert!(!marked.unwrap().dark);
        let (_, _, plain) = read(&files_in(&dir, "plain")).unwrap();
        assert!(plain.unwrap().dark, "a marker beside it is not its own");
    }

    /// A palette is drawn when first read and again only when it changes;
    /// while it cannot be read, or is not a palette, what was drawn stands.
    #[test]
    fn a_palette_is_drawn_as_its_file_changes() {
        let dir = scratch("themes-watched");
        let file = dir.join("mine.toml");
        std::fs::write(&file, DARK).unwrap();
        let mut w = Watch {
            files: files_in(&dir, "mine"),
            read: None,
        };
        assert!(w.look().unwrap().dark, "first read");
        assert!(w.look().is_none(), "unchanged");
        std::fs::write(&file, LIGHT).unwrap();
        assert!(!w.look().unwrap().dark, "changed");
        std::fs::remove_file(&file).unwrap();
        assert!(w.look().is_none(), "mid-switch");
        std::fs::write(&file, "not toml [").unwrap();
        assert!(w.look().is_none(), "not a palette");
        std::fs::write(&file, LIGHT).unwrap();
        assert!(w.look().is_some(), "back again");
    }

    /// Every theme Omarchy ships is bundled, and reads as a palette, light
    /// or dark as it says it is.
    #[test]
    fn omarchys_themes_are_bundled_and_read() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/themes");
        let mut files: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| {
                let name = e.unwrap().file_name().into_string().unwrap();
                name.strip_suffix(".toml").map(str::to_owned)
            })
            .collect();
        files.sort();
        let bundled: Vec<&str> = BUNDLED.iter().map(|(n, _)| *n).collect();
        assert_eq!(bundled, files, "assets/themes/ and BUNDLED differ");
        for (name, text) in BUNDLED {
            let palette = desktop::palette_of(text, false)
                .unwrap_or_else(|| panic!("{name} does not read as a palette"));
            let light = text.contains("mode = \"light\"");
            assert_eq!(palette.dark, !light, "{name}");
            assert_ne!(
                palette.text_1, palette.surface_2,
                "{name}: text on its card"
            );
        }
    }

    /// A bundled theme is a fixed palette; one no longer bundled is the
    /// design system's, as is camcraft.
    #[test]
    fn a_bundled_theme_is_drawn_as_it_is() {
        let Source::Fixed(p) = Choice::Bundled("tokyo-night".into()).source() else {
            panic!("a bundled theme has no file");
        };
        assert_eq!(p.accent, [0x7a, 0xa2, 0xf7]);
        let Source::Fixed(p) = Choice::Bundled("gone".into()).source() else {
            panic!("a theme no longer bundled has no file");
        };
        assert_eq!(p, Palette::CAMCRAFT);
        assert!(matches!(Choice::Camcraft.source(), Source::Fixed(p) if p == Palette::CAMCRAFT));
    }

    #[test]
    fn a_palettes_name_reads_as_a_title() {
        assert_eq!(title("tokyo-night"), "Tokyo Night");
        assert_eq!(title("catppuccin_latte"), "Catppuccin Latte");
        assert_eq!(title("Rose-pine"), "Rose Pine");
        assert_eq!(title("a--b"), "A B");
    }

    /// Stored as a word, or the palette file's name.
    #[test]
    fn a_choice_is_remembered() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct S {
            theme: Choice,
        }
        for theme in [
            Choice::System,
            Choice::Camcraft,
            Choice::Bundled("tokyo-night".into()),
            Choice::File("tokyo-night".into()),
        ] {
            let s = S { theme };
            let stored = toml::to_string(&s).unwrap();
            assert_eq!(toml::from_str::<S>(&stored).unwrap(), s);
        }
        assert!(
            toml::to_string(&S {
                theme: Choice::System
            })
            .unwrap()
            .contains("\"system\"")
        );
    }
}
