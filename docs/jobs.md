# Jobs, checks and the command line

## A job file

A job (`.camcraft`, TOML) has three groups, as in Fusion.
`examples/rail-jig.camcraft` is the reference job.

- **Models**: design files, referenced by path relative to the job file (with `/`
  between folders on every system), never copied in. A STEP file states its own
  units; an STL needs `units = "mm"` or `"inch"`. Each may have a `transform`, as
  Fusion's position, rotation and scale: `position` where the file's origin goes,
  `rotation` in degrees about the job's X, then Y, then Z, about that origin, and
  `scale` along the file's X, Y and Z, more than zero (never mirrored), for
  example `transform = { position = [60.0, 5.0, 0.0], rotation = [0.0, 0.0, 30.0],
  scale = [1.5, 1.5, 1.5] }`. Everything past loading sees the model where it is
  placed; a pick's point stays where the file has it, so picks move with their
  model.
- **Stocks**: blocks of material in the job's coordinates, either around models (by
  name, or the setup's own when none are named) with margins, or a fixed box. A
  fixed box has a size and, on each axis, a place against the same models, as
  Fusion's fixed size box does: centred, offset from the low or high side, or at
  a coordinate. So it follows the models when they move. A top and a bottom
  setup share one block
  and see it from each side.
- **Setups**, in the order they are machined: which models and stocks each
  includes (by name), its work offset, post, orientation (the 24 square ways a part
  sits) and work zero (the model's, a point on the stock or model box, or a point
  picked on a model, plus an offset), and its operations. Everything past layout
  (`core::setup::prepare`) is in the setup's own frame, so strategies, linker,
  simulator and post never see a rotation. With the default orientation and work
  zero, a run is bit-identical to before setups existed.

