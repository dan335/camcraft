# Every strategy a milling setup of Fusion's offers, and every parameter a new
# operation of each starts with: its name, the title Fusion shows, the
# expression it starts as, and its choices where it has them. Written to
# out/params/ in the bridge folder, one JSON file a strategy, with
# strategies.json listing them all. Run through the bridge (see README.md).
#
# This is where camcraft's operations get Fusion's settings and defaults from,
# and PROGRESS.md its list of Fusion's operations. It makes a document of its
# own, a plain block with one setup, and closes it without saving.
import json, os

BRIDGE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(BRIDGE, "out", "params")
os.makedirs(OUT, exist_ok=True)
MM = 0.1


def attr(obj, name):
    try:
        v = getattr(obj, name)
    except Exception as e:
        return "error: %s" % e
    return v if isinstance(v, (bool, int, float, str)) or v is None else str(v)


def choices(param):
    try:
        value = param.value
        if not hasattr(value, "getChoices"):
            return None
        got = value.getChoices()
        # (ok, names, values) or (names, values), by version.
        lists = [list(x) for x in got if isinstance(x, (list, tuple))]
        return {"names": lists[0], "values": lists[1]} if len(lists) >= 2 else str(got)
    except Exception as e:
        return "error: %s" % e


doc = app.documents.add(adsk.core.DocumentTypes.FusionDesignDocumentType)
design = adsk.fusion.Design.cast(app.activeProduct)
root = design.rootComponent
sk = root.sketches.add(root.xYConstructionPlane)
sk.sketchCurves.sketchLines.addTwoPointRectangle(
    adsk.core.Point3D.create(-50 * MM, -35 * MM, 0), adsk.core.Point3D.create(50 * MM, 35 * MM, 0)
)
body = root.features.extrudeFeatures.addSimple(
    sk.profiles.item(0),
    adsk.core.ValueInput.createByReal(20 * MM),
    adsk.fusion.FeatureOperations.NewBodyFeatureOperation,
).bodies.item(0)

ui.workspaces.itemById("CAMEnvironment").activate()
cam = adsk.cam.CAM.cast(doc.products.itemByProductType("CAMProductType"))
si = cam.setups.createInput(adsk.cam.OperationTypes.MillingOperation)
si.models = [body]
setup = cam.setups.add(si)

print("fusion", app.version)
strategies = []
for s in setup.operations.compatibleStrategies:
    strategies.append({"name": attr(s, "name"), "title": attr(s, "title"),
                       "description": attr(s, "description")})
with open(os.path.join(OUT, "strategies.json"), "w", encoding="utf-8") as f:
    json.dump({"fusion": app.version, "strategies": strategies}, f, indent=1)
print(len(strategies), "strategies")

for s in strategies:
    name = s["name"]
    try:
        op = setup.operations.add(setup.operations.createInput(name))
    except Exception as e:
        print("%-28s could not be made: %s" % (name, e))
        continue
    params = []
    for i in range(op.parameters.count):
        p = op.parameters.item(i)
        params.append({
            "name": attr(p, "name"),
            "title": attr(p, "title"),
            "expression": attr(p, "expression"),
            "enabled": attr(p, "isEnabled"),
            "visible": attr(p, "isVisible"),
            "editable": attr(p, "isEditable"),
            "kind": type(p.value).__name__ if attr(p, "value") is not None else None,
            "choices": choices(p),
        })
    with open(os.path.join(OUT, name + ".json"), "w", encoding="utf-8") as f:
        json.dump({"strategy": name, "title": s["title"], "parameters": params}, f, indent=1)
    print("%-28s %-28s %4d parameters" % (name, s["title"], len(params)))
    try:
        op.deleteMe()
    except Exception as e:
        print("  not deleted:", e)

try:
    doc.close(False)
except Exception as e:
    print("document left open:", e)
print("written to", OUT)
