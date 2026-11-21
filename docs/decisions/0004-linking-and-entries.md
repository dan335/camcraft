# 0004 — Linking and entries

**Status:** decided, 2026-09-25 · **Milestone:** before the first cut

## Question

Every link resolved the same way: straight up at feed to a height over the
material plus the clearance offset (10 mm), across, and straight down at the
plunge feed all the way to the cut. Between two zigzag passes one stepover apart
too. Measured from the posted programs, at 5000 mm/min rapids:

| Program            | Total    | Cutting  | Plunging | Retracting |
|--------------------|----------|----------|----------|------------|
| router jig, finish | 81.5 min | 10.2 min | 57.1 min | 11.4 min   |
| lego brick, finish | 30.9 min |  1.6 min | 24.4 min |  4.9 min   |
| router jig, rough  | 402 min  | 210 min  | 158 min  | 32 min     |

And a program ended with the cutter on the part: the last cut, then `M5`.

What should a program do between cuts, and into a cut?

## Decision

The linker measures against the **material** as the operation leaves it rather
than against the model: the simulator's stock, cut as the linker goes, so every
decision sees what the passes before it removed. That is never lower than the
model, so it is as safe as measuring from the model, and a finish after
roughing does not rapid down into the rest material the roughing left, as one
measured from the model would.

- **Entry.** The tool rapids down to the **safe distance** (default 2 mm)
  above the highest material under the tool at the entry, then feeds the rest
  of the way at the plunge feed: rapid across at the retract height, rapid
  down to the safe distance, plunge feed down to the lead's start, the lead,
  then the cut.
- **Retract.** Feed up until the safe distance above the material under the tool,
  then rapid. Nothing rapids out of material.
- **Stay down.** A link no longer than the stay-down distance (default 50 mm
  for parallel) stays down, at the pass's own feed, as a transition, and
  zigzag passes are joined so: for parallel, the straight line between the
  pass ends lifted wherever the part rises above it (so it cannot gouge); for
  Z-level, a straight level move the part does not reach into. A transition that
  would descend steeper than the plunge ratio further than `max_cut_step`
  retracts instead, as a descent at cutting feed is exactly what the gate refuses.
- **Leads** for parallel: a vertical lead-in and lead-out, each 0.1 × the tool's
  diameter. The arc lies in the vertical plane of the pass, from straight down
  to the pass's own tangent at its start, slope included: for a pass starting at
  P in horizontal direction u, climbing at θ, radius R, the centre is
  P + R(−sin θ·u + cos θ·ẑ) and the arc sweeps 90° + θ. The lead-out mirrors
  it. They are
  drawn as short straight moves within the tolerance, and left out where they
  would reach the part, or where the material at the lead's start is above it
  (the lead would enter material); the tool then plunges straight to the pass.
- **Ramps** for Z-level: where the tool would plunge into material, it ramps down
  along the pass it is about to cut, at the ramp angle (default 2°), from the
  safe distance above the material; the pass's own path stays inside what may
  be cut at every height above its level, so the ramp cannot gouge. A pass too
  short to ramp along is plunged.
- **The end.** After the last pass: the lead-out, then up to the clearance height.
- **Stepover** defaults follow the tool when a tool is chosen for an operation:
  for parallel, the tool's diameter ÷ 4 for a flat mill, and for a ball or bull
  nose the cusp the tolerance allows, `2·√(r² − (r − tol)²) / √2`.

The settings are new fields of an operation's `linking`, written only when
changed, and need no newer file version: a build that reads a job without them
links the old way, which is slower and cuts the same.

What stays: the traverse height (the material under the traverse plus the
clearance offset), and the plunge feed's own default.

## Result

The same two jobs, the same checks passing, no gouges, no rapid contacts:

| Program            | Before   | After    |
|--------------------|----------|----------|
| router jig, rough  | 402 min  | 259 min  |
| router jig, finish | 81.5 min | 19.5 min |
| lego brick, rough  | 15.3 min | 5.4 min  |
| lego brick, finish | 30.9 min | 6.6 min  |

Measuring "material under the tool" as the simulator does (cell centres within
the radius) mattered: the traverse's padded disc also sees the material standing
beside the cutter, so a ring along a tall wall seemed to start in material as high
as the wall, and ramped from its top.

Not taken yet: smooth arc transitions between passes (a straight lifted line
stands in), helical ramps for Z-level and pockets (a 2D profile has them since
[decision 0006](0006-2d-linking-and-arcs.md)), and high feedrate mode.

## Amended 2026-11-21: across and down, for the finishes over the part too

Where a finish's next ring starts nearly under the last, down a wall steeper
than a plunge, the straight line over the part between them falls at the cutting feed further
than `max_cut_step`, so the link lifted to the clearance and plunged back onto
nearly the same spot. A finish over the part now takes the link a level
strategy takes to a ring's next depth: off the cut along its lead-out, across
at that height, and straight down at the plunge feed onto the next pass's
vertical lead, or the pass, as a lifted link would come down, wherever the
next pass starts lower, no further off than the stay-down distance, and the
way across is clear of the part, rubs no face avoided, keeps the rest of the
tool clear of the material and cuts no more than `max_cut_step` into it. A
finish that never stays down (a stay-down distance of 0) still lifts.

Two things that way assumed held only for clearing at a level with a flat
tool, and now hold for both:

- **What it cuts going across** was the material anywhere under the tool's
  flat disc. A ball across beside a steep wall has that wall under its disc
  and never reaches it with its rising side; it is now measured against the
  tool's own shape, which for a flat tool is the same test, figure for figure.
- **The rest of the tool** was never asked about going across, where every
  other link that stays down asks; now it is, so with the holder kept clear a
  link no longer crosses beside stock taller than the stickout.

Measured on `ops_3d`: Scallop with no groups lifts once where it lifted 12
times; with the dome avoided 5 times where 7; Spiral once where twice; Parallel
with the dome avoided 9 times where 11; Contour, with the dome avoided and its 6
mm ball, 13 where 15. Material removed is unchanged in each; what is fed moved
most in Pencil, whose links that fed up and went across at rapid now cross at
the feed. On a user's part Scallop lifts 26 times where it lifted 28, and 9
where 12 without its face avoided; the example rail jig's finish 77 where 113.
