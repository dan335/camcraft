# 0031 — Arranged sheets

**Status:** decided, 2026-11-15 · **Milestone:** 0.9

## Question

A router job is often many parts cut from one sheet of board. Each model had
one position, set by hand; a fixed box followed the models rather than held
them, and nothing checked that two parts did not overlap or that they fitted
on the board at all. How should camcraft lay several models, and several of
each, on a sheet of a given size by itself (what Fusion calls Arrange, and
others nesting), and how does what is laid reach the toolpaths and the checks?

## What was decided

**On the stock.** A third stock kind, `kind = "arranged"`, beside
`around-models` and `fixed-box`: a sheet of a size at a corner of its own, which
lays its parts out on itself. The sheet is the material, and a setup already
names the material it cuts, so the setup that uses the sheet is the one that
cuts its parts. Its settings are the sheet's: its size, its corner, the
spacing between parts and the border at its edge, the turns a part may take,
the corner packing starts from, the axis it is used up along, and the face its
parts stand on.

**Always live; positions are never stored.** Where each copy stands is worked
out from the parts' bodies and the sheet's settings whenever either changes,
and is written nowhere: not in the job file, not in the app's document. The
arrangement (`arrange::Arrangement`) is the one record of where a model on a
sheet stands. The command line and the app call the same function
(`arrange::arrange`) with the same bodies, so they lay a sheet alike. A position
cannot be tuned by hand, and none can go stale.

**True shape, on a grid, with an exact check of every layout.** A part is laid
by its outline seen from above (`setup::features::silhouette` of its body as
its own rotation and scale stand it), holes and all. The packer
(`arrange::pack`) covers the sheet with square cells. Each outline is grown by
half the spacing and half a cell's diagonal, and covers every cell whose
centre lies inside that, so two parts whose cells are apart are at least the
spacing apart, and a part whose cells lie on the sheet inset by the border less
half the spacing is at least the border in from its edge. A hole's cells are
free, so a smaller part can stand in it. Each part goes where it uses the least
of the sheet's length, then nearest the start along it and across it, then at
the earliest of its turns. Its turns are searched at once and the place kept by
that order alone, so the layout is the same on any number of threads
(`arrange::tests::is_deterministic`, in the determinism gate). Three orders of
the parts are laid (largest first by area, by the longest side, and by the
width across the sheet), and the layout kept that places the most, then uses
the least of the sheet.

The grid errs to one side only: parts can stand further apart than asked,
never nearer. That is not trusted. Every layout is checked exactly before it is
used (`arrange::verify`), with the outlines and nothing of the grid: each part
grown by the spacing meets no other, and grown by the border lies on the sheet.
A layout that fails is an error, never a layout. The packer keeps clear of the
check by four times what the check can err by, a few micrometres
(`grid::MARGIN`), so a layout as tight as the grid allows still passes.

**A model is one body; each copy is a stand.** A model is loaded and placed
once, by its own transform's rotation and scale, whatever its copies. Each copy
is a `Stand`: a turn about Z and a move. Re-arranging changes stands only, so
no body is placed, copied or indexed again for it. A pick is found once on the
body and its geometry taken into the setup's frame through each copy's stand
(`Frame::to_setup_as`). A turn about Z keeps level things level and circles
round, so a hole is still a hole and an arc still an arc on every copy. No stand
at all does no arithmetic, and a quarter turn swaps and negates coordinates
rather than multiplying, so a job without sheets is laid out and cut bit for
bit as before, which every existing test and Fusion comparison held when the
stands went in.

**The setup machines only the parts it lists.** The sheet lays every part it
has; a setup that uses it machines the models it lists, and the others stand on
the sheet as material. A setup that lists a model on a sheet must use that
sheet: it stands nowhere else.

**Copies, grain, holes and the seat in the first version.** A part asks for
`copies` (1 to 1000), each a part to the packer, of the model's one outline. A
part may give its own `rotation`, its grain, used instead of the sheet's:
`"none"` or `"half-turn"` keeps it running the way the part is drawn. Copies a
sheet has no room for are left out and said to be; the rest are placed, and a
run cuts them, but the setup fails its checks (CC109) until every copy fits.
Parts stand in other parts' holes. The seat stands each part on the sheet's
bottom face by its lowest point, to be cut out of the sheet, or with its highest
point at the sheet's top.

**What means one place is refused on a part of several copies.** A pick is on
every copy, so a work zero picked on a model its sheet lays more than once is
refused from the job alone, before a run; so is a Morph whose curves are picked
on one, since a Morph lays its passes between two curves and picked on several
copies they are two on each. A position on a model a sheet places would do
nothing, and is refused too. The app clears it with the same edit that puts a
model on a sheet.

**Checks.** CC109 fails a sheet that leaves any copy of any of its parts out,
saying how many and why (too large for the sheet at every turn it may take, or
no room beside the others), or that cannot be arranged at all. CC110 fails a part
that stands above or below its sheet by more than the tolerance. CC225 says
which of a sheet's parts the setup neither machines nor checks. `camcraft job`
logs where each copy stands, so what was cut is on record.

## What was considered and not chosen

- **A group under Models**, a set of models with a layout of its own. The
  layout is of material, and its rules (the sheet's size, its border, which face
  parts stand on) are the stock's; a group would need a stock beside it anyway,
  and a second place to say which setup cuts it.
- **An Arrange button that stores positions.** Positions written into the job
  go stale the moment a part, the sheet or a spacing changes, and positions
  tuned by hand afterwards are checked by nothing. Always live, the layout is
  always the one the job describes.
