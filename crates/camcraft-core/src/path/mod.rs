//! The toolpath, and what works on it once a strategy has made it: reordering,
//! tabs, the linker that joins its passes, and the adapters that hand it to the
//! simulator.

pub mod linker;
pub mod ordering;
pub mod simadapt;
pub mod tabs;
pub mod toolpath;
