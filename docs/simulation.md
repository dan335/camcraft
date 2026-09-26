# Simulation, verification and playback

The simulator verifies every run against material, and the gate refuses a program
it has not verified. The same material is what the app plays back.

## The material

The stock is a height field seen from above (`camcraft_sim::stock::Stock`): every
column has one top. That is complete for 3-axis work from above, which cannot make
an undercut. It cannot carry what a top setup leaves to a flipped bottom one; that
needs a volumetric stock.

### Resolution

The grid follows the job (`core::run::sim_pitch`): fine enough that the setup's
narrowest tool cuts a slot 8 cells wide, never coarser than tolerance × 8, finer
still to spend 2 million cells on a small stock, and never more than 64 million
cells (`MAX_SIM_CELLS`), never finer than 0.02 mm. So:

- the lego brick at 0.053 mm;
- a 300 mm stock with a 1/32" cutter at 0.1 mm;
- a full 5' × 12' × 8" sheet at 0.4 mm with a 1/4" cutter, and at 0.3 mm (the cap)
  with a 1/16".

The verifier runs on the same grid, its budget tolerance + pitch.

### Sweeps

Sweeps are exact where they can be. A level move is swept in closed form (every
cell to the tool's underside at its distance from the move's line, in parallel
rows for a long pass), a move straight up or down is the tool at its lowest end,
and only climbing moves are sampled every half cell. The part surface is
rasterised triangle by triangle, over the part's own footprint only.

### Walls

Beside a vertical wall a column's height cannot tell a hair past the wall from a
gouge the wall's height deep. `PartSurface::reaching` judges material against the
lowest the part comes within the budget across (none beside no part, or past the
grid), looked for only where the material is already that far below its own
column. So walls are judged across as well as down.

### Memory

The stock after each operation is kept packed (`camcraft_sim::stock::PackedStock`: runs of equal
heights, and machined material is mostly flat) and shared by the run and its
playback; the part surface is shared too. A full sheet at the 64 million cell cap:
the run peaks at 1.1 GB, the app at about 2.4 GB with the sheet playing
(`sheet_playback` snapshot, measured with `/usr/bin/time -l`).

## Playback

**One position.** The app's playhead is whole segments made plus a fraction of the
next. The clock, the paths drawn, the cutter (part-way along its move) and the
material all come from it, turned into a replay position.

**The UI thread never sweeps.** Each setup being played has a thread that owns a
`camcraft_core::replay::Live` stock. The UI sends the playhead when it moves; the
thread works only on the newest, 12 ms at a time, and sends back the cells that
changed, recoloured. The viewport uploads only that rectangle. "cutting…" beside
the clock means the material is still catching up.

**Exact.** `Live` advances by whole moves and sweeps a part-way move onto the
stock, putting those cells back before the next step. Tests hold any walk,
forward, back and part-way, to `Replay::stock_at` bit for bit, and every changed
cell inside the rectangle reported.

**Rewind is bounded.** `Live` drops a packed checkpoint every 1/64th of the
program, within 256 MB a setup. The thread's idle sweep drops them all; the stock
that seeks does not pack any, so a seek someone waits on never pauses to. A
restore unpacks in place and redraws only the cells that differ. The end of an
operation is the run's own stock after it, so going there, or to the end of the
program, restores at once with no sweeping.

**Only what the cutter touched moves.** Changes are kept in 32 × 32-cell tiles:
only those are classified (in parallel, and only cells whose height changed), and
a frame uploads at most 4 million cells of them, the rest following over the next
frames, so a long seek never stalls the window. The playback thread and the UI
share one image of the material behind a lock, taken only to classify a batch or
upload one. The same thread keeps the material's ambient occlusion up to date:
the cells cut are shaded with the cut, so fresh material never shows the shade of
the surface taken away; the cells round them, whose horizon the cut changed, in
short slices when there is time.

Measured (`cargo test --release -p camcraft-gui -- --ignored keeps_up --nocapture`,
with `CAMCRAFT_SHEET_JOB` naming a sheet job for the third), the thread's work a
frame at 1,000×, and the worst rewind after the idle sweep:

| Job         | Work a frame | Worst rewind |
|-------------|--------------|--------------|
| router jig  | 2 ms         | 60 ms        |
| lego brick  | 8.5 ms       | 15 ms        |
| full sheet  | 9 ms         | 0.3 s        |

## Drawing the material

**Walls are drawn vertical.** The stock shader traces each pixel's ray down a
pyramid of maxima (the height texture's mips) to the first column it meets: a
column's top, or its side as a vertical wall, lit by which way the material around
it falls away so round walls read round. Rays start where they enter the box, so a
sheet seen from metres off draws as cleanly as a brick up close. Colour is one
byte a cell, mapped on the GPU, so switching material and compare colours redraws
nothing.

**Big stock is drawn as tiles**: 2040 cells square, each its own box, textures and
pyramid, with a 4-cell apron of its neighbours so seams do not show; eframe gives
textures 8192 across at most, and a sheet at 0.3 mm is 12,000. The view frames the
part and its stock together and can always zoom out to all the stock.

**Shaders are checked by the tests** (naga, as wgpu builds it), not first by a GPU
when the app draws.
