# 0008 — Pocket clearing, after Fusion's 3D Pocket Clearing

**Status:** decided, 2026-09-28 · **Milestone:** before the first cut

## Question

Z-level roughing cleared each level with offsets of the reachable region, from
its boundary inward. So its first ring ran along the part's wall at full width,
and its passes were reordered for travel, which could run a ring either way. It
had no climb or conventional setting, and it left a terrace a stepdown deep on
any flat face between levels. Fusion's 3D Pocket Clearing is the same idea, an
offset clearing a level at a time, but without those faults. What does it do
differently, and is it worth a strategy of its own?

## Decision

- **One strategy, renamed.** Z-level roughing *is* offset pocket clearing, so it
  is upgraded and called what Fusion calls it: `"pocket-clearing"` in a job,
  *Pocket clearing* in the 3D menu. There is one job format, so the old name
  is not read.
- **The same rings, from the middle out.** The rings are still the reachable
  region's own offsets, a stepover apart in from its edges, clipped to the
  material. What is further in than each stepover falls into regions as it
  narrows, and they nest into a tree. Each region is cut from its middle out:
  the regions inside it first, nearest next, then its own rings. The last ring
  of each region runs along its walls, a light cut that leaves exactly the
  stock to leave, and the tool finishes one arm of a level before it moves to
  the next. A piece with nothing kept beside it, a level above the part, is
  cut the other way, in from the sweep's edge, where the tool starts in air.
- **Its order is its own.** Travel reordering is off for it (`uses().reorder`),
  as it is for the 2D pocket and adaptive clearing, since reordering would undo
  the middle-out order and the direction.
- **Climb by default.** A ring has its region on its left. Cut from the middle
  out, the material it cuts is outward, on its right; cut inward, on its left.
  Climbing with a clockwise spindle wants it on the right, so a ring is reversed
  where it is on the left. Conventional, or the other spindle, is the other way.
  Helixes and ramps turn the same way.
- **Flat areas, as adaptive's.** Each upward flat face, raised by the stock to
  leave down, adds a level. It is cut after the level just below it, over what
  that left: the stock above it, less the level below's reachable region grown by
  the tool's width that far above its tip. On by default, as adaptive's is.

## Rejected: rings grown out from the walls

The first try grew rings out from the walls alone, the part and the machining
boundary, so that round a boss the rings were parallel to it and went in from
the stock's edge. On the router jig those rings, clipped by the stock's
rectangular edge, came out as open arcs. Each arc cut in the climbing
direction, so the tool went back across the part after each one. There were
2879 passes against 1154, 427 m of rapids against 36, and the roughing took
344 min against 259. Fusion's rings are offsets of the whole region, the
stock's side included, and stay closed; so are these.

## Consequences

On the router jig (`examples/rail-jig.camcraft`), against Z-level roughing:

| | Z-level rough | Pocket clearing, no flat areas | Pocket clearing |
|---|---|---|---|
| Rough passes | 1154 | 1154 | 1483 |
| Removed by the rough | 3,502,775 mm³ | 3,502,774 mm³ | 3,546,808 mm³ |
| Rough time, estimated | 258.8 min | 254.2 min | 268.7 min |
| Left for the finish | 67,206 mm³ | 67,207 mm³ | 23,307 mm³ |
| Columns left over 1 mm | 15,813 | 15,813 | 11,794 |

The estimates are from the programs, at their feeds, with rapids at 5000 mm/min.
With the same levels the new order is a little faster, and removes the same
material. The flat levels add a quarter of an hour, and take off two thirds of
what the finish was left.

- The first ring of each region into full material is a slot, which a ramp goes
  down into, as in Fusion's. Holding the load constant is adaptive clearing's.
- Still not Fusion's: no smoothing of sharp corners, no finishing pass, and no
  choice of cutting a pocket from the outside in.
