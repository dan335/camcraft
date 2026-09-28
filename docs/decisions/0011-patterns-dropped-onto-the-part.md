# 0011 — Spiral, radial and morphed spiral: patterns dropped onto the part

**Status:** decided, 2026-09-28 · **Milestone:** before the first cut

## Question

Fusion finishes round shapes with a Spiral about a centre, Radial spokes out
from it, and a Morphed Spiral that turns from the boundary's shape into a
centre. Each is a pattern laid out seen from above and dropped onto the part,
as Parallel's lines are. How much of parallel and scallop do they share, and
how is a spiral that follows any boundary laid out?

## Decision

- **One pipeline for every pattern, `projected::project`.** A strategy lays out
  its pattern as lines in the plane, in the order they are cut and each the way
  it runs. The pipeline does the rest in one place:
  - it thins the lines within a quarter of the tolerance, as scallop does;
  - it keeps them to the slopes asked for, then to the area, in order along
    each line, since the clipper returns pieces in an order of its own;
  - it cuts each on the part with `parallel::sweep_polyline`, exact drops
    refined to the tolerance and broken at walls, gaps and plunges;
  - it writes each pass with `projected::Emit`, which parallel and scallop
    now use too.

  Nothing reorders the passes afterwards: the pattern's order is its own.
- **Slopes for everyone, `slope.rs`.** Scallop's `Sloped` moved out of it.
  A pattern's slope is the cutter location's, as scallop's grid measures it.
  It is measured by central differences of the drop-cutter at each segment's
  middle, a third of a stepover apart, and only when a range is asked for.
- **Spiral.** Archimedean, a stepover further out each lap, from the inner
  radius to the outer (by default out to the area's furthest corner). Each step
  is sized from the spiral's own radius of curvature, which near the centre is
  not its radius, so its chords stay within a quarter of the tolerance. Going
  outward, what is still to finish lies outside, so it turns anticlockwise to
  climb with a clockwise spindle; inward, the other way.
- **Radial.** Spokes from the inner radius to the outer, their angle step the
  stepover at the outer radius, back and forth or all one way.
- **Morphed spiral: a blend of two distances.** Over each piece of the area,
  holes filled, two planar distances are marched across a grid within the
  piece: from its boundary and from its centre. The centre is given, or else it
  is the point furthest from the boundary. The blend `u = d_b / (d_b + d_c)` is
  0 on the boundary and 1 at the centre. Its level lines are rings that turn
  from the boundary's shape into a point, and they go round a concave corner
  rather than across it.
  - **As many rings as keep them a stepover apart everywhere.** Across each
    triangle a step of `u` is `1/|∇u|` long, so the count is the widest of
    those over the stepover. The spacing is then no more than the stepover
    anywhere, and the report says what it came to (`widest`, CC213). The count
    is capped at twenty times the plain one; an L-shape needs about four times.
  - **One spiral through them.** A ray from the centre cuts each ring at a seam
    (on each ring, the crossing nearest the last ring's). Each lap goes from one
    ring's seam round to the next's, the same fraction of the way along both
    and of the way between them. Where a level of the blend is more than one
    ring, as a very concave piece can make it, that piece is cut in rings
    instead (CC214).
  - **The grid places the rings and nothing else.** This is the second
    exception to 0003, bounded as 0009's is. The first ring is the boundary
    itself, and every point is cut on the part exactly. `field::AreaGrid`, lifted
    out of scallop's field, builds the grid for both.

## Rejected

- **Scaling the boundary into its centre.** That is exact for a star-shaped
  boundary, but a concave one's scaled copies cross their own boundary.
- **A harmonic blend (Laplace's equation).** Its level lines are guaranteed to
  be single rings, but they crowd towards the centre like a logarithm. So the
  count that keeps the outer laps a stepover apart puts hundreds at the middle.
  The blend of distances has even spacing on a convex piece and is cheap with
  the march scallop already has; the cases it cannot do fall back to rings.

## Consequences

- **In parallel, a stretch at a time.** A spiral is one line, so the pipeline
  works in parallel over the stretches the area cuts it into, not the lines.
  A long stretch is sampled 512 segments at a time, each from its first point
  afresh, then joined and broken as one. The stretches are the line's own,
  whatever the number of cores, so the result is too.
- **A long piece gets more laps than it needs across its narrow way.** Morphing
  into a point, the blend changes slowly along a long piece's middle, so the
  laps that are a stepover apart there are closer across it. A 110 × 60 mm
  rectangle takes 229 laps at a 1 mm stepover where its width alone needs 30.
  The pattern itself takes a tenth of a second; the time is in cutting it.

- Parallel's and scallop's output is unchanged, bit for bit, by the move to
  the shared emitter, slopes and field. That was checked on a bowl roughed,
  cleared, parallel- and scallop-finished.
- The pattern strategies share the linker's stay-down-over-the-part arm with
  parallel and scallop.
- Still to do: picking the centre on the model; Fusion's spiral limits by
  contact point; smoothing.
