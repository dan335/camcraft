# The ops_3d test part, built in a new Fusion document and exported as STEP:
# fixtures/step/ops_3d.step. Run through the bridge (see ../README.md).
#
# A 60 x 40 x 20 mm block, centred on the origin with its bottom at Z0, with
# something for each of Fusion's 3D finishing operations to follow:
#
#   pocket    38 x 26 at the top, from (-26, -13) to (12, 13), 10 deep: its -X
#             wall drafted 15 degrees (the floor 2.68 narrower there), the rest
#             upright; its corner at (12, -13) R5, the other three left sharp;
#             the floor's edges R4, concave fillets a 6 mm ball can follow.
#             (Rounding the drafted wall's corners too makes Fusion blend them
#             into the floor's fillets with B-spline faces whose edges it
#             writes 0.0096 mm off them, which the importer refuses finer than
#             0.04 mm.)
#   dome      a sphere's cap on the floor, 14 across and 5 tall, about (-8, 0):
#             its foot a sharp concave circle on the floor
#   round     the block's top edge at +X rounded R8: ground going from level to
#             upright, curved, outside the pocket
#
# The outer walls are upright and sharp-cornered. Fusion's API is in
# centimetres; everything here is written in millimetres.
import math, os

BRIDGE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(BRIDGE, "out", "ops_3d")
os.makedirs(OUT, exist_ok=True)

MM = 0.1
TOP = 20.0
FLOOR = 10.0
DRAFT = math.radians(15)
def P(x, y, z=0.0):
    return adsk.core.Point3D.create(x * MM, y * MM, z * MM)
def V(v):
    return adsk.core.ValueInput.createByReal(v * MM)

# A document an earlier run of this script made is closed unsaved first, so
# ops_3d/cam.py finds the one part. It is told by its one body, ops_3d.
def made_here(d):
    try:
        des = adsk.fusion.Design.cast(d.products.itemByProductType("DesignProductType"))
        bodies = des.rootComponent.bRepBodies
        return bodies.count == 1 and bodies.item(0).name == "ops_3d"
    except Exception:
        return False
for old in [app.documents.item(i) for i in range(app.documents.count)]:
    if made_here(old):
        old.close(False)

doc = app.documents.add(adsk.core.DocumentTypes.FusionDesignDocumentType)
design = adsk.fusion.Design.cast(app.activeProduct)
design.designType = adsk.fusion.DesignTypes.ParametricDesignType
design.fusionUnitsManager.distanceDisplayUnits = adsk.fusion.DistanceUnits.MillimeterDistanceUnits
root = design.rootComponent
feats = root.features
F = adsk.fusion.FeatureOperations

sk = root.sketches.add(root.xYConstructionPlane)
sk.name = "block"
sk.sketchCurves.sketchLines.addTwoPointRectangle(P(-30, -20), P(30, 20))
body = feats.extrudeFeatures.addSimple(sk.profiles.item(0), V(TOP), F.NewBodyFeatureOperation).bodies.item(0)
body.name = "ops_3d"

def the_body():
    return root.bRepBodies.item(0)

def collection(items):
    oc = adsk.core.ObjectCollection.create()
    for i in items:
        oc.add(i)
    return oc

def fillet(edges, radius):
    edges = list(edges)
    fi = feats.filletFeatures.createInput()
    fi.edgeSetInputs.addConstantRadiusEdgeSet(collection(edges), V(radius), True)
    feats.filletFeatures.add(fi)
    return len(edges)

# The pocket: its section in the plane y = 0, a trapezoid whose -X side leans
# out 15 degrees going up, cut 13 each way along Y.
s = root.sketches.add(root.xZConstructionPlane)
s.name = "pocket"
foot = -26 + (TOP - FLOOR) * math.tan(DRAFT)
pts = [s.modelToSketchSpace(P(x, 0, z)) for x, z in ((-26, TOP), (12, TOP), (12, FLOOR), (foot, FLOOR))]
for i in range(4):
    s.sketchCurves.sketchLines.addByTwoPoints(pts[i], pts[(i + 1) % 4])
