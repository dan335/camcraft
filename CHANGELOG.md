# Changelog

What changed in each release, newest first. Each release's page
([Releases](https://github.com/dan335/camcraft/releases)) has the downloads and
the same notes at more length.

## 0.11.0 — 2026-11-21

- **Outside in too, Scallop's innermost rings go round a spine**
  ([decision 0040](docs/decisions/0040-scallop-spine-inside-the-last-smooth-ring.md)).
  A new Scallop starts outside in, and there, with nothing avoided, a user's
  bowl's innermost rings still came to sharp points at both ends of its
  floor: 24 of its rings turned more than 30° over a millimetre. Now the
  rings inside the last smooth ring are the ones inside out lays round a
  spine along its middle, cut in to it, the spine last: only the two
  smallest round the spine turn so sharply. It feeds 41.03 m where 40.43,
  lifting once as before. Fusion's `ops_3d`, Scallop's comparisons with
  Fusion, and inside out are as they were.

- **Scallop's blended rings are counted by their real spacing**
  ([decision 0038, amended](docs/decisions/0038-scallop-rings-blend-into-an-edge-at-a-slant.md)).
  A band's count is how far apart its rings would lie at their widest,
  measured on each triangle of the band, and a triangle lying across a
  ridge of the field, where its rings turn a corner, measured the two
  fronts' mean, shorter than either, as width: on a user's bowl cut outside
  in, the band into the floor's edge was counted 11.88 mm wide where it is
  8.48 at most, 12 rings where 8 do. Now each distance's gradient is taken
  as unit, as a distance's is: it feeds 40.13 m where 40.73, and with nothing
  avoided, inside out, 41.44 m where 41.65, the rings no further apart where
  the tool touches than a stepover. Every other band, Fusion's `ops_3d` and
  Scallop's comparisons with Fusion are as they were.

- **From the boundary in, Scallop's rings start where the level rings below
  them end** ([decision 0041](docs/decisions/0041-scallop-hole-rings-after-what-is-inside.md)).
  Cut outside in, the level rings down the wall below where the tool leaves
  the part go first, and the field's rings started where the first of them
  was found: on a user's bowl the tool lifted from the last level ring and
  went 129 mm across the bowl to it. Now the first ring over the part starts
  nearest where the level rings ended, its seam running on from there, and
  the tool steps up onto it staying down: it lifts once, at the end, where
  twice, and goes 0.08 m at rapid where 0.28. Inside out, Fusion's `ops_3d`,
  and Scallop's comparisons with Fusion are as they were.

- **Outside in, Scallop stops at a sharp rim and starts again past it too**
  ([decision 0039](docs/decisions/0039-scallop-zones-at-a-sharp-rim.md)).
  Cut outside in, a user's bowl's rings still crossed its flat rim's two
  edges at a slant and came out in pieces there: 7 rings rolled over each
  edge, and the tool lifted 5 times between the pieces. Now the zones either
  side of each edge are cut from the outermost in, each measured in from its
  outer edges and its rings blended into the one closed edge inside it, the
  strip's border or the floor's edge: one ring touches each edge, and it
  lifts twice, where 7 times, feeding 40.70 m where 39.82, the rings under
  the rim a stepover apart. Fusion's `ops_3d` and Scallop's comparisons with
  Fusion are as they were.

- **From the boundary in, Scallop cuts a region's rings round its holes after
  what is inside it** ([decision 0041](docs/decisions/0041-scallop-hole-rings-after-what-is-inside.md)).
  A region with a hole, measured from both its edges, has a ring toward its
  outside and a ring toward its hole at every level, and each level's two
  were cut together, the nearest next: on a user's bowl, cut outside in, the
  tool went back and forth between the two families, lifting 36 times. Now
  the rings round a region's outside are cut before what is inside them and
  its rings round its holes after, from the middle out the other way: it
  lifts 7 times, feeding 39.82 m where it fed 40.52. Held to Fusion's
  programs, Scallop with the dome avoided lifts 4 times where 3, feeding
  2.9% less, and with only the floor machined 3 times where twice. Pocket
  clearing, flat finishing and inside out are as they were.

- **Scallop's innermost rings go out from a spine, where they came to
  points** ([decision 0040](docs/decisions/0040-scallop-spine-inside-the-last-smooth-ring.md)).
  Inside out with nothing avoided, the rings in a user's bowl were offsets in
  from the rim, and deeper in than the bowl's ends are round they came to
  sharp points at both ends and kept them to the middle: 24 of its rings
  turned more than 30° over a millimetre, the sharpest 162°. Now, going in,
  the last ring with no corner is kept, and inside it the rings go out from
  a spine, a line midway between its two sides, and blend into it: only the
  two smallest rings round the spine turn so sharply, their ends a stepover
  round. It feeds 41.65 m where it fed 40.86, lifting once as before, and
  the simulated stock is left no higher. A round bowl, a pocket with
  corners, outside in, and anything whose band into the last smooth ring is
  refused are as they were.

- **Scallop stops at a sharp rim and starts again past it**
  ([decision 0039](docs/decisions/0039-scallop-zones-at-a-sharp-rim.md)).
  Round a user's bowl, whose flat rim lies between two sharp edges, the
  rings up the inside wall met the rim at a slant, and 11 to 13 of them each
  climbed over an edge onto the rim and dropped back twice a lap: 4 to 9
  crossings at a section did nothing but roll the ball over an edge, its
  contact staying on the edge while its tip rose 3 to 4 mm. Now, inside out,
  where the tool rolls over a sharp convex edge along a strip that runs right
  round, with ground either side, the rings stop where it starts rolling and
  start again where it has rolled over, each of those lines a ring: the rings
  up the wall blend into the first, the rings across the rim start from the
  second, and so on over the outer edge and down the outside. No ring rolls
  over either edge but those two, the tool goes over each once, staying down
  on the seam, and it feeds 41.90 m where it fed 42.87, lifting once at the
  end as before; the simulated stock is left no higher on the wall's top,
  the rim or the outside's top. An edge that runs off the area, or along a
  wall too narrow seen from above, is crossed as before. Where the rim
  breaks the ground into zones whose rings cannot all close, as on a block
  with an off-centre pocket, whose rings from the rim out would end in arcs
  on the block's edge, the ground is cut as one, as it was: the program is
  byte for byte the one before, lifting 3 times.

- **A finish over the part goes across and down to a ring just under the
  last** ([decision 0004, amended](docs/decisions/0004-linking-and-entries.md)).
  Where the next ring starts nearly under the last, down a wall steeper than
  a plunge, the way over the part between them fell at the cutting feed
  further than the cut step, and the tool lifted to the clearance height and
  plunged back onto nearly the same place. It now goes off the ring along
  its lead, across at that height, and straight down at the plunge feed onto
  the next ring's lead, as a profile goes to its ring's next depth. On a
  user's part its scallop lifts 26 times where it lifted 28, and 9 where 12
  without its face avoided, feeding a little less; held to Fusion's programs,
  Scallop lifts once, as Fusion's does, where it lifted 12 times, and 5 times
  where 7 with the dome avoided, Spiral once where twice, and Parallel with
  the dome avoided 9 times where 11; the example rail jig's finish lifts 77
  times where 113. The same material comes off. Going across, what the tool
  would cut is measured against its own shape, so a ball beside a steep wall
  is not held off by a wall its side never reaches (a Contour with a ball,
  the dome avoided, lifts 13 times where 15), and the shank and holder are
  kept clear of the material, as on every other link that stays down: a
  ring's next depth was gone down to without asking.
- **Scallop's rings blend into where the tool leaves the part, where they meet
  it at a slant** ([decision 0038](docs/decisions/0038-scallop-rings-blend-into-an-edge-at-a-slant.md)).
  Down a user's bulging bowl that line runs from Z-32.3 to Z-35.7 round the
  bowl, and the rings, a fixed distance from the floor's edge, met it at a
  slant: for thirteen stepovers before it each was cut into arcs, and below
  it the level rings into pieces, each lifted to and from. Now, inside out,
  the ground between the last ring that goes all the way round and that line
  is filled with closed rings that turn gradually from the one into the
  other, as many as the band's spacing, measured, needs, and below the line
  closed rings go down beside the wall to the first level ring, which starts
  half a stepover under the line's lowest point. Every pass from the floor's
  edge to the Bottom is a closed ring and no gap at the six sections is over
  1.1 mm; it lifts once, at the end, where it lifted 26 times, and feeds
  42.9 m where it fed 39.4, a closed ring being as long however thin the band
  is there. Where the band is wider than twice the tool's diameter, or not a
  ring, the rings are cut by the line as before.
- **The line where the tool leaves the part no longer bobs up and down.**
  Its height there, where the tool's widest circle passes the part's widest
  point, jumped by 0.1 to 0.3 mm from one grid cell to the next with the
  part's facets, 673 times round a user's bowl, though the tool touches the
  same place over that height. It now takes the highest height under that
  which rises and falls no faster than the tool ramps: 29 turns, every 5 mm.
- **Down a steep wall Scallop ramps onto the next ring along it whether the
  ring is level or not**, riding above it, where it lifted; after a ring
  ramped onto, the next starts where the tool is rather than back at the
  seam; and the level rings below the field are ramped onto, the first from
  where the field's last ring ended. On Fusion's `ops_3d` it lifts 3 times
  where 5 with the dome avoided, and twice where 3 with only the floor
  machined.
- Inside out from an inner edge, the line where the tool leaves the part and
  the outer boundary's ring are cut the way the rings before them are; they
  were cut the other way.
- A ring ramped down onto the ring below it is never run backwards: every
  other pair of Scallop's level rings was, climbing the ramp and cutting both
  rings conventional.
- Scallop is less code: one way to read a field at a node, to find the one line of a field at a level, to measure a polyline and to tell which way a ring turns; the spine's part lives in its own module, each step a function; the strip test is split into its pieces, the ground round them and their Euler numbers; and the placing of lines asks the ground itself where a strip is. Every program it writes is unchanged.
- Linking is less code: each way a link is made without lifting (round onto the next pass, over the part, beside it, at a level, across and down) is one function, tried in one order, and what they ask of the part and the material is asked in one place each; every program it writes is unchanged.
- Scallop says when a ring is cut in one place: each line lies on one side of its region, round its outside or a hole, and is cut before what is inside the region where the cut goes on into it; rings that are no region's (a band, the rings below where the tool leaves the part, the rings round a spine) are one sequence each. Every program it writes is unchanged.

## 0.10.0 — 2026-11-20

- **Scallop finishes down to where the tool leaves the part, and up upright
  walls** ([decision 0037](docs/decisions/0037-scallop-to-where-the-tool-leaves-the-part.md)).
  Its field's grid ended a cell short of where the tool leaves the part,
  where the way down falls as a square root, and its area's edge a tolerance
  short of it: down a user's bulging bowl a band round its widest part was
  left with gaps of 2.4 to 3.3 mm between the passes, against 0.97 elsewhere,
  and the ring above it wandered 4.2 mm up and down. Now the grid's cells are
  cut where the ground machined ends, where the tool leaves the part and
  where it leaves a face machined for faces avoided, each found exactly, so
  the rings run right down to that line and the last along it; the level
  rings below start a stepover under its highest point. At the six sections
  measured on that bowl no gap is over 1.1 mm but one of 1.20, where it was
  up to 3.25; it feeds 39.4 m where it fed 38.0, and lifts 28 times where it
  lifted 16: its rings, cut one way, end in arcs on that line, each lower
  down the bulge than the last, and the link from one to the next would
  plunge. Without the floor avoided, cut
  outside in, it feeds 40.1 m where it fed 39.0 and lifts 12 times where it
  lifted 19. Beside an upright wall the tool's path stands upright, and
  where a face avoided meets the wall's foot, as a pocket's floor, the
  wall was left out; the face the tool rises beside now decides, so the
  rings run up such a wall from its foot. The edge of a face avoided is
  found exactly, where it zigzagged by ±6° across a face running smoothly
  into it; with only a pocket's floor machined the Fusion comparison's
  median moved from 0.020 mm to 0.034.
- **A round whose edge stands a little off it reads at 0.01 mm.** STEP import
  is ogeom 0.11.5, which draws the seam it adds round a face with no seam edge
  to the tolerance asked: on 0.11.0 that seam stood 0.09 mm off when the
  face's edge was a file's slop off its surface, and the part was refused.
  Every other part meshes exactly as before. ogeom stays on 0.11: from 0.12 it
  is LGPL-2.1-only, and nothing in a `camcraft` binary is LGPL.
- **Simulating 3D finishing is about ten times faster**: a climbing move's
  samples are swept a row of cells at a time, passing over cells the tool
  cannot come under, leaving the same material bit for bit. Simulating a
  Parallel finish over a pocket and dome took 17.6 s and takes 1.6 s.
- **The tests run faster.** They build camcraft-session, camcraft-import and
  camcraft-post optimised, as the geometry crates already were: two Fusion
  comparisons ran in 165 s where they took 325. Tests that ran the same job
  share one run, and the determinism gate runs three more tests.
- **Cut by area, adaptive clearing comes to each area where the tool last was
  in it** ([decision 0036](docs/decisions/0036-adaptive-levels-order-and-entries.md)):
  a level starts each area nearest where that area's last pass below ended,
  and a pass that follows one of the same area some passes back in the level
  comes from it staying down, rather than lifting. On a user's part the
  adaptive lifts 14 times where it lifted 19, with 1,395 mm of rapids where
  there were 2,119.
- **Adaptive clearing lifts only where it must**
  ([decision 0036](docs/decisions/0036-adaptive-levels-order-and-entries.md)).
  A way back across what a level has cleared stays down wherever it is no
  longer than lifting out, across and down again would be, not only within the
  stay-down distance; from a level up to the next in its slab, the tool rises
  at the feed beside where it is and crosses at the new level, rather than
  lifting out; and a slab's own level starts nearest where the tool is. On a
  user's part the adaptive lifts 19 times where it lifted 57, with 2,119 mm of
  rapids where there were 5,051, removing the same material.
- **3/8" bits in the System library:** an up-cut and a down-cut spiral and a
  ball nose, each on a 3/8" shank, as T21 to T23, so the first twenty keep
  their numbers.
- **Adaptive clearing cuts by area in an order a machinist would**
  ([decision 0036](docs/decisions/0036-adaptive-levels-order-and-entries.md)).
  By area, each area gets each slab's level and then its fine levels up the
  wall from the bottom, then the next slab down, and is finished before the
  next begins: a pass is the area's most of it is in, where a fine level's
  terrace, outside its slab's area, went to whichever area's box was nearest,
  a channel round an island taking the island's. Levels are spaced evenly,
  the fewest no more than the stepdown apart, so the last is never a sliver
  costing a lap; fine levels up to a flat face are spaced evenly to it, so
  none falls a hair below the flat's own level and leaves the face standing
  alone. A pass that would go round on the spot ends, and material narrower
  than the tool is taken in passes along it, a sliver in one: on a user's part a
  third of the program's lines had been a pass going round an octagon a
  quarter of a millimetre across. The helix into a closed area is never made
  smaller than Fusion's Minimum Ramp Diameter, where it went down to a quarter
  of its size; where not even that fits, the area is ramped into back and
  forth along its middle at the ramp angle, rather than left. A fine level
  starts nearest where the level below it ended. On the user's part the
  program is 109,213 lines where it was 169,671, removing the same material,
  with a lift and 185 mm of rapids fewer. Every Fusion comparison of a finish starts
  from a new adaptive's roughing, so their figures were measured again.
- **Scallop cuts every slope, as one family of rings**
  ([decision 0035](docs/decisions/0035-scallop-over-every-slope.md)). Its
  passes no longer leave ground steeper than a plunge to rings round the
  walls: on steep ground each point is found round the wall at the height the
  field puts it at, so the rings stay a stepover apart along the surface up
  and down walls, upright ones included. Below where the tool last rests on
  the part from above, as down a part's upright outside, level rings a
  stepover apart finish it to the Bottom. The edge of a face avoided is an
  edge of the rings, as Fusion's lay them anew round what is avoided, instead
  of being laid across it and cut out of it; inside out, where the ground has
  a hole or a face avoided in it, the rings start along that edge and grow
  outward.
  Each closed ring starts on one straight seam, and the rings are cut in the
  field's order, not reordered for travel, each piece a height or a face
  avoided cuts one into taken from its nearer end. A link from a pass beside
  a wall, below where the tool would rest from above, to the next now stays
  down where the way between is clear of the part, which joins the rings round
  the walls of Parallel, Spiral, Radial and Morphed Spiral too: on `ops_3d`,
  unroughed, they lift 4, 47, 3 and 111 times where they lifted 40, 83, 40 and
  147. Held to Fusion's programs, Scallop's columns alike go from 89.2% to
  94.6% and it feeds 2% further than Fusion's where it fed 16% further, but
  lifts 12 times to Fusion's once, taking the field's regions in its own
  order; with only the floor machined it lifts 3 times, as Fusion's does,
  where it lifted 12. CC211 is no longer raised.
- **The finishes over the part enter and link as Fusion's**
  ([decision 0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)).
  Parallel, Scallop, Spiral, Radial and Morphed Spiral no longer ramp into a
  pass: the tool comes straight down at the plunge feed, onto the vertical
  lead where it is clear of the material, as Fusion's, which have no ramp
  settings, and as Pencil, Project and Morph already did. Spiral, Radial and
  Morphed Spiral cut the nearest pass next, Fusion's *Shortest machining
  distance*, as Parallel does; unticking *Reorder passes* cuts them in the
  order laid out. Where an operation cuts both ways (Parallel always,
  the others with direction both ways), a pass is taken from its nearer end;
  asked to cut one way it is cut as laid. A new Spiral and Morphed Spiral cut
  both ways, as Fusion's start, and a new Flat comes straight down onto each
  ring, as Fusion's Flat starts with its Ramp off; its ramp along the ring is
  a choice on its *Linking* tab. Held to Fusion's programs for `ops_3d`,
  Spiral lifts twice to Fusion's 5 where it lifted 71 times and feeds 30%
  further where it fed four times as far; with the dome avoided, it lifts 8
  times to Fusion's 6 where it lifted 121.
  A job file reads as before: ramp settings on these finishes are not read, a
  Spiral or Morphed Spiral that leaves its direction out climbs, and a Flat
  that leaves its entry out ramps.
- **Faces machined and avoided**, Fusion's Surface Manager, on every 3D
  finish: Parallel, Scallop, Spiral, Radial, Morphed Spiral, Pencil, Project,
  Morph, Contour, Ramp and Flat
  ([decision 0033](docs/decisions/0033-surface-groups.md)). In the job file,
  `[setup.operation.surfaces]` says whether the model is machined or avoided,
  and each `[[setup.operation.surfaces.group]]` is picked faces, machined or
  avoided, with a radial and an axial offset of its own added to the stock to
  leave. In the app, on an operation's Geometry tab, the model is set to
  machine or avoid, and groups of picked faces are each machined or avoided
  with their own radial and axial offsets, tinted in the view. The tool meets
  each group grown by its offsets and rests on whatever is highest; a pass is
  kept where it touches a face machined, and cut out, not lifted over, where it
  touches only faces avoided. A link that would stay down on or beside a face
  avoided lifts. CC111 fails a program that comes within an avoided face's
  clearance, which Fusion does not check. Held to sixteen of Fusion's programs
  for `ops_3d`, eleven of them with groups. camcraft stops where the tool
  touches a face machined and one avoided at once, where Fusion's Parallel,
  Scallop and Spiral run on up the face avoided, and keeps the ring there that
  Fusion's Contour and Ramp cut out; and it lifts where Fusion's Scallop and
  Spiral turn back at the clearance and stay down. Refused: groups on
  roughing, 2D operations and drilling, a face in two groups, the model
  avoided with nothing machined, and a V-bit or chamfer mill where anything is
  avoided. A roughing before such a finish still cuts beside the faces
  avoided. `camcraft post` cannot pick faces. An operation with no groups is
  cut as before.
- **The operating system's own file dialogs** in the app, in place of one drawn
  inside it ([decision 0032](docs/decisions/0032-native-file-dialogs.md)):
  Explorer's on Windows, the desktop's file chooser through the XDG portal on
  Linux, and the open and save panels on macOS, which sort, search and list
  recent files. Each kind of file (jobs, models, machine profiles, tool
  libraries, the folder posted into) starts in the folder it was last picked
  from, remembered between runs; Save As starts at the job's own file, and
  finding a missing model where it was. A job saved under a name without
  `.camcraft` is given it, and the app asks before replacing a file of that
  name, which the dialog did not ask about; a job opened from a file named
  otherwise is saved back to it. On Linux a dialog needs a file chooser portal
  or `zenity`: with neither, none appears, and that looks like Cancel. The
  dialog is not drawn in the app's theme.
- **A boundary or outline set to picked faces and edges starts picking at
  once** when nothing is picked yet, as Fusion's selection does when it is
  chosen. Before, the view waited for **Select in view…** on the Faces or
  Edges card to be pressed, and the operation failed to generate meanwhile.
- **An operation's status line wraps beside its button.** A long reason for a
  failure ran under **Try again** and was cut off.

## 0.9.0 — 2026-11-15

- **Sheets that arrange their parts**, a third kind of stock
  (`kind = "arranged"`) in the job file and the app's stock panel
  ([decision 0031](docs/decisions/0031-arranged-sheets.md)). A sheet of a size
  you give, at a corner of its own, lays its parts out on itself by their true
  shape seen from above, holes and all, and lays them again whenever a part,
  the sheet or a setting changes; where they stand is never written down.
  Spacing and border (10 mm each to start), the turns a part may take (none, a
  half turn, quarter turns or a step), the corner packing starts from, the axis
  it is used up along, and the face the parts stand on. A part can ask for up
  to 1000 copies and have a grain of its own; a smaller part stands in a larger
  one's hole. A model is one body and each copy a turn and a move of it, so a
  face or hole picked once is cut on every copy. Every layout is checked
  exactly before it is used. CC109 fails a sheet that leaves a part out, CC110
  a part standing above or below its sheet, and CC225 says which parts on a
  sheet a setup does not machine; `camcraft job` logs where each copy stands.
  Fifty mixed parts at a 15° step on a 2440 by 1220 sheet are laid in about a
  second on four threads. Parts may stand up to a quarter of the spacing
  further apart than asked; there is one sheet only, so parts with no room are
  reported, not put on another; no part can be placed by hand; and neither a
  work zero nor a Morph's curves can be picked on a part of several copies.
  `examples/sheet.camcraft` lays three copies of one part and two of another on
  a board and cuts them out.
- **Fusion's Morph**, a new 3D finishing strategy (`strategy = "morph"`) in the
  job file and the app's 3D menu
  ([decision 0030](docs/decisions/0030-morph.md)), the last of Fusion's 3-axis
  milling operations: the app's menus list none as planned now. Passes between
  two curves picked on the part, both open or both closed, each a step of the
  way from the one to the other, as many as keep each point of one within a
  stepover of its match on the next, dropped onto the part within the
  boundary. Direction (both ways, one way, other way), the stepover and the
  slopes, as a new Morph of Fusion's starts. Fusion's programs for `ops_3d`
  between the dome's foot and the floor's edge, as a new one starts and one
  way, and between the R8 round's two edges, are `fixtures/nc/ops_3d_morph*.nc`,
  made by `scripts/fusion/ops_3d/cam.py`, and a test holds camcraft's Morph to
  each.
- **Fusion's Project**, a new 3D finishing strategy (`strategy = "project"`) in
  the job file and the app's 3D menu
  ([decision 0029](docs/decisions/0029-project.md)). Edges picked on the part,
  open or closed, each seen from above and dropped onto it, the tool run along
  where it rests there, within the boundary, in the order picked: Fusion's
  Project in its Drop mode. Direction (both ways, one way, other way), Axial
  Offset and the slopes, as a new Project of Fusion's starts. Fusion's
  programs for `ops_3d` along the dome's foot, the floor's edge and the
  round's lower edge, as a new one starts, the other way, a millimetre up
  and with the 12 mm ball, are `fixtures/nc/ops_3d_project*.nc`, made by
  `scripts/fusion/ops_3d/cam.py`, and a test holds camcraft's Project to each.
  A strategy whose picks are the curves it follows can no longer be given
  them as its boundary too.
- **Fusion's Pencil**, a new 3D finishing strategy (`strategy = "pencil"`) in
  the job file and the app's 3D menu
  ([decision 0028](docs/decisions/0028-pencil.md)). The tool run along where
  it touches the part in two places at once: the foot of every wall it
  reaches and every crease between faces that meet sharper than the
  bitangency angle, each point where the tool rests dropped onto the part.
  Direction (both ways, one way, other way), Bitangency Angle, Overthickness
  and Number of Stepovers, as a new Pencil of Fusion's starts. Fusion's
  programs for `ops_3d` with its 12 mm ball, its 6 mm ball, a bitangency angle
  of 40° and two stepovers are `fixtures/nc/ops_3d_pencil*.nc`, made again by
  `scripts/fusion/ops_3d/cam.py`, and a test holds camcraft's Pencil to each.
- **Radial is no longer refused where a spoke stops short of the middle.**
  `camcraft post fixtures/step/ops_3d.step --strategy radial --stepover 0.5`
  ended "the arc to … is not a usable arc from where the path is", and wrote
  nothing. A spoke broken on its way in, and the next one out from the
  middle, lie in line, a micron or two apart by rounding; the link between
  them was made a half turn that wide, which is no arc. Passes in line, no
  further apart than the tolerance, are now joined by the straight line
  between them. No program was written wrong: the operation was refused.
- **Fusion's Manufacturing Extension operations are not planned.** Steep and
  shallow, blend, flow, corner and geodesic are gone from the app's 3D menu,
  where they were listed greyed as not implemented yet, and with hole
  recognition from `PROGRESS.md`'s list, which names them under Not planned.

## 0.8.2 — 2026-11-14

- **Fusion's Ramp**, a new 3D finishing strategy (`strategy = "ramp"`) in the
  job file and the app's 3D menu
  ([decision 0027](docs/decisions/0027-ramp.md)). Contour's rings, gone down
  wall by wall without stopping: each wall entered from the level above its
  top ring along that ring, once round it there but on the first wall,
  then down to each ring below with the tool on the part at every height
  between, a stepdown each time round the longer of the two rings and no
  steeper than the ramp angle, and once round its foot. Direction, flat areas,
  the slopes, the stepdown and the ramp angle, as a new Ramp of Fusion's
  starts; it lifts to the retract height between walls.
- **A part for the 3D finishing operations to come**,
  `fixtures/step/ops_3d.step`, made in Fusion by `scripts/fusion/ops_3d/`:
  steep and drafted walls, a rounded edge, concave fillets, sharp inside
  corners and a dome. Fusion's programs for its Ramp, as a new one starts, with
  a stepdown of 2 mm and with a ramp angle of 0.5°, are
  `fixtures/nc/ops_3d_ramp*.nc`, and a test holds camcraft's Ramp to each.
- **The example job's programs are written again.** `camcraft job
  examples/rail-jig.camcraft --out DIR` refused its roughing since 0.7.0 at
  the latest: "does not read back to its path". The program was right and
  the check was wrong. Where a path holds moves shorter than the check's
  tolerance one after another, as the rings of a pocket do where two corners
  all but meet, the check took each as reached before reading its line, then
  took the lines when they came for the move after. It now takes a line to a
  place the path has just been for what it is. The release's own test of
  each archive now writes the example's programs, which it never did.
- The determinism gate ran one of its six tests: the other five were named
  without their whole paths, and matched nothing. All six ran in the test
  gate all along. The gate now fails unless as many run as it names.
- 0.8.0's operations are less code: what they did each their own way, their levels, leads, order and picks, is shared, and so is their tests' set-up; every program they write is unchanged.

## 0.8.1 — 2026-11-13

- **OpenCascade is gone from the repository.** It was the STEP reader before
  ogeom, kept behind `camcraft-import`'s `opencascade` feature to cross-check
  ogeom against. The feature, its test, `vendor/` (the patched bindings) and
  the `--opencascade` gate are deleted
  ([decision 0001](docs/decisions/0001-step-backend.md)). Nothing in the
  repository is LGPL now, nothing needs cmake or a C++ compiler, and the
  crates no longer carry a dependency patched in by path. No release build
  ever included it.
- **A solid that is ogeom's already loads without a file.**
  `camcraft_import::native::load_shapes` takes shapes held in memory, and
  `load_native` the same written in ogeom's native format, and gives the
  body a STEP file of them would: every face meshed to the tolerance,
  measured against its surface and kept as a face. For programs built on
  ogeom that use camcraft's engine, as
  [PrintCAD-cam-wb](https://github.com/gilbertorconde/PrintCAD-cam-wb) does,
  whose copy of the engine had this first.
- **The app says why the graphics device was lost.** When a driver gives up
  (reset, out of memory, or an error it does not name), the window cannot
  draw again, and the app used to end with a panic about a staging buffer
  that said nothing of the cause. The driver's own words, and the adapter
  they came from, are now written to the terminal first.
- The README's picture of the app is the app as it is now.

## 0.8.0 — 2026-11-13

- **Fusion's other 2D operations**, each a new strategy in the job file and
  the app's 2D menu, and each held by a test to Fusion's own program for the
  same part (`fixtures/step/ops_2d.step`, `ops_engrave.step`,
  `fixtures/nc/ops_2d_*.nc`, `ops_engrave_*.nc`): the two programs leave the
  same material, column by column.
  - **Bore** and **Circular** ([decision 0022](docs/decisions/0022-bore-and-circular.md)):
    holes and bosses picked by their round walls. Bore goes down a helix, by
    pitch or ramp angle; Circular level by level, each pass down its own lead.
    Stepovers, finishing passes, repeat, leads and lead to center, climb or
    conventional.
  - **Slot** and **Trace** ([decision 0023](docs/decisions/0023-slot-and-trace.md)):
    Slot cuts a closed slot along its middle, ramping back and forth along
    its whole length from the ramp clearance height, then once along the
    floor; a tool narrower than the slot leaves the walls, as Fusion's does.
    Trace follows picked edges, on the curve or to its left or right, with
    axial offset, pass extension, maximum angle and multiple depths.
  - **2D Chamfer** and **2D Adaptive Clearing**
    ([decision 0024](docs/decisions/0024-chamfer-and-2d-adaptive.md)):
    2D Chamfer breaks picked edges with a V-bit or chamfer mill, to a width
    asked or along a chamfer the model has, and the checks hold the chamfer
    asked as cut on purpose: anything deeper is still a gouge. 2D Adaptive
    clears a 2D pocket's areas with adaptive's engine, islands and open
    pockets too.
  - **Engrave** ([decision 0025](docs/decisions/0025-engrave.md)): a V-bit's
    flank on a shape's outline, round a wide shape at depth, along the
    middle of a narrow one, and up into its sharp corners.
  - **Thread** ([decision 0026](docs/decisions/0026-thread.md)): a thread
    milled in holes picked by their walls, with a single-profile thread
    mill: down the centre, a helical lead onto the thread, the helix at
    the pitch from the bottom to the top, and back to the centre. Pitch,
    pitch diameter offset, right or left hand, climb or conventional,
    multiple passes. The thread cut into the wall is held by the checks
    as cut on purpose. Fusion's helical lead carries the thread's slope
    below a blind hole's floor, and so does camcraft's: the run says how
    far (CC224) and the part check refuses it until the Bottom is raised
    that much or the leads are levelled.
- **A thread mill** is a kind of tool (`thread-mill`), with its pitch and
  profile angle, in the tool library and editor and read from Fusion's CSV.
- **Operations still to come are in the menus**, greyed out, saying "Not
  implemented yet".
- **[PROGRESS.md](PROGRESS.md)** says where camcraft stands, feature by
  feature, against Fusion's 3-axis milling: done, partial or not started,
  with what each lacks. It replaces `STATUS.md`.
- **A ramp round a whole circle is a helix.** Where a pass was one arc all
  the way round, a tool pocketing a hole not much wider than itself, the ramp
  down onto it went straight down into the material. It now goes round the
  circle as it falls.
- New checks: CC218 (a bore or circular left a core standing), CC219 (a slot
  wider than the tool, its walls left), CC220 (stretches of a trace steeper
  than its maximum angle left out), CC221 (stretches of a 2D chamfer lifted
  to keep its clearance), CC222 (a bore's passes past a hole's middle left
  out), CC223 (an engraving cuts into the part), CC224 (a thread's lead goes
  below a hole's floor).
- **`scripts/fusion/`** gained the scripts that made those parts and
  programs, and `params.py`, which writes out every setting of every Fusion
  milling strategy as a new operation starts with it: what the new
  operations' defaults were taken from.

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
