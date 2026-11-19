# Driving Fusion from here

camcraft's operations follow Fusion's, and the way to know they do is to make
the same thing in both and compare: the same part, a new operation of each
strategy, both programs posted for the same machine, and the material each
leaves cut by one simulator (`camcraft compare`, see
[docs/jobs.md](../../docs/jobs.md)). This folder is how the Fusion side is made
without clicking through it: Fusion's Python API builds the part, sets up the
operations and posts them, and the scripts that did it are kept, so the
fixtures they made can be made again.

## The bridge

Fusion runs Python only from inside itself. `CamcraftBridge` is an add-in that,
while it runs, watches a folder and runs each script dropped into it on Fusion's
main thread, writing back what the script printed, or its traceback:

- `inbox/NAME.py` is run, then moved to `done/`;
- `outbox/NAME.py.ok.txt` (or `.err.txt`) holds its output.

The folder is `camcraft-fusion-bridge` in `%LOCALAPPDATA%` on Windows and in
your home folder on macOS. A script has `adsk`, `app` and `ui` to hand.

To set it up, once:

1. Copy `CamcraftBridge/` into Fusion's add-ins folder:
   `%APPDATA%\Autodesk\Autodesk Fusion 360\API\AddIns` on Windows,
   `~/Library/Application Support/Autodesk/Autodesk Fusion 360/API/AddIns` on
   macOS.
2. Put the post the programs are for beside the bridge folder's `inbox`, as
   `masso.cps` for the scripts here (Fusion's own `Posts` folder has it on a
   machine that has posted for a Masso).
3. In Fusion, **Utilities › Add-Ins › Scripts and Add-Ins**, find
   **CamcraftBridge** and turn it on. It does not start with Fusion: anything
   in its inbox runs inside Fusion while it is on, so it is on only while it is
   wanted.

`run.sh` hands it a script and prints the answer:

```sh
bash scripts/fusion/run.sh scripts/fusion/ops_block/part.py
FUSION_HOST=me@my-mac bash scripts/fusion/run.sh scripts/fusion/ops_block/part.py
```

With `FUSION_HOST`, over SSH to a Fusion on another machine (whose bridge folder
is then `~/camcraft-fusion-bridge` there), with the files a script writes left
there for `scp` to fetch.

A script finds the bridge folder from where the bridge put it, so the same
script runs on either system, and writes under `out/` there. Each makes a
document of its own and leaves the others alone; the documents it leaves open
can be closed without saving.

## params

`params.py` asks Fusion for every strategy a milling setup offers and every
parameter a new operation of each starts with: its name, the title the panel
shows, the expression it starts as and its choices, in
`out/params/<strategy>.json`, with `strategies.json` listing them. This is
where camcraft's operations get Fusion's settings and defaults from. Run it
again after Fusion updates.

## ops_block

`ops_block/part.py` builds `fixtures/step/ops_block.step` in a new document,
and `ops_block/cam.py`, run next with that document active, makes Fusion's
Face, 2D Contour, 2D Pocket and Drill on it, each as a new one starts, and
posts `fixtures/nc/ops_block_*.nc`. Both write to
`camcraft-fusion-bridge/out/ops_block/`. Run again, they make the committed
fixtures byte for byte (but for Windows line ends);
`crates/camcraft-session/src/compare/tests.rs` holds camcraft's new operations
to them.

## ops_2d

`ops_2d/part.py` builds `fixtures/step/ops_2d.step`, a 140 x 100 x 20 mm block
with something for each of Fusion's other 2D operations to cut: a through and
a blind hole, a round pocket, two slots, a pocket with an island and a
chamfered rim, a notch open to two sides, and two V grooves. `ops_2d/cam.py`,
run next with that document active, makes Fusion's Bore, Circular, Slot,
Trace, 2D Adaptive and 2D Chamfer on it, each as a new one starts but for what
its name says, and posts `fixtures/nc/ops_2d_*.nc`. Its header lists them.

Three things it has to say, which the panel says with a click:

- a Trace's chain is open (`isOpen`), or Fusion closes it round the groove's
  end and rim;
- which way a 2D Chamfer's chain runs (`isReverted`) says which side of the
  edge the tool is on, and the wrong way is an empty toolpath, not an error;
- 2D Adaptive takes a pocket open to the part's edge as open to air: its
  program rapids down beside the part, into any stock left standing there.