- **Bounding rectangles.** Cheap and exact to check, but a rectangle wastes what
  lies in an L's corner or a ring's hole. The tests hold true shape to what
  rectangles cannot do: two Ls, one turned half round, use less of the sheet
  than their boxes would (`two_ls_use_less_of_the_sheet_than_their_boxes`), and
  a disc stands in a ring's hole (`a_disc_stands_in_a_rings_hole`).
- **No-fit polygons.** Exact placements, but each needs the polygon of every
  pair of outlines at every pair of turns each may take, and a part at a 15°
  step has 24 turns; holes and outlines that are not convex make each polygon
  harder still. Its cost was not measured. The grid's cost was, before anything
  else was built, and it was affordable (below), with the exact check making
  its layouts as safe as exact ones.
- **A body per copy.** `Body::placed` rebuilds the whole topology, so every
  live re-arrangement would have rebuilt it once per copy, and every pick would
  have been searched for once per copy.
- **A position silently ignored** on a model a sheet places. A setting that
  does nothing is refused, as `JobError::NotOffered` refuses others.
- **"The first copy"** as the one a work zero picked on a part of several copies
  is on. Which copy is first is the packer's choice, and the work zero is where
  the machine cuts from: refused, rather than moved when the sheet is.

## Measured

All in a release build on four threads.

- **The cell.** Two parts can stand at most `1 + 2√2` cells further apart than
  the spacing (`grid::GAP_PER_CELL`), so the cell is set by the most they may:
  a quarter of the spacing (`grid::EXTRA_GAP`), the typical part about half of
  that. Chosen by `arrange::tests::benchmark`, fifty mixed parts at a 15° step on
  a 2440 by 1220 sheet, 10 apart, in all three orders: 0.29 s at a half, 0.55 s
  at 0.35, 1.08 s at a quarter and 3.1 s at 0.15, placing 38, 40, 40 and 40 of
  them. Ten parts at quarter turns took 0.01 s.
- **The cell count.** A sheet is cut into at most eight million cells
  (`grid::MOST_CELLS`); one that would need more at the spacing's cell has
  larger cells. A 2440 by 1220 sheet 10 apart needs 6.9 million, so the quarter
  holds there.
- **The most copies, 1000** (`job::MOST_COPIES`), by
  `arrange::tests::copies_benchmark` on a 2440 by 1220 sheet: 1000 copies of a
  small block, a disc or an L are laid in under 1 s at quarter turns and under
  6 s at a 15° step, the slowest an L 3 apart; 2000 take up to 13 s and 5000 up
  to 34 s. Such a sheet held 799 of a 60 by 40 block 10 apart.
- **Twenty copies of `ops_2d`** on a 2440 by 1220 sheet
  (`run::tests_sheets::twenty_copies_timed`, measured when this was written):
  arranging took 0.038 s with its outline cast and 0.027 s with it kept,
  placing all twenty; laying the copies out as one part of 103,960 triangles
  took 0.002 s, its shadow 0.13 s, the drop-cutter's index 0.03 s and the
  part's surface 0.04 s. A 2D pocket picked once and cut in every copy ran in
  0.74 s. A 3D parallel with a 6 mm ball at a 2 mm stepover, as a new one
  starts, on the part's silhouette with the tool's centre in it, ran in 55 s,
  684,679 moves. Arranging is not what a sheet's time goes on.

## Known limits

- **Parts can stand up to a quarter of the spacing further apart than asked**,
  never nearer. On a very large sheet with a small spacing, the cell count's
  bound makes the cells larger than the spacing's rule, and the extra gap larger
  with them: on a 2440 by 1220 sheet, at a spacing a little under 10 mm and
  below.
- **A layout is the same every time on one machine**, in the app and on the
  command line, on any number of threads. Across platforms it is not promised.
  A turn by a free angle takes its sine and cosine from the platform's maths
  library, which may round differently on another and move a part. A quarter
  turn itself is exact everywhere, but the outlines are grown for the grid by
  the clipper's round joins, which take the platform's trigonometry whatever
  the turn, so even a sheet at quarter turns is not promised the same layout on
  another platform. No difference has been seen; none is tested for.
- **Changing the job's tolerance can re-arrange a sheet.** A part's outline is
  cast from its tessellation, which the tolerance decides.
- **One sheet only.** Parts with no room are reported (CC109), not put on a
  second sheet. No part can be pinned where it is wanted by hand.
- **Morph cannot morph across copies**, and a work zero cannot be picked on a
  part with several copies: both are refused. Give the part one copy, or make
  each copy a model of its own.
- **CC109 holds every part of a sheet**, whichever setup cuts it: a setup that
  uses the sheet fails for any part of it left out, whether or not that setup
  machines it. Parts a setup does not list are neither machined nor checked
  (CC225): an operation kept to the stock can cut through where they stand, and
  nothing sees it.
- **A 2D adaptive open pocket on a sheet part opens onto the sheet.** Where a
  picked pocket runs along the part's edge with nothing of the part beyond it,
  2D adaptive takes in what lies beyond, out to just past the stock's edge, as
  far as the widest gap between the setup's parts and the stock's sides
  (`run::operation::clearing::opened`), keeping off the shadow of the parts the
  setup machines, though not of those it does not list. On a sheet the stock is
  the whole sheet, so an open pocket on a part near the start of a sheet with
  room to spare clears a wide stretch of the sheet round it, to the pocket's
  depth, up to the setup's other parts and the sheet's edge.
- **A 3D operation over a sheet takes as long as what it covers.** Twenty
  copies are twenty parts to finish: the parallel above, on their silhouette,
  took 55 s where the pocket took under a second. A 3D operation kept to the
  stock, which is what a job file that leaves its boundary out gets, covers the
  whole sheet, the board between the parts too, and takes longer again; that
  was not measured. The app starts a new one on the parts' silhouette.
