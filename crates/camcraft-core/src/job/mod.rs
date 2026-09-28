//! The job model: models, stocks and setups, each setup an ordered list of
//! operations sharing one stock.
//!
//! This is the contract between a person and the engine, and it is shaped the way
//! Fusion shapes a CAM document. A job keeps three groups:
//!
//! - **Models**: the design files it machines, referenced by path relative to the
//!   job file, never copied in.
//! - **Stocks**: blocks of material, in model coordinates. A block is a physical
//!   thing, so a top and a bottom setup share one and each sees it from its side.
//! - **Setups**: how the part sits on the machine and where its work zero is, which
//!   models and stocks it includes (by name), its post and work offset, and its
//!   operations.
//!
//! An operation is shaped the way Fusion shapes one, **Geometry, Heights, Passes,
//! Linking**, because that factoring is proven and maps one-to-one onto the
//! provisions this project was built around. Adopting it means the parameter layer
//! never needs restructuring as strategies are added. Fusion's Geometry tab is
//! three settings here, since it means different things to different strategies:
//! the picks (`selection`), a 3D machining `boundary` and a 2D `outline`.
//!
//! # Heights are symbolic
//!
//! `clearance = { ref = "stock-top", offset = 10 }` rather than a bare number. A job
//! written against one block of material then works against a taller one, and the
//! meaning survives being read six months later. Absolute numbers are still
//! available, but they have to be asked for.
//!
//! # One stock per setup, threaded
//!
//! A setup's operations run in order against a single block. Each reads the
//! material as it currently stands and writes back what it removed, so a finishing
//! pass never re-cuts what roughing already cleared. Rest machining is not a
//! feature here; it is the consequence of that ordering.
//!
//! Each setup starts from its own stock. The material is a height field seen from
//! above, so what a top setup leaves cannot be turned over for the bottom one to
//! start from. Carrying stock between setups needs a volumetric model.
//!
//! # Picked features
//!
//! A work zero can sit on a point picked on a model, and an operation can be kept
//! to faces or edges picked on one ([`FeatureRef`], [`PointRef`]). A pick is saved
//! as the model's name, the feature's number, and a point on it in model
//! coordinates. The point is what the pick *is*: the number is only a quick way to
//! find it while the model is unchanged. A model tessellated again, or re-exported
//! with its faces in another order, still has a face through that point, so the
//! pick survives; a model edited so the point is on nothing any more says so, and
//! nothing is cut until the pick is made again. See `crate::features`.
//!
//! # Format
//!
//! There is one job file format, [`JOB_VERSION`]. A file of any other version is
//! refused by name rather than read as something it may not mean.

mod error;
mod model;
mod operation;
mod pick;
mod setup;
mod tool;
mod validate;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

pub use error::JobError;
pub use model::{FixedBox, LengthUnit, ModelDef, Place, StockDef, StockSpec, Transform};
pub use operation::{
    Boundary, Containment, Depths, Entry, FaceDirection, Height, HeightContext, HeightRef, Heights,
    LeadSizes, Leads, Leave, Linking, MachiningBoundary, Operation, Outline, Passes, Picks,
    ProfileSide, Retraction, Selection, StockToLeave, StrategySpec, TabShape, Tabs, Uses,
};
pub use pick::{FeatureRef, PointRef, Snap};
pub use setup::{Along, BoxOf, Origin, Setup};
pub use tool::{
    Coolant, DRILL_POINT, Holder, HolderSegment, Preset, SpindleDirection, ToolFieldError,
    ToolKind, ToolMaterial, ToolSpec, free_preset_name,
};

/// The job file format this build reads and writes, and the only one it reads.
pub const JOB_VERSION: u32 = 1;

/// A complete job.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    /// The file format; [`JOB_VERSION`].
    pub version: u32,
    /// Chordal tolerance in millimetres. Drives sampling and grid resolution alike,
    /// and how finely STEP models are tessellated.
    #[serde(default = "default_tolerance")]
    pub tolerance: f64,
    /// The job's own cutters, by name: copies, never links to a library, so the job
    /// still cuts what was verified after the library changes.
    #[serde(default)]
    pub tools: BTreeMap<String, ToolSpec>,
    /// The design files machined.
    #[serde(default, rename = "model")]
    pub models: Vec<ModelDef>,
    /// Blocks of material.
    #[serde(default, rename = "stock")]
    pub stocks: Vec<StockDef>,
    /// Setups, in the order they are machined.
    #[serde(default, rename = "setup")]
    pub setups: Vec<Setup>,
}

impl Default for Job {
    /// An empty job at the current format.
    fn default() -> Self {
        Self {
            version: JOB_VERSION,
            tolerance: NEW_JOB_TOLERANCE,
            tools: BTreeMap::new(),
            models: Vec::new(),
            stocks: Vec::new(),
            setups: Vec::new(),
        }
    }
}

/// The tolerance a job made in the app starts with: fine enough for router work,
/// and quick to run. It is always written to the file.
pub const NEW_JOB_TOLERANCE: f64 = 0.05;

/// The tolerance of a job file that leaves it out: finer than a new job's, since
/// a file written by hand said nothing, and finer is the safe side to err on.
fn default_tolerance() -> f64 {
    0.01
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

impl Job {
    /// Every pick on model `from` names model `to` instead, for when a model is
    /// renamed.
    pub fn rename_model_refs(&mut self, from: &str, to: &str) {
        for setup in &mut self.setups {
            if let Origin::Point { point, .. } = &mut setup.origin
                && point.model == from
            {
                point.model = to.to_string();
            }
            for op in &mut setup.operations {
                let s = &mut op.selection;
                for r in s.faces.iter_mut().chain(&mut s.edges) {
                    if r.model == from {
                        r.model = to.to_string();
                    }
                }
            }
        }
    }

    /// The tool an operation names, if it names one the job has.
    pub fn tool_for(&self, op: &Operation) -> Option<&ToolSpec> {
        op.tool.as_deref().and_then(|t| self.tools.get(t))
    }

    /// The model called `name`.
    pub fn model(&self, name: &str) -> Option<&ModelDef> {
        self.models.iter().find(|m| m.name == name)
    }

    /// The stock called `name`.
    pub fn stock(&self, name: &str) -> Option<&StockDef> {
        self.stocks.iter().find(|s| s.name == name)
    }

    /// Every model setup `index` needs loaded: its own, and those its stocks are
    /// built around. Each once, in the job's order.
    pub fn models_for(&self, index: usize) -> Vec<&ModelDef> {
        let Some(setup) = self.setups.get(index) else {
            return Vec::new();
        };
        let mut want: BTreeSet<&str> = setup.models.iter().map(String::as_str).collect();
        for s in setup.stocks.iter().filter_map(|s| self.stock(s)) {
            let (StockSpec::AroundModels { models, .. }
            | StockSpec::FixedBox(FixedBox { models, .. })) = &s.spec;
            want.extend(models.iter().map(String::as_str));
        }
        self.models
            .iter()
            .filter(|m| want.contains(m.name.as_str()))
            .collect()
    }
}
