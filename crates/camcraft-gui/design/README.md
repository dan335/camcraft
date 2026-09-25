# Design

The GUI's look comes from the claude.ai/design project **CamCraft Design System**. `system/` is
a copy of it, taken on 2026-09-23, so the rules are readable offline and the tests can hold the
app to them.

| Path | What |
|---|---|
| `system/readme.md` | The system's own guide: voice, visual foundations, iconography. Start here. |
| `system/tokens/*.css`, `system/styles.css` | Colours, type, spacing, effects. `src/ui/theme.rs` mirrors the values it needs, and its tests read these files to check them. |
| `system/components/` | Each component as React (`.jsx`), its props (`.d.ts`) and a usage note (`.prompt.md`). The app's egui versions are in `src/ui/widgets.rs` and `src/ui/theme.rs`. |
| `system/guidelines/` | Specimen pages for the foundations. They open in a browser, next to `styles.css`. |
| `system/SKILL.md` | The system's summary for an agent. |
| `local.css` | Ours, not the system's: deliberate deviations, each with its reason. |

Everything under `system/` is copied as published. Don't edit it here. Its `readme.md`
describes camcraft as it was when the system was made (a 0.1 command line with
`job.toml`, and no GUI planned); the app's own docs are in `docs/app.md` at the root. When the design system
changes, copy it again, then update `src/ui/theme.rs` until its tests pass.

Left out: the project's generated bundle and manifest (`_ds_bundle.js`, `_ds_manifest.json`),
its thumbnail, the component preview cards (`*.card.html`, which need the bundle), and the web
UI kits under `ui_kits/` (a terminal and a docs site, not the GUI).

The fonts are in `../assets/fonts/`, bundled in the binary. The system's `tokens/fonts.css`
loads them from Google Fonts, as a placeholder.
