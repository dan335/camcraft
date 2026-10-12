//! Model import for camcraft: STL and STEP to triangle meshes.
//!
//! Position in the dependency DAG: **leaf consumer**. Depends on `camcraft-geom`, and
//! on `step-check`, which holds the checks of a STEP import against its file;
//! only `camcraft-session` depends on this crate, and through it the two binaries.
//!
//! That isolation is deliberate and load-bearing. Whatever the STEP backend turns
//! out to cost to build, the inner development loop on `geom` / `sim` / `core` /
//! `post` never pays it. This is structural, not a feature flag: STEP is a
//! first-class input, because it is how parts arrive from Plasticity.

pub mod step;
pub mod stl;
mod weld;
