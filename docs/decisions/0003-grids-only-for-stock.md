# 0003 — Grids are only for stock

**Status:** decided, 2026-09 · **Milestone:** 0.2

## Rule

A rule, not a preference. Height fields represent material — the simulation, rest
machining, clearance — and nothing else. Every point a toolpath follows, and every
boundary it is offset from, comes from the model: exact drop-cutter queries, or
shadows of the mesh. A stock grid may decide *whether* a path cuts somewhere, never
*where* it runs. No smoothing is added to hide anything.

## Why

Roughing used to follow a grid. Its rings were marching-squares contours of a
0.4 mm cutter-location grid, their points on cell-edge midpoints, then rounded to
Clipper's 0.01 mm: a staircase on every curved wall of a smooth CAD part. On the
jobs tested it also cleared far less than it could (the two-block test job kept
almost all its material). Smoothing the staircase would have hidden the defect
rather than removed it.

## How Z-level roughing follows it

(`core::reach`, `geom::shadow`)

- Where the cutter may not be at a level is the mesh's shadow from above, grown by
  the cutter's radius: exact for a flat mill. A ball or bullnose (and stock to leave
  makes every cutter a bullnose) is cut into height bands across which its radius
  grows by at most half the tolerance, each grown by its widest radius, so the
  region is never short of the truth and at most that much over. Vertical walls and
  flats come out exact. Tested against the drop-cutter as the oracle.
- The first ring is that boundary itself, then inward by the stepover. Every offset
  is padded by its arc tolerance so arcs err away from the part.
- The stock grid only clips away stretches of rings that would cut nothing but air.

On the lego fixture, the median turn between successive roughing moves fell from
7.2° to 1.1°, and moves turning more than 20° from 15% to 1.3% (the brick's real
corners). Clipper is the pure-Rust `clipper2-rust` port, for its arc tolerance
control; it is slower per call than the C++ one, so levels are worked in parallel.

## Still open

Exact slicing of the offset surface would remove the band error altogether. It
would replace `LevelReach::forbidden` and nothing else, and matters most for
Z-level *finishing* with a ball on freeform surfaces.
