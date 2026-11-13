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
triangle, an L 4 mm wide and a rectangle sunk 1.5 mm into its top, and
`ops_engrave/cam.py` posts Fusion's Engrave on them with a 90 degree V,
together and each alone: `fixtures/nc/ops_engrave_*.nc`.
