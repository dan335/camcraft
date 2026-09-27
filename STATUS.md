# Where this is

Last updated 2026-09-26. Read this before picking the work back up.

**0.2 is done: the desktop app runs, checks, plays back and posts jobs**, and some
of 0.3 came in with it (drilling, 2D profiles and pockets, V-bits and chamfer
mills, speed and feed presets, Fusion CSV tool import, a built-in System library
of twenty common bits). All gates are green. The engine takes STEP or STL models
and a job file and writes G-code, and refuses to write a program that fails a
check, the simulator's against material among them.

**Nothing has been cut yet.** See *Before the first cut*: there are things only the
machine can tell us.

## Where to read

| For                                                     | Read                                              |
|---------------------------------------------------------|---------------------------------------------------|
| What camcraft is, the crates, the rules, how to build   | [README.md](README.md)                            |
| The job file, its versions, the checks, the CLI, posts  | [docs/jobs.md](docs/jobs.md)                      |
| The app: tree, properties, picking, playback, tools     | [docs/app.md](docs/app.md)                        |
| Simulation, verification, playback internals, memory    | [docs/simulation.md](docs/simulation.md)          |
| Decisions and their reasoning                           | [docs/decisions/](docs/decisions/)                |
| Bugs found by looking, and what they taught             | [docs/history.md](docs/history.md)                |

## Running it

```sh
bash scripts/run.sh examples/rail-jig.camcraft        # the app: Generate, read the checks, play, Post
cargo run --release -p camcraft-cli -- job examples/rail-jig.camcraft            # run and check
cargo run --release -p camcraft-cli -- job examples/rail-jig.camcraft --out out  # and write programs
bash scripts/check.sh                                 # the gates, for what changed
```

On Windows, `scripts\run.bat` and `.\scripts\check.ps1`, from an MSVC environment.
Day to day the work is on macOS; the lego brick (`lego_4x2_01.step`) is the part
the app is usually tried on.

Last full run on the 237 × 227 × 76 mm router jig, read straight from
`sled_base.step`, 20 s on an M-series Mac; about 4.6 hours of machining at the
estimate (`docs/decisions/0004-linking-and-entries.md` has the breakdown):

```
rough:  1262 passes, 1057 links stayed down, 169 ramped, removed 3558704 mm3, travel saved 78%, rapid contacts 0
finish:  556 passes,  323 links stayed down,   0 ramped, removed   55737 mm3, travel saved  0%, rapid contacts 0
verified: 920799 column(s) compared, gouges 0, deepest 0.000 mm, budget 0.217 mm
```

Finishing saving 0% travel is correct: a parallel zigzag already ends each pass
next to where the following one starts.

## What is next

Not yet built, roughly in the order they matter:

Linking and entries are done, after Fusion: safe distance, staying down, leads,
ramps, a retract before the spindle stops, and stepovers that follow the tool
([decision 0004](docs/decisions/0004-linking-and-entries.md)). The router jig's
program went from about 8 h to 4.6 h.

1. **Helical ramps and smooth transitions.** Z-level ramps along its own ring and
   parallel's stay-down links are straight lines lifted over the part; Fusion's
   are helices and tangent arcs.
2. **Start rapids split XY then Z.** The first move is one G0 in all three axes
   from wherever the tool change left the spindle.
3. **More strategies**: waterline, adaptive clearing. Facing, as Fusion's Face,
   is done. 2D profile and pocket are
   done ([decision 0005](docs/decisions/0005-2d-profile-and-pocket.md)), with
   tabs (on profiles, as Fusion's on a 2D contour) and a profile's horizontal
   leads; they lack finishing passes, a pocket's leads, and picked entry points
   and tab positions. Stock to leave is Fusion's, radial and axial, negative
   included, on every operation; which shared settings a strategy uses is one
   table (`StrategySpec::uses`), for a new strategy to fill in.
4. **Arc fitting**, as a `Toolpath -> Toolpath` transform.
5. **A volumetric stock**, so what a top setup leaves carries to a flipped bottom
   one.
6. **Exact slicing of the offset surface** for Z-level work
   ([decision 0003](docs/decisions/0003-grids-only-for-stock.md)).
7. The work offset manager in the toolbar; Fusion's `.json` tool libraries, which
   carry holders; tapered ball mills.

## Before the first cut

The simulator verifies against a *simulated* part. It cannot check the things that
actually break cutters on the day:

- **The Masso profile has never run on the machine.** It was built from the
  controller's documented G-code reference. Dry-run it well above the work first.
  Canned drilling cycles are off in both built-in profiles until they have been
  dry-run.
- Nothing knows whether the WCS is where the job assumes, whether the tool in the
  spindle matches its `diameter`, or whether the stock is the size the job was told.
- Roughing is contour-parallel clearing that ramps into material at 2°. It has
  never cut anything.

## Open questions

- **Licensing of distributed binaries.** STEP import links OpenCascade statically
  through LGPL-2.1 crates, while camcraft is MIT or Apache-2.0. This doesn't matter
  while the repo is private, but it has to be settled before anyone ships a binary.
  See [decision 0001](docs/decisions/0001-step-backend.md).
- **One unexplained STEP read failure.** During a full gate run on 2026-09-23, two
  session tests reading `hinge_jig.step` failed with OpenCascade's "failed to read
  STEP file". The same tests passed 80 times since, with and without a lock that
  serialises every OpenCascade call (`camcraft_import::step`). The lock stays as a
  precaution, since OpenCascade's data exchange uses process-wide statics. If it
  happens again, capture the run: this is not understood.

## Standing cautions

- **`fixtures/step/` holds real shop jigs.** They are not customer work, but they are
  not public either. Decide before making the repo public — deleting them later does
  not remove them from history. One commercial part was already removed and
  scrubbed; parts that cannot be published belong in `CAMCRAFT_FIXTURES`.
- **The public fixture set cannot reproduce the bug that disqualified
  `truck-stepio`.** The part that exposed it is private. A synthetic
  sculpted-surface fixture should replace it.
- **`vendor/` patches opencascade** (the MSVC fix, and each B-rep face's triangle
  count, which picking needs). Delete it and its `[patch.crates-io]` block only once
  a release includes both.
- **There is no CI while the repo is private**, so `scripts/check.sh` is the only
  thing between a mistake and `main`. Run it unpiped: piping through `tail` buffers
  the whole run and hides progress.
