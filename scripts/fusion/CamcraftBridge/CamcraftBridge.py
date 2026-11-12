"""Runs Python scripts dropped into a folder, on Fusion's main thread.

For comparing Fusion's CAM with camcraft's: a script written to
inbox/<name>.py under the bridge folder (%LOCALAPPDATA%/camcraft-fusion-bridge on
Windows, ~/camcraft-fusion-bridge on macOS) is run with `adsk`, `app`
and `ui` in scope, its printed output (or its traceback) written to
outbox/<name>.py.ok.txt (or .err.txt), and the script moved to done/.

Only runs while started from Scripts and Add-Ins; not on startup.
Stop it there, or delete this folder, to remove it.
"""

import adsk.core
import adsk.fusion
import adsk.cam
import contextlib
import io
import os
import threading
import time
import traceback

app = adsk.core.Application.get()
ui = app.userInterface

EVENT_ID = "camcraftBridgeRunScript"
if os.name == "nt":
    BRIDGE = os.path.join(os.environ["LOCALAPPDATA"], "camcraft-fusion-bridge")
else:
    BRIDGE = os.path.expanduser("~/camcraft-fusion-bridge")
INBOX = os.path.join(BRIDGE, "inbox")
OUTBOX = os.path.join(BRIDGE, "outbox")
DONE = os.path.join(BRIDGE, "done")

_handlers = []
_stop = threading.Event()
_event = None
_thread = None


def _write_atomically(path, text):
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        f.write(text)
    os.replace(tmp, path)


def _run(claimed):
    # claimed is inbox/<name>.py.running
    name = os.path.basename(claimed)[: -len(".running")]
    buf = io.StringIO()
    ok = True
    started = time.time()
    try:
        with open(claimed, encoding="utf-8") as f:
            code = f.read()
        scope = {
            "__name__": "__camcraft_bridge__",
            "__file__": os.path.join(INBOX, name),
            "adsk": adsk,
            "app": app,
            "ui": ui,
        }
        with contextlib.redirect_stdout(buf):
            exec(compile(code, name, "exec"), scope)
    except Exception:
        ok = False
        buf.write("\n" + traceback.format_exc())
    buf.write("\n[bridge] %.1f s\n" % (time.time() - started))
    out = os.path.join(OUTBOX, name + (".ok.txt" if ok else ".err.txt"))
    _write_atomically(out, buf.getvalue())
    os.replace(claimed, os.path.join(DONE, name))


class _RunHandler(adsk.core.CustomEventHandler):
    def notify(self, args):
        try:
            _run(args.additionalInfo)
        except Exception:
            _write_atomically(
                os.path.join(OUTBOX, "bridge-failure.txt"), traceback.format_exc()
            )


def _poll():
    while not _stop.wait(0.5):
        try:
            _write_atomically(os.path.join(BRIDGE, "alive.txt"), "%f" % time.time())
            for n in sorted(os.listdir(INBOX)):
                if not n.endswith(".py"):
                    continue
                path = os.path.join(INBOX, n)
                claimed = path + ".running"
                try:
                    os.replace(path, claimed)
                except OSError:
                    continue
                app.fireCustomEvent(EVENT_ID, claimed)
        except Exception:
            pass


def run(context):
    global _event, _thread
    try:
        for d in (INBOX, OUTBOX, DONE):
            os.makedirs(d, exist_ok=True)
        _event = app.registerCustomEvent(EVENT_ID)
        handler = _RunHandler()
        _event.add(handler)
        _handlers.append(handler)
        _stop.clear()
        _thread = threading.Thread(target=_poll, daemon=True)
        _thread.start()
        _write_atomically(os.path.join(BRIDGE, "started.txt"), time.ctime())
    except Exception:
        ui.messageBox("CamcraftBridge failed to start:\n" + traceback.format_exc())


def stop(context):
    _stop.set()
    try:
        if _event is not None:
            for h in _handlers:
                _event.remove(h)
        app.unregisterCustomEvent(EVENT_ID)
    except Exception:
        pass
    _handlers.clear()
    try:
        os.remove(os.path.join(BRIDGE, "alive.txt"))
    except OSError:
        pass
