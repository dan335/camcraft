# 0012 — Transitions and entries, from Fusion's own defaults

**Status:** decided, 2026-10-12 · **Milestone:** before the first cut

## Question

STATUS listed smooth transitions first: parallel's stay-down links were straight
lines lifted over the part where Fusion's are tangent arcs, and a pocket had no
helix. Contour had no ramp between levels, and a 2D pocket no finishing passes.
What exactly does Fusion do in each, and what are its defaults?

## Sources

- **Fusion's parameter tables,** strategy by strategy: each parameter's id, its
  default expression and its title, as Fusion's API `createInput` hands them
  back, dumped by a third-party script and published at
  `github.com/grace-xwerks/fusion_mcp_testing` (`docs/strategy_params`). It is
  Fusion's own output, hosted by someone else; it agrees with every default
  decision 0004 recorded from other sources. It covers the 3D strategies, not
  the 2D ones.
- **Fusion's help:** Transitions Between Cutting Passes, and the Parallel,
  Contour and 3D/2D Offset Roughing (formerly 3D Pocket Clearing and 2D Pocket)
  references; and the tooltips shipped with the installed Fusion (2705.1.25).
- **Autodesk staff on the forums,** and the program in Autodesk's post-editor
  repository, whose Face operation with transition type `curve` posts one `G2`
  half circle between zigzag passes.

The help pages' screenshots are not defaults: several show edited values.

## What Fusion does

- **Transition Type** (`transitionType`) is `'curve'`, *Smooth*, for parallel,
  scallop, spiral, radial and contour: "smooth tangential movements using true
  arcs where appropriate", and for contour "gradually ramping down along the
  surface. The ramp never exceeds the specified ramp angle". Flat's is
  *Straight line*. Staff: "the default behavior for the 3D strategies is to stay
  in contact with the part as much as possible when moving between cuts", and
  the only limit on a transition is not gouging.
- **Maximum Stay-Down Distance** is 50 mm for parallel and contour, 2 × the tool
  for scallop, flat and steep-and-shallow, and 5 × for spiral, radial, morphed
  spiral, 3D pocket and adaptive.
- **The 3D helix:** ramp type `'helix'`, ramp angle 2°, Helical Ramp Diameter
  max(0.95 × (D − 2 × corner radius), 0.25 × D), Minimum Ramp Diameter
  max(0.5 × (D − 2 × corner radius), 0.25 × D), maximum ramp stepdown the
  stepdown. With the helix type the pocket may also ramp along its rings (its
  `allowContourRamps`), never zig-zag nor plunge; an area where nothing fits is
  left, with a warning.
- **The 2D pocket's finishing passes** (`doFinishingPasses`) are off in a new
  operation; `numberOfFinishingStepovers`, `finishingStepover` (0.1 × D, the one
  default the API manual states), `finishFeedrate` for the last, `nullPass` to
  repeat it, and `leadsForAllFinishingPasses`, off: one lead onto the first
  pass, the rest joined by short stepovers. By default the passes are cut at
  each depth once it is roughed.

This corrects decision 0004 in two places: a helix with no room falls back to a
ring ramp, not to zig-zag; and Fusion now calls the pockets Offset Roughing.

## Decision

- **Defaults per strategy.** The stay-down distance is Fusion's for each
  strategy (`Uses::reach`). The helix is Fusion's 3D size for pocket clearing,
  contour and adaptive (`HelixSize::Flat`), made smaller where there is no room
  down to its minimum; the 2D strategies keep 0.9 × the tool, as no table says
  otherwise for them.
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
  does not fall steeply. Otherwise the lifted straight line, Fusion's *Straight
  line*. Fusion's exact geometry where the passes' heights or directions differ
  is not documented; a Fusion-posted parallel program would settle it.
- **Contour ramps down the wall** between levels, in the strategy, as the
  profile's spiral is: along the next level's ring, from the height the last
  ring ended at, at no more than the ramp angle, then once round that ring from
  where the ramp reaches it. A lower ring is clear of the part at every height
  above it, since the part's shadow only grows going down. It ramps where the
  next ring passes within half the tool of where the last ended, and only where
  the operation stays down at all.
- **Contour's entries stay a plunge onto its lead.** Fusion's contour defaults to
  a helix, but its leads come down beside the wall, mostly in air. The linker
  judges a pass to start in material by what stands under a flat disc the
  tool's full width, which sees the stock on the wall, so it would helix where
  Fusion drops in on its lead: on the lego, roughed then contoured, that took
  the contour from 8.6 minutes to 13.9.
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

- **Leaving an area no ramp fits,** as Fusion does. The linker cannot drop a
  strategy's passes, and a plunge is counted and shown.
- **Contour helixes into each level,** until the linker's test for material is
  the tool's own shape rather than a flat disc.

## Consequences

- A pocket's roughing rings still have no leads: what leads Fusion puts on them
  is not documented.
- Zigzag passes further apart than the tool is wide, or running other than
  opposite ways, are still joined by the lifted straight line.
- The time estimates count path length and feeds, not acceleration, so they
  do not show what a smooth turn saves on a real machine.
