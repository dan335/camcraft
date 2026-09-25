# Fonts

The CamCraft Design System's typefaces, bundled in the binary so the app looks the same
everywhere and never fetches anything (the design system's own `tokens/fonts.css` loads them
from Google Fonts as a stand-in, and asks for local files).

| File | Source | Licence |
|---|---|---|
| `IBMPlexSans-{Regular,Medium,SemiBold}.ttf` | IBM Plex v6.4.0, `IBM-Plex-Sans/fonts/complete/ttf/` (github.com/IBM/plex) | SIL OFL 1.1, `OFL-IBMPlexSans.txt` |
| `FiraCode-{Regular,Medium}.ttf` | npm `firacode@6.2.0`, `distr/ttf/` | SIL OFL 1.1, `OFL-FiraCode.txt` |
| `lucide-subset.ttf` | npm `lucide-static@1.47.0`, `font/lucide.ttf`, cut to the icons in `src/ui/widgets.rs` | ISC, `LICENSE-Lucide.txt` |

The Lucide font is 900 KB whole, so only the glyphs the app draws are kept. To add an icon, look
its codepoint up in `font/codepoints.json` of the same package, add it to `Icon` in
`src/ui/widgets.rs`, and cut the font again with every codepoint listed there:

```text
pip install fonttools
pyftsubset lucide.ttf --output-file=lucide-subset.ttf --unicodes="U+E13D,U+E1B2,U+E070,U+E06D,U+E06F,U+E09E,U+E18E,U+E151,U+E19E,U+E0D9,U+E0D7,U+E334,U+E042,U+E13C,U+E12E,U+E15F,U+E160,U+E3E9,U+E3EA,U+E147,U+E243,U+E244,U+E120"
```
