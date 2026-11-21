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
- **What it costs.** It feeds 44.10 m where it fed 39.37: 3.24 m more down
  the outside above Z-34 and 1.49 m more below it, the inside and the rim
  as they were. A closed ring of the band is as long all round however thin
  the band is there, where the arcs it replaces stopped short: half the
  band's spread of widths over the stepover, in laps, about 6 laps of 0.49 m,
  as reckoned before it was built, and below the line two laps more. Started a
  stepover from the line rather than half, the band took a ring more and the
  field a ring less: 44.05 m.

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

| | 0.10.0 | now |
|---|---|---|
| Fed | 39.37 m | 44.10 m |
| of it, inside / rim | 16.89 / 2.31 m | 16.89 / 2.31 m |
| of it, outside above Z-34 / below | 13.23 / 6.95 m | 16.47 / 8.44 m |
| Rapid | 1.70 m | 0.35 m |
| Lifts | 28 | 9 |
| Passes not closed rings | 19 arcs and 6 pieces of level rings | none |
| Gaps over 1.1 mm at the six sections | one, 1.20 | none |

Of the 9 lifts, one is the last retract; two are between whole rings on the
steepest part of the outside, and five between rings one below another under
the line, the edge's and the four below it and the first level ring: each
next ring starts under the last, and the way down falls steeper than a
plunge and further than a cut step, which the linker lifts for. One is
between the first two level rings, whose ramp is lost after the level rings
are laid; why is not found yet.

The same job with the floor's group removed, inside out: 40.14 m and 12 lifts
before, 41.46 m and 10 lifts now, the gaps at the six sections as they were;
cut outside in, 40.17 m and 10 lifts before, 41.38 m and 3 now. The job cut
outside in with the floor avoided: 39.78 m and 46 lifts before, 41.00 m and 39
now.

On a bowl in the tests widest higher on one side than the other, a 4 mm ball,
a stepover of 0.8 (`run::tests_bowl`): the band's measured spacing within 1%
of its width, every pass off the floor's edge a closed ring, none further
apart along the outside than 1.03 of a stepover inside out.

On Fusion's `ops_3d` the three Scallop comparisons are as they were, move
for move: cut outside in, as a new one starts, the field is measured from
every edge and no band is blended, and no ring is laid below where the tool
leaves the part.

## Still not done

- The lifts between rings one below another down a steep wall, under the line
  and on the steepest part of the outside: a finish over the part that
  crossed at the height it came off and came down the short way would take
  them (the linker's).
- Outside in with an inner edge, the field is measured from both edges, and
  each level's ring by the floor and its ring by the far edge are cut one
  after the other: on the bowl 29 lifts between the two.
- Up the inside wall each ring still rises onto the flat rim and drops back
  onto the wall, as before.
- With the floor's group removed, the innermost rings still turn to a point
  at the ends of the lens-shaped floor, as before.
