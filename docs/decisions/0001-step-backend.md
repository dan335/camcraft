# 0001 — STEP backend: OpenCascade, not truck; since 2026-10-12, ogeom

**Status:** decided, 2026-09-22; superseded by ogeom 2026-10-12 (see Since) · **Milestone:** 0.1-a spike

## Question

STEP is how parts arrive from Fusion and Plasticity, so it is a first-class input,
not an optional feature. The open question was only the *backend*, and therefore what
the build prerequisites are.

Two candidates:

- **`truck-stepio`** — pure Rust. Zero build cost, `cargo build` stays trivial.
- **`opencascade` / `occt-sys`** — Rust bindings to OpenCascade. Needs cmake and a
  C++ toolchain.

Nothing else on crates.io is a real contender. `ruststep`, `iso-10303`,
`stepperoni` and `openbim-step` are Part 21 *parsers* — they read the file format but
do not evaluate NURBS surfaces or tessellate a B-rep, which is the actual work.

## Pass criteria

Stricter than "it runs". For a drop-cutter a silently dropped face is a hole, and a
hole is a gouge. So:

1. Parses the file.
2. Tessellates **every** face — zero dropped surfaces.
3. Lands inside the convex hull of the control points. A NURBS surface is
   mathematically contained in that hull, so this is a free correctness oracle that
   needs no reference mesh.
4. Applies the STEP unit context. Five of six committed fixtures are in inches.
5. Fast enough to iterate on.

## Result

Measured over `fixtures/step/` (see its README for what each part exercises), plus one
commercial part referred to below as **`sculpted-panel`** — a large freeform panel,
5 faces, 3 `B_SPLINE_SURFACE_WITH_KNOTS`, 2104 control points. It is not in this
repository: it was removed and scrubbed from the history because it is customer work,
and it now lives only in `CAMCRAFT_FIXTURES`. It is named here because it is the part
that produced the disqualifying result.
truck at tolerance 0.1, OCCT at 0.01 — **a ten times finer tolerance**, and it still
wins on wall clock.

| part | truck | OCCT |
|---|---|---|
| `hinge_jig` (39 planar faces) | 443 ms, 74 tris | 7 ms, 74 tris |
| `lego_4x2_01` (76 faces, cones) | 664 ms, 384 tris | 185 ms, 10,090 tris |
| `rail_guide` (134 faces, cones) | 282 ms, 444 tris | 133 ms, 7,628 tris |
| `sled_base` (387 faces, 6 B-spline) | 798 ms, 1,294 tris | 763 ms, 39,780 tris |
| `bowl_01` (revolved + seams) | **114,956 ms**, Irregular | **9,723 ms**, 205,520 tris |
| `sculpted-panel` (3 B-spline, private) | 10,967 ms, **geometry corrupt** | 9,494 ms, 197,490 tris |
| `nist_ftc_06` (AP242) | **parse error** | not retested |

Bounding boxes against ground truth (mm):

| part | true | truck | OCCT |
|---|---|---|---|
| `hinge_jig` | 25 × 32 × 33 | 1 × 1 × 1 | **25.4 × 31.8 × 33.3** |
| `lego_4x2_01` | 101 × 50 × 38 | 4 × 2 × 2 | **100.6 × 49.8 × 38.1** |
| `sled_base` | ≤ 251 × 233 × 76 | 9 × 9 × 3 | **237.0 × 227.0 × 76.2** |
| `sculpted-panel` | ≤ 477 × 281 × 44 | **54 × 105 × 2** | **309.8 × 272.3 × 44.4** |

## Why truck fails

**It does not corrupt simple parts.** On analytic prismatic geometry — planes, cones,
cylinders — truck dropped zero faces on every fixture and was competitive on speed.
Had the fixture set stopped there it would have passed.

It fails precisely on curved surfaces, which is the entire reason this project exists.

