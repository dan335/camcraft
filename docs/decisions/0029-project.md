# 0029 — Project

**Status:** decided, 2026-11-14 · **Milestone:** 0.9

## Question

Project runs the tool along curves picked on the model, each projected onto
the part. A curve could be projected toward or away from a point or a curve
as well as straight down, but on a 3-axis router only straight down means
anything. Which way does each curve run, where does it start, in what order
are the curves cut, and how is the tool linked between them?

## What a new one starts with

The tool's centre kept to the part's silhouette, no additional offset, contact
only; projected straight down; Slope off (0° to 90°); tolerance 0.01 mm;
Direction *both ways*; Axial Offset 0. It has no stepover. Linking: a full
retraction, a safe distance of 2 mm, and vertical lead-in and lead-out radii
a tenth of the tool; no horizontal leads and no ramp.

`ops_3d` ([decision 0027](0027-ramp.md)) needs nothing added: its own edges
are curves to follow. The curve's height plays no part: each point is where
the tool rests, so a ball centred on the dome's foot, a circle at Z10, rests
on the dome's flank, and one on the round's lower edge (Z12) rests on the
round.

## What was decided

**`strategy::project`, a pattern dropped onto the part.** The picked edges
are joined end to end, as a trace's are. Each curve is seen from above,
within the tolerance of its arcs, and laid out as a line of a pattern. It is
then dropped onto the part as a spiral's laps are
([decision 0011](0011-patterns-dropped-onto-the-part.md)):
`strategy::projected::project`, kept to the boundary, where the tool touches
the part, and the slopes, sampled on the part exactly, and broken where it
falls further than a cut step. What is Project's own is small:

- which way each curve runs: one way clockwise or as picked, other way the
  reverse, both ways as one way but an open curve from its end nearer where
  the last left the tool;
- the order, as picked, each closed curve started at its point nearest where
  the tool is, which `project` already did for a pattern;
- the axial offset, a `raise` the projected passes now carry, which every
  pattern leaves at none.

A curve has no stepover. Its line is seeded every quarter of the tool's
width, as a link over the part is, and refined to the tolerance.

**Its picks are its curves.** A project has a boundary, the part's silhouette
to start, and picked edges that are its curves. So the boundary cannot be its
picks too. The job refuses `boundary.kind = "selection"` for any strategy
whose picks are curves it follows, and the app does not offer it.

**It never stays down.** camcraft's first version stayed down within five
tools, as pencil does, and cut 1.86 mm into what the roughing left on its way
from the dome to the next curve: the way between two picked curves crosses
whatever is still standing. A project lifts to the retract height (a full
retraction) and comes straight down onto each curve along its vertical lead,
never ramping.

**Settings.** `direction` (`"both-ways"`, `"one-way"`, `"other-way"`),
`axial_offset` and the slopes. The CLI's `--strategy` does not offer it:
`camcraft post` has no way to pick an edge.

## Measured

On `ops_3d` roughed with a new adaptive clearing and a 6 mm flat, leaving half
a millimetre, a new Project along the dome's foot, the floor's edge (the loop
at Z10 where the fillets round it start) and the round's lower edge along the
block's side, as a new one starts, the other way, with an axial offset of
1 mm, and with a 12 mm ball: none cuts the part, and its rapids meet nothing.
Round the dome's foot the ball rests on the dome exactly, at Z12.2916 with
the 6 mm ball and Z13.0263 with the 12 mm, its points 7.000 mm out from the
dome's axis; along the floor's edge the ball is on the floor. With the axial
offset every point is a millimetre higher.

## Not yet

- **Not built:** the other projection modes (to and from a point or a
  curve), which mean nothing on a 3-axis machine; sketches (camcraft picks
  edges of the model only); a chain's direction picked in the view (closed
  curves run clockwise); Up/Down Milling; Multiple Depths; Fillets; Rest
  Machining; Contact Point Boundary.
- **Not yet measured on a job:** one way, the slopes, stock to leave,
  negative axial offsets and flat or bull nose tools.
