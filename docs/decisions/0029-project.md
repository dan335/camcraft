# 0029 — Project

**Status:** decided, 2026-11-14 · **Milestone:** 0.9

## Question

Fusion's Project runs the tool along curves picked on the model or a sketch,
each projected onto the part. Autodesk's reference calls it "a toolpath that
follows selected contours" projected onto the model, with modes that project
toward or away from a point or a curve as well as straight down. On a 3-axis
router only straight down means anything. Which way does each curve run,
where does it start, in what order are the curves cut, and how is the tool
linked between them?

## What was found

A fresh Project read from Fusion through its API, on this PC
(Fusion 2706.1.14; the older `params/project.json` agrees), keeps the tool's
centre on the part's silhouette (Tool Containment *center*, no additional
offset) with Contact Only on. Its Projection Mode is *drop*, greyed out, the
only one for a 3-axis setup. It starts with Curve Selections empty, Slope off
(0° to 90°), Rest Machining off, tolerance 0.01 mm, Direction *both ways*
(the others *one way* and *other way*), Axial Offset 0, Up/Down Milling
*both*, Multiple Depths off (a Maximum Stepdown of 1 mm and one stepdown when
on), and Stock to Leave, Fillets, Smoothing and Feed Optimization off. It has
no stepover. On its Linking tab it has a full retraction with Allow Rapid
Retract, a safe distance of 2 mm, a Maximum Stay-Down Distance of five tools,
and vertical lead-in and lead-out radii a tenth of the tool. It has no
horizontal leads and no ramp settings.

`fixtures/step/ops_3d.step` ([decision 0027](0027-ramp.md)) needs nothing
added: its own edges are curves to follow. Twenty-seven programs were posted
with Fusion's Masso post, each with the sample library's 6 mm ball unless
said. They covered:

- the dome's foot, the pocket's rim and the line where the R8 round starts on
  the top, alone and together;
- each of those one way, the other way, and with the chain turned round
  (`isReverted`);
- the dome with the 12 mm ball, an axial offset of -1 mm, and stock to leave
  of 0.5 mm;
- the round's line with three stepdowns a millimetre apart;
- a circle and an open edge, both ways and one way;
- an open line, a circle and another open line, picked in an order that is
  not the nearest.

Four are committed (`fixtures/nc/ops_3d_project*.nc`, by
`scripts/fusion/ops_3d/cam.py`): the dome's foot, the floor's edge (the loop
at Z10 where the fillets round it start) and the round's lower edge along
the block's side, as a new one starts, the other way, with an axial offset of
1 mm, and with the 12 mm ball. The pocket's rim and the top's line were left
out of the committed set because a ball on them, at Z20, cuts nothing the
roughing left. What the programs do:

- **Each point is where the tool rests.** The curve's height plays no part: a
  ball centred on the rim rests at Z20, on the dome's foot (a circle at Z10)
  on the dome's flank at Z12.286 to Z12.292, and on the round's lower edge
  (Z12) on the round at Z16.548. Where the 6 mm ball rests on the dome there
  exactly is Z12.2916, and with the 12 mm ball Z13.0263, where Fusion's lowest
  is Z13.018.
- **One way: closed curves clockwise, open ones as picked.** The dome's
  circle, the pocket's rim and the block's top loop all ran clockwise seen
  from above. An open edge ran from its start, as its chain was made. Turning
  a chain round (`isReverted`) turned its pass round, so one way follows the
  chain, and Fusion's new closed chains run clockwise. Other way is the
  reverse of each.
- **Both ways: an open curve from its nearer end.** As one way, but after the
  dome the edge at X12 was cut from (12, -8), its end, nearer where the dome
  left the tool, and one way cut it from (12, 13).
- **In the order picked.** An open line at X22, the dome's circle, then an
  open edge at X12 were cut in that order, though the edge at X12 was the
  nearer after the line.
- **A closed curve starts where the last left off.** The first curve picked
  starts at its own start (the dome's circle at (-1, 0), its edge's vertex).
  Each closed curve after it starts at its point nearest where the tool is:
  the dome after the line at X22 near (-2.2, -3.9), the floor's edge after
  the dome at (-1, 9).
