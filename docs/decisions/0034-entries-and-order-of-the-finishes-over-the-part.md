# 0034 — Entries and order of the finishes over the part

**Status:** decided, 2026-11-19 · **Milestone:** 0.10

> **Amended by [decision 0035](0035-scallop-over-every-slope.md).** Scallop is
> not reordered: it keeps its field's order, each piece taken from its nearer
> end, and it has no rings round the walls, so what is said of Scallop's order
> and of its rings below is as it was decided here, not as it is.

## Question

On `ops_3d` with faces avoided ([decision 0033](0033-surface-groups.md)),
much of what camcraft's Parallel, Scallop and Spiral fed was ramps, and
Scallop and Spiral lifted dozens of times, with no groups at all as much as
with them. Its Flat ramped along every ring. Where did the feed and the lifts
come from, and what should a finish over the part do into a pass and between
its pieces?

## What was found

Each job roughs the part with a new adaptive clearing and a 6 mm flat, leaving
half a millimetre, then makes one new finish. camcraft before:

| Program                 | Lifts | Fed (mm) | of which ramps |
|-------------------------|------:|---------:|---------------:|
| `parallel`              |   1   | 11,886   |   117          |
| `parallel_avoid_dome`   |  16   | 15,303   | 4,635          |
| `parallel_avoid_dome_0` |  10   | 14,117   | 3,382          |
| `parallel_floor_only`   |  13   |  3,635   | 2,631          |
| `parallel_floor_leave`  |   1   | 11,801   |   114          |
| `parallel_avoid_top`    |   2   |  6,500   |   306          |
| `scallop`               |  44   | 19,662   | 8,065          |
| `scallop_avoid_dome`    |  83   | 29,718   | 19,113         |
| `scallop_floor_only`    |  45   | 10,649   | 9,517          |
| `spiral`                |  71   | 36,032   | 23,070         |
| `spiral_avoid_dome`     | 121   | 46,658   | 34,601         |
| `flat`                  |   5   |  2,088   |   716          |
| `flat_avoid_dome`       |   4   |  1,540   |   215          |

Fed is everything at a feed, ramps included; a lift is a rapid straight up.
Two causes:

- **Entries.** camcraft ramped at 2° into every pass that started in
  material, from the safe distance above the highest material anywhere under
  the tool along the pass, so beside an avoided dome it came down several
  millimetres along each piece, back and forth along the short ones. Flat
  ramped along every ring. Pencil, Project and Morph had no ramp: each comes
  down onto the top of a vertical lead a tenth of the tool.
- **Order and direction.** camcraft's Scallop, Spiral, Radial and Morphed
  Spiral were not reordered at all, and cut every piece the way it was laid.
  Where a lap is cut by the boundary, by the steep ground left to the rings
  round the walls ([decision 0014](0014-walls-for-every-finish-over-the-part.md))
  or by a face avoided, the next piece in line was across the part: the linker
  refused 45 of Scallop's links as longer than its stay-down distance, and 94
  of Spiral's, and 59 of Spiral's more as a steep drop from a piece ending at a
  wall's top to the next starting at its foot.

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
  material was tried and measured (below).
- **A new Flat comes straight down.** Flat offers a ramp along its ring or a
  plunge (`linking.entry`); a new one plunges, as the finishes over the part
  now come in, and a job file that leaves the entry out still ramps, as it
  always has (`StrategySpec::fresh_entry`).
- **The nearest next.** Scallop, Spiral, Radial and Morphed Spiral are
  reordered as Parallel is (`Uses::reorder`), after the cuts round faces
  avoided and with the rings round the walls among them, ties going to the
  order laid. `linking.reorder = false` cuts them in the order laid out,
  outside in or inside out as their order asks.
- **Turned round where it cuts both ways.** A piece is reversed in ordering
  wherever its operation cuts both ways (`StrategySpec::cuts_both_ways`, read
  by `Operation::reverses`), not only where `linking.allow_reverse` asks:
  parallel's passes, which alternate already, and those of a scallop, spiral
  or morphed spiral going both ways, or of a radial whose spokes do. The rule
  is one method on the strategy, read wherever passes are reordered. A
  spiral both ways turns to climb, and each piece the part cuts it into is
  taken from its nearer end, which is what `CutDirection::BothWays` already
  said for a finishing pass. Asked to cut one
  way, a piece is cut as laid and the tool goes back to its start. A closed
  ring is never turned round.
- **A new Spiral and Morphed Spiral cut both ways** (`StrategySpec::fresh`), and
  write it out; a job file that leaves the direction out still climbs. Scallop
  and Radial start both ways already.

## Considered and not chosen

