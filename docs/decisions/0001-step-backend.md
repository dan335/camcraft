# 0001 — STEP backend: OpenCascade, not truck

**Status:** decided, 2026-09-22 · **Milestone:** 0.1-a spike

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
- **Revisit if** a pure-Rust B-rep tessellator matures. The prize is dropping the C++
  toolchain requirement. `truck` is not close today on curved surfaces.
