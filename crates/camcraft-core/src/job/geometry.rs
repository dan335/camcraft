//! What an operation machines: Fusion's Geometry tab, which is three settings
//! here: the picks, a 3D machining boundary and a 2D outline.

use serde::{Deserialize, Serialize};

use super::{FeatureRef, Operation, is_default};

/// What a 3D operation is kept to, seen from above: Fusion's boundary mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Boundary {
    /// The stock's footprint: the whole part, and everything around it.
    #[default]
    Stock,
    /// The rectangle around the setup's models.
    BoundingBox,
    /// The setup's models' outline: their shadow from straight above.
    Silhouette,
    /// The picked faces seen from above, and the loops the picked edges close.
    Selection,
}

/// What a 2D operation cuts round or clears, seen from above: Fusion's contour
/// or pocket selection. Unlike a [`MachiningBoundary`] it does not fence the tool
/// in; the tool runs beside it, on the side the strategy says.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outline {
    /// The setup's models' outline: their shadow from straight above.
    #[default]
    Silhouette,
    /// The picked faces seen from above, and the loops the picked edges close.
    Selection,
}

impl From<Outline> for Boundary {
    fn from(o: Outline) -> Self {
        match o {
            Outline::Silhouette => Boundary::Silhouette,
            Outline::Selection => Boundary::Selection,
        }
    }
}

/// Where the tool may go, against the machining boundary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Containment {
    /// The tool's centre stays inside the boundary, so its side reaches past it by
    /// its radius. Fusion's default.
    #[default]
    Center,
    /// The whole tool stays inside.
    Inside,
    /// The tool may go its whole width outside, so its side still reaches the
    /// boundary from beyond it.
    Outside,
}

/// Faces and edges picked on the models.
///
/// What they mean is for the rest of the operation to say, which is why they live
/// here and not with the strategy: a boundary or outline of the selection is
/// their outline from above, a height can be measured from them, a drill's are
/// the rims of its holes.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    /// Faces picked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<FeatureRef>,
    /// Edges picked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<FeatureRef>,
}

impl Selection {
    /// Whether nothing is picked.
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty() && self.edges.is_empty()
    }

    /// Every pick, faces first.
    pub fn iter(&self) -> impl Iterator<Item = &FeatureRef> {
        self.faces.iter().chain(&self.edges)
    }
}

/// Where a 3D operation's tool may go: Fusion's machining boundary, where the
/// tool may go against it, and how far past it.
///
/// The stock's footprint, the default, machines the whole part and everything
/// around it; the models' bounding box or silhouette keeps the tool near the
/// part; the selection keeps it to the picks' outline.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MachiningBoundary {
    /// What the tool is kept to.
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: Boundary,
    /// Where the tool may go against it.
    #[serde(default, skip_serializing_if = "is_default")]
    pub containment: Containment,
    /// Fusion's additional offset, in millimetres: how far the boundary moves
    /// out, or in when the whole tool is kept inside it. Past the stock's
    /// footprint, with no boundary.
    #[serde(default, skip_serializing_if = "is_default")]
    pub margin: f64,
}

impl Default for MachiningBoundary {
    fn default() -> Self {
        Self {
            kind: Boundary::Stock,
            containment: Containment::Center,
            margin: 0.0,
        }
    }
}

impl Operation {
    /// Whether what it cuts is its selection: a 2D outline or a 3D boundary of
    /// it, or a drill's holes.
    pub fn cuts_selection(&self) -> bool {
        let uses = self.strategy.uses();
        uses.holes
            || (uses.outline && self.outline == Outline::Selection)
            || (uses.boundary && self.boundary.kind == Boundary::Selection)
    }
}
