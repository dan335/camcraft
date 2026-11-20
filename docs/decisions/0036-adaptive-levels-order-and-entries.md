# 0036 — Adaptive clearing's levels, order and entries

**Status:** decided, 2026-11-19 · **Milestone:** 0.10

## Question

A user's part roughed by area, with a fine stepdown, came out of adaptive
clearing in an order no machinist would choose: into one area, out to the
other with the first area's fine levels cut between the second's passes, back,
and the first area's fine levels from its deeper slabs after the second's
first. A third of its lines were steps a tenth of a millimetre long, each
turned 45°, round the same octagon a quarter of a millimetre across, cutting
nothing. Its last slab was the sliver of depth the stepdown did not divide,
and cost a whole lap; its helixes were a quarter of their size, below
Fusion's least, dozens of turns each. A program a machinist reads as logical goes into each area once a
slab, clears it, steps up its wall, goes down to the next slab, and leaves.
What made it otherwise?

## What was found

- **A pass was placed in the area its first point was in, or else the area
  whose box was nearest.** A fine level cuts the terrace round its slab's area,
  outside it, so its first point is in no area; and an area that goes round
  another, a channel round an island, has a box that holds the other whole.
  The channel won, at no distance, for most of the inner area's fine levels.
- **Two levels a hair apart.** A face nearly level, a flat level on it, and a
  fine level counted up from the slab a hair below it. The lower
  keeps off the face, leaving it standing as a ridge narrower than the tool for
  the flat level to take alone. On material narrower than the load no heading
  takes enough: the steering turned as hard as it may toward it at every step,
  halving its step as it turned, and so went round a regular octagon of the
  shortest steps, exactly back where it began, until its steps ran out.
- **The last slab took what was left of the depth**, however thin.
- **The helix was halved and halved again where it had no room**, to a
  quarter, below Fusion's own Minimum Ramp Diameter,
  max(0.5 × (D − 2 × corner radius), 0.25 × D)
  ([decision 0012](0012-transitions-and-entries-from-fusions-defaults.md)).
- **Every level started nearest the lower left corner of its box**, wherever
  the tool was.

## Decision

- **A pass is in the area most of it is in**: points along it, each in the
  piece of its slab's level it is deepest in, or nearest the edge of, outside
  them all; never a box. The area above a piece is the one a point well inside
  it is in, as where the tool may be only shrinks going down. Within an area, a
  slab's level, then its fine and flat levels bottom up, then the next slab
  down, as [decision 0007](0007-adaptive-clearing.md) meant.
- **Slabs are even**: the fewest no more than the stepdown, all as deep, as a
  2D cut's even stepdowns are. Never more slabs than a stepdown apart from the
  top makes, and never a sliver. No new setting.
- **Fine levels up to a flat are even.** Between a slab's level and a flat or
  peak above it in the slab, and between two flats, as many as keep each step
  within the fine stepdown, evenly spaced; above the highest, a fine stepdown
  apart up from it, as before. Counted up from below, the last could fall
  anywhere short of the flat, as close as a hair, and the ladder ran on past
  it, its rungs a hair above the flat's own. Leaving out a fine level too near
  a flat by the flat's minimum stepdown, the rule that leaves a flat out near
  the level above read from the other side, was weighed: that setting is off
  unless set, so it would not have fired where it was needed, and a threshold
  of its own would be a second one. Even spacing needs none.
- **A pass going round on the spot ends**: one that turns a full circle within
  the tool's radius along it without once taking the load, and comes back to
  within a step of where the circle began. A loop in a narrow slot turns as
  tightly, and may take less than the load all the way round, but moves on,
  and is not ended.
- **Material narrower than the tool is taken along it.** Where a pass went
  round on the spot, and what it went round on is nowhere as wide as the tool,
  the tool's centre goes round it as far out as leaves the load to cut, within
  where it may be, until all of it is in reach of where it has been: along one
  side of a strip, round one edge of a ring; then again the load further in,
  until none is left. A sliver no wider than the load is taken in one. Where
  the tool may not stand so far in, it goes round further out, taking less.
  Each way starts where the tool just touches the material, as a pass does.
  Not where that would cut into other material: that is left to the passes
  after, which may start where the one that went round started, going on from
  what it left; from where two have gone round on the spot, none starts
  again. (Never started there again, a 2D pocket's corners were left to
  plunges and lifts, and it fed a third further than Fusion's.) What is left
  with no way in from the side, if narrower than the tool, is taken the same
  way rather than gone down into: a pass ended going round on the spot where
  the room narrows to nothing can leave such a crumb, which no helix or ramp
  fits. The material of a closed area not yet gone into is as wide as
  the tool, as it reaches the tool's radius past where the tool may be in it,
  so this never takes the place of a helix.
- **The helix is never smaller than Fusion's least.** Halved from Fusion's
  Helical Ramp Diameter where there is no room, then the least itself
  (`AdaptiveParams::least_helix_radius`, the operation's
  `least_helix_diameter`). Where not even that fits, it ramps along the area:
  back and forth along a stretch of its middle, near its deepest point, as long
  as the largest helix goes round in one turn or as long as the middle is, and
  no shorter than the least helix is across; at the ramp angle and no more than
  the ramp stepdown in one way along, then once along it at the level, as a
  helix ends with a turn at the level. Fusion's helix ramp type ramps along a
  pocket's rings where its helix does not fit; adaptive has no rings, so it
  ramps along the area's middle. Only where there is no such stretch is the
  area left, and said.
- **A fine or flat level starts nearest where the level below it in its slab
  ended**, where the tool is. A slab's own level is worked out at the same time
  as the others, before the one above it has ended, so it still starts where
  its helix has most room, or at the corner of its box.

## Measured

Tests on parts built in code (`strategy/adaptive/tests.rs`): two pockets with
drafted walls, and a pocket in an island with a channel round it, are each cut
one area after the other, each slab's level and then its fine levels bottom
up; the program is the same on one core and on eight. A rim a hair above where
a fine level would have fallen has one level, none within a millimetre of it,
and no pass at it goes round on the spot; a free-standing ring of material
0.4 mm wide is taken in a bounded number of steps; a slot too narrow for the
least helix is ramped into along it, no steeper than the ramp angle.

On the user's job, not in the repository, as it runs, before and after:

| | before | after |
|---|---:|---:|
| lines | 169,671 | 109,213 |
| fed, mm | 63,097 | 61,839 |
| of it removing nothing | 23,945 | 19,925 |
| rapid, mm | 5,236 | 5,051 |
| lifts | 59 | 58 |
| time at the feeds | 11.2 min | 11.0 min |
| entries | 8 helixes | 6 helixes, 2 ramps |

The same material comes off, and every check passes. Each area is now cut
once a slab, each slab's fine levels up its wall, one area and then the other;
the octagons are gone, and no level is cut a hair from another. Most of the
lifts left are the climb-only returns across the lobes of a level, which
cutting both ways removes, and the steps from a level up to its fine levels.

## Consequences

- Every Fusion comparison of a finish starts from a new adaptive's roughing,
  whose levels moved; their figures were measured again.
- A slab's own level does not know where the tool will be when it starts.
- A ridge wider than the load, standing free, is still cut down by passes
  steered for the load, the last of it along it.
