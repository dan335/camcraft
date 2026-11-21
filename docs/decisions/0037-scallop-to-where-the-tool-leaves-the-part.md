# 0037 — Scallop to where the tool leaves the part, and up upright walls

**Status:** decided, 2026-11-20 · **Milestone:** 0.10

## Question

Down the bulging outside of a user's bowl, Scallop left a band round its
widest part with gaps of 2.4 to 3.3 mm between the passes, against 0.97
everywhere else: a cusp about ten times as tall. The ring above that band
wandered 4.2 mm up and down in a lap. Below it the level rings
([decision 0035](0035-scallop-over-every-slope.md)) took over. Where does that
band come from, and what does the field have to be to reach the line where
the tool leaves the part?

## What was found

- **The field ended a cell short of where the tool leaves the part.** A grid
  node has a height only where the tool rests on the part, and a triangle of
  the field exists only where all three of its corners have one. For a ball,
  the path the tool's tip rides always falls vertically to where it leaves
  the part, its side the last of it to touch, so the last node is a fraction
  of a cell short of that line, and the height between goes as the square
  root of that fraction: on the bowl, 4 mm of height in the last 0.25 mm. No
  line of the field was drawn there. A finer grid shortens the band by only
  √2 a halving.
- **The area's edge was a tolerance short of it too.** The contact region is
  drawn a tolerance inside where the tool touches. On that vertical fall a
  tolerance seen from above is 1.5 mm of height, and the boundary's ring,
  dropped there from above, wandered 4.2 mm up and down: the wavy ring.
- **Level rings were kept only well below where the tool rests from above**,
  more than half a stepover under it, so the first of them started up to a
  stepover and a half below the field's end.
- **Beside an upright wall the tool's path stands upright too.** Seen from
  above its height jumps from the wall's foot to its top across a hair, and a
  drop there rests at one or the other, whichever side of the jump it lands,
  and finds nothing between. Where a face avoided meets such a wall, as a
  pocket's floor avoided and its vertical wall, the edge of the ground
  machined was put where the tool dropped on the machined side of the jump
  rests, its top: the wall is left out of the field.
- **The edge of a face avoided zigzagged by ±6°.** It was found where resting
  on a face machined flips to within half the tolerance, and across a tangent
  edge, a floor running smoothly into a wall, the two faces' heights differ
  only by the square of the distance from it: the flip wanders along the
  wall.

## What was decided

- **The grid is cut where the ground machined ends.** Each node is sampled
  with whether a face machined holds the tool there, exactly: over a part in
  groups, whether the group holding it highest is one machined, a machined
  one winning a tie to rounding (`Seat::rest_in`). Where that changes along a
  grid edge, from machined to avoided or to nothing under the tool, the place
  is found by halving the exact drop, to a hundredth of the tolerance seen
  from above and until the height there moves by less than that, and the
  cell is cut there, each side triangulated up to it (`camcraft_geom::cut`).
  The field is measured over that triangulation and its lines drawn across it
  as on the grid (`TriSurface`). So it runs right to where the tool leaves
  the part, and to where it leaves a face avoided, each found exactly.
- **The ground's edges are rings.** The line where the tool leaves the part
  and the edges of faces avoided are lines through their cuts, placed by the
  field as its lines are, height and all, round a wall at their height where
  the ground is steep. Outside in, both are edges the field is measured from,
  rings of its first level; inside out, the edges of faces avoided are its
  inner edges and the line where the tool leaves the part its far end, cut
  last with the outer boundary's ring. Measured out from the inner edges, a
  line of the field near that far end is kept: only lines all within half a
  stepover of the area's outer boundary are left to its ring, as before;
  left out near where the tool leaves the part, the last line before it
  left 2.1 mm to it down a bulge.
- **The area's edge seeds the field only where it crosses the part.** Off
  the part, the node beside the area's edge was measured to it seen from
  above: up the steep way down to where the tool leaves the part, a fraction
  of its way along the surface, so outside in no ring came within 1.7 to 2.3
  mm of that line. That line is the edge there, found exactly.
- **The area runs a tolerance past where the tool touches the part**
  (`past_contact`), so its edge is no longer where the passes end over the
  part: the field finds that itself.
- **Level rings start a stepover below the highest point of where the tool
  leaves the part,** and are kept where the tool on them is more than the
  tolerance below where it would rest from above, or has nothing under it:
  there the field's rings, which run down to that line, are not. A piece of
  one shorter than the tool's radius, where that line dips only just past it,
  is left to the rings either side, as the field leaves its own.
- **Beside an upright wall, the face the tool rises beside decides.** Where
  the halving closes on a jump of more than the tolerance across a hair, the
  tool beside the wall touches the part nearest its widest circle: its
  profile's largest radius, at the height it reaches it, toward the wall
  (`Seat::upright_toward`, `Seat::beside`). That is looked for as far as the
  tool's radius. Machined, as a pocket's wall, the stretch is the field's and
  the edge is at its foot; avoided, at its top. Where nothing is within the
  tool's radius, under an overhang, the tool touches the overhang with its
  side: the top's. The tolerance was the wrong range to look in: on the
  user's bowl the wall is hollowed 0.054 to 0.057 mm under its rim, deeper
  than the tolerance, and looked for within it the answer flipped along the
  wall and the edge zigzagged up and down it; the tool there is within its
  radius of nothing else.

