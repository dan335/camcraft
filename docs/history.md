# Things that were wrong and are now right

Worth knowing, because each was found by looking rather than by a test passing.

- **Roughing followed a grid.** Its rings were marching-squares contours of a
  cutter-location grid: a staircase on every curved wall, and far less cleared
  than could be. See [decision 0003](decisions/0003-grids-only-for-stock.md).
- **Plunges got through finishing, split small.** A steep face that is straight
  passes refinement's chord test at once, so its depth was never compared with
  `max_cut_step`, and the check afterwards judged each move alone. On
  `mount_rush.stl` two moves fell 1.44 mm over 0.07 mm of travel at cutting feed,
  and the engine only warned while the viewer of the day called it a failure.
  Descents are now measured continuously, finishing breaks its passes at them, and
  the gate refuses what remains. The sled's finishing now leaves nothing past the
  allowance (deepest 0.48 mm), where it used to warn of 0.65 mm.
- **Rapids could clip terraces.** The linker's clearance looked at five points under
  the cutter, centre and compass, and a rapid running tangent to roughing's
  terraces passed material on its diagonal. It now takes the highest cell anywhere
  under the swept disc. Hidden until roughing actually cleared material.
- **Cut moves that were really plunges.** On the router jig, parallel finishing
  emitted moves descending 48 mm in 0.01 mm of travel — a full-depth plunge at
  cutting feed. Every point was exactly on the CL surface and every existing check
  passed; only the *connection between* correct points was unmachinable. Found by
  reading real output. Passes now break at walls and at gaps in coverage.
- **Marching squares emitted nothing for a uniformly high field**, because such a
  field has no boundary inside the grid. Roughing read that as "nothing to clear" —
  the difference between clearing the whole block and clearing none of it.
- **The simulator's stamp loop missed the cell at the cutter's rim.** Under-removal,
  which is the direction that hides a gouge.
- **What was looked at was not what ran.** The web inspector (since removed; the
  app replaced it) had its own single-operation engine path, with a flat clearance
  plane, no pass reordering and a differently sized block, while `job` ran the
  job. Now there is one path, `camcraft_session::run_setup`, and the rapid check
  (CC102) drops the cutter onto the part along every rapid rather than comparing
  against a plane, which stays true when clearance follows the material.
- **The STEP containment check wrongly failed correct geometry.** Growing the box
  of the file's points by the largest circle radius is not enough for a surface of
  revolution. `bowl_01`'s rim reaches 190 mm from its axis, on a side of the axis
  where the file has no points at all. A check that fails correct parts gets
  loosened until it means nothing, so it was made right instead.
- **`fixtures/step/README.md` gave the wrong sizes.** They were the box around every
  point in the file, not the part.
- **The CLI logged to stdout.** Errors now go to stderr.
- **The post golden files were never committed.** `*.nc` in `.gitignore` caught
  `tests/golden/*.nc`, so on a fresh clone every golden test failed.
- **The determinism gate could not detect non-determinism.** It ran the suite twice
  at different thread counts; a deterministic test run twice simply passes twice,
  and nothing ever compared the runs. Replaced with tests that diff output against
  itself.
- **STL format detection.** A nameless ASCII file (`solid` then a newline, as most
  opencamlib samples begin) was read as binary and failed, and a binary file whose
  header began `solid ` would have been read as ASCII. The format is now decided
  before `stl_io` sees the file: a binary STL is exactly as long as the triangle
  count in its header says.
- **opencascade 0.3.0 did not build with MSVC.** It assumes OpenCascade's
  `Handle_X` is an alias of `opencascade::handle<X>`, which Visual Studio makes a
  derived class instead. `vendor/` holds the crates.io sources with upstream PR #234
  applied; see the root `Cargo.toml`.
- **An operation turned off was written at a version an older build would cut.**
  `enabled = false` is now version 4.
- **Posting could leave half a job.** The command line loaded each setup's profile
  inside its write loop, so a bad profile left earlier setups' programs on disk,
  and programs were not written atomically. Posting now renders everything, then
  writes each file whole, for the command line and the app alike.
