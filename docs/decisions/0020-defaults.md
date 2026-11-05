# 0020 — Defaults: Fusion's, as expressions where they follow the tool

**Status:** decided, 2026-11-05 · **Milestone:** 0.6

## Question

Each operation starts with values someone has to trust: a stepover, a
stepdown, feeds, heights, leads. Many were camcraft's own guesses, fixed
numbers that suit one tool and starve or overload the next. What should a new
operation start with?

## What was found

Fusion's defaults, from two sources that agree wherever both speak:

- **Autodesk's note of the defaults it changed in March 2022**, old and new
  expressions side by side ("Default Changes in Fusion 360's Manufacturing
  Workspace", [archived](https://web.archive.org/web/20250424091512/https://www.autodesk.com/support/technical/article/caas/sfdcarticles/sfdcarticles/Default-Changes-in-Fusion-360-s-Manufacturing-Workspace.html)).
- **Every parameter of a fresh operation of each strategy**, as a Fusion
  install's API gives them, in a metric document
  ([grace-xwerks/fusion_mcp_testing](https://github.com/grace-xwerks/fusion_mcp_testing/tree/master/docs/strategy_params)).
  It may carry that person's own "Make Default"s: one value looked so (3D
  adaptive's optimal load), and Dan's own Fusion settled it as
  `tool_diameter * 0.4`.
- Saved Fusion templates on GitHub only where several agree; Fusion's online
  help gives no defaults.

## What was decided

A new operation starts with Fusion's defaults. Where Fusion's follows the
tool, so does camcraft's, as the same expression, saved with the operation
(`StrategySpec::default_expressions`). A number typed over one, or a preset's,
takes its place. A job file still reads as it always did: what it leaves out
means what it meant, so a saved job does not change under it; only new
operations start differently.

| Setting | Strategies | Default | Was | Source |
| --- | --- | --- | --- | --- |
| Stepover | parallel, scallop, spiral, radial, morphed spiral | the tolerance's cusp with a round end, `tool_diameter / 4` with a square one | same | dump |
| Stepover | 3D pocket, 2D pocket | `tool_diameter * 0.6` | same | 2022 note |
| Optimal load | adaptive | `tool_diameter * 0.4` | same | Dan's Fusion |
| Stepover | flat | `tool_diameter * 0.3` | 0.6 × | dump |
| Stepover | face | `Math.max((tool_diameter - 2 * tool_cornerRadius) * 0.7; tool_diameter * 0.05)` | 0.95 × the flat | 2022 note |
| Pass extension | face | `tool_diameter / 2 + tolerance` | 0 | 2022 note |
| Stepdown | adaptive | `Math.min(tool_fluteLength * 0.75; tool_diameter * 2.5)` | same | 2022 note |
| Stepdown | 3D pocket, 2D pocket | `Math.min(tool_fluteLength * 0.75; tool_diameter * 0.1)` | adaptive's; 2 mm | 2022 note |
| Stepdown | profile | `tool_fluteLength * 0.75` | 2 mm | 2022 note |
| Stepdown | contour | 1 mm | same | dump |
| Minimum cutting radius | adaptive | `tool_diameter * 0.1` | none | dump |
| Stay-down clearance | adaptive | `Math.max(Math.min(tool_diameter * 0.1; 15mm); 2mm)` | 0 | dump |
| Slot width | adaptive | `tool_diameter * 1.25` | the tool's | dump |
| Lift height | adaptive | 0.2 mm | 0 | 2022 note |
| Plunge feed | all but drilling | `tool_feedCutting / 3` | 300 mm/min | dump (a new tool's) |
| Ramp feed | all that ramp | `tool_feedPlunge` | 60% of cutting | dump (a new tool's) |
| Clearance height | all | 15 mm above the stock: 10 above the retract | 10 above the stock | dump |
| Helix diameter | 2D pocket | `Math.max((tool_diameter - 2 * tool_cornerRadius) * 0.95; tool_diameter * 0.25)` | 0.9 × the tool | 2022 note |
| Maximum directional change | feed optimisation | 25° | 45° | dump |
| Reduced feed radius | feed optimisation | `tool_diameter * 0.05` | the tool's radius | dump |
| Reduced feed distance | feed optimisation | `tool_diameter * 0.2` | the tool's radius | dump |

The helix and feed optimisation's sizes are what the code works out where a
job leaves them out, so they change for saved jobs too: the 2D pocket's helix
is a new default, and feed optimisation arrived in 0.5.

## Left as they are

- **Contour's** 1 mm stepdown, **radial's** limits, **stock to leave** (half
  a millimetre on roughing, none on finishing), the **retract height** (5 mm
  above the stock), **leads** (a tenth of the tool, a quarter turn) and the
  **ramp angle** (2°): already Fusion's.
- **Tabs**: Fusion's templates disagree, and camcraft's (6 mm wide, 3 mm tall,
  every 150 mm) suit sheet goods on a router.
- **Fine stepdown**, Fusion's `Math.min(tool_diameter * 0.5; maximumStepdown * 0.1)`:
  off, as it is not known to be on in a fresh Fusion adaptive.
- **Safe distance and retraction**: Fusion's differ by strategy
  (`maximumStepdown + 1mm` for a pocket, `Math.min(1mm; tool_diameter * 0.2)`
  for a profile, full retraction for adaptive), and camcraft's links are not
  Fusion's, so 2 mm and minimum retraction stay until they are compared on a
  real job.
- **Spiral and morphed spiral** climb; Fusion's go both ways, which a single
  spiral has no meaning for in camcraft.
- **The cutting feed** a new operation gets with no preset, 1200 mm/min
  (Fusion's new tool's is 1000).
