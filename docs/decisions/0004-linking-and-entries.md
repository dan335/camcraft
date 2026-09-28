# 0004 — Linking and entries, after Fusion

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

Fusion's parallel finishing is the reference. What does it do between cuts, and
into a cut, and what of it do we lack?

## What Fusion does

Established from Fusion's parameter table (its API's `createInput` defaults, per
strategy), its help and tooltip pages (including those shipped in the install),
real Fusion-posted programs, Autodesk's post processor guide and stock posts, and
forum answers from Autodesk staff. HSMWorks and Inventor CAM, the same kernel,
filled in older wording. The full notes and citations were gathered on 2026-09-25;
what follows is what we rely on.

### Heights and the rapid-to-feed switch

3D Parallel has no feed height; that is a 2D parameter ("the height that the tool
rapids to before changing to the feed/plunge rate to enter the part"). In 3D the
**Safe Distance** (default 2 mm) plays its part: the tool rapids down until it is
that far clear of the part, then feeds the rest of the way at the **plunge feed**
(default cutting ÷ 3). Its tooltip: "Within this distance, the tool retracts with a
direct move, instead of a rapid move." Fusion measures it from the model; an
Autodesk answer notes the lead-in "needs to be longer than the remaining stock",
since Parallel "will always go to the surface regardless of what roughing has
taken place."

### Staying down

Parallel's **Maximum Stay-Down Distance** is 50 mm, and keep-tool-down is always
on for it: "If the distance between positions is greater than the Maximum Stay
Down value, the tool will retract between cuts", and "regardless of the stay-down
distance, the tool retracts between passes if the rapid motion violates the
workpiece." A stay-down link is a feed move (G1), at the **transition feed**
(default the cutting feed). Zigzag ("both ways", the default direction) joins pass
ends this way. Pocket clearing's stay-down distance is 5 × the tool's diameter.

Otherwise the **Retraction Policy** applies: *full* goes up to the retract height
(stock top + 5 mm), *minimum* "straight up to the lowest height where the tool
clears the workpiece, plus any specified safe distance". Retracts are rapid by
default (Allow Rapid Retract), after the lead-out has lifted the tool off.

### Leads

Parallel has a **vertical lead-in and lead-out**, each 0.1 × the tool's diameter,
and nothing else (no horizontal lead, no ramp). The arc lies in the vertical plane
of the pass, from straight down to the pass's own tangent at its start, slope
included (measured from posted programs): for a pass starting at P in horizontal
direction u, climbing at θ, radius R, the centre is P + R(−sin θ·u + cos θ·ẑ) and
the arc sweeps 90° + θ. The lead-out mirrors it. A lead that does not fit is left
out, and the tool plunges straight to the pass. Lead feeds default to the cutting
feed.

The entry, in order: rapid across at the retract height, rapid down to the safe
distance, plunge feed down to the lead's start, the lead at lead-in feed, cut.

### Ramps

Only roughing ramps. Fusion's 3D pocket clearing defaults: ramp type helix (profile
and zig-zag where a helix does not fit), **ramp angle 2°**, ramp clearance height
1.05 × the stepdown above the previous level, maximum ramp stepdown the stepdown,
**ramp feed = plunge feed**, and no lead before a ramp. Its stepover is at most
0.6 × the tool's diameter, and its safe distance the stepdown + 1 mm.

### Stepover

Parallel's default stepover is the tool's diameter ÷ 4 for a flat mill, and for a
ball or bull nose the cusp the tolerance allows:
`2·√(r² − (r − tol)²) / √2`.

## Decision

The linker does what Fusion does, measured against the **material** as the
operation leaves it rather than against the model: the simulator's stock, cut as
the linker goes, so every decision sees what the passes before it removed. That is
never lower than the model, so it is as safe as Fusion's rule, and it also clears
the rest material Fusion's answer warns about.

- **Entry.** Rapid down to the safe distance above the highest material under the
  tool at the entry, then the plunge feed.
- **Retract.** Feed up until the safe distance above the material under the tool,
  then rapid. Nothing rapids out of material.
- **Stay down.** A link no longer than the stay-down distance stays down, at the
  pass's own feed, as a transition: for parallel, the straight line between the
  pass ends lifted wherever the part rises above it (so it cannot gouge); for
  Z-level, a straight level move the part does not reach into. A transition that
  would descend steeper than the plunge ratio further than `max_cut_step`
  retracts instead, as a descent at cutting feed is exactly what the gate refuses.
- **Leads** for parallel, as Fusion's, drawn as short straight moves within the
  tolerance; left out where they would reach the part, or where the material at
  the lead's start is above it (the lead would enter material).
- **Ramps** for Z-level: where the tool would plunge into material, it ramps down
  along the pass it is about to cut, at the ramp angle (2°), from the safe
  distance above the material; the pass's own path stays inside what may be cut
  at every height above its level, so the ramp cannot gouge. A pass too short to
  ramp along is plunged.
- **The end.** After the last pass: the lead-out, then up to the clearance height.
- **Stepover** defaults follow the tool, as Fusion's do, when a tool is chosen
  for an operation.

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

Not taken yet: Fusion's smooth arc transitions (a straight lifted line stands in),
helical ramps for Z-level and pockets (a 2D profile has them since
[decision 0006](0006-2d-linking-and-arcs.md)), and high feedrate mode.
