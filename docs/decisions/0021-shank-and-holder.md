# 0021 — Shank and holder: the whole tool, checked and avoided

**Status:** built, 2026-11-05 · **Milestone:** 0.6

## Question

Only the cutting end of a tool is cut with, simulated and checked. Its shank,
its holder and the spindle's nose go wherever the tip goes, and nothing looks
at whether they meet the material or the part. Adaptive's new default
stepdown, three quarters of the flutes, makes it more likely: a deep pocket
takes the shank in with it, and a long one the collet nut. How should
camcraft know the rest of the tool, check it, and keep it clear? Does that
need a machine, with its spindle's taper (ISO20, ISO30, an ER collet), and
does a holder belong in the tool library?

## How others do it

- **Fusion** keeps the holder on the tool: a stack of segments from the nose
  up, each a height and a lower and upper diameter, so a taper is one
  segment; an optional *shaft* between tool and holder the same way; and the
  tool's *body length* (`LB`, length below the holder) placing the holder.
  Its library has holder folders, and a holder is chosen onto a tool, not
  onto an operation or a machine. Each 3D operation has *Shaft and Holder*:
  off, **pull away** (lift, leaving material), **trimmed** (cut out the
  passes that would collide), **detect tool length** (the least body length
  that clears, written back to the tool), or **fail**; drilling skips the
  holes that would collide. Shaft clearance 1 mm, holder clearance 5 mm;
  adaptive can check against the stock as well as the part. In 3-axis it is
  off by default, and **simulation reports collisions anyway**, on the
  timeline and in its issues. A machine's spindle can be checked too
  (6 mm clearance), off by default. Nothing found ties holders to a machine's
  taper.
- **PowerMill** keeps a tool assembly (tip, shank, holder) and checks every
  toolpath against the model, or the model and the stock, splitting or
  lifting what collides, and works out the least overhang that clears.
  **Mastercam** has holder libraries and the same modes as Fusion (ignore,
  trim, pull away, detect length, fail).
- **Kiri:Moto** adds a wider shaft to the tool's drop profile, so its 3D
  passes keep it clear; **FreeCAD**, **Carbide Create** and, as far as found,
  **Vectric** do nothing for the shank or holder.
- **OpenCAMLib** gives the primitive the others rest on: a *composite
  cutter*, rings of cylinders and cones each at its own height, dropped onto
  the part as one tool.

## Answers

- **The holder belongs to the tool**, as in Fusion, PowerMill and Mastercam:
  how far the tool sticks out of it is a property of that tool as it is set
  up, and an ATC machine keeps each tool in its own holder. Holders get their
  own **library**, beside the tools' (a *Holders* folder, the System library's
  built in), and choosing one onto a tool copies it in, as a tool is copied
  into a job, so a job still checks what it was verified with.
- **A machine is needed, but small.** On a router that holds the bit in its
  own collet (a trim router, an 80 mm ER20 spindle) there is no holder: the
  collet nut and the spindle's nose are what reach down, and they are the
  same for every tool. So a **machine** says what its spindle is (its
  interface, ER11, ER20, ISO20, ISO30, BT30, informational and for filtering
  holders) and its **nose**, segments like a holder's. A tool with no holder
  of its own is checked with the machine's nose in its place. A setup names
  its machine; the job keeps a copy, as it does tools. The machine is where
  more will go later (travel, rapid rate for the time estimate, the post),
  but it starts with the spindle.
- **Checking is always on; avoiding is chosen per operation.** As in Fusion,
  every program is checked against the material in simulation, and a
  collision fails the setup's checks as a gouge does, so a program that would
  drive a holder into the work is never posted unawares. How generation keeps
  clear is each operation's choice, Fusion's modes, off by default as
  Fusion's are in 3-axis.

## The tool assembly

One profile for the whole tool, from the tip up, each piece a frustum (a
height, a lower and an upper diameter), as Fusion and OpenCAMLib describe it:

1. **The cutter**, as now: what cuts.
2. **The shoulder or neck**, from the flutes to the shoulder length.
3. **The shank**, at its diameter up to the stickout (Fusion's body length).
   An optional **shaft** (an extension) sits here, as Fusion's.
4. **The holder**, from the stickout up, or where the tool has none, the
   machine's **nose**.

`HolderSegment` becomes `{ length, lower_diameter, upper_diameter }`, a
cylinder where the two match, so an ER nut's chamfer and an ISO flange's
groove are drawn and checked as they are. What a tool leaves out is assumed
as the tool's drawing already assumes it, and **said**: a holder checked at an
assumed stickout says so, and asks for it to be measured, rather than passing
quietly.

## Checking: every program, in simulation

The simulator already sweeps each move's cutter through the material height
field. It sweeps the rest of the assembly too, cutting nothing, and a segment
that meets material is a **collision**. Contact is actual: Fusion's
clearances (1 mm on the shank, 5 mm on the holder) are for keeping clear, and a
check against them would fail programs that touch nothing. The body is taken
twice the operation's tolerance narrower (the job's, and smoothing's where it
smooths): a shank as wide as its cutter rides against the wall another pass
cut, and the two passes are each only where they should be to within that.

- The in-process material includes the part, so this covers part and stock
  at once, and fixtures if they are ever modelled as material.
