# 0002 — Picked features: topology, saved picks, and version 2

**Status:** decided, 2026-09-23 · **Milestone:** 0.2, ahead of the 2D operations

## Question

A person machining a part points at *features*: a hole's centre for the work zero,
the floor of a pocket, the edges a trace follows. Until now nothing could be pointed
at. The engine sees one triangle mesh per model, with no faces, edges or corners;
STEP import threw the B-rep face of every triangle away, and STL never had one.

So three things had to be decided: where faces, edges and corners come from; how a
pick is saved in a job so that it still means the same thing next week; and what an
older build does with a job that holds picks.

## Where features come from

A model is now a `Body`: its mesh and its `Topology` (`camcraft_geom::topology`).
The topology is a face label per triangle; edges and corners are derived from the
labels the same way for every source.

- **STEP.** The vendored OpenCascade mesher records how many triangles each B-rep
  face gave, so every triangle keeps its face, numbered in the order the kernel
  explores the shape. That order depends on the file alone, so a face keeps its
  number when the job's tolerance changes the tessellation. The fixtures hold this:
  `hinge_jig` and `rail_guide` give the same numbered planes at 0.01 and 0.05 mm, and
  every closed fixture welds into a solid in which each edge runs between exactly two
  faces. That last property relies on OpenCascade giving both faces along a B-rep
  edge bit-identical nodes, which the exact-bit weld needs; it does.
- **STL.** Faces are found from the triangles. Planes first: a patch grows from the
  largest triangle across neighbours within 0.5° and on the seed's own plane, so a
  gentle curve cannot drift into a plane. A patch is kept as a plane if it fills its
  whole crease-bounded region, or if it is four times the largest patch it runs into
  smoothly: a floor beside its fillet stays a floor, while a cylinder's facets, all
  alike, do not become planes. What is left joins into curved faces across folds
  gentler than 30°. Winding is followed through the mesh, so flipped triangles do not
  split a face.

Edges are chains of mesh edges between two faces, or along an open boundary. They
end where three faces meet, where the faces either side change, or at a turn
sharper than 30°. Two B-rep edges between the same pair of faces that meet smoothly
come out as one chain. Nothing downstream needs them apart, and round edges are
still recognised as circles (a Kasa fit, every point within the fit tolerance),
which is what a work zero on a hole's centre needs.

Everything is deterministic: sorted edge maps, no hashing, and fixed starting points
for chains.

## How a pick is saved

```toml
[[setup.operation.geometry.faces]]
model = "part"
index = 12
at = [30.2, 14.0, 12.7]
```

`at` is a point on the feature, in the model's own coordinates: where the person
clicked. **It is what the pick is.** `index` is only a quick way to it.
`camcraft_core::features` finds a pick in this order:

1. By number, if that feature still passes through `at`.
2. By position: the feature of that kind through `at`. An edge must be the only one
   there, or the pick is ambiguous.
3. Otherwise it is refused, by name. A face the file named that has kept its number
   but moved away from `at` is `Moved`; nothing through the point is `Gone`.

The tolerance is the job's tolerance plus the model's fit tolerance. STEP is
tessellated to a quarter of the job's tolerance, so two tessellations of one surface
agree to half of it.

Step 3 was first planned as "accept by number, with a warning", but a face that
moved and a different face that took its number in a CAD edit look identical from
here. Cutting the wrong face, or the whole part where one face was meant, is exactly
the silent wrong answer this project refuses. So it is an error, and the fix is to
pick again.

The engine and the app find picks with the same functions, so what is lit in the view
is what gets cut. A stock point is not a feature reference at all: picking a corner
or side of the stock writes the existing `Origin::BoxPoint { of = "stock" }`, which
follows the stock when it changes size.

## What a pick does

- **Work zero**: `Origin::Point`, a vertex, a round edge's centre, or an open edge's
  middle, resolved in `prepare`.
- **Operation geometry**: `faces` and `edges` in `Geometry`, not in the strategy,
  because each strategy gives them its own meaning. For Parallel and Z-level rough
  they make a machining boundary: the picked faces seen from above, joined with the
  areas the picked edges close (read even-odd, so an inner loop is a hole). The
  boundary is then offset for `containment` (tool centre, whole tool inside, or tool
  reaching in from outside) and `margin`. Parallel clips each scanline exactly to the
  boundary; roughing intersects it with the stock's footprint. Pocket, trace and
  engrave will read the same fields.

## Versions

A build from before this change ignores fields it does not know. Reading a job with
picked faces, it would machine the whole part. So:

- `JOB_VERSION` is 2, and every version from 1 to 2 is read.
- `Job::required_version` is 2 only if the job holds a pick, and a job is written at
  that version. Every job without picks stays version 1, byte for byte, and an older
  build still reads it. An older build refuses a job with picks by name rather than
  misreading it.

## Consequences

- `vendor/opencascade` now differs from upstream in a second way, `Mesh::face_triangles`,
  and cannot be dropped until upstream has both changes.
- Faces found from STL are a heuristic. Coarse holes (under about twelve facets)
  come out as several planes and are not recognised as circles. Two large planes
  meeting at less than 30° become one curved face. Hovering shows a face before it is
  picked, and faces can be picked several at a time.
- Edge numbers depend on the tessellation, even for STEP. That is why `at`, not
  `index`, is the identity.
- Only the box around all of a setup's stock can be picked from. A single block is
  the common case.
