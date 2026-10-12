# camcraft

Open-source CAM for 3-axis CNC routers, written in Rust. It turns a STEP or STL
model into G-code, and simulates every program against the material before it
will write it.

> [!WARNING]
> **Use at your own risk.** camcraft is pre-alpha and has not been proven on a real
> machine. Its G-code can be wrong in ways that crash the machine, break tools, ruin
> the part or hurt someone. Check every program yourself before it cuts: in a
> simulator you trust, then as an air cut above the stock, with a hand near the
> e-stop. The software comes with no warranty (see the licences), and its authors
> are not responsible for crashed machines, broken tools, ruined parts or any other
> damage.

## What it does

You give it a model and describe the job: the stock, the tools, how the part is
held, and the operations to run. camcraft works out the toolpaths, simulates them
cutting the stock, and writes a G-code program for each operation. There is a
desktop app for doing this by hand and a command line for scripting it. Both run
the same engine, so they always produce the same program.

If you know Fusion's CAM, it should feel familiar: the operations, their settings
and the programs they post follow Fusion's. (camcraft is not affiliated with
Autodesk.)

- **Models:** STEP and STL. STEP files are read in the units they declare, and
  every import is checked against the file it came from.
- **Roughing:** adaptive clearing and pocket clearing.
- **Finishing:** parallel, scallop, contour, flat, spiral, radial and morphed
  spiral.
- **2D:** profiles, pockets, facing and drilling.
- **Tools:** flat, ball, bullnose, V-bit and chamfer cutters, with speed and feed
  presets. There is a built-in library of twenty common bits, and Fusion tool
  libraries import from CSV.
- **Setups:** several per job, each with its own orientation, work zero and work
  offset.
- **Checking:** every program is simulated against the stock. camcraft refuses
  to write one that gouges the part or fails another check, and says why.
- **Playback:** watch the material come off, move by move, before posting.
- **Posts:** Masso (millimetres or inches) and GRBL built in, or your own
  profile written in TOML.

## Status

Version 0.2, pre-alpha. The app runs, checks, plays back and posts jobs. File
formats change without notice, and there is no release or installer yet: you
build it from source. [STATUS.md](STATUS.md) says where the work stands and what
the simulator cannot check for you.

## Try it

You need:

- **Rust**, installed with [rustup](https://rustup.rs). The repository pins the
  version it needs, and rustup fetches it on the first build.
- **cmake and a C++ compiler.** STEP import builds OpenCascade from source, so
  the first build takes several minutes; later builds reuse it.

It is developed on macOS and builds on Windows. On Windows, build from an MSVC
developer prompt so cmake can find `cl.exe`. Other platforms have not been tried.

Open the example job in the app:

```sh
git clone https://github.com/dan335/camcraft
cd camcraft
cargo run --release -- examples/rail-jig.camcraft
```

Press Generate, read the checks, play the program back, then Post. A plain
`cargo run` opens the app too, but a debug build is slower.

Or use the command line:

```sh
cargo run --release -p camcraft-cli -- job examples/rail-jig.camcraft            # run and check, write nothing
cargo run --release -p camcraft-cli -- job examples/rail-jig.camcraft --out out  # also write the G-code
```

## Documentation

| For                                                    | Read                                         |
|--------------------------------------------------------|----------------------------------------------|
| The desktop app                                        | [docs/app.md](docs/app.md)                   |
| Job files, the checks, the command line and the posts  | [docs/jobs.md](docs/jobs.md)                 |
| How simulation and verification work                   | [docs/simulation.md](docs/simulation.md)     |
| Why things are the way they are                        | [docs/decisions/](docs/decisions/)           |
| Where the work stands                                  | [STATUS.md](STATUS.md)                       |

## Development

### How it fits together

The crates depend on each other in one direction only:

```
camcraft-geom     BVH, height maps, contours, shadows, topology     (leaf)
camcraft-import   STL and STEP to meshes, checked against the file -> geom
camcraft-sim      material removal: stock updater + gouge oracle   -> geom
camcraft-core     tools, jobs, strategies, toolpaths, linking, run -> geom, sim
camcraft-post     Op IR, TOML-driven dialect renderers             -> core
camcraft-session  load, run, check, gate, post: both front ends    -> all libraries
camcraft-cli      post / job (.camcraft) -> .nc                     -> session
camcraft-gui      desktop app (egui, wgpu)                         -> session
```

`core` does not depend on `post`, so it *cannot* reference a G-code concept. That
is what enforces the pipeline:

```
Strategy -> Toolpath -> transforms -> Linker -> IR -> Post
```

Arc fitting, tabs, leads, feed optimisation and cutter compensation are each a
`Toolpath -> Toolpath` transform, so no strategy ever emits instructions directly.

Only the two binaries reach `import`, through `session`, so work on `geom`, `sim`,
`core` and `post` never waits for OpenCascade to build.

### Conventions

Millimetres, Z-up, right-handed. The controlled point is the **tool tip**. `f64`
for computation, `f32` only for grid storage. `tolerance` is a job parameter,
never a constant: it drives sampling density, grid resolution and the gouge
threshold alike.

### Correctness rules

This drives a machine that can destroy a part, a cutter, or itself. These are
not style preferences:

- **Deterministic output.** Identical inputs produce byte-identical G-code on any
  core count. No order-dependent floating point reductions.
- **No panics in library crates.** Geometry code meets degenerate meshes,
  zero-area triangles, NaNs and open shells. Fallible paths return `Result`;
  `unwrap` and `expect` are denied by lint outside tests.
- **Every iterative algorithm is explicitly bounded** and reports when it hits
  the cap. A bounded algorithm that reports "incomplete" beats one that spins.
- **Structured diagnostics, never silence.** Silently emitting bad G-code is the
  worst available failure mode.
- **The simulator gates release.** No cut from a toolpath it has not verified.

### Checking a change

There is no CI yet, so one script is the only thing between a mistake and
`main`. Run it before every push:

```sh
bash scripts/check.sh               # the crates your changes can affect: before committing
bash scripts/check.sh --since main  # the same, for everything since main
bash scripts/check.sh --dry-run     # which crates that is, without running anything
bash scripts/check.sh --all         # every crate: before pushing
bash scripts/check.sh --full        # every crate, plus benchmarks
```

By default the script checks the crates your uncommitted changes touch and every
crate that depends on them. A change outside `crates/` that is not documentation (a
manifest, the lockfile, an example, a fixture) checks everything. Run it
unpiped: piping through `tail` hides progress on a suite that takes minutes. On
Windows, `.\scripts\check.ps1` (and `-Full`) checks every crate.

Plain `cargo build` and `cargo test` cover only the app, so that `cargo run`
opens it. Add `--workspace` to build or test everything.

`vendor/` holds OpenCascade's Rust bindings, patched to build with MSVC and to
report each B-rep face's triangle count, which picking needs. The reasoning is
in [docs/decisions/0001-step-backend.md](docs/decisions/0001-step-backend.md).

## License

Dual licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your
option. Third-party code and assets keep their own licences: `vendor/` is
LGPL-2.1, and [NOTICE](NOTICE) lists the rest.

**Not yet settled for distributed binaries.** STEP import links OpenCascade
statically, through the `opencascade` and `occt-sys` crates, and both are
LGPL-2.1 (OpenCascade adds its own exception). The source here stays MIT or
Apache-2.0, but a `camcraft` binary carries LGPL obligations until that is
resolved. See [docs/decisions/0001-step-backend.md](docs/decisions/0001-step-backend.md).
