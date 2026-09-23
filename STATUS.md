# Where this is

Last updated 2026-09-23. Read this before picking the work back up.

**0.2 is feature-complete bar the carried items: the desktop app runs, checks and
posts jobs.** All gates green. The engine takes a STEP or STL model and a job file
and writes G-code, and refuses to write a program that fails a check, the
simulator's against material among them. The web inspector is gone; the app is
where a job is looked at.

**Nothing has been cut yet.** See *Before the first cut* below — there are things
only the machine can tell us.

## What works

```powershell
cd D:\rust\camcraft

# Look first, in the app: Generate, read the checks, scrub, then Post
scripts\run.bat examples\rail-jig.camcraft

# Or from the command line: run and check, nothing written
cargo run --release -p camcraft-cli -- job examples\rail-jig.camcraft

# Then write one program per operation
cargo run --release -p camcraft-cli -- job examples\rail-jig.camcraft --out out
```

A job file names its own models (relative to itself), stocks and setups; each setup
has its own post, work offset, orientation and work zero, and programs are named
`<setup>-NN-<operation>`. `--setup NAME` runs only that setup; `--profile` overrides
every setup's post. A STEP file states its own units; an STL model needs
`units = "mm"` or `"inch"` in the job.

`post` still exists for a single finishing pass described with flags. It builds a
one-operation job and goes through the same `run_setup` as `job` and the app, so
there is exactly one engine path. `crates/camcraft-cli/tests/one_path.rs` parses
every written program back into positions and compares them with the paths
`run_setup` produces in-process, move for move.

**Checks** (`camcraft_session::checks`) are one list, printed by the command line
and shown in the app: `CC1xx` failures (unresolved links, a rapid through the part,
a sideways plunge or retract, feeds that do not suit their move, a plunge at cutting
feed, a rapid through material, a gouge) and `CC2xx` advisories. They are
re-derived from the path and the exact part where they can be — the rapid check
lowers the job's cutter along every rapid — rather than echoing the engine's own
report. The gate is "no failures".

Exit codes: `0` clean, `2` ran with warnings worth reading, `1` a check failed (and,
with `--out`, nothing was written). `--force` overrides and says loudly that it did.

Last full run on the 237 × 227 × 76 mm router jig, read straight from
`sled_base.step`, 17 s on an M-series Mac (26 s before roughing came from the model):

```
rough:  1262 passes, removed 3570370 mm3, travel saved 78%, rapid contacts 0
finish:  556 passes, removed   55924 mm3, travel saved  0%, rapid contacts 0
verified: 155326 columns compared, gouges 0, deepest 0.000 mm
```

Roughing now removes more and finishing less: walls are left exactly the stock to
leave, where they used to be left half a stepover more.

Finishing saving 0% travel is correct, not a failure: a parallel zigzag already ends
each pass next to where the following one starts.

## What is next, in order

1. **Build on Windows: done.** opencascade 0.3.0 did not compile with MSVC: it
   assumes OpenCascade's `Handle_X` is an alias of `opencascade::handle<X>`, which
   Visual Studio makes a derived class instead (`Standard_Handle.hxx`). `vendor/`
   holds the crates.io 0.3.0 sources with upstream PR #234 applied
   (bschwind/opencascade-rs, reviewed line by line; it changes nothing on GCC or
   Clang), patched in from the root `Cargo.toml`. Delete both once a release
   includes the fix. Release and debug builds both link, no further libraries were
   needed, and `camcraft job examples\rail-jig.camcraft` reads the STEP part and
   passes all 13 checks on Windows 11 with MSVC 14.44.
2. **STL format detection: done.** A nameless ASCII file (`solid` then a newline,
   as most opencamlib samples begin) was read as binary and failed, and a binary
   file whose header began `solid ` would have been read as ASCII. The format is
   now decided before `stl_io` sees the file: a binary STL is exactly as long as
   the triangle count in its header says.
