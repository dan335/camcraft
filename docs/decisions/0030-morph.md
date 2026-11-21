# 0030 — Morph

**Status:** decided, 2026-11-14 · **Milestone:** 0.9

## Question

Morph lays passes between two curves picked on the model, each changing shape
a little from the one into the other, and drops them onto the part. Which way
do its passes run, how are they spaced, which point of the one curve goes with
which of the other, in what order are they cut, and how is the tool linked
between them?

## What a new one starts with

The tool's centre kept to the part's silhouette, no additional offset, contact
only; Slope off (0° to 90°); tolerance 0.01 mm; a maximum stepover of
parallel's, the tolerance's cusp with a round end (0.346 mm for a 6 mm ball)
and a quarter of the tool with a square one; Direction *both ways*. Linking
as parallel's: a safe distance of 2 mm, a stay-down distance of 50 mm, smooth
transitions, and vertical lead-in and lead-out radii a tenth of the tool; no
ramp. `ops_3d` ([decision 0027](0027-ramp.md)) needs nothing added: its own
edges are curves to morph between.

## What was decided

**`strategy::morph_curves`, a pattern laid between the curves and dropped onto
the part.** The two curves picked are joined end to end, as a project's are,
seen from above and measured along: a point a fraction of the way along the
one is matched with the point as far along the other. Every corner of either
is a corner of every pass, which is the same step of the way from each point of the first curve to
its match on the second. There are as many steps as keep every point of a pass
within a stepover of its match on the next: matched points move straight
towards each other, so they are furthest apart at a corner of one or the
other, and that is measured exactly. The passes are then dropped onto the part
as a project's curves are ([decision 0029](0029-project.md)):
`strategy::projected::project`, kept to the boundary, where the tool touches the
part and the slopes, sampled on the part exactly. The measuring along a line is
morphed spiral's (`strategy::morph::Lap`), now shared.

**Two open curves are matched end to end, the second turned round where it
runs the other way.** camcraft picks an edge as it runs, with no way to turn
it, so it turns the second to pair the ends that do not cross; matched as
picked, the passes between two curves running opposite ways would cross.

**Two closed curves both run clockwise, the second started where the matched
points are closest on the whole,** the sum of their distances squared, judged
at evenly spaced places along both, at least one for each stepover of the
longer curve. It is the same whichever is picked first. One open and one
closed are refused, as are picks that are not two curves.

**Spacing: the Maximum Stepover held.** As many passes as keep each within a
stepover of the next: between the round's edges, 8 mm apart, 24 steps of
0.333 mm; between the floor's long edges, 18 mm apart, 53 of 0.340 mm. Between
two closed curves, whose matched points can be far apart at a corner, as many
as that takes there.

**Order and direction.** The passes start at the curve nearer the work zero;
one way runs each as the first curve picked runs, other way the reverse, both ways each the other way from the one before, the first
from its end nearer the work zero when open and anticlockwise when closed.

**Linking as parallel's.** It stays down within 50 mm and has no ramp: into a
pass it comes straight down at the plunge feed onto its vertical lead. Its
retraction is camcraft's default, as parallel's is.

**Settings.** `direction` (`"both-ways"`, `"one-way"`, `"other-way"`, Project's
own choices) and the slopes, with the stepover starting as parallel's. The
CLI's `--strategy` does not offer it: `camcraft post` has no way to pick an
edge.

## Measured

On `ops_3d` roughed with a new adaptive clearing and a 6 mm flat, leaving half
a millimetre, a new Morph with a 6 mm ball between the dome's foot and the
floor's edge (the loop at Z10 where the fillets round it start), both ways
and one way, and between the round's two edges: none cuts the part, and no
rapid meets anything. Between the round's edges it lays 25 lines, each where
the ball rests on the round. Between the dome's foot and the floor's edge the
floor's corner at (8, 9) is matched with the circle near 34.5° round from +X
about the dome's axis, where the matched points are closest on the whole.

## Not yet

- **Not built:** other ways to match two closed curves (camcraft's is its
  own, above), Pass Extension, Contour Offset, Number of Stepovers in place of
  a stepover, Number of Offset Stepovers, Up/Down Milling, Multiple Depths,
  Fillets, Rest Machining, Contact Point Boundary, sketches, and a chain's
  direction picked in the view.
- **Not yet measured on a job:** other way, the slopes, stock to leave, a
  stepover of its own, and flat or bull nose tools.