## Considered and not chosen

- **Staying down between the arcs past twice the tool**, where the way on
  the part is no longer than lifting out, across and down again, as
  adaptive clearing does (decision 0036). Tried, for Scallop alone, and
  taken out: on the user's bowl it changed nothing, the same 28 lifts. The
  links it lifts for are no longer than twice the tool; they are refused
  because each falls 5 to 8 mm onto the next arc's start, steeper than a
  plunge and further than a cut step. So every finish over the part still
  stays down no further than twice the tool for Scallop, as Fusion's, and
  their comparisons with Fusion are not measured again here.
- **A finer grid near where the tool leaves the part.** The fall there is
  vertical; each halving of the cell shortens the band missed by only √2.
- **Bands between the part's edges, a morph across each, on a field measured
  where the tool touches the part.** Built and measured, and rejected. The
  ground was cut into bands at every crease and edge, the rings in each the
  lines of `t = d_A / (d_A + d_B)` between its two edges, their count found by
  measuring the widest gap between their contact lines on the part. On the
  user's bowl it fed 71 m where main fed 38: the outside wall's rings went
  from about 40 to 94, the inside's from 48 to 67. The blend rises slowly
  wherever a band's two edges do not face each other, off the end of a ridge
  and down to where the tool leaves the part, and there the rings fan apart;
  measuring the gap there and raising the count added rings to the whole
  band. With the floor no longer avoided, the band from the rim to the
  bottom took 216 rings and still left the floor uncovered, and in a dish
  the rings turned on 0.13 mm round the ends of its ridge. Its rings also
  stopped at the rim's edges, so each was cut in pieces there. It is kept on
  its own branch as a record.
- **Every face edge a guide**, and creases as guides: rings cut into pieces
  at every crease, which a machinist does not want on a bowl's rim.

## Figures

The user's bowl, a 9.525 mm ball, a stepover of 0.973, inside out from the
floor avoided, cut one way; and the same job with the floor's group removed,
cut from the boundary and where the tool leaves the part in. Gaps are
between neighbouring passes where they cross six upright half-planes down
the outside, each side at Y50, Y76 and Y100.

| | before | after |
|---|---|---|
| Fed | 38.01 m | 39.37 m |
| of it, inside / rim | 16.90 / 2.33 m | 16.88 / 2.31 m |
| of it, outside above Z-34 / below | 12.08 / 6.50 m | 12.92 / 6.91 m |
| Rapid | 1.18 m | 1.70 m |
| Lifts | 16 | 28 |
| Gaps over 1.1 mm at the six sections | 2.37 to 3.25 at five, and 1.26 to 1.70 below them | one, 1.20 at Y50 left, Z-33.99 |
| Without the floor's group: fed | 38.98 m | 40.14 m |
| lifts | 19 | 12 |
| gaps over 1.1 mm at the bottom | 2.81 to 2.91 and 1.26 to 1.70 | none |

Of the 28 lifts, 19 are between the arcs the field's rings end in where
the tool leaves the part (13 before, where the field stopped short of it),
8 between the pieces of the level rings below where that line dips past
them (2 before), and the last up to the clearance. The line runs from
Z-32.3 to Z-35.7 round the bowl, so rings a fixed distance from the floor's
edge end on it in arcs; cut one way, each arc ends an arc's length from
where the next starts, lower down the bulge, and the link there would
plunge. The one gap of 1.20 is where a ring crosses only as a piece
shorter than the tool's radius, left out as the field leaves its own.

On Fusion's `ops_3d`: a new Scallop as before, 94.6% of the columns alike;
with the dome avoided 58.7% where 58.9% were, lifting 7 times where it
lifted 10; with only the pocket's floor machined 42.4% where 42.0% were,
the median 0.034 mm where it was 0.020 and 24% less fed than Fusion's where
22% less was, the floor's edge now found exactly where the group holding
the ball changes, where it was found to within half the tolerance.

On a cup with its floor avoided and its wall to a sharp rim at 90°, 89°,
87° and 85°, the rings run up the wall a stepover apart from its foot to its
top, each level to 0.14 mm upright and to 0.05 off it (`run::tests_bowl`).
How main lays them there was not measured.

## Still not done

- The lifts between the arcs where the tool leaves the part: rings that
  follow that line, or links that follow it between arcs, would take them.
- Up the inside wall each ring still rises onto the flat rim and drops back
  onto the wall two or three times a lap, as before.
- With the floor's group removed, the innermost rings still turn to a point
  at the ends of the lens-shaped floor, as before.
- The rest of these are to be designed again on their own; the bands tried
  for them are under "Considered and not chosen".
