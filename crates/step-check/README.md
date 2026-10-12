# step-check

Check a STEP import against the file it came from.

A STEP importer fails in ways that look fine: a face that did not transfer is a hole
in the part, a unit read wrongly makes it 25.4 times the wrong size, and a surface
evaluated wrongly lands somewhere the part is not. `step-check` reads what the file
itself says, independently of any CAD kernel — its length unit, its faces, its B-rep
vertices, a box its geometry cannot leave, and where its assembly places each
component — and holds a kernel's mesh up against it.

```rust,ignore
let facts = step_check::part21::facts(&std::fs::read_to_string("part.step")?)?;
let mesh = step_check::KernelMesh {
    vertices,       // millimetres
    indices,        // three per triangle, triangles grouped by B-rep face
    face_triangles, // triangles per face, in the kernel's face order
};
match step_check::check(&facts, &mesh, tolerance_mm) {
    Ok(step_check::FileCheck::Passed) => {}
    Ok(step_check::FileCheck::Skipped(why)) => eprintln!("not checked: {why}"),
    Err(problem) => return Err(problem.into()), // a dropped or empty face, a unit error, ...
}
```

It checks that every face arrives with triangles, that nothing lands outside the
file's own geometry, that the mesh reaches every vertex, and that every face lies on
its surface: within four times the tolerance of the plane, cylinder, cone, sphere or
torus it could be, with every such face of the file covered. A face that could lie on a B-spline, a surface of revolution or an
extrusion is held to its box alone. To measure an importer everywhere, score it with
[step-conformance](https://github.com/dan335/step-conformance).

Written for [camcraft](https://github.com/dan335/camcraft)'s STEP import and kept free
of it. MIT or Apache-2.0.
