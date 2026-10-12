//! The camcraft CAM engine.
//!
//! Position in the dependency DAG: depends on `camcraft-geom` and `camcraft-sim`.
//! It does **not** depend on `camcraft-post`, and that direction is the point:
//! this crate cannot reference a G-code concept because it cannot see one.
//!
//! # Pipeline stages
//!
//! These stay distinct. Arc fitting, tabs, lead in/out, feed optimisation and
//! cutter compensation are every one of them a `Toolpath -> Toolpath` transform,
//! and they have nowhere to live if a strategy emits instructions directly:
//!
//! ```text
//! Strategy -> Toolpath -> transforms -> Linker -> IR -> Post
//! ```
//!
//! # Invariants
//!
//! - Strategies are **pure functions** of `(&Model, &Stock, &Params)`: they read
//!   the stock and never change it. A run threads one stock explicitly through its
//!   operations, simulating each into it in turn, and keeps a packed copy after
//!   each: the keyframes a replay starts from.
//! - **Grids are only for stock.** Height fields represent material: the
//!   simulation, rest machining, clearance. Every point a toolpath follows, and
//!   every boundary it is offset from, comes from the model's geometry — exact
//!   drop-cutter queries, or shadows of the mesh ([`strategy::reach`]). A stock grid may
//!   decide *whether* a path cuts somewhere, never *where* it runs. Nothing
//!   smooths a path to hide a grid.
//! - The effective tool profile derives from `(tool, radial_stock, axial_stock)`.
//!   The raw tool is never used directly.
//! - Output is **deterministic**: identical inputs produce identical results on any
//!   core count. Parallel map-and-collect preserves order and is fine;
//!   order-dependent floating point reductions are not.

pub mod cutter;
pub mod job;
pub mod path;
/// The material at any point in a finished job, for showing and for playback.
pub mod replay;
pub mod run;
pub mod setup;
pub mod strategy;

#[cfg(test)]
pub(crate) mod fixture;

/// Whether `v` is a usable size: finite and greater than zero, the test every
/// length, feed and tolerance the engine is given must pass.
pub(crate) fn is_positive(v: f64) -> bool {
    v.is_finite() && v > 0.0
}
