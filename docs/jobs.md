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
pocket, facing, bore, circular or drilling), a tool, heights, passes and linking. Scallop (`"scallop"`) is Fusion's: passes a stepover apart
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
profiles, pockets and facing cut the bottom in one pass without (and a new one
starts so, as Fusion's 2D Contour, 2D Pocket and Face start with Multiple Depths
off), a circular starts with them on at a millimetre, as Fusion's does, and
parallel, bore and drilling never read it. An operation may be
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
picks; a clearance never is, nor a top or bottom of an operation whose picks
are holes (drilling, bore, circular), and nothing picked is refused. A profile
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

**Bore and circular** are Fusion's Bore and Circular
([decision 0022](decisions/0022-bore-and-circular.md)): round holes and bosses,
picked by their walls (`selection.faces`), as Fusion's are. A face whose edges
are circles about one upright axis, all one size, is a straight round wall; a
tapered one is refused, as is one lying over. The faces of one wall, as a file
splits a cylinder into halves, are one wall, from the highest of them to the
lowest. Which side the material is on says whether it is a hole, cut from
inside, or a boss, cut round from outside. Each is cut from its wall's top down
to its bottom, as Fusion's go from the hole's top to its bottom, unless a Top
or Bottom is set, which then goes for every one; the floor is raised by the
axial stock to leave, and the tool's centre is its radius and the radial stock
to leave off the wall. A hole no wider than that is refused, saying so: drill
it, or use a smaller tool. Holes are cut from the first picked, then always the
nearest.

`multiple_passes` (off) is Fusion's Multiple Passes: `stepovers` (2) passes out
to the wall, `passes.stepover` apart (half the tool to start), the last at the
wall; from the middle out in a hole, from outside in round a boss. Off, the
wall alone. A hole's pass that would be past its middle is put half the tool's
radius from it, once, so the passes stop there. A hole the passes do not reach
the middle of is left with a core standing in it, as Fusion's is, and the run
says so (CC218): through the part, it comes loose. Both climb unless
`conventional = true`; climbing with a clockwise spindle goes anticlockwise
round a hole and clockwise round a boss. `repeat = true` (Fusion's Repeat
Passes) cuts the last pass twice. Each pass comes onto its circle along the
leads (`linking.leads`, on unless turned off: a straight, a twentieth of the
tool to start, then a level arc a tenth of it, tangent to the circle) and goes
off along them, in the hole, made smaller where the hole has no room for them
and left out where it has none even for a quarter of them; round a boss,
outside it. `lead_to_center = true` leads on from the hole's centre and back to
it instead, a half turn each way. Down onto the leads and up off them is the
linker's vertical arc, the leads' `vertical`.

