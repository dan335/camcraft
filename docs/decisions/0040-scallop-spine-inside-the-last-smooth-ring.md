# 0040 — Inside the last smooth ring, Scallop's rings go out from a spine

**Status:** decided, 2026-11-21 · **Milestone:** 0.11

## Question

Cut inside out with nothing avoided, a user's bowl's inside is a zone with
no inner edge (decision 0039), so its rings are measured in from its one
edge, where the tool starts rolling over the rim. Offsets inward of a smooth
oval get a corner at each end once they are deeper than the oval's end is
round, and keep it to the middle: the user saw "sharp points on both ends".
24 of the bowl's 121 closed rings turned more than 30° over a millimetre,
the sharpest 162°. With the floor avoided the rings grow out from the
floor's smooth edge and have none. Where should the rings in the middle of
a zone with no inner edge come from?

## What was measured

On that bowl, a 9.525 mm ball, a stepover of 0.973, inside out, the floor's
group removed:

- **The outer rings are smooth.** Going in from the rim, the rings are smooth
  eggs, parallel to the wall's top, the wall's varying length taken up by
  them, down to the field's level 30, `R*`; the next has a corner (no turn
  sharper than the crease angle, 30°, between chords a stepover long either
  side of any point, from level 31 on).
- **Inside `R*` a stadium fits.** Its depth `D*`, the furthest any node
  inside is from it, is the field's own value less its level. Split at its
  two points furthest apart along the surface, `R*` has two sides; the line
  as far from the one as the other, along the surface, is its middle, one
  line from end to end. With `D*` of its length left off each end, the spine
  is 39.0 mm long, from (134.0, 75.1) to (95.2, 72.0).
- **The band is narrow.** Measured out from the spine, `R*` is 21.00 mm from
  it at its nearest, in the south-west, to 26.93 at its furthest, the east
  end: a spread of 5.9 mm, what an egg against a stadium costs. 21 rings a
  stepover apart go round the spine, and 7 blend from the last of them into
  `R*`.

## What was decided

- **The rings in from an edge are kept as long as they are smooth**
  (`rings::last_smooth`). In either order, going in down each chain of the
  field's regions, `R*` is the parent of the first ring with a corner, where
  that parent's lines are one closed ring with its region inside it: no
  hole, no arc, no edge of the ground or the area inside it. A chain whose
  outermost ring already has a corner, as a pocket with corners, has none.
  It is asked of any field's tree, a zone's or the whole ground's, and a
  field measured out from inner edges has none: its rings have their
  regions outside them.
- **Inside `R*`, the rings go out from its spine** (`scallop::spine`,
  `rings::out_from_a_spine`): the spine, an open pass, first; rings a
  stepover apart out from it; then rings blending from the last of them into
  `R*`, with `t = a / (a + b)`, `a` the distance from the spine, marched from
  the nodes beside it, and `b` the distance from `R*`, the field's own less
  its level, as a band blends into a far edge (`blend::between`, shared with
  `blend::band`); then `R*` and the field's rings out from it, as before.
- **Outside in, the same rings, the mirror order** (amended 2026-11-21): the
  rings a spine makes do not depend on the order they are cut in. Chained
  under `R*`, the spine deepest, they are cut `R*` first, then the blended
  rings in, the whole rings round the spine in, and the spine last: the tool
  steps from the innermost ring onto the spine's end, staying down, and
  lifts once, at the end.
- **Otherwise the rings are as they were**: no `R*`; a spine shorter than a
  stepover, as in a round bowl, whose innermost circles turn more sharply
  than the crease angle but whose spine is nought long; a spine leaving
  `R*`; a middle line in pieces; or the band refused (`blend::start`,
  `BAND_LIMIT`).

## Considered and not chosen

- **The whole zone's spine.** Measured out from a spine of the whole zone
  and blended into its edge, as a zone with an inner edge is, the spine the
  shortest way between the two points of the edge furthest apart, less the
  zone's depth at each end. On the bowl the zone is the floor and a wall
  about 30 mm tall whose length varies round it: its depth `D` is 55.7 mm,
  the way 132.5 mm along a diagonal set by the wall's length, the spine
  21.1 mm, and the edge 36.2 to 85.6 mm from it, a band of 49 mm against the
  limit of 19. The band was refused, and nothing changed.
