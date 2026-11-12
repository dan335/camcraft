# Changelog

What changed in each release, newest first. Each release's page
([Releases](https://github.com/dan335/camcraft/releases)) has the downloads and
the same notes at more length.

## 0.7.0 — 2026-11-11

- **A Theme setting**, in Settings and **View → Theme**: *System* (the
  desktop's theme on Omarchy, as before), *camcraft*, the 23 themes Omarchy
  ships, light and dark, bundled, or any palette of your own in Omarchy's
  `colors.toml` format dropped into camcraft's themes folder, an Omarchy
  theme's folder as it is ([decision 0018](docs/decisions/0018-themes.md)).
  A palette of your own is watched, so editing it shows at once.
- **Generating is about twice as fast** on a large adaptive job: the serving
  tray from 198 s to 93 s, the regression job from 152 s to 96 s, and Shaft
  and Holder's avoidance from 135 s more to under 10. Adaptive grew the
  material round the tool by its radius before each pass, a jagged outline
  folding over itself many times; it is simplified first now, to the
  tolerance passes start to. Adaptive's steps read only the edges near them,
  its levels' forbidden regions are worked out together, the holder's are
  grown as loosely as its clearance allows, and the drop-cutter tries the
  highest triangles first and stops once the rest are lower.
- A lone cell of the simulated material standing into the shank, a crumb
  narrower than the grid can resolve, no longer fails the shank check.
- **A blind hole is drilled to its floor.** A hole picked by its top rim alone
  went down to the operation's Bottom height, which starts at the part's
  bottom, so a blind hole was drilled through the part (and refused by the
  material check, CC107). Each hole now goes to its own bottom, where its round
  wall ends, as Fusion's drilling goes "from hole bottom"; a Bottom height set
  still takes every hole there.
- **New 2D pockets and contours start as Fusion's do**, found by holding them
  to Fusion's own programs ([decision 0020](docs/decisions/0020-defaults.md),
  amended): one pass to the bottom, Multiple Depths off (a new 2D pocket
  stepped down 0.6 mm a level, taking four times as long as Fusion's); a 2D
  pocket leaving 0.5 mm each way, as Fusion's roughing; and a 2D contour
  straight down onto its lead, Ramp off, where it helixed down at 2°. These
  are how new operations start; saved jobs keep theirs. Face's pass extension
  is now to the tool's centre, as Fusion's, so passes end where Fusion's do: a
  saved job's runs a radius shorter at each end, and one that leaves it out
  stops just clear, as before.
- **`camcraft compare`** holds camcraft's programs to another CAM system's:
  it runs a setup, cuts each other program from the material camcraft had
  before that operation, with the same tool, and compares what the two
  programs do and the material each leaves, column by column and against the
  part. A test holds new Face, 2D Contour, 2D Pocket and Drill operations to
  Fusion's own programs for the same part, made in Fusion
  (`fixtures/step/ops_block.step`, `fixtures/nc/ops_block_*.nc`): each leaves
  the same material to 0.01 mm. The other program is read as Masso, Haas and
  Centroid read one, the motion before a drilling cycle in force again after
  `G80`, as Fusion's posts write; camcraft's own are still held to
  RS274/NGC's stricter reading, so they run on either.
- **`scripts/fusion/`** drives Fusion through its API: an add-in runs the
  scripts dropped into a folder, on this machine or over SSH on another, and
  the scripts kept there build the test part, set up Fusion's operations and
  post them, remaking those fixtures byte for byte.

## 0.6.0 — 2026-11-05

- **The whole tool is checked**, not only what cuts
  ([decision 0021](docs/decisions/0021-shank-and-holder.md)): its neck,
  shank, and holder, or the spindle's nose where it has none, are swept
  through the part and stock with every move, and meeting them fails the
  checks (CC108) with how far out of the holder the tool would have to be. A
  setup names its machine (an 80 mm ER20 spindle, or an ISO30, BT30 or ISO20
  ATC spindle); a holder's segments can taper, and **Standard…** puts an ER
  nut, a BT30 holder or an ISO30 ER16 to ER32 holder on a tool. Playback and
  the tool's drawing show the whole of it.
- **Shaft and holder**, as Fusion's, on each 3D operation and drilling:
  off, as before; pull away or trim, finishes lifted or cut out where the
  rest of the tool would meet the part or stock, levels kept to where it
  clears, links lifted over the material by it, and holes it cannot reach
  not drilled; or fail.
- **New operations start with Fusion's defaults**
  ([decision 0020](docs/decisions/0020-defaults.md)), as expressions where
  they follow the tool: pockets step down
  `Math.min(tool_fluteLength * 0.75; tool_diameter * 0.1)` and profiles three
  quarters of the flutes, flat steps over 0.3 of the tool and facing 0.7 of
  its flat with passes running half a tool past the stock, adaptive blends
  corners tighter than a tenth of the tool and lifts 0.2 mm between passes,
  plunges are a third of the cutting feed and ramps the plunge feed, and the
  clearance height is 10 mm above the retract. Feed optimisation slows at
  25°, within a twentieth of the tool, from a fifth of it before, and a 2D
  pocket's helix is Fusion's size: both for saved jobs too, where they leave
  them out. Otherwise a saved job reads as it did.
- **Undo, made whole** ([decision 0019](docs/decisions/0019-undo.md)): what an
  edit's expressions work out undoes with it, where undoing a tool's diameter
  used to seem to do nothing; each field typed into is its own step; the Edit
  menu says what it undoes (*Undo stepover of rough*), and undoing shows where,
  the operation selected and the setting's tab open; Ctrl+Y redoes; and the
  tool library has undo, so a tool or folder deleted there comes back.
- **Stepovers and stepdowns start as expressions**, as Fusion's defaults do: a
  new operation's follows its tool, and a number typed over one, or a preset's,
  takes its place; ↺ brings the expression back.
- **Generating starts when a value is entered**, without the mouse having to
  move.
- **No pause after each run on a long program**: the replay the cut stock and
  playback are drawn from is built off the window's thread, from toolpaths now
  shared rather than copied.
- **Pocket clearing cuts rest material on walls at every level**: material
  an earlier operation left thicker than this one's stock to leave was only
  cut at the bottom level, where the shank then rubbed it. **Adaptive leaves
  walls to the tolerance**, not twice it.
- Plainer hints in the Show panel and on expressions.

## 0.5.0 — 2026-11-05

- **The cut stock while setting up.** Part or cut stock at the top of the Show
  panel, or M, shows the stock as the selected operation leaves it, or the
  whole setup with none selected, and follows each edit as it is generated.
- **Omarchy's theme, followed**: on Omarchy the window is drawn in the
  desktop's theme, light or dark, and changes with it as soon as it is
  switched. Moves, picks, the material and ok, warning and error keep their
  colours ([decision 0018](docs/decisions/0018-themes.md), which plans the
  rest: a light theme, following the system's light or dark, palette files).
- **Expressions**, as Fusion's: any number of an operation's can be worked
  out from the tool and its other settings, `tool_diameter * 0.4` or
  `Math.min(tool_fluteLength * 0.75; tool_diameter * 2.5)`, with ƒx beside it,
  and follows a new tool. Saved in the job file beside the number.
- **Times in the tree**: each operation's estimated machining time beside it,
  and the setup's total beside the setup, as Fusion has them. The table of
  passes and volumes under the tree is a one-line verdict now.
- **Adaptive's starting values are Fusion's, from the tool**: an optimal load
  of `tool_diameter * 0.4`, and for adaptive and pocket clearing a stepdown of
  `Math.min(tool_fluteLength * 0.75; tool_diameter * 2.5)`, set as the tool is
  chosen, as the stepover already was.
- **A setting that only counts while another is on hangs under it**, indented
  with a guide down its left: slot clearing under machine cavities, the
  minimum stepdown under flat areas, the shallow area settings, a helix's
  diameter, leads, reversing under reordering, and each ticked group's rows.
- **Adaptive clearing gets Fusion's rest machining**, from the operations before
  it in the setup, each swept exactly with the tool it was cut with, and its
  *Ignore stock less than*.
- **And its minimum cutting radius, machine cavities and order by area.** A
  minimum cutting radius blends every sharper inside corner, leaving it for a
  smaller tool; machine cavities unticked leaves the pockets it could only go
  down into; by area clears each pocket all the way down before the next.
- **Adaptive stays down round what stands between passes**, through what it has
  cleared, rather than lifting wherever the way straight is blocked.
- **Adaptive has no Deepest step cut**, as Fusion's has none: its passes stay on
  their levels.
- **Better help for adaptive's Passes tab**: sketches for fine stepdown, flat
  areas, deepest step cut, both ways, minimum cutting radius and machine
  cavities.
- **Adaptive machines shallow areas**, as Fusion's: where the levels leave a
  terrace wider than the most asked, levels are added between, cutting only
  there. On a serving tray the scallop finish after it takes 15% less.
- **And the rest of Fusion's adaptive settings**: flat area detection finds
  peaks too, with a minimum stepdown, and a minimum axial engagement leaves out
  levels that would only skim; slot clearing cuts a slot along a pocket's middle
  before clearing out from it; and on the Linking tab, the stay-down level,
  stay-down clearance and lift height.
- **Smoothing**, as Fusion's, on every operation but drilling: runs of short
  moves fitted with arcs and longer lines within a tolerance, before linking,
  so what is simulated and checked is the program. On a serving tray the
  adaptive program went from 34 MB to 6 MB, and 1.3 million lines to 181,000.
- **Smoothing fits helices**, about Z: a ramp round a ring is a few arcs
  falling as they turn, rather than lines.
- **Feed optimisation**, as Fusion's, on every operation but drilling and
  adaptive clearing: cuts slowed into and out of sharp corners and round tight
  arcs, only those turning into the material unless asked.
- **Generating keeps more of what it worked out.** An operation unchanged
  itself after one that changed links its passes again over the new material
  rather than finding them again; passes are kept by what they were found
  from, so an edit undone or an operation moved finds them too; a kept
  operation's material is put back rather than cut again; and a run takes the
  setup as the window laid it out, with the part's index and surface built
  once. On a serving tray, a finish's edit undone takes 6 s where it took 21.
- **Every program is read back before it is written**, as a controller would
  read it, and refused if it does not cut the path it was posted from. Until
  now only the tests did this.
- **The stock's underside is no longer shaded** by what is cut in its top, in
  playback seen from below.
- **Renaming or removing a model follows fixed-size stocks too**: a stock of a
  fixed size placed against a model kept the old name.
- **A failure in ordering, smoothing or feed optimisation names the operation**
  it happened in.
- **A link staying down keeps clear of the material** between the passes it
  joins, where it could come a tenth of the tool into it.

## 0.4.0 — 2026-11-04

- **Toolpaths show as they are worked out.** Each operation's passes are drawn as
  its strategy finds them, in a quieter green than a cut, until the operation is
  made and its real toolpath replaces them.
- **Every 3D finish finishes the walls.** Parallel, spiral, radial and morphed
  spiral leave ground steeper than a plunge to rings round the walls, as scallop
  does ([decision 0014](docs/decisions/0014-walls-for-every-finish-over-the-part.md)).
  On a serving tray each now leaves 11 columns more than 1 mm above the part,
  where it left between 1,263 and 7,690. A slope range that stops short of
  vertical turns it off. The rings have a level at each flat face, so a floor
  beside a wall is finished too: the router jig's finish leaves 7,898 columns
  more than 1 mm above the part, the tallest 2.5 mm, where 0.3.0 left 11,806
  and the tallest 52 mm.
- **Roughing large parts is faster.** Adaptive keeps the material grown by the
  tool tile by tile: on the tray the slowest level went from 190 s to 80 s, and
  the roughing from 246 s to 153 s.
- **Fixed:** a pass that measured itself engaged while taking nothing could run
  to its level's step limit and leave the rest of the level uncut; it now ends
  once it takes less than a sliver over a tool's width.
- **Fixed:** a stay-down link could climb a wall from a ring round its foot,
  where nothing is under the tool, through a convex corner of the part.

## 0.3.0 — 2026-11-03

- **Toolpaths generate by themselves** as editing settles, keeping what still
  stands; operations that read the part alone are worked out at once; a change
  stops only the operations it touches; Stop, and a setting to turn it off
  ([decision 0013](docs/decisions/0013-generating-by-itself.md)).
- **Each operation shows as it is made,** with how far each has got in the tree
  and the operation's panel.
- **A curved part finished as Fusion does it:** adaptive's fine stepdown; a Top
  and Bottom on every 3D finish; finishing ramped into; scallop leaving its steep
  ground to rings round the walls; contour keeping rings along a boundary; a
  short open pass ramped into back and forth. A serving tray's stock now matches
  Fusion's to 0.03 mm at the 99th percentile.
- **The app:** no freeze adding a 3D operation to a large part, or on Save As;
  every input shows and resets its default; typing `.0` no longer completes
  early; a tool import says in full what it skipped.

## 0.2.0 — 2026-10-14

The first release: the desktop app and command line, STEP and STL import,
adaptive and pocket roughing, parallel, scallop, contour, flat, spiral, radial
and morphed spiral finishing, 2D profiles, pockets, facing and drilling, tool
libraries with presets, several setups a job, simulation of every program
against the stock, playback, and posts for Masso, Centroid and GRBL.
