# 0014 — Walls left to rings by every finish over the part

**Status:** decided, 2026-11-03 · **Milestone:** 0.4

## Question

Scallop leaves the ground steeper than a plunge to rings round the walls
there, a stepover apart down them ([decision 0009](0009-scallop-distance-field.md),
and its steep band since 0.3). Parallel, spiral, radial and morphed spiral cut
down such ground as Fusion's do: a pass is broken only where the part drops
further than a cut step. Do they need the same?

## What the tray showed

On a serving tray, each as its finish, none of them cuts in the short runs
scallop did before its band: parallel 0 of 272 runs under 3 mm, radial 0 of
1717, morphed spiral one unbroken run, spiral 1 of 250. They do not leave steep
ground out, so they do not leave it in pieces.

What they leave is the walls themselves. A pass cut down a wall touches it
only where it crosses it: a wall the passes run across is cut at every pass, a
stepover apart round it, but one the passes run along is crossed by a few, each
at its own height, and left in steps as tall as the wall over that few. Fusion
leaves that to the person, to follow with a contour; camcraft's scallop already
did not.

## What was decided

**Every finish over the part leaves its steep ground to rings round the
walls,** found as scallop finds them: the band where the surface the tool's
centre rides is steeper than a plunge, where the tool falls off the part
included, closed over gaps narrower than the tool ([`steep_ground`], with
[`steep_band`] scallop's own grid uses). Parallel keeps its passes off it
through its boundary, the patterns through the area they are clipped to, and
the rings are the walls scallop cuts ([`steep_walls`]), from the top to the
Bottom, a stepover times the sine of the plunge's slope apart.

**A slope range keeps to it.** Where the range asked for stops short of a
plunge's slope, there is no band and no walls, and the passes keep to the
range as before: the person has said what they want cut, as with Fusion's
contact angles.

Finding the band is a grid of drops over where the tool touches the part, as
fine as scallop's field, coarsened if it must be but to no more than half the
tool: a fifth or so of the finish's work on the tray.

## What was not done

- Leaving the walls to a separate contour operation, as Fusion would have it.
  A finish that leaves walls unfinished by default is not one to post.
- Rings for flat finishing, which finishes only the flats, and for pencil or
  steep-and-shallow, which camcraft does not have.

[`steep_ground`]: ../../crates/camcraft-core/src/strategy/slope.rs
[`steep_band`]: ../../crates/camcraft-core/src/strategy/slope.rs
[`steep_walls`]: ../../crates/camcraft-core/src/run/operation/finishing.rs
