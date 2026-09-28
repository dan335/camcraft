# 0006 — 2D linking and arcs, after Fusion's 2D Contour

**Status:** decided, 2026-09-27 · **Milestone:** before the first cut

## Question

Fusion's 2D Contour round the lego brick (`fixtures/nc/lego_2d_contour_01.nc`,
posted for a Masso) is 40 lines. camcraft's program for the same ring was 575,
and 1,600 at a stepdown. What does Fusion's program do that ours did not?

## What Fusion does

Read from that program, block by block:

- **Start.** `G28 G91 Z0` before the tool change; after it, across (`G0 X Y`) and
  then down (`Z`), never both at once.
- **Heights.** Across at the clearance height, down at rapid to the retract
  height, then at the plunge feed.
- **Entry.** Its ramp is off, so it plunges straight down in the waste, beside the
  ring, to a lead radius above the floor.
- **Lead-in.** A quarter turn down in the upright plane (`G19 G3`), a straight
  (`G1`), and a quarter turn level onto the ring (`G17 G3`), tangent to it: each
  a tenth of the tool's diameter.
- **The ring.** Four straights and four `G2` corners: arcs, not chords.
- **Lead-out.** The lead-in, mirrored, rising at the end.
- **End.** Straight up at rapid, `G28 Z`, `G28 X Y`, `M30`.

Ours had short straight moves for every arc and no vertical lead. It ramped round
the ring *backwards* (conventional, against a climb cut) and out through its own
lead-in, because a pass with leads does not close and was walked out and back. It
went up to the clearance height between every depth of the ring, as the next
ramp started 163 mm back along it. Its first move after `M6` went across and down
at once, it never went home, nothing limited how deep a ramp cut per lap, and an
operation-level `stepdown` the format had moved to `passes` was silently ignored.

## Decision

- **Unknown settings are refused** (`deny_unknown_fields` on every table), and one
  of an operation's sections written in the operation is named with its section.
- **The post starts and ends as Fusion's.** A profile gives how its machine goes
  straight up (`safe_retract`) and home across (`end_home`). The spindle stops and
  the tool goes up before a tool change, and a rapid from an unknown position goes
  across then down.
- **An arc is a move.** `MoveKind::Arc` holds an arc or a helix about any axis
  (`camcraft_geom::arc::Arc`). Every consumer follows it: the simulator (as chords
  a tenth of a cell from it), playback, tabs (cut at their edges, lifted as arcs),
  pass reversal, the app, and the post (`G2`/`G3` in `G17`, `G18` or `G19`, in
  pieces of at most half a turn). The engine makes an arc only about X, Y or Z,
  which a three-axis machine can cut; elsewhere it makes chords within the
  tolerance, so the program is always the path that was simulated.
- **Leads are Fusion's three pieces,** each a tenth of the tool: vertical arc,
  straight, level arc. The strategy makes the level arc and the straight; the
  linker makes the vertical arc, because only it knows how the tool arrives.
- **Entry is a choice for a profile:** a helix beside the lead (the default,
  since a router plunging an end mill 38 mm into birch is not what anyone wants),
  a plunge (Fusion's program), or a ramp round the ring in the direction it cuts,
  entered at its start without its lead-in.
- **Most per lap.** No helix, or ramp round a closed ring, descends more in one
  lap than the stepdown (Fusion's maximum ramp stepdown).
- **Down to the next depth without lifting.** Off the ring along the lead, across
  at that height to where the next depth's way in starts, and down it, when that
  is within the stay-down distance, clear of the part, and cuts little on the way.

## Result

The lego profile with `entry = "plunge"`, one depth and leads posts, block for
block, what Fusion's does: `G1` down, `G19 G3`, `G1`, `G17 G3` onto the ring. With
the default helix and a 6.35 mm stepdown it posts 7 depths, staying down between
all of them, with no gouges and no rapid contacts.
