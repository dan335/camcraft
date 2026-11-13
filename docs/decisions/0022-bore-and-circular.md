# 0022 — Bore and circular: round walls picked as faces

**Status:** decided, 2026-11-13 · **Milestone:** 0.8

## Question

Fusion's Bore and Circular mill round holes and bosses. They are the first of
Fusion's 2D operations still to come that pick holes rather than an area, and
Thread will pick the same way. How are the holes picked, how is a pass round
one laid out, and how is it linked?

## What was found

Autodesk's references for both ("Bore reference", "Circular reference" in the
Fusion CAM help) list the settings tab by tab and give few defaults. Every
parameter of a fresh operation of each, as a Fusion install's API gives them
([grace-xwerks/fusion_mcp_testing](https://github.com/grace-xwerks/fusion_mcp_testing/tree/master/docs/strategy_params),
`bore.txt` and `circular.txt`), gives the rest, as it did for
[decision 0020](0020-defaults.md):

- Both select circular faces (`holeMode = 'selection-faces'`), with their top
  and bottom from the hole's (`from hole top`, `from hole bottom`).
- A bore's helix is set by its angle (`useAngle = true`, 2°) unless its pitch
  is asked for (`threadPitch = 1`). It has no multiple depths.
- Both have Multiple Passes off, two stepovers half the tool apart when on,
  Repeat Passes off and climb. A bore has one finishing pass a tenth of the
  tool from the wall, off. A circular has Multiple Depths on, a millimetre
  apart, not even.
- Both lead with a horizontal arc a tenth of the tool, a straight a twentieth
  of it, and a vertical arc the size of the horizontal one; Lead To Center off.

The Circular reference says its stepover is "95% of the cutter diameter less the
tool corner radius"; the API's is half the tool. The API's is taken, as it is
what a new operation starts with.

## What was decided

**Picked as faces.** A bore's or a circular's picks are faces, as Fusion's
are, read by `strategy::round::rounds`: a face whose edges are circles about
one upright axis, all one size, is a straight round wall; which way its
triangles face says whether it is a hole or a boss. The faces of one wall are
taken together. Drilling keeps its rims: a rim says nothing of which side the
material is on, and a wall says nothing a drill needs that a rim does not.
`Uses::holes` says which a strategy picks (`Holes::Rims` or `Holes::Walls`),
so the panel offers only faces for these, and the run finds them once, before
the strategy runs (`round_walls` in `run::operation::round`).

**Passes laid out by the strategy, linked by the linker.** Each pass is the
strategy's own: a lead onto its circle, the helix or the circle, a lead off,
all level or about Z, so every move is an arc posted as one. A pass that would
be past a hole's middle is put half the tool's radius from it, once: Fusion's
behaviour there is not documented, and this cuts the middle rather than
leaving a pin. A hole the passes do not reach the middle of is left with its
core, as Fusion's is, and said (CC218). The linker brings the tool onto each
pass: a bore's starts at the top, out of the material, so it comes down the
vertical arc onto it and lifts out after; a circular's first pass at a level
ramps down along its circle, as Fusion's Circular ramps on, and the passes after
it stay down.

**No move a whole turn.** Every circle and helix is cut in arcs of at most
half a turn. A move that ends where it starts, seen from above, reads to the
linker's ramp as straight down: a closed pass made of one full-circle arc is
ramped into by plunging.

**The job file mirrors Fusion's parameters**: `use_ramp_angle`, `ramp_angle`,
`pitch`, `multiple_passes`, `stepovers`, `repeat`, `even_stepdowns`,
`lead_to_center`, so a setting confirmed against Fusion later moves no key.
A bore's finishing pass is a pocket's `passes.finishing`, of which it reads
only the stepover.

## What it leaves open

Neither is yet held to a Fusion program for the same part (`camcraft
compare`). Whether Fusion counts its stepovers with the wall's pass or before
it, where its passes past a hole's middle go, where on the circle a pass
starts, whether its lead-in arc turns a quarter turn, and how its Circular goes
down to a level, are not in its references; PROGRESS.md and the report that
came with this list them to check.

## What Fusion's programs showed, 2026-11-13

Fusion's own programs for both on a part made for them (`fixtures/step/ops_2d.step`,
`fixtures/nc/ops_2d_{bore,bore_passes,circular,circular_passes}.nc`, made by
`scripts/fusion/ops_2d/`) settled most of what was open, and changed three
things; `crates/camcraft-session/src/compare/tests/ops_2d.rs` holds camcraft's
to them.

- **A bore's helix by its angle** falls, a turn, the angle's tangent times the
  diameter the tool's outer edge sweeps: the pass's circle widened by the tool,
  not the tool's centre's circle. 0.5587 mm a turn in a hole 16 across with a
  6 mm tool at 2°.
- **A pass past a hole's middle is left out**, as Fusion discards it ("Some
  passes were discarded"), not moved to half the tool's radius: three
  stepovers of 3 mm leave a hole 16 across passes at 2 and 5. The run says so
  (CC222). Where Fusion's cut-off lies between nothing and a millimetre is not
  known; camcraft's is a tenth of a millimetre.
- **A circular neither ramps nor stays down.** Every pass at every level comes
  down its vertical lead, into the material where the level is still there,
  and lifts out after, even to a circle 3 mm away at the same level. Its ramp
  along the circle and its stay-down are gone, so a saved circular's
  `linking.ramp_angle`, `ramp_feed`, `ramp_stepdown`, `stay_down` and
  `retraction` are no longer read.
- **The lead's level arc** is made no wider than half its circle where the
  circle is small, keeping its straight, as Fusion's is (0.5 on a circle of 1),
  before it is halved.

Confirmed as they were: N stepovers are N passes, the wall's among them, from
the middle out; every pass starts on +X and climbs anticlockwise in a hole; a
bore lifts out between passes and starts each at the top, and goes once round
level at the bottom; the leads are a vertical arc a tenth of the tool, a
straight a twentieth and a quarter-turn level arc a tenth, off the mirror; a
circular's levels are a millimetre apart, each level's passes from the middle
out before the next level.

Still different: Fusion feeds from the safe distance down to the vertical
lead's top at its lead-in feed, where camcraft's linker plunges at the plunge
feed; it rises from the safe distance above the level, where camcraft feeds
from the safe distance above the material. Neither changes what is cut. A
single bore's lead off the bottom bites the core it leaves, and camcraft's
vertical arc up is five straight moves within the tolerance where Fusion's is
ten, so the loose core differs by up to 0.06 mm.
