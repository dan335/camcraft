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
face finished earlier by another tool. Fusion's Surface Manager, on an
operation's Geometry tab, puts faces in groups, each machined or avoided with a
radial and an axial offset of its own, and says the same of the model, every
face in no group. What does Fusion's tool do where a machined face and an
avoided one meet, how does camcraft keep to it, and which operations take it?

## What was found

`scripts/fusion/ops_3d/cam.py` sets groups through Fusion's API as the Surface
Manager sets them: the model's mode on its default group, then a direct
selection of faces for each group, machined or avoided, with a radial and an
axial offset, which the API takes in millimetres, not the centimetres its
lengths are elsewhere. It posted sixteen programs of the `ops_3d` part with them
(`scripts/fusion/README.md`): a new Parallel, Scallop, Spiral, Contour and Flat,
and each again with the dome avoided 1 mm sideways and 1 mm down; Parallel too
with the dome avoided and no clearance, with the model avoided and only the
pocket's floor machined, with the floor machined with 0.5 mm of stock of its own
each way, and with the block's top avoided 2 mm down; Scallop with only the
floor machined; and Ramp with the dome avoided 1 mm each way. What they do:

- **Each group is met by the tool grown by its offsets, and the tool rests on
  whatever is highest.** With the dome avoided 1 mm each way, the 6 mm ball
  touches the floor and the dome's clearance at once 10.04 mm from the dome's
  axis, which is where Contour cuts its ring and Spiral turns its laps back;
  Flat's 6 mm flat end mill stays 11 mm from it, its radius, the dome's and the
  clearance. With the top avoided 2 mm down, Parallel neither cuts the top nor
  rolls over its edges into the pocket, where the ball would rest on the top
  raised. With the floor machined with 0.5 mm of its own, Parallel leaves it at
  Z10.5.
- **A pass is kept where it touches a face machined, and cut out where it
  touches only faces avoided.** Fusion lifts between the pieces, its rapids
  doubled where a face is avoided (`path/avoid.rs`).
- **Where the tool touches both, Fusion goes either way.** Parallel's passes
  and Spiral's laps run on past the tie, up the face avoided: round the dome's
  foot Parallel's points reach Z10.69 with no clearance and Z10.93 with 1 mm,
  where the tie is at Z10. Contour's and Ramp's rings at the floor are cut out
  there, and go round the dome's foot nowhere. Measured earlier round a boss,
  Fusion's ring at the foot of an avoided wall, touching the machined floor,
  stayed (`cutter/surfaces.rs`).
- **Scallop lays its lines anew round what it avoids**: on the floor round the
  avoided dome its points stand in rings about the dome's axis a stepover
  apart, from 10.2 mm out, as from a boundary of its own.
- **Spiral turns each lap back** where the ball meets the dome's clearance and
  runs the next the other way, staying down.
- **Fusion takes settings camcraft refuses.** With the model avoided and no
  group machined it cuts everything; a face in two groups goes by the lower
  group; roughing (Adaptive and Pocket Clearing) with any group set answered
  "Toolpath is empty".

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
already keeps an avoided face's clearance.

**Kept to what is machined.** One transform after the strategy has laid its
passes and they are trimmed to the heights, `path::avoid::machined_only`, cuts
out what rests only on faces avoided, to within the tolerance, looked at along
each move at least every half the tool's radius. A tie is machined, as
Fusion's Parallel and Spiral machine it and its ring round a boss kept it: the
face machined is finished right up to the clearance. What is cut out is not
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
machined. Fusion cuts everything then; here that is nothing to cut. By the
run, which has the models: a face in two groups. Fusion lets the lower win, a
rule nobody can see in a job file. A V-bit or chamfer mill on an operation that
avoids anything: grown by a clearance a cone is no cone, so how near it comes
to a face, beside it as well as above, cannot be told. A V-bit is refused a
group machined with stock of its own, as it is refused stock to leave.

**Checked.** CC111, a failure: the program as posted, after linking, arc
fitting and feed changes, every lead, ramp and link with the cuts, comes within
an avoided face's clearance. The faces are found again from the job, and the
tool, grown by each group's clearance and the stock to leave, is dropped along
every move that feeds, as CC102 lowers it along rapids; resting higher than the
move by more than the tolerance and smoothing's, it is inside the clearance.
Fusion has no such check. Grouped Parallel, Scallop and Contour runs are in the
determinism gate.

