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
prompt before unsaved work is lost, and undo and redo (⌘Z; ⌘⇧Z or ⌘Y) of whole
edits ([decision 0019](decisions/0019-undo.md)): a field typed into is one step,
what its expressions work out from it is part of it, the Edit menu names the
step (*Undo stepover of rough*), and undoing selects where it was, the tab of
the setting showing. The tool library has its own undo, with Undo and Redo in
its window and the same keys while it is open, so a tool or folder deleted
there comes back. A job's models load in the background; a model not where the job says is asked
about, and can be found elsewhere. Save As keeps relative model paths pointing at
the same files.

The window is the job's tree on the left, the properties panel beside it, the view,
and the transport under the view. The Add menu adds models, stocks, setups and
operations; View opens the log and the checks in their own windows; the Setup menu
posts. Operations are grouped as Fusion's toolbar groups them, in the Add menu and
a setup's right-click menu alike: 2D (face, 2D adaptive clearing, pocket,
profile, slot, trace, bore, circular, engrave, 2D chamfer), 3D
(adaptive clearing, pocket clearing, then parallel, scallop, contour, flat,
spiral, radial and morphed spiral) and drilling. Fusion's operations not built yet
are listed after the rest of their group, greyed, and say *Not implemented yet*
under the pointer: in 2D, thread; in 3D, after the finishing, steep
and shallow, ramp, pencil, project,
morph, blend, flow, corner and geodesic. They are listed in `browser::PLANNED`,
which an operation leaves once it is built.

## The tree

After Fusion's browser: Models, Stocks and Setups, each setup with its operations.
Each setup and each operation has its own status dot (not run, running with
progress, verified, warnings, refused, out of date, turned off). Right-click adds,
duplicates, moves up or down, removes, and turns an operation off or on; a model
not found can be found elsewhere. Operations drag to a new place, in their setup
or another. Selecting a setup shows all its toolpaths, an operation only its
own.

A result goes out of date when anything it was made from changes (the operation,
its tool, or where the part and stock sit), and says so. A changed operation
takes its toolpath with it, and every operation after it, since each cuts what
the ones before left: the run is cut back to the operations before it, still
drawn and ready to be kept, and the playhead comes back to their end. A change
to the part, stock or tolerance takes the whole run. A new setup name, work
offset or post changes no path, so the paths stay; the setup is out of date
until it is generated again, since its checks and program depend on those.

## Properties

The properties panel only edits; what the tree and its menus do is not repeated in
it.

- **Model**: its file, units for an STL, and its transform: position, rotation
  and scale, uniform unless unticked, with a reset back to where the file puts
  it. A model moved is placed again from the one loaded, not read again, and
  shown where it was until it is; a setup waits for it before generating.
  Right-click a model in the tree to duplicate it: another of the same part,
  beside it along X by its width and 10 mm (to the next whole millimetre), in
  every setup and stock that has
  the original.
- **Stock**: laid out as the tool library lays out a tool. At the top is a
  drawing of the block (amber) around the models, from the top and the front,
  with its size under each view and its corners in model coordinates below.
  Then its fields: for a fixed box, one row per axis with the size, where it
  sits against the models and the offset; around models, one row per margin.
- **Setup**, on tabs:
  - *Parts*: cards for the models and stocks it includes, each with a remove
    button. Add lists the job's others; **Select in view…** picks them in the view,
    where the job's other models and stocks show faintly where the setup would
    put them.
  - *Position*: orientation (the 24 square ways, with Turn over) and work zero,
    chosen from a small top view of the box with a height, or picked on the part,
    with its offset folded away.
  - *Post*: work offset, machine profile, and the machine, whose spindle nose
    holds a tool without a holder of its own: one of the built-in machines,
    copied into the job when chosen.