- **The shank matters even at the cutter's own diameter**: the flutes only
  clear what they reach, so a shank going down past the flute length meets
  what is left above them. This is the case adaptive's new stepdown makes
  common.
- Each colliding move is counted per operation with its deepest intrusion and
  the segment that hit, a failing check (as gouges and rapids into material
  are), marked on the playback timeline and listed in the checks window, the
  playhead able to go to it with the assembly drawn in red there.
- **The least stickout** that would clear is worked out from the same sweep
  (Fusion's *Detect tool length*, Cimatron's *minimum clear length*) and
  reported per tool: "needs 38 mm out of the holder; it is set to 30".

A disk's maximum over the height field is what each segment asks at each
step; a holder 50 mm across covers many cells, so the field keeps a maximum
pyramid (each level the maximum of four below) to answer *nowhere above*
at once and look closely only where it might be. A sloping move is looked at
in pieces that drop no more than the tolerance, a straight plunge once. On
the regression job the check costs about 3% of the run.

### What it found

Turned on, the check failed the regression job, and each contact was a real
defect of a strategy, each now fixed:

- **Pocket clearing left thin rest material on walls** at every level but the
  bottom. It counted material only past its own stock to leave plus a cell's
  diagonal, then opened away anything narrower than two more: on a 0.175 mm
  grid, any skin less than about 0.75 mm thicker than its own leave, such as
  adaptive's 0.5 mm under its 0.2 mm, was not cut, and the bottom level's ring
  then ran under it with the shank rubbing it 20 mm up. Now a piece of what is
  past the leave and the tolerance counts wherever one of the grid's points,
  which the simulator cuts by, stands in it; the outline's slivers between the
  points, which have none, do not.
- **Adaptive left walls up to twice the tolerance proud**: material narrower
  than twice its sliver width, the tolerance, was not worth a pass. A level
  below, with material in front of the wall, ran it exactly, and the shank
  rubbed what the level above had left. The sliver is half the tolerance now.

## Avoiding: Fusion's modes, per operation

`passes.holder = { mode, shank_clearance, holder_clearance }`, modes
`off` (the default), `pull-away`, `trim`, `fail`, and for drilling,
`skip-holes`:

- **Finishing over the part** (parallel, scallop, spiral, radial, morphed
  spiral): the assembly dropped as one composite tool (OpenCAMLib's
  CompositeCutter). *Pull away* takes the highest the pieces allow, lifting
  the tip and leaving material; *trim* cuts out the stretches lifted by more
  than the tolerance and links round them; *fail* stops at the first.
- **Levels** (adaptive, pocket clearing, contour, flat): at each level the
  tool's centre is kept out of what each piece of the assembly would meet
  above it, the material as it stands then grown by the piece's radius and
  clearance: the region the level may clear shrinks, which is *trim*, the
  only meaningful mode at a fixed height.
- **Drilling**: a hole the assembly cannot reach is skipped and said.
- **Links** lift over material by the assembly, not the cutter alone, so a
  link never drags a holder across the work.

## Plan

As planned; what was built is marked.

1. **The assembly.** Frustum segments for holders and shafts; the profile
   built from a tool and its machine; what is assumed, said. The tool
   editor's holder table gains taper; its drawing and playback's cutter draw
   the whole assembly. *Built*, but for a shaft (an extension) between tool
   and holder.
2. **Machines.** A machine (name, spindle interface, nose segments);
   built-in ones for an 80 mm ER20 spindle and ISO30, BT30 and ISO20 ATC
   spindles, picked in the setup's Post tab; a setup names its machine, the
   job keeps a copy. *Built.* The person's own machines in a library of their
   own, and trim routers' noses, are not yet.
3. **Holders.** Built-in holders (the ER11 to ER32 nuts, BT30 ER25 and ER32,
   from makers' tables; ISO30 ER16 to ER32, 70 long, their 50 mm flange from
   Kemmler's drawing of its SK30 ER16), put onto a tool from its Holder tab.
   *Built.* A library of the person's own holders, and Fusion's JSON
   libraries imported with their holders and shafts, are not yet.
4. **The check.** Sweeping the assembly in simulation, collisions as a
   failing check, CC108, at its first move on the timeline and in the checks
   window, the whole tool drawn in playback; the stickout that would clear in
   its detail. *Built.*
5. **Avoiding, finishing over the part:** the composite drop and the modes.
   *Built*, in the cutting stage rather than planning, so it keeps clear of
   the material earlier operations left as well as the part: a finish is
   planned before they are cut. A move that is clear is kept as it is.
6. **Avoiding, levels:** the shrunk region at each level, against the part;
   contour and flat, and clearing where the levels above left material over
   it, trimmed against the material; links lifted by the rest of the tool.
   *Built.*
7. **Drilling** skips what the assembly cannot reach. *Built.*

## Not known yet

- ISO20's flange size, BT30's flange length (so its ER16 and ER20 holders,
  narrower than the 46 mm flange), and the noses of trim routers (Makita
  RT0701C, DeWalt 611) and 65 mm ER11 spindles: measured, or left out of the
  built-ins until they are.
- Fusion's JSON key for a segment above the gauge line.
- Whether a fixture should be modelled, as stock the tool must not cut.
