# STEP fixtures

Real parts, kept here so the test set can't evaporate from a Downloads folder.
Chosen to span what the importer actually has to survive, not what's convenient.

| file | units | faces | surfaces | size (mm) | what it exercises |
|---|---|---|---|---|---|
| `hinge_jig.step` | inch | 39 | planar | 25.4×31.8×33.3 | smallest sane case |
| `lego_4x2_01.step` | inch | 76 | plane, cone | 100.6×49.8×38.1 | cylinders and fillets |
| `door_handle_guide.step` | inch | 89 | plane, cone | 57.1×215.9×33.0 | a real router jig |
| `rail_guide.step` | inch | 134 | plane, cone | 237.0×40.6×12.7 | many faces, all analytic |
| `sled_base.step` | inch | 387 | plane, cone, **6 B-spline** | 237.0×227.0×76.2 | largest; mixed analytic + NURBS |
| `bowl_01.step` | mm | 5 | **2 surface-of-revolution + seam curves** | 380.0×380.0×20.0 | revolved geometry and seam handling |

Sizes are the extent of each file's own B-rep vertices, read straight from the text,
and `crates/camcraft-import/tests/step_fixtures.rs` holds the importer to them. An
earlier version of this table gave the box around *every* `CARTESIAN_POINT`, which
includes placement origins and circle centres that are not on the part: `rail_guide`
read 242 mm deep instead of 40.6.

## Two things this set is designed to catch

**Units.** Five of six are in inches (`CONVERSION_BASED_UNIT('inch')`). An importer
that ignores the STEP unit context produces geometry 25.4x too small and a program
that will happily drive a cutter through the spoilboard. `bowl_01` is in mm
specifically so a hardcoded inch conversion fails just as loudly as a missing one.

**Curved surfaces.** Analytic prismatic parts are the easy case and most of this set
passes trivially. `sled_base` and `bowl_01` are the parts that separate a usable
backend from an unusable one — see `docs/decisions/0001-step-backend.md`.

## Invariants every importer must satisfy

All three are checked on every STEP import, not only in tests (`crates/step-check`,
which any importer can use).

- **Every face arrives, with triangles.** The kernel must give at least as many faces
  as the file defines, and every one of them must have triangles. A kernel that gives
  up on a face may hand it back empty instead of refusing, and the count alone then
  passes a part with a hole in it: upstream truck's plain mesher did exactly that to
  half the sculpted panel.
- **Nothing lands outside the file's own geometry.** A NURBS surface lies within the
  convex hull of its control points, so the mesh must fit inside the box around the
  raw `CARTESIAN_POINT` values, after unit conversion. The box has to be grown for
  analytic geometry: a circle leaves it by up to its radius, and a surface of
  revolution can land on the far side of its axis from every point in the file. The
  rim of `bowl_01` is a torus reaching 190 mm from its axis, while every point in
  the file sits on one side of it. This check catches corrupt surface evaluation
  outright.
- **The mesh reaches every vertex.** B-rep vertices lie on the part, so the mesh must
  extend at least as far as they do. The first check cannot catch a unit error that
  makes the part too *small*, because a shrunken mesh still sits inside the box. This
  one does.

Both geometric checks are made where an assembly puts each component: the file's
points are moved by each component's placement, read from the product structure, and
the points that only place things are left out of the box. Before that, any file with
a placement skipped the checks, the sculpted panel among them: solidcraft's mesh of it
at 0.05 mm, with surface 108 mm off the part, passed them then and is refused now.
Only placements that cannot be followed, such as mapped items, still skip them.
`crates/camcraft-import/tests/assembly.rs` wraps the hinge jig in an assembly and
holds the placement to OpenCascade's.

The geometric checks are boxes, so they catch surface thrown outside the part, not
surface that is wrong inside it: upstream truck's bowl, 53 mm off at 0.05 mm, passes
them. `scripts/kernel-trial.sh` measures that, against a fine OpenCascade mesh.

## Private fixtures: `CAMCRAFT_FIXTURES`

Some of the most informative parts are commercial work and must never enter this
repository or its history. Tests therefore also sweep any `*.step` under the directory
named by the `CAMCRAFT_FIXTURES` environment variable, and skip that sweep when the
variable is unset. For example:

```
CAMCRAFT_FIXTURES=../camcraft-fixtures-private
```

### Known coverage gap

The part that actually caught the disqualifying `truck-stepio` bug — a large sculpted
panel, 5 faces, 3 `B_SPLINE_SURFACE_WITH_KNOTS`, 2104 control points — is commercial
and now lives only in `CAMCRAFT_FIXTURES`. Nothing in the committed set has that
shape: `sled_base` has B-splines, but buried among 387 analytic faces, and `bowl_01`
is revolved rather than freeform.

So the public set can no longer reproduce that specific corruption class on its own.
Closing this properly means generating a synthetic sculpted-surface part with a
comparable control-point count and committing that instead.

## Before this repo goes public

`sled_base.step`, `door_handle_guide.step` and `rail_guide.step` are shop jigs. Far
less sensitive than a customer part, but still not public work. Decide on them before
flipping visibility — deleting them later doesn't help, because they stay in the git
history.
