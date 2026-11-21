# 0024 — 2D chamfer and 2D adaptive clearing

**Status:** decided, 2026-11-13 · **Milestone:** 0.8

## Question

2D Chamfer cuts into the part on purpose, which the gouge check refuses; 2D
Adaptive Clearing is adaptive clearing applied to a 2D pocket. How is a
chamfer told from a gouge, and how is 2D adaptive made without a second copy
of the adaptive engine?

## What a new one starts with

- **2D Chamfer** (`chamfer-2d`): Chamfer Width 0 mm, Chamfer Tip Offset 1 mm,
  Chamfer Clearance `tool_diameter * 0.1`, Finishing Overlap 0, Outer Corner
  Mode *roll around corner*, Preserve Order off. No stock to leave, multiple
  depths, retraction policy or stay-down. Lead-in on, its linear distance
  `tool_diameter * 0.1`, perpendicular, no arc; lead-out the same. Shaft and
  Holder offered. It needs a tapered tool. A sharp edge is picked itself, a
  chamfer the model has by its lower edge, with the width and clearance zero.
  The tip offset goes on the depth, and the pass moves sideways by as much as
  keeps the flank on the picked edge.
- **2D Adaptive** (`adaptive-2d`): 3D Adaptive's optimal load
  (`tool_diameter * 0.4`), minimum cutting radius, slot clearing and its width,
  stay-down clearance, lift height (0.2 mm) and helix sizes; Multiple Depths
  off, its stepdown 3D Adaptive's, even stepdowns off; Stock to Leave on at
  0.5 mm; Stay-Down Level *least*; its Bottom from the selected contours, as
  the 2D pocket's is; Detect Open Pockets on. No flat area detection, fine
  stepdown, shallow areas, machine cavities, Shaft and Holder or rest
  machining from operations.

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
top face is another face: with a clearance it lifts, so for one the clearance
is set to none.

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

**Names.** The job file calls them `chamfer-2d` and `adaptive-2d`: beside 3D
Adaptive's `adaptive`, a 2D one is plainer named so, and `chamfer` alone
would not say which it is.

## What it leaves open

Whether a chamfer's clearance should lift the tool, trim the pass or narrow
the chamfer, and against which faces; outer corner modes other than roll; how
open pockets are extended (tangent, to the closest boundary, parallel) and
where they are entered; how pockets of different depths are ordered by depth;
and 2D Adaptive's Stay-Down Level, *least* in a new operation, where 3D
Adaptive starts at the most.

## Amended 2026-11-13

Measured on `ops_2d`, each a new operation:

- **2D Adaptive** of the pocket with the island, the 6 mm flat: 12,035 mm³
  removed, fed 1,544 mm, its half millimetre left on the walls.
- **2D Adaptive** of the notch open to two sides. An open edge is not taken
  as open to air: a program that rapids down beside the part there goes into
  whatever stock stands beside it, so camcraft clears that stock too. With
  the stock flush with the part, 3,478.9 mm³ removed, fed 362 mm. How an open
  pocket should meet stock beyond the part is still open.
- **2D Chamfer** of the pocket's chamfered rim by its lower loop, with no
  clearance, feeds 166 mm, and of the island's sharp rim, one edge a
  millimetre wide, 47 mm. By its upper loop, as a new one starts, 171 mm.
  Each is measured on the pocket cleared first, by a 2D Adaptive leaving
  nothing on its walls: on uncut stock the V ploughs through the pocket's
  stock where it leads onto the rim.

Clearance that lifts the tool, the outer corner modes, open pockets'
extension and entry, and pockets of different depths ordered by depth are
still not measured on a job.
