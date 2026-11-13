# Fusion's Engrave on the ops_engrave part, as a new operation of Fusion's
# starts, posted with Fusion's Masso post: fixtures/nc/ops_engrave_*.nc. Run
# through the bridge after part.py, with that document active (see
# ../README.md).
#
# The setup's stock is the plate's own box, its work zero the model's origin.
# The tool is the sample library's 6 mm 90 degree engraving tool (18,000
# rev/min, 1000 mm/min, plunging at 300), without a holder or coolant.
#
#   engrave   all three shapes, each picked by an edge of its rim
#   triangle, stroke, rectangle   each alone
import copy, json, os, time, traceback

BRIDGE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(BRIDGE, "out", "ops_engrave")
os.makedirs(OUT, exist_ok=True)
MM = 0.1

doc = app.activeDocument
design = adsk.fusion.Design.cast(doc.products.itemByProductType("DesignProductType"))
body = design.rootComponent.bRepBodies.item(0)
if body.name != "plate":
    raise RuntimeError("the ops_engrave part is not the active document: %s" % body.name)
ui.workspaces.itemById("CAMEnvironment").activate()
cam = adsk.cam.CAM.cast(doc.products.itemByProductType("CAMProductType"))

# The document is this script's own part, so its setups are this script's too.
for old in [cam.setups.item(i) for i in range(cam.setups.count)]:
    old.deleteMe()

si = cam.setups.createInput(adsk.cam.OperationTypes.MillingOperation)
si.models = [body]
si.name = "top"
setup = cam.setups.add(si)
for name, expr in (
    ("job_stockOffsetMode", "'simple'"),
    ("job_stockOffsetSides", "0 mm"),
    ("job_stockOffsetTop", "0 mm"),
    ("job_stockOffsetBottom", "0 mm"),
    ("wcs_origin_mode", "'modelOrigin'"),
):
    setup.parameters.itemByName(name).expression = expr

lib = adsk.cam.CAMManager.get().libraryManager.toolLibraries
tl = lib.toolLibraryAtURL(adsk.core.URL.create("systemlibraryroot://Samples/Milling Tools (Metric)"))
j = None
for i in range(tl.count):
    t = json.loads(tl.item(i).toJson())
    if t.get("type") == "chamfer mill" and abs(t["geometry"].get("DC", 0) - 6) < 1e-9:
        j = copy.deepcopy(t)
j.pop("holder", None)
j["description"] = "6mm 90deg engraving"
for key in ("number", "length-offset", "diameter-offset"):
    j["post-process"][key] = 4
preset = j["start-values"]["presets"][0]
preset.update({"v_f": 1000, "v_f_plunge": 300, "v_f_ramp": 300, "v_f_leadIn": 1000, "v_f_leadOut": 1000,
               "v_f_retract": 1000, "v_f_transition": 1000, "n": 18000, "tool-coolant": "disabled"})
if "n_ramp" in preset:
    preset["n_ramp"] = 18000
j["start-values"]["presets"] = [preset]
vee = adsk.cam.Tool.createFromJson(json.dumps(j))

def edge_through(x, y, z):
    best, far = None, 1.0 * MM
    for e in body.edges:
        m = e.pointOnEdge
        d = ((m.x - x * MM) ** 2 + (m.y - y * MM) ** 2 + (m.z - z * MM) ** 2) ** 0.5
        if d < far:
            best, far = e, d
    if best is None:
        raise LookupError((x, y, z))
    return best

RIMS = {"triangle": (-20, -15, 10), "stroke": (10, -15, 10), "rectangle": (31, -12, 10)}

def contours(names, reverted):
    def select(op):
        v = op.parameters.itemByName("contours").value
        cs = v.getCurveSelections()
        for n in names:
            chain = cs.createNewChainSelection()
            chain.inputGeometry = [edge_through(*RIMS[n])]
            chain.isReverted = reverted
        v.applyCurveSelections(cs)
    return select

made = []
for name, names in (("engrave", ["triangle", "stroke", "rectangle"]), ("triangle", ["triangle"]),
                    ("stroke", ["stroke"]), ("rectangle", ["rectangle"])):
    try:
        oi = setup.operations.createInput("engrave")
        oi.tool = vee
        oi.displayName = name
        op = setup.operations.add(oi)
        contours(names, False)(op)
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
    for what in ("hasWarning", "warning", "error", "hasToolpath"):
        try:
            v = getattr(op, what)
            if v not in (False, "", None):
                notes.append("%s: %s" % (what, v))
        except Exception:
            pass
    try:
        notes.append("bottom offset: %s" % op.parameters.itemByName("bottomHeight_offset").expression)
    except Exception:
        pass
    posted = None
    if op.isToolpathValid:
        pi = adsk.cam.PostProcessInput.create("ops_engrave_" + op.name, post, OUT,
            adsk.cam.PostOutputUnitOptions.MillimetersOutput)
        pi.isOpenInEditor = False
        try:
            posted = cam.postProcess(op, pi)
        except Exception as e:
            posted = "failed: %s" % e
    print(op.name, "valid:", op.isToolpathValid, "posted:", posted, "; ".join(notes))
print(sorted(os.listdir(OUT)))
