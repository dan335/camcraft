# Themes

The themes Omarchy ships, bundled in the app (`src/ui/themes.rs`, `BUNDLED`) and listed under
Settings' Theme and View → Theme. Each is its theme's `colors.toml`, unchanged, named for its
folder in Omarchy.

| Source | Licence |
|---|---|
| github.com/basecamp/omarchy, `themes/<name>/colors.toml`, branch `quattro` at `6194f46` (2026-11-05) | MIT, `LICENSE-Omarchy.txt` |

To add one, put its `colors.toml` here as `<name>.toml` and add the name to `bundled!` in
`src/ui/themes.rs`; a test holds the two lists to each other.
