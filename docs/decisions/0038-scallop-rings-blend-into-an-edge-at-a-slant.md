# 0038 — Scallop's rings blend into an edge they meet at a slant

**Status:** decided, 2026-11-21 · **Milestone:** 0.11

## Question

Down a user's bulging bowl the line where the tool leaves the part runs from
Z-32.3 to Z-35.7 round the bowl, but the rings, each a fixed distance along
the surface from the edge of the floor the operation avoids, are not level
with it: the last that goes all the way round spans Z-22.4 to Z-32.0. So for
thirteen stepovers before that line every ring was cut by it into arcs, and
below it the level rings were cut into pieces by it too. Each arc and piece
was lifted to and from, 27 of the 28 lifts of
[decision 0037](0037-scallop-to-where-the-tool-leaves-the-part.md), and one
section kept a gap of 1.20 mm where a piece too short to cut was dropped.
How should the rings meet an edge they run at a slant to?

## What was measured

On that bowl, a 9.525 mm ball, a stepover of 0.973, inside out from the
floor avoided:

- **The band is narrow and its two edges run side by side.** Along the line
  where the tool leaves the part the field, the distance from the floor's
  edge, runs from 88.3 to 99.9 mm. The last ring at least half a stepover
  from the line all round is level 90, at 87.6 mm, so the band between that
  ring and the line is 12.4 mm wide at its widest, against 19 for twice the
  tool's diameter.
- **The spacing of the blend's lines, measured, is the band's width.** On
  each whole triangle of the band, how far apart along the surface lines of
  `t = a / (a + b)` a unit apart are (`a` the distance from the last whole
  ring, `b` from the line) is `1 / |∇t|`: the median 8.9 mm, the 99th
  percentile 12.1, the most 12.9 where the band's two edges face each other a
  little off square, against 12.4 for `a + b` at its widest. So 14 lines a
  unit of `t` over 14 apart, 13 rings between and the line itself, no two
  more than 0.92 mm apart along the surface. Each came out one closed line.
- **Below the line**, from Z-35.7 at its lowest to Z-32.3 at its highest,
  down to a level ring half a stepover under its lowest, at Z-36.2, four rings
  are needed, the deepest place 3.9 mm.
- **What it costs.** It feeds 42.87 m where it fed 39.35: 2.44 m more down
  the outside above Z-34 and 1.07 m more below it, the inside and the rim
  as they were. A closed ring of the band is as long all round however thin
  the band is there, where the arcs it replaces stopped short: half the
  band's spread of widths over the stepover, in laps, about 6 laps of 0.49 m,
  as reckoned before it was built, and below the line two laps more. Started a
  stepover from the line rather than half, the band took a ring more and the
  field a ring less, for the same feed.
- **The line's height is ill-conditioned, and bobbed.** Dropped at the cuts
  along the grid's edges, the height where the tool leaves the part went up
  and down by 0.1 to 0.3 mm from one cut to the next, 673 times round the
  bowl, and the rings near it with it. There the tool's tip is where its
  widest circle passes the part's widest point, a flat maximum, and on the
  part's facets which facet is widest changes from one grid edge to the
  next: just inside the line the drop is already 0.03 to 0.16 mm higher a
  ten-thousandth of a millimetre in, and beside it, nothing being under the
  tool, the tool touches the part at the same place, to a ten-thousandth of a
  millimetre, from 0.6 mm below that height up to it. So any height in that
  range cuts the same; the drop's picks one at random. Fitting the fall's
  square root further in, at a quarter and a whole millimetre, still bobbed
  228 times.
- **Steps down a steep wall.** With rings one under another, under the line
  and on the steepest part above it, the way from one to the next fell
  further than a cut step and steeper than a plunge, and lifted: two between
  whole rings, five under the line. The ramp down along a ring was refused
  on a ring not level. And after a ring was ramped onto, the next started
  back at the seam, 28 mm behind where the ramp had left the tool, and the
  way back lifted too.

## What was decided