3. **0.2: a native desktop app.** Agreed scope, high level; each item gets its own
   detailed plan before work starts:
   - **App shell: done.** `camcraft-gui`, on eframe/egui 0.36 with wgpu, and
     `egui-file-dialog` as in nodemangler. Jobs are documents, as in Fusion: File
     › New, Open (several at once), Open Recent, Save, Save As and Close, each job
     in its own tab (⌃Tab switches), with a prompt before unsaved work is lost, and
     undo/redo (⌘Z, ⌘⇧Z) of whole edits. A job's models load in the background; a
     model not where the job says is asked about, and can be found elsewhere.
     Setups run on worker threads exactly as `camcraft job` runs them, several at
     once, from the job as it is on screen, saved or not. The orchestration lives in
     `camcraft-session`, which both use. Run with
     `bash scripts/run.sh [JOB]...`. Headless PNGs of the real UI:
     `cargo test --release -p camcraft-gui --features ui-snapshots -- --ignored`,
     written to `target/ui-snapshots/`. Built and looked at on macOS only.
   - **3D viewport: done, bar the checks.** Every operation's toolpath, coloured by
     move role from the design system's palette, as screen-space lines that stay
     readable on Retina. A scrub through the whole program in machine order, with
     arrow-key stepping and a readout of the move. Toggles by role, by operation,
     for the part and for the stock outline. The job's own cutter is drawn at the
     scrub position, and still shows, fainter, when a pocket hides it.
   - **Simulated stock view: done.** The material at any stage (the block, after
     each operation, or following the scrub), coloured by the verifier's own rule:
     red below the part, amber left above it (deeper amber for more), grey finished,
     dark where there is no part. The legend's counts are the verifier's counts,
     and a test holds them equal. **Steep descents** are listed deepest first; clicking one
     scrubs to it, follows the stock and flies the camera there. The scrub readout
     gives estimated machining time.
   - **Job editing: done.** The job's tree, after Fusion's browser: Models,
     Stocks and Setups, each setup with its operations and a status dot (not run,
     running, verified, warnings, refused, out of date). Right-click adds, copies,
     moves and removes; the properties panel edits whatever is selected: a model's
     file and STL units, a stock's shape (around models, or a fixed box), a setup's
     models and stocks, work offset, post, orientation (the 24 square ways a part
     sits, with Turn over) and work zero (the model's, or a point on the stock or
     model box, plus an offset), and every operation parameter. An operation's tool
     is chosen from the job's tools or a library folder, and copied in. The view
     shows the selected setup as it sits on the machine, with its stock and its
     work zero's axes; each setup keeps its own camera, scrub and stock view. A
     result goes out of date when anything it was made from changes, and says so.
   - **Picking features: done.** Every model has faces, edges and corners: the
     file's own B-rep faces for STEP, faces found from the triangles for STL
     (`camcraft_geom::topology`). A property filled by picking has **Select in
     view…**, which arms the view for what it takes. Hovering lights what a click
     would pick. The work zero takes a corner, a round edge's centre or an edge's
     middle on a model, or a corner, edge or side of the stock. An operation takes
     faces and edges, which keep Parallel and Z-level rough to their outline seen
     from above (tool centre, whole tool inside, or reaching in from outside, plus
     the margin), drawn in the view at the stock's top. With nothing armed, a click
     shows what is there. Picks are saved as a point on the feature, found again
     after the model is re-tessellated or renumbered, and refused by name when the
     model has changed under them (`docs/decisions/0002-feature-references.md`).
   - **Run, check, post: done.** Background runs (Generate, Generate all). The
     checks panel under each setup's result: a HALT banner when anything fails,
     findings by operation, and a click on one at a move scrubs, follows the stock
     and flies the camera there. **Post {setup}…** and **Post all…** write the
     same programs, names and folder layout as `camcraft job --out`, all setups or
     none, from results that are up to date; they ask before replacing files, and
     a setup that failed a check is posted only after ticking "I know why" and
     pressing Post anyway. The web inspector, `scripts/view.sh` and `--inspect`
     are gone; the design tokens moved to `crates/camcraft-gui/design/tokens.css`.
   - **Steep descents: done.** A plunge is a *continuous* descent steeper than
     `PLUNGE_RATIO` (10:1) that falls further than `max_cut_step`, measured over
     the whole run of moves, not move by move. Finishing breaks its passes there,
     and the gate refuses any that remains; the checks and the app measure it the
     same way.

   Also carried along: item 1 above, and the OpenCascade licensing decision.
   Not in 0.2: new strategies (2D profile, pocket, drilling, waterline, adaptive),
   arc fitting, and feeds and speeds. Those are 0.3. The first physical cut can
   happen whenever it is ready and does not gate the release.

## Before the first cut

The simulator verifies against a *simulated* part. It cannot check the things that
actually break cutters on the day:

- **The Masso profile has never run on the machine.** It was built from the
  controller's documented G-code reference. Dry-run it well above the work first.
