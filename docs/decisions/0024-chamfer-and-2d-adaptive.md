# 0024 — 2D chamfer and 2D adaptive clearing

**Status:** decided, 2026-11-13 · **Milestone:** 0.8

## Question

Fusion's 2D Chamfer cuts into the part on purpose, which the gouge check
refuses; its 2D Adaptive Clearing is its 3D Adaptive's way of clearing applied
to a 2D pocket. How is a chamfer told from a gouge, and how is 2D adaptive made
without a second copy of the adaptive engine?

## What was found

Autodesk's references ("2D Chamfer reference", "2D Adaptive Clearing
reference", now "2D Adaptive Roughing", and their "Generate a ... toolpath"
pages) list the settings and give no defaults. Every parameter of a fresh
operation of each, read from Dan's Fusion (2706.1.14) through its API, gives
them. The public dump of
[decision 0020](0020-defaults.md) has neither strategy.

- **2D Chamfer** (`chamfer2d`): Chamfer Width 0 mm, Chamfer Tip Offset 1 mm,
  Chamfer Clearance `tool_diameter * 0.1`, Finishing Overlap 0, Outer Corner
  Mode *roll around corner*, Preserve Order off. No stock to leave, multiple
  depths, retraction policy or stay-down. Lead-in on, its linear distance
  `tool_diameter * 0.1`, perpendicular, no arc; lead-out the same. Shaft and
  Holder offered. "A tapered tool is required"; a sharp edge is picked
  itself, a chamfer the model has by its lower edge, and Autodesk says to set
  the width and clearance to zero for one. The tip offset "is added to the
  toolpath depth, while keeping the tool in contact with the selected edge by
  adjusting the toolpath radial offset".
- **2D Adaptive** (`adaptive2d`): 3D Adaptive's optimal load
  (`tool_diameter * 0.4`), minimum cutting radius, slot clearing and its width,
  stay-down clearance, lift height (0.2 mm) and helix sizes; Multiple Depths
  off, its stepdown 3D's Maximum Roughing Stepdown, even stepdowns off; Stock
  to Leave on at 0.5 mm; Stay-Down Level *least*; its Bottom from the selected
  contours; Detect Open Pockets on. No flat area detection, fine stepdown,
  shallow areas, machine cavities, Shaft and Holder or rest machining from
  operations; its rest machining is from a previous tool's size.

Where Autodesk's Generate page says 2D Adaptive's Bottom is the model's bottom,
the API's selected contours is taken, as the 2D pocket's is.

## What was decided

**A chamfer is the flank's line.** Square to an edge, with `d` across into the
part, the chamfer is `z = z_edge + (d - width) * rise`, `rise` the cone's `1 /
tan` of its half-angle: on a sharp edge a chamfer `width` across and as deep at
45°, on a chamfer the model has its own face moved `width` in. The tip goes
`tip_offset` further down the line, so the axis is the tip's radius and
`tip_offset * tan` out from the edge, whatever the width, and the tip `width *
rise + tip_offset` below it. A width the flank does not reach is refused when
the job is checked. Which side the part is, the part says: level with the edge
or above it on one side, lower on the other.

**What is meant is kept, and verified against.** The run already lowered the
part the material is verified against where an operation with negative stock
to leave cut, and only there (`run::cut_past`). A chamfer adds what it means:
each edge's `Bevel`, the flank's line on the part's side of the edge and
within the flank's reach, kept with its result
(`OperationResult::meant`). Where the chamfer cut, between the material before
it and after, the part is lowered to the bevel
(`PartSurface::lowered`), and nowhere else. A chamfer of the width asked is no
gouge; one deeper, or a cut anywhere else, still is: a trace half a
millimetre under a 1 mm chamfer gouges 0.5 mm. A chamfer the model has needs
none of it: its bevel is the model's face, and lowers nothing.

**Clearance lifts.** The tool, grown by the clearance, is dropped onto the
part's faces other than those the chamfered edges join; where it would rest
higher than the chamfer, the pass is lifted to it, the chamfer narrower there,
and said (CC221). A chamfer the model has is picked by its lower edge, so its
top face is another face: with a clearance it lifts, as Fusion says to set none.

**2D adaptive is the engine's, given its levels.** The engine (`adaptive_rough`)
found its levels and where the tool may be at each by slicing the part, then
cleared them. That second half is now its own function, and
`adaptive_clear` hands it levels found another way: each a height and a region.
2D adaptive gives it the 2D pocket's areas (`planar::areas`, its outline offset
in by the tool), each pocket's levels as the pocket's (whole stepdowns, or
even), the pockets with the same levels cleared together by depth, each alone
by area. 3D adaptive's programs are byte for byte what they were.

**Open pockets open.** Where a picked pocket's outline runs along the edge of
the part's shadow, what lies beyond is taken into it, out past the stock far
enough for the tool to stand clear: the engine then comes at it from outside,
as it does beside a block. Only what joins a pocket is taken in.

**Names.** The job file calls them `chamfer-2d` and `adaptive-2d`: Fusion's
`chamfer` is its 3D Chamfer, and a 2D one beside 3D Adaptive's `adaptive` is
plainer named so.

## What it leaves open

Neither is yet held to a Fusion program. To check against Fusion: whether its
chamfer clearance lifts the tool, trims the pass or narrows the chamfer, and
against which faces; its outer corner modes other than roll; how its open
pockets are extended (Tangent, Closest Boundary, Parallel) and where it enters
them; how it orders pockets of different depths by depth; and 2D Adaptive's
Stay-Down Level, *least* in a fresh operation, where camcraft's 3D Adaptive
starts at the most.

## What Fusion's programs showed (2026-11-13)

Fusion's own programs for `ops_2d` (`fixtures/nc/ops_2d_adaptive.nc`,
`ops_2d_adaptive_open.nc`, `ops_2d_chamfer_lower.nc`,
`ops_2d_chamfer_upper.nc`, `ops_2d_chamfer_width.nc`) now hold both, each a new
operation (`compare::tests::ops_2d`):

- **2D Adaptive** of the pocket with the island, the 6 mm flat: 250,599 of
  250,923 columns agree, 12,035 mm³ removed to Fusion's 12,038, fed 1,544 mm
  to its 1,442. The 324 that differ are on the walls each leaves its half
  millimetre on, cut whole or not as a column's centre falls within the
  tolerance of either wall: camcraft's lower in the pocket's round corners,
  Fusion's round the island. They are held to two in a thousand.
- **2D Adaptive** of the notch open to two sides: Fusion takes an open edge as
  open to air, and its program rapids down beside the part there, into
  whatever stock stands beside it, where camcraft clears that stock too. With
  the stock flush with the part, 98,832 of 98,840 columns agree, 3,478.9 mm³
  to 3,479.2, fed 362 mm to 360. How Fusion's open pockets meet stock beyond
  the part is still open.
- **2D Chamfer** of the pocket's chamfered rim by its lower loop, with no
  clearance, and of the island's sharp rim, one edge a millimetre wide: every
  one of 25,143 and 4,425 columns agree, fed 166 to 165 mm and 47 to 46. By its
  upper loop, as a new one starts: all 25,135 agree, fed 171 to 171. Each is
  held on the pocket cleared first, by a 2D Adaptive leaving nothing on its
  walls: on uncut stock, each program's V ploughs through the pocket's stock
  where it leads onto the rim, which is a different place in each, and about
  1.4% of the columns differ there by up to 0.6 mm.

Clearance that lifts the tool, the outer corner modes, open pockets'
extension and entry, and pockets of different depths ordered by depth are
still not compared.
