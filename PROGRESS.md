# Progress

camcraft 0.10.0, updated 2026-11-20.

What camcraft does and what it has yet to do, measured against Fusion's
3-axis milling: each of Fusion's milling operations, and the settings, tools,
setups and posts a router job uses; then what comes next, what the simulator
cannot check, and the open questions. [CHANGELOG.md](CHANGELOG.md) says what
each release brought.

> [!WARNING]
> **Nothing has been cut on a machine yet.** Every row here is done in the
> software and checked against a simulated part, nothing more. See
> [Before the first cut](#before-the-first-cut).

- **done**: does what Fusion's does, with the settings a 3-axis router job uses.
- **partial**: works, without what is listed beside it.
- **not started**: not in camcraft.

*Matches Fusion* means a test holds a new operation to Fusion's own program for
the same part, and the two leave the same material to 0.01 mm
(the tests in `crates/camcraft-session/src/compare/`). What Fusion has only with its
Manufacturing Extension is [not planned](#not-planned).

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
   cuts are now plunged onto, as Fusion's, and taken the nearest next by
   Parallel and Spiral
   ([decision 0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)),
   but most of what still lifts is a link that would rest on the face
   avoided, where Fusion's Scallop and Spiral turn back at its clearance and
   stay down; then the rings round the walls of Parallel and Spiral, most of
   what is still fed past Fusion's; and Scallop's lifts between the pieces
   its outer boundary cuts a ring into
   ([decision 0035](docs/decisions/0035-scallop-over-every-slope.md)); and
   its blended rings nearest where the tool leaves the part, which still bob
   a tenth of a millimetre every few millimetres
   ([decision 0038](docs/decisions/0038-scallop-rings-blend-into-an-edge-at-a-slant.md));
   and outside in, a zone between two edges measured from its outer one alone,
   where now its rings from each edge meet in the middle and lift between
   them, and inside out the rings outside an off-centre pocket, which end in
   arcs on the block's edge ([decision 0039](docs/decisions/0039-scallop-zones-at-a-sharp-rim.md)).
4. **Surface groups for roughing**, once Fusion's rule there is known: its
   Adaptive and Pocket Clearing gave no toolpath with any group set.
5. **A volumetric stock**, so what a top setup leaves carries to a flipped
   bottom one.
6. **Exact slicing of the offset surface** for Z-level work
   ([decision 0003](docs/decisions/0003-grids-only-for-stock.md)).
7. **Exact circles from the B-rep**, rather than through the tessellation's
   points.
8. The work offset manager in the toolbar; Fusion's `.json` tool libraries,
   which carry holders; tapered ball mills.

## 2D operations

| Feature              | Status      | Notes |
|----------------------|-------------|-------|
| Face                 | done        | Matches Fusion. Direction, pass extension, stock offset, multiple depths. |
| 2D Contour           | partial     | Matches Fusion. Outside, inside or on; tabs; leads; helix, plunge or ramp in; multiple depths; rings follow round edges as arcs ([0005](docs/decisions/0005-2d-profile-and-pocket.md), [0006](docs/decisions/0006-2d-linking-and-arcs.md)). Missing: sideways roughing passes, finishing passes, picked entry points. |
| 2D Pocket            | partial     | Matches Fusion. Finishing passes with leads; helix, ramp or plunge in. Missing: leads on its roughing rings, picked entry (predrill) points. |
| 2D Adaptive Clearing | partial     | Matches Fusion: held to its programs for `ops_2d`'s pocket with an island and its notch open to two sides (the stock flush with the part there, as Fusion's open edge is open to air). A 2D pocket's areas, picked or the silhouette, each cleared by 3D adaptive's engine at the pocket's levels: to each floor in one, or multiple depths, whole or even; islands cut round; open pockets found where the outline runs along the part's edge, and entered from outside the stock; optimal load, minimum cutting radius, slot clearing, direction, order by depth or area, helix or plunge in, stay-down level, clearance and lift ([0024](docs/decisions/0024-chamfer-and-2d-adaptive.md)). Missing: open pockets' extension methods (tangent, closest boundary, parallel) and stock contours, rest machining from a previous tool's size, predrill and entry positions, ramp clearance height and taper, both ways' other-way load and feed, no-engagement feed, separate lead radii, sketches. |
| Slot                 | partial     | Matches Fusion: held to its programs for `ops_2d`'s two slots with a tool narrower than them and one as wide. Closed slots picked by the edges round their floors, or their floors: straight, curved or several of either; the middle only, a narrower tool leaving the walls; ramped from the ramp clearance height back and forth along the whole middle, each leg no more than half the maximum ramp stepdown, then once along the floor; multiple depths; repeat finishing pass, backoff, preserve order ([0023](docs/decisions/0023-slot-and-trace.md)). Missing: open and semi-open slots and tangential extension, ramp types (predrill, plunge), picked entry positions, sketches. Multiple depths, backoff and repeat are not yet compared with Fusion's programs. |
| Trace                | partial     | Matches Fusion: held to its program for `ops_2d`'s two V grooves, on the curve. Picked edges followed, open or closed, joined end to end, a click taking the smooth chain; on the curve or beside it left or right; axial offset, pass extension, maximum angle, multiple depths with number of stepdowns, by depth or by curve, both ways, repeat, preserve order, keep tool down, vertical lead-in and lead-out ([0023](docs/decisions/0023-slot-and-trace.md)). Missing: chamfer (the checks can now hold a chamfer cut on purpose, as a 2D chamfer's), chamfer tip offset, up/down milling, multiple passes, spiral stepdowns, separate lead-out radius, flipping a chain's direction (Fusion runs a round groove the other way), tool orientation, sketches. Compensation, multiple depths and the rest not yet compared. |
| Thread               | partial     | Matches Fusion: held to its programs for `ops_2d`'s through and blind holes, as a new one and with a pitch diameter offset of a millimetre, the holes bored first. Holes picked by their walls, threaded with a single-profile thread mill from each wall's bottom to its top: down the centre, a straight and a helical arc onto the thread, the helix at the pitch in half turns, the same off and back to the centre; pitch (the tool's), pitch diameter offset, right or left hand, climb or conventional, multiple passes, helical or level leads, lead to center. The thread cut into the wall is held by the checks as cut on purpose, and anything deeper is a gouge; a helical lead into a blind hole's floor, where Fusion's goes too, is said (CC224) and fails the part check ([0026](docs/decisions/0026-thread.md)). Missing: outside threads (bosses), multi-tooth thread mills, start angle, multiple threads, repeat passes, stock to leave, compensation in control, threads from a standard or the model, diameter range and same-diameter selection, hole order choices. Left hand, conventional, multiple passes, level leads and leads from beside the arc are not yet compared with Fusion's programs. |
| Bore                 | partial     | Holes and bosses picked by their walls, from each wall's top to its bottom; pitch or ramp angle, stepovers, finishing passes, repeat, leads and lead to center, climb or conventional ([0022](docs/decisions/0022-bore-and-circular.md)). Matches Fusion. Missing: tapered walls, diameter range and same-diameter selection, hole order choices, compensation in control, bottom-to-top. |
| Circular             | partial     | Holes and bosses picked by their walls, cut level by level, whole or even stepdowns, each pass down its lead and lifted out after; stepovers, repeat, leads and lead to center, climb or conventional ([0022](docs/decisions/0022-bore-and-circular.md)). Matches Fusion. Missing: diameter range and same-diameter selection, hole order choices, compensation in control. |
| Engrave              | partial     | Matches Fusion, on all four of its programs for a plate with a triangle, a stroke and a rectangle sunk into it. Shapes picked by their rims or floors, islands too; the V's flank on the outline, round a wide shape at the depth, along a narrow one's middle, up into corners sharper than the sharp corner angle; depth (0.95 of the flank, as Fusion's Bottom), flat tips, multiple depths; a shape sunk less deep than the V goes is said (CC223) and fails the part check ([0025](docs/decisions/0025-engrave.md)). Missing: Top and Bottom from other references, Keep Tool Down, an island far from its outline joined to it in one pass, entry positions, sketches and text, polar and wrap. |
| 2D Chamfer           | partial     | Matches Fusion: held to its programs for `ops_2d`'s chamfered rim, picked by its lower loop and by its upper, and a sharp rim a millimetre wide, each on the pocket cleared first. Picked edges, or every edge of picked faces, chamfered with a chamfer mill or V-bit: a sharp edge to the chamfer width asked, a chamfer the model has followed from its lower edge; tip offset, clearance (the tool lifted off the part's other faces), finishing overlap, preserve order; the side of the part found from the part; the chamfer asked is held by the checks as cut on purpose, and anything deeper is a gouge ([0024](docs/decisions/0024-chamfer-and-2d-adaptive.md)). Missing: outer corner modes other than roll, lead-in and lead-out settings (always Fusion's default straight, a tenth of the tool), compensation in control, tangential extension of open chains, sketches. |

## 3D roughing

| Feature          | Status      | Notes |
|------------------|-------------|-------|
| Adaptive Clearing | partial    | Fusion's optimal load, fine stepdown, flat and shallow areas, minimum cutting radius, machine cavities, slot clearing, order by area, stay-down level, clearance and lift, rest machining from earlier operations, shaft and holder ([0007](docs/decisions/0007-adaptive-clearing.md)). Even slabs, fine levels even up to a flat; by area, each slab's level then its fine levels up the wall, area by area; a pass going round on the spot ends and material too narrow for the load is taken along it ([#1](https://github.com/dan335/camcraft/issues/1)); helixes no smaller than Fusion's minimum, a ramp along the area's middle where none fits ([0036](docs/decisions/0036-adaptive-levels-order-and-entries.md)). Links stay down across what is cleared wherever lifting would go further, rise to a slab's fine levels at the feed, and a slab's level starts nearest where the tool is; by area, each area is come to where the tool last was in it. Missing: rest machining from the setup's stock or a file, predrill points, minimum retraction over the material as it stands rather than as the operation found it. |
| Pocket Clearing  | partial     | Levels cut from the middle out, climb or conventional, flat areas, rest material on walls, helix, ramp or plunge in ([0008](docs/decisions/0008-pocket-clearing.md)). Missing: corner smoothing, a finishing pass. Its time grows with the part's triangles ([open questions](#open-questions)). |

## 3D finishing

| Feature           | Status      | Notes |
|-------------------|-------------|-------|
| Parallel          | partial     | Held to Fusion's programs for `ops_3d` as a new one starts, with the dome avoided 1 mm each way and with no clearance, with only the pocket's floor machined, with the floor machined with 0.5 mm of its own and with the top avoided 2 mm down: 74.4%, 69.7%, 72.0%, 55.2%, 65.1% and 73.7% of the columns either cuts alike, the 95th percentile 0.061, 0.106, 0.077, 0.53, 0.13 and 0.062 mm; lifting 1, 9, 9, 9, 1 and 3 times to Fusion's 1, 15, 2, 2, 1 and 2. Passes at any angle, steep ground left to rings round the walls ([0014](docs/decisions/0014-walls-for-every-finish-over-the-part.md)), the nearest next from either end, entered straight down with no ramp, as Fusion's ([0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)); surface groups ([0033](docs/decisions/0033-surface-groups.md)). Missing: a slope range, passes cut one way, Fusion's lead through the roughing's stock (camcraft plunges onto the pass where the lead's top is in material), links that stay down past a face avoided (most of what still lifts), its runs past where the tool touches a face machined and one avoided at once (to Z10.93 up the dome's foot, where camcraft stops at Z10). With no groups most of what the figures lose is camcraft's rings round the walls, which take down the steps Fusion's passes leave there, up to 6.7 mm, and feed 35% further. Other angles, stock to leave and flat or bull nose tools are not compared. |
| Scallop           | partial     | Held to Fusion's programs for `ops_3d` as a new one starts, with the dome avoided 1 mm each way and with only the pocket's floor machined: 94.6%, 58.7% and 42.4% of the columns either cuts alike, the 95th percentile 0.010, 0.25 and 3.7 mm; lifting 1, 3 and 2 times to Fusion's 1, 8 and 3, and feeding 1% further, 2% less and 24% less. Rings a constant distance apart along the surface, slopes, both directions and orders ([0009](docs/decisions/0009-scallop-distance-field.md)); every slope cut by the passes themselves, walls included, each point found round a wall at its height, and level rings only below where the tool rests from above, down to the Bottom; rings laid anew round faces avoided, inside out measured from the inner edges, one straight seam, the field's own order ([0035](docs/decisions/0035-scallop-over-every-slope.md)); the field's grid cut where the ground machined ends, so its rings run down to where the tool leaves the part from above, found exactly, the last along it, with level rings from a stepover below its highest point, and up an upright wall above a face avoided from the wall's foot ([0037](docs/decisions/0037-scallop-to-where-the-tool-leaves-the-part.md)); where the rings meet that line at a slant, closed rings that blend from the last whole ring into it, and below it closed rings down to the first level ring, so no ring there ends in arcs, that line's height rising and falling no faster than a ramp, and a ring under the last down a steep wall ramped onto along it ([0038](docs/decisions/0038-scallop-rings-blend-into-an-edge-at-a-slant.md)); inside out, a sharp rim a break between zones, the rings stopping where the tool starts rolling over its edge and starting again past it, each border a ring, so none climbs over the edge and back ([0039](docs/decisions/0039-scallop-zones-at-a-sharp-rim.md)); inside out, inside the last ring going in with no corner, rings out from a spine midway between its sides, blended into it, where offsets came to points ([0040](docs/decisions/0040-scallop-spine-inside-the-last-smooth-ring.md)); entered straight down with no ramp ([0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)); surface groups ([0033](docs/decisions/0033-surface-groups.md)). Missing: a limit on stepovers, rest machining, a contact point boundary, Fusion's turning back at a face avoided and staying down, its runs past where the tool touches a face machined and one avoided at once (with only the floor machined, up to 7.8 mm lower on the fillets and walls), staying down between the pieces the outer boundary cuts a ring into, and the blended rings nearest where the tool leaves the part still turning in height a tenth of a millimetre every few millimetres (decision 0038); outside in, a zone measured from both its edges, its rings meeting in the middle (decision 0039); inside out on a block with an off-centre pocket, the rings outside the pocket end in arcs on the block's edge (decision 0039); the band into the last smooth ring bunching where that ring is nearest the spine (decision 0040). Slopes, other directions and orders, stock to leave and flat or bull nose tools are not compared. |
| Contour           | partial     | Held to Fusion's programs for `ops_3d` as a new one starts and with the dome avoided 1 mm each way: 55.4% and 43.8% of the columns either cuts alike, the 95th percentile 17.75 and 17.93 mm; inside the pocket the rings are Fusion's levels. Waterline levels, ramped down the wall from level to level, by depth or by area, slopes ([0010](docs/decisions/0010-contour-and-flat.md)); surface groups ([0033](docs/decisions/0033-surface-groups.md)). Missing: helix entries, Fusion's containment as a new one starts (camcraft's keeps the tool outside the silhouette and goes round the block's outside walls down to Z0, where Fusion's keeps the tool's centre on it and stops at the pocket's floor: most of the figures above), Fusion's ring at a floor cut out where the tool touches it and an avoided face's clearance at once (camcraft keeps it, taking the roughing's half millimetre off the floor round the dome). Other directions and orders, slopes, stock to leave and flat or bull nose tools are not compared. |
| Flat              | partial     | Held to Fusion's programs for `ops_3d` with the 6 mm flat end mill as a new one starts and with the dome avoided 1 mm each way: 65.9% and 70.5% of the columns either cuts alike, the 95th percentile 0.5 and 5.5 mm. Each flat face cleared at its own height ([0010](docs/decisions/0010-contour-and-flat.md)); only the flats it machines, kept clear of faces avoided ([0033](docs/decisions/0033-surface-groups.md)); a new one comes straight down onto each ring, as Fusion's starts with its Ramp off, a ramp along the ring a choice ([0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)). Missing: multiple depths on a flat, Fusion's lead-ins, telling a tessellated curve from a flat (camcraft clears the dome's top at Z15 as one, which Fusion leaves alone), Fusion's skipping of slivers (beside the dome avoided camcraft clears slivers a tenth of a millimetre round, plunging into the 5.5 mm the roughing left, which Fusion does not: three times as much removed). It still feeds three times as far as Fusion's, not yet looked into. Conventional, inside-out, stock to leave and ball or bull nose tools are not compared. |
| Spiral            | partial     | Held to Fusion's programs for `ops_3d` as a new one starts and with the dome avoided 1 mm each way: 85.8% and 82.7% of the columns either cuts alike, the 95th percentile 0.029 and 0.042 mm; lifting 1 and 8 times to Fusion's 5 and 6, and feeding 30% and 34% further, the rings round the walls; the spiral is Fusion's, 0.345 mm a lap about the part's middle. Centre, radii, slopes, both directions and orders ([0011](docs/decisions/0011-patterns-dropped-onto-the-part.md)); a new one both ways, as Fusion's, the pieces the part cuts it into the nearest next from their nearer ends, entered straight down with no ramp ([0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)); surface groups ([0033](docs/decisions/0033-surface-groups.md)). Missing: a centre picked on the model, Fusion's order as a new one starts (from the middle out, where camcraft's runs from its outer radius in), its laps turned back at an avoided face's clearance (camcraft lifts over the dome), its runs past where the tool touches a face machined and one avoided at once (up to 3.0 mm lower on the dome's flank). Slopes, radii, other directions, stock to leave and flat or bull nose tools are not compared. |
| Radial            | partial     | Centre, radii, angles, slopes; the nearest spoke next, each from its nearer end where they go both ways, entered straight down with no ramp ([0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)). Missing: a centre picked on the model. Not compared with Fusion's programs. |
| Morphed Spiral    | partial     | Turns from the boundary's shape into its centre; a new one both ways, its pieces the nearest next from their nearer ends, entered straight down with no ramp ([0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)). Missing: a centre picked on the model. Not compared with Fusion's programs. |
| Pencil            | partial     | Held to Fusion's programs for `ops_3d` as a new one starts with a 12 mm ball, larger than its floor's fillets, and a 6 mm one, which fits them, at a 40° bitangency angle and with two stepovers: 95.3%, 74.5%, 95.3% and 69.3% of the columns either cuts alike, the 95th percentile 0.0098, 2.35, 0.0098 and 0.73 mm. Where the tool touches the part twice, found on a grid of drops and each point found exactly across the line: every crease between sheets of where the tool rests that meet sharper than the bitangency angle, and every wall's foot where what is below faces the wall at that angle or more; the nearest line next, both ways, one way (the wall on the right) or the other; overthickness; stepovers, passes either side a stepover apart along the surface ([0028](docs/decisions/0028-pencil.md)). Missing: Fusion's order and links (with the 6 mm ball its links lift where camcraft's stay down through the roughing's leftovers, most of what its figures lose), straight plunges down an upright corner at the cutting feed (left to the linker), Up/Down Milling, Inside/Outside Direction, unlimited stepovers, fragment settings, fillets, smoothing, a slope range, a contact point boundary, rest machining. Overthickness, one way and the other, stock to leave and flat or bull nose tools are not compared. |
| Ramp              | partial     | Held to Fusion's programs for `ops_3d` as a new one starts, at a 2 mm stepdown, at a 0.5° ramp angle and with the dome avoided 1 mm each way: 61.7%, 46.2%, 60.6% and 59.6% of the columns either cuts alike, the 95th percentile 0.31, 0.82, 0.32 and 1.57 mm. Contour's rings gone down wall by wall: each wall from the level above its top ring along it, once round it there but on the first wall, down to each ring below on the part at every height between, a stepdown a lap round the longer ring and no steeper than the ramp angle, and once round its foot; the next wall the nearest; direction, flat areas, slopes ([0027](docs/decisions/0027-ramp.md)); surface groups, a wall's way down cut out where it touches only faces avoided ([0033](docs/decisions/0033-surface-groups.md)). Missing: where Fusion ends a pocket's wall above its floor (at a 1 mm stepdown it goes once round at Z11 and down Z10's ring on its own; camcraft goes on down, the main reason for the figures above), cusps placed as Fusion's (the outside starts 25° round its corner from Fusion's), Fusion's ring at a floor cut out where the tool touches it and an avoided face's clearance at once (camcraft keeps it, taking the roughing's half millimetre off the floor round the dome), horizontal leads, Order Bottom-Up, shallow areas, Minimum Diameter, Minimum Cutting Radius, fillets, ramp types. Stock to leave, slopes, conventional and flat or bull nose tools are not compared. |
| Project           | partial     | Held to Fusion's programs for `ops_3d` along the dome's foot, the floor's edge and the round's lower edge, as a new one starts, the other way, a millimetre up and with the 12 mm ball: 98.8%, 98.8%, 99.1% and 90.1% of the columns either cuts alike, the 95th percentile 0.0045, 0.0045, 0.0044 and 0.012 mm. Picked edges, open or closed, joined end to end, each seen from above and dropped onto the part where the tool rests, within the boundary, in the order picked; one way (closed clockwise, open as picked), the other way, or both ways (an open curve from its nearer end); each closed curve started nearest where the tool is; axial offset; slopes; lifted between curves, as Fusion's ([0029](docs/decisions/0029-project.md)). Missing: the other projection modes (greyed out for 3-axis), sketches, a chain's direction picked, Up/Down Milling, multiple depths, fillets, rest machining, a contact point boundary. One way, slopes, stock to leave, negative axial offsets and flat or bull nose tools are not compared. |
| Morph             | partial     | Held to Fusion's programs for `ops_3d` between the dome's foot and the floor's edge, as a new one starts and one way, and between the R8 round's two edges: 91.9%, 91.9% and 99.4% of the columns either cuts alike, the 95th percentile 0.014, 0.014 and 0.0048 mm. Passes between two picked curves, both open or both closed, each a step of the way from every point of the one to its match as far along the other, as many as keep each within the stepover of its match on the next, dropped onto the part within the boundary; two open curves matched end to end, the second turned round where it runs the other way; two closed ones clockwise, the second started where the matched points are closest; from the curve nearer the work zero; both ways, one way or the other way; slopes ([0030](docs/decisions/0030-morph.md)). Missing: Fusion's own start for two closed curves (its passes between the dome and the floor bend elsewhere, and it lays fewer than the stepover asks, most of what those figures lose), Morph Mode (closest, automatic), Pass Extension, Contour Offset, a number of stepovers, offset stepovers, Up/Down Milling, multiple depths, fillets, rest machining, a contact point boundary, sketches, a chain's direction picked, Fusion's linker's order. Other way, slopes, stock to leave and flat or bull nose tools are not compared. |

## Drilling

| Feature                         | Status      | Notes |
|---------------------------------|-------------|-------|
| Holes picked by their rims      | done        | |
| Drilling to each hole's bottom  | done        | Matches Fusion. A blind hole to its floor, a through hole through. |
| Peck drilling                   | done        | |
| Dwell at the bottom             | done        | |
| Canned cycles in the program    | partial     | Posted when a profile asks; off in every built-in post until dry-run on a machine. |
| Tapping                         | not started | |
| Reaming and boring cycles       | not started | |

## Shared settings

What Fusion's Geometry, Heights, Passes and Linking tabs hold, across operations.

| Feature                                | Status      | Notes |
|----------------------------------------|-------------|-------|
| Machining boundary and containment     | done        | Stock, bounding box, silhouette or selection; centre, inside or outside; offset. |
| Faces and edges picked on the model    | done        | Picks survive a model re-exported ([0002](docs/decisions/0002-feature-references.md)). |
| Heights                                | done        | Clearance, retract, top and bottom, from the stock, model or selection. |
| Multiple depths                        | done        | |
| Stock to leave, radial and axial       | done        | Negative included. |
| Smoothing                              | done        | Every strategy but drilling ([0015](docs/decisions/0015-smoothing.md)). |
| Feed optimisation                      | done        | Every strategy but drilling and adaptive, as Fusion's ([0016](docs/decisions/0016-feed-optimisation.md)). |
| Shaft and holder                       | done        | Off, pull away, trim or fail ([0021](docs/decisions/0021-shank-and-holder.md)). |
| Expressions                            | done        | `tool_diameter * 0.4`, Fusion's names and functions. |
| Fusion's defaults for new operations   | done        | As expressions where they follow the tool ([0020](docs/decisions/0020-defaults.md)). |
| Tabs                                   | partial     | On profiles, as Fusion's. Missing: tab positions picked on the model. |
| Rest machining                         | partial     | Adaptive and pocket clearing, from earlier operations. Missing: from the setup's stock or a file, and on finishing. |
| Machine and avoid surfaces             | partial     | Fusion's Surface Manager on every 3D finish: the model machined or avoided, and groups of picked faces, each machined or avoided with its own radial and axial offsets. The tool meets each group grown by its offsets and rests on whatever is highest; a pass is kept where it touches a face machined, cut out where it touches only faces avoided, machined where it touches both; a flat avoided gets no passes; links lift off faces avoided; CC111 fails a program inside an avoided face's clearance. In the app, on an operation's Geometry tab, the model set to machine or avoid, and groups of picked faces each machined or avoided with their own radial and axial offsets, tinted in the view. Held to Fusion's programs for `ops_3d` with Parallel, Scallop, Spiral, Contour, Ramp and Flat, eleven with groups ([0033](docs/decisions/0033-surface-groups.md)). Missing: roughing (Fusion's rule there is not known), 2D operations and drilling, Ignore, fixture surfaces, machining over holes, V-bits and chamfer mills where anything is avoided, picking faces in `camcraft post`, and Fusion's linking past what is avoided (camcraft plunges onto the next piece, as Fusion's Parallel does, but lifts where its Scallop and Spiral turn back at the clearance and stay down, [0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)), and its runs past where the tool touches a face machined and one avoided at once. |
| Operation patterns                     | not started | Copies on an arranged sheet are not patterns: each is a copy of the part, and every operation cuts them all. |
| Templates                              | not started | |
| Toolpath trimming                      | not started | |
| Manual NC                              | not started | |

## Linking and entries

| Feature                               | Status      | Notes |
|---------------------------------------|-------------|-------|
| Retraction and safe distance          | done        | Minimum or full, as Fusion's ([0004](docs/decisions/0004-linking-and-entries.md)). |
| Staying down between passes           | done        | Fusion's stay-down distance for each strategy. Passes reordered the nearest next, as Fusion's Shortest Machining Distance, for Parallel, Spiral, Radial and Morphed Spiral, each taken from its nearer end where the operation cuts both ways ([0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)); Scallop's rings in the field's own order ([0035](docs/decisions/0035-scallop-over-every-slope.md)). Beside a wall below where the tool rests from above, a link stays down where the way is clear of the part. |
| Leads                                 | done        | Vertical and level arcs and a straight. |
| Ramps and helices                     | done        | Ramp angle, most per lap, helix size as Fusion's, never below its Minimum Ramp Diameter; adaptive ramps along an area's middle where no helix fits ([0036](docs/decisions/0036-adaptive-levels-order-and-entries.md)). The finishes over the part take none, as Fusion's have none; a new Flat starts without, as Fusion's ([0034](docs/decisions/0034-entries-and-order-of-the-finishes-over-the-part.md)). |
| Smooth transitions between passes     | partial     | Zigzag passes joined by a half turn; contour ramps down the wall ([0012](docs/decisions/0012-transitions-and-entries-from-fusions-defaults.md)). Missing: a turn between passes further apart than the tool, or not running opposite ways. |
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
| Fusion CSV import                       | done        | |
| Tapered end mills                       | not started | |
| Thread mills                            | partial     | Single-profile, as Fusion's sample library's: a tooth of its profile angle one pitch tall on a neck as narrow as its root; imported from Fusion's CSV with its pitch and profile. Missing: multi-tooth and tapered thread mills, tips other than a point. |
| Taps                                    | not started | |
| Slot, dovetail and lollipop cutters     | not started | |
| Fusion `.json` tool libraries           | not started | They carry holders. |
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
| Tree, after Fusion's browser             | done        | Status dot and machining time on each operation. |
| Properties on Fusion's tabs              | done        | Help with a sketch on hover. |
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

What Fusion has only with its Manufacturing Extension: steep and shallow,
blend, flow, corner, geodesic and hole recognition.

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
