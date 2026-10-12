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

2D linking is Fusion's 2D Contour's
([decision 0006](docs/decisions/0006-2d-linking-and-arcs.md)): a profile's lead
is a vertical arc, a straight and a level arc; it goes down by a helix beside
the lead (or a plunge, or a ramp round the ring in the direction it cuts), stays
down between depths, and its rings follow the model's round edges as exact
arcs, posted as `G2`/`G3` in `G17`, `G18` and `G19`. Programs start and end as
Fusion's post does. The lego contour posts block for block as Fusion's does.

1. **Smooth transitions,** after Fusion's defaults
   ([decision 0012](docs/decisions/0012-transitions-and-entries-from-fusions-defaults.md)):
   zigzag passes are joined by a half turn tangent to both, contour ramps down
   the wall from level to level, and pockets go down a helix. Still to come: a
   turn between passes further apart than the tool, or not running opposite
   ways, and contour's helix entries, which need the linker to judge material
   by the tool's own shape.
2. **Exact circles from the B-rep.** A ring's arcs take their centres and radii
   from the model's topology, whose round edges are circles through the
   tessellation's points, which lie on the B-rep's curves; OpenCascade's own
   `gp_Circ` is not read yet. Ellipses and splines are followed by lines.
3. **More strategies**. Contour (a waterline, which scallop leaves the walls
   to) and flat finishing are done, in Z levels from the region the tool may
   not enter ([decision 0010](docs/decisions/0010-contour-and-flat.md)),
   without yet a flat's own depths. Spiral,
   radial and morphed spiral are done, patterns dropped onto the part through
   one pipeline with parallel's sampling and scallop's slopes
   ([decision 0011](docs/decisions/0011-patterns-dropped-onto-the-part.md)),
   without yet a centre picked on the model. Still to come: steep and shallow,
   pencil, ramp, and rest machining. Scallop,
   as Fusion's, is done
   ([decision 0009](docs/decisions/0009-scallop-distance-field.md)), without
   yet a limit on its stepovers, rest machining, smoothing or a contact point
   boundary; it measures along where the tool's tip rides, not the part itself.
   Adaptive clearing, as Fusion's 3D Adaptive,
   is done ([decision 0007](docs/decisions/0007-adaptive-clearing.md)), without
   yet rest material from earlier operations, fine stepdown, shallow areas or
   slot clearing; its stay-down links are straight. Pocket clearing, as
   Fusion's 3D Pocket Clearing, is Z-level roughing renamed, now cutting each
   level's rings from the middle out, the ring along the walls last, climb or
   conventional, with flat areas
   ([decision 0008](docs/decisions/0008-pocket-clearing.md)); it has no corner
   smoothing or finishing pass. Facing, as Fusion's Face, is done. 2D profile
   and pocket are done
   ([decision 0005](docs/decisions/0005-2d-profile-and-pocket.md)), with tabs
   (on profiles, as Fusion's on a 2D contour), a profile's horizontal leads,
   and a pocket's finishing passes with leads onto them; they lack leads on a
   pocket's roughing rings, a profile's finishing passes, and picked entry
   points and tab positions. Stock to leave is Fusion's, radial and axial, negative
   included, on every operation; which shared settings a strategy uses is one
   table (`StrategySpec::uses`), for a new strategy to fill in.
4. **A volumetric stock**, so what a top setup leaves carries to a flipped bottom
   one.
5. **Exact slicing of the offset surface** for Z-level work
   ([decision 0003](docs/decisions/0003-grids-only-for-stock.md)).
6. The work offset manager in the toolbar; Fusion's `.json` tool libraries, which
   carry holders; tapered ball mills.

## Before the first cut

The simulator verifies against a *simulated* part. It cannot check the things that
actually break cutters on the day:

- **No built-in post has run on a machine.** The Masso profiles were built from
  the controller's documented G-code reference, the Centroid ones for an Avid
  router from Avid's own Fusion post for CNC12. Dry-run them well above the work
  first. The tests read every program back as a controller would and hold it to
  its path (`camcraft_post::readback`), but nothing there knows what a machine's
  own macros do, such as Avid's tool change at `M6`. Canned drilling cycles are
  off in every built-in profile until they have been dry-run.
- Nothing knows whether the WCS is where the job assumes, whether the tool in the
  spindle matches its `diameter`, or whether the stock is the size the job was told.
- Roughing is contour-parallel clearing that ramps into material at 2°. It has
  never cut anything.

## Open questions

- **Licensing of distributed binaries.** STEP import links OpenCascade statically
  through LGPL-2.1 crates, while camcraft is MIT or Apache-2.0. Publishing the source
  is fine, but this has to be settled before anyone ships a binary. Replacing
  OpenCascade with a pure-Rust reader would settle it. None is ready yet: ogeom's
  unreleased main comes closest, passing 91 of step-conformance's 96 runs where
  OpenCascade passes all 96. `scripts/kernel-trial.sh` says when one is. See
  [decision 0001](docs/decisions/0001-step-backend.md).
- **One unexplained STEP read failure.** During a full gate run on 2026-09-23, two
  session tests reading `hinge_jig.step` failed with OpenCascade's "failed to read
  STEP file". The same tests passed 80 times since, with and without a lock that
  serialises every OpenCascade call (`camcraft_import::step`). The lock stays as a
  precaution, since OpenCascade's data exchange uses process-wide statics. If it
  happens again, capture the run: this is not understood.

## Standing cautions

- **`fixtures/step/` holds real shop jigs,** published with the repository by choice.
  Parts that cannot be published belong in `CAMCRAFT_FIXTURES`, never in a commit.
- **B-spline and revolved faces are only box-checked on import.** Faces on planes,
  cylinders, cones, spheres and tori are held to their surfaces, but a B-spline, a
  surface of revolution or an extrusion only to its box, so surface wrong inside the
  part there passes. `scripts/kernel-trial.sh` measures it for a candidate kernel.
- **The public fixture set cannot reproduce the bug that disqualified
  `truck-stepio`.** The part that exposed it is private. A synthetic
  sculpted-surface fixture should replace it.
- **`vendor/` patches opencascade** (the MSVC fix, and each B-rep face's triangle
  count, which picking needs). Delete it and its `[patch.crates-io]` block only once
  a release includes both.
- **There is no CI yet**, so `scripts/check.sh` is the only
  thing between a mistake and `main`. Run it unpiped: piping through `tail` buffers
  the whole run and hides progress.
