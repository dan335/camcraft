# 0030 — Morph

**Status:** decided, 2026-11-14 · **Milestone:** 0.9

## Question

Fusion's Morph lays passes between two curves picked on the model, each
changing shape a little from the one into the other, and drops them onto the
part. Which way do its passes run, how are they spaced, which point of the one
curve goes with which of the other, in what order are they cut, and how is the
tool linked between them?

## What was found

A fresh Morph read from Fusion through its API, on this PC (the older
`params/morph.json` agrees), keeps the tool's centre on the part's silhouette
(Tool Containment *center*, no additional offset) with Contact Only on. It
starts with Curve Selections empty, Slope off (0° to 90°), Rest Machining off,
tolerance 0.01 mm, Morph Mode *simple* (the others *closest* and *automatic*),
Pass Extension and Contour Offset none, Use Stepover on with a Maximum
Stepover of `Math.round(tool_cornerRadius != 0 ? (Math.sqrt((tool_cornerRadius)^2
- (tool_cornerRadius - tolerance)^2) * 2) / Math.sqrt(2.0) * 1000 :
(tool_diameter / 4) * 1000) / 1000`, parallel's cusp (0.346 mm for the 6 mm
ball), Number of Offset Stepovers none, Direction *both ways* (the others *one
way* and *other way*), Up/Down Milling *both*, Multiple Depths off, and Stock
to Leave, Fillets, Smoothing and Feed Optimization off. On its Linking tab it
has parallel's: a full retraction with Allow Rapid Retract, a safe distance of
2 mm, a Maximum Stay-Down Distance of 50 mm, *curve* transitions, and
vertical lead-in and lead-out radii a tenth of the tool. It has no ramp
settings.

`fixtures/step/ops_3d.step` ([decision 0027](0027-ramp.md)) needs nothing
added: its own edges are curves to morph between. Fusion generated and posted
a Morph on it with two curves picked; an earlier attempt that stopped on "No
curves selected" had none. Seventeen Morphs were set up with the sample
library's 6 mm ball; thirteen gave programs, posted with Fusion's Masso post,
and four, between open curves running opposite ways, gave none. They covered:

- the dome's foot and the floor's edge (the loop at Z10 where the fillets round
  it start), closed, picked in each order; both ways and one way; at the
  default stepover and at 2 mm; in each of the three morph modes;
- the round's two edges, open, the one along the top at X22 and the one along
  the block's side at X30, as picked (they run opposite ways) and with one
  reverted, in each order;
- the floor's two long edges at Y9 and Y-9, open, as picked (opposite ways),
  with one reverted, in each order, both ways and one way.

Three are committed (`fixtures/nc/ops_3d_morph*.nc`, by
`scripts/fusion/ops_3d/cam.py`): the dome's foot then the floor's edge, as a
new one starts and one way, and the round's upper edge then its lower,
reverted, as a new one starts. What the programs do:

- **Each pass is a step of the way from one curve to the other.** Between the
  floor's long edges, 28.25 and 27.25 mm long, the passes are lines whose ends
  are each a step of the way between the edges' ends: at Y-8.66 from X-20.251 to
  X7.019. Between the closed curves, every pass has a corner where the floor's
  loop has one, moved a step at a time from the corner's match on the dome's
  circle; the corners' matches on the circle are spaced round it as the corners
  are round the loop, by length (the loop's corners at 0.498, 0.195 and 0.307 of
  its length apart, their matches 0.498, 0.195 and 0.307 of the circle). So a
  point a fraction of the way along the one curve is matched with the point as
  far along the other, and each pass is the same step from every point to its
  match. Each pass is then dropped onto the part where the tool rests.
- **As many passes as keep matched points a stepover apart, for open
  curves.** The round's edges are 8 mm apart: 24 steps of 0.333 mm. The floor's
  long edges are 18 mm apart: 53 steps of 0.340 mm. For the closed curves
  Fusion took 29 steps, where its matched points are up to 12.8 mm apart; each
  pass is then up to 0.44 mm from the next, more than the stepover. Nothing
  measured between those curves explains 29.
- **Two open curves that run opposite ways give nothing.** Both pairs of open
  edges as picked, each running the other way, gave "No passes to link" and an
  empty toolpath; with one reverted, both gave passes.
- **Where a closed curve's match starts was not explained.** With the dome's
  foot picked first, the floor's corner at (8, 9) was matched with the circle at
  6.4° about the dome's axis; with the floor's edge picked first, at 37.5°. Where
  the matched points are closest on the whole is 34.5°, near the second but not
  the first; neither the curves' own starts nor their nearest points account
  for both. *Automatic* matched (8, 9) at 36° and spaced the others otherwise;
  *closest* gave other passes again. Only *simple*, a new one's, was followed.
- **Order: from the curve nearer the work zero.** Every program started at the
  curve whose passes come nearer the work zero, whichever was picked first: the
  dome's circle before the floor's loop, the round's upper edge before its lower,
  and between the floor's long edges the one at Y-9, whose end at (7, -9) is
  nearer than any other.
- **Direction.** One way ran every closed pass clockwise and every open one as
  the first curve runs. Both ways turned each pass the other way from the one
  before: the first open one from its end nearer the work zero, the first closed
  one anticlockwise.
