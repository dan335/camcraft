# 0028 — Pencil

**Status:** decided, 2026-11-14 · **Milestone:** 0.9

## Question

Fusion's Pencil runs the tool along the inside corners and fillets a larger
tool before it could not reach into. Autodesk's reference says it "creates
toolpaths along internal corners and fillets with small radii" and that a
pencil pass is made where the tool touches the part in two places. Which
places, as its Bitangency Angle decides; where does a line start and stop;
what do its Number of Stepovers, Overthickness and Direction do; and in what
order are the lines cut?

## What was found

A fresh Pencil read from Dan's Fusion through its API (`params/pencil.json`)
keeps the tool's centre on the silhouette (Tool Containment *center*, its
Additional Offset the stock to leave), and starts with Bitangency Angle 20°,
Overthickness none, Limit Number of Stepovers on with Number of Stepovers 0,
Stepover half the tool, Direction *both ways* (the others *one way* and
*other way*), Up/Down Milling *both*, Link from Inside to Outside off
(*outside → in* when on), Stock to Leave, Fillets, Smoothing, Feed
Optimization, Slope, Contact Point Boundary and Rest Machining off, tolerance
0.01 mm; on its Linking tab a full retraction, a safe distance of 2 mm, a
Maximum Stay-Down Distance of five tools, vertical leads a tenth of the tool,
the *smooth* transition, and no ramp settings at all.

`fixtures/step/ops_3d.step`, Ramp's part ([decision 0027](0027-ramp.md)),
gives a Pencil a good deal to do with the sample library's 12 mm ball, which
is larger than the pocket's R4 floor fillets and its R5 corner, and enough
with its 6 mm ball, which fits them. Eleven programs were posted with
Fusion's Masso post: a new Pencil with each ball; with the 12 mm ball one way,
the other way, a bitangency angle of 10° and of 40°, an overthickness of 1 mm,
two stepovers, and two stepovers a millimetre apart; with the 6 mm ball an
overthickness of 2 mm and two stepovers. Four are committed
(`fixtures/nc/ops_3d_pencil*.nc`, by `scripts/fusion/ops_3d/cam.py`): each
ball as a new one starts, the 12 mm at 40° and the 6 mm with two stepovers.
What the programs do:

- **A line is where the tool touches twice, a crease or a wall's foot.** With
  the 12 mm ball (two programs as new, and every 12 mm variant): all round
  the pocket's floor at Z10, a radius from the walls (Y±7.000, X6.000, and
  X-18.716 by the drafted wall), the ball's end on the floor and its side
  against the wall above the fillet; round the dome's foot where the ball
  rests on the floor and the dome, cut off where it meets that ring; up each
  sharp corner, the ball on both rim edges, at exactly the height that puts
  it there (Z19.766 where it is 1.659 mm from both); down the drafted wall at
  Y7 from Z14.023 to Z10.004, the ball against the +Y wall and on the drafted
  one; and a short one up the R5 corner to where its rim's arc takes over,
  (7, -8). With the 6 mm ball (two programs): round the dome's foot, radius
  8.89 about its middle at Z10; down each sharp corner's mitre, where the two
  fillets meet, and up the corner to the rim; and nothing along the walls or
  fillets, which the ball fits.
- **The bitangency angle ends a line.** The lines up the corners stop where
  the angle between the normals of the two rim edges the ball rests on falls
  below it: 11.6° at a setting of 10°, 22.5° at 20° and 41.1° at 40° (three
  programs), each where the last point before the lead out is, measured from
  the ball's centre to each edge. The 1.5° to 2.5° past the setting is not
  explained (one look).
- **One way has the wall on the right.** One way went anticlockwise round the
  pocket's floor and clockwise round the dome, every line with the higher
  side, wall or dome, on the tool's right; the other way, the reverse (two
  programs). With a clockwise spindle that is climbing. Both ways cut each
  line whichever way was nearer.
- **Straight down an upright corner.** Where the floor's ring meets a corner
  between two upright walls, (6, ±7), the ball touches both walls from Z10 to
  Z14 at one place seen from above: Fusion goes straight up or down those
  4 mm at the cutting feed.
- **Linking.** Between lines it stays down over the floor within five tools,
  as across from a corner's mitre to the dome's ring, and lifts otherwise;
  into a line it comes straight down at the plunge feed onto a vertical arc
  a tenth of the tool, and never ramps.
- **Overthickness finds the lines for a larger tool.** With the 6 mm ball and
  2 mm, its lines ran along the floor 5 mm from the walls (Y±8), where an R5
  ball touches floor and wall above the R4 fillets, at the 6 mm ball's own
  height there, as well as its own (one program). With the 12 mm ball and
  1 mm its lines lay along its own and a second set inside them, wavering
  round the dome (one program); that was not followed further.
- **Stepovers are offsets along the surface.** With two, passes run either
  side of every line a stepover and two apart along the surface: up the
  dome, out over the floor, up the walls and over the rim, with the 12 mm
  ball even past the block's edge, joining where they meet (three programs).
  Their order mixes outer and inner passes and the lines; it was not
  followed.

## What was decided

