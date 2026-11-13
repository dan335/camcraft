# Fusion's Bore, Circular, Slot, Trace, 2D Adaptive and 2D Chamfer on the
# ops_2d part, each as a new operation of Fusion's starts but for what its
# name says, posted with Fusion's
# Masso post: fixtures/nc/ops_2d_*.nc. Run through the bridge after part.py,
# with that document active (see ../README.md).
#
# The setup's stock is 5 mm round the part and level with its top, its work
# zero the model's origin. The tools are the sample library's 6 mm and 8 mm
# flats (18,000 rev/min, 1500 mm/min, plunging and ramping at 500) and its
# 6 mm 90 degree engraving tool (18,000 rev/min, 1000 mm/min, plunging at 300),
# without holders and without coolant, as camcraft's tools have them. The
# Masso post is looked for beside the bridge's inbox (masso.cps).
#
#   bore             the through hole and the blind hole, 6 mm flat
#   bore_passes      the same, Multiple Passes on, 3 stepovers
#   circular         the round pocket, 6 mm flat
#   circular_passes  the same, Multiple Passes on, 4 stepovers
#   slot             both slots, 8 wide, 6 mm flat
#   slot8            both slots, 8 mm flat: the tool as wide as the slot
#   trace            both V grooves along their valleys, the engraving tool,
#                    Chamfer off
#   adaptive         the pocket with its island, 6 mm flat
#   adaptive_open    the notch open to two sides, 6 mm flat
#   chamfer_lower    the pocket's chamfered rim, picked by the chamfer's lower
#                    edge, the engraving tool
#   chamfer_upper    the same, picked by its upper edge
#   chamfer_width    the island's sharp rim, Chamfer Width 1 mm
#   thread           both holes, the sample library's largest thread mill
#                    (18,000 rev/min, 600 mm/min), as a new Thread starts: a
#                    pitch diameter offset of none, so it only touches the wall
#   thread_cut       the same, Pitch Diameter Offset 1 mm: the thread cut half
#                    a millimetre into the wall
import copy, json, os, time, traceback

BRIDGE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(BRIDGE, "out", "ops_2d")
os.makedirs(OUT, exist_ok=True)
MM = 0.1

doc = app.activeDocument
design = adsk.fusion.Design.cast(doc.products.itemByProductType("DesignProductType"))
body = design.rootComponent.bRepBodies.item(0)
box = body.boundingBox
if body.name != "part" or abs(box.maxPoint.x - box.minPoint.x - 140 * MM) > 1e-6:
    raise RuntimeError("the ops_2d part is not the active document: %s" % body.name)
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

def sample_all(kind):
    tl = lib.toolLibraryAtURL(adsk.core.URL.create("systemlibraryroot://Samples/Milling Tools (Metric)"))
    return [j for j in (json.loads(tl.item(i).toJson()) for i in range(tl.count)) if j.get("type") == kind]

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

FLAT = {"v_f": 1500, "v_f_plunge": 500, "v_f_ramp": 500, "v_f_leadIn": 1500, "v_f_leadOut": 1500,
        "v_f_retract": 1500, "v_f_transition": 1500}
VEE = {"v_f": 1000, "v_f_plunge": 300, "v_f_ramp": 300, "v_f_leadIn": 1000, "v_f_leadOut": 1000,
       "v_f_retract": 1000, "v_f_transition": 1000}
flat6 = make_tool(sample("flat end mill", 6), 1, "6mm flat", 18000, FLAT)
flat8 = make_tool(sample("flat end mill", 8), 3, "8mm flat", 18000, FLAT)
vee = make_tool(sample("chamfer mill", 6), 4, "6mm 90deg engraving", 18000, VEE)
MILL = {"v_f": 600, "v_f_plunge": 300, "v_f_ramp": 300, "v_f_leadIn": 600, "v_f_leadOut": 600,
        "v_f_retract": 600, "v_f_transition": 600}
thread_mill = make_tool(max(sample_all("thread mill"), key=lambda j: j["geometry"]["DC"]),
                        5, "thread mill", 18000, MILL)

ST = adsk.core.SurfaceTypes
def cylinders(radius):
    return [f for f in body.faces if f.geometry.surfaceType == ST.CylinderSurfaceType
            and abs(f.geometry.radius - radius * MM) < 1e-6]

def floor_at(x, y, z):
    for f in body.faces:
        if f.geometry.surfaceType == ST.PlaneSurfaceType:
            p = f.pointOnFace
            ok, n = f.evaluator.getNormalAtPoint(p)
            if n.z > 0.99 and abs(p.z - z * MM) < 1e-6 and abs(p.x - x * MM) < 0.5 and abs(p.y - y * MM) < 0.5:
                return f
    raise LookupError((x, y, z))

def valleys():
    """The grooves' bottom edges: the two edges 2 mm under the top."""
    es = [e for e in body.edges
          if abs(e.startVertex.geometry.z - 18 * MM) < 1e-6 and abs(e.endVertex.geometry.z - 18 * MM) < 1e-6]
    assert len(es) == 2, len(es)
    return es

