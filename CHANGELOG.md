# Changelog

What changed in each release, newest first. Each release's page
([Releases](https://github.com/dan335/camcraft/releases)) has the downloads and
the same notes at more length.

## Unreleased

- **Adaptive clearing gets Fusion's rest machining**, from the operations before
  it in the setup, each swept exactly with the tool it was cut with, and its
  *Ignore stock less than*.
- **And its minimum cutting radius, machine cavities and order by area.** A
  minimum cutting radius blends every sharper inside corner, leaving it for a
  smaller tool; machine cavities unticked leaves the pockets it could only go
  down into; by area clears each pocket all the way down before the next.
- **Adaptive stays down round what stands between passes**, through what it has
  cleared, rather than lifting wherever the way straight is blocked.
- **Adaptive has no Deepest step cut**, as Fusion's has none: its passes stay on
  their levels.
- **Better help for adaptive's Passes tab**: sketches for fine stepdown, flat
  areas, deepest step cut, both ways, minimum cutting radius and machine
  cavities.
- **Adaptive machines shallow areas**, as Fusion's: where the levels leave a
  terrace wider than the most asked, levels are added between, cutting only
  there. On a serving tray the scallop finish after it takes 15% less.
- **And the rest of Fusion's adaptive settings**: flat area detection finds
  peaks too, with a minimum stepdown, and a minimum axial engagement leaves out
  levels that would only skim; slot clearing cuts a slot along a pocket's middle
  before clearing out from it; and on the Linking tab, the stay-down level,
  stay-down clearance and lift height.
- **Smoothing**, as Fusion's, on every operation but drilling: runs of short
  moves fitted with arcs and longer lines within a tolerance, before linking,
  so what is simulated and checked is the program. On a serving tray the
  adaptive program went from 34 MB to 6 MB, and 1.3 million lines to 181,000.
- **Smoothing fits helices**, about Z: a ramp round a ring is a few arcs
  falling as they turn, rather than lines.
- **Feed optimisation**, as Fusion's, on every operation but drilling and
  adaptive clearing: cuts slowed into and out of sharp corners and round tight
  arcs, only those turning into the material unless asked.
- **A link staying down keeps clear of the material** between the passes it
  joins, where it could come a tenth of the tool into it.

## 0.4.0 — 2026-11-04

- **Toolpaths show as they are worked out.** Each operation's passes are drawn as
  its strategy finds them, in a quieter green than a cut, until the operation is
  made and its real toolpath replaces them.
- **Every 3D finish finishes the walls.** Parallel, spiral, radial and morphed
  spiral leave ground steeper than a plunge to rings round the walls, as scallop
  does ([decision 0014](docs/decisions/0014-walls-for-every-finish-over-the-part.md)).
  On a serving tray each now leaves 11 columns more than 1 mm above the part,
  where it left between 1,263 and 7,690. A slope range that stops short of
  vertical turns it off. The rings have a level at each flat face, so a floor
  beside a wall is finished too: the router jig's finish leaves 7,898 columns
  more than 1 mm above the part, the tallest 2.5 mm, where 0.3.0 left 11,806
  and the tallest 52 mm.
- **Roughing large parts is faster.** Adaptive keeps the material grown by the
  tool tile by tile: on the tray the slowest level went from 190 s to 80 s, and
  the roughing from 246 s to 153 s.
- **Fixed:** a pass that measured itself engaged while taking nothing could run
  to its level's step limit and leave the rest of the level uncut; it now ends
  once it takes less than a sliver over a tool's width.
- **Fixed:** a stay-down link could climb a wall from a ring round its foot,
  where nothing is under the tool, through a convex corner of the part.

## 0.3.0 — 2026-11-03

- **Toolpaths generate by themselves** as editing settles, keeping what still
  stands; operations that read the part alone are worked out at once; a change
  stops only the operations it touches; Stop, and a setting to turn it off
  ([decision 0013](docs/decisions/0013-generating-by-itself.md)).
- **Each operation shows as it is made,** with how far each has got in the tree
  and the operation's panel.
- **A curved part finished as Fusion does it:** adaptive's fine stepdown; a Top
  and Bottom on every 3D finish; finishing ramped into; scallop leaving its steep
  ground to rings round the walls; contour keeping rings along a boundary; a
  short open pass ramped into back and forth. A serving tray's stock now matches
  Fusion's to 0.03 mm at the 99th percentile.
- **The app:** no freeze adding a 3D operation to a large part, or on Save As;
  every input shows and resets its default; typing `.0` no longer completes
  early; a tool import says in full what it skipped.

## 0.2.0 — 2026-10-14

The first release: the desktop app and command line, STEP and STL import,
adaptive and pocket roughing, parallel, scallop, contour, flat, spiral, radial
and morphed spiral finishing, 2D profiles, pockets, facing and drilling, tool
libraries with presets, several setups a job, simulation of every program
against the stock, playback, and posts for Masso, Centroid and GRBL.
