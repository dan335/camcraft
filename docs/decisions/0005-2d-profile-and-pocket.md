# 0005 — 2D profile and pocket, after Fusion

**Status:** decided, 2026-09-26 · **Milestone:** before the first cut

## Question

The machine is a router that cuts up to a 5 × 12 ft sheet, most of it parts cut
out of sheet goods and pockets cut into them. Z-level roughing and parallel
finishing can clear a pocket, slowly and in 3D, but nothing can cut a part out.
Fusion does both with 2D Contour and 2D Pocket. What do we take from them, and
where does each decision live?

## What we take

**The boundary is the operation's own Geometry.** Decision 0002 put picks in
`Geometry`, not the strategy, so "a pocket will clear a picked floor, a trace
will follow picked edges". A 2D operation reads the same `boundary` and picks as
a shape to cut round or clear, not as a limit. With nothing chosen and nothing
picked, its boundary is the part's silhouette (`Geometry::boundary_2d`), so a new
profile goes round the part. `containment` is for a boundary that limits a 3D
strategy; a 2D one's tool goes its radius, plus the stock to leave sideways,
beside the boundary, on the side the strategy says. `margin`, the additional
offset, moves that further out, or in when negative, as if the boundary were
moved (added 2026-09-26, version 8). It moves the tool, not the boundary, so
floors are still found among the picks on the boundary.

**A profile's side is the strategy's.** Fusion flips a contour's side with an
arrow on each chain. We have one `side` for the operation: outside, inside or on.
Outside, a region's holes are cut from inside them, which is what cutting out a
part with holes means; a part with holes the other way is two operations.

**Floors come from the picks, area by area.** Fusion's bottom defaults to "the
selected contour". The boundary falls into areas (an outer ring and its holes),
and each is cut to the lowest picked point in it: picking two pocket floors at
different depths cuts each to its own. A bottom given overrides them all. With
nothing picked, the part's bottom.

**Levels are even.** Fusion's multiple depths space levels by the maximum
stepdown, optionally evenly. We always space them evenly, so no level is a
sliver and none is deeper than asked.

**The order is the strategy's.** Reordering by nearest neighbour would undo both
orders that matter:

- A profile cuts each ring all the way down before the next (Fusion's, with
  "order by depth" off), and never a ring before the rings inside it: a part's
  holes are cut while it is held, the outline that frees it last, and a part
  nested in another's hole before that hole.
- A pocket is cleared from its middle out, level by level, area by area. The tool
  enters where there is room, every ring after the first cuts one stepover of
  material on its outer side, and the wall is cut last and lightly. Where the tool
  on the innermost ring would leave some of the middle uncut, one more ring is put
  as near the middle as an offset reaches. That is judged by the real tool's
  radius, as are tabs: the stock to leave moves where the centre goes, not how
  wide the tool cuts (corrected 2026-09-26).

Each ring starts where it comes nearest the tool, so a pocket's rings link a
stepover apart and stay down.

**Climb, by the spindle.** With a clockwise spindle a climb cut keeps the material
it leaves on the tool's right. The rings are wound to that, reversed for
`conventional = true` or a tool whose spindle turns backwards.

**Linking is Z-level's.** Stay down up to 5 × the tool, level only, and ramp into
material along the ring about to be cut, wrapping round it as often as the ramp
needs. A ring too short to ramp along is plunged.

**Version 7.** A build that does not know the strategy would fail to parse it with
a message about an unknown variant. Version 7 makes it refuse by name.

## Rules kept

- **Paths from the model** (decision 0003): every ring is an offset of the boundary,
  which comes from the model's faces and edges, its shadow, or the stock's box.
  Nothing reads the stock grid.
- The part below the boundary is not consulted, as Fusion's 2D operations do not
  consult it. A part that reaches past its boundary lower down is cut into, and the
  verifier refuses the program.

## Not taken yet

- An onion skin, to hold parts cut free from a sheet. Tabs are taken
  (2026-09-26): a `Toolpath -> Toolpath` transform any operation can use, in
  `camcraft_core::tabs`, on the loops of its deepest cut.
- Finishing passes. A profile's horizontal lead-in and lead-out arcs are taken
  (2026-09-26): in the waste, checked against the areas, shrunk or moved to the
  middle of the longest side to fit.
- A start point chosen by the person.
- Chamfering with a V-bit beside an edge: a V-bit profiles only on the line.
- Rest machining: a pocket cuts its whole area at every level, air included.