## What was considered and not chosen

- **Fusion's rules for a face in two groups and for nothing machined.** Both
  would run, and neither can be read from the job file: the one goes by the
  groups' order, the other cuts every face the file says to avoid.
- **Lifting a pass over what it avoids**, in one piece. Fusion's programs lift
  between the pieces; the pieces are the passes.
- **Cutting out every tie**, as Fusion's Contour and Ramp did at the dome's
  foot. It leaves the roughing's stock on the machined floor round what is
  avoided: half a millimetre all round the dome in Fusion's programs.
- **Running on past the tie**, as Fusion's Parallel, Scallop and Spiral do. The
  tool would rest on the face avoided alone and take the material on it down to
  its clearance, as Fusion's did on the dome's flank.

## Held to Fusion

The comparison tests (`crates/camcraft-session/src/compare/tests_parallel.rs`,
`tests_scallop.rs`, `tests_spiral.rs`, `tests_contour.rs`, `tests_flat.rs` and
`tests_ramp.rs`) rough the part with a new adaptive clearing and a 6 mm flat,
leaving half a millimetre, then make one finish as the app makes a new one,
with the same tool, feeds and groups, and cut Fusion's program from the
material the roughing left. camcraft's run passes every check, CC111 among
them, neither program cuts the part, and:

| Program                 | Groups                           | Columns cut | Alike | Median    | 95th     | Removed | Fed    |
|-------------------------|----------------------------------|------------:|------:|----------:|---------:|--------:|-------:|
| `parallel`              | none                             |     735,513 | 74.7% | 0.0046 mm | 0.060 mm | +1.8%   | +45%   |
| `parallel_avoid_dome`   | dome avoided, 1 mm each way      |     642,835 | 70.0% | 0.0047 mm | 0.106 mm | +1.9%   | +110%  |
| `parallel_avoid_dome_0` | dome avoided, no clearance       |     675,708 | 72.3% | 0.0046 mm | 0.075 mm | +1.6%   | +90%   |
| `parallel_floor_only`   | model avoided, floor machined    |     412,705 | 55.1% | 0.0078 mm | 0.51 mm  | +0.4%   | ×4     |
| `parallel_floor_leave`  | floor machined, 0.5 mm its own   |     643,587 | 65.5% | 0.0054 mm | 0.13 mm  | +1.9%   | +45%   |
| `parallel_avoid_top`    | top avoided, 2 mm down           |     705,115 | 73.8% | 0.0047 mm | 0.062 mm | +1.9%   | +108%  |
| `scallop`               | none                             |     733,638 | 89.2% | 0.0031 mm | 0.015 mm | +0.03%  | +99%   |
| `scallop_avoid_dome`    | dome avoided, 1 mm each way      |     685,840 | 61.7% | 0.0056 mm | 0.24 mm  | -1.1%   | +221%  |
| `scallop_floor_only`    | model avoided, floor machined    |     447,140 | 42.9% | 0.028 mm  | 3.6 mm   | -11.3%  | ×9     |
| `spiral`                | none                             |     734,626 | 84.9% | 0.0036 mm | 0.036 mm | +0.6%   | ×4     |
| `spiral_avoid_dome`     | dome avoided, 1 mm each way      |     641,428 | 80.6% | 0.0035 mm | 0.054 mm | +0.7%   | ×6     |
| `contour`               | none                             |     701,863 | 55.5% | 0.0073 mm | 17.75 mm | +45%    | +197%  |
| `contour_avoid_dome`    | dome avoided, 1 mm each way      |     621,661 | 43.8% | 0.0174 mm | 17.93 mm | +64%    | +238%  |
| `flat`                  | none                             |     235,274 | 65.9% | 0         | 0.5 mm   | -4.3%   | +357%  |
| `flat_avoid_dome`       | dome avoided, 1 mm each way      |     126,216 | 70.5% | 0         | 5.5 mm   | +210%   | +258%  |
| `ramp_avoid_dome`       | dome avoided, 1 mm each way      |     611,524 | 59.7% | 0.0054 mm | 1.57 mm  | +5.3%   | +0.6%  |

The tests hold each to a little worse than that. Where the figures come from:

