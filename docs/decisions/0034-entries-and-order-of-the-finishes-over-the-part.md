# 0034 — Entries and order of the finishes over the part

**Status:** decided, 2026-11-19 · **Milestone:** 0.10

## Question

Held to Fusion's programs for `ops_3d` with faces avoided
([decision 0033](0033-surface-groups.md)), camcraft's Parallel, Scallop and
Spiral fed two to six times as far as Fusion's, and Scallop and Spiral lifted
dozens of times where Fusion's lifted a handful, with no groups at all as much
as with them. Its Flat fed four times as far. Where does the difference come
from, and which of Fusion's own settings does camcraft depart from?

## What was found

Each job roughs the part with a new adaptive clearing and a 6 mm flat, leaving
half a millimetre, then makes one new finish, and cuts Fusion's program from the
same material (`crates/camcraft-session/src/compare/tests_*.rs`). camcraft
before, against Fusion's:

| Program                 | Lifts, camcraft / Fusion | Fed, camcraft / Fusion (mm) | of which ramps |
|-------------------------|-------------------------:|----------------------------:|---------------:|
| `parallel`              |   1 / 1                  | 11,886 / 8,193              |   117          |
| `parallel_avoid_dome`   |  16 / 15                 | 15,303 / 7,281              | 4,635          |
| `parallel_avoid_dome_0` |  10 / 2                  | 14,117 / 7,423              | 3,382          |
| `parallel_floor_only`   |  13 / 2                  |  3,635 / 909                | 2,631          |
| `parallel_floor_leave`  |   1 / 1                  | 11,801 / 8,157              |   114          |
| `parallel_avoid_top`    |   2 / 2                  |  6,500 / 3,122              |   306          |
| `scallop`               |  44 / 1                  | 19,662 / 9,860              | 8,065          |
| `scallop_avoid_dome`    |  83 / 8                  | 29,718 / 9,262              | 19,113         |
| `scallop_floor_only`    |  45 / 3                  | 10,649 / 1,173              | 9,517          |
| `spiral`                |  71 / 5                  | 36,032 / 8,317              | 23,070         |
| `spiral_avoid_dome`     | 121 / 6                  | 46,658 / 7,365              | 34,601         |
| `flat`                  |   5 / 4                  |  2,088 / 457                |   716          |
| `flat_avoid_dome`       |   4 / 2                  |  1,540 / 430                |   215          |

Fed is everything at a feed, ramps included; a lift is a rapid straight up.
Two causes, both departures from Fusion's own settings, read from a new
operation of each strategy through Fusion's API:

- **Entries.** Fusion's Parallel, Scallop, Spiral, Radial and Morphed Spiral
  have no ramp setting at all, nor do its Pencil, Project and Morph. Every
  entry in their programs is a rapid down to above the material, a plunge at
  the plunge feed onto the top of the vertical lead (`entry_verticalRadius`, a
  tenth of the tool), and the lead at the cutting feed onto the pass, through
  whatever the roughing left: Scallop's with the dome avoided plunges from
  Z19.707 to Z10.838 and arcs onto its pass at Z10.24. camcraft ramped at 2°
  into every pass that started in material, from the safe distance above the
  highest material anywhere under the tool along the pass, so beside an avoided
  dome it came down several millimetres along each piece, back and forth along
  the short ones. Fusion's Flat starts with `doRamp = false`: straight down
  onto each ring. camcraft's ramped along every ring.
- **Order and direction.** Fusion's Scallop, Spiral, Radial and Morphed Spiral
  start with `linkInsideOutside = 'dont care'`, shown as *Shortest machining
  distance* (the others *Outside -> in* and *Inside -> out*), and every one of
  them and Parallel with `direction = 'both ways'`. camcraft's four were not
  reordered at all, and cut every piece the way it was laid. Where a lap is cut
  by the boundary, by the steep ground left to the rings round the walls
  ([decision 0014](0014-walls-for-every-finish-over-the-part.md)) or by a face
  avoided, the next piece in line was across the part: the linker refused 45 of
  Scallop's links as longer than its stay-down distance, and 94 of Spiral's, and
  59 of Spiral's more as a steep drop from a piece ending at a wall's top to the
  next starting at its foot.

Reordering had been turned off for Scallop when it came (decision 0009), when
passes were grouped by the height each starts at: on a flat, where every ring
starts level, they would be one group, and the nearest-first order would
shuffle the rings nested in from the boundary and could turn them round. Since
then a finish over the part is reordered as one group, whatever height its
passes start at, a closed ring is never reversed, and scallop starts each ring
nearest where the last ended, so on an uncut part the order laid is the nearest
next already. Spiral, Radial and Morphed Spiral took the same setting when they
came (decision 0011), as patterns cut in the order laid out.

## What was decided

