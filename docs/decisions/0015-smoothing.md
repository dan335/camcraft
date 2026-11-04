# 0015 — Smoothing: arcs and longer lines fitted to the passes

**Status:** decided, 2026-11-04 · **Milestone:** 0.5

## Question

A strategy's passes are its steps. Adaptive's are at most a fifth of the
tool's radius, so on a serving tray with a 3/8" flat its program is 1.3 million
lines, 34 MB, and at its 5,842 mm/min feed about 200 moves a second. A
controller reads only so many lines a second; past that it waits between
moves, and the cut stutters. Fusion's Smoothing fits the moves with arcs and
longer lines within a tolerance. Is that the smoothing
[decision 0003](0003-grids-only-for-stock.md) rules out?

## What was decided

**It is not, and every strategy but drilling has it.** Decision 0003 rules out
smoothing that hides where a path really runs, as a grid's staircase would have
been hidden. Fitting describes the path the strategy made in fewer moves, each
within a tolerance of it, both ways; it moves nothing further than that, and it
is off unless asked for, as Fusion's is.

- **A transform, before linking** (`core::path::fit`), as the toolpath module
  always meant arc fitting to be. The linker joins the fitted passes and cuts
  them into the simulated stock as it does, so what is simulated, checked for
  rapids through material and verified against the part is what is written.
- **What is fitted:** straight feeding moves one after another with the same
  role and feed, that may be arcs: cuts, transitions, leads and ramps. Anything
  else, or a change of role or feed, ends a run and is kept, so every pass
  starts and ends where it did.
- **Arcs only where a three-axis machine cuts them:** the points level, or
  square to X or Y (`G17`, `G18`, `G19`), or round a circle seen from above and
  climbing or falling steadily as they turn, a helix about Z, as a ramp round a
  ring is (taken 2026-11-04: each point is held to where the helix is at the
  angle it has turned to, along Z as well as across); never more than a half turn, so an
  arc written by its radius is not ambiguous, and no larger than 2 m across,
  where a line does. Each move turns by less than a quarter turn, all the same
  way.
- **Within the tolerance both ways:** every point of the moves within it of
  the fitted arc or line, counting how far a chord's ends are off the circle and
  how far the circle bows past the chord; and along a line, every point further
  along than the one before, so it never doubles back. From each point the arc
  or line reaching furthest is taken, found by doubling the reach, then halving
  back.
- **The tolerance is its own,** the job's unless given, and adds to it, as
  Fusion's total tolerance is the two together: the setup is verified to the
  job's tolerance and the largest smoothing tolerance among its operations.

## Result

On the serving tray, both operations smoothed at the job's 0.05 mm: adaptive's
1,298,557 moves fitted to 172,786, 85,111 of them arcs, its program 34.2 MB to
6.1 MB; scallop's 187,469 to 15,944, 0.56 MB from 6.0 MB. Every check passed,
no gouge, no rapid through material; adaptive removed within 0.04% of what it
removed unsmoothed, scallop within 0.3%. The
example job, every operation smoothed, posted for every built-in machine and
read back as a controller would, cuts the paths it was posted from.

## Not taken yet

- Helices about X or Y: a helix is fitted only about Z.
- Feed optimisation at corners, Fusion's other filter on the same tab, is
  taken: [decision 0016](0016-feed-optimisation.md).
