# 0022 — Bore and circular: round walls picked as faces

**Status:** decided, 2026-11-13 · **Milestone:** 0.8

## Question

Bore and Circular mill round holes and bosses. They are the first of the 2D
operations still to come that pick holes rather than an area, and Thread will
pick the same way. How are the holes picked, how is a pass round one laid out,
and how is it linked?

## What a new one starts with

- Both pick round faces, with their top and bottom from the hole's.
- A bore's helix is set by its angle, 2°, unless its pitch is asked for (1 mm
  a turn). It has no multiple depths.
- Both have Multiple Passes off, two stepovers half the tool apart when on,
  Repeat Passes off and climb. A bore has one finishing pass a tenth of the
  tool from the wall, off. A circular has Multiple Depths on, a millimetre
  apart, not even.
- Both lead with a horizontal arc a tenth of the tool, a straight a twentieth
  of it, and a vertical arc the size of the horizontal one; Lead To Center off.

## What was decided

**Picked as faces.** A bore's or a circular's picks are faces, read by
`strategy::round::rounds`: a face whose edges are circles about
one upright axis, all one size, is a straight round wall; which way its
triangles face says whether it is a hole or a boss. The faces of one wall are
taken together. Drilling keeps its rims: a rim says nothing of which side the
material is on, and a wall says nothing a drill needs that a rim does not.
`Uses::holes` says which a strategy picks (`Holes::Rims` or `Holes::Walls`),
so the panel offers only faces for these, and the run finds them once, before
the strategy runs (`round_walls` in `run::operation::round`).

**Passes laid out by the strategy, linked by the linker.** Each pass is the
strategy's own: a lead onto its circle, the helix or the circle, a lead off,
all level or about Z, so every move is an arc posted as one. A pass that would
be past a hole's middle is put half the tool's radius from it, once: this cuts
the middle rather than leaving a pin. A hole the passes do not reach the
middle of is left with its core, and said (CC218). The linker brings the tool onto each
pass: a bore's starts at the top, out of the material, so it comes down the
vertical arc onto it and lifts out after; a circular's first pass at a level
ramps down along its circle, and the passes after it stay down.

**No move a whole turn.** Every circle and helix is cut in arcs of at most
half a turn. A move that ends where it starts, seen from above, reads to the
linker's ramp as straight down: a closed pass made of one full-circle arc is
ramped into by plunging.

**The job file has a key for each setting above**: `use_ramp_angle`,
`ramp_angle`, `pitch`, `multiple_passes`, `stepovers`, `repeat`,
`even_stepdowns`, `lead_to_center`.
A bore's finishing pass is a pocket's `passes.finishing`, of which it reads
only the stepover.

## What it leaves open

Whether the stepovers count the wall's pass or not, where passes past a hole's
middle go, where on the circle a pass starts, whether the lead-in arc turns a
quarter turn, and how a circular goes down to a level were left open here, and
settled the same day (below).

## Amended 2026-11-13

- **A bore's helix by its angle** falls, a turn, the angle's tangent times the
  diameter the tool's outer edge sweeps: the pass's circle widened by the tool,
  not the tool's centre's circle. 0.5587 mm a turn in a hole 16 across with a
  6 mm tool at 2°.
- **A pass past a hole's middle is left out**, not moved to half the tool's
  radius: three stepovers of 3 mm leave a hole 16 across passes at 2 and 5.
  The run says so (CC222). The cut-off is a tenth of a millimetre.
- **A circular neither ramps nor stays down.** Every pass at every level comes
  down its vertical lead, into the material where the level is still there,
  and lifts out after, even to a circle 3 mm away at the same level. Its ramp
  along the circle and its stay-down are gone, so a saved circular's
  `linking.ramp_angle`, `ramp_feed`, `ramp_stepdown`, `stay_down` and
  `retraction` are no longer read.
- **The lead's level arc** is made no wider than half its circle where the
  circle is small, keeping its straight (0.5 on a circle of 1), before it is
  halved.

Kept as they were: N stepovers are N passes, the wall's among them, from
the middle out; every pass starts on +X and climbs anticlockwise in a hole; a
bore lifts out between passes and starts each at the top, and goes once round
level at the bottom; the leads are a vertical arc a tenth of the tool, a
straight a twentieth and a quarter-turn level arc a tenth, off the mirror; a
circular's levels are a millimetre apart, each level's passes from the middle
out before the next level.

A single bore's lead off the bottom bites the core it leaves, and its
vertical arc up is five straight moves within the tolerance.
