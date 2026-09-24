# CamCraft Design System

**camcraft** is 3-axis CAM for CNC routers. It takes a 3D model (`.stl` / `.step`) and a job description (`job.toml`) and emits G-code (`.nc`). Model in Plasticity or FreeCAD; machine with camcraft. Status at time of writing: **0.1-a, pre-alpha** — a Rust workspace scaffold, headless (CLI + library), no GUI planned for 0.1.

## Sources

- GitHub: https://github.com/dan335/camcraft (Rust workspace: `camcraft-geom`, `-import`, `-sim`, `-core`, `-post`, `-cli`; `docs/decisions/0001-step-backend.md`; `fixtures/step/README.md`). Read in full on 2026-09-22.
- Visual reference given by the user: https://www.plasticity.xyz/ — used only for *direction* (dark, minimal, technical, no marketing fluff). No Plasticity assets, colours or marks are reproduced here.

**The repo contains no UI code, no logo, no fonts, no icons, no screenshots.** Everything visual in this system is therefore authored from scratch against the product's character (see Visual Foundations) and flagged as such. The brand mark is the word `camcraft` set in plain type; there is no logo.

## Products / surfaces

1. **CLI** — `camcraft <model> <job.toml> -o <out.nc> [--no-verify]`. The real product today. Its "UI" is terminal output: structured diagnostics, a gate summary (`pass` / `FAIL`), tracing logs.
2. **Docs / project site** — README-style documentation: architecture DAG, correctness rules, decision records, fixture tables. Authored here as a plausible docs surface using the repo's own copy verbatim.
3. **Future GUI** — explicitly out of scope for 0.1. Not designed. Components here are sized for a dense desktop tool so a viewer/job editor can be built later without a restyle.

---

## CONTENT FUNDAMENTALS

Copy is written by an engineer for engineers. It is terse, declarative and unafraid of numbers.

- **Voice:** third person / imperative. No "we", no "you'll love". `Do not point this at a machine.` `Run it before every push.`
- **Sentence shape:** short declaratives, often fragments. `Millimetres, Z-up, right-handed.` `Headless. A CLI and a library.`
- **Casing:** sentence case everywhere, including headings (`What it is`, `Correctness rules`). Product name always lowercase `camcraft`. Crate names in code style.
- **Emphasis:** bold for the rule or the noun that matters (`**Deterministic output.**`, `**tool tip**`). Italics for the logical pivot (`*cannot*`, `*not* to do`). Never both.
- **Numbers are content.** Specifics beat adjectives: `443 ms, 74 tris`, `25.4× too small`, `50+ person-years`. Tables of measurements are the primary persuasive device.
- **Reasoning is shown.** Every rule carries its consequence: `Silently emitting bad G-code is the worst available failure mode.` Decision records use Question / Pass criteria / Result / Decision / Consequences.
- **Safety register:** blunt, not alarmist. `This drives a machine that can destroy a part, a cutter, or itself. These are not style preferences.`
- **Terminal output:** lowercase gate names (`fmt`, `clippy`, `test`), `pass` lowercase green, `FAIL` uppercase red, summary line `All gates passed.` / `N gate(s) failed.`
- **Punctuation:** em dashes for asides, `->` in ASCII diagrams, backticks for every identifier, unit, file and flag.
- **No emoji. No exclamation marks. No marketing superlatives.** Where Plasticity says "unparalleled", camcraft says "no mesh decimation, no vertex budgets".
- **Spelling:** British-leaning (`millimetres`, `optimisation`, `tokenise`).

## VISUAL FOUNDATIONS

Direction: a machine enclosure at night. Near-black graphite, one amber indicator, monospace numbers. Precision over decoration.

