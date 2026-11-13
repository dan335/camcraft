# 0007 — Adaptive clearing, after Fusion's 3D Adaptive

**Status:** decided, 2026-09-28 · **Milestone:** before the first cut

## Question

Z-level roughing offsets the reachable region inward by the stepover, so its first
ring into full material is a full slot and every inside corner buries the tool.
Fusion's 3D Adaptive Clearing holds the tool's load constant instead, so it takes
the whole flute length down at a light radial load. How is it done, and can
camcraft do it?

## What Fusion does

Fusion's Adaptive is Freesteel's Adaptive Clearing (Julian Todd and Martin
Dunschen, 2004), which became the HSMWorks kernel Autodesk bought in 2012; it was
never patented, and the idea is Stori and Wright's constant-engagement paths
(2000). Each Z level is a 2D problem in two regions: where the tool's centre may
be, and what is still standing. The tool is stepped forward a little at a time,
each step turned toward or away from the material until the arc of its edge in
material is what the *optimal load* gives; what it sweeps is taken out of what is
standing. A pass ends when it runs out of material or would overload the tool,
and the next starts at the nearest edge of what is left, reached without lifting
where the way is clear. Open sides are entered from outside the stock; a closed
area goes down a helix in its widest place, and is left if none fits. Rough
levels are a stepdown apart; flat area detection adds a level on each flat face,
and fine stepdown steps back up each slab over the terraces left on slopes.

## Decision

- **Written here, from the published method.** FreeCAD's `Adaptive.cpp` is
  LGPL-2.1 and Freesteel's `libactp` GPL; either ported would bind camcraft,
  which is MIT or Apache-2.0. Nothing is taken from their code.
- **Exact regions, not the stock grid.** The path is steered by where material
  is, so that is exact polygons ([decision 0003](0003-grids-only-for-stock.md)):
  the stock's box, less the exact sweep of the path. The allowed region is
  `LevelReach::forbidden`'s complement, as Z-level roughing's.
- **Engagement is measured exactly** (`camcraft_geom::engagement`): the arcs of
  the tool's circle in material, from its crossings with the region's edges and
  the sweep of the last few steps. The target is `acos(1 − load/R)`; the most a
  pass may take on is twice the load's.
- **Passes start where an edge of material begins**, heading along it with the
  material on the climbing side: a pass started part way along an edge runs into
  whatever ends it behind. **Stay-down links are straight**, only where the tool
  stays clear of the material; otherwise the linker lifts, and goes straight down
  into what is cleared. **Adaptive makes its own helixes**, the size it planned,
  at its entries only; the linker's would helix into every re-entry. An entry
  whose pass then finds nothing left to take (the helix's disc held all there
  was) is still cut, as a helix alone: the disc is counted cleared, so it must
  be. The linker arrives at a helix's start, the safe distance above the level
  before, by rapid: that is no plunge, and is not reported as one (CC202).
- **Levels** are rough levels a stepdown apart, each worked on its own and in
  parallel, and a level on each up-facing flat face, raised by the axial stock to
  leave, cut after the level below it and starting from what that level swept.
- **Material and room are kept in tiles**, and every question between passes is
  asked of a window round the tool first: a whole sheet's level is otherwise
  rebuilt for every pass.

## Result