## ops_engrave

`ops_engrave/part.py` builds `fixtures/step/ops_engrave.step`, a plate with a
triangle, an L 4 mm wide and a rectangle sunk 3 mm into its top, and
`ops_engrave/cam.py` posts Fusion's Engrave on them with a 90 degree V,
together and each alone: `fixtures/nc/ops_engrave_*.nc`.

## ops_3d

`ops_3d/part.py` builds `fixtures/step/ops_3d.step`, a 60 x 40 x 20 mm block
for Fusion's 3D finishing: a pocket 38 x 26 and 10 deep, one wall drafted
15 degrees and the rest upright, one corner R5 and three left sharp, and its
floor's edges R4; a dome 14 across and 5 tall standing on its floor, its foot
a sharp concave circle; and the block's top edge at +X rounded R8, ground
going from level to upright. Steep, drafted and curved walls, shallow and
curved ground, concave fillets, sharp inside corners and a dome: what Ramp,
Pencil, Project, Morph, Parallel, Scallop, Spiral, Contour and Flat each have to
follow. Its one body is named `ops_3d`. `ops_3d/cam.py` finds that document by
it, so it runs with any document active, and posts Fusion's Ramp with the
sample library's 6 mm ball, as a new one starts, with a stepdown of 2 mm, with
a ramp angle of 0.5 degrees and with the dome avoided 1 mm each way:
`fixtures/nc/ops_3d_ramp*.nc`; and its Pencil, as a new one
starts with the library's 12 mm ball, which is larger than the floor's
fillets and so touches twice all round the pocket's floor, and with its 6 mm
ball, which fits them, then the 12 mm ball at a bitangency angle of 40 degrees
and the 6 mm one with two stepovers: `fixtures/nc/ops_3d_pencil*.nc`; and its
Project with the 6 mm ball along three curves picked on the part (the dome's
foot, the floor's edge where its fillets start and the round's lower edge), as a
new one starts, the other way, with an axial offset of 1 mm and with the 12 mm
ball: `fixtures/nc/ops_3d_project*.nc`; and its Morph with the 6 mm ball between
the dome's foot and the floor's edge, as a new one starts and one way, and
between the round's two edges: `fixtures/nc/ops_3d_morph*.nc`; and its
Parallel with the 6 mm ball as a new one starts, then with surface groups set
as the Surface Manager sets them: the dome avoided with offsets of 1 mm and
with none, the model avoided with only the pocket's floor machined, the floor
machined with stock of its own, and the block's top avoided 2 mm down:
`fixtures/nc/ops_3d_parallel*.nc`; and its Scallop with the 6 mm ball as a new
one starts, with the dome avoided 1 mm each way, and with the model avoided and
only the floor machined: `fixtures/nc/ops_3d_scallop*.nc`; its Spiral as a
new one starts and with the dome avoided 1 mm each way:
`fixtures/nc/ops_3d_spiral*.nc`; its Contour, the same two ways:
`fixtures/nc/ops_3d_contour*.nc`; and its Flat with the library's 6 mm flat
end mill, the same two ways: `fixtures/nc/ops_3d_flat*.nc`. A group's radial
and axial offsets are given
to the API in millimetres, not the centimetres its lengths are elsewhere. A
click on an edge picks its chain, which Fusion closes round the loop it is in
unless it is told the chain is open (`isOpen`); a new closed chain runs
clockwise seen from above, and `isReverted` turns it round. Morph gives no passes between two open
chains that run opposite ways, so the round's lower edge is reverted. Run
again, it makes every one of them byte for byte (but for Windows line ends),
but for Parallel's, Scallop's and Spiral's: Fusion does not post the same
bytes for those from one generation to the next. On 2026-11-19 the committed
`ops_3d/cam.py` was run three times, and each run all but a few of their
programs came out as the fixtures (`parallel` differed the first time,
`parallel_floor_leave` the second, and those two with `scallop_avoid_dome` and
`spiral` the third). One that differed had the same passes with some sixty to
two hundred of its points moved along them, by a few hundredths of a
millimetre: its point reduction. So a fixture of those three is one of
Fusion's answers, which Fusion makes again only to within that. Contour's,
Flat's and `ramp_avoid_dome`, made first by these same rows in a copy of the
script that made only them, came out of the committed script byte for byte
the once it has been run since, that same day.
