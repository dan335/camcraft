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

## ops_block

`ops_block/part.py` builds `fixtures/step/ops_block.step` in a new document,
and `ops_block/cam.py`, run next with that document active, makes Fusion's
Face, 2D Contour, 2D Pocket and Drill on it, each as a new one starts, and
posts `fixtures/nc/ops_block_*.nc`. Both write to
`camcraft-fusion-bridge/out/ops_block/`. Run again, they make the committed
fixtures byte for byte (but for Windows line ends);
`crates/camcraft-session/src/compare/tests.rs` holds camcraft's new operations
to them.
