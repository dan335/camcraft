# 0025 — Engrave

**Status:** decided, 2026-11-13 · **Milestone:** 0.8

## Question

Fusion's Engrave cuts a shape with a V, the tool going up and down as the shape
narrows and widens. What path does it take, how is that found without
medial-axis code, how are the shapes picked, and what of a shape the V goes
deeper than?

## What was found

Autodesk's "Engrave reference" lists the settings: contour selection (edges,
sketches or faces, chained), Tolerance, Sharp Corner Angle ("if the angle
between edges is greater than this value, there will not be a corner clean out
move"), Multiple Depths, Smoothing, Keep Tool Down and its Maximum Stay-Down
Distance. It gives no defaults. A fresh Engrave read from Dan's Fusion through
its API gives them: Top *from contour*, offset 0; Bottom *from top*, offset
`0.95 * -((tool_diameter - tool_tipDiameter) / 2) * tan(90° - tool_taperAngle)`
for a chamfer mill, the same with half the tip angle for a tool given by one;
Sharp Corner Angle 165°; Multiple Depths off, 1 mm; Keep Tool Down on, 50 mm;
tolerance 0.01 mm. No stock to leave, direction, order, ramp or Shaft and
Holder.

What it cuts is in four programs it posted for `fixtures/step/ops_engrave.step`,
a plate with a triangle, an L 4 mm wide and a rectangle sunk 3 mm into its top,
engraved with a 6 mm 90° V (`fixtures/nc/ops_engrave_*.nc`):

- The depth is set by the nearest wall: the cone touches the outline, no deeper
  than the Bottom, 2.85 mm here.
- In the rectangle and the triangle's middle, wider than the V reaches at that
  depth, it runs round the outline 2.85 mm in at the Bottom, and at each corner
  up the bisector to the corner at the surface and back down. It does not clear
  the middle.
- In the L it runs along the middle, 2 mm deep, round the parabola the inside
  corner pulls the middle into, up into each outside corner and back, along the
  middle twice.
- It starts at an outline corner at the surface, 0.001 mm in from it, and
  stays down round the shape. Its last run, back up to the corner it started at
  or along a middle it has cut, is left off. Between shapes it lifts, though
  they are nearer than 50 mm.
- Its straight stretches are one move each; its parabolas are moves 0.083 mm
  apart in X.
- With all three shapes picked it went round each anticlockwise; with each
  alone, the triangle and the rectangle clockwise and the L's end in another
  order. The material is the same either way.

## What was decided

**The path is a walk round the outline.** At each point of the outline, the
largest circle that fits in the shape and touches the outline there, no larger
than the depth allows, is stood on it; the tool's axis goes through its centre,
as deep as makes the flank touch the outline: `(r - tip) / tan(half angle)`,
clamped to the depth. The circle at `p`, its centre `p + r n` along the inward
normal, is limited by each other piece of the outline: a corner `q` to
`|q - p|^2 / 2 n.(q - p)`, a line or an arc to the radius it touches at, where
it touches inside the piece's ends. This gives Fusion's path and its order: along
a wall the centre runs parallel to it; into a convex corner it runs up the
bisector; at a reflex corner the point stays put while its normal turns, and the
centre runs round a parabola, or round an arc at the depth; in a narrow stroke
it runs along the middle once for each wall. It needs no medial axis, is exact
on the outline, and is bounded: each piece is sampled at a quarter of the reach
apart, at most 4096 times, each change of what limits the circle found by
halving, at most 48 times, and each curved stretch halved at most 12 times.

Each stretch limited by one thing is one curve, emitted as such: straight where
a line limits it or the cap does along a line, as one move; an arc where the cap
does along an arc or round a reflex corner, posted as `G2` or `G3`; anything
else halved until each move is within half the tolerance, as Fusion's contour
linearization tolerance is. A convex corner at least the sharp corner angle
between its edges is gone round: the stretches of the walk up its bisector are
left out. With a flat tip, what is narrower than the tip, where the tip only
rides the surface, is left out, and the walk breaks there.

**Where it starts and stops.** Each loop's walk starts at its lowest, then
leftmost, corner it runs into, at the surface; Fusion's three programs start at
the first point each sketch was drawn from, which is that corner in each. It
ends with what it would go back over, cut already, left off: each move from the
end whose ends and quarter points lie within the tolerance of what was cut
before it. The outline runs anticlockwise, as Fusion's did with all three
picked, its islands clockwise, the shape on the left. An island's walk that
meets the outline's, at the end of a move of each within the tolerance, as it
does along the middle of a channel narrower than twice the reach, is spliced
in there and gone round whole, so the shape is one pass; one that meets it
nowhere is a pass of its own.

**Lifting between shapes.** Fusion's Keep Tool Down is not offered. In every
program it lifted between shapes nearer than 50 mm; camcraft's level stay-down,
tried first, linked the L to the rectangle at Z10 with the point on the plate,
which on a machine scribes the part. Between passes it lifts, to the retract
height in a new operation, as a 2D chamfer does.

**Picking.** A shape is picked as a 2D pocket's outline is: the edges round its
rim, a click taking the whole loop, or its floor as a face. A loop is engraved
from its own height, Fusion's Top "from contour". A floor stands for the shape
sunk into the part: its edges are the outline, and it is engraved from the
highest point of the faces sharing an edge with it, the top of its walls; a
face with nothing standing round it is engraved at its own height. Loops are
read even-odd, a loop inside another an island of it. Shapes are cut in the
order picked.

**Depth.** Fusion's Bottom, from the Top by an offset, is the operation's
`depth`, left out for Fusion's own: 0.95 of how deep the flank reaches. A depth
past the flank is refused: the cone would no longer touch the outline at the
surface. camcraft has no height "from top"; its Top and Bottom settings are not
offered for an engraving.

**A shape the V goes deeper than.** The model is the part: the V cuts into
whatever the shape is not sunk deep enough to clear. The run drops the tool on
the part along every cutting move, every quarter of its radius, and where the
path is more than the tolerance below where the tool would rest, says so, how
deep and where (CC223); the finished material then fails against the part
(CC107), unchanged. The 2D chamfer's way of declaring a cut meant
(`OperationResult::meant`) is not used. An engraving is picked on the model, so
the model says what is kept: a shape modelled shallower than the V is either a
mistake or a modelling shortcut, and either way a gouge is the honest answer.
Holding it as meant would need the V's swept surface as what is meant, a second
kind of meant cut beside a chamfer's bevels, which is more than this step
takes.

## Held to Fusion

`crates/camcraft-session/src/compare/tests_engrave.rs` makes a new Engrave of
all three shapes and of each alone, runs it, and cuts Fusion's program from the
same material with the same tool. On every one every column cut agrees to
0.01 mm, the volumes removed are within 0.01%, the lowest points are Fusion's,
neither gouges or rapids through material, the distance fed is within 3%, and
the moves are fewer: walls are one move each, and parabolas take fewer moves
within the tolerance than Fusion's evenly spaced ones.

## Not yet

Top and Bottom from other references; Keep Tool Down; an island's walk joined
to its outline's where they meet only partway along a move, or not at all,
where Fusion's medial axis would still join them; entry positions; sketches
and text, which camcraft cannot pick; polar and wrap.
