# STEP fixtures

Real parts, kept here so the test set can't evaporate from a Downloads folder.
Chosen to span what the importer actually has to survive, not what's convenient.

| file | units | faces | surfaces | size (mm) | what it exercises |
|---|---|---|---|---|---|
| `hinge_jig.step` | inch | 39 | planar | 25×32×33 | smallest sane case |
| `lego_4x2_01.step` | inch | 76 | plane, cone | 101×50×38 | cylinders and fillets |
| `door_handle_guide.step` | inch | 89 | plane, cone | 60×221×45 | a real router jig |
| `rail_guide.step` | inch | 134 | plane, cone | 237×242×13 | many faces, all analytic |
| `sled_base.step` | inch | 387 | plane, cone, **6 B-spline** | 251×233×76 | largest; mixed analytic + NURBS |
| `production_large_single_03_remake.step` | inch | 5 | **3 B-spline** | 477×281×44 | **the 3D CAM case** — sculpted surface, few faces, 2104 control points |
| `bowl_01.step` | mm | 5 | **2 surface-of-revolution + seam curves** | 380×380×63 | revolved geometry and seam handling |

## Two things this set is designed to catch

**Units.** Six of seven are in inches (`CONVERSION_BASED_UNIT('inch')`). An importer
that ignores the STEP unit context produces geometry 25.4× too small and a program
that will happily drive a cutter through the spoilboard. `bowl_01` is in mm
specifically so a hardcoded inch conversion fails just as loudly as a missing one.

**Curved surfaces.** Analytic prismatic parts are the easy case and most of this set
passes trivially. `production_large_single_03_remake` and `bowl_01` are the parts that
separate a usable backend from an unusable one — see
`docs/decisions/0001-step-backend.md`.

## Invariant every importer must satisfy

A NURBS surface lies within the convex hull of its control points. So for any of these
files, the tessellated bounding box must fit inside the bounding box of the raw
`CARTESIAN_POINT` values (after unit conversion). That check is cheap, needs no
reference mesh, and catches corrupt surface evaluation outright.

## Before this repo goes public

`production_large_single_03_remake.step` is real commercial work and
`sled_base.step`, `door_handle_guide.step` and `rail_guide.step` are shop jigs. The
repo is private today, but the project is intended to be open source. Either swap
these for equivalent synthetic parts before flipping visibility, or move the set
behind a `CAMCRAFT_FIXTURES` path and keep it out of the tree. Deleting them later
does not help — they stay in the git history.
