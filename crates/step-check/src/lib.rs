//! Check a STEP import against the file it came from.
//!
//! A STEP importer fails in ways that look fine. A face that did not transfer is a
//! hole in the part; a unit read wrongly makes the part 25.4 times the wrong size; a
//! surface evaluated wrongly lands somewhere the part is not. None of that shows in
//! the mesh alone. So [`part21::facts`] reads what the file itself says,
//! independently of any CAD kernel: its length unit, how many faces it defines,
//! where its B-rep vertices are, and a box its geometry cannot leave, each component
//! placed where the file's assembly puts it. [`check`] then holds a kernel's mesh up
//! against those facts:
//!
//! - **Every face arrives, with triangles.** At least as many faces as the file
//!   defines, and none of them empty.
//! - **Nothing lands outside the file's own geometry.** A NURBS surface lies within
//!   the convex hull of its control points and a circle within its radius of its
//!   centre, so the mesh must fit inside the box around the file's points, grown for
//!   circles, surfaces of revolution and extrusions.
//! - **The mesh reaches every vertex.** B-rep vertices lie on the part, so the mesh
//!   must extend at least as far as they do. This catches a unit error that makes the
//!   part too small, which a shrunken mesh inside the box would otherwise pass.
//! - **Every face lies on its surface.** Each face the kernel gave must fit inside
//!   the box of some face of the file, and where every face it could be lies on a
//!   plane, cylinder, cone, sphere or torus, every triangle's corners and centre must
//!   lie within [`SLACK`] times the tolerance of one of those surfaces. And every
//!   face of the file on such a surface must have some of the mesh on it, or a face
//!   flattened onto its neighbour would pass. This sees surface that is wrong inside
//!   the part, which the boxes cannot.
//!
//! A face that could lie on a B-spline, a surface of revolution or an extrusion is
//! held to its box alone, so surface wrong inside the part there still passes.
//!
//! This is the checking half of camcraft's STEP import, kept free of camcraft and of
//! any kernel so that any importer can run it on its own output.

mod check;
pub mod part21;
mod surface;

pub use check::{FileCheck, KernelMesh, Problem, SLACK, check};
pub use surface::Surface;