**`strategy::pencil`, from where the tool rests.** The tool's height dropped
onto the part is the highest of smooth sheets, one for each place it can
touch; where two meet it touches both, and the height has a crease, always a
valley, or, where one stands above the other, a wall between them, the
tool's side against it. The angle between the sheets either side, between
where the tool touches each, is the bitangency angle; at a wall's foot, the
angle between the lower sheet and the wall's face. A grid of drops, each
node given its sheet by drops a ten-thousandth of a millimetre either side,
draws where the lines run, cell to cell; each point is then found exactly
across the line by halving to an eighth of the tolerance, judged once more
there, and put on the lower sheet, at a wall's foot as far again from the
wall, at the height the tool rests at. The grid's pitch keeps a sheet's turn
from node to node under a quarter of the angle, and is no coarser than ten
tolerances: 0.1 mm here. A line is broken where more than two cells running
are not found again, and a point found behind the last is left out, so the
tool never goes back and forth. Thinned to a quarter of the tolerance, each
chord's middle is looked at again: found again on the line, or the tool's
height there, further than a quarter of the tolerance off, puts a point there.

**Not along a fillet the ball fits.** A ball whose side meets the wall where
a fillet below rises into it rests on a sheet that faces the wall's way
there: its height jumps at the wall, but the two places it touches are one.
The first version cut every wall of the pocket so with the 6 mm ball, along
the fillets' tops, where Fusion's does not; a wall's foot is a line only
where the lower sheet faces the wall's face at the angle or more, judged
where they meet.

**Down an upright corner, the linker.** camcraft refuses a cut that falls as
steeply as a plunge further than the deepest step cut (CC105); a stretch of a
line that does is left to the linker, which comes down it at the plunge feed,
as a parallel pass's is.

**Settings.** `direction` as Fusion's (both ways, one way as `climb`, other
way as `conventional`), `bitangency_angle`, `overthickness` (the lines found
for the tool grown all round by it, on top of the stock to leave, and cut by
the tool itself) and `stepovers`, `passes.stepover` apart, each as a new
Pencil of Fusion's starts. The passes beside the lines are lines of equal
distance from them along the tool's height over the part, worked out on the
grid as scallop's are ([decision 0009](0009-scallop-distance-field.md)), and
cut from the outermost in before the lines. It links over the part, staying
down within five tools, and comes straight down onto each line, with no ramp,
as Fusion's.

## Held to Fusion

`crates/camcraft-session/src/compare/tests_pencil.rs` roughs the part with a
new adaptive clearing and a 6 mm flat, leaving half a millimetre, then makes
a new Pencil, and cuts Fusion's program from the material the roughing left,
with the same ball. Neither program cuts the part, camcraft's rapids meet
nothing, both go as deep, and:

| Program             | Ball  | Columns cut | Alike | Median    | 95th      | Removed | Fed    |
|---------------------|-------|------------:|------:|----------:|----------:|--------:|-------:|
| `pencil`            | 12 mm |     524,965 | 95.3% | 0.0015 mm | 0.0098 mm | -0.09%  | -7.3%  |
| `pencil_bitangency` | 12 mm |     522,681 | 95.3% | 0.0015 mm | 0.0098 mm | -0.09%  | +0.9%  |
| `pencil_ball6`      | 6 mm  |     266,231 | 74.5% | 0.0021 mm | 2.35 mm   | +4.5%   | +9.6%  |
| `pencil_stepovers`  | 6 mm  |     522,836 | 69.3% | 0.0024 mm | 0.73 mm   | +2.3%   | +8.0%  |

The test holds each to a little worse than that. With the 12 mm ball the
lines are Fusion's: what is left is a band along the dome's foot, where
Fusion's line lies 0.006 mm nearer the dome than where the ball touches floor
and dome (10.434 mm from the dome's middle to camcraft's 10.440, the sphere's
own 10.4403), a tolerance's difference in the columns of the dome's steep
foot; the lines up the corners, which camcraft runs on to the angle asked and
Fusion stops a degree or two short of; and where each comes down onto its
lines and lifts off them. With the 6 mm ball the lines are Fusion's too, but the links between
them are not: both stay down over the floor through what the roughing left,
Fusion from a corner's mitre to the dome's ring and camcraft from the ring to
the next corner's mitre, past the drafted wall, where the roughing's 6 mm flat
left material standing between the wall and the dome. That is most of what
its figures lose, and the stepovers' too, whose passes run as Fusion's do but
are linked otherwise. Fusion's stepover program rapids down onto a pass into
the roughing's stock, 3.33 mm deep, once; the test allows it that.

A new Pencil of `ops_3d` with the 12 mm ball, posted by `camcraft post` from a
test build, took four minutes, its simulation and checks included; the part
is 64,527 triangles, and the grid in the tests above 246,036 nodes at 0.1 mm,
five drops each. How that divides between finding the lines and the rest was
not measured.

## Not yet

Fusion's order of the lines, which joins them through where they meet into
long passes, and of the passes beside them; its links, which lift where
camcraft's stay down through the roughing's leftovers; why its lines up a
corner stop a degree or two short of the bitangency angle, and why its line
round the dome's foot lies 0.006 mm into it; Up/Down Milling; Link from Inside
to Outside and its Inside/Outside Direction (camcraft cuts the passes beside
the lines from the outermost in); Limit Number of Stepovers off; Minimum
Fragment Length and Fragment Extension Distance; Fillets; Smoothing; the slope
range; Contact Point Boundary; Rest Machining; Fusion's straight plunges down
an upright corner at the cutting feed, which camcraft leaves to its linker.
Overthickness, one way and the other way, stock to leave and flat or bull nose
tools are offered but not compared with Fusion's programs.
