# 0039 — Scallop stops at a sharp rim and starts again past it

**Status:** decided, 2026-11-21 · **Milestone:** 0.11

## Question

A user's bowl has a flat rim 3 to 4.2 mm wide between two sharp edges: the
inside wall turns onto it through 64° to 94°, and it turns down onto the
outside. The rings, each a fixed distance along the surface from the edge of
the floor the operation avoids, meet the rim at a slant, the wall being
longer on some sides than others. So 11 to 13 of them each climbed onto the
rim and dropped back twice a lap, and at any section 5 to 9 crossings near
the rim did nothing but roll the ball over an edge: its contact stays on the
edge line while its tip goes round it, a tool's radius times the angle the
faces turn through, 4 mm and more of height. Every one of those re-cuts the
same line. How should the rings meet a sharp edge they run at a slant to?

## What was measured

On that bowl, a 9.525 mm ball, a stepover of 0.973, inside out from the
floor avoided:

- **Where the tool rolls.** The tool rests on the part by a sharp convex edge
  alone where the triangles holding it, those whose own drop is where it
  comes to rest to rounding, include two whose normals are further apart
  than the crease angle, 30°, the angle `topology::derive` already calls an
  edge between faces, and that lie convex to one another, each on or under
  the other's plane. A convex tool cannot touch a convex pair in two places,
  so that is one contact, on their shared edge or corner. Asked at every node
  of the field's grid (pitch 0.324), 25,695 nodes roll, in two pieces, each
  round all 72 five-degree sectors of the bowl: the inner edge's, 14,211
  nodes from Z-8.53 to Z-3.78, 4.7 to 8.0 mm wide seen from above; the
  outer's, 11,484 from Z-5.56 to Z-3.81, 3.1 to 6.3 mm wide. Nothing else
  rolls, not the floor's edge, which is hollow, nor any facet of a tight
  curve. The same without the floor's group.
- **The rings across the strips.** By the sections' own measure (each
  crossing with its neighbours on a circle the tool's radius, convex, above
  Z-12), 4 to 9 crossings rolled over an edge at eight sections round the
  bowl.
- **Up a wall nearly upright, a strip's near border is the wall's top.** Seen
  from above the ground there is a sliver between the floor's edge and the
  strip, and its cells are cut on both sides: on a bowl in the tests whose
  wall is 0.6 mm wide at its steepest, the rings up the wall came out open.
  And across a cell cut at the strip, the straight line from a corner to the
  far cut strays into the strip, where the tool is higher than either end:
  a grid edge's profile ([`guided::bent`]) went over that bump, and the
  heights it gave the rings up the user's wall jumped by up to 0.27 mm from
  one point to the next, breaking the blended rings into open pieces.

## What was decided

- **Rolling is told by the drop** (`DropCutter::rest_on_in`, `Rest::rolling`):
  the drop cutter gives the triangles that hold the tool as it finds the
  rest, looking on past the highest only while another could be as high; the
  seat asks of the group that holds the tool whether two of them are a convex
  pair across the crease angle. The crease angle is one constant,
  `topology::CREASE_DEGREES`.
- **A ring-shaped strip with ground round it is a break in the ground**
  (`scallop::ground`). In either order, the nodes where the tool rolls are a
  class of their own; a piece of them, its nodes joined across cells, corners
  included, is kept as one only where it runs round one hole, its Euler
  number from the grid's squares of four nodes (Gray's) nought, and the ground
  lies all round it two nodes deep. Any other rolling node is ground as
  before: an edge that runs off the grid, or to where the tool leaves the
  part or to a face avoided, or along a wall too narrow seen from above.
  Toward a strip the ground ends where the tool starts rolling, found by
  halving along each grid edge as the ground's other edges are, so both of
  its borders are edges of the ground: the tool touching the edge from one
  face, then from the other. Both are needed: with only one, the face on the
  other side would be left a sliver as tall as the stepover squared over
  twice the tool's radius at the edge.
- **The ground either side is a zone, with a field of its own, cut one after
  the next** (`scallop::distance`). Inside out, from the innermost out, each
  is measured from its inner edges: the area's holes on it, faces avoided,
  and the strip's far border round its hole. Its outer edge, the near border
  of the strip round it or where the tool leaves the part, is its far end,
  and its rings blend into it as into the line where the tool leaves the part
  (decision 0038). Outside in, the mirror, from the outermost in: a zone with
  one inner edge, closed, and no hole of the area on it is measured from its
  outer edges, and that inner edge, the near border of the strip inside it
  or the edge of a face avoided, is its far end, its rings blended into it.
  Any other zone is measured from every edge of it, as the whole ground with
  no strip is. Each of the area's rings is cut with
  the zone most of it lies beside, or the last. One field a zone, the trees
  cut one after another, rather than one field with each zone's levels
  offset: each zone's far edge, band and order are its own, and the ground,
  its grid and the lengths down its cliffs are shared.
