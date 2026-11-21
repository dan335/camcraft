# 0020 — Defaults, as expressions where they follow the tool

**Status:** decided, 2026-11-05 · **Milestone:** 0.6

## Question

Each operation starts with values someone has to trust: a stepover, a
stepdown, feeds, heights, leads. Many were camcraft's own guesses, fixed
numbers that suit one tool and starve or overload the next. What should a new
operation start with?

## What was decided

A new operation starts with the defaults below. Where a default follows the
tool, it is an expression of the tool's sizes and feeds, saved with the
operation (`StrategySpec::default_expressions`), so it suits whichever tool
the operation has. A number typed over one, or a preset's, takes its place.
A job file still reads as it always did: what it leaves out
means what it meant, so a saved job does not change under it; only new
operations start differently.

| Setting | Strategies | Default | Was |
| --- | --- | --- | --- |
| Stepover | parallel, scallop, spiral, radial, morphed spiral | the tolerance's cusp with a round end, `tool_diameter / 4` with a square one | same |
| Stepover | 3D pocket, 2D pocket | `tool_diameter * 0.6` | same |
| Optimal load | adaptive | `tool_diameter * 0.4` | same |
| Stepover | flat | `tool_diameter * 0.3` | 0.6 × |
| Stepover | face | `Math.max((tool_diameter - 2 * tool_cornerRadius) * 0.7; tool_diameter * 0.05)` | 0.95 × the flat |
| Pass extension | face | `tool_diameter / 2 + tolerance`, to the tool's centre (amended below) | 0 |
| Stepdown | adaptive | `Math.min(tool_fluteLength * 0.75; tool_diameter * 2.5)` | same |
| Stepdown | 3D pocket | `Math.min(tool_fluteLength * 0.75; tool_diameter * 0.1)` | adaptive's; 2 mm |
| Stepdown | 2D pocket | none: Multiple Depths off; ticked, `Math.min(tool_fluteLength * 0.75; tool_diameter * 0.1)` (amended below) | 2 mm |
| Stepdown | profile | none: Multiple Depths off; ticked, `tool_fluteLength * 0.75` (amended below) | 2 mm |
| Stock to leave | 2D pocket | 0.5 mm each way (amended below) | none |
| Entry | profile | plunge: Ramp off (amended below) | helix |
| Stepdown | contour | 1 mm | same |
| Minimum cutting radius | adaptive | `tool_diameter * 0.1` | none |
| Stay-down clearance | adaptive | `Math.max(Math.min(tool_diameter * 0.1; 15mm); 2mm)` | 0 |
| Slot width | adaptive | `tool_diameter * 1.25` | the tool's |
| Lift height | adaptive | 0.2 mm | 0 |
| Plunge feed | all but drilling | `tool_feedCutting / 3` | 300 mm/min |
| Ramp feed | all that ramp | `tool_feedPlunge` | 60% of cutting |
| Clearance height | all | 15 mm above the stock: 10 above the retract | 10 above the stock |
| Helix diameter | 2D pocket | `Math.max((tool_diameter - 2 * tool_cornerRadius) * 0.95; tool_diameter * 0.25)` | 0.9 × the tool |
| Maximum directional change | feed optimisation | 25° | 45° |
| Reduced feed radius | feed optimisation | `tool_diameter * 0.05` | the tool's radius |
| Reduced feed distance | feed optimisation | `tool_diameter * 0.2` | the tool's radius |

The helix and feed optimisation's sizes are what the code works out where a
job leaves them out, so they change for saved jobs too: the 2D pocket's helix
is a new default, and feed optimisation arrived in 0.5.

## Left as they are

- **Contour's** 1 mm stepdown, **radial's** limits, **stock to leave** (half
  a millimetre on roughing, none on finishing), the **retract height** (5 mm
  above the stock), **leads** (a tenth of the tool, a quarter turn) and the
  **ramp angle** (2°).
- **Tabs**: camcraft's (6 mm wide, 3 mm tall, every 150 mm) suit sheet goods
  on a router.
- **Fine stepdown**: off.
- **Safe distance and retraction**: 2 mm and minimum retraction.
- **Spiral and morphed spiral** climb (amended below).
- **The cutting feed** a new operation gets with no preset, 1200 mm/min.

## Amended 2026-11-11: four defaults set again

Four defaults above were set again, each for a new operation only:

- **2D Pocket and 2D Contour start with Multiple Depths off**: one pass to
  the bottom, the pocket's helix straight down to its floor. Their stepdown
  expressions apply only once the box is ticked; camcraft's had them on, the
  pocket at 0.6 mm a level (25 helixes on the test part, 6.6 minutes
  feeding). A new one now starts with none, and ticking it brings the
  expression.
- **2D Pocket starts with Stock to Leave ticked, 0.5 mm each way**, as a
  roughing pass leaves for a finish; camcraft's started unticked.
- **2D Contour starts with Ramp off**: straight down onto the lead-in at the
  plunge feed. camcraft's helixed down at 2°, about 630 mm of helix on a
  22 mm cut. A new profile starts with `entry = "plunge"`; a job file that
  leaves the entry out still reads as a helix.
- **Face's pass extension is to the tool's centre**: half the tool and the
  tolerance ends each pass with the tool just clear. camcraft added it after
  the tool was clear, so each pass ran a radius further than it needed at
  each end. Passes now cross the faced area grown by the extension, which
  keeps an extension of at least the radius clear at any angle; a job file
  that leaves it out runs the tool's radius, just clear, as it did.

## Amended 2026-11-19: spirals both ways, flat straight down

A new Spiral and a new Morphed Spiral start both ways: the spiral turns to
climb and each piece the part cuts it into is taken from its nearer end,
which is what both ways means for one. A new Flat comes straight down onto
each ring, its ramp along the ring a setting. A job file that leaves either
out reads as before: climbing, and ramping
([decision 0034](0034-entries-and-order-of-the-finishes-over-the-part.md)).
