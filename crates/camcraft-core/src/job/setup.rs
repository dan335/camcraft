//! A setup: how the part sits on the machine, where its work zero is, and what
//! it machines.

use serde::{Deserialize, Serialize};

use super::{Operation, PointRef, is_default};
use crate::setup::frame::Orientation;

/// How a part is held and machined from one side.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    /// Name, unique in the job. Programs are named after it.
    pub name: String,
    /// The job's models this setup machines, by name.
    #[serde(default)]
    pub models: Vec<String>,
    /// The job's stocks this setup starts from, by name.
    #[serde(default)]
    pub stocks: Vec<String>,
    /// Work offset, 54 for `G54`.
    #[serde(default = "default_wcs")]
    pub wcs: u8,
    /// The post processor: a built-in name, or a profile file relative to the job.
    #[serde(default = "default_post")]
    pub post: String,
    /// Which model directions become the machine's Z and X.
    #[serde(default, skip_serializing_if = "is_default")]
    pub orientation: Orientation,
    /// Where the work zero is.
    #[serde(default, skip_serializing_if = "is_default")]
    pub origin: Origin,
    /// Operations, in the order they run.
    #[serde(default, rename = "operation")]
    pub operations: Vec<Operation>,
}

impl Setup {
    /// An empty setup with the usual defaults.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            models: Vec::new(),
            stocks: Vec::new(),
            wcs: default_wcs(),
            post: default_post(),
            orientation: Orientation::default(),
            origin: Origin::default(),
            operations: Vec::new(),
        }
    }
}

fn default_wcs() -> u8 {
    54
}

fn default_post() -> String {
    "masso".into()
}

/// Where a setup's work zero is.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Origin {
    /// The design's own zero, turned with the part, moved by `offset` along the
    /// setup's axes.
    ModelOrigin {
        /// Millimetres along the setup's X, Y and Z.
        #[serde(default, skip_serializing_if = "is_default")]
        offset: [f64; 3],
    },
    /// A point on the box around the setup's stock or models, as the part sits on
    /// the machine, moved by `offset`.
    BoxPoint {
        /// Which box.
        of: BoxOf,
        /// Where along X.
        x: Along,
        /// Where along Y.
        y: Along,
        /// Where along Z.
        z: Along,
        /// Millimetres along the setup's X, Y and Z.
        #[serde(default, skip_serializing_if = "is_default")]
        offset: [f64; 3],
    },
    /// A point picked on a model: a corner, the centre of a round edge, or the
    /// middle of an edge, moved by `offset`.
    Point {
        /// The point.
        point: PointRef,
        /// Millimetres along the setup's X, Y and Z.
        #[serde(default, skip_serializing_if = "is_default")]
        offset: [f64; 3],
    },
}

impl Origin {
    /// Millimetres the work zero is moved by, along the setup's axes.
    pub fn offset(&self) -> [f64; 3] {
        match self {
            Origin::ModelOrigin { offset }
            | Origin::BoxPoint { offset, .. }
            | Origin::Point { offset, .. } => *offset,
        }
    }

    /// The same work zero, moved by `to` instead.
    pub fn with_offset(mut self, to: [f64; 3]) -> Self {
        match &mut self {
            Origin::ModelOrigin { offset }
            | Origin::BoxPoint { offset, .. }
            | Origin::Point { offset, .. } => *offset = to,
        }
        self
    }

    /// The stock's front-left corner on its top, so X and Y run into the part
    /// and Z is zero where the tool is touched off: the corner Autodesk's own
    /// Fusion tutorials and setup sample pick (`TOP_XMIN_YMIN`).
    pub fn stock_corner() -> Self {
        Origin::BoxPoint {
            of: BoxOf::Stock,
            x: Along::Min,
            y: Along::Min,
            z: Along::Max,
            offset: [0.0; 3],
        }
    }
}

impl Default for Origin {
    /// The design's own zero, as models are drawn.
    fn default() -> Self {
        Origin::ModelOrigin { offset: [0.0; 3] }
    }
}

/// Which box an origin is placed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BoxOf {
    /// The setup's stock.
    Stock,
    /// The setup's models.
    Model,
}

/// Where along one side of a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Along {
    /// The low end: left, front or bottom.
    Min,
    /// The middle.
    Center,
    /// The high end: right, back or top.
    Max,
}

impl Along {
    /// The point along `lo..hi`.
    pub fn pick(self, lo: f64, hi: f64) -> f64 {
        match self {
            Along::Min => lo,
            Along::Center => (lo + hi) * 0.5,
            Along::Max => hi,
        }
    }
}