- **Zones only where they do their job** (`scallop_finish`). Strips break
  the ground into zones so that every ring closes and none climbs over a
  sharp edge. Where a zone has a far edge and the band into it is refused
  (`blend::band`, asked as the plan asks it), its rings cannot close: they
  would end in arcs on that edge. There the zones are not doing their job,
  and the ground is cut as one, as before strips broke it: every rolling node
  ground again, the grid cut a second time from the same drops, and the
  field measured over it. It is all or nothing for the operation; choosing
  strip by strip, keeping those whose zones close and grinding the rest
  again, was not tried. The bands the test finds go on to the plan, so none
  is worked out twice.
- **Between zones the tool rolls over the edge once**: the link from the
  near border's ring to the far border's, on the seam, stays down over the
  part, a few millimetres. The seam carries on into the next zone.
- **Across a cell cut at a strip, a grid edge's profile keeps between its
  ends' heights** (`guided::bent`): the way from one to the other climbs or
  falls, and where the straight line strays into the strip the drop there is
  not the ground's. Only edges with an end where the ground ends against a
  strip; every other profile is as it was.
- **A band with no whole triangle of the lattice in it**, thin down a wall
  so steep every cell is cut, has its width at its widest, `a + b`, for the
  count of its rings, where no spacing could be measured (`blend::band`).
  On the bowl in the tests, the band into where the tool leaves its tilted
  bulge, measured from the rim's far border, was such a band.

## Considered and not chosen

- **One field, each zone's levels offset**: the band, the far edge, the order
  and the direction would each have to be told which zone they are in.
- **Clamping every bent profile between its ends**: it moved the points of
  rings near where the tool leaves the bowl by a tenth, outside in too, where
  no strip is.
- **Strips beside a wall too narrow, broken anyway**: the rings up the wall
  came out open on the bowl in the tests. Rings up such a wall cross the rim
  as before.

## Figures

The user's bowl, inside out, the floor avoided, before (8ef8e32) and now:

| | before | now |
|---|---|---|
| Fed | 42.87 m | 41.90 m |
| of it, inside / rim | 16.89 / 2.31 m | 16.72 / 2.44 m |
| of it, outside above Z-34 / below | 15.65 / 8.02 m | 14.85 / 7.88 m |
| Rapid | 0.09 m | 0.10 m |
| Lifts | 1, the last retract | 1, the last retract |
| Crossings rolling over an edge, eight sections | 4 to 9 | 0 or 1 |
| Rings rolling over the inner edge, every 5° | 0 to 7, 5.4 on average | 1, its near border's |
| Gaps over 1.1 mm at the six sections | none | one, 2.92, across the outer strip |
| Most left on the wall's top 10 mm (simulated) | 0.870 mm | 0.864 mm |
| on the rim | 0.026 mm | 0.025 mm |
| on the outside's top 10 mm | 0.052 mm | 0.049 mm |

Across a strip the passes are a tool's radius times its angle apart where
the tool's tip is, and nought where it touches: the gap at the sections is
the outer strip's, and the stock says what is left. The most left on the
wall's top is in the same place before and now, under the inner edge where
the wall overhangs.

The same job with the floor's group removed: 40.96 m and 40.86 m, one lift
each; the most left on the wall's top 10 mm 2.201 mm before and 0.884 now,
the rim and the outside's top as above. Outside in, both programs were then
byte for byte as before: zones were built inside out only, until the mirror
below.

On Fusion's `ops_3d` cut inside out with its 6 mm ball at a stepover of
0.6, the pocket's rim is a strip, and the rest of the part is a zone
measured out from the rim. The band from it to where the tool leaves the
part is too wide to blend, and with the zones its rings ended in arcs
there, four families of them: 14.42 m fed where 14.89, 6 lifts where 3, and
0.49 m at rapid where 0.11. So the ground is cut as one, and its program is
byte for byte the one before zones: 14.89 m, 3 lifts, 0.11 m at rapid. On
a block in the tests with a round pocket off centre (`run::tests_bowl`),
the zones gave 20 arcs from the block's top down its sides, 33 open passes
over the part and 12 lifts; cut as one, no arc, one open pass, a sliver on
the pocket's wall, and 4 lifts. On the user's bowl, with its floor's group and
without, every zone closes, and its programs are byte for byte as they
were. Outside in
`ops_3d` is byte for byte as it was.

On a bowl in the tests with a sharp flat rim and a wall from 48° to 71°,
longer on some sides than others (`run::tests_bowl`): with the strips no
break, 1 to 3 crossings at each of 36 sections rolled over the inner edge
and 2 or 3 over the outer; now none, and either side of each edge the rings
are no further apart than a stepover where the tool touches the part. On
Fusion's `ops_3d`, cut outside in, Scallop's programs and Contour's are as
they were.

### Outside in

The user's bowl, outside in, the floor avoided, before the mirror (163a9dc)
and with it:

