//! Faces, edges and points picked on a model, saved by what they are.

use serde::{Deserialize, Serialize};

/// A face or an edge picked on a model.
///
/// `at` is what the pick is: a point on the feature, in the model's own
/// coordinates, where the person clicked. `index` is the feature's number when it
/// was picked, which finds it at once while the model is unchanged.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct FeatureRef {
    /// The model, by name.
    pub model: String,
    /// The face's or edge's number in the model's topology.
    pub index: u32,
    /// A point on it, in model coordinates, millimetres.
    pub at: [f64; 3],
}

/// Which point of a feature a picked point is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Snap {
    /// A corner, where edges end.
    Vertex,
    /// The centre of a round edge: a hole's, say.
    CircleCenter,
    /// Halfway along an edge.
    EdgeMiddle,
}

/// A point picked on a model.
///
/// As with [`FeatureRef`], `at` is what the pick is: the point itself, in model
/// coordinates. `index` is the vertex's number for [`Snap::Vertex`], and the edge's
/// otherwise.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PointRef {
    /// The model, by name.
    pub model: String,
    /// Which point of the feature.
    pub snap: Snap,
    /// The vertex's or edge's number in the model's topology.
    pub index: u32,
    /// The point, in model coordinates, millimetres.
    pub at: [f64; 3],
}