- **Keeping the ramp.** It is gentler than a plunge into deep material, but over
  a roughing's half millimetre it came down along the pass from as high as the
  material anywhere under the tool, several millimetres beside a wall or an
  avoided face: more than half of all Spiral fed. The tool plunges into no more
  than what is left over the pass's start, at the plunge feed.
- **A ramp only over deep material.** How deep is deep would be a rule of its
  own, with nothing to choose it by. A roughing that leaves terraces deep
  enough to want one is a roughing to fix.
- **The lead through the material.** The arc would go through the roughing's
  half millimetre at the cutting feed. Tried for all eight finishes over the
  part: with only the floor machined, Parallel fed 1,012 mm where it feeds
  1,006 coming straight down; every Project fed half a percent further; with
  no groups, nothing changed. The arc goes back a tenth of the tool from where
  the pass starts, and down through what stands there, which camcraft's passes
  stop short of beside a face avoided: it takes what the roughing left over
  the fillets beside the floor.
- **The order laid**, outside in or inside out. It stays a setting; taken the
  nearest next, the pieces lift far less (below).

## Figures after

The same jobs, every check passing, none cutting the part:

| Program                 | Lifts | Fed (mm) |
|-------------------------|------:|---------:|
| `parallel`              |   1   | 11,055   |
| `parallel_avoid_dome`   |  11   | 10,102   |
| `parallel_avoid_dome_0` |   9   | 10,194   |
| `parallel_floor_only`   |   9   |  1,006   |
| `parallel_floor_leave`  |   1   | 10,887   |
| `parallel_avoid_top`    |   3   |  5,892   |
| `scallop`               |   2   | 11,390   |
| `scallop_avoid_dome`    |  15   | 10,346   |
| `scallop_floor_only`    |  12   |  1,066   |
| `spiral`                |   2   | 10,804   |
| `spiral_avoid_dome`     |   8   |  9,857   |
| `flat`                  |   5   |  1,396   |
| `flat_avoid_dome`       |   4   |  1,332   |

Nothing is ramped onto. Spiral feeds a third as far as it did, Scallop with
only the floor machined a tenth. Pencil, Project, Morph, Contour and Ramp are
as they were. Radial and Morphed Spiral are not on `ops_3d`; on a synthetic
part cut into short pieces by faces avoided, each lifts for fewer than half
its pieces and ramps onto none (`crates/camcraft-core/src/run/tests_surfaces.rs`).

Taken in the nearest order and from either end, the pieces are joined by
other links, which stay down over the part where the tool lifted before, take
a little of what the roughing left, and leave fewer lead-outs past the ends
of pieces. Parallel with the top avoided lifts 3 times where it lifted 2, two
of its links in the new order resting on the top.

What still lifts, by the linker's reason for not staying down:

| Program                 | Lifts | Too far | Over a face avoided | A steep drop |
|-------------------------|------:|--------:|--------------------:|-------------:|
| `parallel`              |   1   |  0      |  0                  | 0            |
| `parallel_avoid_dome`   |  11   |  0      |  9                  | 1            |
| `parallel_avoid_dome_0` |   9   |  0      |  8                  | 0            |
| `parallel_floor_only`   |   9   |  0      |  8                  | 0            |
| `parallel_floor_leave`  |   1   |  0      |  0                  | 0            |
| `parallel_avoid_top`    |   3   |  0      |  2                  | 0            |
| `scallop`               |   2   |  2      |  0                  | 2            |
| `scallop_avoid_dome`    |  15   |  4      | 10                  | 2            |
| `scallop_floor_only`    |  12   |  0      | 11                  | 0            |
| `spiral`                |   2   |  0      |  0                  | 2            |
| `spiral_avoid_dome`     |   8   |  0      |  7                  | 1            |

A lift is a rapid straight up in the program; a refused link that only feeds
up to the safe distance and across, or the last lift to the clearance height,
counts in one column and not the other. Before, the same links were refused
as too far 45 times on Scallop and 94 on Spiral, and as a steep drop 59 times
on Spiral.

## Still not done

- **Over a face avoided** is most of what still lifts. Neither a hop over
  what is avoided nor turning back at its clearance and staying down is
  built.
- **Radial and Morphed Spiral** are measured on synthetic parts only.

## Amended 2026-11-21: a lower pass near the last is not lifted to

A pass that starts lower than the tool came off the last, within the
stay-down distance, is no longer lifted to and rapided down onto: the tool
goes across at the height it came off and straight down at the plunge feed
onto the pass's vertical lead, where the lead is clear, or onto the pass, the
same way in as after a lift
([decision 0004, amended](0004-linking-and-entries.md#amended-2026-11-21-across-and-down-for-the-finishes-over-the-part-too)).
The figures above are as they were measured; since then Scallop lifts once
with no groups and 5 times with the dome avoided, and Spiral once.
