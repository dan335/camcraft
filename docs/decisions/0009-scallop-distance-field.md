# 0009 — Scallop finishing, by a field of distances

**Status:** decided, 2026-09-28 · **Milestone:** before the first cut

## Question

Parallel finishing lays its passes a stepover apart seen from above. On a slope
they are further apart along the surface, and the cusps between them taller, the
steeper it is. Fusion's Scallop "creates passes that are at a constant distance
from one another by offsetting them inwards along the surface", from the
boundary, so the cusp is the same everywhere. Offsetting along a surface has no
exact form for a triangle mesh under a round tool. How is it done, and how does
it square with decision 0003, that grids are only for stock?

## Decision

- **A field of distances, sampled from the part.** The cutter location (where
  the tool's tip sits over the part) is sampled on a grid by exact drop-cutter
  queries: a third of a stepover apart, and at most half the tool's radius. The
  distance along that surface from the boundary is worked out across the grid,
  and the passes are the lines where it is a whole number of stepovers.
- **An exception to 0003, and only this one.** The grid decides where a pass
  runs, seen from above, and which stretches fall in the slope range. It never
  gives a Z, and it is not the stock.
  - The first pass is the boundary itself: the machining boundary, and where the
    tool stops touching the part, the mesh's shadow grown by the tool.
  - Every point of every pass is an exact drop-cutter query, refined along the
    pass until it is within tolerance of the part, as a parallel pass is. It is
    broken where a parallel pass is: off the part, at a wall, down a plunge.
  - Nothing is smoothed. Lines are placed by interpolating along each triangle's
    edges, never at an edge's midpoint, which was 0003's staircase. The only
    change to a line is thinning it where a point is within a quarter of the
    tolerance of the line through its neighbours, seen from above.
- **Fast marching, not Dijkstra.** Dijkstra measures along the grid's edges, so
  the distance from a point grows as a polygon (sixteen-sided with sixteen
  neighbours). Every ring would have kinks at its corners and be spaced closer
  across some directions than others. Fast marching on the grid's triangles, each
  cell split on its better diagonal and lifted to its height (Kimmel and Sethian),
  carries a plane front exactly and a curved one to first order. It is exact to
  nanometres on a flat or sloping plane, and within 1.5 pitches of a point
  source, falling with the pitch.
- **Seeds at their exact distance.** Each node within a cell's diagonal of the
  boundary starts at its straight 3D distance to the nearest point of it, over
  the part, and the march may still lower it.
- **Regions, nested, and top-ups.** The regions each level bounds are found by one
  union–find sweep down the nodes, so annuli and islands nest correctly. Passes
  are cut region by region, nearest next: from the boundary in, or each region's
  inner ones first. Where a region rises more than half a stepover past its last
  pass, it gets one more halfway up. Without that the cusp along a crest could be
  four times as tall; with it, the passes run in until they collapse, as Fusion's
  do.
- **Steep faces are left.** The drop-cutter's surface cannot show a wall, so the
  passes bunch up on one. Any stretch steeper than a plunge (84°) is left out and
  counted, for a waterline pass. The slope range asked for leaves out the rest,
  judged by the slope of the lifted triangle under each stretch, which is the
  slope where the tool touches the part wherever it touches at one smooth point.
- **Direction.** Climbing with a clockwise spindle has the side still to finish
  on the tool's right: inward from the outside in, outward from the inside out.
  Both ways, Fusion's default, keeps closed rings climbing and starts an open pass
  at its nearer end. Its order is its own, so travel reordering is off for it.

## Consequences

- On a dome, adjacent passes are within a tenth of a stepover of a stepover apart
  along the surface, most off where the rings close in on the crest. On a plate
  they are the boundary shrunk by whole stepovers, within a micron.
- On the router jig, a scallop finish passes every check with no gouge. It takes
  about 20.5 min at the estimate against parallel's 28.5, for the same length of
  cut, and leaves the jig's walls: 209 stretches, reported for a waterline pass.
  The whole job takes about 25 s either way.
- **The distance is measured where the tool's tip rides, not on the part.** In a
  concave fillet of radius ρ, a ball of radius R has cusps ρ/(ρ − R) as tall as on
  the flat. Over a sharp convex edge the tip rolls round while the point touching
  the part stays put, so rings bunch within about R of it. Measuring at the
  contact point instead is a change to the one closure the march takes positions
  from.
- Features smaller than a cell don't shape the spacing, though every Z stays
  exact. A part too large for the grid's budget (2²³ nodes) is coarsened and said
  so, and one needing a pitch over half a stepover is refused.
- Not yet Fusion's: a limit on the number of stepovers, rest machining,
  smoothing, a contact point boundary, and up/down milling.
