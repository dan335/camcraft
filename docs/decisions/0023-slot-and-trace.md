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
