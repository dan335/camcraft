# 0026 — Thread

**Status:** decided, 2026-11-13 · **Milestone:** 0.8

## Question

Thread mills a thread into a round hole with a thread mill, the last of the 2D
operations camcraft did not have. It needs a tool camcraft did not have
either. What path does it take, what is the tool as the simulator and the
checks see it, and how is a thread cut into a hole the model has plain told
from a gouge?

## What a new one starts with

Thread Pitch `tool_threadPitch` for a thread mill, 1 otherwise; Pitch Diameter
Offset 0; Threading Hand right (or left); Direction climb (or conventional);
Multiple Passes off, two stepovers a millimetre apart when on; Use Helical
Leads on; Lead To Center on; a horizontal lead radius of a tenth of the tool, a
linear lead of a twentieth, a vertical lead radius the horizontal one's; Top
from the hole's top and Bottom from its bottom, offsets 0; retract 5 mm over
the stock, clearance 10 over that; the holes picked by their cylindrical
faces. No Shaft and Holder.

## The tool and the path

A single-profile thread mill has one tooth, a 60° profile, a flute length
equal to the pitch, a pointed tip with no offset, and a shoulder (neck)
diameter of exactly the cutting diameter less the pitch over the tangent of
30°: 6.108911 mm for an M12 x 1.75 one 9.14 mm across. The tooth is a V one
pitch tall from the tip, its point at the cutting diameter half a pitch up,
its root the neck's width.

With that M12 thread mill, in the two holes of `ops_2d`, a through hole 16
across and a blind one 14 across with its floor at Z10, each hole is cut the
same way:

- A rapid over its centre, down to 2 mm above where the lead starts, and a feed
  down the centre to it.
- A straight from the centre to an arc 0.914 mm in radius, a tenth of the tool,
  centred on the thread's circle less that, a quarter turn onto the circle on
  +X.
- The helix, anticlockwise, from the hole's bottom to its top at 1.75 mm a turn,
  in half turns from +X and what is left.
- An arc off the same size, a quarter turn on, and a straight back to the
  centre; a rapid up.

The circle is the hole's radius less the tool's, and half the offset out: 3.43
mm in the through hole, 3.93 with the offset. Both arcs rise as the helix does,
at its slope round its own circle, so they run into it without a step: the
lead in starts its arc's length times `pitch / (2 pi r)` below the bottom,
0.117 mm in the through hole, 0.165 in the blind one, and the lead off ends as
far above the top. In the blind hole that is below its floor: the tool's tip
goes to Z9.835, and with the offset Z9.864, under a floor at Z10.

## What was decided

**The path is laid out move for move.** `strategy::thread` lays each pass out
as above, to the thousandth. Each pass starts with a link to the hole's centre
where the lead starts; the linker comes down there and lifts from where it
ends. A thread has no vertical leads: the tool comes straight down the centre
and goes straight up, where the linker would bring a pass that starts in the
air down a vertical arc, sweeping the tool off the centre at the bottom.

**Which way.** A right-hand thread rises anticlockwise, seen from above. With a
clockwise spindle a climb cut keeps the wall on the tool's right: round a hole,
anticlockwise. So right hand climbing rises from the bottom;
conventional goes clockwise, so falls from the top; left hand climbing falls
anticlockwise from the top, and conventional rises clockwise from the bottom.
A falling thread's leads mirror a rising one's, in above its top and out below
its bottom. A spindle turning the other way swaps climb and conventional, as it
does for every strategy.

**Multiple passes** are each a whole pass, from the centre and back, each a
stepover further in than the next, the last at the thread; one past the hole's
middle is left out and said, as a bore's is.

**The thread mill** (`kind = "thread-mill"`) has a `pitch`, needed, and a
profile `angle`, 60° unless given, beside the fields every tool has. Its tooth is
a V of the profile angle one pitch tall, its root at the tip. The simulator's
material is a height field: it cannot hold the undercut a thread is, and takes
every tool as reaching up from its lowest point. So the thread mill cuts, as
the simulator and the drop-cutter take it, as its tooth's lower flank: a flat at
the root's radius, rising at half the profile angle from level to the point half
a pitch up, as wide as the point above it. That is a chamfer mill's shape, so it
is built as one (`Cutter::Cone`). Seen from above it is honest about its widest
point: everything within the tooth's reach, above the lowest the tooth went
there, is cut. A thread cut into a wall shows as the wall moved out to the
tooth's reach, down to where the tooth first went. The shank check sweeps the neck at the
root's radius, its widest, which keeps off the thread the tooth cuts, as a
thread mill's neck must; at the cutting diameter it would meet, at every move,
the wall the tooth was cutting into.

**What it cuts on purpose.** The model has a plain hole, so with an offset the
part check called the thread a gouge, the whole height of the wall. A thread
keeps with its report, for each hole, the thread it meant to cut: the hole's
radius, the tooth's reach (the radius and half the offset), and the thread's
bottom (`ThreadCut`). `OperationResult::meant` now says what an operation cuts on
purpose in either kind, a 2D chamfer's bevels or a thread's threads (`Meant`),
and `run::cut_past` lowers the part to it where the operation cut, as for a
chamfer (`PartSurface::lowered`): between the hole's wall and the tooth's reach,
down to the thread's bottom, and nowhere else. The existing mechanism held
it; nothing in `camcraft-sim` changed. A thread cut deeper than asked is not
meant: the program for a 1 mm offset checked against a thread asked for
0.5 mm gouges every wall. Nor is anything below a hole's floor.

**A lead into the floor is said, not hidden.** The blind hole's floor is the
model's, and the lead, rising at the helix's slope, starts below it. camcraft
cuts it so and says so: the run drops the tool on the part at each hole's
centre, and where a pass's lowest point is more than the tolerance below where
it rests there, warns how far and where (CC224); the part check fails there
(CC107), unchanged. A thread
meant to reach a blind hole's floor with this tool needs the Bottom raised by the
lead's dip, or level leads, whose ends are the thread's.

**Settings.** `pitch`, `pitch_diameter_offset`, `hand`, `conventional`,
`multiple_passes`, `stepovers` and `helical_leads`, `lead_to_center`; the
stepover is `passes.stepover`, the leads `linking.leads`, as a bore's are. A
new thread starts with the tool's pitch, by the expression
`tool_type == 'thread mill' ? tool_threadPitch : 1mm`, and a full retraction
between holes. A pitch the tooth is not ground for is said.
Anything but a thread mill is refused, and a thread mill is refused by every
other strategy. A boss is refused.

## Measured

On `ops_2d`'s two holes, each bored first in three stepovers:

- **Thread** (the tooth only touches the walls): 13.78 mm³ removed, fed
  358 mm.
- **With the offset**: 682.86 mm³ removed, fed 413 mm.

The lowest and highest points at the cutting feed are Z-0.117 and Z20.165,
and with the offset Z-0.102 and Z20.136, and the only gouge either leaves is
the blind hole's floor. The linker feeds up out of each hole the safe
distance before it rapids.

## Not yet

Not yet measured on a job: left hand and conventional, which way each goes
and where a falling helix starts; Multiple Passes on, whether the stepover is radial, which way the
passes are ordered, whether each goes back to the centre and lifts, and where
one past the middle is left out; Use Helical Leads off, where its level leads
are; Lead To Center off, where it comes down and how the linear lead lies, and
whether the vertical lead radius is used then; the lead arc in a hole too small
for it. Not offered: outside threads, multi-tooth thread mills, Start Angle,
multiple threads, Repeat Passes, Stock to Leave, compensation in control,
threads from a standard or the model, diameter range and same-diameter
selection, hole order choices.
