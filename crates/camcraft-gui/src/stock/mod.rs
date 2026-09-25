//! The simulated material as the app shows it: its image of heights and colours,
//! its shading, its height pyramid, its GPU tiles, and the thread that cuts it
//! during playback.

pub mod dirty;
pub mod gpu_tiles;
pub mod live;
pub mod pyramid;
pub mod stock_ao;
pub mod stock_view;
