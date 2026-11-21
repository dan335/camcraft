# Progress

camcraft 0.11.0, updated 2026-11-21.

What camcraft does and what it has yet to do: each of its 3-axis milling
operations, and the settings, tools, setups and posts a router job uses; then
what comes next, what the simulator cannot check, and the open questions.
[CHANGELOG.md](CHANGELOG.md) says what each release brought.

> [!WARNING]
> **Nothing has been cut on a machine yet.** Every row here is done in the
> software and checked against a simulated part, nothing more. See
> [Before the first cut](#before-the-first-cut).

- **done**: does what it is for, with the settings a 3-axis router job uses.
- **partial**: works, without what is listed beside it.
- **not started**: not in camcraft.

*Tested on* a part means a test runs the operation, as a new one starts and in
the cases named, on one of the parts in [fixtures/step](fixtures/step/README.md)
(`ops_block`, `ops_2d`, `ops_engrave`, `ops_3d`), and holds it to camcraft's own
results: every check passes, nothing gouges, and the lifts, the length fed and
the simulated stock left are as camcraft's own run recorded them. Some
operations are [not planned](#not-planned).

| Area                                          | Done | Partial | Not started |
|-----------------------------------------------|-----:|--------:|------------:|
| [2D operations](#2d-operations)               |    1 |      10 |           0 |
| [3D roughing](#3d-roughing)                   |    0 |       2 |           0 |
| [3D finishing](#3d-finishing)                 |    0 |      11 |           0 |
| [Drilling](#drilling)                         |    4 |       1 |           2 |
| [Shared settings](#shared-settings)           |   10 |       3 |           4 |
| [Linking and entries](#linking-and-entries)   |    4 |       1 |           1 |
| [Models](#models)                             |    3 |       2 |           1 |
| [Setups and stock](#setups-and-stock)         |    4 |       2 |           5 |
| [Tools](#tools)                               |    8 |       1 |           5 |
| [Simulation and checks](#simulation-and-checks) |  7 |     0 |           1 |
| [Posts and programs](#posts-and-programs)     |    6 |       0 |           2 |
| [App](#app)                                   |    7 |       1 |           0 |
| [Project](#project)                           |    1 |       0 |           4 |
| **All**                                       |   55 |      34 |          25 |

`grep "| not started" PROGRESS.md` lists everything not yet begun.

## What is next

Roughly in the order they matter:

1. **Cut on a machine.** See [Before the first cut](#before-the-first-cut).
2. **Fixtures on the setup**: clamps and jigs as a setup's own, planned on
   their own, rather than as surfaces each operation avoids.
3. **3D finishing's links past a face avoided**: the pieces a face avoided
   cuts are now plunged onto, and taken the nearest next by Parallel and
   Spiral
   ([decision 0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)),
   but most of what still lifts is a link that would rest on the face
   avoided, which could turn back at its clearance and stay down instead; and
   Scallop's lifts between the pieces its outer boundary cuts a ring into
   ([decision 0035](docs/decisions/0035-scallop-over-every-slope.md)); and
   its blended rings nearest where the tool leaves the part, which still bob
   a tenth of a millimetre every few millimetres
   ([decision 0038](docs/decisions/0038-scallop-rings-blend-into-an-edge-at-a-slant.md));
   and the rings climbing an off-centre pocket's rim, as before zones, where
   the zones round it cannot all close their rings, until the field can say
   where two fronts meet inside a cliff's edge ([decision 0039](docs/decisions/0039-scallop-zones-at-a-sharp-rim.md)).
4. **Surface groups for roughing**: deciding what adaptive and pocket clearing
   should do with a face machined or avoided, then doing it.
5. **A volumetric stock**, so what a top setup leaves carries to a flipped
   bottom one.
6. **Exact slicing of the offset surface** for Z-level work
   ([decision 0003](docs/decisions/0003-grids-only-for-stock.md)).
7. **Exact circles from the B-rep**, rather than through the tessellation's
   points.
8. The work offset manager in the toolbar; JSON tool libraries, which carry
   holders; tapered ball mills.

## 2D operations

| Feature              | Status      | Notes |
|----------------------|-------------|-------|
| Face                 | done        | Tested on `ops_block`. Direction, pass extension, stock offset, multiple depths. |
| 2D Contour           | partial     | Tested on `ops_block`. Outside, inside or on; tabs; leads; helix, plunge or ramp in; multiple depths; rings follow round edges as arcs ([0005](docs/decisions/0005-2d-profile-and-pocket.md), [0006](docs/decisions/0006-2d-linking-and-arcs.md)). Missing: sideways roughing passes, finishing passes, picked entry points. |
| 2D Pocket            | partial     | Tested on `ops_block`. Finishing passes with leads; helix, ramp or plunge in. Missing: leads on its roughing rings, picked entry (predrill) points. |
| 2D Adaptive Clearing | partial     | Tested on `ops_2d`'s pocket with an island and its notch open to two sides, the stock flush with the part there so the notch is open to air. A 2D pocket's areas, picked or the silhouette, each cleared by 3D adaptive's engine at the pocket's levels: to each floor in one, or multiple depths, whole or even; islands cut round; open pockets found where the outline runs along the part's edge, and entered from outside the stock; optimal load, minimum cutting radius, slot clearing, direction, order by depth or area, helix or plunge in, stay-down level, clearance and lift ([0024](docs/decisions/0024-chamfer-and-2d-adaptive.md)). Missing: open pockets' extension methods (tangent, closest boundary, parallel) and stock contours, rest machining from a previous tool's size, predrill and entry positions, ramp clearance height and taper, both ways' other-way load and feed, no-engagement feed, separate lead radii, sketches. |
| Slot                 | partial     | Tested on `ops_2d`'s two slots with a tool narrower than them and one as wide. Closed slots picked by the edges round their floors, or their floors: straight, curved or several of either; the middle only, a narrower tool leaving the walls; ramped from the ramp clearance height back and forth along the whole middle, each leg no more than half the maximum ramp stepdown, then once along the floor; multiple depths; repeat finishing pass, backoff, preserve order ([0023](docs/decisions/0023-slot-and-trace.md)). Missing: open and semi-open slots and tangential extension, ramp types (predrill, plunge), picked entry positions, sketches. |
| Trace                | partial     | Tested on `ops_2d`'s two V grooves, on the curve. Picked edges followed, open or closed, joined end to end, a click taking the smooth chain; on the curve or beside it left or right; axial offset, pass extension, maximum angle, multiple depths with number of stepdowns, by depth or by curve, both ways, repeat, preserve order, keep tool down, vertical lead-in and lead-out ([0023](docs/decisions/0023-slot-and-trace.md)). Missing: chamfer (the checks can now hold a chamfer cut on purpose, as a 2D chamfer's), chamfer tip offset, up/down milling, multiple passes, spiral stepdowns, separate lead-out radius, flipping a chain's direction, tool orientation, sketches. |
| Thread               | partial     | Tested on `ops_2d`'s through and blind holes, as a new one and with a pitch diameter offset of a millimetre, the holes bored first. Holes picked by their walls, threaded with a single-profile thread mill from each wall's bottom to its top: down the centre, a straight and a helical arc onto the thread, the helix at the pitch in half turns, the same off and back to the centre; pitch (the tool's), pitch diameter offset, right or left hand, climb or conventional, multiple passes, helical or level leads, lead to center. The thread cut into the wall is held by the checks as cut on purpose, and anything deeper is a gouge; a helical lead that reaches into a blind hole's floor is said (CC224) and fails the part check ([0026](docs/decisions/0026-thread.md)). Missing: outside threads (bosses), multi-tooth thread mills, start angle, multiple threads, repeat passes, stock to leave, compensation in control, threads from a standard or the model, diameter range and same-diameter selection, hole order choices. |
| Bore                 | partial     | Holes and bosses picked by their walls, from each wall's top to its bottom; pitch or ramp angle, stepovers, finishing passes, repeat, leads and lead to center, climb or conventional ([0022](docs/decisions/0022-bore-and-circular.md)). Tested on `ops_2d`. Missing: tapered walls, diameter range and same-diameter selection, hole order choices, compensation in control, bottom-to-top. |
| Circular             | partial     | Holes and bosses picked by their walls, cut level by level, whole or even stepdowns, each pass down its lead and lifted out after; stepovers, repeat, leads and lead to center, climb or conventional ([0022](docs/decisions/0022-bore-and-circular.md)). Tested on `ops_2d`. Missing: diameter range and same-diameter selection, hole order choices, compensation in control. |
| Engrave              | partial     | Tested on `ops_engrave`, a plate with a triangle, a stroke and a rectangle sunk into it, the three together and each alone. Shapes picked by their rims or floors, islands too; the V's flank on the outline, round a wide shape at the depth, along a narrow one's middle, up into corners sharper than the sharp corner angle; depth (0.95 of the flank to start), flat tips, multiple depths; a shape sunk less deep than the V goes is said (CC223) and fails the part check ([0025](docs/decisions/0025-engrave.md)). Missing: Top and Bottom from other references, keeping the tool down between shapes, an island far from its outline joined to it in one pass, entry positions, sketches and text, polar and wrap. |
| 2D Chamfer           | partial     | Tested on `ops_2d`'s chamfered rim, picked by its lower loop and by its upper, and a sharp rim a millimetre wide, each on the pocket cleared first. Picked edges, or every edge of picked faces, chamfered with a chamfer mill or V-bit: a sharp edge to the chamfer width asked, a chamfer the model has followed from its lower edge; tip offset, clearance (the tool lifted off the part's other faces), finishing overlap, preserve order; the side of the part found from the part; the chamfer asked is held by the checks as cut on purpose, and anything deeper is a gouge ([0024](docs/decisions/0024-chamfer-and-2d-adaptive.md)). Missing: outer corner modes other than roll, lead-in and lead-out settings (always a straight a tenth of the tool long), compensation in control, tangential extension of open chains, sketches. |

## 3D roughing

| Feature          | Status      | Notes |
|------------------|-------------|-------|
| Adaptive Clearing | partial    | Optimal load, fine stepdown, flat and shallow areas, minimum cutting radius, machine cavities, slot clearing, order by area, stay-down level, clearance and lift, rest machining from earlier operations, shaft and holder ([0007](docs/decisions/0007-adaptive-clearing.md)). Even slabs, fine levels even up to a flat; by area, each slab's level then its fine levels up the wall, area by area; a pass going round on the spot ends and material too narrow for the load is taken along it ([#1](https://github.com/dan335/camcraft/issues/1)); helixes no smaller than half the tool less its corners, nor a quarter of the tool, a ramp along the area's middle where none fits ([0036](docs/decisions/0036-adaptive-levels-order-and-entries.md)). Links stay down across what is cleared wherever lifting would go further, rise to a slab's fine levels at the feed, and a slab's level starts nearest where the tool is; by area, each area is come to where the tool last was in it. Missing: rest machining from the setup's stock or a file, predrill points, minimum retraction over the material as it stands rather than as the operation found it. |
| Pocket Clearing  | partial     | Levels cut from the middle out, climb or conventional, flat areas, rest material on walls, helix, ramp or plunge in ([0008](docs/decisions/0008-pocket-clearing.md)). Missing: corner smoothing, a finishing pass. Its time grows with the part's triangles ([open questions](#open-questions)). |

## 3D finishing

| Feature           | Status      | Notes |
|-------------------|-------------|-------|
| Parallel          | partial     | Tested on `ops_3d` as a new one starts, with the dome avoided 1 mm each way and with no clearance, with only the pocket's floor machined, with the floor machined with 0.5 mm of its own and with the top avoided 2 mm down. Passes at any angle, steep ground left to rings round the walls, which take down the steps the passes would leave there ([0014](docs/decisions/0014-walls-for-every-finish-over-the-part.md)), the nearest next from either end, entered straight down with no ramp ([0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)); surface groups ([0033](docs/decisions/0033-surface-groups.md)). Missing: a slope range, passes cut one way, a lead through the roughing's stock (it plunges onto the pass where the lead's top is in material), links that stay down past a face avoided (most of what still lifts). |
| Scallop           | partial     | Tested on `ops_3d` as a new one starts, with the dome avoided 1 mm each way and with only the pocket's floor machined. Rings a constant distance apart along the surface, slopes, both directions and orders ([0009](docs/decisions/0009-scallop-distance-field.md)); every slope cut by the passes themselves, walls included, each point found round a wall at its height, and level rings only below where the tool rests from above, down to the Bottom; rings laid anew round faces avoided, inside out measured from the inner edges, one straight seam, the field's own order ([0035](docs/decisions/0035-scallop-over-every-slope.md)); the field's grid cut where the ground machined ends, so its rings run down to where the tool leaves the part from above, found exactly, the last along it, with level rings from a stepover below its highest point, and up an upright wall above a face avoided from the wall's foot ([0037](docs/decisions/0037-scallop-to-where-the-tool-leaves-the-part.md)); where the rings meet that line at a slant, closed rings that blend from the last whole ring into it, and below it closed rings down to the first level ring, so no ring there ends in arcs, that line's height rising and falling no faster than a ramp, and a ring under the last down a steep wall ramped onto along it ([0038](docs/decisions/0038-scallop-rings-blend-into-an-edge-at-a-slant.md)); a sharp rim a break between zones, inside out and outside in, the rings stopping where the tool starts rolling over its edge and starting again past it, each border a ring, so none climbs over the edge and back, where the zones close every ring, the ground cut as one where they cannot ([0039](docs/decisions/0039-scallop-zones-at-a-sharp-rim.md)); each region's rings round its outside cut before what is inside them and its rings round its holes after, from the boundary in, and the other way from the middle out, so the two families are not cut by turns, and from the boundary in the field's first ring started where the level rings below it end, the tool stepping onto it staying down ([0041](docs/decisions/0041-scallop-hole-rings-after-what-is-inside.md)); inside the last ring going in with no corner, rings round a spine midway between its sides, blended into it, out from it inside out and in to it outside in, where offsets came to points ([0040](docs/decisions/0040-scallop-spine-inside-the-last-smooth-ring.md)); a band's rings counted by their real spacing, each distance's gradient taken as unit across a ridge of the field ([0038](docs/decisions/0038-scallop-rings-blend-into-an-edge-at-a-slant.md), amended); entered straight down with no ramp ([0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)); surface groups ([0033](docs/decisions/0033-surface-groups.md)). Missing: a limit on stepovers, rest machining, a contact point boundary, turning back at a face avoided and staying down, staying down between the pieces the outer boundary cuts a ring into, and the blended rings nearest where the tool leaves the part still turning in height a tenth of a millimetre every few millimetres (decision 0038); on a block with an off-centre pocket, the rings climbing its rim, the ground cut as one there (decision 0039); the band into the last smooth ring bunching where that ring is nearest the spine (decision 0040). |
| Contour           | partial     | Tested on `ops_3d` as a new one starts and with the dome avoided 1 mm each way. Waterline levels, ramped down the wall from level to level, by depth or by area, slopes ([0010](docs/decisions/0010-contour-and-flat.md)); surface groups ([0033](docs/decisions/0033-surface-groups.md)). A new one keeps the tool outside the silhouette, so it goes round the block's outside walls down to Z0 as well as round the pocket. A ring at a floor where the tool touches the floor and an avoided face's clearance at once is kept, taking the roughing's half millimetre off the floor round the dome. Missing: helix entries. |
| Flat              | partial     | Tested on `ops_3d` with the 6 mm flat end mill as a new one starts and with the dome avoided 1 mm each way. Each flat face cleared at its own height ([0010](docs/decisions/0010-contour-and-flat.md)); only the flats it machines, kept clear of faces avoided ([0033](docs/decisions/0033-surface-groups.md)); a new one comes straight down onto each ring, a ramp along the ring a choice ([0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)). Missing: multiple depths on a flat, lead-ins, telling a tessellated curve from a flat (it clears the dome's top at Z15 as one), skipping slivers (beside the dome avoided it clears slivers a tenth of a millimetre round, plunging into the 5.5 mm the roughing left). |
| Spiral            | partial     | Tested on `ops_3d` as a new one starts and with the dome avoided 1 mm each way; there a new one laps 0.345 mm apart about the part's middle, with rings round the walls. Centre, radii, slopes, both directions and orders ([0011](docs/decisions/0011-patterns-dropped-onto-the-part.md)); a new one both ways, from its outer radius in, the pieces the part cuts it into the nearest next from their nearer ends, entered straight down with no ramp ([0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)); surface groups ([0033](docs/decisions/0033-surface-groups.md)). Missing: a centre picked on the model, laps turned back at an avoided face's clearance (it lifts over the dome). |
| Radial            | partial     | Centre, radii, angles, slopes; the nearest spoke next, each from its nearer end where they go both ways, entered straight down with no ramp ([0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)). Missing: a centre picked on the model. |
| Morphed Spiral    | partial     | Turns from the boundary's shape into its centre; a new one both ways, its pieces the nearest next from their nearer ends, entered straight down with no ramp ([0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)). Missing: a centre picked on the model. |
| Pencil            | partial     | Tested on `ops_3d` as a new one starts with a 12 mm ball, larger than its floor's fillets, and a 6 mm one, which fits them, at a 40° bitangency angle and with two stepovers. Where the tool touches the part twice, found on a grid of drops and each point found exactly across the line: every crease between sheets of where the tool rests that meet sharper than the bitangency angle, and every wall's foot where what is below faces the wall at that angle or more; the nearest line next, both ways, one way (the wall on the right) or the other; overthickness; stepovers, passes either side a stepover apart along the surface ([0028](docs/decisions/0028-pencil.md)). Missing: straight plunges down an upright corner at the cutting feed (left to the linker), up or down milling, an inside or outside direction, unlimited stepovers, fragment settings, fillets, smoothing, a slope range, a contact point boundary, rest machining. |
| Ramp              | partial     | Tested on `ops_3d` as a new one starts, at a 2 mm stepdown, at a 0.5° ramp angle and with the dome avoided 1 mm each way. Contour's rings gone down wall by wall: each wall from the level above its top ring along it, once round it there but on the first wall, down to each ring below on the part at every height between, a stepdown a lap round the longer ring and no steeper than the ramp angle, and once round its foot; the next wall the nearest; direction, flat areas, slopes ([0027](docs/decisions/0027-ramp.md)); surface groups, a wall's way down cut out where it touches only faces avoided ([0033](docs/decisions/0033-surface-groups.md)). A ring at a floor where the tool touches the floor and an avoided face's clearance at once is kept, as contour's is. Missing: horizontal leads, cutting bottom up, shallow areas, a minimum diameter, a minimum cutting radius, fillets, ramp types. |
| Project           | partial     | Tested on `ops_3d` along the dome's foot, the floor's edge and the round's lower edge, as a new one starts, the other way, a millimetre up and with the 12 mm ball. Picked edges, open or closed, joined end to end, each seen from above and dropped onto the part where the tool rests, within the boundary, in the order picked; one way (closed clockwise, open as picked), the other way, or both ways (an open curve from its nearer end); each closed curve started nearest where the tool is; axial offset; slopes; lifted between curves ([0029](docs/decisions/0029-project.md)). Missing: sketches, a chain's direction picked, up or down milling, multiple depths, fillets, rest machining, a contact point boundary. Projecting other than straight down needs more than 3 axes. |
| Morph             | partial     | Tested on `ops_3d` between the dome's foot and the floor's edge, as a new one starts and one way, and between the R8 round's two edges. Passes between two picked curves, both open or both closed, each a step of the way from every point of the one to its match as far along the other, as many as keep each within the stepover of its match on the next, dropped onto the part within the boundary; two open curves matched end to end, the second turned round where it runs the other way; two closed ones clockwise, the second started where the matched points are closest; from the curve nearer the work zero; both ways, one way or the other way; slopes ([0030](docs/decisions/0030-morph.md)). Missing: other ways of matching the two curves' points, pass extension, a contour offset, a number of stepovers, offset stepovers, up or down milling, multiple depths, fillets, rest machining, a contact point boundary, sketches, a chain's direction picked. |

## Drilling

| Feature                         | Status      | Notes |
|---------------------------------|-------------|-------|
| Holes picked by their rims      | done        | |
| Drilling to each hole's bottom  | done        | Tested on `ops_block`. A blind hole to its floor, a through hole through. |
| Peck drilling                   | done        | |
| Dwell at the bottom             | done        | |
| Canned cycles in the program    | partial     | Posted when a profile asks; off in every built-in post until dry-run on a machine. |
| Tapping                         | not started | |
| Reaming and boring cycles       | not started | |

## Shared settings

What an operation's Geometry, Heights, Passes and Linking tabs hold, across
operations.

| Feature                                | Status      | Notes |
|----------------------------------------|-------------|-------|
| Machining boundary and containment     | done        | Stock, bounding box, silhouette or selection; centre, inside or outside; offset. |
| Faces and edges picked on the model    | done        | Picks survive a model re-exported ([0002](docs/decisions/0002-feature-references.md)). |
| Heights                                | done        | Clearance, retract, top and bottom, from the stock, model or selection. |
| Multiple depths                        | done        | |
| Stock to leave, radial and axial       | done        | Negative included. |
| Smoothing                              | done        | Every strategy but drilling ([0015](docs/decisions/0015-smoothing.md)). |
| Feed optimisation                      | done        | Every strategy but drilling and adaptive ([0016](docs/decisions/0016-feed-optimisation.md)). |
| Shaft and holder                       | done        | Off, pull away, trim or fail ([0021](docs/decisions/0021-shank-and-holder.md)). |
| Expressions                            | done        | `tool_diameter * 0.4`: the tool's and the operation's values by name, and functions such as `Math.min` ([jobs](docs/jobs.md#expressions)). |
| Defaults for new operations            | done        | As expressions where they follow the tool ([0020](docs/decisions/0020-defaults.md)). |
| Tabs                                   | partial     | On profiles. Missing: tab positions picked on the model. |
| Rest machining                         | partial     | Adaptive and pocket clearing, from earlier operations. Missing: from the setup's stock or a file, and on finishing. |
| Machine and avoid surfaces             | partial     | On every 3D finish: the model machined or avoided, and groups of picked faces, each machined or avoided with its own radial and axial offsets. The tool meets each group grown by its offsets and rests on whatever is highest; a pass is kept where it touches a face machined, cut out where it touches only faces avoided, machined where it touches both; a flat avoided gets no passes; links lift off faces avoided; CC111 fails a program inside an avoided face's clearance. In the app, on an operation's Geometry tab, the model set to machine or avoid, and groups of picked faces each machined or avoided with their own radial and axial offsets, tinted in the view. Tested on `ops_3d` with Parallel, Scallop, Spiral, Contour, Ramp and Flat, eleven cases with groups ([0033](docs/decisions/0033-surface-groups.md)). Missing: roughing (what it should do with a group is not yet decided), 2D operations and drilling, faces ignored, fixture surfaces, machining over holes, V-bits and chamfer mills where anything is avoided, picking faces in `camcraft post`, and staying down past what is avoided (camcraft plunges onto the next piece, but lifts where a link could turn back at the clearance and stay down, [0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)). |
| Operation patterns                     | not started | Copies on an arranged sheet are not patterns: each is a copy of the part, and every operation cuts them all. |
| Templates                              | not started | |
| Toolpath trimming                      | not started | |
| Manual NC                              | not started | |

## Linking and entries

| Feature                               | Status      | Notes |
|---------------------------------------|-------------|-------|
| Retraction and safe distance          | done        | Minimum or full ([0004](docs/decisions/0004-linking-and-entries.md)). |
| Staying down between passes           | done        | A stay-down distance for each strategy. Passes reordered the nearest next for Parallel, Spiral, Radial and Morphed Spiral, each taken from its nearer end where the operation cuts both ways ([0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)); Scallop's rings in the field's own order ([0035](docs/decisions/0035-scallop-over-every-slope.md)). Beside a wall below where the tool rests from above, a link stays down where the way is clear of the part. |
| Leads                                 | done        | Vertical and level arcs and a straight. |
| Ramps and helices                     | done        | Ramp angle, most per lap, helix size from the tool, never below half the tool less its corners, nor a quarter of the tool; adaptive ramps along an area's middle where no helix fits ([0036](docs/decisions/0036-adaptive-levels-order-and-entries.md)). The finishes over the part take none: they come straight down onto a pass. A new Flat starts without one ([0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)). |
| Smooth transitions between passes     | partial     | Zigzag passes joined by a half turn; contour ramps down the wall ([0012](docs/decisions/0012-transitions-and-entries-defaults.md)). Missing: a turn between passes further apart than the tool, or not running opposite ways. |
| Entry positions picked on the model   | not started | |

## Models

| Feature                               | Status      | Notes |
|---------------------------------------|-------------|-------|
| STEP                                  | done        | Pure Rust (ogeom 0.11.5, its last release under MIT or Apache-2.0), in the file's units, each import checked against the file ([0001](docs/decisions/0001-step-backend.md)). |
| STL                                   | done        | |
| Position, rotation and scale          | done        | |
| Arcs from the model's round edges     | partial     | From the topology. Missing: the B-rep's own circles; ellipses and splines are followed by lines. |
| Fine tolerances on large curved parts | partial     | Held to 0.01 mm since ogeom 0.11 ([ogeom-rs#216](https://github.com/gilbertorconde/ogeom-rs/issues/216)), and a round whose edge stands a file's slop off it since 0.11.2 ([ogeom-rs#261](https://github.com/gilbertorconde/ogeom-rs/issues/261)). Missing: a private part with a thin B-spline strip is refused at 0.005 mm and finer ([0001](docs/decisions/0001-step-backend.md)). |
| IGES, 3MF, OBJ                        | not started | |

## Setups and stock

| Feature                                | Status      | Notes |
|----------------------------------------|-------------|-------|
| Several setups a job                   | done        | |
| Work zero                              | done        | The model's, a point on the stock or model box, or picked; with an offset. |
| Work offset                            | done        | |
| Box stock                              | done        | Around models with margins, or a fixed box placed against them. |
| Sheets that arrange their parts        | partial     | A sheet of a given size that lays its parts out on itself by their true shape seen from above, holes and all, laid again whenever a part, the sheet or a setting changes, never stored; every layout checked exactly. Spacing, border, rotation (none, half turn, quarter turns or a step), the corner packing starts from, the axis it is used up along, the face parts stand on; up to 1000 copies of a part and a grain of its own; parts in other parts' holes; a pick cuts every copy; CC109, CC110 and CC225 ([0031](docs/decisions/0031-arranged-sheets.md)). Missing: several sheets (parts with no room are reported, not put on another), parts placed by hand, Morph across copies, a work zero picked on a part of several copies, the same layout promised across platforms. Parts may stand up to a quarter of the spacing further apart than asked. |
| Orientation                            | partial     | The 24 square ways a part sits. Missing: an axis or plane picked on the model. |
| Cylinder and tube stock                | not started | |
| Stock from a solid                     | not started | |
| Stock carried to the next setup        | not started | Needs a volumetric stock: the material is a height field seen from above ([0003](docs/decisions/0003-grids-only-for-stock.md)). |
| Fixtures and clamps checked against    | not started | |
| Work offset manager                    | not started | |

## Tools

| Feature                                 | Status      | Notes |
|-----------------------------------------|-------------|-------|
| Flat, ball and bullnose end mills       | done        | |
| V-bits and chamfer mills                | done        | Finishing only; refused for clearing. |
| Drills                                  | done        | |
| Shank, neck and holder                  | done        | Built-in ER, BT30 and ISO30 holders; four built-in spindles. |
| Speed and feed presets                  | done        | Surface speed and chip load shown beside them. |
| System library                          | done        | Twenty-three common inch router bits, 1/32" to 2", with presets per material. |
| Tool library, in folders, with undo     | done        | |
| Tool libraries imported from CSV        | done        | As tool makers publish them. |
| Tapered end mills                       | not started | |
| Thread mills                            | partial     | Single-profile: a tooth of its profile angle one pitch tall on a neck as narrow as its root; imported from a library's CSV with its pitch and profile. Missing: multi-tooth and tapered thread mills, tips other than a point. |
| Taps                                    | not started | |
| Slot, dovetail and lollipop cutters     | not started | |
| JSON tool libraries                     | not started | They carry holders. |
| Holders and machines of one's own       | not started | |

## Simulation and checks

| Feature                                  | Status      | Notes |
|------------------------------------------|-------------|-------|
| Material removal                         | done        | Every program simulated against the stock ([simulation.md](docs/simulation.md)). |
| Gouge check                              | done        | Nothing is posted that cuts below the part. |
| Rapid, plunge and feed checks            | done        | CC101 to CC106, among the failures, which run CC101 to CC111; the warnings and notes run CC201 to CC225, and CC299 ([jobs.md](docs/jobs.md#checks)). CC111 fails a program inside an avoided face's clearance. |
| Shank, holder and spindle nose check     | done        | CC108. |
| Playback                                 | done        | Move by move, with the cut stock and the whole tool. |
| Cut stock while setting up               | done        | |
| Machining time                           | done        | Rapids at an assumed 5000 mm/min unless the machine gives a rate. |
| Machine simulation                       | not started | The machine's axes, travel and limits. |

## Posts and programs

| Feature                                  | Status      | Notes |
|------------------------------------------|-------------|-------|
| Masso                                    | done        | Millimetres or inches. |
| Centroid CNC12 for an Avid router        | done        | Millimetres or inches. |
| GRBL                                     | done        | |
| A post of your own, in TOML              | done        | |
| Arcs and helices in the program          | done        | `G2`/`G3` in `G17`, `G18` and `G19`. |
| Every program read back before writing   | done        | Read as a controller would, and held to its path. |
| Several operations in one program        | not started | One program per operation today. |
| Setup sheets                             | not started | |

## App

| Feature                                  | Status      | Notes |
|------------------------------------------|-------------|-------|
| Jobs as documents, in tabs, with undo    | done        | ([0019](docs/decisions/0019-undo.md)) |
| Tree of models, stocks and setups        | done        | Status dot and machining time on each operation. |
| Properties on tabs                       | done        | Help with a sketch on hover. |
| Picking in the view                      | done        | |
| Generating by itself                     | done        | As editing settles, keeping what still stands ([0013](docs/decisions/0013-generating-by-itself.md), [0017](docs/decisions/0017-what-a-run-keeps.md)). |
| View cube                                | done        | |
| Millimetres or inches                    | done        | |
| Themes                                   | partial     | Omarchy's, bundled or your own ([0018](docs/decisions/0018-themes.md)). Missing: a light camcraft theme that follows the system. |

## Project

| Feature                                  | Status      | Notes |
|------------------------------------------|-------------|-------|
| Windows and Linux builds                 | done        | On [Releases](https://github.com/dan335/camcraft/releases), no installer. |
| macOS build                              | not started | Builds from source. |
| Installer                                | not started | |
| CI running the gates                     | not started | `scripts/check.sh` is the only gate. |
| Cut on a real machine                    | not started | |

## Not planned

Turning, multi-axis (3+2, 4- and 5-axis), probing and additive.

Steep and shallow, blend, flow, corner and geodesic finishing, and hole
recognition.

## Before the first cut

The simulator verifies against a *simulated* part. It cannot check the things that
actually break cutters on the day:

- **No built-in post has run on a machine.** The Masso profiles were built from
  the controller's documented G-code reference, the Centroid ones for an Avid
  router from the CNC12 post processor Avid publishes. Dry-run them well above the work
  first. The tests read every program back as a controller would and hold it to
  its path (`camcraft_post::readback`), but nothing there knows what a machine's
  own macros do, such as Avid's tool change at `M6`. Canned drilling cycles are
  off in every built-in profile until they have been dry-run.
- Nothing knows whether the WCS is where the job assumes, whether the tool in the
  spindle matches its `diameter`, or whether the stock is the size the job was told.
- Roughing has never cut anything.

## Open questions

- **Fine tolerances on large curved parts.** ogeom 0.11 meshes `bowl_01`, 380
  mm across, and `round_edge` to 0.01 mm (gilbertorconde/ogeom-rs#216, fixed),
  where 0.10 stopped a face at 512 steps a direction. A private part 340 mm
  across is still refused at 0.005 mm and finer, where a thin B-spline strip
  along its edge stands 0.0075 mm off at 0.0025
  ([decision 0001](docs/decisions/0001-step-backend.md)).
- **Pocket clearing's time grows with the part's triangles.** Profiling the
  example job, its roughing took 5.7 s on 33,824 triangles and 41.3 s on 206,058,
  where finishing, which drop-cuts through a BVH, went from 0.67 s to 0.83 s.
  Something in clearing visits every triangle where an index would visit few. Not
  yet found; the ruled-face rebuild (decision 0001) hides most of it for now.
- **ogeom's corners.** ogeom can place a B-rep vertex up to 1e-14 mm apart on the
  faces meeting there, which would leave the welded solid open, so the importer
  joins faces' points within 1e-9 mm (gilbertorconde/ogeom-rs#217).

## Standing cautions

- **`fixtures/step/` holds real shop jigs,** published with the repository by choice.
  Parts that cannot be published belong in `CAMCRAFT_FIXTURES`, never in a commit.
- **B-spline and revolved faces are held to their surfaces only as ogeom read
  them.** `step-check` holds faces on planes, cylinders, cones, spheres and tori to
  the surfaces in the file's text, but a B-spline, a surface of revolution or an
  extrusion only to its box. The importer measures every curved face's mesh against
  ogeom's surface for it, which sees a mesh too coarse but not a surface misread.
  `scripts/kernel-trial.sh` measures that against the true shape.
- **The public fixture set cannot reproduce the bug that disqualified
  `truck-stepio`.** The part that exposed it is private. A synthetic
  sculpted-surface fixture should replace it.
- **There is no CI yet**, so `scripts/check.sh` is the only
  thing between a mistake and `main`. Run it unpiped: piping through `tail` buffers
  the whole run and hides progress.