- **Colour:** dark-first, single theme. Surfaces step through `--gray-0…4` (#0B0C0E → #2A2D35); text through `--gray-6…9`. One accent, **amber `#F5A524`** — the spindle-warning colour — used for primary actions, focus, links and "plunge" moves. Toolpath semantics follow G-code viewers: cut green `#5BE37A`, rapid red `#FF5C5C`, plunge amber, lead-in cyan `#4FD1FF`; the same four double as ok/err/warn/info. Subtle tints are the colour at 12% alpha, never a lighter solid.
- **Type:** IBM Plex Sans for UI and prose (400/500/600), Fira Code for code, numbers, coordinates, file names and anything a machine reads. Tabular numerals and slashed zero on (`--font-numeric`). Headings sentence case, semibold, tight tracking (-0.02em); small labels 11–12px, uppercase with +0.06em tracking. Base UI size 14px; body prose 16px; nothing below 11px.
- **Spacing:** 4px base, tokens `--space-1…10` (2 4 8 12 16 24 32 48 64 96). Controls 24/28/36px tall. Dense: a desktop tool, not a website — 8px between related controls, 24px between groups.
- **Radii:** sharp. 2px default (`--radius-sm`), 4px max on dialogs. No pills except status dots. No circles except radios.
- **Borders:** 1px hairlines (`--border` #2A2D35) do the structural work that shadows do elsewhere. Cards, panels and inputs are all 1px-bordered surfaces one step lighter than their parent.
- **Shadows:** none on resting elements. Popovers and dialogs get a 1px ring plus a deep soft black drop (`--shadow-popover`, `--shadow-dialog`). No inner shadows.
- **Backgrounds:** flat colour. The only texture is an optional 24px CAD grid of 4%/8% white lines (`--grid-line`, `--grid-size`) behind viewport-like areas. No gradients, no illustrations, no photography. Imagery, when it exists, is a rendered toolpath or a terminal.
- **Hover:** +5% white overlay (`--hover-overlay`) on neutral controls; accent buttons lighten to `--accent-hover`. Links underline on hover. **Press:** +9% overlay / `--accent-press`; no scale change. **Focus:** 3px amber ring offset by a 1px bg gap (`--focus-ring`), keyboard-visible only. **Disabled:** 40% opacity, no colour change.
- **Motion:** minimal and fast. 80/140/220ms, `--ease-out`. Fades and 4px slides for popovers; no bounces, no springs. Toolpath drawing may animate along the path.
- **Transparency & blur:** only for floating panels over a viewport (`--glass` at 85% with 12px blur). Never on static layout.
- **Layout:** fixed 40px top bar; left sidebar 260px; right inspector 320px; content fills. Docs: 720px measure. Fixed elements are separated by 1px borders, not shadows.
- **Cards:** `--surface-2` on `--surface-1`, 1px `--border`, 2px radius, 16px padding, no shadow. Header row is an 11px uppercase label.
- **Data:** numbers right-aligned in mono; units in `--text-3`; the value the reader should compare is `--text-1`, everything else `--text-3`.

## ICONOGRAPHY

The repo ships no icons. The system uses **Lucide** (CDN, 1.5px stroke, 16px default) as the icon set: outline, geometric, technical — it sits well against Fira Code. Load via `https://unpkg.com/lucide@latest` or inline SVG paths copied from Lucide. **Substitution flagged** — swap for a brand set if one appears.

- Icons are always 16px in controls, 20px in empty states; stroke colour `currentColor`; never filled; never coloured except status.
- Unicode is used as icon in terminal contexts: `->` (never `→`) in diagrams, `×` for dimensions (`25 × 32 × 33`), `·` as a separator.
- Status is a 6px square dot (not a circle), coloured with `--ok/--warn/--err/--info`.
- No emoji, anywhere.
- No logo exists. Render `camcraft` in Fira Code 500 where a mark would go.

## Intentional additions

Components beyond the standard set, added because the product needs them:
- **Diagnostic** — structured diagnostic row (level, code, message, location). "Structured diagnostics, never silence" is a correctness rule.
- **Terminal** — monospace output block with gate summary styling. The CLI *is* the product.
- **KeyValue** — label/value pairs for job parameters (tool, stepover, tolerance) with mono values and units.

## Index

- `styles.css` — entry; imports `tokens/` (`fonts`, `colors`, `typography`, `spacing`, `effects`).
- `guidelines/` — foundation specimen cards (Colors, Type, Spacing, Brand).
- `components/core/` — Button, IconButton, Input, Select, Checkbox, Radio, Switch, Badge, Tag, Tabs, Card, Dialog, Toast, Tooltip, KeyValue.
- `components/cam/` — Diagnostic, Terminal.
- `ui_kits/cli/` — terminal recreation of a camcraft run (job.toml, check.sh gates, diagnostics).
- `ui_kits/docs/` — project docs site (README copy verbatim, architecture DAG, fixture table).
- `assets/` — empty by design; no brand assets exist in the source.
- `thumbnail.html`, `SKILL.md`.
