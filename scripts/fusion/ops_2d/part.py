# The ops_2d test part, built in a new Fusion document and exported as STEP:
# fixtures/step/ops_2d.step. Run through the bridge (see ../README.md).
#
# A 140 x 100 x 20 mm block, centred on the origin with its bottom at Z0, with
# something for each of Fusion's other 2D operations to cut:
#
#   bore       a through hole 16 across at (-55, 35), and a blind one 14 across
#              and 10 deep at (-30, 35)
#   circular   a round pocket 30 across and 8 deep at (0, 32)
#   slot       a slot 8 wide and 6 deep between (25, 35) and (55, 35), and one
#              8 wide and 4 deep, 30 between its ends' centres, through (45, 8)
#              at 30 degrees
#   adaptive   a pocket 50 x 36 x 8 deep, corners R5, from (-62, -40) to
#              (-12, -4), with a round island 10 across left standing in its
#              middle; and a notch open to two sides, 30 x 22 x 6 deep, in the
#              corner at (70, -50)
#   chamfer    the pocket's rim, chamfered 1 mm; the island's is left sharp
#   trace      two V grooves, 90 degrees and 2 deep, each end half a cone:
#              straight from (-5, -15) to (30, -15), and half a circle of
#              radius 12 about (10, -33)
#
# Fusion's API is in centimetres; everything here is written in millimetres.
import math, os

BRIDGE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(BRIDGE, "out", "ops_2d")
os.makedirs(OUT, exist_ok=True)

MM = 0.1
TOP = 20.0
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
sk.sketchCurves.sketchLines.addTwoPointRectangle(P(-70, -50), P(70, 50))
body = feats.extrudeFeatures.addSimple(sk.profiles.item(0), V(TOP), F.NewBodyFeatureOperation).bodies.item(0)
body.name = "part"

def offset_plane(base, by):
    pin = root.constructionPlanes.createInput()
    pin.setByOffset(base, V(by))
    return root.constructionPlanes.add(pin)

top = offset_plane(root.xYConstructionPlane, TOP)

def the_body():
    return root.bRepBodies.item(0)

def cut_down(profiles, depth):
    """Cuts `profiles` (one, or a list) down from the top by `depth`."""
    if isinstance(profiles, list):
        oc = adsk.core.ObjectCollection.create()
        for p in profiles:
            oc.add(p)
        profiles = oc
    ei = feats.extrudeFeatures.createInput(profiles, F.CutFeatureOperation)
    ei.setOneSideExtent(
        adsk.fusion.DistanceExtentDefinition.create(V(depth)),
        adsk.fusion.ExtentDirections.NegativeExtentDirection,
    )
    ei.participantBodies = [the_body()]
    return feats.extrudeFeatures.add(ei)

def on_top(name):
    s = root.sketches.add(top)
    s.name = name
    return s, (lambda x, y: s.modelToSketchSpace(P(x, y, TOP)))

def smallest(sketch):
    """The sketch's profile of least area: the inside of a shape drawn alone."""
    return min((sketch.profiles.item(i) for i in range(sketch.profiles.count)),
               key=lambda p: p.areaProperties().area)

def largest(sketch):
    return max((sketch.profiles.item(i) for i in range(sketch.profiles.count)),
               key=lambda p: p.areaProperties().area)

def circle(name, x, y, across, depth):
    s, S = on_top(name)
    s.sketchCurves.sketchCircles.addByCenterRadius(S(x, y), across / 2 * MM)
    cut_down(s.profiles.item(0), depth)

def slot(name, a, b, width, depth):
    """A slot `width` wide with round ends centred on `a` and `b`."""
    s, S = on_top(name)
    r = width / 2
    dx, dy = b[0] - a[0], b[1] - a[1]
    n = math.hypot(dx, dy)
    ux, uy = dx / n, dy / n      # along the slot
    px, py = -uy, ux             # to its left
    a1, a2 = (a[0] + px * r, a[1] + py * r), (a[0] - px * r, a[1] - py * r)
    b1, b2 = (b[0] + px * r, b[1] + py * r), (b[0] - px * r, b[1] - py * r)
    lines, arcs = s.sketchCurves.sketchLines, s.sketchCurves.sketchArcs
    lines.addByTwoPoints(S(*a1), S(*b1))
    lines.addByTwoPoints(S(*a2), S(*b2))
    arcs.addByThreePoints(S(*b1), S(b[0] + ux * r, b[1] + uy * r), S(*b2))
    arcs.addByThreePoints(S(*a1), S(a[0] - ux * r, a[1] - uy * r), S(*a2))
    cut_down(s.profiles.item(0), depth)

# Bore and circular.
circle("through hole", -55, 35, 16, TOP)
circle("blind hole", -30, 35, 14, 10)
circle("round pocket", 0, 32, 30, 8)

# Slots.
slot("slot", (25, 35), (55, 35), 8, 6)
c, half, ang = (45.0, 8.0), 15.0, math.radians(30)
slot("slot at 30", (c[0] - half * math.cos(ang), c[1] - half * math.sin(ang)),
     (c[0] + half * math.cos(ang), c[1] + half * math.sin(ang)), 8, 4)

# The pocket with its island: the rectangle less the circle is the larger of
# the sketch's two profiles.
s, S = on_top("pocket")
s.sketchCurves.sketchLines.addTwoPointRectangle(S(-62, -40), S(-12, -4))
s.sketchCurves.sketchCircles.addByCenterRadius(S(-37, -22), 5 * MM)
cut_down(largest(s), 8)

