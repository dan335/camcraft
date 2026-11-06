//! The Theme setting: which palette the window is drawn in, and keeping it
//! drawn in it as its file changes.
//!
//! *System* follows the desktop's theme where there is one to follow
//! (Omarchy's, [`crate::ui::desktop`]) and is the design system's elsewhere;
//! *camcraft* is the design system's whatever the desktop; and any other is a
//! palette file in camcraft's themes folder, in Omarchy's `colors.toml`
//! format, so an Omarchy theme can be dropped in on any system: a `name.toml`,
//! or a theme's folder as it is, `name/colors.toml`. A file chosen is watched
//! as Omarchy's is, so a palette being written is seen as it is saved. See
//! decision 0018.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::ui::desktop;
use crate::ui::theme::{self, Palette};

/// How often the theme's file is looked at for a change: soon enough to seem
/// to follow it, and a few hundred bytes read.
const LOOK_EVERY: Duration = Duration::from_millis(750);

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
    /// The palette file of this name in the themes folder ([`folder`]).
    File(String),
}

impl Choice {
    /// What it is called in the Theme setting.
    pub fn title(&self) -> String {
        match self {
            Self::System if desktop::omarchy_files().is_some() => "System (Omarchy)".into(),
            Self::System => "System".into(),
            Self::Camcraft => "camcraft".into(),
            Self::File(name) => title(name),
        }
    }

    /// The files it is read from, the first there is: none for the design
    /// system's.
    fn files(&self) -> Vec<PathBuf> {
        match self {
            Self::System => desktop::omarchy_files().unwrap_or_default(),
            Self::Camcraft => Vec::new(),
            Self::File(name) => folder().map(|f| files_in(&f, name)).unwrap_or_default(),
        }
    }
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
        let files = choice.files();
        let watch = !files.is_empty();
        {
            let mut w = self.watch.lock().unwrap_or_else(PoisonError::into_inner);
            *w = Watch { files, read: None };
            theme::set_palette(ctx, w.look().unwrap_or(Palette::CAMCRAFT));
        }
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
#[allow(clippy::unwrap_used)]
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
