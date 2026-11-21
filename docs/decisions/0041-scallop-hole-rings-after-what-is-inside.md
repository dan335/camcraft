# 0041 — A region's rings round its holes are cut on the other side of its children

**Status:** decided, 2026-11-21 · **Milestone:** 0.11

## Question

A new Scallop starts outside in, and outside in a field is measured from
every edge of the ground at once: a region with a hole, as a user's bowl's
ground between its rim's outside and the edge of the floor it avoids, has at
every level a ring toward its outside and a ring toward its hole. Each tree
node held both and cut them together, the nearest next, so the tool went
from one family to the other and back at every level: on the bowl, 36 lifts.
In what order should a region's two families be cut?

## What was decided

- **Seen as rings nesting in one another, the order is plain.** From the
  boundary in: the outer rings `O0, O1 .. Om` going in, then whatever is in
  the middle, then the holes' rings `Im .. I0` going in to the hole; from the
  middle out, the same backwards. So a nested region holds the passes it
  cuts before its children and those it cuts after them (`nesting::Nested`),
  with no flag for which way round: pocket clearing and flat finishing put
  their rings in the one or the other by their order, as they did.
- **Scallop sorts a region's lines into its outer lines and its holes'
  rings** (`lay::Layer::lay`). A line has its region on its left, so a
  closed line turning right goes round a hole of it. Measured out from the
  inner edges, every line is a hole's, as every ring grows out from one,
  arcs where the boundary cuts them too: then the whole node goes first,
  inside out, which is what the old special case for a field measured out
  did. Nodes chained one in the next (the band into a far edge, the rings
  below where the tool leaves the part, the rings round a spine) are no
  region and keep the chain's order. From the boundary in the outer lines
  go before the children and the holes' rings after; from the middle out the
  other way.
- **From the boundary in, the field's rings start where the level rings
  below them end** (`scallop_plan`, `ScallopPlan::cut`), the mirror of the
  middle out, where the level rings are gone down onto from where the
  field's last ring ended. The rings are planned and laid first, which says
  where the level rings start; the level rings, cut from the top of the wall
  down, then say where the tool is; and the field's first ring starts
  nearest there, its seam running on from it. So the tool steps up from the
  last level ring onto the first ring over the part, staying down. Before,
  the field's seam was fixed where its first ring was found, and the tool
  lifted to it: on the user's bowl from (137.5, 139.4, Z-39.2) to
  (115.6, 12.3, Z-34.6), 129 mm across. Now it lifts once, at the end, and
  goes 0.08 m at rapid where 0.28, feeding 40.73 m where 40.70; the same with
  the floor's group removed, once where twice. Inside out, and `ops_3d`
  either way, are byte for byte and figure for figure as they were.

## What was measured

| | before | now |
|---|---|---|
| A user's bowl, outside in: fed | 40.52 m | 39.82 m |
| lifts | 36 | 7 |
| The same with the floor's group removed, outside in | 40.90 m, 2 lifts | the same, byte for byte |
| Both, inside out | | byte for byte |
| `ops_3d` cut either way | | byte for byte |
| `ops_3d`, Scallop new: fed / lifts | 9960 mm / 1 | the same |
| with the dome avoided | 9084 mm / 3 | 8817 mm / 4 |
| with only the floor machined | 887 mm / 2 | 889 mm / 3 |

Contour, Flat, Parallel, Pencil, Project, Ramp, Spiral and Morphed Spiral
feed and lift the same on their jobs to the figure; pocket clearing's and
flat finishing's programs are unchanged.

**The two lifts gained.** Each is one link the rule's own order makes long,
not an order wrong within a family:

- *With the dome avoided.* A region's two rings round the dome near the
  pocket's left wall are open there, broken where the wall falls steeper than
  a plunge: the deeper one's two pieces run the wall's length from y = -10 to
  y = 10 and back, and were cut ending at (-21.82, -10.00); the next out,
  one piece whose both ends are at the wall's other end, (-21.58, 10.00) and
  (-21.64, 10.00), was then 20 mm off, and the tool lifted. Before, those
  rings were cut beside the outer rings of their regions, and the tool came
  to them from elsewhere.
- *With only the floor machined.* The floor is a ring round the dome, its
  outer edge and its edge round the dome the two lines of its level-0
  region, and it splits into two lobes going in. The edge round the dome,
  which the tool used to go to straight from the outer edge, is now cut
  after both lobes; from the outer edge's end at (-20.59, -8.44) the nearer
  lobe's first ring starts at (7.63, -8.01), 28 mm off, and the tool lifts.

Cutting the holes' rings the other way within their family, from the hole
out (`I0, I1 .. Im` after the middle), was measured too: the same lifts on
both `ops_3d` variants, and on the bowl 8 where 7. From the middle out to the
hole the family starts beside where the middle ended, and each ring is a step
from the last; from the hole out it starts at the hole, wherever the middle
ended. So the order stays as the rings nest.

## Considered and not chosen

- **The holes' rings from the hole out**, measured above.
- **Each family's nearest ring next, whichever family**: what it replaces.

## Still not done

Nothing on the user's bowl cut outside in: it lifts once, the last retract.
The five lifts near the rim this left, where the rings met the rim's edges
at a slant and came out in pieces there, went when outside in had zones too
(decision 0039); the one from the level rings below to the first ring over
the part, when the field started where they end (above).
