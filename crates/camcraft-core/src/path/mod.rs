//! The toolpath, and what works on it once a strategy has made it, in the
//! order a run applies them: trimming to heights, reordering, tabs, smoothing
//! and feed optimisation; then the linker that joins its passes, and the
//! adapters that hand it to the simulator.

pub mod corners;
pub mod fit;
pub mod linker;
pub mod ordering;
pub mod simadapt;
pub mod tabs;
pub mod toolpath;
pub mod trim;
