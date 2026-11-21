# 0035 — Scallop over every slope, from the inner edges out

**Status:** decided, 2026-11-20 · **Milestone:** 0.10

> **Amended by [decision 0037](0037-scallop-to-where-the-tool-leaves-the-part.md).**
> The field's grid is cut where the ground machined ends, so it runs right to
> where the tool leaves the part and to the edge of a face avoided, each found
> exactly, and each edge is a ring. The area runs a tolerance past where the
> tool touches the part. The level rings start a stepover below the highest
> point of where the tool leaves the part and are kept where the tool on them
> is more than the tolerance below where it would rest from above. Beside an
> upright wall, the face the tool rises beside decides which side of the edge
> the wall is on.

## Question

Scallop left ground steeper than a plunge to rings round the walls
([decision 0009](0009-scallop-distance-field.md), and every finish over the
part since [decision 0014](0014-walls-for-every-finish-over-the-part.md)).
Where a part's walls stand near the plunge's slope round part of their way,
as a bowl's may, that band is not a ring but crescents: the wall rings
are open arcs, the field's rings that cross a crescent are cut open too, and
the two families meet along ragged edges. Inside out, the rings were outside
in's cut in reverse, laid across the face the operation avoids and cut out of
it afterwards. Seen from above a bowl's program was two patterns laid over
each other, scattered pieces each lifted to, and a seam that wandered. What
does a scallop finish have to be for a machinist to call it logical: one
family of rings a stepover apart, one entry, a short step from each ring to
the next along one straight seam, lifts only where the part forces them?

## What was found

- **The field was never the trouble on a wall; placing points on it was.**
  The march measures down a cliff in the grid as well as across a flat: on a
  plateau 6 high falling to a floor down an upright face, a ball of 2, the
  field at the cliff's top and foot nodes was the distance along the surface
  the tool rides to within a thousandth of a millimetre. What made passes
  bunch and wobble there was finding each point as a place seen from above
  and dropping the tool there: a hundredth of a millimetre sideways is tenths
  in height, and on an upright face the drop goes from its top to its foot at
  once.
- **A grid edge down a cliff cuts the cliff's corner.** Where an upright face
  meets the floor inside one cell, the straight edge from the node above to
  the node below is the hypotenuse of the way down and along: the ring at the
  foot was 0.66 of a 0.5 stepover from the one above it.
- **A face with no width seen from above has no place in a field of
  heights.** A bowl's outside that turns upright below its widest, standing
  on its base, is not under the tool from above at all below where the tool
  last rests on it: the field ends there. Laid on down it by giving the
  ground off the part the Bottom's height, its rings came out sloping round
  the bowl, a stepover apart along the surface from the floor's edge, and in
  358 pieces, each lifted to: the linker stays down only between points the
  tool rests on from above.
- **Travel reordering had nothing to gain** from one family laid in order and
  took later rings' pieces before earlier ones' wherever they started nearer.

## What was decided

- **Scallop cuts every slope itself.** No steep band and no rings round the
  walls for it; the slope range asked for still limits it. Parallel, Spiral,
  Radial and Morphed Spiral keep theirs: their stepover is seen from above.
- **On steep ground the height is given and the place found.** Where a line
  of the field crosses a grid edge, its height is the grid's there, by the
  field's own interpolation, or, down a cliff, as far along the surface down
  the edge, sampled by drops, as the line is along the edge. On ground
  steeper than 45° the point is found across the slope where the tool,
  dropped, rests at that height, by halving the exact drop; on an upright
  face it closes on the face, the tool beside it at that height, touching it.
  No point is ever below where the tool rests. Between two such points the
  pass is refined as a curve round the wall, its height between theirs
  (`strategy/scallop/guided.rs`).
- **A cliff's grid edge is measured along the surface the tool rides**, by
  eight drops along it, where it falls steeper than a plunge
  (`geodesic::march_along`). Every other caller's march is as it was.
- **The edge of a face avoided is an edge of the field.** Where the tool goes
  from resting on a face machined to resting on faces avoided alone, found
  along the grid's edges to half the tolerance, the ground machined ends, as
  it does at a hole in the area: the field is not measured across what is
  avoided, and a pass runs along the edge, placed seen from above where it
  was found. So the rings are laid anew round what is avoided
  ([decision 0033](0033-surface-groups.md)), rather than laid across it and
  cut out of it.
- **Inside out measures from the inner edges.** Where the ground has inner
  edges, the area's holes or the edges of faces avoided, the field is
  measured from them alone, the first pass along them. The rings grow
  outward and are cut in that order, those within half a stepover of the
  outer boundary along the surface left to its ring, cut last: there they
  would be slivers along it. Without an inner edge, inside out is as it was;
  outside in measures from the boundary and every edge, as it always has from
  the area's holes.
