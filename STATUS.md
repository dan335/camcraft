# Where this is

Last updated 2026-09-22. Read this before picking the work back up.

**0.1-a, 0.1-b and 0.1-c are complete, plus the `job.toml` contract, and the
inspector now shows the real multi-operation job.** All gates green. The engine takes
an STL and a job file and writes G-code, and refuses to write a program the simulator
has not verified against material.

**Nothing has been cut yet.** See *Before the first cut* below — there are things
only the machine can tell us.

## What works

```powershell
cd D:\rust\camcraft

# Look first: every operation's path as JSON, no G-code written
cargo run --release -p camcraft-cli -- job `
  "D:\temp\coping_sled_rail_01 v12.stl" examples\rail-jig.toml `
  --units mm --inspect job.json
bash scripts/view.sh     # then drop job.json on the page

# Then write one program per operation (--inspect and --out can go together)
cargo run --release -p camcraft-cli -- job `
  "D:\temp\coping_sled_rail_01 v12.stl" examples\rail-jig.toml `
  --units mm --profile masso --out out
```

`inspect` and `post` still exist for a single finishing pass described with flags.
They build a one-operation job and go through the same `run_job` as `job`, so there
is exactly one engine path. The inspection JSON is written *before* the verification
gate, so a job that is refused can still be looked at.
`crates/camcraft-cli/tests/one_path.rs` parses every written program back into
positions and compares them with the inspection JSON, move for move.

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

1. **0.1-d, STEP import.** The backend decision is made and the spike is done
   (`docs/decisions/0001-step-backend.md`), so this is wiring rather than research.
   OpenCascade, with cmake and a C++ toolchain as documented prerequisites.
2. **Nameless ASCII STL fails to load.** A file beginning `solid` followed by a
   newline (no name) is read as binary and fails with "failed to fill whole buffer".
   Most opencamlib sample files are like this. `stl_io` appears to detect ASCII by
   `solid ` with a trailing space; detect it ourselves before handing over.
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

## Open questions

Deferred on purpose: these are hard to judge until the GUI can show what is going
on. Come back to them once it can.

- **The viewer and the gate disagree about steep descents.** A cutting move that
  drops further than `max_cut_step` while barely advancing is a plunge at cutting
  feed. The viewer calls it a blocking failure (CC105, HALT). The engine only warns:
  it still writes the program and exits 2. Seen on real output: `mount_rush.stl` from
  the opencamlib samples gets a 0.76 mm descent from a single finishing pass, and a
  0.93 mm one under `rail-jig.toml`. The same numbers came out before the
  one-engine-path change, so this is old behaviour. Either the gate should refuse it
  (as the viewer implies), or the viewer should call it a warning.

  *Why they get through at all* (found, not yet fixed). `refine` in
  `crates/camcraft-core/src/parallel.rs` accepts a span as soon as its midpoint lies
  within tolerance of the chord. It only reaches the wall-break check when that test
  fails and the span has shrunk to the geometric limit. A steep but *straight* face
  passes the chord test, so its depth is never compared with `max_cut_step`. In the
  `rail-jig.toml` finishing pass on `mount_rush.stl`, moves 444 and 445 drop 0.93 mm
  and 0.51 mm over 0.047 mm and 0.023 mm of travel, 1.44 mm almost straight down at
  cutting feed. Only `steep_cut_descents`, run afterwards, notices, and it only
  warns. Likely fix: check depth before accepting a span, so the pass breaks there
  like at any other wall. Then decide what the gate does with whatever is left.

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
- **The inspector showed a different path from the one that ran.** `inspect` had its
  own single-operation engine path, with a flat clearance plane, no pass reordering
  and a differently sized block, while `job` ran `run_job`. Now there is one path.
  The viewer's rapid check (CC102) used to compare rapids against that plane. It now
  rasterises the part triangles, grows the result by the cutter radius, and checks
  that every rapid clears it, which stays true when clearance follows the material.
- **The post golden files were never committed.** `*.nc` in `.gitignore` caught
  `tests/golden/*.nc`, so on a fresh clone every golden test failed.
- **The viewer drew off-screen on a 2x display.** The canvas laid out at its pixel
  size, which pushed its own column wider, over and over.
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
- **This has now been worked on from macOS as well as Windows.** Paths above are the
  Windows ones. `scripts/view.sh` falls back to `python3`, because macOS has no
  `python`.
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
camcraft-cli      inspect / post / job, all through run_job           -> all
viewer/           local inspector; part geometry never leaves the machine
```

`core` cannot see `post`, so it cannot name a G-code concept. That is what keeps
strategies emitting toolpaths rather than instructions.

The full design and its reasoning live in the approved plan at
`C:\Users\danph\.claude\plans\i-use-autodesk-fusion-wobbly-wave.md`. Read it before
changing anything structural; it records eight decisions that would force a rewrite.
