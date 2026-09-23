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
//! - Strategies are **pure functions** of `(&Model, &Stock, &Params)`. Stock is
//!   threaded explicitly rather than mutated in place, which is what later makes
//!   associativity, caching and undo possible.
//! - A cutter-location map is a function of `(effective tool profile, grid
//!   resolution, region)`. There is no global `Zcl`; maps are cached and keyed.
//! - The effective tool profile derives from `(tool, radial_stock, axial_stock)`.
//!   The raw tool is never used directly.
//! - Output is **deterministic**: identical inputs produce identical results on any
//!   core count. Parallel map-and-collect preserves order and is fine;
//!   order-dependent floating point reductions are not.

pub mod tool;

pub mod dropcutter;

pub mod clmap;

pub mod stock;

pub mod parallel;
pub mod toolpath;

pub mod linker;

pub mod simadapt;

pub mod roughing;

pub mod ordering;

pub mod job;

/// The material at any point in a finished job, for showing and for playback.
pub mod replay;
pub mod run;
