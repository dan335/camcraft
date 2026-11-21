# 0028 — Pencil

**Status:** decided, 2026-11-14 · **Milestone:** 0.9

## Question

Pencil runs the tool along the inside corners and fillets a larger tool before
it could not reach into: wherever the tool touches the part in two places.
Which places, as its bitangency angle decides; where does a line start and
stop; what do its number of stepovers, overthickness and direction do; and in
what order are the lines cut?

## What a new one starts with

The tool's centre kept to the silhouette, its additional offset the stock to
leave; a bitangency angle of 20°; no overthickness; no stepovers, half the
tool apart when asked for; both ways; tolerance 0.01 mm. Linking: a full
retraction, a safe distance of 2 mm, staying down within five tools, vertical
leads a tenth of the tool, and no ramp.

## Where the lines run on `ops_3d`

Ramp's part ([decision 0027](0027-ramp.md)) gives a Pencil a good deal to do
with a 12 mm ball, larger than the pocket's R4 floor fillets and its R5
corner, and enough with a 6 mm ball, which fits them.

- **With the 12 mm ball** a line runs all round the pocket's floor at Z10, a
  radius from the walls, the ball's end on the floor and its side against the
  wall above the fillet; round the dome's foot, where the ball rests on the
  floor and the dome, cut off where it meets that ring; up each sharp corner,
  the ball on both rim edges; down the drafted wall, the ball against the +Y
  wall and on the drafted one; and a short one up the R5 corner to where its
  rim's arc takes over.
- **With the 6 mm ball**: round the dome's foot at Z10; down each sharp
  corner's mitre, where the two fillets meet, and up the corner to the rim;
  and nothing along the walls or fillets, which the ball fits.
- **Where the floor's ring meets a corner between two upright walls**, the
  ball touches both walls from Z10 to Z14 at one place seen from above.

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
the fillets' tops, where the ball touches in one place; a wall's foot is a
line only
where the lower sheet faces the wall's face at the angle or more, judged
where they meet.

**Down an upright corner, the linker.** camcraft refuses a cut that falls as
steeply as a plunge further than the deepest step cut (CC105); a stretch of a
line that does is left to the linker, which comes down it at the plunge feed,
as a parallel pass's is.

**Settings.** `direction` (both ways, which cuts each line whichever way is
nearer; one way as `climb`, every line with the higher side, wall or dome, on
the tool's right, climbing with a clockwise spindle; the other way as
`conventional`), `bitangency_angle`, `overthickness` (the lines found
for the tool grown all round by it, on top of the stock to leave, and cut by
the tool itself) and `stepovers`, `passes.stepover` apart. The passes beside the lines are lines of equal
distance from them along the tool's height over the part, worked out on the
grid as scallop's are ([decision 0009](0009-scallop-distance-field.md)), and
cut from the outermost in before the lines. It links over the part, staying
down within five tools, and comes straight down onto each line, with no
ramp.

## Measured

On `ops_3d` roughed with a new adaptive clearing and a 6 mm flat, leaving half
a millimetre, a new Pencil with either ball does not cut the part, and its
rapids meet nothing. With the 12 mm ball the line round the dome's foot lies
10.440 mm from the dome's middle, where the sphere puts the ball on floor and
dome at 10.4403; the lines up the corners run on to the angle asked. With the
6 mm ball the links stay down over the floor through what the roughing left,
from the dome's ring to the next corner's mitre, past the drafted wall, where
the roughing's 6 mm flat left material standing between the wall and the
dome.

A new Pencil of `ops_3d` with the 12 mm ball, posted by `camcraft post` from a
test build, took four minutes, its simulation and checks included; the part
is 64,527 triangles, and the grid in the tests above 246,036 nodes at 0.1 mm,
five drops each. How that divides between finding the lines and the rest was
not measured.

## Not yet

Joining the lines through where they meet into long passes, and an order for
the passes beside them; links that stay down through the roughing's
leftovers; Up/Down Milling; Link from Inside to Outside and its
Inside/Outside Direction (camcraft cuts the passes beside the lines from the
outermost in); Limit Number of Stepovers off; Minimum Fragment Length and
Fragment Extension Distance; Fillets; Smoothing; the slope range; Contact
Point Boundary; Rest Machining. Overthickness, one way and the other way,
stock to leave and flat or bull nose tools are offered but not yet measured on
a job.