- **Between the last whole ring and an edge the rings meet at a slant,
  rings that blend from the one into the other** (`strategy/scallop/blend.rs`).
  Where the field is measured out from the ground's inner edges and the far
  edge, here where the tool leaves the part, is closed, the ground between the
  last ring that goes all the way round and that edge is filled with closed
  rings: the lines of `t = a / (a + b)` at whole fractions, `a` the field
  less that ring's level and `b` the distance from the edge, measured as the
  field is, down each cliff along the surface the tool rides. The field's own
  lines past that ring, and any top-up, give way to them; the edge is the last
  of them.
- **The band starts from the last ring at least half a stepover from the edge
  all round.** The field is a distance, so that ring is that far from every
  point of the edge: the band's thin end is half a stepover wide or more,
  where `t` would be nought over nought where the ring met the edge.
- **The count is the band's measured spacing over the stepover**, rounded up:
  the most `1 / |∇t|` on any whole triangle of the lattice inside the band,
  the slivers a cut cell is fanned into being too thin to say.
  *Amended 2026-11-21:* `1 / |∇t|` is `(a + b)² / |b ∇a − a ∇b|`, and `∇a`
  and `∇b` are now taken as unit, as a distance's are. A triangle across a
  ridge of either field, where its rings turn a corner, has three nodes on
  two fronts, and its plane is their mean, shorter than either: `cos 45°`
  where they meet square. Taken at its length, that shortness read as width.
  On a user's bowl cut outside in, one such triangle on the band into the
  floor's edge, its `|∇a|` 0.703 where the triangles round it have unit
  gradients turning through about 74°, measured 11.88 mm where the band is
  8.48 wide at its widest, and gave 13 intervals, as little as 0.09 mm apart
  where the band is narrow; with unit gradients it measures 8.44, 9
  intervals. Their directions are kept, so where the two do not point
  opposite ways, at a band's corners, the lines of `t` still spread past
  `a + b` as they do.
- **Below the edge, rings a fraction of the way down.** Where the edge is the
  one line the tool leaves the part along, closed, the slope range reaches
  upright and the Bottom is under it, the level rings start half a stepover
  under its lowest point, so each goes all the way round, and between the edge
  and the first of them each point of the edge goes down a fraction of the
  way at a time, as many rings as its deepest place needs, each point found
  beside the wall at its height as the level rings' points are
  (`waterline::Puller`).
- **Bounded.** A band is blended only where the edge is closed, each blended
  ring comes out one closed line, every node of the band is inside the area,
  and the band, and the spacing measured on it, are no wider than twice the
  tool's diameter (`blend::BAND_LIMIT`), as far as the tool stays down
  between passes over the part. Otherwise the rings are cut by the edge as
  they were.
- **Order and direction as the rings round them.** Inside out: the last whole
  ring, the blended rings out from it, the edge, the rings below it down to
  the level rings. Outside in, the field is measured from the edge itself and
  no band is blended above it; the rings below it are cut first after the
  level rings, from the deepest up to the edge, and then the field's rings in.
  Each is cut the way the edge is, started on the seam.
- **Where the tool leaves the part, the highest height that rises and falls
  no faster than the tool ramps** (`ground::level_where_it_leaves`): each cut
  along that line gets the least of its own dropped height and every other's
  raised by the slope of the default ramp angle times how far along the line
  it is. It is never above where the tool rests there, so the tool there still
  touches the part where it leaves it, and placed beside the wall at that
  height it is clear of the part.
- **The ramp onto a ring rides above it** (`projected::Emit`): at each point,
  the ring's own height and what is left of the drop, so it is never under
  the ring however the ring rises and falls; a level ring's ramp is as it was.
  A ring a step under the last, down a steep wall, is ramped onto whether it
  is level or not. A ring starts on the seam only where the last left the tool
  beside it, within as near as a ramp starts; after a ramp, nearest where the
  tool is.
- **The level rings below are ramped onto once the rings the field covers are
  left out**, the first from where the field's last ring ended: laid first and
  ramped, the ramps had run onto rings inside the bowl that were then left
  out, and the first two level rings were joined by a lift.
