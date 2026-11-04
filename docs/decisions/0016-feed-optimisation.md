# 0016 — Feed optimisation: cuts slowed into and out of their corners

**Status:** decided, 2026-11-04 · **Milestone:** 0.5

## Question

Where a pass turns sharply into the material, as into a pocket's corner, the
tool is wrapped in more of it for a moment than along the walls either side,
and at the same feed each tooth takes more: the load jumps, the tool deflects
and chatters, and a small one breaks. Fusion's Feed Optimization, beside
Smoothing on the Passes tab, slows the cut there. Its reference names five
settings, Maximum Directional Change, Reduced Feed Radius, Reduced Feed
Distance, Reduced Feedrate and Only Inner Corners, but gives no defaults, and
does not say how far past a corner the feed stays down or how an inner corner
is told from an outer one. What should camcraft's do?

## What was decided

**A transform after smoothing, before linking** (`core::path::corners`), on
every strategy that has smoothing but adaptive clearing. It moves nothing: a
move is cut in two where the feed changes along it, an arc into two arcs about
the same centre, so what is simulated and verified is the same path, at the
feeds the program has.

- **What is slowed:** cuts, one after another. A corner is where one cut ends
  and the next starts turning by more than the most allowed; a tight arc is one
  whose radius is less than the reduced feed radius. Smoothing goes first, so
  its arcs are what is measured: the corners and arcs slowed are the program's.
  The cut slows the distance before each **and the same distance after**, as
  the load builds going in and falls coming out; spans that meet are one.
  A cut already slower than the reduced feed keeps its own.
- **Inner corners from the side the material is on.** A pass with the material
  on its right, climbing with a clockwise spindle, turns into it at a corner
  that turns left, away from what it cuts, towards the wall ahead: a pocket's
  corner, not a boss's. The other way round with it on the left. Where an
  operation's passes cut both ways (parallel, radial, facing, or a direction of
  both ways), the side is not known, and every corner seen from above is taken
  as inner. A corner that turns up or down has the material below: inner where
  it turns up, along a valley, not over a ridge.
- **Adaptive clearing does not have it.** It holds the load at every step,
  round its tight loops as along its passes, so no corner of its loads the tool
  more than its walls. On a serving tray, turned on, it slowed 259 m of
  adaptive's cut at 39,824 arcs, all of it lost time.
- **camcraft's defaults,** as Fusion gives none: a corner turns more than 45°;
  the reduced feed radius and distance are the tool's radius, the radius of a
  wall's corner the tool's centre turns round when the wall is sharp; the
  reduced feed is half the cutting feed; only inner corners.

## Result

The example job, every operation smoothed and slowed at every corner, posted
for every built-in machine and read back as a controller would, cuts the paths
it was posted from. A pocket slowed at its corners removes what it did, and
verifies. On the serving tray, scallop smoothed and slowed at its corners and
tight arcs, 2.3 m of it slowed, every check passing.

## Not taken yet

- Corners judged by the material as it stands when they are cut, as adaptive's
  engagement is, rather than by which way the passes go.
- Feeds raised where the tool is cutting less than the stepover, Fusion's
  other kind of feed optimisation, which it leaves to adaptive.