An operation has a strategy (parallel finishing, Z-level roughing, a 2D profile,
a 2D pocket, facing or drilling), a tool, heights, passes and linking. What
Fusion's Geometry tab holds is three settings, each read only by the strategies
`StrategySpec::uses` says: `selection`, the faces and edges picked on the
models; a 3D operation's `boundary`, which, as Fusion's machining boundary, keeps
it to the stock (the default), the part's bounding box or silhouette, or the
selection, with the tool's `containment` and an additional offset (`margin`);
and a 2D operation's `outline`, what it cuts round or clears. `heights` holds
the clearance (where the tool arrives and leaves, 10 mm over the stock), the
retract height (Fusion's, 5 mm over it) and the top and bottom it cuts between. `passes.stepdown` is
Fusion's multiple depths, shared the same way: Z-level roughing must have one,
profiles, pockets and facing cut the bottom in one pass without, and parallel
and drilling never read it. An operation may be
turned off, which leaves it in the job but out of every run, check and post.

```toml
[[setup.operation]]
name = "finish"
strategy = "parallel"
tool = "finish-4-ball"
boundary = { kind = "silhouette", containment = "center", margin = 1.0 }
heights = { clearance = { ref = "stock-top", offset = 10.0 }, bottom = { ref = "model-bottom" } }
```

**2D profiles and pockets** are Fusion's 2D Contour and 2D Pocket
([decision 0005](decisions/0005-2d-profile-and-pocket.md)). Their `outline` is
what they cut round or clear: the part's silhouette (the default), or
`outline = "selection"`, the picked faces and edges. The tool's centre goes its radius, plus the stock to leave
sideways, beside it: outside (the default, cutting a part out), inside (cutting a
hole out) or on it for a profile, inside for a pocket. Each is cut from the top
(the stock's, unless given) down in even levels no deeper than `passes.stepdown`
(Fusion's multiple depths; without one, the floor in one pass), to its
floor: the bottom given, or else the lowest picked point in each area, so pockets
picked at different depths each go to theirs, or the part's bottom. A top or
bottom measured from the selection (`ref = "selection"`, Fusion's "selected
contours") is each area's own: the highest or lowest point picked in it, plus the
offset, so `bottom = { ref = "selection", offset = -0.5 }` cuts half a millimetre
under every picked loop. A Z-level or facing operation measures one from all its
picks; a clearance or a drill's bottom never is, and nothing picked is refused. A profile
cuts each ring all the way down before the next, and never a ring before the
rings inside it, so a part's holes are cut while it is held; a pocket clears
from its middle out, the wall last. Both climb unless `conventional = true`, and
their order is their own, so they are not reordered. A V-bit profiles only on
the outline. The radial stock to leave, negative included, moves the cut out
from the outline, or in; floors are still found among the picks on the outline
itself. A ring follows the model: where the outline runs round
one of the model's round edges that lies level, the ring is that circle's arc,
and the corner the tool's centre turns round a sharp one is an arc about the
corner, each posted as one `G2` or `G3`. Rings are checked against the polygon
clipping, which decides what the shape is (where a narrow neck closes or a small
hole vanishes), and a ring that is not the same shape, within twice the
tolerance, follows the polygon instead.

```toml
[[setup.operation]]
name = "cut out"
strategy = "profile"          # side = "outside" | "inside" | "on"
tool = "quarter-upcut"
heights = { bottom = { ref = "stock-bottom", offset = -0.3 } }
passes = { stepdown = 3.0, tabs = { width = 6.0, height = 3.0, spacing = 150.0 } }
```

**Tabs** (`passes.tabs`) hold what a cut frees: bridges of material left
standing across it, as Fusion's holding tabs. They go on profiles, as Fusion's
go only on a 2D contour: every ring a profile cuts is beside what it keeps,
where a pocket's or a roughing level's clearing rings are in the waste, and
tabs on them would leave pillars standing there (`camcraft_core::tabs` works on
any path's closed loops; `StrategySpec::uses` says which strategy has them). A
pocket or roughing operation given them keeps them, unused. They go evenly
round each loop cut at the operation's deepest level, about `spacing` apart and at
least two to a loop, clear of where the loop starts; a loop too short for one
gets none, with a warning. Each is `width` of material wide along the cut at its
top and `height` above the floor, and every cutting move over one, at any level,
rides over it: `shape = "rectangular"` (the default) steps straight up and
plunges down off it at the plunge feed, `"triangular"` climbs and ramps off at
45°. They are put on before linking, so links and ramps plan round them.

**Stock to leave** (`passes.stock_to_leave`) is Fusion's radial and axial
stock to leave, in millimetres: `radial` on the walls, `axial` on the floors,
and between the two on a slope. Positive leaves material for a later operation
to finish; negative cuts past the part, for a looser fit. Beside, it must stay
under the tool's radius; down, in 3D with a ball or bullnose, within its corner
radius (a flat end, or a 2D floor, may go any depth). Every operation has it,
and cuts with as much as its strategy uses: a drill none, a profile on the
boundary only its floor's. New Z-level roughing starts at 0.5 mm each way, as
Fusion's roughing does; everything else at none, which is not written. The 3D
strategies grow the tool by it (`camcraft_core::allowance`); a 2D one moves its
cut out by it and raises its floors by it. The material is cut by the real
tool, so what is left shows in the simulation, and what was cut past the part
on purpose is not called a gouge.

```toml
passes = { stock_to_leave = { radial = 0.5, axial = 0.25 } }
```

**Leads** (`linking.leads`) are how a profile comes onto each ring and goes off
it, in the waste beside it, as Fusion's lead-in and lead-out, the same both ways:
down a vertical arc (`vertical`), along a straight (`straight`), and round a level
arc (`radius`, turning `sweep` degrees, 90) onto the ring, tangent to it; and off
it the mirror image, up at the end. Each size is a tenth of the tool's diameter
unless given, as Fusion's, and zero leaves that piece out. Every piece is an arc
or a straight line in the program. A level arc that would leave the waste is
tried at half and a quarter the radius; a ring that starts at a corner starts
instead halfway along its longest side, where an arc can join it; a ring with no
room for any goes without, with a warning. A groove on the boundary has none. New
profiles have them.

```toml
linking = { leads = { radius = 3.0, sweep = 90.0, straight = 1.0, vertical = 0.6 } }
```

A profile's **entry** (`linking.entry`) is how it goes down into material to
start each ring, beside the lead:

- `"helix"`, the default: down a helix in the waste beside where the lead
  starts (or the ring, without one), at the ramp angle, onto the vertical arc's
  top, then once round level, as Fusion's helix ends, so its circle is cut to
  the depth all round and the next depth's way in cuts nothing on the way. Its circle, as the
  tool's centre goes, is `helix_diameter` across (0.9 × the tool's); where the
  part leaves no room for it, or for one half or a quarter the size, the tool
  goes straight down instead.
- `"plunge"`: straight down at the plunge feed onto the vertical arc's top, as
  Fusion's 2D contour does without a ramp.
- `"ramp"`: down along the ring itself, in the direction it cuts, onto its start,
  where it is entered without its lead-in.

Neither a helix nor a ramp round a ring goes down more in one lap than the
*most per lap* (`ramp_stepdown`, Fusion's maximum ramp stepdown): the
operation's stepdown, or the tool's diameter without one. From one depth of a
ring to the next the tool stays down, as Fusion's does: off the ring along the
lead, across to where the next depth's way in starts, and down it.

**Linking** is how the tool moves between cuts, after Fusion's
([decision 0004](decisions/0004-linking-and-entries.md), and for 2D
[decision 0006](decisions/0006-2d-linking-and-arcs.md)). It rapids down to the
*safe distance* (2 mm) above the material under it and feeds from there, and feeds
out of a cut only that far before rapiding. It arrives at the clearance height
above the first cut, and between cuts it goes across as Fusion's *retraction*
says: `"minimum"` (the default), the safe distance over the material under the
way across as the operation found it; `"full"`, the retract height, or higher
where the material is. A link no longer than the *stay-down*
distance is made without lifting, at the cutting feed: 50 mm for parallel, riding
over the part; five times the tool's diameter for Z-level and 2D, straight and
level, and for a profile down to its ring's next depth.
Parallel enters and leaves each pass along a vertical arc (the *lead radius*, a
tenth of the tool); Z-level, pockets and facing ramp into material along the pass
they are about to cut, in the direction it cuts (the *ramp angle*, 2°, at the
*ramp feed*, a third of the cutting feed). After the last cut the tool goes up to
the clearance height before the spindle stops. The settings are written only when
changed.

A job's **tools** are its own copies, by name, never links to a library, so the
job still cuts what was verified after the library changes. A job whose used tools
share a T number is refused.

Each setup simulates from its own stock: the material is a height field seen from
above, so what a top setup leaves cannot be carried to a flipped bottom one. That
needs a volumetric stock.

The tolerance is a job parameter, never a constant: it drives sampling, grid
resolution, STEP tessellation and the gouge threshold alike. A job made in the app
starts at 0.05 mm; a file that leaves it out gets 0.01 mm.

### Format

There is one job format, `version = 1`, and no backwards compatibility while
camcraft is pre-alpha: a file of any other version is refused by name, and a file
written before a change may no longer read. There is no converter.

A setting the format does not have is refused, never ignored: a misspelt key, or
one moved since the file was written, would otherwise leave the job cutting
without it. One of an operation's `passes`, `heights` or `linking` written
directly in the operation is named with the section it belongs in. Tool
libraries and machine profiles are read the same way.

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

A program starts and ends as Fusion's do. Before the tool change the spindle
stops and the tool goes straight up clear of everything (`safe_retract`: Masso's
`G28 G91 Z0`, GRBL's `G53 G0 Z0`); the first move then goes across at that height
and down to where the path starts, never diagonally from wherever the change left
the tool. A profile without a safe retract rises (or falls) to the path's crossing
height first and goes across there. After the last cut the spindle stops, the tool
goes up again, and the machine goes home across (`end_home`: Masso's `G28 G91 X0
Y0`; GRBL's stored `G28` position may be anywhere, so it has none).

A path's arcs are arcs in the program, not strings of short moves: `G2` or `G3`
in the plane they turn in (`G17` level, `G18` or `G19` for an arc in an upright
plane, such as a lead), a helix with the third axis moving, the plane put back to
`G17` before the spindle stops. The engine makes an arc only where a three-axis
machine can cut one, about X, Y or Z; elsewhere, as a lead on a pass running
across the part at an angle, it makes short straight moves within the tolerance,
so the program is always the path that was simulated. With centre offsets
(`arc_style = "ij"`) no block turns more than half a turn, and with a radius
(`"r"`) no more than a quarter, whose sign would otherwise have to say which way
round.
