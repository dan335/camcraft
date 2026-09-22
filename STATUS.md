# Where this is

Last updated 2026-09-22. Read this before picking the work back up.

**0.1-a, 0.1-b and 0.1-c are complete, plus the `job.toml` contract.** 197 tests,
all gates green. The engine takes an STL and a job file and writes G-code, and
refuses to write a program the simulator has not verified against material.

**Nothing has been cut yet.** See *Before the first cut* below — there are things
only the machine can tell us.

## What works

```powershell
cd D:\rust\camcraft

# Two operations, one block of material, one program per operation
cargo run --release -p camcraft-cli -- job `
  "D:\temp\coping_sled_rail_01 v12.stl" examples\rail-jig.toml `
  --units mm --profile masso --out out

# Or look at a single finishing pass before trusting any of it
cargo run --release -p camcraft-cli -- inspect `
  "D:\temp\coping_sled_rail_01 v12.stl" `
  --units mm --tool ball --diameter 6 --stepover 2 --out path.json
bash scripts/view.sh     # then drop path.json on the page
```

Exit codes: `0` clean, `2` ran with warnings worth reading, `1` refused because the
simulator objected. `--force` overrides and says loudly that it did.

Last full run on the 237 × 227 × 76 mm router jig:

```
rough:  1048 passes, removed 3373651 mm3, travel saved 77%, rapid contacts 0
finish:  556 passes, removed   98421 mm3, travel saved  0%, rapid contacts 0
verified: 155330 columns compared, gouges 0, deepest 0.000 mm
```

Finishing saving 0% travel is correct, not a failure: a parallel zigzag already ends
each pass next to where the following one starts.

## What is next, in order

1. **Wire `--inspect` into the `job` subcommand.** Today the viewer shows a
   single-operation approximation while `job` produces the real multi-operation
   program. Two code paths where there should be one, and a defect could live in only
   the one nobody looks at. This is the most valuable remaining item precisely because
   it is about being able to *see* what will run.
2. **0.1-d, STEP import.** The backend decision is made and the spike is done
   (`docs/decisions/0001-step-backend.md`), so this is wiring rather than research.
   OpenCascade, with cmake and a C++ toolchain as documented prerequisites.
3. Everything else is post-0.1: waterline, constant scallop, pencil, adaptive
   clearing, 2D operations, V-carve, tabs, arc fitting, feeds and speeds.

## Before the first cut

The simulator verifies against a *simulated* part. It cannot check the things that
actually break cutters on the day:

- **The Masso profile has never run on the machine.** It was built from the
  controller's documented G-code reference. Dry-run it well above the work first.
- Nothing knows whether the WCS is where the job assumes, whether the tool in the
  spindle matches `diameter`, or whether the stock is the size the job was told.
- Roughing is conservative contour-parallel clearing. It has never cut anything.

## Things that were wrong and are now right

Worth knowing, because each was found by looking rather than by a test passing:

- **Cut moves that were really plunges.** On the router jig, parallel finishing
  emitted moves descending 48 mm in 0.01 mm of travel — a full-depth plunge at cutting
  feed. Every point was exactly on the CL surface and every existing check passed; only
  the *connection between* correct points was unmachinable. Found by reading real
  output in the inspector. Passes now break at walls and at gaps in coverage.
- **Marching squares emitted nothing for a uniformly high field**, because such a
  field has no boundary inside the grid. Roughing read that as "nothing to clear" —
  the difference between clearing the whole block and clearing none of it.
- **The simulator's stamp loop missed the cell at the cutter's rim.** Under-removal,
  which is the direction that hides a gouge.
- **The determinism gate could not detect non-determinism.** It ran the suite twice at
  different thread counts; a deterministic test run twice simply passes twice, and
  nothing ever compared the runs. Replaced with tests that diff output against itself.

## Standing cautions

- **`fixtures/step/` holds real shop jigs.** They are not customer work, but they are
  not public either. Decide before making the repo public — deleting them later does
  not remove them from history. One commercial part was already removed and scrubbed;
  parts that cannot be published belong in `CAMCRAFT_FIXTURES`.
- **The public fixture set cannot reproduce the bug that disqualified `truck-stepio`.**
  The part that exposed it is private. A synthetic sculpted-surface fixture should
  replace it.
- **Run `bash scripts/check.sh` unpiped.** Piping through `tail` buffers the whole run
  and hides progress.
- There is no CI while the repo is private, so that script is the only thing between a
  mistake and `main`.

## Map of the code

```
camcraft-geom     BVH, height maps, contours, synthetic fixtures     (leaf)
camcraft-import   STL, units required rather than assumed            -> geom
camcraft-sim      material removal: stock updater + gouge oracle     -> geom
camcraft-core     tools, CL maps, strategies, toolpaths, job, run    -> geom, sim
camcraft-post     Op IR, TOML dialect renderers                      -> geom, core
camcraft-cli      inspect / post / job                               -> all
viewer/           local inspector; part geometry never leaves the machine
```

`core` cannot see `post`, so it cannot name a G-code concept. That is what keeps
strategies emitting toolpaths rather than instructions.

The full design and its reasoning live in the approved plan at
`C:\Users\danph\.claude\plans\i-use-autodesk-fusion-wobbly-wave.md`. Read it before
changing anything structural; it records eight decisions that would force a rewrite.
