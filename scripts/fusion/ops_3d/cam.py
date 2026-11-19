# Fusion's Ramp, Pencil, Project, Morph and Parallel on the ops_3d part, each as
# a new operation of Fusion's starts but for what its name says, posted with
# Fusion's Masso post: fixtures/nc/ops_3d_*.nc. Run through the bridge after
# part.py (see ../README.md). It finds the document part.py made by its one
# body, named ops_3d, and works in that document alone.
#
# The setup's stock is 5 mm round the part and level with its top, its work
# zero the model's origin. The tools are the sample library's 6 mm ball end
# mill, T2, and its 12 mm one, T3 (18,000 rev/min, 1500 mm/min, plunging and
# ramping at 500), without a holder and without coolant, as camcraft's tools
# have them. The Masso post is looked for beside the bridge's inbox
# (masso.cps).
#
#   ramp        a new Ramp: Maximum Stepdown 1 mm, Ramping Angle the tool's 2
#               degrees
#   ramp_step2  the same, Maximum Stepdown 2 mm
#   ramp_angle  the same, Ramping Angle 0.5 degrees: shallower than a stepdown
#               each time round every wall but the outside's
#
#   pencil              a new Pencil with the 12 mm ball, which is larger than
#                       the pocket's R4 fillets and its R5 corner, so is held off
#                       them: Bitangency Angle 20 degrees, no stepovers
#   pencil_ball6        the same with the 6 mm ball, which fits the fillets and
#                       touches twice only round the dome's foot and in the
#                       sharp corners
#   pencil_bitangency   the 12 mm ball, Bitangency Angle 40 degrees
#   pencil_stepovers    the 6 mm ball, Number of Stepovers 2, the stepover half
#                       the tool
#
#   project             a new Project with the 6 mm ball along three curves
#                       picked on the part: the dome's foot, the floor's edge
#                       where its fillets start, and the R8 round's lower edge
#   project_other_way   the same, Direction Other Way
#   project_axial       the same, Axial Offset 1 mm
#   project_ball12      the same with the 12 mm ball
#
#   morph               a new Morph with the 6 mm ball between two closed
#                       curves picked on the part: the dome's foot, then the
#                       floor's edge
#   morph_one_way       the same, Direction One Way
#   morph_open          a new Morph between two open curves: the R8 round's
#                       upper edge, then its lower edge, reverted
#
#   parallel                a new Parallel with the 6 mm ball, no surface groups
#   parallel_avoid_dome     the dome avoided, radial and axial offsets 1 mm
#   parallel_avoid_dome_0   the dome avoided, no offsets
#   parallel_floor_only     the model avoided, the pocket's floor machined
#   parallel_floor_leave    the floor machined with stock of its own, 0.5 mm
#                           radial and axial
#   parallel_avoid_top      the block's top avoided, axial offset 2 mm
#
# A surface group is added as the Surface Manager adds one: a direct selection
# of faces, machined or avoided, with its own radial and axial offsets, which
# the API takes in millimetres (not centimetres, as its lengths elsewhere). The
# dome is the sphere face, the floor the plane at Z10, the top the plane at
# Z20.
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
ball12 = make_tool(sample("ball end mill", 12), 3, "12mm ball", 18000, FEEDS)

def edge_through(x, y, z):
    """The edge whose own point is nearest (x, y, z), within half a millimetre."""
    best, far = None, 0.5 * MM
    for e in body.edges:
        m = e.pointOnEdge
        d = ((m.x - x * MM) ** 2 + (m.y - y * MM) ** 2 + (m.z - z * MM) ** 2) ** 0.5
        if d < far:
            best, far = e, d
    if best is None:
        raise LookupError((x, y, z))
    return best

def picking(*chains):
    """Curves picked as the panel picks them, each (edge, open, reverted): a
    click on an edge takes its chain, closed round the loop it is in unless
    the chain is open, and run the other way where it is reverted."""
    def pick(op):
        v = op.parameters.itemByName("curves").value
        cs = v.getCurveSelections()
        for edge, is_open, reverted in chains:
            chain = cs.createNewChainSelection()
            chain.isOpen = is_open
            chain.isReverted = reverted
            chain.inputGeometry = [edge]
        v.applyCurveSelections(cs)
    return pick

# The dome's foot, the circle where it meets the floor; the floor's edge,
# where the fillets round it start, a loop from its edge along +Y; and the R8
# round's lower edge, along the block's side at X30, and its upper edge,
# where it meets the top at X22. Fusion's new closed chains run clockwise
# seen from above, and an open one from its edge's start: the round's upper
# edge from Y20 to Y-20, its lower from Y-20 to Y20.
DOME_FOOT = edge_through(-15, 0, 10)
FLOOR_EDGE = edge_through(-6.13, 9, 10)
ROUND_LOW = edge_through(30, 0, 12)
ROUND_TOP = edge_through(22, 0, 20)