On the lego brick with a quarter-inch flat at a tenth of the tool, no gouges and
no rapid through material, 484,000 mm³ removed against Z-level roughing's 459,000
(its flat levels take the terraces over the brick's top and between its studs).
The studs' channels are cleared as Fusion's are, in arcs advancing into them.

| stepdown | passes | worked out | program, at the feeds |
|----------|--------|------------|-----------------------|
| 6.35 mm  | 1837   | 22–30 s    | 43.5 min              |
| 25.4 mm (the flutes) | 872 | 12 s | 14.5 min          |

Z-level roughing, 6.35 mm down at 0.6 of the tool, is 8.2 min: a full slot at
every level's first ring. Adaptive is for going the whole flute down at a light
load; at a shallow stepdown it only makes more path.

Two things made most of the difference on the way. The linker helixed into every
pass it lifted for, as the tool touches material where a pass starts: about 180 mm
of helix each, 1000 times; adaptive now makes only its own entries' helixes. And
a pass lifted for started and ended against the material, so the linker fed down
the whole wall at the plunge feed: a pass it lifts for now starts and ends a third
of the tool off the material, clear of every simulator cell, and the linker comes
down at rapid to the safe distance (16.6 min of plunging became 4.2).

## Taken since

- **Fine stepdown** (0.3), back up each slab over slopes, seeded as the flat
  levels are.
- **Rest machining** from the operations before it in the setup, as Fusion's
  from previous operations, with its Ignore Stock Less Than. Each earlier
  operation's moves are swept exactly at every level, with the real tool it
  was cut with, as wide as it is that far above its tip, so the grid rule holds:
  what the simulator's stock says is not read. A tool only widens going up, so
  a move far enough below a level to cut it at its full width cuts every level
  above too, and is swept once, from the lowest level up; only the moves close
  below a level are swept for it alone, each piece at its narrowest, so nothing
  is taken as cut that was not. What is left thinner than the stock to ignore
  goes by an opening of half of it. With it on, the operation waits for those
  before it.
- **Minimum cutting radius**: an opening of where the tool may be by it, kept
  inside it. Its inside corners are rounded, so the tool never turns tighter
  there, and the material in them is left, as Fusion's leaves it; nothing
  narrower than the tool and twice the radius is cut, which is Fusion's too.
- **Machine cavities**: off, a level ends where what is left has no way in
  from the side, and what it left is counted apart from what it missed.
- **Order by area**: each pass is placed by the piece of where the tool may be
  at its slab's own level that its start is in, and at every level above;
  where the tool may be only shrinks going down, so those pieces nest. Areas
  are cut depth first, each before the areas below it and those before the
  next beside it, side by side in the order they were first come to. A slab's
  flat and fine levels stay after its own in each area, as each cuts what that
  one left; a level's passes keep their order, so a stay-down link still
  follows the pass it was found after.
- **Stay-down links round what stands between**, through what is cleared: the
  shortest way by the corners of where the tool stands clear of the material,
  where it turns away from itself, one each quarter-radian round a curve,
  within the stay-down distance, every leg held to what a straight link is.

- **Machine shallow areas**: between every two levels of a slab, and its
  highest and the level above, where the terrace between them is wider than
  the maximum shallow stepover, a level halfway; and halfway again, until none
  is, or the levels would come nearer than the minimum shallow stepdown. The
  terrace is where the tool may be at the upper level, a hair below it so a flat
  face there is the part's, and not at the lower, where there is stock: as the
  tool's centre goes, so its width is the horizontal step Fusion measures. Each
  level added cuts only where the terrace it was added for is that wide, and as
  far again as the tool's radius to come at it from: where the tool may be is
  kept to there, never the material, so the tool is never steered into material
  it was told is not there. Material left in crumbs where that edge crosses the
  material within the tool's reach, smaller across than a quarter of the tool,
  is counted left, not an area to go down into. On the serving tray, 32 levels
  were added; the scallop finish after the roughing took 277,406 mm³ where it
  took 327,528.
- **Flat area detection finds peaks**, as Fusion's does: each corner of the
  mesh, found by where it is, that no corner within the tool's width stands
  above, on a face that faces up, is a level as a flat face is. Its **minimum
  stepdown** leaves out a flat or peak level less than it below the level above
  its slab. Every level of a slab cuts into material standing up to that
  level, as nothing above the slab's levels is cut until it is, so that is how
  deep the level cuts, however near the levels between. The **minimum axial
  engagement** leaves out fine and shallow levels so: those too near the level
  above would only skim with the flutes.
- **Slot clearing**: going down into a closed area, a slot along its longest way
  through where the helix went down, the axis its area spreads furthest along,
  out to its ends; then, wider than the tool, rings round it, each no more than
  the optimal load out, to the width. One pass, cut out of the material, then
  the clearing goes on from it. A slot that does not fit, or would be shorter
  than the tool's radius, is not cut, and the pass starts from the helix.
- **The stay-down level, minimum stay-down clearance and lift height.** The
  level is how much of what the stay-down distance leaves past going straight a
  way round may take: none stays down only straight. The clearance is kept
  along every leg but within the tool and it of the passes the link joins,
  where the tool grazes the material it leaves and comes to; with none, that is
  the tool's radius, where it was 0.9 of it all along: a way round now keeps
  clear of the material. A lifted link rises and comes down again over a fifth
  of its height each way, half as steep as a link may be without being taken
  for a plunge, lower on a link too short for that, and no higher than the
  level above.

## Not taken yet

- Rest machining from a file, bodies or the setup stock, and from earlier
  setups (that needs [the volumetric stock](../../PROGRESS.md#what-is-next)).
- Shaft and holder clearance.
- A slot along a pocket's middle that bends with it, as a medial axis would: the
  slot is straight, along the pocket's longest way.
- Arc fitting of the steps is every strategy's smoothing since
  ([decision 0015](0015-smoothing.md)).
