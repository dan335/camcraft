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

An operation has a strategy (parallel, scallop, contour, flat, spiral, radial or
morphed spiral finishing, adaptive clearing, pocket clearing, a 2D profile, a 2D
pocket, facing or drilling), a tool, heights, passes and linking. Scallop (`"scallop"`) is Fusion's: passes a stepover apart
along the surface, offset in from the boundary and from where the tool stops
touching the part, so the cusp is the same on a slope as on the flat
([decision 0009](decisions/0009-scallop-distance-field.md)). It cuts `direction =
"both-ways"` (closed rings climbing, open passes from their nearer end) unless
`"climb"` or `"conventional"`, `order = "outside-in"` unless `"inside-out"`, and
only where the surface is between `slope_from` and `slope_to` degrees from level
(0 and 90), never steeper than a plunge. Its stepover starts as parallel's.
Contour (`"contour"`) is Fusion's: passes round the part at levels a stepdown
apart (`passes.stepdown`, 1 mm to start), each where the tool touches the walls
at its height, and one at the foot of every wall standing on a flat, from the
part's top to its bottom unless `heights` says otherwise
([decision 0010](decisions/0010-contour-and-flat.md)). It climbs unless
`direction = "conventional"` or `"both-ways"`, cuts `order = "by-depth"` (each
level before the next) unless `"by-area"`, and keeps to `slope_from` and
`slope_to`. From a level's last ring to the next level's, where that starts
beside where the last ended, it goes down the wall rather than off and down, by
Fusion's default *smooth* transition: along the lower ring at no more than the
ramp angle (`linking.ramp_angle`, 2°), then once round it from where the ramp
reaches it. Where it may not stay down (`stay_down = 0`) it lifts as before.
Into each level otherwise it comes down onto its lead beside the wall. Flat (`"flat"`) is Fusion's too: each flat face of the part between
the top and bottom cleared at its own height, rings a stepover apart in from its
edges, climbing unless conventional, `order = "outside-in"` unless
`"inside-out"`; its stepover starts as a pocket's. Spiral (`"spiral"`), radial
(`"radial"`) and morphed spiral (`"morphed-spiral"`) are patterns laid out seen
from above and dropped onto the part, as Fusion's
([decision 0011](decisions/0011-patterns-dropped-onto-the-part.md)), each with
the slopes and a stepover that starts as parallel's. Their slopes are Fusion's,
the range asked for and nothing else, both ends included: a steep stretch is cut
down the part as a parallel pass is, and broken where it falls further than a cut
step (CC204). A spiral and radial spokes
turn about `centre = [x, y]` (the boundary's middle unless given) from
`inner_radius` to `outer_radius` (0, and all of the boundary); a spiral climbs
unless conventional and runs outside in unless `order = "inside-out"`; spokes
run from `angle_from` to `angle_to` degrees (0 to 360), `direction =
"both-ways"` unless `"outward"` or `"inward"`. A morphed spiral turns from the
boundary's shape into its `centre`, the point furthest inside unless given, its
laps no more than a stepover apart. Pocket clearing (`"pocket-clearing"`) is Fusion's 3D Pocket Clearing: each
level's rings a stepover apart in from its edges, cut from the middle out so the
ring along the walls is last, climbing unless `conventional = true`, with a level
on each flat face unless `flat_areas = false`. What
Fusion's Geometry tab holds is three settings, each read only by the strategies
`StrategySpec::uses` says: `selection`, the faces and edges picked on the
models; a 3D operation's `boundary`, which, as Fusion's machining boundary, keeps
it to the stock (the default), the part's bounding box or silhouette, or the
selection, with the tool's `containment` and an additional offset (`margin`);
and a 2D operation's `outline`, what it cuts round or clears. `heights` holds
the clearance (where the tool arrives and leaves, 10 mm over the stock), the
retract height (Fusion's, 5 mm over it) and the top and bottom it cuts between. `passes.stepdown` is
Fusion's multiple depths, shared the same way: adaptive and pocket clearing must have one,
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
under every picked loop. A pocket clearing or facing operation measures one from all its
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

A pocket has Fusion's **finishing passes** when asked for
(`passes.finishing`; off without it, as Fusion's): the clearing stops their
stepovers short of the walls, and once each level is cleared they go round the
walls with the side of the tool, the innermost first, the last at the walls.
`passes` says how many (one), `stepover` how far apart they are (a tenth of the
tool, Fusion's), `feed` how fast the last goes (the cutting feed), and
`repeat = true` cuts the last twice, for a tool that springs from the wall. The
pocket's leads (`linking.leads`, on unless turned off) take the tool onto the
first pass and off the last, level, in what is cleared, as Fusion's do with
*Leads on all finishing passes* off; between passes it stays down.

```toml
passes = { stepdown = 3.0, stepover = 2.5, finishing = { passes = 2, feed = 900.0 } }
```

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
tabs on them would leave pillars standing there (`camcraft_core::path::tabs` works on
any path's closed loops; `StrategySpec::uses` says which strategy has them). A
pocket or roughing operation given them keeps them, unused. They go evenly
round each loop cut at the operation's deepest level, about `spacing` apart and at
least two to a loop, clear of where the loop starts; a loop too short for one
gets none, with a warning. Each is `width` of material wide along the cut at its
top and `height` above the floor, and every cutting move over one, at any level,
rides over it: `shape = "rectangular"` (the default) steps straight up and
plunges down off it at the plunge feed, `"triangular"` climbs and ramps off at
45°. They are put on before linking, so links and ramps plan round them.

**Adaptive clearing** (`strategy = "adaptive"`) is Fusion's 3D Adaptive
Clearing ([decision 0007](decisions/0007-adaptive-clearing.md)): Z levels a
stepdown apart, each cleared by passes steered step by step to hold the tool's
engagement at the *optimal load*, which is its `passes.stepover` (a tenth of the
tool to start). Passes start beside the block where they can, and go down a helix
(`linking.helix_diameter`, Fusion's: 0.95 × the tool less its corners, no less
than a quarter of the tool) where they cannot; `direction` is
`"climb"` (the default), `"conventional"` or `"both-ways"`; `flat_areas` (on by
default) adds a level on each flat face, raised by the stock to leave down. It
clears the stock's block as it starts: an operation before it is not yet taken
into account, so after one it cuts some air.

```toml
[[setup.operation]]
name = "adaptive"
strategy = "adaptive"
tool = "rough-6-flat"
passes = { stepover = 0.6, stepdown = 12.0, stock_to_leave = { radial = 0.5, axial = 0.5 } }
```

**Stock to leave** (`passes.stock_to_leave`) is Fusion's radial and axial
stock to leave, in millimetres: `radial` on the walls, `axial` on the floors,
and between the two on a slope. Positive leaves material for a later operation
to finish; negative cuts past the part, for a looser fit. Beside, it must stay
under the tool's radius; down, in 3D with a ball or bullnose, within its corner
radius (a flat end, or a 2D floor, may go any depth). Every operation has it,
and cuts with as much as its strategy uses: a drill none, a profile on the
boundary only its floor's. New adaptive and pocket clearing start at 0.5 mm each way, as
Fusion's roughing does; everything else at none, which is not written. The 3D
strategies grow the tool by it (`camcraft_core::cutter::allowance`); a 2D one moves its
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
room for any goes without, with a warning. A groove on the boundary has none.
A profile has them unless turned off, as Fusion's does: `leads = false`.

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
  part leaves no room for it, or for one three quarters, half or a quarter the
  size, the tool goes straight down instead.
- `"plunge"`: straight down at the plunge feed onto the vertical arc's top, as
  Fusion's 2D contour does without a ramp.
- `"ramp"`: the ring cut as one spiral, as Fusion cuts one: on along the
  lead-in once, the safe distance above the top; down along the ring, in the
  direction it cuts, to the first depth; once round there from where the ramp
  reached it; straight on down to the next depth, and so on; and off along the
  lead-out once, from where the last lap ends. Never up, nor off and on again,
  between depths.

Pocket clearing and a 2D pocket have an entry too, into the first ring of each
level, as Fusion's 3D and 2D Offset Roughing: `"helix"` (the default), down a
helix beside where the ring starts, then once round level; `"ramp"`, along the
ring in the direction it cuts; or `"plunge"`. Pocket clearing's helix is
Fusion's size, 0.95 × the tool less its corners and no less than a quarter of
the tool, made smaller where the part leaves no room, down to half the tool
less its corners; a 2D pocket's is 0.9 × the tool, down to a quarter of that.
Where no helix fits, the ring is ramped along instead, as Fusion's helix ramp
type allows, and only where that does not fit either does the tool go
straight down.

Adaptive clearing has an entry too, into what it cannot reach from the side:
`"helix"` (the default) or `"plunge"`, as Fusion's 3D Adaptive's ramp types. It
has no ring to ramp along, so `"ramp"` is refused.

Neither a helix nor a ramp round a ring goes down more in one lap than the
*most per lap* (`ramp_stepdown`, Fusion's maximum ramp stepdown): the
operation's stepdown, or the tool's diameter without one; as Fusion's, the
lesser of that and the ramp angle holds. With a helix or a plunge, from one
depth of a ring to the next the tool stays down: off the ring along the lead,
across to where the next depth's way in starts, and down it.

**Linking** is how the tool moves between cuts, after Fusion's
([decision 0004](decisions/0004-linking-and-entries.md), and for 2D
[decision 0006](decisions/0006-2d-linking-and-arcs.md)). It rapids down to the
*safe distance* (2 mm) above the material under it and feeds from there, and feeds
out of a cut only that far before rapiding. It arrives at the clearance height
above the first cut, and between cuts it goes across as Fusion's *retraction*
says: `"minimum"` (the default), the safe distance over the material under the
way across as the operation found it; `"full"`, the retract height, or higher
where the material is. A link no longer than the *stay-down*
distance is made without lifting, at the cutting feed; the distance is Fusion's
Maximum Stay-Down Distance for the strategy: 50 mm for parallel and contour;
twice the tool's diameter for scallop and flat; five times for spiral, radial,
morphed spiral, pocket clearing, adaptive clearing and the 2D strategies.
Parallel and the other finishing strategies ride over the part: between passes
side by side that run opposite ways, as zigzag passes do, by Fusion's default
*smooth* transition, a half turn out past their ends as wide as they are apart,
tangent to both (straight on along the shorter first, or after, where one ends
short of the other), wherever the part and the material leave it room and the
passes are no further apart than the tool is wide; otherwise by the straight
line between them, lifted wherever the part rises above it. Pocket clearing and
the 2D strategies go straight and level, and a profile down to its ring's next
depth. Adaptive clearing makes
its own links at the level, only straight across what it has cleared; it goes
down its own helix into what it cannot reach
from the side, and straight down everywhere else, into what it has cleared.
Parallel enters and leaves each pass along a vertical arc (the *lead radius*, a
tenth of the tool); pocket clearing and 2D pockets go down a helix into material
(their entry, above), and facing ramps along the pass it is about to cut, in
the direction it cuts. Helixes and ramps go down at the *ramp angle*, 2°, at the
*ramp feed*, 60% of the cutting feed, as Fusion's. After the last cut the tool goes up to
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
| CC208 | warn    | adaptive clearing left an area it had no room to go down into    |
| CC209 | warn    | a finishing strategy found nothing to finish                     |
| CC210 | warn    | a scallop's or morphed spiral's grid coarsened to its budget     |
| CC211 | note    | stretches too steep for a finishing pass left, for a contour pass |
| CC212 | warn    | points of a scallop's area not reached from its boundary         |
| CC213 | warn    | a morphed spiral's laps further apart than the stepover          |
| CC214 | note    | pieces of a morphed spiral cut in rings, too concave for one spiral |
| CC215 | note    | contour points not pulled onto the part, left clear of it         |
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

`post` writes a single finishing operation described with flags (`camcraft post --help`):
any 3D finishing strategy, `--strategy parallel` unless another is named as a job
names it (`scallop`, `contour`, `flat`, `spiral`, `radial`, `morphed-spiral`), each
with its own defaults, and a ball, flat or bullnose cutter (`--tool bullnose
--corner 0.5`); `--stepdown` sets contour's levels, 1 mm apart unless given.
It builds a one-operation job and goes through the same `run_setup` as `job` and
the app, so there is exactly one engine path. `crates/camcraft-cli/tests/one_path.rs`
parses every written program back into positions and compares them with the paths
`run_setup` produces in-process, move for move.

Exit codes: `0` clean, `2` ran with warnings worth reading (or, with `--force`, wrote
programs that failed a check), `1` a check failed (and, with `--out`, nothing was
written).

## Posts

A setup's `post` names a builtin profile (`masso`, `masso-inch`, `grbl`,
`centroid-avid` or `centroid-avid-inch`, from `crates/camcraft-post/profiles/`)
or a TOML profile file, found from the job's folder. A profile is a dialect:
header and footer, word
formats, units (`units = "inch"` writes `G20` and converts every length and
feed, as `masso-inch` does), line numbers, and whether drilling is posted as
canned cycles (`drill_cycles`, off in
every built-in profile until cycles have been dry-run on the machine). Golden files
in `crates/camcraft-post/tests/golden/` hold the output.

The Masso profiles write as Fusion's Masso post does: blocks numbered from `N10`
in fives, comments not, and numbers without their trailing zeros but with
their point (`Z2.2`, `I0.`, `F150.`), inches to four places.

The Centroid profiles are for CNC12 on an Avid CNC router (the EX control), and
write as Avid's own Fusion post for it does: `.tap` files, no line numbers, no
trailing zeros and no bare point (`Z2.2`, `I0`, `F1200`; feeds to a tenth in
inches), and comments in capitals holding only letters, digits and ` .,=_-`
(`comment_upper`, `comment_chars`). CNC12 runs Avid's tool change at `M6`, which
measures the tool on the tool height setter, and the first move down after it
takes up that length (`length_offset`: `G43 Z25 H3`).

A program starts and ends as Fusion's do. Before the tool change the spindle
stops and the tool goes straight up clear of everything (`safe_retract`: Masso's
`G28 G91 Z0`, GRBL's and Avid's `G53 G0 Z0`); the first move then goes across
at that height and down to where the path starts, never diagonally from wherever
the change left the tool. A profile without a safe retract rises (or falls) to the path's crossing
height first and goes across there. After the last cut the spindle stops, the tool
goes up again, and the machine goes home across (`end_home`: Masso's `G28 G91 X0
Y0`; Avid's `G0 X0 Y0`, over the work origin, where Fusion's Avid post ends; GRBL's
stored `G28` position may be anywhere, so it has none).

A path's arcs are arcs in the program, not strings of short moves: `G2` or `G3`
in the plane they turn in (`G17` level, `G18` or `G19` for an arc in an upright
plane, such as a lead), a helix with the third axis moving, the plane put back to
`G17` before the spindle stops. The engine makes an arc only where a three-axis
machine can cut one, about X, Y or Z; elsewhere, as a lead on a pass running
across the part at an angle, it makes short straight moves within the tolerance,
so the program is always the path that was simulated. With centre offsets
(`arc_style = "ij"`) no block turns more than half a turn, and with a radius
(`"r"`) no more than a quarter, whose sign would otherwise have to say which way
round. An arc shorter than 0.25 mm across, whose ends could round to the same
point and read as a full circle, is written as straight moves within 0.002 mm
of it, as Fusion's posts write one (`minimumChordLength`, `tolerance`).

A program is checked by what it does, not by what it says.
`camcraft_post::readback` reads one back as a controller would (units, distance
mode, arcs by centre or radius in any plane, helices, drilling cycles, `G28`,
`G53`, length offsets, tool changes), refusing anything it does not understand,
and replays it against the path it was posted from: every move reached in
order, no cut made a rapid, feeds as asked, arcs on the same curve turning the
same way. It holds the program to its safety too: never across from a height
it does not know, nor across and down at once; the spindle stopped for the
tool change; the right tool, work offset, length offset, spindle and coolant
in force before the first cut; after the last cut only up, and across only
once clear. The tests read back every built-in profile, set every way it can
be, in both units, over the golden fixture, hundreds of arcs and drilled
holes, and the real paths of the example job and the lego contour; and
programs broken on purpose, one fault each, to show each fault is caught. To
read back a job of your own, every setup for every built-in post:

```sh
CAMCRAFT_READBACK_JOB=path/to/job.camcraft cargo test -p camcraft-session readback_job
```