A bore goes down a helix round each circle from the top to the bottom, at
`ramp_angle` (2°) from level, each circle's turn as far down as the angle takes
it round, or with `use_ramp_angle = false` at `pitch` millimetres a turn (1), as
a new Bore of Fusion's starts; then once round level at the bottom, and off
along the leads. A helix that would fall more steeply than a plunge round a
small circle is refused. It has no multiple depths: each pass is the helix all
the way down, and between passes it lifts out and comes down to the top again.
Its finishing pass (`passes.finishing`, Fusion's Finishing Passes) is one, at
the cutting feed: the passes before it stop its `stepover` (a tenth of the
tool) short of the wall. Its `passes`, `feed` and `repeat` are a pocket's, and
not read.

A circular cuts each circle level by level, a stepdown apart (Fusion's Multiple
Depths, on to start, a millimetre): each level a whole stepdown and the last
what is left, as Fusion's, or evenly with `even_stepdowns = true`. At each
level each pass comes on along its lead, goes once round and leads off; the
first goes down into the material along its circle, at the ramp angle and no
more than the most per lap, as Fusion's Circular ramps on, and the passes after
it at the level stay down between. A repeat cuts the wall's pass twice at the
last level.

Every cut of either is a true arc or helix about the hole's axis, posted as
`G2` or `G3`. Neither has smoothing, feed optimisation or a shaft and holder
mode of its own; the checks still sweep the whole tool.

```toml
[[setup.operation]]
name = "bore"
strategy = "bore"             # or "circular"
tool = "flat-6"
multiple_passes = true
stepovers = 3
selection = { faces = [{ model = "part", index = 12, at = [37.0, 10.0, 16.0] }] }
passes = { stepover = 3.0 }
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
engagement at the *optimal load*, which is its `passes.stepover`
(`tool_diameter * 0.4` to start, as Fusion's). Passes start beside the block where they can, and go down a helix
(`linking.helix_diameter`, Fusion's: 0.95 × the tool less its corners, no less
than a quarter of the tool) where they cannot; `direction` is
`"climb"` (the default), `"conventional"` or `"both-ways"`; `flat_areas` (on by
default) adds a level on each flat face and on each peak, raised by the stock to
leave down.
Where the straight way between two passes runs through what is still standing, it
stays down round it, through what is cleared, if that is no longer than the
stay-down distance. As Fusion's 3D Adaptive:

- `machine_cavities` (on by default): off, it only cuts from the outside in, and
  leaves the areas it could only go down into, saying how much it left.
- `min_cutting_radius` (none by default): the tightest its centre turns in an
  inside corner. Every sharper corner is blended, leaving material in it for a
  smaller tool, and nothing narrower than the tool and twice this is cut.
- `order`: `"by-depth"` (the default), each level everywhere before the next
  down, or `"by-area"`, each area a level splits into, such as a pocket, cleared
  all the way down before the next.
- `rest_machining` (off by default): it clears only what the operations before
  it in the setup left, each swept exactly with the tool it was cut with, so it
  waits for them; `ignore_stock_less_than` lets be what they left thinner than
  that. Off, it clears the stock's block as it starts, so after another
  operation it cuts air where that one cleared.
- `min_flat_stepdown` (none by default): with flat areas, a flat face or a peak
  less than this below the level above gets no level of its own, and that much
  is left on it.
- `min_axial_engagement` (none by default): a fine or shallow level cutting less
  depth than this is left out. Every level between two a stepdown apart cuts
  into material standing up to the upper one, so this is how far below it it is.
- `shallow_areas` (off by default): where the terrace two levels leave between
  them is wider than `max_shallow_stepover` (2 mm), as the tool's centre goes, a
  level is added halfway, then halfway again, until none is, or levels would
  come closer than `min_shallow_stepdown` (0.5 mm). Each added level cuts only
  where its terrace is that wide.
- `slot_clearing` (off by default): a pocket gone down into is first cut a slot
  along its longest way through its middle, `slot_width` wide (the tool's width
  when 0, and never narrower), then cleared out from it.
- `stay_down_level` (100 by default), in percent: how far round what stands
  between a link stays down. 0 stays down only going straight; 100 goes round
  as far as the stay-down distance allows.
- `stay_down_clearance` (0 by default): how far a link staying down keeps off the
  material, but where it leaves one pass and comes to the next.
- `lift_height` (0 by default): how far a link staying down lifts off the level,
  up and down again no steeper than a link may be, and no higher than the
  level above.

```toml
[[setup.operation]]
name = "adaptive"
strategy = "adaptive"
tool = "rough-6-flat"
passes = { stepover = 0.6, stepdown = 12.0, stock_to_leave = { radial = 0.5, axial = 0.5 } }

[[setup.operation]]
name = "rest"
strategy = "adaptive"
tool = "rough-3-flat"
rest_machining = true
ignore_stock_less_than = 0.2
min_cutting_radius = 0.5
order = "by-area"
shallow_areas = true
slot_clearing = true
lift_height = 0.5
passes = { stepover = 0.3, stepdown = 6.0, stock_to_leave = { radial = 0.5, axial = 0.5 } }
```

**Stock to leave** (`passes.stock_to_leave`) is Fusion's radial and axial
stock to leave, in millimetres: `radial` on the walls, `axial` on the floors,
and between the two on a slope. Positive leaves material for a later operation
to finish; negative cuts past the part, for a looser fit. Beside, it must stay
under the tool's radius; down, in 3D with a ball or bullnose, within its corner
radius (a flat end, or a 2D floor, may go any depth). Every operation has it,
and cuts with as much as its strategy uses: a drill none, a bore and a circular
beside the wall and under the floor, a profile on the
boundary only its floor's. New adaptive, pocket clearing and 2D pockets start at 0.5 mm
each way, as Fusion's roughing, its 2D Pocket among it, does; everything else at
none, which is not written. The 3D
strategies grow the tool by it (`camcraft_core::cutter::allowance`); a 2D one moves its
cut out by it and raises its floors by it. The material is cut by the real
tool, so what is left shows in the simulation, and what was cut past the part
on purpose is not called a gouge.

```toml
passes = { stock_to_leave = { radial = 0.5, axial = 0.25 } }
```

**Smoothing** (`passes.smoothing`, off by default) is Fusion's Smoothing: the
passes' runs of short moves are fitted with arcs, where they lie on a circle in
a plane a three-axis machine cuts arcs in, and with longer lines, none further
than `passes.smoothing_tolerance` (the job's `tolerance` when not given) from
the moves it stands for ([decision 0015](decisions/0015-smoothing.md)). Every
strategy but drilling, bore and circular, whose moves are arcs already, has it. The passes are fitted before they are linked, so
what is simulated and checked is what is written, and the setup is verified to
the job's tolerance and the largest smoothing tolerance together. On a serving
tray, adaptive's program went from 1,307,065 lines to 181,294 and scallop's from
193,295 to 17,260.

```toml
passes = { smoothing = true, smoothing_tolerance = 0.01 }
```

Helices are fitted too, about Z: a ramp round a ring is a few `G2`s or `G3`s
falling as they turn.

**Feed optimisation** (`passes.feed_optimization`, off by default) is Fusion's
Feed Optimization: cuts are slowed into and out of their sharp corners and round
their tight arcs, where the tool is wrapped in more material than along the
walls ([decision 0016](decisions/0016-feed-optimisation.md)). A corner turns by
more than `max_directional_change` (degrees, 25); an arc tighter than
`reduced_feed_radius` is slowed along; the cut slows `reduced_feed_distance`
before each and speeds up as far after; and it slows to `reduced_feed` (mm/min).
The radius and distance are a twentieth and a fifth of the tool when not given,
as Fusion's, the feed half the cutting feed. With `only_inner_corners` (on unless turned off), only the corners
turning into the material are slowed, judged from which side of the passes the
material is on: where they cut both ways, every corner is. Every strategy but
drilling, bore, circular and adaptive clearing has it; adaptive holds its load
at every step.

```toml
passes = { feed_optimization = { reduced_feed = 600, reduced_feed_distance = 2 } }
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

- `"helix"`, what a job file that leaves it out has: down a helix in the waste beside where the lead
  starts (or the ring, without one), at the ramp angle, onto the vertical arc's
  top, then once round level, as Fusion's helix ends, so its circle is cut to
  the depth all round and the next depth's way in cuts nothing on the way. Its circle, as the
  tool's centre goes, is `helix_diameter` across (0.9 × the tool's); where the
  part leaves no room for it, or for one three quarters, half or a quarter the
  size, the tool goes straight down instead.
