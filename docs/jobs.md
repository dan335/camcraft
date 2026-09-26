# Jobs, checks and the command line

## A job file

A job (`.camcraft`, TOML) has three groups, as in Fusion.
`examples/rail-jig.camcraft` is the reference job.

- **Models**: design files, referenced by path relative to the job file (with `/`
  between folders on every system), never copied in. A STEP file states its own
  units; an STL needs `units = "mm"` or `"inch"`.
- **Stocks**: blocks of material in model coordinates, either around models (by
  name, or the setup's own when none are named) with margins, or a fixed box. A
  fixed box has a size and, on each axis, a place against the same models, as
  Fusion's fixed size box does: centred, offset from the low or high side, or at
  a coordinate. So it follows the models when they move. A box placed any way
  but at a corner is a version 5 job. A top and a bottom setup share one block
  and see it from each side.
- **Setups**, in the order they are machined: which models and stocks each
  includes (by name), its work offset, post, orientation (the 24 square ways a part
  sits) and work zero (the model's, a point on the stock or model box, or a point
  picked on a model, plus an offset), and its operations. Everything past layout
  (`core::setup::prepare`) is in the setup's own frame, so strategies, linker,
  simulator and post never see a rotation. With the default orientation and work
  zero, a run is bit-identical to before setups existed.

An operation has a strategy (parallel finishing, Z-level roughing, a 2D profile,
a 2D pocket or drilling), a tool, heights, passes and linking. Its machining boundary, as Fusion's, keeps it
to the stock, the part's bounding box or silhouette, or faces and edges picked on
a model, with the tool's containment and an additional offset. A job from before
boundaries reads picks as a selection and no picks as the stock. Any other
boundary, or a positive offset with the whole tool inside (which now moves the
boundary in), is version 6. It may be turned off, which leaves it in the job but out of every run,
check and post.

**2D profiles and pockets** are Fusion's 2D Contour and 2D Pocket
([decision 0005](decisions/0005-2d-profile-and-pocket.md)). Their boundary is what
they cut round or clear: picked faces and edges, or with nothing picked, the
part's silhouette. The tool's centre goes its radius, plus the stock to leave
sideways, beside it: outside (the default, cutting a part out), inside (cutting a
hole out) or on it for a profile, inside for a pocket. Each is cut from the top
(the stock's, unless given) down in even levels no deeper than `stepdown`, to its
floor: the bottom given, or else the lowest picked point in each area, so pockets
picked at different depths each go to theirs, or the part's bottom. A profile
cuts each ring all the way down before the next, and never a ring before the
rings inside it, so a part's holes are cut while it is held; a pocket clears
from its middle out, the wall last. Both climb unless `conventional = true`, and
their order is their own, so they are not reordered. A V-bit profiles only on
the line. Any job with one is version 7.

```toml
[[setup.operation]]
name = "cut out"
strategy = "profile"          # side = "outside" | "inside" | "on"
stepdown = 3.0
tool = "quarter-upcut"
geometry = { bottom = { ref = "stock-bottom", offset = -0.3 } }
```

**Linking** is how the tool moves between cuts, after Fusion's
([decision 0004](decisions/0004-linking-and-entries.md)). It rapids down to the
*safe distance* (2 mm) above the material under it and feeds from there, and feeds
out of a cut only that far before rapiding. A link no longer than the *stay-down*
distance is made without lifting, at the cutting feed: 50 mm for parallel, riding
over the part; five times the tool's diameter for Z-level and 2D, straight and
level.
Parallel enters and leaves each pass along a vertical arc (the *lead radius*, a
tenth of the tool); Z-level and 2D ramp into material along the pass they are about to cut
(the *ramp angle*, 2°, at the *ramp feed*, a third of the cutting feed). After the
last cut the tool goes up to the clearance height before the spindle stops. The
settings are written only when changed, and need no newer version: a build that
reads the file without them links the old, slower way, and cuts the same.

A job's **tools** are its own copies, by name, never links to a library, so the
job still cuts what was verified after the library changes. A job whose used tools
share a T number is refused.

Each setup simulates from its own stock: the material is a height field seen from
above, so what a top setup leaves cannot be carried to a flipped bottom one. That
needs a volumetric stock.

The tolerance is a job parameter, never a constant: it drives sampling, grid
resolution, STEP tessellation and the gouge threshold alike. A job made in the app
starts at 0.05 mm; a file that leaves it out gets 0.01 mm.

### Versions

A build that predates a field would read a job that uses it, skip the field, and
cut something other than what was asked. So a job is written at the oldest version
that holds everything it says (`Job::required_version`), and a build refuses a
version newer than it reads, by name.

| Version | Needed when                                                            |
|---------|------------------------------------------------------------------------|
| 1       | nothing below                                                          |
| 2       | a pick: faces or edges on an operation, or a work zero on a model point |
| 3       | an operation's own spindle speed, or a tool that turns backwards or wants coolant |
| 4       | an operation turned off                                                |
| 5       | a fixed box placed against the models, rather than at a corner         |
| 6       | a machining boundary chosen other than picks alone, or an offset that moves one the whole tool is inside inward |
| 7       | a 2D profile or pocket                                                 |

Every version from 1 is read. The old `job.toml` format, from before setups, is
refused by name; there is no converter.

A pick is saved as the model's name, the feature's number and a point on it. The
point is what the pick *is*: a model tessellated again or re-exported with its
faces in another order still has a face through that point. A model edited so the
point is on nothing any more says so, and nothing is cut until the pick is made
again ([decision 0002](decisions/0002-feature-references.md)).

## Checks

`camcraft_session::checks` is one list, printed by the command line and shown in
the app. The gate is "no failures": what the list calls a failure and what refuses
to post are the same thing by construction.

Checks are re-derived from the path and the exact part where they can be, rather
than echoing the engine's own report: the rapid check lowers the job's cutter along
every rapid onto the mesh, and does not consult the clearance the linker planned
with.

| Code  | Level   | What it says                                                     |
|-------|---------|------------------------------------------------------------------|
| CC101 | fail    | a link was left unresolved                                       |
| CC102 | fail    | a rapid passes through the part                                  |
| CC103 | fail    | a plunge or retract moves sideways                               |
| CC104 | fail    | a feed does not suit its move (a plunge at cutting feed, say)    |
| CC105 | fail    | a cutting descent plunges past the allowance                     |
| CC106 | fail    | simulator: a rapid touched material                              |
| CC107 | fail    | simulator: the finished stock is below the part (a gouge)        |
| CC201 | warn    | spans hit the refinement limit                                   |
| CC202 | warn    | plunges that do not start above the surface                      |
| CC203 | note    | spans stopped at a surface discontinuity                         |
| CC204 | note    | passes broken at a wall or steep face                            |
| CC205 | warn    | roughing found nothing to clear                                  |
| CC206 | note    | material left more than 1 mm above the part                      |
| CC207 | warn    | a profile's or pocket's area not cut: its floor is not below the top |
| CC299 | warn    | a warning from reading the model                                 |

A **plunge** is a continuous descent steeper than `PLUNGE_RATIO` (10:1) that falls
further than `max_cut_step`, measured over the whole run of moves, not move by
move. Finishing breaks its passes there, and the gate refuses any that remain; the
checks and the app measure it the same way.

## The command line

```sh
# Run and check every setup; nothing is written
cargo run --release -p camcraft-cli -- job examples/rail-jig.camcraft

# Then write one program per operation
cargo run --release -p camcraft-cli -- job examples/rail-jig.camcraft --out out
```

`job` options:

- `--setup NAME` runs only that setup; give it more than once for several.
- `--profile P` posts every setup with `P` (a builtin name, or a TOML profile's
  path) instead of each setup's own post.
- `-o, --out DIR` writes the programs, named `<setup>-NN-<operation>`, one per
  operation that was made.
- `--force` writes even though a check failed, and says loudly that it did.

With `--out`, every setup passes or nothing is written, and every program is
rendered before any is written: half a job's programs are a trap on the machine.
The app's Post goes through the same code (`camcraft_session::plan_posting`), so
the two write the same files under the same names.

`post` writes a single finishing pass described with flags (`camcraft post --help`).
It builds a one-operation job and goes through the same `run_setup` as `job` and
the app, so there is exactly one engine path. `crates/camcraft-cli/tests/one_path.rs`
parses every written program back into positions and compares them with the paths
`run_setup` produces in-process, move for move.

Exit codes: `0` clean, `2` ran with warnings worth reading, `1` a check failed (and,
with `--out`, nothing was written).

## Posts

A setup's `post` names a builtin profile (`masso` or `grbl`, from
`crates/camcraft-post/profiles/`) or a TOML profile file, found from the job's
folder. A profile is a dialect: header and footer, word
formats, and whether drilling is posted as canned cycles (`drill_cycles`, off in
both built-in profiles until cycles have been dry-run on the machine). Golden files
in `crates/camcraft-post/tests/golden/` hold the output.
