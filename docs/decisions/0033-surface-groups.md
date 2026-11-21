# 0033 — Surface groups

**Status:** decided, 2026-11-19 · **Milestone:** 0.10

> **Amended by decisions [0034](0034-entries-and-order-of-the-finishes-over-the-part.md)
> and [0035](0035-scallop-over-every-slope.md).** The finishes over the part no
> longer ramp back onto a cut piece, and Scallop lays its rings anew round
> what is avoided and has no rings round the walls. The figures and the
> linking below are as they were measured here; those decisions and the tests
> have today's.

## Question

A finish over the part cut all of it. Often only some of it is to be cut: a
floor and not the walls round it, everything but a face left as it came, or a
face finished earlier by another tool. Surface groups put an operation's
faces in groups, each machined or avoided with a radial and an axial offset of
its own, and say the same of the model, every face in no group. What should
the tool do where a machined face and an avoided one meet, and which
operations take groups?

## What was decided

**In the job file, on the operation.** `Surfaces { model: { action }, groups:
[{ action, radial, axial, faces }] }`, written as `[setup.operation.surfaces]`
and `[[setup.operation.surfaces.group]]`, `action` `"machine"` or `"avoid"`
(`job/geometry.rs`). The offsets are millimetres beside and down, added to the
operation's stock to leave: on a face machined, more stock left on it; on one
avoided, how far the tool keeps from it. The faces are picks as every other
pick is ([decision 0002](0002-feature-references.md)), found once on their
model's body, so on every copy a sheet lays
([decision 0031](0031-arranged-sheets.md)). An operation whose surfaces are as
they start, no group and the model machined, writes nothing of them, so every
job file written before is written as it was.

**On every 3D finish.** Parallel, Scallop, Spiral, Radial, Morphed Spiral,
Pencil, Project, Morph, Contour, Ramp and Flat take them (`Uses::surfaces`).
Every other strategy refuses them: left unread, an avoided face would be cut.

**The part as the operation meets it.** Every strategy asks where its tool
rests, and where at a level it may not be, of `cutter::surfaces::PartSurfaces`,
not of the part's mesh. To an operation with no groups that is the whole part,
dropped onto through the drop-cutter every operation shares, as before there
were groups. To one with groups it is a mesh of each group's faces and one of
the model's, each with an index of its own and the tool grown for it. The tool
is held over it once, as a `Seat`; dropped, it rests on the highest of each
group's drops, exactly. At a level, a group's forbidden region is found as the
part's was and the regions joined (`LevelReach::grouped`). So every pass
already keeps an avoided face's clearance: with `ops_3d`'s dome avoided 1 mm
each way, a 6 mm flat end mill stays 11 mm from the dome's axis, its radius,
the dome's and the clearance.

**Kept to what is machined.** One transform after the strategy has laid its
passes and they are trimmed to the heights, `path::avoid::machined_only`, cuts
out what rests only on faces avoided, to within the tolerance, looked at along
each move at least every half the tool's radius. A tie is machined: the face
machined is finished right up to the clearance. What is cut out is not
lifted over: the pieces either side are passes of their own, for the linker to
join round. The cutting is the holder trim's (`path::held::cut_out`,
[decision 0021](0021-shank-and-holder.md)). A piece left that touches no face
machined anywhere is left out too: it would only go through the air towards
what is avoided, as Ramp's way down to a ring cut out does.

**Links keep off.** A link that would stay down resting on faces avoided, or
rubbing them beside it, lifts instead (`path/linker`).

**Flats.** Flat finishing clears only the flats it machines: an avoided flat
gets no level and no passes. Contour, Ramp and the rings round the walls of the
finishes over the part ([decision 0014](0014-walls-for-every-finish-over-the-part.md))
keep a level at an avoided flat, raised by its clearance, so a wall machined
standing on an avoided floor is finished down to the clearance. A flat machined
with stock of its own is cleared that much higher.

**Refused, each with its own message.** By the job: groups on a strategy that
takes none, an offset below zero or not finite, a group with no faces, a face of
a model the setup does not machine, and the model avoided with no group
machined: that is nothing to cut. By the run, which has the models: a face in
two groups, as any rule for which wins is one nobody can see in a job file. A
V-bit or chamfer mill on an operation that
avoids anything: grown by a clearance a cone is no cone, so how near it comes
to a face, beside it as well as above, cannot be told. A V-bit is refused a
group machined with stock of its own, as it is refused stock to leave.