- Nothing knows whether the WCS is where the job assumes, whether the tool in the
  spindle matches `diameter`, or whether the stock is the size the job was told.
- Roughing is contour-parallel clearing that plunges into each pass; there is no
  ramp or helix entry yet. It has never cut anything.

## Jobs

A job (`.camcraft`, TOML) has three groups, as in Fusion. It is written as
`version = 1` unless it holds picked features, which need `version = 2`: an older
build would skip them and cut the whole part, so it must refuse the file instead.
An operation with its own spindle speed, or a tool that turns backwards or wants
coolant, needs `version = 3`, for the same reason: an older build would skip it.
All three are read.

- **Models**: design files, referenced by path relative to the job file, never
  copied in. STL needs `units`; STEP states its own.
- **Stocks**: blocks of material in model coordinates, either around models (by
  name, or the setup's own when none are named) with margins, or a fixed box. A
  top and a bottom setup share one block and see it from each side.
- **Setups**: which models and stocks each includes (by name), its work offset,
  post, orientation and work zero (the model's, a point on the stock or model box,
  or a point picked on a model), and its operations, each optionally kept to picked
  faces and edges. Everything past layout
  (`core::setup::prepare`) is in the setup's own frame, so strategies, linker,
  simulator and post never see a rotation.

Each setup simulates from its own stock: the material is a height field seen from
above, so what the top leaves cannot be carried to a flipped bottom. That needs a
volumetric stock, later. The old `job.toml` format is refused by name; there is no
converter. `examples/rail-jig.camcraft` is the reference job. With the default
orientation and work zero, a run is bit-identical to before setups existed.

## Tool manager

**Tools** in the app's toolbar opens a window inside the app, after Fusion's Tool Library,
with the per-user library only
(`~/Library/Application Support/camcraft/tools.toml`, or `CAMCRAFT_TOOL_LIBRARY`),
which saves itself as you edit. Tools are grouped in folders that nest (a folder per
machine, say); drag a tool onto a folder, or use Move to. A library from before
folders reads as its top folder. Tools carry Fusion-style detail: flutes, lengths,
shank, stickout, holder, material and vendor. Each gets a scale drawing and
per-field checks. A job's tools are chosen inside its operations, which copy them
into the job, never link them. A job whose used tools share a T number is refused,
by the CLI too.

Speeds and feeds are **presets**: named sets on a tool ("Roughing", "Finishing",
"MDF", whatever the person calls them), each with any of spindle speed, cutting,
plunge and ramp feed, stepdown, stepover and stock to leave. Choosing a tool for an
operation chooses one of its presets too, the first by default, and copies the
values it has into the operation; the operation then says what has been changed
since, can be reset to the preset, and can save its own values as a new preset of
the job's copy. An operation has its own spindle speed, the tool's unless given; a
job that sets one is written as version 3. A tool's old single `cutting` table
reads as a preset called "Default". Nothing uses the ramp feed until entries ramp.
Beside each speed and feed the editor shows the surface speed (m/min or ft/min)
and chip load (per tooth, needing the tool's flutes), as makers chart them; typing
either sets the spindle speed or feed. Neither is kept, so neither goes stale.

A tool also says which way its spindle turns (M3, or M4 for a left-hand cutter)
and its coolant (off, flood M8 or mist M7), and the program says so; before, every
program was M3 with no coolant. A job with a tool that turns backwards or wants
coolant is written as version 3, since an older build would drop either silently.

**Import…** reads a Fusion tool library exported as CSV, as makers such as Bits &
Bits publish theirs (`fixtures/tools/` holds two), into the folder showing. Each
tool's rows become one tool with a preset per row, named as Fusion named it, in
millimetres; columns are found by their key, not their place. A tool the folder
already has (same product and diameter) gets the file's presets beside its own
instead of a twin. Chamfer mills and spot drills come in as cones; types this build
cannot cut with (drills, face mills, taps and the like) are skipped, and the manager
says which and why.

**V-bits and chamfer mills** are a fourth cutter, a cone: an included angle and an
optional flat tip. The drop-cutter contacts it exactly (vertex and facet in closed
form, edges by the same bounded search as the bullnose), the simulator sweeps it,
and the tool drawing and 3D cutter show its flank. A cone finishes; it is refused
for Z-level roughing, whose reach offsets a widening flank would multiply, and for
stock to leave, since a cone grown by a ball is no longer a cone.

The work offset manager is to join the toolbar. Not yet: Fusion's own `.json`
libraries, which carry holders; drills and a drilling operation; tapered ball
mills.

## Playback (after 0.2): how it is set up

The user wants Fusion-style simulation: play, pause, step, seek and speed, with the
material cut as the tool moves. Item 3 was designed so this is an addition:

- `camcraft_core::replay::Replay` recovers the stock at any move. It starts from the
  nearest stage (`JobResult::stages`) and sweeps forward with the tools the run
  used (`run::cutting_tool`). Sweeping in pieces equals sweeping whole, bit for bit,
  and a test checks it. Removing the one-move overlap makes that test fail. A
  position part-way along a move is a sweep onto a copy, for display only.
- The GUI draws the stock from height and colour textures and a grid with no
  vertex buffer, so a step is a texture write of the tool's footprint.
- Machining time per move is in the scene (`Segment::ends_at`), which is what play
  speed means. Rapids assume 5000 mm/min until a machine profile gives a rate.
- Still to build: the controls, exact per-move seeking with denser keyframes, the
  playhead as move plus fraction, and `Stock::apply` reporting the rectangle it
  touched.

## Open questions

- **Licensing of distributed binaries.** STEP import links OpenCascade statically
  through LGPL-2.1 crates, while camcraft is MIT or Apache-2.0. This doesn't matter
  while the repo is private, but it has to be settled before anyone ships a binary.
  See the decision record.

- **Plunges spend three of the router jig's seven hours in air.** The machining-time
  estimate, checked independently from the inspection JSON, puts `rail-jig.toml` on
  `sled_base.step` at about 7 h. Of that, 129 min is rough plunges and 57 min
  finish plunges. Every link descends from the clearance height to the cut at the
  plunge feed, including through air roughing already cleared. Fusion rapids to a
  feed height just above the material first. This is a linker improvement, and
  worth doing before the first real cut.

- **One unexplained STEP read failure.** During a full gate run on 2026-09-23, two
  session tests reading `hinge_jig.step` failed with OpenCascade's "failed to read
  STEP file". The same tests passed 80 times since, with and without a new lock
  that serialises every OpenCascade call (`camcraft_import::step`). The lock stays
  as a precaution, since OpenCascade's data exchange uses process-wide statics.
  If it happens again, capture the run: this is not understood.

## Grids are only for stock

A rule, not a preference. Height fields represent material — the simulation, rest
machining, clearance — and nothing else. Every point a toolpath follows, and every
boundary it is offset from, comes from the model: exact drop-cutter queries, or
shadows of the mesh. A stock grid may decide *whether* a path cuts somewhere, never
*where* it runs. No smoothing is added to hide anything.

Z-level roughing works that way now (`core::reach`, `geom::shadow`):

- Where the cutter may not be at a level is the mesh's shadow from above, grown by
  the cutter's radius: exact for a flat mill. A ball or bullnose (and stock to leave
  makes every cutter a bullnose) is cut into height bands across which its radius
  grows by at most half the tolerance, each grown by its widest radius, so the
  region is never short of the truth and at most that much over. Vertical walls and
  flats come out exact. Tested against the drop-cutter as the oracle.
- The first ring is that boundary itself, then inward by the stepover. Every offset
  is padded by its arc tolerance so arcs err away from the part.
- The stock grid only clips away stretches of rings that would cut nothing but air.
- Exact slicing of the offset surface would remove the band error altogether. It
  would replace `LevelReach::forbidden` and nothing else, and matters most for
  Z-level *finishing* with a ball on freeform surfaces.

On the lego fixture, the median turn between successive roughing moves fell from 7.2°
to 1.1°, and moves turning more than 20° from 15% to 1.3% (the brick's real corners).
Clipper is now the pure-Rust `clipper2-rust` port, for its arc tolerance control;
it is slower per call than the C++ one, so levels are worked in parallel.

## Things that were wrong and are now right

Worth knowing, because each was found by looking rather than by a test passing:

- **Roughing followed a grid.** Its rings were marching-squares contours of a
  0.4 mm cutter-location grid, their points on cell-edge midpoints, then rounded to
  Clipper's 0.01 mm: a staircase on every curved wall of a smooth CAD part, and on
  the jobs tested it also cleared far less than it could (the two-block test job
  kept almost all its material). See *Grids are only for stock*.
- **Plunges got through finishing, split small.** A steep face that is straight
  passes refinement's chord test at once, so its depth was never compared with
  `max_cut_step`, and the check afterwards judged each move alone. On
  `mount_rush.stl` two moves fell 1.44 mm over 0.07 mm of travel at cutting feed,
  and the engine only warned while the viewer called it a failure. Descents are
  now measured continuously, finishing breaks its passes at them, and the gate
  refuses what remains. The sled's finishing now leaves nothing past the
  allowance (deepest 0.48 mm), where it used to warn of 0.65 mm.
- **Rapids could clip terraces.** The linker's clearance looked at five points under
  the cutter, centre and compass, and a rapid running tangent to roughing's
  terraces passed material on its diagonal. It now takes the highest cell anywhere
  under the swept disc. Hidden until roughing actually cleared material.
- **Cut moves that were really plunges.** On the router jig, parallel finishing
  emitted moves descending 48 mm in 0.01 mm of travel — a full-depth plunge at cutting
  feed. Every point was exactly on the CL surface and every existing check passed; only
  the *connection between* correct points was unmachinable. Found by reading real
  output in the inspector. Passes now break at walls and at gaps in coverage.
- **Marching squares emitted nothing for a uniformly high field**, because such a
  field has no boundary inside the grid. Roughing read that as "nothing to clear" —
  the difference between clearing the whole block and clearing none of it.
- **The simulator's stamp loop missed the cell at the cutter's rim.** Under-removal,
  which is the direction that hides a gouge.
- **The inspector showed a different path from the one that ran.** `inspect` had its
  own single-operation engine path, with a flat clearance plane, no pass reordering
  and a differently sized block, while `job` ran `run_job`. Now there is one path.
  The viewer's rapid check (CC102) used to compare rapids against that plane. It now
  rasterises the part triangles, grows the result by the cutter radius, and checks
  that every rapid clears it, which stays true when clearance follows the material.
- **The STEP containment check wrongly failed correct geometry.** Growing the box
  of the file's points by the largest circle radius is not enough for a surface of
  revolution. `bowl_01`'s rim reaches 190 mm from its axis, on a side of the axis
  where the file has no points at all. A check that fails correct parts gets
  loosened until it means nothing, so it was made right instead.
- **`fixtures/step/README.md` gave the wrong sizes.** They were the box around every
  point in the file, not the part.
- **The CLI logged to stdout.** Errors now go to stderr.
- **The post golden files were never committed.** `*.nc` in `.gitignore` caught
  `tests/golden/*.nc`, so on a fresh clone every golden test failed.
- **The viewer drew off-screen on a 2x display.** The canvas laid out at its pixel
  size, which pushed its own column wider, over and over.
- **The determinism gate could not detect non-determinism.** It ran the suite twice at
  different thread counts; a deterministic test run twice simply passes twice, and
  nothing ever compared the runs. Replaced with tests that diff output against itself.

## Standing cautions

- **`fixtures/step/` holds real shop jigs.** They are not customer work, but they are
  not public either. Decide before making the repo public — deleting them later does
  not remove them from history. One commercial part was already removed and scrubbed;
  parts that cannot be published belong in `CAMCRAFT_FIXTURES`.
- **The public fixture set cannot reproduce the bug that disqualified `truck-stepio`.**
  The part that exposed it is private. A synthetic sculpted-surface fixture should
  replace it.
- **This has now been worked on from macOS as well as Windows.** Paths above are the
  Windows ones.
- **Run `bash scripts/check.sh` unpiped.** Piping through `tail` buffers the whole run
  and hides progress.
- There is no CI while the repo is private, so that script is the only thing between a
  mistake and `main`.

## Map of the code

```
camcraft-geom     BVH, height maps, contours, shadows, synthetic     (leaf)
camcraft-import   STL and STEP (OpenCascade), checked against the file -> geom
camcraft-sim      material removal: stock updater + gouge oracle     -> geom
camcraft-core     tools, reach, strategies, toolpaths, job, run      -> geom, sim
camcraft-post     Op IR, TOML dialect renderers                      -> geom, core
camcraft-session  load, run, check, gate, post: shared by CLI and app -> all libraries
camcraft-cli      post / job                                          -> session
camcraft-gui      desktop app: egui/eframe, wgpu viewport             -> session
```

`core` cannot see `post`, so it cannot name a G-code concept. That is what keeps
strategies emitting toolpaths rather than instructions.

The full design and its reasoning live in the approved plan at
`C:\Users\danph\.claude\plans\i-use-autodesk-fusion-wobbly-wave.md`. Read it before
changing anything structural; it records eight decisions that would force a rewrite.
