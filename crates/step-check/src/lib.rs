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
//!
//! The geometric checks are boxes: they catch surface thrown outside the part, not
//! surface that is wrong inside it.
//!
//! This is the checking half of camcraft's STEP import, kept free of camcraft and of
//! any kernel so that any importer can run it on its own output.

mod check;
pub mod part21;

pub use check::{FileCheck, KernelMesh, Problem, check};