- **No ramp for the finishes over the part.** Parallel, Scallop, Spiral, Radial
  and Morphed Spiral take no ramp, as Pencil, Project and Morph already did
  (`Uses::ramp`). A job file that still carries ramp settings for one of them
  reads as before; the settings are not read, as no setting a strategy does not
  use is.
- **Straight down, onto the lead where it is clear.** Into each pass the tool
  rapids to the safe distance above the material and plunges at the plunge
  feed: onto the top of the vertical lead and down it where the lead is clear
  of the part and of the material, onto the pass itself where it is not, as
  Pencil, Project and Morph came in already. So most of these entries, over
  what a roughing left, are a plunge onto the pass; leading through the
  material, as Fusion's does, was tried and measured (below).
- **A new Flat comes straight down.** Flat offers a ramp along its ring or a
  plunge (`linking.entry`), as Fusion's Flat has its Ramp; a new one plunges, as
  Fusion's starts, and a job file that leaves the entry out still ramps, as it
  always has (`StrategySpec::fresh_entry`).
- **The nearest next.** Scallop, Spiral, Radial and Morphed Spiral are
  reordered as Parallel is (`Uses::reorder`), after the cuts round faces
  avoided and with the rings round the walls among them, ties going to the
  order laid. `linking.reorder = false` cuts them in the order laid out,
  outside in or inside out as their order asks: Fusion's other two.
- **Turned round where it cuts both ways.** A piece is reversed in ordering
  wherever its operation cuts both ways (`StrategySpec::cuts_both_ways`, read
  by `Operation::reverses`), not only where `linking.allow_reverse` asks:
  parallel's passes, which alternate already, and those of a scallop, spiral
  or morphed spiral going both ways, or of a radial whose spokes do. The rule
  is one method on the strategy, read wherever passes are reordered. A spiral both ways turns to climb, and each piece
  the part cuts it into is taken from its nearer end, which is what
  `CutDirection::BothWays` already said for a finishing pass. Asked to cut one
  way, a piece is cut as laid and the tool goes back to its start. A closed
  ring is never turned round.
- **A new Spiral and Morphed Spiral cut both ways,** as Fusion's start
  (`StrategySpec::fresh`), and write it out; a job file that leaves the
  direction out still climbs. Scallop and Radial start both ways already.

## Considered and not chosen

- **Keeping the ramp.** It is gentler than a plunge into deep material, but
  none of these strategies of Fusion's has one, and over a roughing's half
  millimetre it came down along the pass from as high as the material anywhere
  under the tool, several millimetres beside a wall or an avoided face: more
  than half of all Spiral fed. The tool plunges into no more than what is left
  over the pass's start, at the plunge feed.
- **A ramp only over deep material.** It would keep a setting Fusion's does not
  have, and choosing how deep is deep would be camcraft's own rule, held to
  nothing. A roughing that leaves terraces deep enough to want one is a
  roughing to fix.
- **The lead through the material, as Fusion's.** Fusion's arc goes through
  the roughing's half millimetre at the cutting feed. Tried for all eight
  finishes over the part, it brought none of them nearer Fusion's material:
  with only the floor machined, Parallel removed 0.87% more than Fusion where
  it removes 0.59% more coming straight down, 55.1% of the columns alike to
  55.2%, and fed 1,012 mm to 1,006; with the dome avoided and no clearance,
  71.9% alike to 72.0%; with no groups, nothing changed. Pencil with the 6 mm
  ball went from 74.5% alike to 74.1% and 4.5% more removed to 4.8%, and every
  Project fed half a percent further. The arc goes back a tenth of the tool
  from where the pass starts, and down through what stands there, which
  camcraft's passes stop short of beside a face avoided: it takes what the
  roughing left over the fillets beside the floor.
- **The order laid.** It is Fusion's *Outside -> in* and *Inside -> out*, and
  stays a setting, but Fusion's new operation is *Shortest machining distance*.

## Figures after

The same jobs, every check passing, neither program cutting the part:

