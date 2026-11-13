# Progress

camcraft 0.7.0, updated 2026-11-13.

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
(`crates/camcraft-session/src/compare/tests.rs`). *Extension* marks what Fusion
has only with its Manufacturing Extension.

| Area                                          | Done | Partial | Not started |
|-----------------------------------------------|-----:|--------:|------------:|
| [2D operations](#2d-operations)               |    1 |       9 |           1 |
| [3D roughing](#3d-roughing)                   |    0 |       2 |           0 |
| [3D finishing](#3d-finishing)                 |    0 |       7 |           9 |
| [Drilling](#drilling)                         |    4 |       1 |           3 |
| [Shared settings](#shared-settings)           |   10 |       2 |           5 |
| [Linking and entries](#linking-and-entries)   |    4 |       1 |           1 |
| [Models](#models)                             |    3 |       2 |           1 |
| [Setups and stock](#setups-and-stock)         |    4 |       1 |           5 |
| [Tools](#tools)                               |    8 |       0 |           5 |
| [Simulation and checks](#simulation-and-checks) |  7 |     0 |           1 |
| [Posts and programs](#posts-and-programs)     |    6 |       0 |           2 |
| [App](#app)                                   |    7 |       1 |           0 |
| [Project](#project)                           |    1 |       0 |           4 |
| **All**                                       |   55 |      26 |          37 |

`grep "| not started" PROGRESS.md` lists everything not yet begun.

## What is next

Roughly in the order they matter:

1. **Cut on a machine.** See [Before the first cut](#before-the-first-cut).
2. **More strategies:** steep and shallow, pencil, ramp.
3. **A volumetric stock**, so what a top setup leaves carries to a flipped
   bottom one.
4. **Exact slicing of the offset surface** for Z-level work
   ([decision 0003](docs/decisions/0003-grids-only-for-stock.md)).
5. **Exact circles from the B-rep**, rather than through the tessellation's
   points.
6. The work offset manager in the toolbar; Fusion's `.json` tool libraries,
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
| Thread               | not started | |
| Bore                 | partial     | Holes and bosses picked by their walls, from each wall's top to its bottom; pitch or ramp angle, stepovers, finishing passes, repeat, leads and lead to center, climb or conventional ([0022](docs/decisions/0022-bore-and-circular.md)). Matches Fusion. Missing: tapered walls, diameter range and same-diameter selection, hole order choices, compensation in control, bottom-to-top. |
| Circular             | partial     | Holes and bosses picked by their walls, cut level by level, whole or even stepdowns, each pass down its lead and lifted out after; stepovers, repeat, leads and lead to center, climb or conventional ([0022](docs/decisions/0022-bore-and-circular.md)). Matches Fusion. Missing: diameter range and same-diameter selection, hole order choices, compensation in control. |
| Engrave              | partial     | Matches Fusion, on all four of its programs for a plate with a triangle, a stroke and a rectangle sunk into it. Shapes picked by their rims or floors, islands too; the V's flank on the outline, round a wide shape at the depth, along a narrow one's middle, up into corners sharper than the sharp corner angle; depth (0.95 of the flank, as Fusion's Bottom), flat tips, multiple depths; a shape sunk less deep than the V goes is said (CC223) and fails the part check ([0025](docs/decisions/0025-engrave.md)). Missing: Top and Bottom from other references, Keep Tool Down, an island far from its outline joined to it in one pass, entry positions, sketches and text, polar and wrap. |
| 2D Chamfer           | partial     | Matches Fusion: held to its programs for `ops_2d`'s chamfered rim, picked by its lower loop and by its upper, and a sharp rim a millimetre wide, each on the pocket cleared first. Picked edges, or every edge of picked faces, chamfered with a chamfer mill or V-bit: a sharp edge to the chamfer width asked, a chamfer the model has followed from its lower edge; tip offset, clearance (the tool lifted off the part's other faces), finishing overlap, preserve order; the side of the part found from the part; the chamfer asked is held by the checks as cut on purpose, and anything deeper is a gouge ([0024](docs/decisions/0024-chamfer-and-2d-adaptive.md)). Missing: outer corner modes other than roll, lead-in and lead-out settings (always Fusion's default straight, a tenth of the tool), compensation in control, tangential extension of open chains, sketches. |

## 3D roughing

| Feature          | Status      | Notes |
|------------------|-------------|-------|
| Adaptive Clearing | partial    | Fusion's optimal load, fine stepdown, flat and shallow areas, minimum cutting radius, machine cavities, slot clearing, order by area, stay-down level, clearance and lift, rest machining from earlier operations, shaft and holder ([0007](docs/decisions/0007-adaptive-clearing.md)). Missing: rest machining from the setup's stock or a file, predrill points. Open: [#1](https://github.com/dan335/camcraft/issues/1), a pass can circle a speck of material until its step limit. |
| Pocket Clearing  | partial     | Levels cut from the middle out, climb or conventional, flat areas, rest material on walls, helix, ramp or plunge in ([0008](docs/decisions/0008-pocket-clearing.md)). Missing: corner smoothing, a finishing pass. Its time grows with the part's triangles ([open questions](#open-questions)). |

## 3D finishing

| Feature           | Status      | Notes |
|-------------------|-------------|-------|
| Parallel          | partial     | Passes at any angle, steep ground left to rings round the walls ([0014](docs/decisions/0014-walls-for-every-finish-over-the-part.md)), ramped into. Missing: a slope range, passes cut one way. |
| Scallop           | partial     | Rings a constant distance apart along the surface, slopes, both directions and orders ([0009](docs/decisions/0009-scallop-distance-field.md)). Missing: a limit on stepovers, rest machining, a contact point boundary. |
| Contour           | partial     | Waterline levels, ramped down the wall from level to level, by depth or by area, slopes ([0010](docs/decisions/0010-contour-and-flat.md)). Missing: helix entries. |
| Flat              | partial     | Each flat face cleared at its own height ([0010](docs/decisions/0010-contour-and-flat.md)). Missing: multiple depths on a flat. |
| Spiral            | partial     | Centre, radii, slopes, both directions and orders ([0011](docs/decisions/0011-patterns-dropped-onto-the-part.md)). Missing: a centre picked on the model. |
| Radial            | partial     | Centre, radii, angles, slopes. Missing: a centre picked on the model. |
| Morphed Spiral    | partial     | Turns from the boundary's shape into its centre. Missing: a centre picked on the model. |
| Steep and Shallow | not started | Extension. |
| Pencil            | not started | |
| Ramp              | not started | |
| Project           | not started | |
| Morph             | not started | |
| Blend             | not started | Extension. |
| Flow              | not started | Extension. |
| Corner            | not started | Extension. |
| Geodesic          | not started | Extension. |

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
| Hole recognition                | not started | Extension. |

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
| Avoid and touch surfaces               | not started | |
| Operation patterns                     | not started | |
| Templates                              | not started | |
| Toolpath trimming                      | not started | |
| Manual NC                              | not started | |

## Linking and entries

| Feature                               | Status      | Notes |
|---------------------------------------|-------------|-------|
| Retraction and safe distance          | done        | Minimum or full, as Fusion's ([0004](docs/decisions/0004-linking-and-entries.md)). |
| Staying down between passes           | done        | Fusion's stay-down distance for each strategy. |
| Leads                                 | done        | Vertical and level arcs and a straight. |
| Ramps and helices                     | done        | Ramp angle, most per lap, helix size as Fusion's. |
| Smooth transitions between passes     | partial     | Zigzag passes joined by a half turn; contour ramps down the wall ([0012](docs/decisions/0012-transitions-and-entries-from-fusions-defaults.md)). Missing: a turn between passes further apart than the tool, or not running opposite ways. |
| Entry positions picked on the model   | not started | |

## Models

| Feature                               | Status      | Notes |
|---------------------------------------|-------------|-------|
| STEP                                  | done        | Pure Rust (ogeom), in the file's units, each import checked against the file ([0001](docs/decisions/0001-step-backend.md)). |
| STL                                   | done        | |
| Position, rotation and scale          | done        | |
| Arcs from the model's round edges     | partial     | From the topology. Missing: the B-rep's own circles; ellipses and splines are followed by lines. |
| Fine tolerances on large curved parts | partial     | Held to 0.01 mm since ogeom 0.11 ([ogeom-rs#216](https://github.com/gilbertorconde/ogeom-rs/issues/216)). Missing: a private part with a thin B-spline strip is refused at 0.005 mm and finer ([0001](docs/decisions/0001-step-backend.md)). |
| IGES, 3MF, OBJ                        | not started | |

## Setups and stock

| Feature                                | Status      | Notes |
|----------------------------------------|-------------|-------|
| Several setups a job                   | done        | |
| Work zero                              | done        | The model's, a point on the stock or model box, or picked; with an offset. |
| Work offset                            | done        | |
| Box stock                              | done        | Around models with margins, or a fixed box placed against them. |
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
| System library                          | done        | Twenty common inch router bits with presets per material. |
| Tool library, in folders, with undo     | done        | |
| Fusion CSV import                       | done        | |
| Tapered end mills                       | not started | |
| Taps and thread mills                   | not started | |
| Slot, dovetail and lollipop cutters     | not started | |
| Fusion `.json` tool libraries           | not started | They carry holders. |
| Holders and machines of one's own       | not started | |

## Simulation and checks

| Feature                                  | Status      | Notes |
|------------------------------------------|-------------|-------|
| Material removal                         | done        | Every program simulated against the stock ([simulation.md](docs/simulation.md)). |
| Gouge check                              | done        | Nothing is posted that cuts below the part. |
| Rapid, plunge and feed checks            | done        | CC101 to CC106 ([jobs.md](docs/jobs.md#checks)). |
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
- **`vendor/` patches opencascade** (the MSVC fix, and each B-rep face's triangle
  count) for `camcraft-import`'s `opencascade` feature, kept to cross-check ogeom.
  Delete the feature, `vendor/` and its `[patch.crates-io]` block together once
  ogeom has run on real jobs.
- **There is no CI yet**, so `scripts/check.sh` is the only
  thing between a mistake and `main`. Run it unpiped: piping through `tail` buffers
  the whole run and hides progress.