ei = feats.extrudeFeatures.createInput(s.profiles.item(0), F.CutFeatureOperation)
ei.setSymmetricExtent(V(13), False)
ei.participantBodies = [the_body()]
feats.extrudeFeatures.add(ei)

# The pocket's corner at (12, -13): the edge from its rim down to its floor
# there.
def is_corner(e):
    p, q = e.startVertex.geometry, e.endVertex.geometry
    lo, hi = (p, q) if p.z < q.z else (q, p)
    if abs(lo.z - FLOOR * MM) > 1e-6 or abs(hi.z - TOP * MM) > 1e-6:
        return False
    if abs(abs(lo.y) - 13 * MM) > 1e-6 or abs(hi.y - lo.y) > 1e-6:
        return False
    return abs(lo.x - 12 * MM) < 1e-6 and lo.y < 0
corners = [e for e in the_body().edges if is_corner(e)]
assert len(corners) == 1, len(corners)
fillet(corners, 5)

# The floor's edges, all round it.
def floor_face():
    for f in the_body().faces:
        if f.geometry.surfaceType == adsk.core.SurfaceTypes.PlaneSurfaceType:
            p = f.pointOnFace
            if abs(p.z - FLOOR * MM) < 1e-6:
                return f
    raise LookupError("floor")
assert fillet(floor_face().edges, 4) >= 4

# The dome: a cap 14 across and 5 tall, a sphere of radius (7^2 + 5^2) / 10,
# turned about the upright line through (-8, 0) and joined to the floor.
R = (7.0 ** 2 + 5.0 ** 2) / (2 * 5.0)
cz = FLOOR + 5.0 - R
s = root.sketches.add(root.xZConstructionPlane)
s.name = "dome"
S = lambda x, z: s.modelToSketchSpace(P(x, 0, z))
lines, arcs = s.sketchCurves.sketchLines, s.sketchCurves.sketchArcs
axis = lines.addByTwoPoints(S(-8, FLOOR), S(-8, FLOOR + 5))
lines.addByTwoPoints(S(-8, FLOOR), S(-1, FLOOR))
a = math.asin((FLOOR - cz) / R)
mid = math.radians(45) + a / 2
arcs.addByThreePoints(S(-1, FLOOR), S(-8 + R * math.cos(mid), cz + R * math.sin(mid)), S(-8, FLOOR + 5))
ri = feats.revolveFeatures.createInput(s.profiles.item(0), axis, F.JoinFeatureOperation)
ri.setAngleExtent(False, adsk.core.ValueInput.createByString("360 deg"))
ri.participantBodies = [the_body()]
feats.revolveFeatures.add(ri)

# The block's top edge at +X.
def top_edge_at_x(x):
    for e in the_body().edges:
        p, q = e.startVertex.geometry, e.endVertex.geometry
        if all(abs(v.z - TOP * MM) < 1e-6 and abs(v.x - x * MM) < 1e-6 for v in (p, q)) and abs(p.y - q.y) > 30 * MM:
            return e
    raise LookupError("top edge at %s" % x)
fillet([top_edge_at_x(30)], 8)

b = the_body()
box = b.boundingBox
print("faces:", b.faces.count, "edges:", b.edges.count, "volume mm3:", round(b.volume / MM ** 3, 3))
print("box mm:", [round(v / MM, 4) for v in (box.minPoint.x, box.minPoint.y, box.minPoint.z,
                                              box.maxPoint.x, box.maxPoint.y, box.maxPoint.z)])
kinds = {}
for f in b.faces:
    k = str(f.geometry.surfaceType)
    kinds[k] = kinds.get(k, 0) + 1
print("surface types:", kinds)
em = design.exportManager
path = os.path.join(OUT, "ops_3d.step")
em.execute(em.createSTEPExportOptions(path, root))
print("step:", path)