corners = adsk.core.ObjectCollection.create()
for e in the_body().edges:
    p, q = e.startVertex.geometry, e.endVertex.geometry
    if abs(p.x - q.x) < 1e-6 and abs(p.y - q.y) < 1e-6 and abs(p.z - q.z) > 1e-6:
        x, y = p.x / MM, p.y / MM
        if (abs(x + 62) < 1e-3 or abs(x + 12) < 1e-3) and (abs(y + 40) < 1e-3 or abs(y + 4) < 1e-3):
            corners.add(e)
assert corners.count == 4, corners.count
fi = feats.filletFeatures.createInput()
fi.edgeSetInputs.addConstantRadiusEdgeSet(corners, V(5), False)
feats.filletFeatures.add(fi)

# The notch, open to the block's +X and -Y sides.
s, S = on_top("notch")
s.sketchCurves.sketchLines.addTwoPointRectangle(S(40, -50), S(70, -28))
cut_down(s.profiles.item(0), 6)

# The pocket's rim: every edge at the top whose middle is on the pocket's
# outline and not the island's.
rim = adsk.core.ObjectCollection.create()
for e in the_body().edges:
    m = e.pointOnEdge
    x, y, z = m.x / MM, m.y / MM, m.z / MM
    if abs(z - TOP) > 1e-6:
        continue
    inside = -62 - 1e-3 <= x <= -12 + 1e-3 and -40 - 1e-3 <= y <= -4 + 1e-3
    island = math.hypot(x + 37, y + 22) < 5 + 1e-3
    if inside and not island:
        rim.add(e)
assert rim.count == 8, rim.count
ci = feats.chamferFeatures.createInput2()
ci.chamferEdgeSets.addEqualDistanceChamferEdgeSet(rim, V(1), False)
feats.chamferFeatures.add(ci)

# The V grooves: a triangle 4 wide at the top and 2 deep, cut along a line and
# turned half way round an upright axis.
#
# Straight: its section on the plane x = -5, facing +X, cut 35 along.
yz = offset_plane(root.yZConstructionPlane, -5)
s = root.sketches.add(yz)
s.name = "straight groove"
pts = [s.modelToSketchSpace(P(-5, y, z)) for y, z in ((-17, TOP), (-13, TOP), (-15, TOP - 2))]
for i in range(3):
    s.sketchCurves.sketchLines.addByTwoPoints(pts[i], pts[(i + 1) % 3])
ei = feats.extrudeFeatures.createInput(s.profiles.item(0), F.CutFeatureOperation)
ei.setOneSideExtent(
    adsk.fusion.DistanceExtentDefinition.create(V(35)),
    adsk.fusion.ExtentDirections.PositiveExtentDirection,
)
ei.participantBodies = [the_body()]
feats.extrudeFeatures.add(ei)

# Round: its section on the plane y = -33, at x = 22, turned about the upright
# line through (10, -33).
xz = offset_plane(root.xZConstructionPlane, -33)
s = root.sketches.add(xz)
s.name = "round groove"
pts = [s.modelToSketchSpace(P(x, -33, z)) for x, z in ((20, TOP), (24, TOP), (22, TOP - 2))]
for i in range(3):
    s.sketchCurves.sketchLines.addByTwoPoints(pts[i], pts[(i + 1) % 3])
axis = s.sketchCurves.sketchLines.addByTwoPoints(
    s.modelToSketchSpace(P(10, -33, 0)), s.modelToSketchSpace(P(10, -33, TOP + 5)))
axis.isConstruction = True
ri = feats.revolveFeatures.createInput(smallest(s), axis, F.CutFeatureOperation)
ri.setAngleExtent(False, adsk.core.ValueInput.createByString("180 deg"))
ri.participantBodies = [the_body()]
feats.revolveFeatures.add(ri)

# Each groove ends in half a cone, the shape the V-bit's point leaves where it
# stops: a groove ending square would be cut past by any tool that reaches
# its end. The triangle's upright side is the axis it turns about.
def cone_end(x, y):
    plane = offset_plane(root.xZConstructionPlane, y)
    s = root.sketches.add(plane)
    s.name = "groove end"
    a, b, c = (s.modelToSketchSpace(P(px, y, pz)) for px, pz in ((x, TOP), (x + 2, TOP), (x, TOP - 2)))
    lines = s.sketchCurves.sketchLines
    lines.addByTwoPoints(a, b)
    lines.addByTwoPoints(b, c)
    axis = lines.addByTwoPoints(c, a)
    ri = feats.revolveFeatures.createInput(s.profiles.item(0), axis, F.CutFeatureOperation)
    ri.setAngleExtent(False, adsk.core.ValueInput.createByString("360 deg"))
    ri.participantBodies = [the_body()]
    feats.revolveFeatures.add(ri)

for end in ((-5, -15), (30, -15), (22, -33), (-2, -33)):
    cone_end(*end)

b = the_body()
box = b.boundingBox
print("faces:", b.faces.count, "volume mm3:", round(b.volume / MM ** 3, 2))
print("box mm:", [round(v / MM, 4) for v in (box.minPoint.x, box.minPoint.y, box.minPoint.z,
                                              box.maxPoint.x, box.maxPoint.y, box.maxPoint.z)])
em = design.exportManager
path = os.path.join(OUT, "ops_2d.step")
em.execute(em.createSTEPExportOptions(path, root))
print("step:", path)