- **Axial offset moves every pass up.** At 1 mm every point is a millimetre
  higher. At -1 mm the dome's pass went a millimetre into the dome.
- **Linking.** Between every two curves it lifts to the retract height and
  rapids over. That held even between two curves 10 mm apart on the same flat
  top, well within its stay-down distance of 30 mm. It rapids down to 2 mm
  above where the curve's vertical lead starts, feeds down at the plunge feed
  onto the lead, a tenth of the tool, and goes up one off its end.
- **Stock to leave** of 0.5 mm put the dome's pass where a ball half a
  millimetre larger rests (Z12.945). **Multiple depths** cut the open line
  three times above its own pass, a stepdown apart, as Trace's levels are.
  Neither was followed further.

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

**It never stays down.** Fusion's programs lifted between every two curves,
even 10 mm apart on one flat; camcraft's first version stayed down within
five tools, as pencil does, and cut 1.86 mm into what the roughing left on its
way from the dome to the next curve. A project lifts to the retract height
(a full retraction, as Fusion's starts) and comes straight down onto each
curve along its vertical lead, never ramping, as Fusion's does.

**Settings.** `direction` (`"both-ways"`, `"one-way"`, `"other-way"`),
`axial_offset` and the slopes, each as a new Project of Fusion's starts. The
CLI's `--strategy` does not offer it: `camcraft post` has no way to pick an
edge.

## Held to Fusion

`crates/camcraft-session/src/compare/tests_project.rs` roughs the part with a
new adaptive clearing and a 6 mm flat, leaving half a millimetre. It then
makes a new Project along the same seven edges, and cuts Fusion's program
from the material the roughing left, with the same ball. Neither program cuts
the part, camcraft's rapids meet nothing, both go as deep to within a
tolerance, and:

| Program             | Ball  | Columns cut | Alike | Median    | 95th      | Removed | Fed   |
|---------------------|-------|------------:|------:|----------:|----------:|--------:|------:|
| `project`           | 6 mm  |     432,937 | 98.8% | 0.0038 mm | 0.0045 mm | +0.05%  | +7.6% |
| `project_other_way` | 6 mm  |     432,937 | 98.8% | 0.0038 mm | 0.0045 mm | +0.05%  | +7.6% |
| `project_axial`     | 6 mm  |     403,553 | 99.1% | 0.0038 mm | 0.0044 mm | +0.06%  | +7.1% |
| `project_ball12`    | 12 mm |     358,704 | 90.1% | 0.0049 mm | 0.0120 mm | -0.30%  | +9.2% |

The test holds each to a little worse than that. The passes are Fusion's to
within the tolerance:

- **Round the dome's foot**, camcraft's ball rests on the dome exactly, and
  Fusion's points lie up to 0.006 mm below that with the 6 mm ball and
  0.008 mm with the 12 mm. Fusion's chords also lie a little outside the
  circle (its points 7.003 mm out from the dome's axis on average to
  camcraft's 7.000). Where the ball's side is steep that is a band of columns
  just past the tolerance all round. With the 12 mm ball it is most of what
  the figures lose.
- **Along the floor's edge**, Fusion's ball is 0.004 mm above the floor,
  camcraft's on it.
- **A few columns**, in the R5 corner and at the open line's ends, differ by
  up to 2.7 mm, where the leads and the corner's arc fall.

The lengths fed differ by more than the material: the links and the leads,
not compared move by move. Fusion's rapids down to 2 mm above where it comes
onto a curve meet what the roughing left twice, 0.77 mm deep at most (0.26
with the 12 mm ball); the test allows Fusion that.

## Not yet

- **Not built:** Fusion's other projection modes (to and from a point or a
  curve), which it greys out for 3-axis machining; sketches (camcraft picks edges of the
  model only); a chain's direction picked in the view (closed curves run
  clockwise as Fusion's new chains do); Up/Down Milling; Multiple Depths;
  Fillets; Rest Machining; Contact Point Boundary.
- **Not compared with Fusion's programs:** one way, the slopes, stock to
  leave, negative axial offsets and flat or bull nose tools.
