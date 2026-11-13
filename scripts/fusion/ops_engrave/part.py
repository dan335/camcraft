# The ops_engrave test part, built in a new Fusion document and exported as
# STEP: fixtures/step/ops_engrave.step. Run through the bridge (see
# ../README.md).
#
# A plate 80 x 50 x 10 mm, centred on the origin with its bottom at Z0, with
# three shapes sunk 1.5 mm into its top for Fusion's Engrave to cut with a V:
#
#   a triangle, (-30, -15), (-10, -15), (-20, 10): sharp corners to run into
#   an L, 4 wide: a stroke narrower than the V at its full depth, with an
#       inside and an outside corner
#   a rectangle 14 x 24, from (24, -12) to (38, 12): wider than the V reaches,
#       so cut round at its full depth
#
# Fusion's API is in centimetres; everything here is written in millimetres.
import os

BRIDGE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(BRIDGE, "out", "ops_engrave")
os.makedirs(OUT, exist_ok=True)

MM = 0.1
TOP = 10.0
def P(x, y, z=0.0):
    return adsk.core.Point3D.create(x * MM, y * MM, z * MM)
def V(v):
    return adsk.core.ValueInput.createByReal(v * MM)

doc = app.documents.add(adsk.core.DocumentTypes.FusionDesignDocumentType)
design = adsk.fusion.Design.cast(app.activeProduct)
design.designType = adsk.fusion.DesignTypes.ParametricDesignType
design.fusionUnitsManager.distanceDisplayUnits = adsk.fusion.DistanceUnits.MillimeterDistanceUnits
root = design.rootComponent
feats = root.features
F = adsk.fusion.FeatureOperations

sk = root.sketches.add(root.xYConstructionPlane)
sk.name = "plate"
sk.sketchCurves.sketchLines.addTwoPointRectangle(P(-40, -25), P(40, 25))
body = feats.extrudeFeatures.addSimple(sk.profiles.item(0), V(TOP), F.NewBodyFeatureOperation).bodies.item(0)
body.name = "plate"

pin = root.constructionPlanes.createInput()
pin.setByOffset(root.xYConstructionPlane, V(TOP))
top = root.constructionPlanes.add(pin)

def sunk(name, points, depth=1.5):
    s = root.sketches.add(top)
    s.name = name
    pts = [s.modelToSketchSpace(P(x, y, TOP)) for x, y in points]
    lines = s.sketchCurves.sketchLines
    for i in range(len(pts)):
        lines.addByTwoPoints(pts[i], pts[(i + 1) % len(pts)])
    ei = feats.extrudeFeatures.createInput(s.profiles.item(0), F.CutFeatureOperation)
    ei.setOneSideExtent(
        adsk.fusion.DistanceExtentDefinition.create(V(depth)),
        adsk.fusion.ExtentDirections.NegativeExtentDirection,
    )
    ei.participantBodies = [root.bRepBodies.item(0)]
    feats.extrudeFeatures.add(ei)

sunk("triangle", [(-30, -15), (-10, -15), (-20, 10)])
sunk("L", [(0, -15), (20, -15), (20, -11), (4, -11), (4, 10), (0, 10)])
sunk("rectangle", [(24, -12), (38, -12), (38, 12), (24, 12)])

b = root.bRepBodies.item(0)
print("faces:", b.faces.count, "volume mm3:", round(b.volume / MM ** 3, 2))
em = design.exportManager
path = os.path.join(OUT, "ops_engrave.step")
em.execute(em.createSTEPExportOptions(path, root))
print("step:", path)
