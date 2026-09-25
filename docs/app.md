# The desktop app

`camcraft-gui`, on eframe/egui 0.36 with wgpu. Run it with
`bash scripts/run.sh [JOB]...` (`scripts\run.bat` on Windows). Built and looked
at on macOS; it builds on Windows.

It goes through `camcraft-session` for everything that is not drawing: loading,
running, checking and posting are the command line's code, so the two cannot
disagree about what runs.

## Documents

Jobs are documents, as in Fusion: File › New, Open (several at once), Open Recent,
Save, Save As and Close, each job in its own tab at the top (⌃Tab switches), with a
prompt before unsaved work is lost, and undo and redo (⌘Z, ⌘⇧Z) of whole edits. A
job's models load in the background; a model not where the job says is asked
about, and can be found elsewhere. Save As keeps relative model paths pointing at
the same files.

The window is the job's tree on the left, the properties panel beside it, the view,
and the transport under the view. The Add menu adds models, stocks, setups and
operations; View opens the log and the checks in their own windows; the Setup menu
posts.

## The tree

After Fusion's browser: Models, Stocks and Setups, each setup with its operations.
Each setup and each operation has its own status dot (not run, running with
progress, verified, warnings, refused, out of date, turned off). Right-click adds,
duplicates, moves up or down, removes, and turns an operation off or on; a model
not found can be found elsewhere. Operations drag to a new place, in their setup
or another. Selecting a setup shows all its toolpaths, an operation only its
own.

A result goes out of date when anything it was made from changes (the operation,
its tool, or where the part and stock sit), and says so.

## Properties

The properties panel only edits; what the tree and its menus do is not repeated in
it.

- **Model**: its file, and units for an STL.
- **Stock**: its shape. Around models, it has margins. A fixed box has a width,
  depth and height, and a position on each axis against the models. Below that
  is the block it comes to: its size, and its corners in model coordinates.
- **Setup**, on tabs:
  - *Parts*: cards for the models and stocks it includes, each with a remove
    button. Add lists the job's others; **Select in view…** picks them in the view,
    where the job's other models and stocks show faintly where the setup would
    put them.
  - *Position*: orientation (the 24 square ways, with Turn over) and work zero,
    chosen from a small top view of the box with a height, or picked on the part,
    with its offset folded away.
  - *Post*: work offset and machine profile.