- **Below where the tool rests from above, level rings.** Down a wall the
  field cannot cover, rings at heights a stepover apart down to the Bottom,
  found as contour finishing finds its rings, each level going down along the
  wall onto the next at the default ramp angle, kept only where the tool on
  them is more than half a stepover below where it would rest dropped from
  above, or has nothing under it. Above that the field's passes are within
  half a stepover, so nothing is cut twice and nothing is left wider than a
  stepover between the last ring of the field and the first of the wall's.
  Inside out the field's rings come first and then the wall's top down;
  outside in, the wall's first, then the field's. With no Bottom below the
  ground touched there are none.
- **The linker stays down beside a wall.** A link from or to a point below
  where the tool would rest from above, or with nothing under it, was always
  refused. It is made, straight, where the tool all along it at the lower of
  its two heights would rest no higher, as a link at a level is made, the
  tool grazing the wall it runs beside; and where it falls no further than a
  descent at the cutting feed may (`path/linker`). It serves every pass beside
  a wall: Contour's and Ramp's programs on `ops_3d` are unchanged, and the
  rings round the walls of Parallel, Spiral, Radial and Morphed Spiral lift
  far less (below).
- **One seam.** Each closed ring starts where it crosses the half of an
  upright plane through where the first started, square to it, on the side
  still to finish; one that does not cross it starts nearest the tool
  (`strategy::projected::Seam`).
- **The field's order, not the nearest next**, as
  [decision 0034](0034-entries-and-order-of-the-finishes-over-the-part.md)
  had it. Scallop keeps the order it lays its passes in
  (`Uses::nearer_ends`): each piece that a height or a
  face avoided cuts a ring into is taken from its nearer end, where it cuts
  both ways, and none is moved (`path::ordering::nearer_ends`).

## Considered and not chosen

- **Keeping the steep band**, closed over the crescents: it would still be
  two families meeting along an edge the part does not have.
- **Laying the field on down beside the part to the Bottom**, the ground off
  the part at the Bottom's height. Tried: a face upright under the tool's
  rest has no width seen from above, so its rings follow the field's
  distance, sloping round the part, and the linker cannot join them.
- **Solving for the place on every slope.** Below 45° dropping is the better
  conditioned of the two, and it is what every other pass does.

## Figures

**Down a wall** (`strategy/scallop/tests.rs`): a plateau 6 high falling to a
floor down a face at 90°, 87°, 84°, 70° and 50°, a 2 mm ball, a 0.5 stepover.
Measured on the surface the exact drop traces across the wall, the passes are
within 5% of a stepover apart, over the edge, down the face and round its
foot, and each is level to 0.021 along the surface. Before the cliff's edges
were measured along the surface, the ring at an upright face's foot was 0.66
from the one above it.

**A bowl** (`run/tests_bowl.rs`): a floor avoided, an inside wall 87° to 59°
round it, a rim, an outside sloping and then upright to its base, a 4 mm
ball, a 0.8 stepover, inside out, the Bottom a millimetre above the base. Up
the inside wall each ring is within 5% of a stepover from the next along the
surface (0.78 to 0.82 measured); every ring over the part starts on one
straight line seen from above; nothing it machines is further than three
quarters of a stepover from a cut (0.60 measured: half a stepover but along
the outer boundary, where the last ring and the boundary's are up to a
stepover and a half apart); seven lifts; no gouge, no rapid into the
material, and the same on one thread or eight.

**Scallop on `ops_3d`**, before and after:

| Program              | Lifts, before / after |
|----------------------|-----------------------|
| `scallop`            | 2 / 12                |
| `scallop_avoid_dome` | 15 / 10               |
| `scallop_floor_only` | 12 / 3                |

With no groups the passes now cut the pocket's walls and the R8 round, where
rings round the walls took the steps down. They lift more: taken in the
field's order, not the nearest next, the ways between some of its regions are
longer than the tool stays down.

**The other finishes.** On `ops_3d` with no roughing, Pencil's, Contour's and
Ramp's programs are byte for byte as they were; Parallel's, Spiral's,
Radial's and Morphed Spiral's cut the same points, and their rings round the
walls, joined beside the walls where they lifted, lift 4, 47, 3 and 111 times
where they lifted 40, 83, 40 and 147.

## Still not done

- **Lifts at the far end** where the field's rings are cut open by the outer
  boundary, as where an outside's upright skirt rises higher in places: each
  piece is taken from its nearer end, but the pieces of one ring lie either
  side of the part, and the ways between are lifted for.
- **A level ring's arcs** where the top of the skirt is not level are cut
  level by level from the top down, so a ring's arcs on two stretches of the
  wall are taken one after the other, with a lift between.
- **Along the outer boundary**, where the surface the tool rides falls
  steeply within a cell, as the ball rolls off the top of an upright skirt,
  the last ring and the boundary's are up to two or three stepovers apart
  along that surface, over what is a sharp bend of the part.
- **Scallop's own order** lifts more than the nearest next on parts of
  separate regions (above).
