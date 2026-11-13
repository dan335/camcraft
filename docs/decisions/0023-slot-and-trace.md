# 0023 — Slot and trace: what runs along picked edges

**Status:** decided, 2026-11-13 · **Milestone:** 0.8

## Question

Fusion's Slot and Trace are the first of its 2D operations that run along
what is picked rather than over an area or round a hole. How are their edges
picked, a slot found from them, a curve offset for the tool, and each linked?

## What was found

Autodesk's references ("Slot reference", "Trace reference", "Generate a Slot
toolpath", "Generate a Trace toolpath" in the Fusion CAM help) list the
settings and give almost no defaults. Every parameter of a fresh operation of
each, as a Fusion install's API gives them
([grace-xwerks/fusion_mcp_testing](https://github.com/grace-xwerks/fusion_mcp_testing/tree/master/docs/strategy_params),
`slot.txt` and `trace.txt`), gives the rest, as it did for
[decision 0020](0020-defaults.md) and [0022](0022-bore-and-circular.md):

- A slot is picked as a pocket is (`pockets`, "select the bottom edges of the
  slots"); its Top is the stock's, its Bottom "from contour". It has Multiple
  Depths off, a Maximum Stepdown of a millimetre, axial stock to leave only,
  Backoff Distance and Repeat Finishing Pass off, Preserve Order off. It ramps
  by Profile ("ramps along the slot shape", "zig-zag to the full depth") at the
  tool's 2°, its Maximum Ramp Stepdown the Maximum Stepdown, from a Ramp
  Clearance Height of 2.5 mm; it stays down within twice the tool.
- A trace picks curves (`curves`): edges, open or closed. It has only
  Clearance, Retract and Feed heights. Sideways Compensation is center; Axial
  Offset and Pass Extension none; Maximum Angle 90°; Multiple Depths off, with
  a Maximum Stepdown of a millimetre and one stepdown; Order by Depth, Both
  Ways off, Repeat Passes off, Preserve Order off; no stock to leave. Its
  linking retracts fully, keeps the tool down only when asked (50 mm), and has
  Lead-In and Lead-Out off, their vertical radius a tenth of the tool. It has
  no ramp.

The Trace reference lists Top and Bottom heights; the API's fresh Trace has
none, and Inventor HSM's Trace reference lists only Clearance, Retract and Feed.
The API's is taken. The API's Trace has Multiple Passes and Spiral Stepdowns,
which the reference does not describe; they are left out.

## What was decided

**A slot is found from its loop.** The picked edges, or the edges round a
picked floor, are joined end to end into closed loops. A loop is a slot's if it
lies level and two of its arcs turn a half turn each, the same size, and every
point of it is half their width from the middle between them: the side between
the two, moved in by that much. So straight, curved and bent slots are found
alike, and anything else is refused, saying so. Only closed slots are found yet.

**Ramped by the linker, each way no deeper than the most.** The linker already
ramps back and forth along an open pass. That is Fusion's Slot's profile ramp,
but for the Maximum Ramp Stepdown, which bounds each way along the slot: the
linker is told to keep each leg to it (`LinkParams::ramp_legs`) for a slot alone,
so no other strategy's ramps change. Without multiple depths, it is a
millimetre, as Fusion's Maximum Stepdown stays a millimetre with Multiple Depths
off. The ramp starts at the safe distance over the material, not Fusion's Ramp
Clearance Height.

**Round the walls with a narrower tool.** Autodesk says the tool must be no
wider than the slot, so one narrower is allowed, and must leave the slot cut to
its width. After the middle at each level the tool goes once round the walls,
as a pass of its own so the ramp along the middle keeps to the middle. What
Fusion does there is not documented.

**Picked edges run on along smooth joints.** A click for a trace takes the edge
and the edges running on from it without a corner (`Topology::smooth_chain`),
either way, as Fusion's chain selection does; a slot's takes the loop, as a
boundary's does. What is saved is the edges, as for any operation, so how a
click widens them can change without changing a job file.

**Offset by pieces.** A curve is lines and level arcs; offset for the tool,
each piece moves square to itself, the outside of a corner is rounded about
it, and the inside trimmed where the two cross (`strategy::curve::offset`).
One that turns more tightly than the tool's radius on the inside is refused,
not cut through. A slot's middle is found the same way, from one side.

**The job file mirrors Fusion's parameters**: a slot's `backoff_distance`,
`repeat`, `preserve_order`; a trace's `compensation`, `axial_offset`,
`pass_extension`, `max_angle`, `stepdowns`, `order`, `both_ways`, `repeat`,
`preserve_order`, `keep_tool_down`, `lead_in`, `lead_out`, so a setting
confirmed against Fusion later moves no key. A new trace starts with a full
retraction, written into the job, as a new Fusion Trace does; a file that
leaves it out has the minimum, as every other strategy.

## What it leaves open

Neither is yet held to a Fusion program (`camcraft compare`), and no fixture has
a slot: the slot is tested on a block made in its test. Which way Fusion runs
an open curve, how it counts its stepdowns, what it does with a tool narrower
than a slot, how its ramp starts and where on a slot it enters, and what its
Order by Depth orders, are not in its references; PROGRESS.md and the report
that came with this list them to check. Trace's Chamfer, which cuts into the
part's edge on purpose, waits for a way to tell the checks a cut past the part
is meant.

## What Fusion's programs showed (2026-11-13)

Fusion's own programs for `ops_2d`'s two slots, 8 wide, one 6 deep and
straight, one 4 deep at 30°, cut with the 6 mm flat and with the 8 mm
(`fixtures/nc/ops_2d_slot.nc`, `ops_2d_slot8.nc`), settled what was
assumed above:

- **The middle only.** With the 6 mm flat in the 8 mm slots Fusion cuts along
  the middle and nothing else, leaving a millimetre on each wall. camcraft's
  loop round the walls is gone: a tool narrower than the slot leaves its
  walls, and the checks say how much (CC219, a note now).
- **Its own ramp.** Fusion feeds down at the ramp feed to the Ramp Clearance
  Height, 2.5 mm over the stock's top, and ramps back and forth along the
  whole middle, end to end, every leg falling as far, until it is on the
  floor; then once along the floor at the cutting feed to the other end. Its
  legs are the fewest that fall no more than half the Maximum Ramp Stepdown
  each, from where the ramp starts to the floor: 22 legs of 0.295 mm for 6.5
  mm with the 6 mm flat, 17 of 0.382 mm with the 8 mm. So the Maximum Ramp
  Stepdown is what it falls there and back, and with Multiple Depths off it is
  `Math.min(tool_fluteLength * 0.75; tool_diameter * 0.1)`: Fusion's
  expression, `maximumStepdown` counting as none. The ramp is now the slot's
  own (`strategy::slot`), not the linker's, and starts at
  `ramp_clearance_height`; the linker no longer limits a ramp's legs along an
  open pass (`LinkParams::ramp_legs` is gone). The ramp angle stays the other
  limit, which on these slots allows 1.05 mm a leg.
- **Order and ends.** Fusion cut the angled slot first, from its lower-left
  end, then the straight one from its left. camcraft starts each slot at its
  end lowest in X, then Y, and cuts first the slot starting nearest the work
  zero, then always the one starting nearest where the last ended. That rule
  gives both programs; two slots do not show it is Fusion's.

Held to those programs (`compare::tests::ops_2d`), the 6 mm's slots agree in
all 65,080 columns cut and remove 2,082.6 mm³ to Fusion's 2,082.6, fed 1,594
mm to its 1,595; the 8 mm's agree in 91,065 of 91,067, fed 1,244 to 1,235.
Before, the 6 mm's differed in 25,966 of 91,046 columns, the walls, and both
fed under half as far.

Still not compared with a program of Fusion's: Multiple Depths (here, each
level ramped down to from the one above along the middle, then cut along),
Backoff Distance (back along the middle off the end wall), Repeat Finishing
Pass (along the floor again, back the way it came), and Preserve Order.

Trace's program for `ops_2d`'s two V grooves agrees with camcraft's in every
one of its 48,779 columns, fed 89 mm to Fusion's 101: Fusion writes the
round groove as some fifty short lines where camcraft writes one arc. Fusion
runs that groove, an open arc, from its end at +X, where camcraft starts an
open curve at its end lowest in X; on the curve, it leaves the same. Its
compensation, multiple depths and the rest are not yet compared.