- **Operation**, on tabs: *Tool* (the tool and its feeds and speeds), *Geometry*
  (Fusion's machining boundary: the stock, the part's bounding box or silhouette,
  or picked faces and edges; where the tool goes against it; an additional
  offset), *Heights* (clearance, top and bottom,
  drawn in the view as planes), *Passes* and *Linking* (retract feed, safe
  distance, stay-down distance, and the lead radius or the ramp angle and feed;
  one left out says what the strategy's default comes to for the tool). Choosing a
  tool whose preset gives no stepover sets Fusion's: a quarter of the diameter for
  parallel with a square end, the tolerance's cusp with a round one, and 0.6 of
  the diameter for Z-level. A Generate button sits at
  the panel's foot. The strategy is fixed once the operation is made.

## Picking in the view

Every model has faces, edges and corners: the file's own B-rep faces for STEP,
faces found from the triangles for STL (`camcraft_geom::topology`). As in Fusion,
the view picks only while a slot is armed by its button in the panel (**Select in
view…**); otherwise a click does nothing. Hovering lights what a click would pick,
and a slot being picked into brings its tab forward.

- The work zero takes a corner, a round edge's centre or an edge's middle on a
  model, or a corner, edge or side of the stock.
- An operation whose boundary is a selection takes faces and edges, which keep
  parallel finishing and Z-level roughing to their outline seen from above.

Any boundary but the stock's is drawn in the view at the stock's top. It shows
the boundary as chosen (wide), where the tool's centre may go against it (thin),
and, hatched, what the tool cuts outside the part. Containment and offset are as
Fusion's: the tool's centre on the boundary, the whole tool inside it, or the
tool reaching in from outside it. A positive offset moves the boundary out, or
in when the whole tool is kept inside. Choosing the part's bounding box or
silhouette with the tool reaching in from outside, when the offset is 0, sets
the offset to the tool's diameter. That clears a channel two tools wide round
the part, where an offset of 0 would only touch its walls.
- A drilling operation takes holes by their round top rims, and optionally their
  bottom rims.

## Running and checking

**Generate** on a setup works out every operation, on a worker thread, exactly as
`camcraft job` runs it, from the job as it is on screen, saved or not; several
setups run at once. **Generate** on an operation works it out alone, keeps every
other one made before and unchanged since (cutting it into the material again),
and leaves out the rest, which may be unfinished.

The checks window lists each setup's findings by operation, under a HALT banner
when anything fails; a click on one at a move takes the playhead there, follows
the stock and flies the camera there. **Steep descents** are listed deepest first
in the Show panel, and work the same way.

## Posting

**Post {setup}…** and **Post all…** write the same programs, names and folder
layout as `camcraft job --out`, all setups or none, from results that are up to
date. They ask before replacing files, and a setup that failed a check is posted
only after ticking "I know why" and pressing Post anyway.

## The view

Every operation's toolpath, coloured by move role, as screen-space lines that stay
readable on Retina; toggles by role, by operation, for the part and for the stock
outline. The job's own cutter is drawn at the playhead, hidden by the stock and
part in front of it. The part is shaded with half-resolution GTAO, the material
with horizon ambient occlusion on its own heights. Each setup keeps its own
camera, playhead and stock view.

The stock can be shown as the block, as it stands after any operation, or cut as
the program plays. *Compare* colours it by the verifier's own rule: red below the
part, amber left above it (deeper amber for more), grey finished, dark where there
is no part. The legend's counts are the verifier's counts, and a test holds them
equal.

## Playback

A transport bar under the view: the timeline (one span per operation, as long as
it takes to run, failed checks marked red, warnings and steep descents amber; click
or drag to go anywhere), start, previous operation, step back, play backwards,
play, step on, next operation and end, the estimated machining clock, the speed
(¼× to 10,000×) and where playing stops by itself (at problems, on by default; at
the end of each operation). Under it, the operation and the move under way.

Keys, when no field has the keyboard: Space plays and pauses; J, K and L shuttle
backwards, pause and forwards, and J or L again goes faster; Left and Right step a
move (Shift: a hundred); Up and Down go to the previous and next operation; Home
and End; `[` and `]` change the speed.

Machining time is length over feed, rapids at an assumed 5000 mm/min until a
machine profile gives a rate. How the material keeps up is in
[simulation.md](simulation.md).

## Tool library

**Tools** opens the per-user library, after Fusion's Tool Library
(`~/Library/Application Support/camcraft/tools.toml` on macOS, or
`CAMCRAFT_TOOL_LIBRARY`), which saves itself as you edit. Tools are grouped in
folders that nest (a folder per machine, say); drag a tool onto a folder, or use
Move to. Tools carry Fusion-style detail: flutes, lengths, shank, stickout,
holder, material and vendor, with a scale drawing and per-field checks. A value
left out reads "not given".

The **System library** is always there, above the person's own folders: twenty
common inch router bits (up-cut, down-cut and compression spirals, O-flutes, ball
noses, V-bits and surfacing bits, T1 to T20), built into the app
(`camcraft_session::library::system`). It is read-only and never written to
`tools.toml`. **Copy to my library**, or dragging a bit onto a folder, copies it
into the person's last folder under their next tool number, to change there.
Up-cut, down-cut and compression are flat end mills, as Fusion has them; the flute
direction is in the name and description. Each bit has a preset for each material
it suits (softwood, hardwood, MDF / plywood, and acrylic, HDPE, aluminum or sign
foam where they fit), its feed from a conservative chip load at 18,000 rev/min
(16,000 for 1/2" shanks and surfacing): a start for a hobby router, not a maker's
chart.

Cutters: flat, ball and bullnose end mills; **V-bits and chamfer mills** (a cone:
an included angle and an optional flat tip), which finish but are refused for
Z-level roughing and for stock to leave; and **drills** (a cone point, 118° unless
given), which only drill.

**Presets** are named sets of speeds and feeds on a tool ("Roughing", "MDF",
whatever the person calls them), each with any of spindle speed, cutting, plunge
and ramp feed, stepdown, stepover and stock to leave. Choosing a tool for an
operation chooses one of its presets too and copies its values in; the operation
then says what has changed since, can be reset to the preset, and can save its
values as a new preset of the job's copy. Beside each speed and feed the editor
shows surface speed and chip load, and typing either sets the spindle speed or
feed. Nothing uses the ramp feed until entries ramp.

A tool says which way its spindle turns (M3, or M4 for a left-hand cutter) and its
coolant (off, flood M8 or mist M7), and the program says so.

**Import…** reads a Fusion tool library exported as CSV, as makers such as
Bits & Bits publish theirs (`fixtures/tools/` holds two), into the folder showing.
Each tool's rows become one tool with a preset per row, in millimetres; columns
are found by their key, not their place. A tool the folder already has (same
product and diameter) gets the file's presets beside its own instead of a twin.
Chamfer mills and spot drills come in as cones, drills as drills; types this build
cannot cut with are skipped, and the manager says which and why.

Not yet: Fusion's own `.json` libraries, which carry holders; tapered ball mills;
the work offset manager.

## Design system

The claude.ai/design project "CamCraft Design System" is copied whole into
`crates/camcraft-gui/design/system/`. `src/ui/theme.rs` mirrors its tokens, and tests hold
them to the CSS; `theme::apply` gives every window its graphite surfaces,
hairlines, 2 px corners, amber selection, and IBM Plex Sans and Fira Code, bundled
with a cut of Lucide for icons (`assets/fonts/`). `src/ui/widgets.rs` has the components
egui lacks: buttons by role, tabs, cards, tags, diagnostics, status dots, empty
states.

## Seeing the real UI without a screen

```sh
cargo test --release -p camcraft-gui --features ui-snapshots -- --ignored
```

writes PNGs of the real UI to `target/ui-snapshots/`: the main window after a run,
setups in tabs, the setup panel, the tool manager, and the lego brick and a full
sheet playing.