| Program                 | Lifts, camcraft / Fusion | Fed, camcraft / Fusion (mm) | Alike, before / after | Removed against Fusion's, before / after |
|-------------------------|-------------------------:|----------------------------:|----------------------:|-----------------------------------------:|
| `parallel`              |   1 / 1                  | 11,055 / 8,193              | 74.7% / 74.4%         | +1.8% / +1.8%                            |
| `parallel_avoid_dome`   |  11 / 15                 | 10,102 / 7,281              | 70.0% / 69.7%         | +1.9% / +1.9%                            |
| `parallel_avoid_dome_0` |   9 / 2                  | 10,194 / 7,423              | 72.3% / 72.0%         | +1.6% / +1.6%                            |
| `parallel_floor_only`   |   9 / 2                  |  1,006 / 909                | 55.1% / 55.2%         | +0.38% / +0.59%                          |
| `parallel_floor_leave`  |   1 / 1                  | 10,887 / 8,157              | 65.5% / 65.1%         | +1.9% / +1.9%                            |
| `parallel_avoid_top`    |   3 / 2                  |  5,892 / 3,122              | 73.8% / 73.7%         | +1.9% / +1.9%                            |
| `scallop`               |   2 / 1                  | 11,390 / 9,860              | 89.2% / 89.2%         | +0.03% / +0.03%                          |
| `scallop_avoid_dome`    |  15 / 8                  | 10,346 / 9,262              | 61.7% / 61.8%         | -1.1% / -1.2%                            |
| `scallop_floor_only`    |  12 / 3                  |  1,066 / 1,173              | 42.9% / 42.8%         | -11.3% / -11.6%                          |
| `spiral`                |   2 / 5                  | 10,804 / 8,317              | 84.9% / 85.8%         | +0.6% / +0.6%                            |
| `spiral_avoid_dome`     |   8 / 6                  |  9,857 / 7,365              | 80.6% / 82.7%         | +0.7% / +0.5%                            |
| `flat`                  |   5 / 4                  |  1,396 / 457                | 65.9% / 65.9%         | -4.3% / -4.3%                            |
| `flat_avoid_dome`       |   4 / 2                  |  1,332 / 430                | 70.5% / 70.5%         | +210% / +210%                            |

Nothing is ramped onto. Spiral feeds a third as far as it did, Scallop with
only the floor machined a tenth, and less than Fusion's. Pencil, Project, Morph,
Contour and Ramp are as they were. Radial and Morphed Spiral have no Fusion
programs; on a synthetic part cut into short pieces by faces avoided, each
lifts for fewer than half its pieces and ramps onto none
(`crates/camcraft-core/src/run/tests_surfaces.rs`).

Some figures are a little worse, and none of it is the entries. Taken in the
nearest order and from either end, the pieces are joined by other links,
which stay down over the part where the tool lifted before and take a little
of what the roughing left where Fusion's do not, and leave fewer lead-outs
past the ends of pieces: Parallel's columns alike are 0.1 to 0.4% fewer, its
floor alone 0.59% more removed than Fusion's where it was 0.38%, Scallop with
only the floor machined has its 95th percentile 0.04 mm higher, and Parallel
with the top avoided lifts 3 times where it lifted 2, two of its links in the
new order resting on the top.

What still lifts, by the linker's reason for not staying down (Fusion's lifts
for comparison):

| Program                 | Lifts | Too far | Over a face avoided | A steep drop | Fusion |
|-------------------------|------:|--------:|--------------------:|-------------:|-------:|
| `parallel`              |   1   |  0      |  0                  | 0            |  1     |
| `parallel_avoid_dome`   |  11   |  0      |  9                  | 1            | 15     |
| `parallel_avoid_dome_0` |   9   |  0      |  8                  | 0            |  2     |
| `parallel_floor_only`   |   9   |  0      |  8                  | 0            |  2     |
| `parallel_floor_leave`  |   1   |  0      |  0                  | 0            |  1     |
| `parallel_avoid_top`    |   3   |  0      |  2                  | 0            |  2     |
| `scallop`               |   2   |  2      |  0                  | 2            |  1     |
| `scallop_avoid_dome`    |  15   |  4      | 10                  | 2            |  8     |
| `scallop_floor_only`    |  12   |  0      | 11                  | 0            |  3     |
| `spiral`                |   2   |  0      |  0                  | 2            |  5     |
| `spiral_avoid_dome`     |   8   |  0      |  7                  | 1            |  6     |

A lift is a rapid straight up in the program; a refused link that only feeds
up to the safe distance and across, or the last lift to the clearance height,
counts in one column and not the other. Before, the same links were refused
as too far 45 times on Scallop and 94 on Spiral, and as a steep drop 59 times
on Spiral.

## Still not Fusion's

- **Over a face avoided** is most of what still lifts: Fusion's Parallel
  plunges onto the next piece too, but its Scallop and Spiral turn back at the
  clearance and stay down, and its pieces run on past where the tool touches a
  face machined and one avoided at once ([decision 0033](0033-surface-groups.md)).
  Neither a hop over what is avoided nor turning back at its clearance is
  built.
- **The rings round the walls** ([decision 0014](0014-walls-for-every-finish-over-the-part.md))
  are most of what camcraft still feeds past Fusion's with no groups: a third
  further for Parallel and Spiral, a sixth for Scallop.
- **Fusion's lead through the stock.** camcraft comes down onto the pass
  itself wherever the lead's top is in material (above).
- **Spiral's order.** A new one of camcraft's is cut from its outer radius
  in, Fusion's from the middle out.
- **Flat** still feeds three times as far as Fusion's, which is not yet looked
  into.
- **Radial and Morphed Spiral** are held to no programs of Fusion's.
