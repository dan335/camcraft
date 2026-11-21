# 0012 — Transitions and entries, and their defaults

**Status:** decided, 2026-10-12 · **Milestone:** before the first cut

## Question

STATUS listed smooth transitions first: parallel's stay-down links were straight
lines lifted over the part, which the machine slows into and out of, and a
pocket had no helix. Contour had no ramp between levels, and a 2D pocket no
finishing passes. What should each do, and what should each start with?

## Decision

- **Defaults per strategy.** The stay-down distance is the strategy's own
  (`Uses::reach`): 50 mm for parallel and contour, 2 × the tool for scallop and
  flat, and 5 × for spiral, radial, morphed spiral, pocket clearing and
  adaptive. The helix for pocket clearing, contour and adaptive
  (`HelixSize::Flat`) is max(0.95 × (D − 2 × corner radius), 0.25 × D)
  across, made smaller where there is no room, down to its minimum,
  max(0.5 × (D − 2 × corner radius), 0.25 × D). The 2D strategies keep 0.9 ×
  the tool. (Adaptive
  made its own helixes a half and a quarter of their size regardless, below
  the minimum, until
  [decision 0036](0036-adaptive-levels-order-and-entries.md): it now stops at
  the minimum, and ramps along the area's middle where that does not fit.)
- **A pocket's entry,** 3D or 2D, is a choice as a profile's is: a helix by
  default, beside where a level's first ring starts; a ramp along the ring; or a
  plunge. A helix with no room ramps along the ring instead. Where neither fits
  the tool still plunges, as before, rather than leave the area: leaving
  material behind silently is worse than a plunge the checks count.
- **The smooth transition** is a half turn between passes side by side that run
  opposite ways: as wide as they are apart, out past their ends, tangent to
  both, from one pass's height to the other's as it turns; with a straight on
  along the shorter pass where their ends are staggered, so the turn is at the
  further end. It is made only where the passes are no further apart than the
  tool is wide, the part does not rise into it, it cuts no more than a cut step
  into what stands within half the tool's radius, as a level link may, and it
  does not fall steeply. Otherwise the lifted straight line.
- **Contour ramps down the wall** between levels, in the strategy, as the
  profile's spiral is: along the next level's ring, from the height the last
  ring ended at, at no more than the ramp angle, then once round that ring from
  where the ramp reaches it. A lower ring is clear of the part at every height
  above it, since the part's shadow only grows going down. It ramps where the
  next ring passes within half the tool of where the last ended, and only where
  the operation stays down at all.
- **Contour's entries stay a plunge onto its lead.** Its leads come down beside
  the wall, mostly in air. The linker judges a pass to start in material by
  what stands under a flat disc the tool's full width, which sees the stock on
  the wall, so a helix by default would helix into every level where a plunge
  onto the lead drops in beside it: on the lego, roughed then contoured, that
  took the contour from 8.6 minutes to 13.9.
- **A 2D pocket's finishing passes,** off unless asked for: the clearing stops
  their stepovers short of the walls, and after each level they go round, the
  innermost first, the last at the finishing feed and, if asked, twice. The
  pocket's leads take the tool onto the first and off the last, level, in what
  is cleared.

## Measured

On the example job (the hinge fixture): pocket clearing stays down for 55 links
where it did 43 and rapids 176 mm where it rapided 901, 3.6 minutes from 4.1;
every one of parallel finishing's 24 links between passes turns, its path 5%
longer, every check passing with no gouges. On the lego, roughed then
contoured, the contour comes to 8.45 minutes from 8.57, plunging 0.58 minutes
where it plunged 0.97.

## Rejected

- **Leaving an area no ramp fits.** The linker cannot drop a
  strategy's passes, and a plunge is counted and shown.
- **Contour helixes into each level,** until the linker's test for material is
  the tool's own shape rather than a flat disc.

## Consequences

- A pocket's roughing rings still have no leads.
- Zigzag passes further apart than the tool is wide, or running other than
  opposite ways, are still joined by the lifted straight line.
- The time estimates count path length and feeds, not acceleration, so they
  do not show what a smooth turn saves on a real machine.