1. **Corrupt NURBS refinement.** On `sculpted-panel`, truck returns
   a Y extent of 105 mm where the control-point hull caps it at 11.06 inches. That is
   ~9.5× *outside* the convex hull, which a NURBS surface cannot leave. At coarse
   tolerance (0.5) the same file lands inside the hull; refinement is what breaks it.
   On a machine this is a 2.6 m Y move.
2. **Unusable on revolved surfaces.** `bowl_01` is 11 KB with 5 faces and takes
   115–128 s. Triangle counts are non-monotonic in tolerance — 17,280 tris at 0.1 but
   12,824 at 0.05 — so the tessellator is unstable, not merely slow.
3. **No unit handling whatsoever.** `SI_UNIT`, `CONVERSION_BASED_UNIT` and
   `LENGTH_UNIT` do not appear anywhere in the crate. Inch files come back 25.4× small.
4. **Cannot read AP242.** `ruststep` fails to tokenise the DATA section of
   `nist_ftc_06.step`. AP242 is the current standard.

Items 3 and 4 we could work around. Items 1 and 2 are its surface evaluator, and we
cannot fix those without becoming the maintainers of a CAD kernel — which is the thing
this project exists *not* to do.

## Decision

**`opencascade` 0.3 / `occt-sys` 7.8.1 from crates.io.** No fork and no git dependency;
the existing `dan335/opencascade-rs` fork is not needed.

Accepted costs, all of which were actually hit and resolved during the spike:

- **cmake and a C++ toolchain become documented build prerequisites.** On Windows,
  cmake must find `cl.exe`, which means building from an MSVC environment.
- **The first OCCT build is slow.** It is a one-off per machine. When CI is restored
  (there is none while the repo is private) it should cache the OCCT artifact.
- **Windows needs explicit system-library links** — `advapi32` and friends, for
  registry and SID symbols that `occt-sys` does not propagate. Sixteen unresolved
  externals without it.

## Consequences

- **`camcraft-import` stays a leaf crate that only `camcraft-cli` depends on.** This
  was already the plan; it now carries real weight, because `geom`/`sim`/`core`/`post`
  must keep building and testing without paying the OCCT cost.
- **The unit context is still ours to verify.** OCCT converts to mm correctly, but per
  the correctness rules the importer must confirm the unit it resolved and hard-error
  on an unknown one rather than defaulting. Silently assuming mm is the failure mode
  that destroys parts.
- **The convex-hull check becomes a permanent import test**, not just a spike tool. It
  is cheap, needs no reference mesh, and it is what caught this.
- **The public fixture set can no longer reproduce finding 1 on its own.** The part
  that exposed it is commercial and was removed from this repository and scrubbed from
  its history. A synthetic sculpted-surface fixture with a comparable control-point
  count should replace it; until then that regression is only caught on a machine with
  `CAMCRAFT_FIXTURES` set.
- **Licensing was not weighed here, and must be before binaries are distributed.**

## Since

- **2026-09: vendored and patched.** "No fork" did not last: opencascade 0.3.0 does
  not build with MSVC, and picking needs each B-rep face's triangle count, which
  upstream does not give. `vendor/` holds the crates.io sources with upstream PR #234
  and that count added, patched in from the root `Cargo.toml`, until a release
  includes both.
- **2026-09: the app depends on it too.** `camcraft-import` is reached through
  `camcraft-session`, which both binaries use. The engine crates still never pay
  for OCCT.
  `opencascade` and `occt-sys` are LGPL-2.1, and OpenCascade itself is LGPL-2.1
  with an additional exception. `occt-sys` links it statically. camcraft is MIT or
  Apache-2.0. A static LGPL link in a distributed binary brings obligations, such as
  letting users relink against a modified library, that a permissive project does
  not otherwise have. The options include dynamic linking, shipping object files, a
  feature flag, or accepting the obligations. None of this blocks publishing the
  source.