def faces(*sets):
    def select(op):
        op.parameters.itemByName("circularFaces").value.value = [f for s in sets for f in s]
    return select

def pockets(*floors):
    def select(op):
        v = op.parameters.itemByName("pockets").value
        cs = v.getCurveSelections()
        for floor in floors:
            cs.createNewPocketSelection().inputGeometry = [floor]
        v.applyCurveSelections(cs)
    return select

def chains(edges):
    def select(op):
        v = op.parameters.itemByName("curves").value
        cs = v.getCurveSelections()
        for e in edges:
            # Open, or Fusion closes the chain round the groove's end and rim.
            chain = cs.createNewChainSelection()
            chain.isOpen = True
            chain.inputGeometry = [e]
        v.applyCurveSelections(cs)
    return select

def edge_through(x, y, z):
    """The edge whose own point is nearest (x, y, z), within a millimetre."""
    best, far = None, 1.0 * MM
    for e in body.edges:
        m = e.pointOnEdge
        d = ((m.x - x * MM) ** 2 + (m.y - y * MM) ** 2 + (m.z - z * MM) ** 2) ** 0.5
        if d < far:
            best, far = e, d
    if best is None:
        raise LookupError((x, y, z))
    return best

def island_rim():
    for e in body.edges:
        g = e.geometry
        if g.objectType.endswith("Circle3D") and abs(g.radius - 5 * MM) < 1e-6                 and abs(g.center.z - 20 * MM) < 1e-6 and abs(g.center.x + 37 * MM) < 1e-6:
            return e
    raise LookupError("island rim")

def contours(edge, reverted=False):
    def select(op):
        v = op.parameters.itemByName("contours").value
        cs = v.getCurveSelections()
        chain = cs.createNewChainSelection()
        chain.inputGeometry = [edge]
        # Which way the chain runs says which side of it the tool is on.
        chain.isReverted = reverted
        v.applyCurveSelections(cs)
    return select

slots = pockets(floor_at(40, 35, 14), floor_at(45, 8, 16))
PASSES3 = {"doMultiplePasses": "true", "numberOfStepovers": "3"}
PASSES4 = {"doMultiplePasses": "true", "numberOfStepovers": "4"}

made = []
for strategy, name, tool, select, settings in (
    ("bore", "bore", flat6, faces(cylinders(8), cylinders(7)), {}),
    ("bore", "bore_passes", flat6, faces(cylinders(8), cylinders(7)), PASSES3),
    ("circular", "circular", flat6, faces(cylinders(15)), {}),
    ("circular", "circular_passes", flat6, faces(cylinders(15)), PASSES4),
    ("slot", "slot", flat6, slots, {}),
    ("slot", "slot8", flat8, slots, {}),
    ("trace", "trace", vee, chains(valleys()), {"doChamfer": "false"}),
    ("adaptive2d", "adaptive", flat6, lambda op: pockets(floor_at(-47, -22, 12))(op), {}),
    ("adaptive2d", "adaptive_open", flat6, lambda op: pockets(floor_at(55, -39, 14))(op), {}),
    # A chamfer the model has: Fusion asks for no width and no clearance.
    ("chamfer2d", "chamfer_lower", vee, lambda op: contours(edge_through(-37, -40, 19), True)(op),
     {"chamferClearance": "0 mm"}),
    ("chamfer2d", "chamfer_upper", vee, lambda op: contours(edge_through(-37, -41, 20))(op), {}),
    ("chamfer2d", "chamfer_width", vee, lambda op: contours(island_rim(), True)(op), {"chamferWidth": "1 mm"}),
    ("thread", "thread", thread_mill, faces(cylinders(8), cylinders(7)), {}),
    ("thread", "thread_cut", thread_mill, faces(cylinders(8), cylinders(7)), {"pitchDiameterOffset": "1 mm"}),
):
    try:
        oi = setup.operations.createInput(strategy)
        oi.tool = tool
        oi.displayName = name
        op = setup.operations.add(oi)
        select(op)
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
    for what in ("hasWarning", "hasError", "warning", "error"):
        try:
            v = getattr(op, what)
            if v:
                notes.append("%s: %s" % (what, v))
        except Exception:
            pass
    posted = None
    if op.isToolpathValid:
        pi = adsk.cam.PostProcessInput.create("ops_2d_" + op.name, post, OUT,
            adsk.cam.PostOutputUnitOptions.MillimetersOutput)
        pi.isOpenInEditor = False
        try:
            posted = cam.postProcess(op, pi)
        except Exception as e:
            posted = "failed: %s" % e
        try:
            notes.append("has toolpath: %s" % op.hasToolpath)
        except Exception:
            pass
    if op.name.startswith("thread"):
        pitch = op.parameters.itemByName("threadPitch")
        notes.append("pitch: %s = %s" % (pitch.expression, pitch.value.value))
    print(op.name, "valid:", op.isToolpathValid, "posted:", posted, "; ".join(notes))
print(sorted(os.listdir(OUT)))
