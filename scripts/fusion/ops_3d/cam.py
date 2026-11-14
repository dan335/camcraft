# Fusion's Ramp on the ops_3d part, as a new operation of Fusion's starts but
# for what its name says, posted with Fusion's Masso post:
# fixtures/nc/ops_3d_*.nc. Run through the bridge after part.py (see
# ../README.md). It finds the document part.py made by its one body, named
# ops_3d, and works in that document alone.
#
# The setup's stock is 5 mm round the part and level with its top, its work
# zero the model's origin. The tool is the sample library's 6 mm ball end mill
# (18,000 rev/min, 1500 mm/min, plunging and ramping at 500), without a holder
# and without coolant, as camcraft's tools have them. The Masso post is looked
# for beside the bridge's inbox (masso.cps).
#
#   ramp        a new Ramp: Maximum Stepdown 1 mm, Ramping Angle the tool's 2
#               degrees
#   ramp_step2  the same, Maximum Stepdown 2 mm
#   ramp_angle  the same, Ramping Angle 0.5 degrees: shallower than a stepdown
#               each time round every wall but the outside's
import copy, json, os, time, traceback

BRIDGE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(BRIDGE, "out", "ops_3d")
os.makedirs(OUT, exist_ok=True)
MM = 0.1

def ours():
    """The document part.py made: its only body is named ops_3d."""
    for i in range(app.documents.count):
        d = app.documents.item(i)
        try:
            des = adsk.fusion.Design.cast(d.products.itemByProductType("DesignProductType"))
            bodies = des.rootComponent.bRepBodies
            if bodies.count == 1 and bodies.item(0).name == "ops_3d":
                return d
        except Exception:
            pass
    raise RuntimeError("no document made by ops_3d/part.py is open")

doc = ours()
if app.activeDocument != doc:
    doc.activate()
design = adsk.fusion.Design.cast(doc.products.itemByProductType("DesignProductType"))
body = design.rootComponent.bRepBodies.item(0)
box = body.boundingBox
if abs(box.maxPoint.x - box.minPoint.x - 60 * MM) > 1e-6:
    raise RuntimeError("the ops_3d part is not 60 mm across")
ui.workspaces.itemById("CAMEnvironment").activate()
cam = adsk.cam.CAM.cast(doc.products.itemByProductType("CAMProductType"))

# The document is this script's own part, so its setups are this script's too:
# run again, it starts over, and the operations keep their names.
for old in [cam.setups.item(i) for i in range(cam.setups.count)]:
    old.deleteMe()

si = cam.setups.createInput(adsk.cam.OperationTypes.MillingOperation)
si.models = [body]
si.name = "top"
setup = cam.setups.add(si)
for name, expr in (
    ("job_stockOffsetMode", "'simple'"),
    ("job_stockOffsetSides", "5 mm"),
    ("job_stockOffsetTop", "0 mm"),
    ("job_stockOffsetBottom", "0 mm"),
    ("wcs_origin_mode", "'modelOrigin'"),
):
    setup.parameters.itemByName(name).expression = expr

lib = adsk.cam.CAMManager.get().libraryManager.toolLibraries
def sample(kind, diameter):
    tl = lib.toolLibraryAtURL(adsk.core.URL.create("systemlibraryroot://Samples/Milling Tools (Metric)"))
    for i in range(tl.count):
        j = json.loads(tl.item(i).toJson())
        if j.get("type") == kind and abs(j["geometry"].get("DC", 0) - diameter) < 1e-9:
            return j
    raise LookupError("%s %s" % (kind, diameter))

def make_tool(j, number, desc, rpm, feeds):
    j = copy.deepcopy(j)
    j.pop("holder", None)
    j["description"] = desc
    for key in ("number", "length-offset", "diameter-offset"):
        j["post-process"][key] = number
    preset = j["start-values"]["presets"][0]
    preset.update(feeds)
    preset["n"] = rpm
    if "n_ramp" in preset:
        preset["n_ramp"] = rpm
    preset["tool-coolant"] = "disabled"
    j["start-values"]["presets"] = [preset]
    print("tool", number, desc, json.dumps(j["geometry"]))
    return adsk.cam.Tool.createFromJson(json.dumps(j))

FEEDS = {"v_f": 1500, "v_f_plunge": 500, "v_f_ramp": 500, "v_f_leadIn": 1500, "v_f_leadOut": 1500,
         "v_f_retract": 1500, "v_f_transition": 1500}
ball6 = make_tool(sample("ball end mill", 6), 2, "6mm ball", 18000, FEEDS)

made = []
for name, settings in (
    ("ramp", {}),
    ("ramp_step2", {"maximumStepdown": "2 mm"}),
    ("ramp_angle", {"rampAngle": "0.5 deg"}),
):
    try:
        oi = setup.operations.createInput("ramp")
        oi.tool = ball6
        oi.displayName = name
        op = setup.operations.add(oi)
        for key, expr in settings.items():
            op.parameters.itemByName(key).expression = expr
        made.append(op)
    except Exception:
        print(name, "NOT MADE:", traceback.format_exc().strip().splitlines()[-1])

fut = cam.generateAllToolpaths(False)
t0 = time.time()
while not fut.isGenerationCompleted and time.time() - t0 < 300:
    adsk.doEvents()
    time.sleep(0.05)

post = os.path.join(BRIDGE, "masso.cps")
for op in made:
    notes = []
    for what in ("hasWarning", "warning", "error"):
        try:
            v = getattr(op, what)
            if v not in (False, "", None):
                notes.append("%s: %s" % (what, v))
        except Exception:
            pass
    for key in ("maximumStepdown", "rampAngle", "flatAreaMachining", "direction"):
        p = op.parameters.itemByName(key)
        notes.append("%s: %s" % (key, p.expression))
    posted = None
    if op.isToolpathValid:
        pi = adsk.cam.PostProcessInput.create("ops_3d_" + op.name, post, OUT,
            adsk.cam.PostOutputUnitOptions.MillimetersOutput)
        pi.isOpenInEditor = False
        try:
            posted = cam.postProcess(op, pi)
        except Exception as e:
            posted = "failed: %s" % e
    print(op.name, "valid:", op.isToolpathValid, "posted:", posted, "; ".join(notes))
print(sorted(os.listdir(OUT)))
