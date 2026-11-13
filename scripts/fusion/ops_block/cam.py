# Fusion's Face, 2D Contour, 2D Pocket and Drill on the ops_block part, each as
# a new operation of Fusion's starts, posted with Fusion's Masso post:
# fixtures/nc/ops_block_*.nc. Run through the bridge after part.py, with that
# document active (see ../README.md).
#
# The setup's stock is 5 mm round the part and 2 mm over it, its work zero the
# model's origin. The tools are the sample library's 6 mm flat (18,000 rev/min,
# 1500 mm/min, plunging and ramping at 500) and 6 mm drill (3000 rev/min,
# 300 mm/min), without holders, ramping at the cutting speed and without
# coolant, as camcraft's tools have them. The Masso post is looked for beside
# the bridge folder (masso.cps), as Fusion's own post library may not have it.
import copy, json, os, time

BRIDGE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(BRIDGE, "out", "ops_block")
os.makedirs(OUT, exist_ok=True)
MM = 0.1

doc = app.activeDocument
ui.workspaces.itemById("CAMEnvironment").activate()
cam = adsk.cam.CAM.cast(doc.products.itemByProductType("CAMProductType"))
design = adsk.fusion.Design.cast(doc.products.itemByProductType("DesignProductType"))
body = design.rootComponent.bRepBodies.item(0)

si = cam.setups.createInput(adsk.cam.OperationTypes.MillingOperation)
si.models = [body]
si.name = "top"
setup = cam.setups.add(si)
for name, expr in (
    ("job_stockOffsetMode", "'simple'"),
    ("job_stockOffsetSides", "5 mm"),
    ("job_stockOffsetTop", "2 mm"),
    ("job_stockOffsetBottom", "0 mm"),
    ("wcs_origin_mode", "'modelOrigin'"),
):
    setup.parameters.itemByName(name).expression = expr

lib = adsk.cam.CAMManager.get().libraryManager.toolLibraries
def sample(libname, kind):
    tl = lib.toolLibraryAtURL(adsk.core.URL.create("systemlibraryroot://Samples/" + libname))
    for i in range(tl.count):
        j = json.loads(tl.item(i).toJson())
        if j.get("type") == kind and abs(j["geometry"].get("DC", 0) - 6) < 1e-9:
            return j
    raise LookupError(kind)

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
    return adsk.cam.Tool.createFromJson(json.dumps(j))

flat = make_tool(sample("Milling Tools (Metric)", "flat end mill"), 1, "6mm flat", 18000,
    {"v_f": 1500, "v_f_plunge": 500, "v_f_ramp": 500, "v_f_leadIn": 1500, "v_f_leadOut": 1500,
     "v_f_retract": 1500, "v_f_transition": 1500})
drill = make_tool(sample("Hole Making Tools (Metric)", "drill"), 2, "6mm drill", 3000,
    {"v_f": 300, "v_f_plunge": 300, "v_f_retract": 300})

def planar_at(z_mm, normal_z, pred=lambda p: True):
    for f in body.faces:
        if f.geometry.surfaceType == adsk.core.SurfaceTypes.PlaneSurfaceType:
            p = f.pointOnFace
            ok, n = f.evaluator.getNormalAtPoint(p)
            if abs(p.z - z_mm * MM) < 1e-6 and n.z * normal_z > 0.5 and pred(p):
                return f
    raise LookupError(z_mm)
bottom = planar_at(0, -1)
floors = [planar_at(14, 1, lambda p: p.x < 0), planar_at(12, 1)]
hole_walls = [f for f in body.faces if f.geometry.surfaceType == adsk.core.SurfaceTypes.CylinderSurfaceType
              and abs(f.geometry.radius - 0.3) < 1e-6]

def contours(op):
    v = op.parameters.itemByName("contours").value
    cs = v.getCurveSelections()
    cs.createNewFaceContourSelection().inputGeometry = [bottom]
    v.applyCurveSelections(cs)

def pockets(op):
    v = op.parameters.itemByName("pockets").value
    cs = v.getCurveSelections()
    for floor in floors:
        cs.createNewPocketSelection().inputGeometry = [floor]
    v.applyCurveSelections(cs)

def holes(op):
    op.parameters.itemByName("holeFaces").value.value = hole_walls

made = []
for strategy, name, tool, select in (
    ("face", "face", flat, None),
    ("contour2d", "contour", flat, contours),
    ("pocket2d", "pocket", flat, pockets),
    ("drill", "drill", drill, holes),
):
    oi = setup.operations.createInput(strategy)
    oi.tool = tool
    oi.displayName = name
    op = setup.operations.add(oi)
    if select:
        select(op)
    made.append(op)

fut = cam.generateAllToolpaths(False)
t0 = time.time()
while not fut.isGenerationCompleted and time.time() - t0 < 300:
    adsk.doEvents()
    time.sleep(0.05)

post = os.path.join(BRIDGE, "masso.cps")
for op in made:
    pi = adsk.cam.PostProcessInput.create("ops_block_" + op.name, post, OUT,
        adsk.cam.PostOutputUnitOptions.MillimetersOutput)
    pi.isOpenInEditor = False
    print(op.name, "valid:", op.isToolpathValid, "posted:", cam.postProcess(op, pi))
print(sorted(os.listdir(OUT)))