# Project's curves: each has the roughing's leftovers to cut where the tool
# rests over it, round the dome's foot, in the fillets and on the round.
project_curves = picking((DOME_FOOT, False, False), (FLOOR_EDGE, False, False),
                         (ROUND_LOW, True, False))
# Morph's two closed curves: the passes between them cover the floor and the
# dome's flank, from the circle to the floor's edge.
morph_closed = picking((DOME_FOOT, False, False), (FLOOR_EDGE, False, False))
# Morph's two open ones, the round's edges, the lower reverted to run as the
# upper does: Fusion's Morph gives no passes for two that run opposite ways.
morph_open = picking((ROUND_TOP, True, False), (ROUND_LOW, True, True))

T = adsk.core.SurfaceTypes
M = adsk.cam.MachiningMode
AVOID, MACHINE = M.Avoid_MachiningMode, M.Machine_MachiningMode

def faces(kind, z=None):
    """The body's faces of surface type `kind`, those level at `z` alone if
    given."""
    got = []
    for f in body.faces:
        if f.geometry.surfaceType != kind:
            continue
        b = f.boundingBox
        if z is not None and not (abs(b.minPoint.z - z * MM) < 1e-6 and abs(b.maxPoint.z - z * MM) < 1e-6):
            continue
        got.append(f)
    if not got:
        raise LookupError((kind, z))
    return got

DOME = faces(T.SphereSurfaceType)
FLOOR = faces(T.PlaneSurfaceType, 10)
TOP = faces(T.PlaneSurfaceType, 20)

def grouping(model, *groups):
    """Surface groups set as the Surface Manager sets them: the model's mode,
    or as it starts where `model` is None, and groups added, each (mode,
    faces, radial mm, axial mm)."""
    def pick(op):
        v = op.parameters.itemByName("checkSurfaceSelectionSets").value
        gs = v.getMachineAvoidGroups()
        if model is not None:
            gs.defaultGroup(adsk.cam.DefaultGroupType.Model_GroupType).machineMode = model
        for mode, geometry, radial, axial in groups:
            g = gs.createNewMachineAvoidDirectSelectionGroup()
            g.machineMode = mode
            g.inputGeometry = geometry
            g.radialOffset, g.axialOffset = radial, axial
        v.applyMachineAvoidGroups(gs)
    return pick

made = []
for name, strategy, tool, select, settings in (
    ("ramp", "ramp", ball6, None, {}),
    ("ramp_step2", "ramp", ball6, None, {"maximumStepdown": "2 mm"}),
    ("ramp_angle", "ramp", ball6, None, {"rampAngle": "0.5 deg"}),
    ("pencil", "pencil_new", ball12, None, {}),
    ("pencil_ball6", "pencil_new", ball6, None, {}),
    ("pencil_bitangency", "pencil_new", ball12, None, {"bitangentAngle": "40 deg"}),
    ("pencil_stepovers", "pencil_new", ball6, None, {"numberOfStepovers": "2"}),
    ("project", "project", ball6, project_curves, {}),
    ("project_other_way", "project", ball6, project_curves, {"direction": "'other way'"}),
    ("project_axial", "project", ball6, project_curves, {"axialOffset": "1 mm"}),
    ("project_ball12", "project", ball12, project_curves, {}),
    ("morph", "morph", ball6, morph_closed, {}),
    ("morph_one_way", "morph", ball6, morph_closed, {"direction": "'one way'"}),
    ("morph_open", "morph", ball6, morph_open, {}),
    ("parallel", "parallel", ball6, None, {}),
    ("parallel_avoid_dome", "parallel", ball6, grouping(None, (AVOID, DOME, 1, 1)), {}),
    ("parallel_avoid_dome_0", "parallel", ball6, grouping(None, (AVOID, DOME, 0, 0)), {}),
    ("parallel_floor_only", "parallel", ball6, grouping(AVOID, (MACHINE, FLOOR, 0, 0)), {}),
    ("parallel_floor_leave", "parallel", ball6, grouping(None, (MACHINE, FLOOR, 0.5, 0.5)), {}),
    ("parallel_avoid_top", "parallel", ball6, grouping(None, (AVOID, TOP, 0, 2)), {}),
):
    try:
        oi = setup.operations.createInput(strategy)
        oi.tool = tool
        oi.displayName = name
        op = setup.operations.add(oi)
        if select:
            select(op)
        for key, expr in settings.items():
            op.parameters.itemByName(key).expression = expr
        made.append(op)
    except Exception:
        print(name, "NOT MADE:", traceback.format_exc().strip().splitlines()[-1])

fut = cam.generateAllToolpaths(False)
t0 = time.time()
while not fut.isGenerationCompleted and time.time() - t0 < 900:
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
    for key in ("maximumStepdown", "rampAngle", "flatAreaMachining", "direction",
                "bitangentAngle", "numberOfStepovers", "stepover", "overthickness",
                "axialOffset", "boundaryMode", "boundaryContainment", "morphMode"):
        p = op.parameters.itemByName(key)
        if p is not None:
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
