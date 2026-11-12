# The ops_block test part, built in a new Fusion document and exported as STEP:
# fixtures/step/ops_block.step. Run through the bridge (see ../README.md).
#
# A 100 x 70 x 20 mm block, centred on the origin with its bottom at Z0; a pocket
# 40 x 36 x 6 mm at (-20, 0) with R6 corners; a round pocket 24 across and 8 deep
# at (25, 10); two holes 6 across and 12 deep at (15, -22) and (35, -22).
#
# Fusion's API is in centimetres; everything here is written in millimetres.
import os

OUT = os.path.expanduser("~/camcraft-fusion-bridge/out/ops_block")
os.makedirs(OUT, exist_ok=True)

MM = 0.1
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
sk.name = "block"
sk.sketchCurves.sketchLines.addTwoPointRectangle(P(-50, -35), P(50, 35))
body = feats.extrudeFeatures.addSimple(sk.profiles.item(0), V(20), F.NewBodyFeatureOperation).bodies.item(0)
body.name = "part"

pin = root.constructionPlanes.createInput()
pin.setByOffset(root.xYConstructionPlane, V(20))
top = root.constructionPlanes.add(pin)

def cut_down(profile, depth):
    ei = feats.extrudeFeatures.createInput(profile, F.CutFeatureOperation)
    ei.setOneSideExtent(
        adsk.fusion.DistanceExtentDefinition.create(V(depth)),
        adsk.fusion.ExtentDirections.NegativeExtentDirection,
    )
    ei.participantBodies = [root.bRepBodies.item(0)]
    return feats.extrudeFeatures.add(ei)

def on_top(name):
    s = root.sketches.add(top)
    s.name = name
    return s, (lambda x, y: s.modelToSketchSpace(P(x, y, 20)))

s, S = on_top("rect pocket")
s.sketchCurves.sketchLines.addTwoPointRectangle(S(-40, -18), S(0, 18))
cut_down(s.profiles.item(0), 6)

corners = adsk.core.ObjectCollection.create()
for e in root.bRepBodies.item(0).edges:
    p, q = e.startVertex.geometry, e.endVertex.geometry
    if abs(p.x - q.x) < 1e-6 and abs(p.y - q.y) < 1e-6 and abs(p.z - q.z) > 1e-6:
        x, y = p.x / MM, p.y / MM
        if (abs(x + 40) < 1e-3 or abs(x) < 1e-3) and abs(abs(y) - 18) < 1e-3:
            corners.add(e)
fi = feats.filletFeatures.createInput()
fi.edgeSetInputs.addConstantRadiusEdgeSet(corners, V(6), False)
feats.filletFeatures.add(fi)

s, S = on_top("round pocket")
s.sketchCurves.sketchCircles.addByCenterRadius(S(25, 10), 12 * MM)
cut_down(s.profiles.item(0), 8)

s, S = on_top("holes")
for x in (15, 35):
    s.sketchCurves.sketchCircles.addByCenterRadius(S(x, -22), 3 * MM)
for i in range(s.profiles.count):
    cut_down(s.profiles.item(i), 12)

b = root.bRepBodies.item(0)
print("faces:", b.faces.count, "volume mm3:", round(b.volume / MM ** 3, 2))
em = design.exportManager
path = os.path.join(OUT, "ops_block.step")
em.execute(em.createSTEPExportOptions(path, root))
print("step:", path)
