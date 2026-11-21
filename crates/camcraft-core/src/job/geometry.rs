//! What an operation machines, which is four settings: the picks, a 3D
//! machining boundary, a 2D outline, and the faces a 3D operation machines and
//! those it keeps clear of.

use serde::{Deserialize, Serialize};

use super::{FeatureRef, Operation, is_default};

/// What a 3D operation is kept to, seen from above.
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

/// What a 2D operation cuts round or clears, seen from above: the contour it
/// follows or the pocket it clears. Unlike a [`MachiningBoundary`] it does not
/// fence the tool in; the tool runs beside it, on the side the strategy says.
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
    /// its radius. The default.
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

/// Where a 3D operation's tool may go: the machining boundary, where the
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
    /// The additional offset, in millimetres: how far the boundary moves
    /// out, or in when the whole tool is kept inside it. Past the stock's
    /// footprint, with no boundary.
    #[serde(default, skip_serializing_if = "is_default")]
    pub margin: f64,
}

impl MachiningBoundary {
    /// Whether it leaves the tool no room round the part: a boundary round
    /// the part that the tool reaches in from outside, with no additional
    /// offset. Its centre stops a radius outside the part's walls, so it only
    /// touches them and clears nothing past them. A new operation's boundary
    /// is given room ([`Operation::fit_channel`]).
    pub fn needs_room(&self) -> bool {
        matches!(self.kind, Boundary::BoundingBox | Boundary::Silhouette)
            && self.containment == Containment::Outside
            && self.margin == 0.0
    }
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

/// What a 3D operation does to a face: machine it or avoid it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SurfaceAction {
    /// It is cut.
    #[default]
    Machine,
    /// It is kept clear of: the tool may rest on it, grown by its clearance,
    /// but never cuts there.
    Avoid,
}

/// The faces a 3D operation machines and those it keeps clear of: groups of
/// picked faces, each machined or avoided with its own offsets, and the model,
/// every face in no group.
///
/// Each group's faces are met by the tool grown by the operation's stock to
/// leave and the group's own offsets; the tool rests on whatever is highest,
/// and a pass is kept where it rests on a face that is machined, and cut out
/// where it rests only on one that is avoided. Left out, the whole part is
/// machined, as it always was.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Surfaces {
    /// Every face in no group.
    #[serde(default, skip_serializing_if = "is_default")]
    pub model: ModelSurfaces,
    /// The groups, each of its own faces.
    #[serde(default, rename = "group", skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<SurfaceGroup>,
}

/// What an operation does to the faces in no group of its [`Surfaces`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSurfaces {
    /// Machined, unless asked.
    #[serde(default, skip_serializing_if = "is_default")]
    pub action: SurfaceAction,
}

/// Picked faces an operation machines or avoids, with offsets of their own.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceGroup {
    /// Machined or avoided.
    pub action: SurfaceAction,
    /// Beside, in millimetres, added to the operation's stock to leave: on a
    /// face machined, more stock left on it; on one avoided, how far the tool
    /// keeps from it sideways.
    #[serde(default, skip_serializing_if = "is_default")]
    pub radial: f64,
    /// Down, as `radial`: how far above a face avoided the tool keeps.
    #[serde(default, skip_serializing_if = "is_default")]
    pub axial: f64,
    /// Its faces.
    #[serde(default)]
    pub faces: Vec<FeatureRef>,
}

impl Surfaces {
    /// Whether it is the whole part, machined: no group, and the model not
    /// avoided.
    pub fn is_whole(&self) -> bool {
        self.groups.is_empty() && self.model.action == SurfaceAction::Machine
    }

    /// Whether any of the part is avoided: the model, or a group.
    pub fn avoids_any(&self) -> bool {
        self.model.action == SurfaceAction::Avoid
            || self.groups.iter().any(|g| g.action == SurfaceAction::Avoid)
    }
}

impl Operation {
    /// Whether what it cuts is its selection: a 2D outline or a 3D boundary of
    /// it, a drill's holes, or the slots or curves it runs along.
    pub fn cuts_selection(&self) -> bool {
        let uses = self.strategy.uses();
        uses.holes.is_some()
            || uses.curves.is_some()
            || (uses.outline && self.outline == Outline::Selection)
            || (uses.boundary && self.boundary.kind == Boundary::Selection)
    }
}
