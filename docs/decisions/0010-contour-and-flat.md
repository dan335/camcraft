# 0010 — Contour and flat finishing, in Z levels

**Status:** decided, 2026-09-28 · **Milestone:** before the first cut

## Question

Scallop and parallel finishing leave anything steeper than a plunge "for a
waterline pass" (CC211), and they are poor on floors: a ball end mill's cusps,
or a flat one riding over a floor's edge. Fusion finishes walls with Contour,
passes round the part at levels a stepdown apart, and floors with Flat, each
flat face cleared at its own height. How is each done with what the engine
already has, and how does that square with decision 0003, that grids are only
for stock?

## Decision

- **Both from the region the tool may not enter.** At any height the tool's tip
  may not be inside `LevelReach::forbidden(z)`: the part's shadow above that
  height, grown by the tool band by band, with the stock to leave in the tool.
  Pocket clearing already cuts to it. Nothing here is gridded, so 0003 holds.
- **Contour: that region's rings.** Its boundary is where the tool at that height
  meets the part, so a contour pass is its rings, kept to the area. There is
  one level a stepdown apart from the top down, the last at the bottom, and
  one at each flat face's height (`adaptive::flat_heights`), so a pass runs
  round the foot of every wall that stands on one. Every point of a pass is at
  its level: it goes round the part, never over it.
- **Pulled onto the part, so the region may be coarse.** The region is
  conservative, larger than the true one by its bands and its arcs. So each
  point of a ring is pulled towards the part by bisection on exact drop-cutter
  queries, to within 2% of the tolerance of where the tool first touches, and
  never past. It is kept clear: a point already inside is pulled out.
  - Between each two points, the chord's middle is pulled too, and kept where
    it is more than a quarter of the tolerance off the chord, then the halves
    are looked at the same way. A chord into a convex wall has its middle's
    point outside it, so this bounds the stray both ways.
  - Since the part places every point, the region only has to find the walls.
    It is drawn at ten tolerances, or a quarter of the tool if less: a tenth
    of the bands. It misses only gaps narrower than that which the tool would
    have squeezed into, which leaves material and never gouges.
  - A ring is thinned to a quarter of the tolerance before it is pulled. The
    region's rings carry every facet of the part's shadow, and each point
    costs a dozen drops; the pull puts back what the part needs. The direction
  is taken across neighbours at least half a tolerance away, because the
  clipper's points can sit a tick apart, and a normal from them points
  anywhere. A ring is kept to the area *before* it is pulled, because the
  clipper rounds what it returns to its tick (0.1 µm). That is harmless on the
  region's own rings, which are on the tick already, but it moved pulled
  points up to 70 nm into a sphere.
- **Contour: slopes, order and direction.** The slope range works as scallop's
  and the pattern strategies' do: on the slope of the cutter-location surface,
  measured by central differences of the drop-cutter. It leaves out shallow
  stretches for a scallop or parallel pass. Fusion's pairing is scallop from 0°
  to 30° and contour from 30° to 90°. Each ring has the part on its left, so
  climbing with a clockwise spindle cuts it against the way it runs. By depth,
  each level is cut before the next; by area, each piece of the lowest level's
  footprint is cut all the way down before the next.
- **Flat: between two of those regions.** At a flat's height `z`, the region at
  `z` keeps the tool off every wall standing on the flat, since a face lying
  exactly at a height does not block it. The region half a tolerance lower
  shows where the flat is under the tool. What lies between the two is where
  the tool rests on the flat and touches nothing else. The lower region is
  shrunk by what it may overstate, so a ring along an edge with nothing
  beyond it still rests on the flat.
- **Flat: cleared as a pocket clearing level is.** Each piece is cleared with
  rings a stepover apart in from its edges, `pocket_clearing::Concentric`, now
  shared. The ring along the walls is the region's boundary itself, drawn as
  finely as the region was. Outside in ends on the walls; inside out starts on
  them. The highest flat is cut first.
- **Linking.** Contour stays down over the part as the other finishing
  strategies do, stepping down the wall between levels. Flat links at its
  level, as pocket clearing does, and ramps in.

## Rejected

- **Waterlines from a heightmap's isolines.** Marching squares on a grid of
  drop-cutter heights is 0003's staircase, which is exactly what it rejected.
- **Contour from the pocket clearing path.** Pocket clearing's ring on the
  walls is the same ring, but clipped to the material and cut from the middle
  out. A finishing pass wants every level, whatever the stock, and wants it
  round the whole part.

## Consequences

- CC209 (nothing to finish) and CC211 (too steep, for a contour pass) now name
  whichever finishing strategy raised them. CC215 notes contour points that
  could not be pulled onto the part: still clear of it, and within the
  tolerance.
- **The regions are built all at once.** A ball's corner is taller than a
  contour's step, so asked level by level the next level down starts above
  where the last one ended, and the mesh's shadows are built again from the
  top every level. `LevelReach::forbidden_all` asks every band of every level
  once, highest first: the same regions, the shadows built once.
  - On a sphere this took the tests from 82 s to 33 s. Pulling in parallel,
    point by point, and thinning first took a contour of a 380 mm bowl from
    467 s to 23 s, and one of the lego brick from 64 s to 7 s, with no gouge
    either time. A band whose shadow is the one above's adds nothing and is
    skipped, which is most of them on walls and flats.
  - Pocket clearing asks level by level still. A shadow built from the top can
    differ, point for point, from one built up to the same height, so moving
    it would change its paths.
- Contour's cost is otherwise pocket clearing's per level: the banded region. A
  ball at a fine tolerance has many bands. Exact slicing of the offset
  surface, still open under 0003, would make it cheaper.
- Still to do: Fusion's contour ramps from one level to the next, a helix round
  the wall, rather than stepping down; smoothing; a flat's own multiple depths.