- **Inside `R*`, the shortest way between its two furthest points.** The
  surface up the lower wall pulled the ends round, and a straight way
  between them does not pass between the sides evenly on an egg: the spine,
  37.1 mm, lay off centre and tilted, `R*` 19.05 to 31.53 mm from it, 13
  rings in the band bunched along its north side, and it fed 42.43 m.
- **The ridge of the distance from the edge.** On an ellipse 62 by 34 it
  runs from focus to focus, and the edge is up to 30 mm further from it at
  the sides than at the ends: a band 30 mm wide where the trimmed middle
  gives one of about 4.

## Figures

The bowl with the floor's group removed, inside out, before and now:

| | before | now |
|---|---|---|
| Fed | 40.86 m | 41.65 m |
| of it, inside the rim / rim / outside | 15.67 / 2.44 / 22.74 m | 16.49 / 2.44 / 22.71 m |
| Lifts | 1, the last retract | 1, the last retract |
| Closed rings turning over 30° over 1 mm | 24 of 121, up to 162° | 2 of 122, 47.8° and 33.0° |
| Rings rolling over the rim's edges at a section (`rim.py`) | 1 and 0 | 1 and 0 |
| Most left on the wall's top 10 mm / rim / outside's top (simulated) | 0.785 / 0.026 / 0.049 mm | the same |
| Most left inside below that / volume | 0.168 mm / 105.4 mm³ | 0.168 mm / 98.3 mm³ |

The first two rings round the spine turn as their ends are round: a
stepover, and two. On sections across the spine's middle and along it past
its ends, the rings are 0.97 to 0.98 mm apart where the tool touches the
floor. The rim's rings are the same lines as before; the seam they start
on is where the first closed ring starts, now the first round the spine, so
it runs a different way.

The bowl with its floor avoided, inside out, and both bowls outside in, are
byte for byte as they were, as are `ops_3d` cut either way and a new
Scallop on it, with the dome avoided and with only the floor machined. In
the tests, a shallow
egg-shaped bowl with nothing avoided keeps no ring but the first two round
its spine over the bound, its rings a stepover apart on its floor; a round
bowl and a square pocket make the rings they made; and on a flat egg the
spine is its long axis to within the grid's pitch (`run::tests_bowl`,
`strategy::scallop::tests`).

Since the band's count takes its two distances' gradients as unit
(decision 0038, amended 2026-11-21), the band into `R*` has 6 rings where
7: one triangle just inside `R*`, where the depth's ridge begins, measured
7.21 mm across, its `|∇b|` 0.886, where the band is 6.49 wide at its widest.
The bowl feeds 41.44 m where 41.65; on three sections its band's rings are
0.37 to 1.00 mm apart where the tool touches, where 0.32 to 0.89.

The bowl with the floor's group removed, outside in, before the spine was
laid in that order (bff711a) and with it:

| | before | now |
|---|---|---|
| Fed | 40.43 m | 41.03 m |
| of it, inside the rim | 17.01 m | 17.61 m |
| Lifts | 1, the last retract | 1, the last retract |
| Closed rings turning over 30° over 1 mm | 24 of 121 | 2 of 121, the two round the spine, 48.1° and 30.8° |

The innermost ring round the spine ends 0.87 mm from the spine's east end,
and the tool goes onto it staying down; the spine runs to its west end, and
the tool lifts there, once. Across the spine, on sections square to it
through its middle, the rings are as far apart where the tool touches as
inside out's, gap for gap; along its line past its east end, 0.91 to 1.05
mm, as inside out's. On `ops_3d`, the chain round the block's outside has
an `R*`, at level 15, whose spine is refused, in either order; a new
Scallop on it, with the dome avoided and with only the floor machined, has
none. Those programs, the bowl with its
floor avoided either way, and every program inside out are byte for byte as
they were.

## Still not done

- The band's rings bunch where `R*` is nearest the spine, on the user's bowl
  its south-west side: 6 rings across a band 0.6 mm wide there and 6.5 at its
  widest.
- Where `R*` runs along a crease, as where a floor meets its wall at an
  angle, `R*` and the band's rings next to it turn there by the crease,
  31° to 47° over a millimetre on a bowl in the tests with a 63° wall
  straight off its floor; the test bowl's wall rises off its floor with no
  crease.
- The first two rings round the spine turn as sharply as their ends are
  round.
