# 0018 — Themes: Omarchy's followed, and a plan for everywhere else

**Status:** Omarchy decided and built, 2026-11-04; the Theme setting and
palette files built, 2026-11-05; the light variant and following the OS's
light or dark planned ·
**Milestone:** 0.5

## Question

The window is drawn in the design system's one theme, dark graphite and amber
([readme](../../crates/camcraft-gui/design/system/readme.md)). On Omarchy
every app follows the desktop's theme as it is switched. What should camcraft
do there, and on Windows, macOS and other Linux desktops?

## How other software does it

- **Omarchy** keeps the theme it is in as one palette, `colors.toml` (4.x:
  `~/.local/state/omarchy/current/theme/`; 3.3–3.8:
  `~/.config/omarchy/current/theme/`), with semantic keys: `mode`, `accent`,
  `background`, `dark_background`, `darker_background`,
  `lighter_background`, `foreground`, `dark_foreground`,
  `bright_foreground`, `muted`, `selection`, and the terminal's colours.
  `omarchy-theme-set` replaces the folder on a switch, restarts what it ships,
  sets GNOME's `color-scheme`, and runs `~/.config/omarchy/hooks/theme-set`.
  Its own guide for apps it does not ship: read `colors.toml`, watch it, and
  never write into Omarchy's config. Zed follows it by a hook (omazed), VS Code
  and Obsidian by scripts Omarchy runs; no egui app was found that does.
- **The XDG portal** gives light or dark (`org.freedesktop.appearance
  color-scheme`) and, on some desktops, an accent; on Omarchy, light or dark
  only, through xdg-desktop-portal-gtk.
- **eframe** follows the system's light or dark on Windows and macOS, as it
  changes; on Linux winit reports nothing, so it falls back to dark. No part
  of egui knows an accent colour.
- **Zed** has `"theme": {"mode": "system", "light": …, "dark": …}` and theme
  files; **VS Code** a preferred light and dark theme it switches between
  with the system's; **Obsidian** light, dark or the system's, with its own
  accent picker; **Blender** theme presets; **Fusion** a few built-in UI
  themes, not following the system. None takes its palette from the
  system, only light or dark; an OS accent is rare.

## What was decided

- **On Omarchy the window follows its theme.** Where its `colors.toml` is,
  it is read when the app starts and again every three quarters of a second,
  and the window is redrawn in it as soon as it changes, light themes light
  (`ui::desktop`, watched by `ui::themes`). Keys a theme leaves out are worked out as
  `omarchy-theme-color` works them out, older themes' terminal colours
  included. Nothing is written into Omarchy's folders and no hook is
  installed: reading is enough, and an app that is not running needs nothing.
- **The palette is the window's, not the view's meanings.** A theme sets the
  surfaces (`darker_background` behind everything, then `dark_background`,
  `background`, `lighter_background`), hairlines and text (mixed from the
  foregrounds), and the accent. What says something keeps the system's
  colours whatever a theme calls red: moves by role, the axes, picks and
  boundaries in the view, the material and its comparison with the part, and
  ok, warning and error (deepened on a light ground so they can be read).
  Omarchy's *Matte Black* names amber "green", and *White* has no red at all.
- **The colours are a value, not constants** (`theme::Palette`, read with
  `theme::palette()` and its named colours, changed with
  `theme::set_palette`), so any source of a theme drives the same window.

- **A Theme setting chooses the palette** (`ui::themes`): *System*, the
  default, follows the desktop's theme where there is one, Omarchy's, and is
  the design system's elsewhere; *camcraft* is the design system's whatever
  the desktop; and the rest are palette files. That is items 2 and 3 below,
  but for *Dark* and *Light*, which wait on item 1: until there is a light
  variant, *System* has nothing to switch to off Omarchy.
- **The themes Omarchy ships are bundled**, all of them, as its
  `colors.toml` files (`assets/themes/`, MIT): listed after *camcraft* and
  before the person's own, as Omarchy lists its own before the person's.
  They are compiled in, not copied into the themes folder, so a release's
  changes to them reach everyone and the folder holds only what the person
  put there; a theme no longer bundled falls back to the design system's.
  **View → Theme** lists them too.
- **Palette files are Omarchy's format**, in camcraft's config folder's
  `themes/` (`%APPDATA%\camcraft\config	hemes` on Windows,
  `~/.config/camcraft/themes` on Linux): a `name.toml`, or an Omarchy theme's
  folder dropped in as it is, `name/colors.toml`, light by its `light.mode`
  as Omarchy's are. Only those that read as a palette are listed, and the one
  chosen is watched as Omarchy's file is, so a palette being written is seen
  as it is saved.

## Planned for everywhere else

1. **A light variant of the design system**, designed in the design project
   rather than derived here, with its own tokens and tests as the dark one has.
2. **A Theme setting**: *System* (the default), *Dark*, *Light*; on Omarchy a
   fourth, *Omarchy*, chosen by itself. *System* follows light or dark:
   eframe's own on Windows and macOS; on Linux the portal's `color-scheme`,
   read and listened to (the `dark-light` crate, or `ashpd`), since winit
   gives nothing there.
3. **Palette files**: a theme is a palette in Omarchy's `colors.toml` format,
   read by the same code, so an Omarchy theme can be dropped in on any system
   and one written for camcraft works on Omarchy. Kept in camcraft's config
   folder, listed under the Theme setting.
4. **The system's accent, if asked for**: off by default, since amber is the
   system's identity and the spindle's warning colour; where it is on, from
   Windows' and macOS's accent and the portal's `accent-color` (the
   `system-theme` crate covers all three).

What stays as it is: the view's meanings above, and one palette at a time
for the whole window.