- **2026-10-12: re-tested, still no.** solidcraft (bherbruck/solidcraft at 07e263d)
  reads STEP with its own reader and meshes with a truck-meshalgo patched against
  runaway B-spline searches. `scripts/kernel-trial.sh` put it, upstream truck and
  OpenCascade through camcraft's import checks and measured each against a 0.001 mm
  OpenCascade mesh, on every fixture and the sculpted panel at 0.0125, 0.05, 0.1 and
  0.5 mm. The trial has since become its own project,
  [step-conformance](https://github.com/dan335/step-conformance), which scores any
  STEP reader that way, and the checks their own crate, `step-check`. A reader passes when the checks pass and it lands within four times the
  tolerance, since OpenCascade itself reaches 3.4 times on the bowl.

  | reader | passes | where it fails |
  |---|---|---|
  | OpenCascade | 32 of 32 | |
  | solidcraft | 29 of 32 | sculpted panel at 0.05, 0.1 and 0.5 mm: surface 39 to 108 mm off, on a B-spline face closed round the part |
  | truck 0.3/0.4 | 23 of 32 | bowl, sled base and panel: 60 s timeouts, or surface 5 to 77 mm off |

  solidcraft matches OpenCascade on everything else, gives the same bytes every run
  and needs no cmake, so it is the reader to switch to once it holds on the panel.
  The run also showed two gaps in the checks, both closed: they skipped any file with
  a placement, the panel among them, and they relied on the kernel to refuse a face
  it could not mesh, where truck hands one back empty. Since then every face on a
  plane, cylinder, cone, sphere or torus is also held to its surface, which sees
  surface wrong inside the part. A face on a B-spline or a surface of revolution is
  still held to its box alone, so truck's bowl, 53 mm off on its revolved rim, passes
  the checks, and only the trial's distance catches it.
- **2026-10-12, later: ogeom leads.** step-conformance now scores 24 parts at the
  four tolerances, 96 runs a reader. Its published results: OpenCascade 96,
  solidcraft (f0292b1) 80, truck master 79, ogeom 0.9.16 75, truck 0.3/0.4 70,
  brepkit 38 (AGPL, so scored only). ogeom's maintainer then fixed the B-spline
  faces closed round with no seam, which came back with no triangles, and the
  dome bounded by its equator, 20 mm off, both reported in ogeom-rs#202. Its main
  (63da721, not yet released) passes 91 of 96 and nothing that passed before got
  worse. It still fails the freeform panel at 0.0125, 0.05 and 0.1 mm, 0.6 to
  5.5 mm off, and a cone to a point at 0.0125 mm, 0.09 mm off, whose fix
  ogeom-rs#204 plans. No reader but OpenCascade passes the freeform panel at
  every tolerance.
- **2026-10-12, last: switched to ogeom.** ogeom 0.10.0 fixed the panel and the
  cone (ogeom-rs#206) and passes all 108 of step-conformance's runs, and the
  private sculpted panel at all four tolerances. `load_step` reads with it, and
  OpenCascade stays behind `camcraft-import`'s `opencascade` feature, with a test
  that holds the two to the same faces and extent on every fixture, until ogeom has
  run on real jobs. cmake and the C++ toolchain are no longer needed, and a binary
  links no LGPL code. What making it the importer added:
  - **A chord of a third of the tolerance.** ogeom's chord bounds its edges, but
    its faces stand off doubly curved surfaces by up to 3.1 times it (ogeom-rs#212).
    At a third, every run and the private panel came within 1.04 times the
    tolerance, and everything still meshes faster than OpenCascade did.
  - **Every curved face measured against its surface.** Each triangle's middle and
    each side's midpoint are projected onto ogeom's surface for the face, and the
    import is refused past the tolerance. Measuring to the point at the middle
    parameters instead was wrong twice over: where a surface's parameters run
    unevenly it counts sliding along the surface as distance (5.9 times the
    tolerance on the private panel, which really lies within it), and beside a
    cone's apex, where a vertex's `u` is arbitrary, it finds the far side.
  - **It refuses the bowl below a tolerance of about 0.004 mm.** ogeom stops a
    face's grid at 512 steps a direction, which the bowl's rim, 188 mm out, needs
    more of; it stands 0.0124 mm off at a chord of 0.0033 and of 0.00083 alike.
    With the cap raised to 8192 it lands 0.002 mm off at 0.00083, faster. Reported
    as ogeom-rs#216. OpenCascade, asked for 0.0025, gave 0.009 mm on the bowl and
    0.010 on the private panel: it missed fine tolerances too, unmeasured.
  - **Faces' copies of a point within 1e-9 mm are joined.** `sled_base` writes one
    vertex as 0.0399999999999995 in and the line through it from 0.04, and ogeom
    takes a face's corner from the curve, so faces meeting there disagree by
    1.3e-14 mm and the exact-bit weld left the solid open (ogeom-rs#217).
  - **Cylinders and cones are triangulated again from their boundaries.** At a
    third of the tolerance ogeom gave `sled_base` 206,058 triangles where
    OpenCascade gave 33,824, 83 percent of them on cylinders and cones, in rows
    along the rulings that hold no part of the tolerance (ogeom-rs#213). The
    example job's roughing, which scales with the triangles, took it from 7.5 s
    to 43.6 s. `camcraft_import::ruled` lays such a face again on its boundary
    points alone, constrained Delaunay in its parameters, and keeps ogeom's
    triangles for any face whose new ones measure past the tolerance (2 faces of
    `sled_base`, none elsewhere). `sled_base` comes to 46,680 triangles, the lego
    to 9,780 (OpenCascade 9,064), and the job to 9.5 s, with the same passes.
  - **The bowl's file has a hole.** Its inner floor is bounded by a circle
    4.7e-6 mm across on the axis that no face closes. ogeom meshes it as written;
    OpenCascade's mesh of it came out closed.
- **Revisit when** ogeom-rs#216 is fixed: move the bowl's fixture test back to
  0.01 mm. When ogeom meshes ruled faces without the rows (ogeom-rs#213), delete
  `ruled`; when its faces hold to their chord (ogeom-rs#212), mesh at the
  tolerance rather than a third of it. Delete the `opencascade` feature and `vendor/` once ogeom has run on
  real jobs.

## Implemented (0.1-d, 2026-09-22)

`camcraft_import::step::load_step`, wired into every CLI subcommand by file
extension. What building it on real files added to the above:

- **The containment check needed more than the points' box.** Grown by the largest
  circle radius, it still wrongly failed `bowl_01`, whose rim is a surface of
  revolution reaching 190 mm from its axis on a side where the file has no points.
  Each revolved surface is now bounded by a ball around its axis point, as far as its
  profile reaches, and each extrusion by its vector. A check that fails correct
  geometry gets loosened until it means nothing, so it was made right instead.
- **A second check bounds the mesh from below.** Containment cannot catch a unit
  error that makes the part too small, because a shrunken mesh still sits inside the
  hull. B-rep vertices lie on the part, so the mesh must reach every one of them.
  Both failure directions are covered by a test that scales a real mesh by 25.4 each
  way.
- **The unit is read from the file independently of OpenCascade,** from the
  `GLOBAL_UNIT_ASSIGNED_CONTEXT` rather than the first unit defined. Fusion exports
  define inch, millimetre and metre and assign only one. Unknown or conflicting
  units are errors, never millimetres.
- **The file's face count is checked against the kernel's,** since a face dropped in
  transfer is a hole.
- **The tessellation tolerance is a quarter of the job tolerance.** The simulator
  verifies against the mesh, so tessellation error is invisible to it and must stay
  a small part of the budget.
- On an M-series Mac the first OpenCascade build took 4 min 49 s in release and
  under a minute more for the dev profile. `sled_base` tessellates in 0.2 s at
  0.0125 mm, and `bowl_01` in 1.6 s.