- **Operation**, on tabs: *Tool* (the tool and its feeds and speeds), *Geometry*
  (Fusion's machining boundary: the stock, the part's bounding box or silhouette,
  or picked faces and edges; where the tool goes against it; an additional
  offset. For a 2D profile or pocket, the outline it cuts round or clears (the
  part's silhouette or picked faces and edges) and whether a profile cuts
  outside, inside or on it. New operations start on the
  part's silhouette: parallel with the tool's centre inside it, adaptive and pocket
  clearing reaching in from outside with a channel a tool wide), *Heights* (clearance, top and bottom,
  drawn in the view as planes), *Passes* and *Linking* (retract feed, safe
  distance, stay-down distance, and the lead radius or the ramp angle and feed;
  a profile's level lead arcs on and off each ring, their radius, sweep and
  straight run;
  one left out says what the strategy's default comes to for the tool). A new
  operation starts with Fusion's defaults, as expressions where they follow
  the tool ([decision 0020](decisions/0020-defaults.md)): its stepover (the
  tolerance's cusp with a round end, a quarter of the diameter with a square
  one; 0.6 of it for pockets, 0.3 for flat, adaptive's *Optimal load* 0.4,
  facing's 0.7 of the flat of the end), its stepdown (adaptive's
  `Math.min(tool_fluteLength * 0.75; tool_diameter * 2.5)`, a pocket's
  `Math.min(tool_fluteLength * 0.75; tool_diameter * 0.1)`, a profile's three
  quarters of the flutes), its plunge feed a third of the cutting feed and its
  ramp feed the plunge feed; a tool with no flute length is taken to have two
  diameters of flutes. A number typed over one, or
  a preset's, takes the expression's place; ↺ brings it back. Scallop's stepover starts as parallel's, and its *Passes* has both
  ways, climb or conventional, outside in or inside out, and the slopes it
  finishes between. Contour's has the same directions, by depth or by area, and
  the slopes, and it has *Multiple depths*, Top and Bottom but no stepover;
  flat's climb or conventional and outside in or inside out, with Top and Bottom
  keeping it to the flats between. Spiral's and radial's have a *Centre* (the
  boundary's middle unless given), inner and outer radii and the slopes; a
  spiral climbs or cuts conventional, outside in or inside out, and spokes have
  their angles and go both ways, outward or inward. A morphed spiral's has a
  *Centre* (the point furthest inside unless given), climb or conventional, its
  order and the slopes. Adaptive's *Passes* has a *Minimum cutting radius*,
  *Machine cavities* and, ticked, *Slot clearing* and its width, climb,
  conventional or both ways, by depth or by area, and *Flat areas* with its
  *Minimum stepdown*, and no *Deepest step cut*; under *Multiple depths*,
  *Shallow areas*, ticked, with its minimum stepdown and maximum stepover, and
  *Minimum axial engagement*. Its *Linking* has the *Stay-down level*,
  *Stay-down clearance* and *Lift height*; its *Geometry* has *Rest machining*
  and, ticked, *Ignore stock less than*; pocket clearing's climb or conventional, *Flat areas* and the
  shortest ring worth cutting. A 2D operation's *Passes* has climb
  or conventional; a profile has no stepover. *Multiple depths*, as Fusion's,
  is ticked to step down in levels, then the stepdown: unticked cuts the bottom
  in one pass. Profiles and pockets start ticked at 2 mm, facing unticked, and
  pocket and adaptive clearing always cut in levels, so cannot be unticked; parallel,
  bore and drilling have none (a drill has its pecks, a bore its helix). A bore's
  *Passes* has *Use ramp angle*, ticked, and the helix's *Angle* or, unticked,
  its *Pitch*; a bore's and a circular's have climb or conventional, *Multiple
  passes*, ticked, with the *Number of stepovers* and their stepover, and
  *Repeat passes*. A circular starts with *Multiple depths* ticked at a
  millimetre, with *Even stepdowns* under it; a bore's *Finishing passes* has
  only its stepover. Their *Linking* has the leads, with *Lead to center* under
  them, and neither a ramp nor a stay-down distance: each pass comes down its
  lead and lifts out after, as Fusion's do. A slot's *Passes* has *Backoff distance*, *Repeat finishing pass* and
  *Preserve order*; it has *Multiple depths* (a millimetre when ticked, as
  Fusion's Maximum Stepdown) and only a floor's stock to leave; its *Linking* has
  the ramp angle and feed, the *Maximum ramp stepdown*, how far its ramp back
  and forth along the slot goes down there and back (half each leg; a tenth of
  the tool without levels), and the *Ramp clearance height* it starts at over
  the Top. A tool narrower than the slot leaves its walls, as Fusion's does.
  A trace's *Passes* has *Sideways
  compensation* (on the curve, left or right), *Axial offset*, *Pass extension*,
  *Maximum angle*, *Repeat passes* and *Preserve order*; under *Multiple depths*,
  the *Number of stepdowns*, by depth or by curve, and *Both ways*. Its
  *Linking* has *Keep tool down*, with the stay-down distance under it, and
  *Lead-in* and *Lead-out*, with the lead radius under them; its *Heights*
  only the clearance and retract heights. A 2D chamfer's *Passes* has *Chamfer
  width*, *Chamfer tip offset*, *Chamfer clearance*, *Finishing overlap* and
  *Preserve order*, and no stock to leave or multiple depths; its *Heights* only
  the clearance and retract heights, and its *Linking* nothing of its own. An
  engraving's *Passes* has *Depth*, unset to start, 0.95 of how deep the
  tool's flank reaches, and *Sharp corner angle*, then *Multiple depths*
  (a millimetre when ticked) and *Smoothing*; no stock to leave, feed
  optimization or shaft and holder. Its *Heights* are only the clearance and
  retract heights, and its *Linking* nothing of its own: it lifts between
  shapes. 2D
  adaptive's *Passes* is 3D adaptive's less what a 2D pocket has no use for:
  *Minimum cutting radius*, *Slot clearing* and its width, climb, conventional
  or both ways, by depth or by area, the *Optimal load*, and *Multiple depths*,
  unticked to start, with *Even stepdowns* under it; its *Linking* has the
  *Stay-down level*, *Stay-down clearance* and *Lift height*, and its *Geometry*
  a 2D pocket's outline. *Stock to
  leave*, as Fusion's, is ticked to leave any (starting at 0.5 mm), then its
  wall and floor amounts, negative to cut past the part; the floor follows the
  wall until it is given its own. A drill has none, and a profile on the
  boundary only a floor. Every strategy but drilling, bore and circular, whose
  moves are arcs already, has *Smoothing* at the
  foot of *Passes*, ticked to fit arcs and longer lines, then its tolerance,
  the job's unless given; and below it, all but adaptive, *Feed optimization*,
  ticked to slow down in corners, then the most a cut may turn, the reduced
  feed's radius, distance and feed, each saying its default, and whether only
  inner corners are slowed. Above *Smoothing*, every 3D strategy and drilling
  has *Shaft and holder*: off, pull away (finishes over the part only), trim
  (*skip holes* for drilling) or fail, then the shaft and holder clearances.
  Profiles can leave tabs, from *Passes*: shape,
  width, height and spacing. A setting that only counts while another is on
  hangs under it, indented with a guide down its left. Any number of an
  operation's can be worked out from an expression instead, as Fusion's:
  *ƒx* beside it starts one at its value, typed in place as
  `tool_diameter * 0.4` ([jobs](jobs.md#expressions)); the field then shows
  what it comes to, an expression that cannot be worked out is red with why on
  hover, and × drops it, keeping the number. Which rows a
  strategy shows is the core's to say (`StrategySpec::uses`), as is which
  settings it reads. The panel's foot says where generating the operation
  stands, with Stop while it is generated and Generate again where it was
  stopped or failed. The strategy is fixed once the operation is made.

## Help on hover

As in Fusion, hovering any field, its label or the field itself, pops up its
name, a sentence or two on what it does, and for many a small sketch with the
dimension it sets in amber: stepover between two passes, the four heights through
the stock, a tool's stickout. It covers every panel, the tool library, settings
and playback's stops. The text lives in `src/help/topics/`, one file per panel,
with the rules for writing it; the sketches are painted, not pictures, in
`src/help/figures/`. Every form row goes through `widgets::row` or
`widgets::field_label`, which take a topic, so a field cannot be added without
its help. Tests hold each topic to 160 characters and each sketch inside its
frame; the `help_popups` snapshot lays them all out to look over.

## Picking in the view

Every model has faces, edges and corners: the file's own B-rep faces for STEP,
faces found from the triangles for STL (`camcraft_geom::topology`). As in Fusion,
the view picks only while a slot is armed by its button in the panel (**Select in
view…**); otherwise a click does nothing. Hovering lights what a click would pick,
and a slot being picked into brings its tab forward. Choosing another tab stops
picking. Legends under the picks name what the view draws.

- The work zero takes a corner, a round edge's centre or an edge's middle on a
  model, or a corner, edge or side of the stock.
- An operation whose boundary is a selection takes faces and edges, which keep
  parallel finishing and pocket clearing to their outline seen from above.
  Edges are only ever cut round in loops, so a click takes the whole loop the
  edge goes round: the edges joined to it end to end at its height, taking at
  each corner the one that turns least. An edge of a pocket's floor or of the
  part's outline takes all of it, however its faces are split (a chamfer in
  strips still gives the outline), and a hole's rim is its own loop. An edge
  that is not level is taken alone. A click on a loop all picked takes it out.
  The panel lists each loop once. Picks and boundaries the part hides are drawn
  faintly through it.

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
  bottom rims. Each hole is drilled down to its own bottom, where its round wall
  ends, as Fusion's drilling goes to the hole's bottom: a blind hole to its
  floor, a through hole to the part's far side. A picked bottom rim takes its
  hole there instead, and a Bottom height set takes every hole there.
- A bore or a circular takes faces, as Fusion's: the round wall of each hole or
  boss, which it cuts from the wall's top down to its bottom, inside a hole and
  round the outside of a boss. The faces of one wall, as a file often splits a
  cylinder into halves, are taken together: one is enough where it runs the
  wall's whole height. Its *Geometry* has no edges to pick.
- A slot takes the edges round each slot's floor, a click taking the whole
  loop as for a boundary, or its floor as a face. Each slot is cut from the Top
  down to its own floor.
- A trace takes edges, open or closed, as Fusion's chain selection does: a click
  takes the edge and the edges running on from it without a corner, either way,
  so a hole's rim in halves, or an outline rounded all the way round, comes
  whole, and a square outline a side at a time. The panel lists each chain once.
  Picked edges that meet end to end are followed as one curve.
- A 2D chamfer takes edges or faces. A click on an edge takes the loop it lies
  in, as for a boundary, or where it closes none the edges running on smoothly
  from it, as for a trace; a face's every edge is chamfered, its outline and the
  rim of every hole in it, and is drawn so.
- An engraving takes the shapes it engraves: the edges round each one's rim, a
  click taking the whole loop as for a boundary, or its floor as a face, whose
  edges are its outline and are drawn so. A loop inside another, an island's
  rim, is part of the shape's outline.
- A 2D profile, pocket or 2D adaptive takes faces and edges as what it cuts
  round or clears.
  The view draws that boundary (wide), each piece of it at the height of what is
  picked in it so it lies on the picks, or at the stock's top with nothing
  picked, and, hatched, what the tool cuts away: a band beside it for a profile,
  outside, inside or on it, all of it for a pocket.

## Running and checking

A setup is generated by itself, off the window's thread, exactly as `camcraft
job` runs it, from the job as it is on screen, saved or not: as soon as its
models load, and again as soon as editing it settles, a moment after the last
change, or longer while a field is still being typed in. What still stands is
kept: every operation made before and unchanged since, up to the first changed
one, with the material it left. That one and every one after it are made
again, since each cuts what the ones before left; but one unchanged itself
only links again, over the new material, the passes it found from the part,
kept by what they were found from, so an edit undone or an operation moved
finds them too ([decision 0017](decisions/0017-what-a-run-keeps.md)). Every
open job's setups are generated at once.

Within a setup, every operation's passes that are worked out from the part alone
are worked out at once, each on a thread of its own; pocket clearing, which
clears what the operations before it left, waits for them. Each is then linked
over the material and cut into it in order, so the result is the same as one
after another. Generating takes every core but one, which keeps the window
drawing.

While it runs, the tree says how far the setup has got, and each operation a dot
that fills as it is worked out, how far it is through and for how long; one
waiting for the operations before it says so. Each operation's passes are drawn
as they are found, in a quieter green than a cut, and the operation itself as
soon as it is made, before the rest are; the playhead stays where it was. The
operation's panel says what it is doing: working out its passes, or linking them
and cutting them into the material.

A change to a setup that is being generated stops its run from the first
operation the change touches: the ones before it go on and are kept, and the
rest are generated again once editing settles. **Stop** (on the operation's
panel, the setup's menu or the Setup menu) stops it outright; what it made
stands, and it is generated again once it changes, or with **Generate again**.
A run that fails marks the operation it failed at and is not tried again until
the setup changes. Generating by itself can be turned off in Settings; then a
setup is generated only with Generate again.

The checks window lists each setup's findings by operation, under a HALT banner
when anything fails; a click on one at a move switches to Playback, takes the
playhead there and flies the camera there. **Steep descents** are listed deepest first
in the Show panel, and work the same way.

## Posting

**Post {setup}…** and **Post all…** write the same programs, names and folder
layout as `camcraft job --out`, all setups or none, from results that are up to
date. They ask before replacing files, and a setup that failed a check is posted
only after ticking "I know why" and pressing Post anyway.

## The view

The view has two modes for each setup, switched by the **Setup** and **Playback**
tabs on the bar under it. Each setup keeps its own mode, camera and playhead.

**The view cube**, in its top right corner, after Fusion's, turns the view. Click
the middle of a face to look straight at it (Top, Bottom, Front, Back, Left,
Right), a side of one for the edge between two faces, or a corner for the
corner's isometric view; the view turns there in a fifth of a second. Drag the
cube to orbit. The house beside it, or R, turns back to the first view. Looking
straight at a face the view is **orthographic**, a true drawing with nothing
foreshortened; from anywhere else, **perspective**. Orbiting away from a
straight view is perspective again at once. Straight down, +Y is up the screen
and the front at the bottom. Picking works the same in both.

**Setup** is for setting the job up: the part, the stock's outline, and the
selection's toolpaths in full, coloured by move role (an operation's alone, or
the whole setup's), as screen-space lines that stay readable on Retina. There is
no cutter. An operation's picked geometry shows only on its Geometry tab, and
its height planes only on its Heights tab.

**Part or cut stock**, at the top of the Show panel while setting up (or M),
puts the stock as the selected operation leaves it cut in place of the part,
its outline and work zero; with no operation selected, as the whole setup
leaves it. It follows each edit as the setup is generated again, so a change to
an operation is seen in what it cuts, not only in its toolpaths. Each end of an
operation is kept from the run, so it is put back, not cut again. The
toolpaths over it are off until asked for, as in Playback. Until the new run
comes in, the last one's stock stays in view rather than the view going blank.

**Playback** is for watching the program cut: the material as it stands at the
playhead, and the job's own cutter there, hidden by the material and part in front
of it. The toolpaths (as far as the playhead) and the part are off until asked
for, with the toolpaths button on the bar or the Show panel, which also has
toggles by role and by operation. Playback needs a run that fits the setup as it
is; without one the tab says it plays back once it is generated, and an
out-of-date run drops the view back to Setup.

Part and material are lit by one rig, dcpwood's viewers' turned to Z up: a warm
sky-to-ground hemisphere, a warm key from high above the front left of where the
camera starts (so tops read lighter than walls), a faint cool fill and rim, and
the room's even light, tone mapped as three.js's Neutral
(`view/viewport/shaders/common.wgsl`). Surfaces are shaded as three.js's standard
material, with no textures: GGX highlights off the three lights, and Karis's
analytic fit for what the room reflects. Each has a finish, a roughness and a
metalness (`theme::Finish`): the part satin (0.62), the material a little smoother
(0.5), and what is drawn to point something out matte. The part is shaded with half-resolution
GTAO, the material with horizon ambient occlusion on its own heights, shaded as it
is cut. *Compare* colours the material
by the verifier's own rule: red below the part, amber left above it (deeper amber
for more), grey finished, dark where there is no part. The legend's counts are the
verifier's counts, and a test holds them equal.

## Playback

Playing back, the bar under the view has the timeline (one span per operation, as long as
it takes to run, failed checks marked red, warnings and steep descents amber; click
or drag to go anywhere), start, previous operation, step back, play backwards,
play, step on, next operation and end, the estimated machining clock, the speed
(¼× to 10,000×) and where playing stops by itself (at problems, on by default; at
the end of each operation) and the toolpaths button. Under it, the operation and
the move under way. Going to the end of an operation, or of the program, is
immediate: the stock there is kept from the run.

Keys, when no field has the keyboard, while playing back: Space plays and pauses
(and in Setup, switches to Playback and plays); J, K and L shuttle
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
left out reads "not given". A holder is segments from its face up, each a
cylinder or a taper (*Ø* to *to Ø*); **Standard…** puts one of the built-in
holders on the tool, to change there. The drawing is the whole tool as it is
held, and so is playback's.

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
pocket and adaptive clearing and for stock to leave; and **drills** (a cone point, 118° unless
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

Not yet: Fusion's own `.json` libraries, which carry holders; a library of
the person's own holders and machines; tapered ball mills;
the work offset manager.

## Themes

Settings' **Theme** chooses the window's colours. *System*, the default,
follows the desktop's theme on Omarchy: its `colors.toml` is read as the app
starts and watched, and the window changes with it as soon as the theme is
switched, a light theme light. Elsewhere *System* is the design system's,
dark, as *camcraft* is everywhere. Then come the 23 themes Omarchy ships,
bundled in the app (`assets/themes/`, as Omarchy has them). Last are your
own: palette files in Omarchy's `colors.toml` format, kept in camcraft's
themes folder (**Themes folder** opens it), a `name.toml`, or an Omarchy
theme's folder dropped in as it is. The one chosen is watched too.
**View → Theme** lists the same. The surfaces, hairlines, text and accent are
the theme's; what says something keeps the design system's colours: the
moves by role, the axes, picks and boundaries in the view, the material, and
ok, warning and error. A light variant of the design system, for *System* to
follow the OS's light or dark, is planned
([decision 0018](decisions/0018-themes.md)).

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
