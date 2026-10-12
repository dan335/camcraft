# camcraft

3-axis CAM for CNC routers. Takes a 3D model and a job description, emits G-code.

**Status: 0.2, pre-alpha.** A desktop app and a command line on one engine. Takes
STEP or STL models and a job file, writes G-code, and refuses to write a program the
simulator has not verified against material. STEP files are read in the units they
declare, and every import is checked against the file it came from.
Nothing has been cut yet. See [STATUS.md](STATUS.md) for where the work left off and
what the simulator cannot check for you.

## What it is

- **3D-mesh-first.** Real surface machining, not 2.5D with 3D bolted on.
- **Native and parallel.** No mesh decimation, no vertex budgets, no accuracy traded
  for speed.
- **A CLI and a desktop app on one engine path.** The `camcraft` command line
  scripts it ([docs/jobs.md](docs/jobs.md)). `camcraft-gui` edits jobs, runs and
  checks them, plays the program back against the material, and posts
  ([docs/app.md](docs/app.md)). Both go through the same library, so they cannot
  disagree about what runs or what is posted.

## Architecture

The dependency graph is acyclic, and the direction carries meaning:

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

`core` does not depend on `post`. It therefore *cannot* reference a G-code concept,
which is what structurally enforces the pipeline:

```
Strategy -> Toolpath -> transforms -> Linker -> IR -> Post
```

Arc fitting, tabs, lead in/out, feed optimisation and cutter compensation are each a
`Toolpath -> Toolpath` transform. They have nowhere to live if a strategy emits
instructions directly, so no strategy ever does.

`import` is reached only through `session`, which only the two binaries use. So
whatever the STEP backend costs to build, the inner development loop on
`geom` / `sim` / `core` / `post` never pays it.

## Conventions

Millimetres, Z-up, right-handed. The controlled point is the **tool tip**. `f64` for
computation, `f32` only for grid storage. `tolerance` is a job parameter, never a
constant — it drives sampling density, grid resolution and the gouge threshold alike.

## Correctness rules

This drives a machine that can destroy a part, a cutter, or itself. These are not
style preferences:

- **Deterministic output.** Identical inputs produce byte-identical G-code on any
  core count. No order-dependent floating point reductions.
- **No panics in library crates.** Geometry code meets degenerate meshes, zero-area
  triangles, NaNs and open shells. Fallible paths return `Result`; `unwrap` and
  `expect` are denied by lint outside tests.
- **Every iterative algorithm is explicitly bounded** and reports when it hits the
  cap. A bounded algorithm that reports "incomplete" beats one that spins.
- **Structured diagnostics, never silence.** Silently emitting bad G-code is the
  worst available failure mode.
- **The simulator gates release.** No cut from a toolpath it has not verified.

## Build, run and check

```sh
cargo build --workspace
bash scripts/run.sh [JOB]...                                    # the app
cargo run --release -p camcraft-cli -- job examples/rail-jig.camcraft --out out
```

There is deliberately **no CI** while this repository is private, so one script is
the only thing standing between a mistake and `main`. Run it before every push:

```sh
bash scripts/check.sh               # the crates your changes can affect: before committing
bash scripts/check.sh --since main  # the same, for everything since main
bash scripts/check.sh --dry-run     # which crates that is, without running anything
bash scripts/check.sh --all         # every crate: before pushing
bash scripts/check.sh --full        # every crate, plus benchmarks
```

By default it checks only what the uncommitted changes can affect: the crates they
touch and every crate that depends on them. A change to `camcraft-gui` checks only
`camcraft-gui`; one to `camcraft-core` checks core and everything above it. Anything
outside `crates/` that is not documentation (a manifest, the lockfile, an example,
a fixture) checks everything.

Run it unpiped. Piping through `tail` buffers the whole run and hides progress on
a suite that now takes minutes; read the tail of the output afterwards instead.

On Windows, `.\scripts\check.ps1` (and `-Full`) checks every crate.

It checks each gate on its own exit status and prints a summary, rather than
chaining commands in a way that can report success without having checked anything.
Restore the GitHub Actions workflow when the repository goes public.

`camcraft-import` pulls in OpenCascade, which adds **cmake and a C++ toolchain** as
prerequisites (on Windows, build from an MSVC environment so cmake can find
`cl.exe`). That crate is reached only by the two binaries, so the inner loop on
`geom` / `sim` / `core` / `post` never pays that cost. `vendor/` holds opencascade's
sources, patched on every platform: the MSVC fix, and each B-rep face's triangle
count, which picking needs (see the root `Cargo.toml`). Reasoning in
[docs/decisions/0001-step-backend.md](docs/decisions/0001-step-backend.md).

## License

Dual licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
Third-party code and assets keep their own licences: `vendor/` is LGPL-2.1, and
[NOTICE](NOTICE) lists the rest.

**Not yet settled for distributed binaries.** STEP import links OpenCascade
statically, through the `opencascade` and `occt-sys` crates, and both are LGPL-2.1
(OpenCascade adds its own exception). The source here stays MIT or Apache-2.0, but a
`camcraft` binary carries LGPL obligations until that is resolved. See
[docs/decisions/0001-step-backend.md](docs/decisions/0001-step-backend.md).