- `"plunge"`: straight down at the plunge feed onto the vertical arc's top, as
  Fusion's 2D contour does without a ramp. A new profile starts with it, as
  Fusion's 2D Contour starts with Ramp off.
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
less its corners; a 2D pocket's is the same, as Fusion's.
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
morphed spiral, pocket clearing, adaptive clearing and the 2D strategies but
bore, which lifts between its passes.
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
*ramp feed*: a new operation's is its plunge feed, a third of the cutting feed,
as a new tool of Fusion's has them; one a job file leaves out is 60% of the
cutting feed. After the last cut the tool goes up to
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

### Expressions

An operation's setting can be worked out from an expression, as Fusion's
parameter expressions are, under `expressions` with the setting's place in the
file as its key:

```toml
[setup.operation.expressions]
"passes.stepover" = "tool_diameter * 0.4"
"passes.stepdown" = "Math.min(tool_fluteLength * 0.75; tool_diameter * 2.5)"
```

The setting still holds the number, so the file reads the same without them;
it is worked out again when the job is read and whenever what it reads
changes, a new tool above all. The names are Fusion's: `tool_diameter`,
`tool_cornerRadius`, `tool_fluteLength` (two diameters where the tool gives
none), `tool_shoulderLength`, `tool_shaftDiameter`, `tool_overallLength`,
`tool_taperAngle`, `tool_numberOfFlutes`, `tool_type` (`'flat end mill'`,
`'ball end mill'`, `'bull nose end mill'`, `'chamfer mill'`, `'drill'`),
`tool_spindleSpeed`, `tool_feedCutting`, `tool_feedPlunge`, `tolerance`,
`stepover`, `optimalLoad`, `maximumStepdown`, `stockToLeave`,
`axialStockToLeave` and `safeDistance`. Numbers take a length unit (`2mm`,
`0.1in`, `1cm`); one without is millimetres, or the setting's own unit. There
are `+ - * / ^`, comparisons, `&& || !`, `a ? b : c`, and Fusion's `Math.min`,
`Math.max`, `Math.abs`, `Math.sqrt`, `Math.floor`, `Math.ceil`, `Math.round`,
`Math.floorto`, `Math.ceilto`, `Math.roundto` and the trigonometric functions,
their arguments split by `;` or `,`. Expressions read the settings as they
stand, and are worked out in turns until none changes, so one may read what
another sets. An expression that cannot be worked out stops its setup from
running, and says why.

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

## Shank, holder and machine

The whole tool is checked, not only what cuts
([decision 0021](decisions/0021-shank-and-holder.md)). From the tip up it is the
cutter, a neck at the cutting diameter to `shoulder_length`, the shank
(`shank_diameter`) to `stickout`, how far it comes out of what holds it, and
then that: the tool's own `holder`, segments from its face up, each a cylinder
or a taper:

