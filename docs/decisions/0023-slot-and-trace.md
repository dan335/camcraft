# 0023 — Slot and trace: what runs along picked edges

**Status:** decided, 2026-11-13 · **Milestone:** 0.8

## Question

Slot and Trace are the first 2D operations that run along what is picked
rather than over an area or round a hole. How are their edges picked, a slot
found from them, a curve offset for the tool, and each linked?

## What a new one starts with

- A slot is picked as a pocket is, by the bottom edges of the slots; its Top
  is the stock's, its Bottom from the contour. It has Multiple Depths off, a
  Maximum Stepdown of a millimetre, axial stock to leave only, Backoff
  Distance and Repeat Finishing Pass off, Preserve Order off. It ramps along
  the slot's shape at the tool's 2°, its Maximum Ramp Stepdown the Maximum
  Stepdown; it stays down within twice the tool.
- A trace picks curves (`curves`): edges, open or closed. It has only
  Clearance, Retract and Feed heights. Sideways Compensation is center; Axial
  Offset and Pass Extension none; Maximum Angle 90°; Multiple Depths off, with
  a Maximum Stepdown of a millimetre and one stepdown; Order by Depth, Both
  Ways off, Repeat Passes off, Preserve Order off; no stock to leave. Its
  linking retracts fully, keeps the tool down only when asked (50 mm), and has
  Lead-In and Lead-Out off, their vertical radius a tenth of the tool. It has
  no ramp, and no multiple passes or spiral stepdowns.

## What was decided

**A slot is found from its loop.** The picked edges, or the edges round a
picked floor, are joined end to end into closed loops. A loop is a slot's if it
lies level and two of its arcs turn a half turn each, the same size, and every
point of it is half their width from the middle between them: the side between
the two, moved in by that much. So straight, curved and bent slots are found
alike, and anything else is refused, saying so. Only closed slots are found yet.

**Ramped by the linker, each way no deeper than the most.** The linker already
ramps back and forth along an open pass. That is the slot's profile ramp, but
for the Maximum Ramp Stepdown, which bounds each way along the slot: the
linker is told to keep each leg to it (`LinkParams::ramp_legs`) for a slot alone,
so no other strategy's ramps change. Without multiple depths, it is a
millimetre, as the Maximum Stepdown stays a millimetre with Multiple Depths
off. The ramp starts at the safe distance over the material.

**Round the walls with a narrower tool.** The tool must be no wider than the
slot, so one narrower is allowed, and should leave the slot cut to its width.
After the middle at each level the tool goes once round the walls, as a pass
of its own so the ramp along the middle keeps to the middle.

**Picked edges run on along smooth joints.** A click for a trace takes the edge
and the edges running on from it without a corner (`Topology::smooth_chain`),
either way; a slot's takes the loop, as a
boundary's does. What is saved is the edges, as for any operation, so how a
click widens them can change without changing a job file.

**Offset by pieces.** A curve is lines and level arcs; offset for the tool,
each piece moves square to itself, the outside of a corner is rounded about
it, and the inside trimmed where the two cross (`strategy::curve::offset`).
One that turns more tightly than the tool's radius on the inside is refused,
not cut through. A slot's middle is found the same way, from one side.

**The job file has a key for each setting above**: a slot's
`backoff_distance`, `repeat`, `preserve_order`; a trace's `compensation`,
`axial_offset`, `pass_extension`, `max_angle`, `stepdowns`, `order`,
`both_ways`, `repeat`, `preserve_order`, `keep_tool_down`, `lead_in`,
`lead_out`. A new trace starts with a full retraction, written into the job;
a file that leaves it out has the minimum, as every other strategy.

## What it leaves open

The slot is tested on a block made in its test. Which way an open curve is
run, how the stepdowns are counted, what a tool narrower than a slot should
do, how the ramp starts and where on a slot it enters, and what Order by
Depth orders were left open here; the slot's were settled the same day
(below). Trace's Chamfer, which cuts into the part's edge on purpose, waits
for a way to tell the checks a cut past the part is meant.

## Amended 2026-11-13

On `ops_2d`'s two slots, 8 wide, one 6 deep and straight, one 4 deep at 30°,
cut with the 6 mm flat and with the 8 mm:

- **The middle only.** With the 6 mm flat in the 8 mm slots the slot is cut
  along the middle and nothing else, leaving a millimetre on each wall: the
  loop round the walls is gone. A tool narrower than the slot leaves its
  walls, and the checks say how much (CC219, a note now).
- **Its own ramp.** The slot feeds down at the ramp feed to the ramp clearance
  height, 2.5 mm over the stock's top, and ramps back and forth along the
  whole middle, end to end, every leg falling as far, until it is on the
  floor; then once along the floor at the cutting feed to the other end. Its
  legs are the fewest that fall no more than half the Maximum Ramp Stepdown
  each, from where the ramp starts to the floor: 22 legs of 0.295 mm for 6.5
  mm with the 6 mm flat, 17 of 0.382 mm with the 8 mm. So the Maximum Ramp
  Stepdown is what it falls there and back, and with Multiple Depths off it is
  `Math.min(tool_fluteLength * 0.75; tool_diameter * 0.1)`, the Maximum
  Stepdown counting as none. The ramp is now the slot's
  own (`strategy::slot`), not the linker's, and starts at
  `ramp_clearance_height`; the linker no longer limits a ramp's legs along an
  open pass (`LinkParams::ramp_legs` is gone). The ramp angle stays the other
  limit, which on these slots allows 1.05 mm a leg.
- **Order and ends.** camcraft starts each slot at its end lowest in X, then
  Y, and cuts first the slot starting nearest the work zero, then always the
  one starting nearest where the last ended: here the angled slot first, from
  its lower-left end, then the straight one from its left.

With the 6 mm flat the two slots remove 2,082.6 mm³, fed 1,594 mm; with the
8 mm, fed 1,244 mm.

Not yet measured on a job: Multiple Depths (here, each level ramped down to
from the one above along the middle, then cut along), Backoff Distance (back
along the middle off the end wall), Repeat Finishing Pass (along the floor
again, back the way it came), and Preserve Order.

Trace on `ops_2d`'s two V grooves feeds 89 mm, the round groove one arc. An
open curve starts at its end lowest in X. Its compensation, multiple depths
and the rest are not yet measured on a job.