- **Linking.** Between passes it stays down, along *curve* transitions at the
  ends of open passes just past the boundary, and over the part between closed
  ones a stepover apart. Into the first it rapids down to 2 mm above, feeds down
  at the plunge feed onto its vertical lead, a tenth of the tool, and goes up
  one off the last. Fusion split the closed passes into two runs each starting
  at the dome, and one way between the floor's edges started in the middle; its
  linker's order was not followed further.

## What was decided

**`strategy::morph_curves`, a pattern laid between the curves and dropped onto
the part.** The two curves picked are joined end to end, as a project's are,
seen from above and measured along. Every corner of either is a corner of every
pass, which is the same step of the way from each point of the first curve to
its match on the second. There are as many steps as keep every point of a pass
within a stepover of its match on the next: matched points move straight
towards each other, so they are furthest apart at a corner of one or the
other, and that is measured exactly. The passes are then dropped onto the part
as a project's curves are ([decision 0029](0029-project.md)):
`strategy::projected::project`, kept to the boundary, where the tool touches the
part and the slopes, sampled on the part exactly. The measuring along a line is
morphed spiral's (`strategy::morph::Lap`), now shared.

**Two open curves are matched end to end, the second turned round where it
runs the other way.** Fusion gives no passes for two that run opposite ways,
and its panel can turn a chain round; camcraft picks an edge as it runs, with no
way to turn it, so it turns the second to pair the ends that do not cross.

**Two closed curves both run clockwise, the second started where the matched
points are closest on the whole,** the sum of their distances squared, judged
at evenly spaced places along both, at least one for each stepover of the
longer curve. Fusion's own start for its simple mode was not found (above):
this is its Automatic mode's aim, near its simple mode's with the floor's edge
picked first, and the same whichever is picked first. One open and one closed
are refused, as are picks that are not two curves.

**Spacing: the Maximum Stepover held.** Fusion's count for the open curves is
camcraft's. For the closed ones camcraft lays more passes than Fusion's 29, so
that none is more than the stepover from the next.

**Order and direction as Fusion's programs.** The passes start at the curve
nearer the work zero; one way runs each as the first curve picked runs, other
way the reverse, both ways each the other way from the one before, the first
from its end nearer the work zero when open and anticlockwise when closed.

**Linking as parallel's.** It stays down within 50 mm, as Fusion's Morph's
linking starts, and has no ramp: into a pass it comes straight down at the
plunge feed onto its vertical lead, as Fusion's does. Its retraction is
camcraft's default, as parallel's is.

**Settings.** `direction` (`"both-ways"`, `"one-way"`, `"other-way"`, Project's
own choices) and the slopes, with the stepover starting as parallel's. The
CLI's `--strategy` does not offer it: `camcraft post` has no way to pick an
edge.

## Held to Fusion

`crates/camcraft-session/src/compare/tests_morph.rs` roughs the part with a new
adaptive clearing and a 6 mm flat, leaving half a millimetre. It then makes a
new Morph between the same edges and cuts Fusion's program from the material
the roughing left, with the same ball. Neither program cuts the part, neither's
rapids meet anything, both go as deep to within a tolerance, and:

| Program         | Curves              | Columns cut | Alike | Median    | 95th      | Removed | Fed    |
|-----------------|---------------------|------------:|------:|----------:|----------:|--------:|-------:|
| `morph`         | dome's foot, floor  |     436,148 | 91.9% | 0.0038 mm | 0.0137 mm | +0.04%  | +13.1% |
| `morph_one_way` | dome's foot, floor  |     436,148 | 91.9% | 0.0038 mm | 0.0137 mm | +0.04%  | +12.1% |
| `morph_open`    | the round's edges   |     171,445 | 99.4% | 0.0013 mm | 0.0048 mm | -0.16%  | +0.3%  |

The test holds each to a little worse than that.

- **Between the round's edges** the passes are Fusion's to within the
  tolerance: the same 25 lines, each where the ball rests on the round.
- **Between the dome's foot and the floor's edge** they are not. Fusion matches
  the floor's corner at (8, 9) with the circle 6.4° round from +X about the
  dome's axis, camcraft near 34.5°, so the passes between bend in other places;
  and camcraft lays a few more than Fusion's 30, so none is more than the
  stepover from the next. On the flat floor both cut alike. On the dome's steep
  flank their cusps fall in other places, a band of columns a few hundredths
  apart all round it, which is most of what the figures lose; the extra passes
  are most of the length fed. The rest are in the floor's corners, by the
  difference map, where the passes turn up the fillets and the walls; there a
  few columns differ by up to 2.8 mm.
- One of Fusion's rapids down onto a pass meets what the roughing left,
  0.70 mm deep; the test allows Fusion that.

## Not yet

- **Not built:** Fusion's Morph Mode (camcraft's matching for two closed curves
  is its own, above; *closest* and *automatic* are not offered), Pass Extension,
  Contour Offset, Number of Stepovers in place of a stepover, Number of Offset
  Stepovers, Up/Down Milling, Multiple Depths, Fillets, Rest Machining, Contact
  Point Boundary, sketches, a chain's direction picked in the view, and Fusion's
  linker's order between passes.
- **Not compared with Fusion's programs:** other way, the slopes, stock to
  leave, a stepover of its own, and flat or bull nose tools.
