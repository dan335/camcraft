//! Geometry primitives for camcraft.
//!
//! Position in the dependency DAG: **leaf**. This crate depends on no other
//! camcraft crate, and everything else depends on it.
//!
//! # Conventions
//!
//! These are fixed once, here, and hold throughout the workspace:
//!
//! - Millimetres, Z-up, right-handed. Unit conversion happens only at IO boundaries.
//! - The controlled point is the **tool tip**, never the ball centre or the holder
//!   gauge line.
//! - `f64` for all geometry computation and path points; `f32` **only** for grid
//!   storage. f32 carries ~7 significant digits, so across a 300 mm part the storage
//!   error is ~0.00003 mm, far below any tolerance of interest, while halving the
//!   memory of a large grid. The split is deliberate.
//! - `tolerance` (chordal, default 0.01 mm) is a job parameter, never a constant.
//!   It drives adaptive sampling density, grid resolution, arc fitting and the
//!   simulator's gouge threshold. Nothing here hardcodes a step size.
//!
//! # Two geometry channels
//!
//! camcraft carries two distinct geometry channels, and this crate owns both:
//!
//! - **Height maps**, consumed by the 3D strategies.
//! - **Contours**, consumed by roughing, by boundaries picked on a model, and by the
//!   2D operations still to come.
//!
//! Collapsing these into one would leave the entire 2D half of a CAM system with
//! nowhere to live.

pub mod arc;
pub mod mesh;
pub mod synthetic;

pub mod bvh;

pub mod heightmap;

pub mod contour;

pub mod engagement;

pub mod ring;

pub mod shadow;

pub mod steps;

pub mod topology;