| | before | now |
|---|---|---|
| Fed | 39.82 m | 40.70 m |
| of it, inside / rim | 16.88 / 2.45 m | 17.29 / 2.43 m |
| of it, outside above Z-34 / below | 12.81 / 7.68 m | 13.30 / 7.68 m |
| Rapid | 0.56 m | 0.28 m |
| Lifts | 7 | 2: the first, as before, and the last retract |
| Rings with the tool on an edge's line over a tenth of their length | 7 on each edge | 1, the border's |
| Most left inside the rim, Z-10 to -5 (simulated) | 0.232 mm | 0.226 mm |
| columns there over 0.1 mm | 176 | 254 |

The extra 0.88 m is inside, 0.41 m, and on the outside above Z-34, 0.49 m,
where the two bands are. The inside's band into the floor's edge has 12
rings: its far edge's nodes are 1.15 to 6.48 mm from the last whole ring,
but its rings are counted by where they would lie furthest apart, 11.88 mm
across a triangle of the lattice, so where it is narrow they are as little
as 0.09 mm apart. The outside's band into the outer strip's border has 10
rings over 0.78 to 10.32 mm; the rim's into the inner border, 1 over 0.76
to 1.87 mm.

Inside out leaves less under the inner edge, 0.194 mm and 120 columns over
0.1, because its band into the rim's border packs its rings there 0.47 to
0.49 mm apart, half a stepover. Outside in that band is at the floor's edge,
and the rings under the rim are a stepover apart, 0.97 to 0.98, as asked.
The wall there is upright or nearly, where what is left measured straight up
is the cusp over the cosine of the wall's angle: 0.025 mm at 85° is 0.29 mm
up. So the yardstick is the rings' spacing where the tool touches, and the
most left against outside in before. At two sections, 170° and 300° round
the bowl's middle, one ring touches on the inner edge, its border's, with the
last touch up the wall 0.02 to 0.26 mm below it and the first on the rim 0.05
to 0.66 mm past it, where before 7 or 8 rings rolled on that edge; across
the rim and the outer edge the largest gap is 1.11 mm, as before (1.10).

With the floor's group removed: 40.39 m where 40.90, 2 lifts as before, one
ring on each edge where 12. `ops_3d` outside in, and Scallop's three
comparisons with Fusion, are byte for byte as they were: the zones round the
pocket's rim do not all close, and the ground is cut as one.

## Still not done

- Where a zone measured out from its inner edge cannot blend into its outer
  edge, as `ops_3d`'s outside of its pocket inside out, the ground is cut as
  one, and its rings climb the pocket's rim and drop back as before.
  Measuring such a zone from every edge of it instead, so its rings close and
  the two families meet in the middle, was tried (2026-11-21) and closes the
  rings on the block's top, but breaks those down its upright sides: 18
  lifts, and 214 passes broken at a wall where 1. The cause is in the field,
  not the rule. An upright side is one grid edge 19.9 mm long along the
  surface, and the field is a straight line along an edge between its two
  nodes. Measured from the foot alone, as before zones, that is exact, and
  the rings down the side are level and a stepover apart. Measured from the
  pocket's rim too, 7.84 mm from the top of the side, the two fronts meet
  inside that one edge, about Z11.8, which a straight line from 7.84 at the
  top to 0 at the foot cannot say: its 13 rings are spread over the whole
  side, 1.52 mm apart and at a slant, a different one on each side of the
  block. A field measured from one edge, and the blend between two, are
  straight lines along such an edge and are right as they are; one measured
  from two edges that meet on an upright wall is not, in either order.
  Measured on what is cut today, outside in, where every field is measured
  from every edge, it leaves nothing: on the user's bowl no grid edge can
  lose more than 0.56 of a stepover's ring, and on `ops_3d` with the dome
  avoided the 428 edges that can lose a whole one, up to 4.9, are all on the
  pocket's upright walls, which the tool's side cuts at any height; on its
  left wall the stock left is 0.183 mm at most to Fusion's 0.234. Until
  the field keeps where two fronts meet inside a cliff's edge, as a node of
  its own there, a zone is not measured from every edge to close its rings,
  and such a pocket's rim is climbed.
- With the floor's group removed, outside in still measures the inside in
  from the rim's inner border, the zone having no inner edge, so its
  innermost rings still come to points at the ends of the lens-shaped floor:
  16 rings turn more than 45° over a millimetre, where 21 did. The spine is
  laid inside out only (decision 0040).
- A band whose rings are counted by its widest spacing measured on a
  triangle, as the inside's into the floor's edge outside in, packs them
  where it is narrow: 12 rings across 1.15 to 6.48 mm, as little as 0.09 mm
  apart.
- Up the user's inside wall each ring still rises and falls 2 to 6 mm a lap,
  turning four times: the wall's height round the bowl, the rings a fixed
  distance from the floor's edge.
- The three blended rings nearest where the tool leaves the part still turn
  in height by 0.06 to 0.09 mm every few millimetres, about the job's
  tolerance of 0.05 (decision 0038).

With the floor's group removed, inside out, the innermost rings turned to a
point at the ends of the lens-shaped floor; decision 0040 took those away.