- **Coming back onto a cut piece.** camcraft lifts over what it avoids and
  comes back down onto each piece on a 2° ramp along it, over what the roughing
  left, where Fusion plunges. So it feeds about twice as far as Fusion's
  Parallel with the dome avoided; Scallop with only the floor machined fed
  1,133 mm at the cutting feed to Fusion's 1,173, and 9,517 mm more down its
  ramps. Scallop and Spiral feed further with no groups too, for the same
  ramps: Scallop's new program lifts 44 times to Fusion's once, Spiral's 71 to
  5. Linking cut pieces as Fusion does is a change of its own.
- **Past the tie.** Fusion's Parallel, Scallop and Spiral run on up the face
  avoided past where the ball touches both (above), cutting what the roughing
  left there up to 3.2 mm lower on the dome's flank (Parallel), 3.0 mm
  (Spiral) and 7.8 mm on the fillets and walls round the floor (Scallop, only
  the floor machined). camcraft stops at the tie. With the floor machined with
  stock of its own, the lowest each goes, on the fillets beside it, is Z10.38
  for camcraft and Z10.337 for Fusion.
- **Rings at a wall's foot.** With only the floor machined, camcraft's ring
  round the foot of each wall, touching the floor and the wall avoided, is kept
  and sweeps the roughing's stock along the walls up to 3.7 mm lower than
  Fusion's Parallel, whose passes touch the walls only where each ends.
- **Scallop's lines.** Fusion's are laid anew round the dome; camcraft's are
  the lines it lays without groups, cut out where they rest on the dome alone,
  so round the dome the cusps fall elsewhere. Fusion lifts 8 times to
  camcraft's 83.
- **Spiral's laps.** Fusion's turn back at the clearance and stay down;
  camcraft's keep their direction and lift over the dome: 121 lifts to
  Fusion's 6.
- **Contour's and Ramp's floor ring.** Fusion cuts it out where the ball
  touches the floor and the dome's clearance at once and leaves the roughing's
  half millimetre on the floor round the dome; camcraft keeps it and finishes
  the floor up to the clearance. Ramp's way down the dome is cut out, and the
  way down to it, touching nothing, left out with it; both lift 4 times.
- **Not about groups.** A new camcraft Contour starts with the tool outside the
  silhouette and goes round the block's outside walls down to Z0, where
  Fusion's keeps the tool's centre on the silhouette and stops at the floor
  ([decision 0027](0027-ramp.md)): most of Contour's 95th percentile. camcraft's
  Flat clears the tessellated dome's top at Z15 as a flat, ramps into each ring
  where Fusion's comes down once and stays down, and, with the dome avoided,
  clears slivers a tenth of a millimetre round between its clearance and the
  fillets at X-17.2, plunging into the 5.5 mm the roughing left there, which
  Fusion clears none of: three times as much removed. Parallel, Scallop and
  Spiral leave the walls to rings round them
  ([decision 0014](0014-walls-for-every-finish-over-the-part.md)).
- **Fusion's own programs.** Fusion does not post Parallel, Scallop or Spiral
  the same twice: run again, some came out with points moved a few hundredths
  of a millimetre along their passes by its point reduction
  (`scripts/fusion/README.md`). A fixture of those is one of Fusion's answers.

## Known limits

- **No roughing takes groups.** Fusion answered "Toolpath is empty" for
  Adaptive and Pocket Clearing with any group set, so its rule there is not
  established, and camcraft's operations follow Fusion's by test. A roughing
  before a finish that avoids a face still cuts beside that face, down to its
  own stock to leave.
- **2D operations and Drill take none**, though Fusion's Drill does.
- **A group is faces.** Avoiding a whole model means picking its faces.
- **No Ignore** mode, no fixture surfaces (fixtures are to be a setup's, planned
  on their own) and no machining over holes.
- **No V-bit or chamfer mill** where anything is avoided.
- **`camcraft post` cannot pick faces**: groups are set in job files and the
  app.
- **Not built:** linking cut pieces as Fusion does (plunging onto a pass,
  turning back at a clearance and staying down), and Fusion's runs past the
  tie. Since
  [decision 0034](0034-entries-and-order-of-the-finishes-over-the-part.md) the
  pieces are taken the nearest next, from either end, and plunged onto.
  Scallop's lines are laid anew round what is avoided since
  [decision 0035](0035-scallop-over-every-slope.md).
- **Not compared with Fusion's programs:** Radial, Morphed Spiral, Pencil,
  Project and Morph with groups, which are tested on synthetic parts only, and
  bull nose tools.