```toml
[tools.rough.holder]
name = "ER20 nut"
segments = [
  { length = 19.5, diameter = 35.0 },
  { length = 10.0, lower_diameter = 35.0, upper_diameter = 42.0 },
]
```

A tool without a holder is held by its setup's machine's spindle nose, where the
setup names a machine. The job keeps its machines as it keeps its tools, a copy
of what the setup was set up with:

```toml
[machines."80 mm ER20 spindle"]
spindle = "ER20"
nose = [
  { length = 19.5, diameter = 35.0 },
  { length = 6.5, diameter = 35.0 },
  { length = 15.0, diameter = 78.0 },
  { length = 160.0, diameter = 80.0 },
]

[[setup]]
name = "top"
machine = "80 mm ER20 spindle"
```

`holders = true` marks a machine whose tools sit in holders of their own (ISO30,
BT30, ISO20): its nose is not drawn. The built-in machines are an 80 mm ER20
spindle (its nose from the GDK80's drawing) and ISO30, BT30 and ISO20 ATC
spindles; the built-in holders are the ER11 to ER32 nuts, the BT30 ER25 and
ER32 holders, and ISO30 ER16 to ER32 holders, their 50 mm flange drawn from a
maker's drawing. What a tool
leaves out (flute length, stickout, shank diameter) is assumed and said
(CC216).

**Checking** sweeps everything above the flutes through the material, part and
stock as they stand, alongside every move, cutting nothing: where it would meet
material is CC108, a failure, with the deepest it went and how far out of the
holder the tool would have to be to clear. Contact is actual, not by a
clearance, and the body is taken twice the operation's tolerance (the job's,
and smoothing's where it smooths) narrower: the wall one pass left and the tool
on another are each only known to that.

**Avoiding** is each operation's, `passes.holder`, as Fusion's Shaft and
Holder:

```toml
passes = { holder = { mode = "trim", shank_clearance = 1.0, holder_clearance = 5.0 } }
```

`mode` is `off` (the default, as Fusion's in 3-axis: the check says where),
`pull-away`, `trim` or `fail`; the clearances are how near the shank and the
holder may come, Fusion's 1 and 5 mm. A finish over the part (parallel,
scallop, spiral, radial, morphed spiral) drops the rest of the tool onto the
part and the material as one tool, and lifts the pass clear or cuts it out
there. Contour and flat, at fixed heights, trim. Adaptive and pocket clearing
shrink what each level may clear by where the rest of the tool would meet the
part above it, and trim where the levels above left material it would meet.
Drilling skips a hole it cannot reach. Links lift over the material by the rest
of the tool too, and stay down between passes only where it keeps clear. `fail` stops the operation at the first place it would meet
anything.

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
| CC108 | fail    | simulator: the shank, holder or spindle nose met the material    |
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
| CC216 | note    | the shank and holder were checked with an assumed length or size |
| CC217 | note    | no holder or spindle nose to check: only the shank was           |
| CC218 | warn    | a bore or circular left a core standing in a hole                |
| CC299 | warn    | a warning from reading the model                                 |

A **plunge** is a continuous descent steeper than `PLUNGE_RATIO` (10:1) that falls
further than `max_cut_step`, measured over the whole run of moves, not move by
move. Finishing breaks its passes there, and the gate refuses any that remain; the
checks and the app measure it the same way. Adaptive clearing has no
`max_cut_step` of its own, as Fusion's has none: its passes stay on their levels,
so it is not shown and not read, and its links and the check use the default,
0.5 mm.

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

`compare` holds camcraft's programs to another CAM system's, operation by
operation, and writes nothing:

```sh
camcraft compare job.camcraft --setup top face=fusion/face.nc drill=fusion/drill.nc
```

It runs the setup, then cuts each other program from the material camcraft had
before that operation, with the operation's tool, and prints what each program
does as read back (moves, arcs, distance fed and at rapid, minutes feeding,
plunges, the lowest it goes), how much each removed and left on the part, the
gouges the run's own check finds in each, and how the material differs, column
by column, with a map of where. The other program is read as the Fanuc-style
controllers Fusion's posts write for (Masso, Haas, Centroid) read it.
`crates/camcraft-session/src/compare/tests.rs` holds Face, 2D Contour, 2D Pocket
and Drill to Fusion's own programs for a part made in Fusion
(`fixtures/nc/ops_block_*.nc`): set as Fusion's are, each leaves the same
material to 0.01 mm.

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
`G53`, length offsets, tool changes), by RS274/NGC, where `G80` leaves no motion
in force, so camcraft's programs run on controllers of either convention (another
system's is read by the Fanuc one, where the motion before a cycle returns after
it, as Fusion's posts for Masso write), refusing anything it does not understand,
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
