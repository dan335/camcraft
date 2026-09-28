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
  at its entries only; the linker's would helix into every re-entry.
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

## Not taken yet

- **Rest material from earlier operations.** The block is cleared as it starts;
  after another operation it cuts air. Sweeping the earlier operations' moves
  exactly, level by level, keeps to the grid rule.
- **Fine stepdown**, back up each slab over slopes: the flat levels' seeding is
  the same mechanism.
- Shallow areas, slot clearing, minimum cutting radius, order by area.
- Stay-down links routed through what is cleared, rather than only straight.
- Arc fitting of the steps: a pass is its steps, as the rule against smoothing
  says.