- **Two plain mistakes mended on the way.**
  - Measured out from the inner edges, the far end's lines, where the tool
    leaves the part and the outer boundary's ring, had the ground on their
    left, where the field's lines have its higher side: they were cut the
    other way from every ring before them. They are turned round.
  - A level ring ramped down onto the ring below it is one pass that ends a
    level lower than it starts, and Scallop takes each open pass from its
    nearer end: every other pair of level rings was run backwards, up the
    ramp, both rings conventional. A pass that ramps is never run backwards
    (`path::ordering`).

## Considered and not chosen

- **A zigzag of the arcs**, each arc's end joined to the next arc's start
  along the line: no lifts, but a pass that turns back on itself at every
  arc's end, and the arcs are cut alternately climbing and conventional.
- **The potential between the two edges**, a Laplace solve on the band, the
  principled form of the same blend. Its lines are as far apart as `t`'s
  where the edges face each other, as they do on a band this narrow; not
  built.
- **The count from the band's width**, `a + b` at its widest: 13 rings on the
  bowl where the measured spacing asks for 14, one gap 4% over the stepover
  where the edges face each other off square.
- **The band from a stepover clear of the edge**: one ring more in the band,
  one fewer of the field's, and the same feed.

## Figures

The user's bowl, inside out, the floor avoided, before and after:

| | before | now |
|---|---|---|
| Fed | 39.35 m | 42.87 m |
| of it, inside / rim | 16.89 / 2.31 m | 16.89 / 2.31 m |
| of it, outside above Z-34 / below | 13.21 / 6.95 m | 15.65 / 8.02 m |
| Rapid | 1.66 m | 0.09 m |
| Lifts | 26 | 1, the last retract |
| Passes not closed rings | 19 arcs and 6 pieces of level rings | none |
| Gaps over 1.1 mm at the six sections | one, 1.20 | none |
| The line's ring, turns in height over 0.05 mm | 673 | 29 |

Before is the program as it was with links across and down (decision 0004,
amended). The rings below the line, and the level rings, turn once a lap; the
three blended rings nearest the line still 38 to 75 times, 0.07 to 0.10 mm
(below).

The same job with the floor's group removed, inside out: 40.12 m and 9 lifts
before, 40.96 m and the last retract now, the gaps at the six sections as
they were; cut outside in, 40.17 m and 10 lifts before, 40.90 m and 2 now.
The job cut outside in with the floor avoided: 39.78 m and 46 lifts before,
40.52 m and 36 now.

On a bowl in the tests widest higher on one side than the other, a 4 mm ball,
a stepover of 0.8 (`run::tests_bowl`): the band's measured spacing within 1%
of its width, every pass off the floor's edge a closed ring, none further
apart along the outside than 1.03 of a stepover inside out.

On `ops_3d`, cut outside in as a new one starts, no band is blended and no
ring laid below where the tool leaves the part; with rings ramped onto along
themselves, a new Scallop, the same with the dome avoided and the same with
only the pocket's floor machined lift once, 3 times and twice where they
lifted once, 5 and 3 times. Contour's programs, whose rings are level, are as
they were.

## Still not done

- The three blended rings nearest the line, within about 3 mm of it along
  the surface, still turn in height 38 to 75 times a lap, 0.07 to 0.10 mm,
  every 3 to 7 mm: they run where the tool's path stands nearly upright
  beside the bulge's widest, and take their heights from the grid there, its
  nodes dropped on the same facets.
- Outside in with an inner edge, the field is measured from both edges, and
  each level's ring by the floor and its ring by the far edge are cut one
  after the other: on the bowl 29 lifts between the two.
- Up the inside wall each ring still rose onto the flat rim and dropped back
  onto the wall, as before. Inside out it no longer does
  ([decision 0039](0039-scallop-zones-at-a-sharp-rim.md)).
- With the floor's group removed, the innermost rings still turn to a point
  at the ends of the lens-shaped floor, as before.
