## todo list by dan for claude

## done

* [x] can camcraft-core be organized better?
  **Done.** Its 30-odd flat modules now sit in four folders, the same code in each (pure moves,
  plus the paths that name them):
  - `strategy/`: every strategy (parallel, scallop, waterline, flat, patterns, morph, projected,
    pocket clearing, adaptive, planar, face, drilling) and what they share (field, nesting, slope,
    reach); `strategy/mod.rs` is the old `strategy.rs`.
  - `path/`: the toolpath and what works on it after a strategy: ordering, tabs, the linker,
    simadapt.
  - `cutter/`: tool, allowance, cutting, the drop-cutter.
  - `setup/`: setup (now `setup/mod.rs`), frame, features.
  - `job/`, `run/`, `replay.rs` as they were.
  Every `camcraft_core::…` path in session, the app, the CLI, the tests and the docs follows
  (`camcraft_core::strategy::parallel`, `camcraft_core::path::toolpath::Toolpath`, …). Every gate
  passes (`scripts/check.sh`).
* [x] in camcraft-core why is there an adaptive foler with only tests.rs in it?  is this wrong?
  **Done — not wrong.** `adaptive.rs` ends with `#[cfg(test)] mod tests;`, and Rust looks for
  that module's file at `adaptive/tests.rs`. It is how a module keeps its tests in a file of their
  own without itself becoming a folder (`adaptive/mod.rs`). `job/tests.rs` and `run/tests.rs` are
  the same thing for modules that are already folders.
* [x] make an icon for app so that we can use it instead of egui's
  **Done.** A pocket cleared from the middle out, seen from above, in the design system's graphite
  and amber, with the tool at the end of its last lap. Source `crates/camcraft-gui/assets/icon.svg`,
  rendered to `assets/icon.png` (512 px, `inkscape icon.svg --export-type=png -w 512 -h 512`),
  set as the window and Dock icon in `main.rs`, with a test that it loads. A packaged macOS `.app`
  would also want an `.icns` made from the same SVG.
* [x] does camcraft-cli still work?  bring it up to date.
  **Done.** It worked (every CLI test passed), but `post` only knew parallel finishing with a ball
  or flat cutter. Now `post --strategy` takes every 3D finishing strategy by the name a job file
  gives it (parallel, scallop, contour, flat, spiral, radial, morphed-spiral), each starting from
  its own defaults (`StrategySpec::named`, which the Add menu now uses too), `--tool bullnose
  --corner R` for a bullnose, and `--stepdown` for contour. New tests: every `--strategy` value is
  that strategy, and `post --strategy contour --tool bullnose --stepdown 2` writes the same path
  as the equivalent job file, move for move. `docs/jobs.md` says so.
* [x] move post processors to their own file.  make a folder for them.  for example masso should have it's own file.  grbl it's own file.
  **Done.** They already were, mostly: each machine is a TOML file in `crates/camcraft-post/profiles/`
  (`masso.toml`, `grbl.toml`), and the Rust renders whatever a profile says. The one exception was
  Masso in inches, built in Rust from the Masso file; it is now `profiles/masso-inch.toml`, with a
  test that it is the Masso profile in everything but its name and units. The built-in list is one
  table (`BUILTIN` in `profile.rs`), so adding a machine is adding its file and one line there. A
  profile of your own can also be any `.toml` file, chosen per setup.
* [x] audit camcraft-core crate.  run simplify skill on it
* [x] go through each crate and audit it.  run simplify skill on it
  **Done, both.** Every crate was reviewed from four sides (reuse, simplification, efficiency,
  and fixes at the wrong level), about 130 findings, and applied crate by crate. Generated programs
  are byte-identical to before on 22 lego operations of every strategy, except where a bug was fixed
  on purpose (below); every gate passes (`scripts/check.sh --all`). Highlights:
  - **Shared rather than repeated:** the strategies' pass cutting (one `Emit`), slope keeping,
    grid pitch and level lines; the operation model split along Fusion's tabs
    (`job/{strategy_spec,uses,geometry,heights,passes,linking}.rs`); `Uses` now says how a strategy
    links, whether its picks are holes, whether its stepover is a load and whether it has a cutting
    feed, and the run, checks and panel all ask it; linking defaults in one place (the panel said
    adaptive stays down "up to 5 × the tool" while it ran 10 ×); the verifier's cell rule in sim,
    used by the view; the design system's Select, segmented control and Dialog as widgets.
  - **Faster:** parallel finishing samples its lines in parallel (21 s to 6.6 s on the lego);
    the part's shadow is cast once per setup; region ops, shadows and stock restores do less
    work (restore about 8×, verify and cut-past about 6×); posting and scene building left the UI
    thread; the view stopped locking the playback image and rewriting constant GPU buffers every
    frame.
  - **Bugs fixed (these change output where they apply):** an inward Spiral (the default) turned
    the wrong way, cutting conventional when asked to climb; slope-limited Contour, Spiral, Radial
    and Morphed Spiral carried every judging point into the program; Contour never pulled a
    two-point stretch onto the part; 19.05 mm stock at a 3.175 mm stepdown cleared its bottom twice;
    two rims at one height made a drill refuse; a hole touching its outer ring could be dropped from
    a region; the stock volume was not deterministic; the verifier dropped a disc's rim cells at a
    rounding edge; STL welding left orphan vertices; the CLI counted warnings it never printed.
  - **Split up:** `linker/`, `job/`, geom's `contour/` and `topology/build.rs`, the GUI's
    `properties/operation/`, `tools/manager/`, `view/viewport/{lines,upload}.rs`,
    `picking/{shown,highlight}.rs`.
  - **Contour gouged the three-pin lego** (`fixtures/step/lego_3_pins.step`, from the commit that
    added it): 51 columns up to 13.6 mm deep, 31 up to 25 mm by area. Its passes run beside the
    walls but it was linked as if riding over the part, so a short link from one level up to the
    next went straight through the top of a wall. Contour now links at its level, as pocket
    clearing does, and lifts between levels (`Uses::links`); the linker also refuses to stay down
    over the part from a point below it. No gouge on any of the 22 lego operations now, and every
    other program is unchanged. Decision 0010 says so.
  - **Found and left for you** (they changed output or needed a decision): since done, below.
