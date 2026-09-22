//! Material removal simulation for camcraft.
//!
//! Position in the dependency DAG: depends on `camcraft-geom` only.
//!
//! This crate is two things that are deliberately the same code:
//!
//! 1. **The stock updater.** Simulating a toolpath *is* how stock is updated. Rest
//!    machining is therefore not a feature; it is what happens when operation two
//!    runs after operation one.
//! 2. **The test oracle.** Simulate the emitted program and diff the result against
//!    the target cutter-location surface: below target is a gouge, far above is
//!    leftover stock. This is the primary regression test for the whole system, and
//!    it finds real defects without anyone reading G-code.

pub mod stock;
