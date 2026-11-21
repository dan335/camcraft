# Contributing to camcraft

camcraft turns a model into G-code for a 3-axis router, and that G-code drives a
machine that can destroy a part, a cutter or itself. A change is welcome when it
keeps to the rules on this page.

## Build it and run the gates

You need [rustup](https://rustup.rs). The repository pins the Rust version in
`rust-toolchain.toml`, and rustup fetches it on the first build.

```sh
cargo run --release -- examples/rail-jig.camcraft   # the app, with the example job
cargo test --workspace                              # every crate's tests
```

There is no CI, so `scripts/check.sh` is the only thing between a mistake and
`main`. Run it before every commit, and with `--all` before every push:

```sh
bash scripts/check.sh               # the crates your changes can affect
bash scripts/check.sh --since main  # the same, for everything since main
bash scripts/check.sh --all         # every crate
bash scripts/check.sh --full        # every crate, plus benchmarks
```

On Windows, `.\scripts\check.ps1` (and `-Full`) checks every crate. The gates are:

- **fmt**: `cargo fmt --check`.
- **clippy**: no warnings, on every target.
- **test**: every test of the crates checked.
- **doc**: the API documentation builds.
- **determinism**: the tests that compare camcraft's output against itself, such
  as one roughing run at one thread and at eight. The gate fails unless every one
  of them ran.
- **names**: no tracked file carries a product name the rules below keep out.
- **bench**: the benchmarks, with `--full` only.

Each gate is reported on its own exit status in a summary at the end. Run the
script unpiped: piping it through `tail` hides progress on a suite that takes
minutes.

## Rules that are not negotiable

**camcraft is its own work.** Do not copy another product's code, help text,
tooltips, icons, artwork, formulas or templates, and do not paraphrase them closely
either. Design behaviour from general machining practice, public documentation,
the geometry of the problem and what users report.

**No commercial CAD or CAM software in the loop.** Do not drive, script or
automate commercial CAD or CAM software for this project, and do not take data
from it: no programs, measurements, screenshots, settings or files made with it
go into the repository or into a test. Tests check camcraft's own results: that
every check passes and nothing gouges, figures recorded from camcraft's own runs
(lifts, feed length, the simulated stock left), and values worked out by hand or
from the geometry.

**Commercial products are not named.** Not in code, comments, documentation,
commit messages, issues or release notes. Say what camcraft does in its own
terms. The **names** gate fails if a tracked file names the ones it knows. An
open-source project may be named to say what camcraft uses from it, or that it
takes nothing from it. File formats keep their own names (STEP, STL, G-code),
and identifiers already in job files stay as they are.

**No copyleft code.** Do not copy, port or read GPL, LGPL or AGPL code for this
project, and do not add a dependency under any of them. Every dependency must be
under a licence that `scripts/release/about.toml` accepts, and each release ships
the `cargo about` list of them (`scripts/release/README.md`). STEP import is held
on ogeom 0.11 because later versions are LGPL
([decision 0001](docs/decisions/0001-step-backend.md)). An asset (a font, an icon,
a theme, a test part) is original or openly licensed, and [NOTICE](NOTICE) says
where it came from.

**Private parts stay private.** A part that cannot be published never goes in a
commit, not even one that is later reverted, because it stays in the history.
Tests read such parts from `CAMCRAFT_FIXTURES` ([fixtures/step](fixtures/step/README.md)).
In anything written about one, it is "a user's part".

**The correctness rules in the [README](README.md#correctness-rules) hold.**
Identical inputs give byte-identical G-code on any core count. Library crates do
not panic. Every iterative algorithm is bounded and says when it hits its bound.
A problem is reported, never swallowed. No program is written from a toolpath the
simulator has not checked.

**Fix the cause.** A test that fails is a finding. Do not loosen its expectation or
edit its input to make it pass; find why it fails and fix that, and say in the
commit why any figure a test holds has moved.

## Commits and documents

A commit message is one plain sentence saying what changed. A change that lands
on `main` brings [PROGRESS.md](PROGRESS.md) up to date: its row, its counts and
*What is next*. A decision worth keeping goes in [docs/decisions/](docs/decisions/).
camcraft comes with no warranty: see [DISCLAIMER.md](DISCLAIMER.md).