* [x] should tests be in their own file next to their source so that the files are longer?
  **Done — yes, for the big ones.** A module whose tests run to 150 lines or more now keeps them
  in a `tests.rs` beside it (`parallel.rs` + `parallel/tests.rs`), declared at the bottom with
  `#[cfg(test)] mod tests;`, as `adaptive.rs` already did: 36 files moved, the source files a third
  to a half shorter, tests still able to reach private items. Smaller test modules stay inline,
  where the test sits next to what it tests. `ui/theme.rs` keeps its inline tests because they
  `include_str!` the design system's CSS by a path relative to the file.
* [x] any files need to be split up?
  **Done.** Split, all as pure moves (programs byte-identical): `linker.rs` into `linker/` (params,
  clearance, world, entry); `job/operation.rs` into `job/` by Fusion's tabs; `adaptive.rs` into
  `adaptive/` (tiles, clearing, steer); `run/operation.rs` into `run/operation/` (link, finishing,
  clearing); geom's `contour.rs` into `contour/` (clip, stock) and `topology/build.rs`; the app's
  `properties/operation.rs`, `tools/manager.rs`, `view/viewport/gpu.rs` and `picking/slots.rs`. No
  source file is over 1,000 lines now; the longest (adaptive, planar, features, the app's main
  window) are each one piece of work.

* [x] "Needs your call": refer to what Fusion does, verified; definitely do the lighting.
  **Done, all ten.** Fusion's behaviour was checked in Autodesk's help pages, its posts and its
  support articles, not from memory. Where Fusion says nothing, the decision is noted below.
  - **Lighting:** part and material now share dcpwood's rig, turned to Z up
    (`view/viewport/shaders/common.wgsl`, `lit()`). It has a warm hemisphere, a warm key
    raised so tops read lighter than walls, a faint cool fill and rim, the room's light and a
    satin sheen, tone mapped as three.js's Neutral. The key casts no shadows. `docs/app.md`
    says so.
  - **Same-level stay-down checks the material** (`Linker::ploughs`). A level link lifts
    where its middle would plough through stock. Its first and last tool radius count as the
    cuts' own, and it may brush half a radius in. Fusion: keep tool down "will only work if
    ... the toolpath does not violate the work piece".
  - **A stay-down link counts the steep fall it joins**, the cut's before it and the pass's
    after it. It never makes the plunge the gate would refuse.
  - **Parallel's reorder works on curved parts.** Only an operation cut in levels keeps its
    passes to their level (`uses().depths`). Fusion's Parallel orders "by shortest distance"
    unless Simple Ordering is on.
  - **Spiral, Radial and Morphed Spiral keep the slope range asked for and nothing else**,
    both ends included, as Fusion's From/To slope angles. 0-89.9° now cuts what 0-90° does,
    less exactly vertical faces. A steep stretch is cut down the part and broken where it
    falls more than a cut step, as Parallel does. Scallop still leaves steep faces for a
    contour pass.
  - **Faster drop-cutter:** flat and ball mills use their exact contacts instead of the
    bullnose search, and triangles that cannot raise a drop are skipped. All 22 lego
    programs came out byte-identical. The slowest ops ran 5-14 times faster (lego contour
    37 s to 5 s, morphed spiral 72 s to 5.5 s, scallop 26 s to 2.5 s, rough 51 s to 24 s).
  - **One stock corner:** a new setup and the Work zero panel's "back to a corner" both use
    the stock's top front-left corner. That is the corner Autodesk's tutorials and its setup
    API sample pick (`TOP_XMIN_YMIN`). Fusion's own default was not documented.
  - **Adaptive's entries are Fusion's 3D Adaptive's:** helix or plunge (it also has
    Predrill), with no ramp along a ring. Each strategy lists its own in
    `Uses::entries`, the panel offers only those, and a job asking for another is refused.
  - **Dwell** is written in seconds to the millisecond, before the feed, as Fusion's Masso
    post writes G82 (`P0.25 F250.`).
  - **A futile adaptive entry** is cut as a helix alone, so the disc it counts as cleared
    really is. The warning that backed this out (CC202) was a false alarm: arriving at a
    helix's start, already the safe distance clear, is no plunge. Now it is not reported as
    one. On the lego the lone helix cuts air, a sliver the adaptive thought was left.
  - **On the 22 lego operations:** every one verifies with no gouge and passes every check,
    with no warning. The changes are the ones meant:
    - Level links now lift over stock. Contour went from 45 stay-downs to 0 on a raw block,
      rough from 583 to 523, flat from 192 to 168.
    - Parallel saves 58% of its travel (it was 0%).
    - Radial and morphed spiral with only a lower slope limit now cut their steep stretches:
      radial went from 3,883 to 6,428 passes.
    - Adaptive gains one lone helix.
    - The other programs are byte-identical.