**Checked.** CC111, a failure: the program as posted, after linking, arc
fitting and feed changes, every lead, ramp and link with the cuts, comes within
an avoided face's clearance. The faces are found again from the job, and the
tool, grown by each group's clearance and the stock to leave, is dropped along
every move that feeds, as CC102 lowers it along rapids; resting higher than the
move by more than the tolerance and smoothing's, it is inside the clearance.
Grouped Parallel, Scallop and Contour runs are in the
determinism gate.

## What was considered and not chosen

- **Rules for a face in two groups and for nothing machined**: the groups'
  order deciding the one, every face cut for the other. Both would run, and
  neither can be read from the job file: the one goes by the groups' order,
  the other cuts every face the file says to avoid.
- **Lifting a pass over what it avoids**, in one piece: the pieces either side
  are passes of their own, which the linker can join as it joins any two.
- **Cutting out every tie.** It leaves the roughing's stock on the machined
  floor round what is avoided: half a millimetre all round the dome on
  `ops_3d`.
- **Running on past the tie.** The tool would rest on the face avoided alone
  and take the material on it down to its clearance.

## Measured

On `ops_3d` roughed with a new adaptive clearing and a 6 mm flat, leaving half
a millimetre, one finish as the app makes a new one, with the groups below:
Parallel with none, with the dome avoided 1 mm each way, with the dome avoided
and no clearance, with the model avoided and only the floor machined, with
the floor machined with 0.5 mm of its own, and with the block's top avoided
2 mm down; Scallop and Spiral with none, with the dome avoided, and Scallop
with only the floor machined; Contour and Flat with none and with the dome
avoided; Ramp with the dome avoided. Every run passes every check, CC111
among them, and none cuts the part.

- **Coming back onto a cut piece.** camcraft lifts over what it avoids and
  comes back down onto each piece on a 2° ramp along it, over what the roughing
  left. Scallop with only the floor machined fed 1,133 mm at the cutting feed
  and 9,517 mm more down its ramps. Scallop and Spiral feed further with no
  groups too, for the same ramps: Scallop's new program lifts 44 times,
  Spiral's 71. Linking cut pieces otherwise is a change of its own.
- **At the tie.** camcraft stops where the ball touches both. With the floor
  machined with stock of its own, the lowest each goes, on the fillets beside
  it, is Z10.38.
- **Rings at a wall's foot.** With only the floor machined, the ring round the
  foot of each wall, touching the floor and the wall avoided, is kept and
  sweeps the roughing's stock along the walls.
- **Scallop's lines** are the lines it lays without groups, cut out where they
  rest on the dome alone: 83 lifts.
- **Spiral's laps** keep their direction and lift over the dome: 121 lifts.
- **Contour's and Ramp's floor ring** is kept where the ball touches the floor
  and the dome's clearance at once, and finishes the floor up to the
  clearance. Ramp's way down the dome is cut out, and the way down to it,
  touching nothing, left out with it; it lifts 4 times.
- **Not about groups.** A new Contour starts with the tool outside the
  silhouette and goes round the block's outside walls down to Z0
  ([decision 0027](0027-ramp.md)). Flat clears the tessellated dome's top at
  Z15 as a flat, ramps into each ring, and, with the dome avoided, clears
  slivers a tenth of a millimetre round between its clearance and the fillets
  at X-17.2, plunging into the 5.5 mm the roughing left there. Parallel,
  Scallop and Spiral leave the walls to rings round them
  ([decision 0014](0014-walls-for-every-finish-over-the-part.md)).

## Known limits

- **No roughing takes groups.** A roughing
  before a finish that avoids a face still cuts beside that face, down to its
  own stock to leave.
- **2D operations and Drill take none.**
- **A group is faces.** Avoiding a whole model means picking its faces.
- **No Ignore** mode, no fixture surfaces (fixtures are to be a setup's, planned
  on their own) and no machining over holes.
- **No V-bit or chamfer mill** where anything is avoided.
- **`camcraft post` cannot pick faces**: groups are set in job files and the
  app.
- **Not built:** turning a pass back at a clearance and staying down, as a
  spiral's laps could, where they now lift over what is avoided. Since
  [decision 0034](0034-entries-and-order-of-the-finishes-over-the-part.md) the
  pieces are taken the nearest next, from either end, and plunged onto.
  Scallop's lines are laid anew round what is avoided since
  [decision 0035](0035-scallop-over-every-slope.md).
- **Not yet measured on `ops_3d`:** Radial, Morphed Spiral, Pencil, Project
  and Morph with groups, which are tested on synthetic parts only, and
  bull nose tools.
