//! An operation, shaped the way Fusion shapes one: its strategy, then what it
//! machines (Fusion's Geometry tab: a selection, a 3D boundary or a 2D outline),
//! Heights, Passes and Linking.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::validate::positive;
use super::{FeatureRef, JobError, Preset, ToolKind, ToolSpec, is_default};

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

/// One operation.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Operation {
    /// Name, for comments and diagnostics.
    pub name: String,
    /// Which strategy, and its own parameters.
    #[serde(flatten)]
    pub strategy: StrategySpec,
    /// Which cutter, by name from the job's tools. A new operation has none yet,
    /// and cannot run until it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// The tool's preset its values were copied from, by name. Only a record: the
    /// operation cuts with its own values, whatever the preset now says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// Faces and edges picked on the models, for whatever of it reads them: a
    /// boundary or outline of the selection, a height measured from it, the holes
    /// a drill drills.
    #[serde(default, skip_serializing_if = "is_default")]
    pub selection: Selection,
    /// Where a 3D operation's tool may go ([`Uses::boundary`]).
    #[serde(default, skip_serializing_if = "is_default")]
    pub boundary: MachiningBoundary,
    /// What a 2D operation cuts round or clears ([`Uses::outline`]).
    #[serde(default, skip_serializing_if = "is_default")]
    pub outline: Outline,
    /// Where the tool travels, and the levels it cuts between.
    #[serde(default, skip_serializing_if = "is_default")]
    pub heights: Heights,
    /// How the passes are laid out.
    #[serde(default, skip_serializing_if = "is_default")]
    pub passes: Passes,
    /// How the tool moves between cuts.
    #[serde(default, skip_serializing_if = "is_default")]
    pub linking: Linking,
    /// Whether it runs. One turned off stays in the job, but is not worked out,
    /// cut, checked or posted.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub enabled: bool,
}

/// Strategy and the parameters only it has.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(tag = "strategy", rename_all = "kebab-case")]
pub enum StrategySpec {
    /// Raster finishing.
    Parallel {
        /// Pass direction in degrees; zero runs along +X.
        #[serde(default)]
        angle: f64,
    },
    /// Z-level clearing, a slice each stepdown ([`Depths::Required`]).
    ZlevelRough {
        /// Rings shorter than this are not worth the entry.
        #[serde(default = "default_min_ring")]
        min_ring_length: f64,
    },
    /// A 2D contour: the tool run round the boundary, beside it or on it, level
    /// by level down to its floor. Fusion's 2D Contour.
    Profile {
        /// Which side of the boundary the tool cuts on.
        #[serde(default, skip_serializing_if = "is_default")]
        side: ProfileSide,
        /// Cut conventional rather than climb.
        #[serde(default, skip_serializing_if = "is_default")]
        conventional: bool,
    },
    /// A 2D pocket: the boundary cleared, from its middle out to its walls, level
    /// by level down to its floor. Fusion's 2D Pocket.
    Pocket {
        /// Cut conventional rather than climb.
        #[serde(default, skip_serializing_if = "is_default")]
        conventional: bool,
    },
    /// The stock's top cut flat, straight passes side by side across its
    /// footprint, down to the bottom: the model's top unless another is asked
    /// for. Fusion's Face.
    Face {
        /// Pass direction in degrees; zero runs along +X.
        #[serde(default, skip_serializing_if = "is_default")]
        angle: f64,
        /// Which way the passes cut.
        #[serde(default, skip_serializing_if = "is_default")]
        direction: FaceDirection,
        /// How far each pass runs on once the tool is clear of the stock, in
        /// millimetres.
        #[serde(default, skip_serializing_if = "is_default")]
        pass_extension: f64,
        /// How far past the stock's footprint is faced, in millimetres: Fusion's
        /// stock offset.
        #[serde(default, skip_serializing_if = "is_default")]
        stock_offset: f64,
    },
    /// Holes, picked by their rims, each drilled straight down.
    Drill {
        /// Depth of each peck, backing out between; all at once when not given.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        peck: Option<f64>,
        /// Seconds to pause at the bottom, for a canned cycle that can.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dwell: Option<f64>,
    },
}

impl StrategySpec {
    /// The strategy's name as a job file writes it, which is also how a program's
    /// section comment names it.
    pub fn name(&self) -> &'static str {
        match self {
            StrategySpec::Parallel { .. } => "parallel",
            StrategySpec::ZlevelRough { .. } => "zlevel-rough",
            StrategySpec::Profile { .. } => "profile",
            StrategySpec::Pocket { .. } => "pocket",
            StrategySpec::Face { .. } => "face",
            StrategySpec::Drill { .. } => "drill",
        }
    }

    /// Whether it is a 2D strategy: flat passes round a boundary, down to a floor.
    pub fn is_2d(&self) -> bool {
        matches!(
            self,
            StrategySpec::Profile { .. } | StrategySpec::Pocket { .. }
        )
    }

    /// Which of the settings every operation has this strategy uses.
    pub fn uses(&self) -> Uses {
        // Tabs go only where every ring is beside what is kept: a profile's, as
        // Fusion's go only on a 2D contour. Put on a pocket's or a roughing
        // level's clearing rings, they would leave pillars standing in the
        // waste.
        let cuts_loops = Uses {
            boundary: true,
            outline: false,
            stepover: true,
            stock_to_leave: Leave::Both,
            depths: Depths::Optional,
            tabs: false,
            stay_down: true,
            lead_radius: false,
            ramp: true,
            entry: false,
            leads: false,
            reorder: true,
        };
        match *self {
            // Down the surface in one pass, as Fusion's 3D finishing.
            StrategySpec::Parallel { .. } => Uses {
                depths: Depths::None,
                lead_radius: true,
                ramp: false,
                ..cuts_loops
            },
            // Slice by slice is what it is.
            StrategySpec::ZlevelRough { .. } => Uses {
                depths: Depths::Required,
                ..cuts_loops
            },
            // A 2D operation's order is its own: what is inside before what frees
            // it, a pocket's middle before its walls.
            StrategySpec::Pocket { .. } => Uses {
                boundary: false,
                outline: true,
                reorder: false,
                ..cuts_loops
            },
            StrategySpec::Profile { side, .. } => Uses {
                boundary: false,
                outline: true,
                stepover: false,
                // On the line, it has no side to leave anything beside.
                stock_to_leave: if side == ProfileSide::On {
                    Leave::Axial
                } else {
                    Leave::Both
                },
                tabs: true,
                leads: true,
                entry: true,
                reorder: false,
                ..cuts_loops
            },
            // Straight passes across the stock, in its own order, with only the
            // floor to leave anything on.
            StrategySpec::Face { .. } => Uses {
                boundary: false,
                stock_to_leave: Leave::Axial,
                reorder: false,
                ..cuts_loops
            },
            // Its picks are the holes: no area to keep to or cut round.
            StrategySpec::Drill { .. } => Uses {
                boundary: false,
                outline: false,
                stepover: false,
                stock_to_leave: Leave::None,
                // Its pecks are its own.
                depths: Depths::None,
                tabs: false,
                stay_down: false,
                lead_radius: false,
                ramp: false,
                entry: false,
                leads: false,
                // Holes are drilled in the order found, nearest first.
                reorder: false,
            },
        }
    }

    /// The stock to leave Fusion starts this strategy with: roughing leaves half
    /// a millimetre each way for the finish, and the rest cut to size.
    pub fn default_stock_to_leave(&self) -> StockToLeave {
        match self {
            StrategySpec::ZlevelRough { .. } => StockToLeave {
                radial: 0.5,
                axial: 0.5,
            },
            _ => StockToLeave::default(),
        }
    }

    /// The multiple depths this strategy starts with: roughing and 2D cutting
    /// 2 mm a level, facing its bottom in one pass.
    pub fn default_stepdown(&self) -> Option<f64> {
        match self.uses().depths {
            Depths::None => None,
            _ if matches!(self, StrategySpec::Face { .. }) => None,
            _ => Some(2.0),
        }
    }
}

/// Which of the settings every operation has (its boundary or outline,
/// [`Passes`], [`Linking`]) its strategy uses, as [`StrategySpec::uses`] says.
///
/// Every operation has them all, so switching strategy keeps what was set; one
/// its strategy does not use is not shown, not checked and not read. Anything
/// that asks whether a strategy has a setting asks this, so the panel, the
/// checks and the run always agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Uses {
    /// A machining boundary that keeps the tool in: a 3D strategy's.
    pub boundary: bool,
    /// An outline it cuts round or clears: a 2D strategy's.
    pub outline: bool,
    /// The distance between passes.
    pub stepover: bool,
    /// Which stock to leave it cuts with.
    pub stock_to_leave: Leave,
    /// Whether it cuts in levels a stepdown apart.
    pub depths: Depths,
    /// Tabs across the deepest cut of the loops it cuts.
    pub tabs: bool,
    /// Links made without lifting.
    pub stay_down: bool,
    /// Vertical arcs into and out of each pass.
    pub lead_radius: bool,
    /// Ramping into material.
    pub ramp: bool,
    /// A choice of how each pass is entered: a helix, a plunge or a ramp.
    pub entry: bool,
    /// Level arcs into and out of each ring.
    pub leads: bool,
    /// Passes reordered, and maybe reversed, to shorten travel.
    pub reorder: bool,
}

/// Which of the stock to leave a strategy cuts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leave {
    /// None: it cuts to size.
    None,
    /// Only down, at the floor.
    Axial,
    /// Both beside, at the walls, and down, at the floor.
    Both,
}

/// Whether a strategy cuts in levels a stepdown apart: Fusion's multiple depths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depths {
    /// Never: it has no levels, or its own way down.
    None,
    /// When asked: otherwise straight to the bottom in one.
    Optional,
    /// Always: it has no way down without.
    Required,
}

/// Which way facing passes cut.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FaceDirection {
    /// Back and forth, one pass each way. Fusion's default.
    #[default]
    BothWays,
    /// Every pass climbing.
    Climb,
    /// Every pass conventional.
    Conventional,
}

/// Which side of its boundary a profile cuts on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileSide {
    /// Outside it: the tool's side runs along the boundary from beyond, and what
    /// the boundary encloses is kept. Cutting a part out: its holes are cut from
    /// inside them.
    #[default]
    Outside,
    /// Inside it: what the boundary encloses is cut away along its edge. Cutting
    /// a hole out: anything left standing in it is cut round from outside.
    Inside,
    /// The tool's centre on it: a slot or a groove along it.
    On,
}

fn default_min_ring() -> f64 {
    3.0
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

/// Where the tool travels, and the levels it cuts between: Fusion's Heights tab.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Heights {
    /// The height the tool arrives at the operation and leaves it at.
    #[serde(default = "default_clearance")]
    pub clearance: Height,
    /// The height it goes up to between cuts with a full retraction, and never
    /// above with a minimum one: Fusion's retract height.
    #[serde(
        default = "default_retract",
        skip_serializing_if = "is_default_retract"
    )]
    pub retract: Height,
    /// Highest level to cut at; the stock's top when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<Height>,
    /// Lowest level to cut at; the strategy's own when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<Height>,
}

impl Default for Heights {
    fn default() -> Self {
        Self {
            clearance: default_clearance(),
            retract: default_retract(),
            top: None,
            bottom: None,
        }
    }
}

impl Heights {
    /// Whether its top or bottom is measured from the selection.
    pub fn measured_from_selection(&self) -> bool {
        [self.top, self.bottom]
            .iter()
            .flatten()
            .any(Height::from_selection)
    }
}

fn default_clearance() -> Height {
    Height {
        reference: HeightRef::StockTop,
        offset: 10.0,
    }
}

/// Fusion's: 5 mm above the stock.
fn default_retract() -> Height {
    Height {
        reference: HeightRef::StockTop,
        offset: 5.0,
    }
}

// Serde's `skip_serializing_if` hands the field by reference.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_retract(h: &Height) -> bool {
    *h == default_retract()
}

/// A height, stated relative to something that can be measured.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Height {
    /// What the offset is measured from.
    #[serde(rename = "ref", default = "default_ref")]
    pub reference: HeightRef,
    /// Millimetres above the reference.
    #[serde(default)]
    pub offset: f64,
}

fn default_ref() -> HeightRef {
    HeightRef::StockTop
}

/// What a height is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HeightRef {
    /// Top of the block.
    StockTop,
    /// Bottom of the block.
    StockBottom,
    /// Highest point of the model.
    ModelTop,
    /// Lowest point of the model.
    ModelBottom,
    /// The offset is the height, in setup coordinates.
    Absolute,
    /// The operation's picked faces and edges: their highest point for a top,
    /// their lowest for a bottom. A 2D operation measures each of its areas from
    /// the picks in it, as Fusion's "selected contours". Only a top or a bottom
    /// can be, and only with something picked.
    Selection,
}

/// What a height resolves against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeightContext {
    /// Top of the block.
    pub stock_top: f64,
    /// Bottom of the block.
    pub stock_bottom: f64,
    /// Highest point of the model.
    pub model_top: f64,
    /// Lowest point of the model.
    pub model_bottom: f64,
    /// How high the operation's picks go, when it has any and a height is
    /// measured from them.
    pub picked: Option<Picks>,
}

/// The lowest and highest of some picked points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Picks {
    /// The lowest.
    pub lowest: f64,
    /// The highest.
    pub highest: f64,
}

impl Picks {
    /// How high `zs` go; `None` for none.
    pub fn of(zs: impl IntoIterator<Item = f64>) -> Option<Picks> {
        zs.into_iter().fold(None, |p, z| {
            Some(p.map_or(
                Picks {
                    lowest: z,
                    highest: z,
                },
                |p: Picks| Picks {
                    lowest: p.lowest.min(z),
                    highest: p.highest.max(z),
                },
            ))
        })
    }
}

impl Height {
    /// Resolve to a height in setup coordinates, as a clearance. A clearance is
    /// never measured from the picks ([`Operation::check`] turns that away); one
    /// that was would read them as a top.
    pub fn resolve(&self, ctx: &HeightContext) -> f64 {
        self.resolve_top(ctx)
    }

    /// Resolve as a top: the picks read by their highest point, or the model's
    /// top with none.
    pub fn resolve_top(&self, ctx: &HeightContext) -> f64 {
        self.measured_from(ctx, ctx.picked.map_or(ctx.model_top, |p| p.highest))
    }

    /// Resolve as a bottom: the picks read by their lowest point, or the model's
    /// bottom with none.
    pub fn resolve_bottom(&self, ctx: &HeightContext) -> f64 {
        self.measured_from(ctx, ctx.picked.map_or(ctx.model_bottom, |p| p.lowest))
    }

    fn measured_from(&self, ctx: &HeightContext, picked: f64) -> f64 {
        let base = match self.reference {
            HeightRef::StockTop => ctx.stock_top,
            HeightRef::StockBottom => ctx.stock_bottom,
            HeightRef::ModelTop => ctx.model_top,
            HeightRef::ModelBottom => ctx.model_bottom,
            HeightRef::Absolute => 0.0,
            HeightRef::Selection => picked,
        };
        base + self.offset
    }

    /// Whether it is measured from the picks.
    pub fn from_selection(&self) -> bool {
        self.reference == HeightRef::Selection
    }
}

impl Operation {
    /// Whether what it cuts is its selection: a 2D outline or a 3D boundary of
    /// it, or a drill's holes.
    pub fn cuts_selection(&self) -> bool {
        let uses = self.strategy.uses();
        matches!(self.strategy, StrategySpec::Drill { .. })
            || (uses.outline && self.outline == Outline::Selection)
            || (uses.boundary && self.boundary.kind == Boundary::Selection)
    }

    /// A 2D operation's top and bottom over one piece of its outline with
    /// `picks` in it, if any. A height measured from the selection reads the
    /// piece's picks, or all of them, `ctx.picked`, where it has none. Without a
    /// top, the stock's; without a bottom, the piece's lowest pick when the picks
    /// are the outline, or else the models' bottom.
    pub fn levels_2d(&self, ctx: &HeightContext, picks: Option<Picks>) -> (f64, f64) {
        let own = HeightContext {
            picked: picks.or(ctx.picked),
            ..*ctx
        };
        let h = &self.heights;
        let top = h.top.map_or(ctx.stock_top, |h| h.resolve_top(&own));
        let bottom = match h.bottom {
            Some(h) => h.resolve_bottom(&own),
            None if self.outline == Outline::Selection => {
                picks.map_or(ctx.model_bottom, |p| p.lowest)
            }
            None => ctx.model_bottom,
        };
        (top, bottom)
    }

    /// The deepest each of its levels may be, when it cuts in levels: its own,
    /// unless its strategy has none. `None` is straight to the bottom in one.
    pub fn stepdown(&self) -> Option<f64> {
        self.passes
            .stepdown
            .filter(|_| self.strategy.uses().depths != Depths::None)
    }

    /// The stock to leave it cuts with: its own, less what its strategy does not
    /// use. A drill cuts to size; a profile on the line leaves only its floor.
    pub fn stock_to_leave(&self) -> StockToLeave {
        let s = self.passes.stock_to_leave;
        match self.strategy.uses().stock_to_leave {
            Leave::None => StockToLeave::default(),
            Leave::Axial => StockToLeave { radial: 0.0, ..s },
            Leave::Both => s,
        }
    }

    /// How far below zero its stock to leave can go with `tool`: beside, more
    /// than minus the tool's radius, which it would shrink to nothing; down, no
    /// lower than minus a round end's corner radius in 3D, where the end has to
    /// stay round, and in 2D or with a flat end as low as it likes.
    pub fn least_stock_to_leave(&self, tool: &ToolSpec) -> StockToLeave {
        let corner = match tool.kind {
            ToolKind::Ball => tool.diameter / 2.0,
            ToolKind::Bullnose => tool.corner,
            ToolKind::Flat | ToolKind::Cone | ToolKind::Drill => 0.0,
        };
        StockToLeave {
            radial: -tool.diameter / 2.0,
            axial: if !self.strategy.is_2d() && corner > 0.0 {
                -corner
            } else {
                f64::NEG_INFINITY
            },
        }
    }

    /// Why this operation cannot cut with `tool`, if it cannot.
    ///
    /// A cone slices badly into Z levels (its reach widens all the way up, so the
    /// levels need far too many offsets) and cannot leave stock (grown by a ball it
    /// is not a cone); it finishes, engraves and chamfers.
    ///
    /// Negative stock shrinks the tool, as Fusion's does, so it can take no more
    /// than the tool's radius beside; and in 3D, where a round end has to stay
    /// round, no more than its corner radius down.
    pub fn cannot_use(&self, tool: &ToolSpec) -> Option<&'static str> {
        let drilling = matches!(self.strategy, StrategySpec::Drill { .. });
        if tool.kind == ToolKind::Drill && !drilling {
            return Some("a drill only drills; use a drilling operation");
        }
        if drilling && !matches!(tool.kind, ToolKind::Drill | ToolKind::Flat) {
            return Some("drill holes with a drill, or a flat end mill that plunges");
        }
        if matches!(self.strategy, StrategySpec::Face { .. }) && tool.kind == ToolKind::Ball {
            return Some("a ball end mill cannot face flat; use a flat or bull-nose end mill");
        }
        if self.strategy.uses().stepover && self.passes.stepover > tool.diameter {
            return Some(
                "the stepover is wider than the tool: it would leave ridges between passes",
            );
        }
        let leave = self.stock_to_leave();
        let leaves = leave.radial != 0.0 || leave.axial != 0.0;
        let least = self.least_stock_to_leave(tool);
        if leave.radial <= least.radial {
            return Some("negative stock to leave beside must be less than the tool's radius");
        }
        if leave.axial < least.axial {
            return Some(
                "negative stock to leave down must be no more than the tool's corner radius",
            );
        }
        if tool.kind != ToolKind::Cone {
            return None;
        }
        match self.strategy {
            StrategySpec::ZlevelRough { .. } => {
                return Some(
                    "a V-bit or chamfer mill cannot rough in Z levels; use a parallel pass",
                );
            }
            StrategySpec::Pocket { .. } => {
                return Some("a V-bit or chamfer mill cannot clear a pocket's floor flat");
            }
            StrategySpec::Face { .. } => {
                return Some("a V-bit or chamfer mill cannot face flat");
            }
            StrategySpec::Profile { side, .. } if side != ProfileSide::On => {
                return Some(
                    "a V-bit or chamfer mill profiles only on the line: its width changes \
                     with depth",
                );
            }
            _ => {}
        }
        if leaves {
            return Some("a V-bit or chamfer mill cuts to size: set stock to leave to zero");
        }
        None
    }

    /// Why a height of this operation cannot be measured from its picks, if one
    /// is and cannot: a clearance never is, a drill's holes each go to their own
    /// bottom, and with nothing picked there is nothing to measure from.
    pub fn cannot_measure_from_picks(&self) -> Option<&'static str> {
        if self.heights.clearance.from_selection() {
            return Some("the clearance cannot be measured from the selection");
        }
        if self.heights.retract.from_selection() {
            return Some("the retract height cannot be measured from the selection");
        }
        if !self.heights.measured_from_selection() {
            return None;
        }
        if matches!(self.strategy, StrategySpec::Drill { .. }) {
            return Some(
                "a drill's bottom cannot be measured from the selection: each hole goes \
                 to its own",
            );
        }
        if self.selection.is_empty() {
            return Some("a height is measured from the selection, but nothing is picked");
        }
        None
    }

    /// Check this operation of setup `setup` can run with the job's `tools`: it
    /// names one the job has, the tool can do what it asks, and its values are
    /// usable. Its picks are checked with the setup's.
    pub(super) fn check(
        &self,
        setup: &str,
        tools: &BTreeMap<String, ToolSpec>,
    ) -> Result<(), JobError> {
        let at = format!("{setup}/{}", self.name);
        if let Some(why) = self.cannot_measure_from_picks() {
            return Err(JobError::BadHeight { op: at, why });
        }
        let Some(tool) = &self.tool else {
            return Err(JobError::NoTool { op: at });
        };
        let Some(spec) = tools.get(tool) else {
            return Err(JobError::UnknownTool {
                op: at,
                tool: tool.clone(),
            });
        };
        if let Some(why) = self.cannot_use(spec) {
            return Err(JobError::WrongTool {
                op: at,
                tool: tool.clone(),
                why: why.into(),
            });
        }
        positive(&format!("{at}.passes.stepover"), self.passes.stepover)?;
        positive(&format!("{at}.passes.feed"), self.passes.feed)?;
        if self.passes.rpm == Some(0) {
            return Err(JobError::BadValue {
                field: format!("{at}.passes.rpm"),
                value: 0.0,
            });
        }
        positive(
            &format!("{at}.passes.max_cut_step"),
            self.passes.max_cut_step,
        )?;
        positive(
            &format!("{at}.linking.retract_feed"),
            self.linking.retract_feed,
        )?;
        positive(
            &format!("{at}.linking.plunge_feed"),
            self.linking.plunge_feed,
        )?;
        self.passes
            .stock_to_leave
            .check(&format!("{at}.passes.stock_to_leave"))?;
        let uses = self.strategy.uses();
        if let Some(l) = self.linking.leads.filter(|_| uses.leads) {
            if let Some(r) = l.radius {
                positive(&format!("{at}.linking.leads.radius"), r)?;
            }
            if !(l.sweep > 0.0 && l.sweep <= 180.0) {
                return Err(JobError::BadValue {
                    field: format!("{at}.linking.leads.sweep"),
                    value: l.sweep,
                });
            }
            for (name, v) in [("straight", l.straight), ("vertical", l.vertical)] {
                if let Some(v) = v
                    && !(v.is_finite() && v >= 0.0)
                {
                    return Err(JobError::BadValue {
                        field: format!("{at}.linking.leads.{name}"),
                        value: v,
                    });
                }
            }
        }
        if uses.entry
            && let Some(d) = self.linking.helix_diameter
        {
            positive(&format!("{at}.linking.helix_diameter"), d)?;
        }
        if uses.ramp
            && let Some(d) = self.linking.ramp_stepdown
        {
            positive(&format!("{at}.linking.ramp_stepdown"), d)?;
        }
        if let Some(t) = self.passes.tabs.filter(|_| uses.tabs) {
            positive(&format!("{at}.passes.tabs.width"), t.width)?;
            positive(&format!("{at}.passes.tabs.height"), t.height)?;
            positive(&format!("{at}.passes.tabs.spacing"), t.spacing)?;
        }
        if let Some(stepdown) = self.stepdown() {
            positive(&format!("{at}.passes.stepdown"), stepdown)?;
        } else if uses.depths == Depths::Required {
            return Err(JobError::NoStepdown { op: at });
        }
        if let StrategySpec::Face {
            angle,
            pass_extension,
            stock_offset,
            ..
        } = self.strategy
        {
            for (name, value, least) in [
                ("angle", angle, f64::NEG_INFINITY),
                ("pass_extension", pass_extension, 0.0),
                ("stock_offset", stock_offset, f64::NEG_INFINITY),
            ] {
                if !(value.is_finite() && value >= least) {
                    return Err(JobError::BadValue {
                        field: format!("{at}.{name}"),
                        value,
                    });
                }
            }
        }
        if let StrategySpec::Drill { peck, dwell } = self.strategy {
            if let Some(p) = peck {
                positive(&format!("{at}.peck"), p)?;
            }
            if let Some(d) = dwell
                && !(d.is_finite() && d >= 0.0)
            {
                return Err(JobError::BadValue {
                    field: format!("{at}.dwell"),
                    value: d,
                });
            }
        }
        Ok(())
    }

    /// The stepover Fusion gives this operation with `tool`, at `tolerance`: for
    /// parallel finishing, a quarter of the tool's diameter with a square end, and
    /// with a round one the spacing that leaves a cusp no taller than the
    /// tolerance, `2·√(r² − (r − tol)²) / √2`; for Z-level clearing, 0.6 of the
    /// diameter, its 3D pocket's, and a 2D pocket's the same; for facing, 95% of
    /// the flat of the tool's end. `None` for a profile or drilling, which have
    /// none, and for facing with a tool that has no flat.
    pub fn default_stepover(&self, tool: &ToolSpec, tolerance: f64) -> Option<f64> {
        let d = tool.diameter;
        match self.strategy {
            StrategySpec::Parallel { .. } => {
                let r = match tool.kind {
                    ToolKind::Ball => d / 2.0,
                    ToolKind::Bullnose => tool.corner,
                    ToolKind::Flat | ToolKind::Cone | ToolKind::Drill => 0.0,
                };
                let over = if r > tolerance && r > 0.0 {
                    2.0 * (r * r - (r - tolerance).powi(2)).sqrt() / 2f64.sqrt()
                } else {
                    d / 4.0
                };
                // To the micron, as Fusion rounds it.
                Some((over * 1000.0).round() / 1000.0)
            }
            StrategySpec::ZlevelRough { .. } | StrategySpec::Pocket { .. } => Some(0.6 * d),
            // Fusion's: 95% of the flat of the tool's end, less its corners.
            StrategySpec::Face { .. } => match tool.kind {
                ToolKind::Flat => Some(0.95 * d),
                ToolKind::Bullnose => Some(0.95 * (d - 2.0 * tool.corner)),
                ToolKind::Ball | ToolKind::Cone | ToolKind::Drill => None,
            },
            StrategySpec::Profile { .. } | StrategySpec::Drill { .. } => None,
        }
    }

    /// Copy the values `preset` has into this operation, leaving the rest, and
    /// record where they came from.
    pub fn apply_preset(&mut self, preset: &Preset) {
        if let Some(rpm) = preset.rpm {
            self.passes.rpm = Some(rpm);
        }
        if let Some(feed) = preset.feed {
            self.passes.feed = feed;
        }
        if let Some(plunge) = preset.plunge_feed {
            self.linking.plunge_feed = plunge;
        }
        if let Some(ramp) = preset.ramp_feed {
            self.linking.ramp_feed = Some(ramp);
        }
        if let Some(stepover) = preset.stepover {
            self.passes.stepover = stepover;
        }
        if let Some(d) = preset.stepdown {
            // Multiple depths stay as they were chosen, on or off.
            let depths = self.strategy.uses().depths;
            match (&mut self.strategy, depths) {
                (StrategySpec::Drill { peck, .. }, _) => *peck = Some(d),
                (_, Depths::Required) => self.passes.stepdown = Some(d),
                (_, Depths::Optional) if self.passes.stepdown.is_some() => {
                    self.passes.stepdown = Some(d)
                }
                _ => {}
            }
        }
        if let Some(s) = preset.stock_to_leave {
            self.passes.stock_to_leave = s;
        }
        self.preset = Some(preset.name.clone());
    }
}

/// How the passes are laid out.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Passes {
    /// Distance between neighbouring passes, in millimetres.
    #[serde(default = "default_stepover")]
    pub stepover: f64,
    /// Deepest each level may be, in millimetres: Fusion's multiple depths, for
    /// a strategy that cuts in levels ([`Uses::depths`]). Straight to the bottom
    /// in one when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stepdown: Option<f64>,
    /// Cutting feed, in mm/min.
    #[serde(default = "default_feed")]
    pub feed: f64,
    /// Material to leave behind: Fusion's radial and axial stock to leave. What
    /// the operation cuts with is [`Operation::stock_to_leave`].
    #[serde(default, skip_serializing_if = "is_default")]
    pub stock_to_leave: StockToLeave,
    /// Largest Z drop that may be traversed as a cut.
    #[serde(default = "default_max_cut_step")]
    pub max_cut_step: f64,
    /// Spindle speed; the tool's own when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpm: Option<u32>,
    /// Tabs left across the deepest cut, to hold what it frees; none when not
    /// given. Any operation that cuts closed loops can have them
    /// ([`crate::tabs`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tabs: Option<Tabs>,
}

impl Default for Passes {
    fn default() -> Self {
        Self {
            stepover: default_stepover(),
            stepdown: None,
            feed: default_feed(),
            stock_to_leave: StockToLeave::default(),
            max_cut_step: default_max_cut_step(),
            rpm: None,
            tabs: None,
        }
    }
}

/// Tabs: bridges of material left standing across the deepest cut, so a part cut
/// free of its sheet stays held in it. Fusion's holding tabs, placed by distance.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Tabs {
    /// Square-sided, or with sides at 45°.
    #[serde(default, skip_serializing_if = "is_default")]
    pub shape: TabShape,
    /// Material left along the cut, at the tab's top, in millimetres.
    pub width: f64,
    /// How far above the cut's floor the tab's top is, in millimetres.
    pub height: f64,
    /// About how far apart they are round each loop, in millimetres: at least
    /// two to a loop.
    pub spacing: f64,
}

impl Default for Tabs {
    /// Two sheet-sized tabs a foot apart, or so: 6 mm wide, 3 mm tall, every
    /// 150 mm.
    fn default() -> Self {
        Self {
            shape: TabShape::default(),
            width: 6.0,
            height: 3.0,
            spacing: 150.0,
        }
    }
}

/// A tab's shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TabShape {
    /// Square-sided: the tool steps straight up onto it and straight down off it.
    #[default]
    Rectangular,
    /// Sides at 45°: the tool climbs onto it and ramps off, never plunging.
    Triangular,
}

fn default_stepover() -> f64 {
    1.0
}

fn default_feed() -> f64 {
    1200.0
}

fn default_max_cut_step() -> f64 {
    0.5
}

/// Material to leave, radial and axial, in millimetres: Fusion's stock to leave.
///
/// Positive leaves material for a later operation to finish, zero cuts to the
/// part, and negative cuts past it, for a looser fit. Where a surface is neither
/// wall nor floor, what is left is between the two.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StockToLeave {
    /// Perpendicular to the tool axis: on the walls.
    #[serde(default)]
    pub radial: f64,
    /// Along the tool axis: on the floors.
    #[serde(default)]
    pub axial: f64,
}

impl StockToLeave {
    /// The first of the two that is not a number, if one is not. How negative
    /// they may be is the tool's to say ([`Operation::cannot_use`]).
    pub(super) fn unusable(&self) -> Option<f64> {
        [self.radial, self.axial]
            .into_iter()
            .find(|v| !v.is_finite())
    }

    /// Check both are numbers, naming `field` if not.
    fn check(&self, field: &str) -> Result<(), JobError> {
        match self.unusable() {
            Some(value) => Err(JobError::BadValue {
                field: field.to_string(),
                value,
            }),
            None => Ok(()),
        }
    }
}

/// How the tool moves between cuts.
///
/// What each setting does, and why its default is Fusion's, is in
/// `docs/decisions/0004-linking-and-entries.md`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Linking {
    /// Feed for coming up out of a cut.
    #[serde(default = "default_retract_feed")]
    pub retract_feed: f64,
    /// Feed for descending into material.
    #[serde(default = "default_plunge_feed")]
    pub plunge_feed: f64,
    /// Whether passes may be reordered to shorten travel.
    #[serde(default = "yes")]
    pub reorder: bool,
    /// Whether an open pass may be reversed while reordering.
    ///
    /// Off by default: reversing flips climb for conventional, and that is a cutting
    /// decision rather than a travel one.
    #[serde(default)]
    pub allow_reverse: bool,
    /// How far above the material the tool stops rapiding down and feeds, and up to
    /// which it feeds on the way out, in millimetres.
    #[serde(
        default = "default_safe_distance",
        skip_serializing_if = "is_default_safe_distance"
    )]
    pub safe_distance: f64,
    /// The longest link made without lifting clear, in millimetres: the
    /// strategy's own when not given (50 mm for parallel, 5 × the tool's diameter
    /// for Z-level), zero to always lift.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stay_down: Option<f64>,
    /// The radius of the vertical arcs a parallel pass is entered and left along,
    /// in millimetres: a tenth of the tool's diameter when not given, zero for
    /// none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_radius: Option<f64>,
    /// How steeply Z-level roughing ramps into material, in degrees from level.
    #[serde(
        default = "default_ramp_angle",
        skip_serializing_if = "is_default_ramp_angle"
    )]
    pub ramp_angle: f64,
    /// Feed along a ramp, in mm/min: a third of the cutting feed when not given,
    /// as Fusion's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp_feed: Option<f64>,
    /// The leads a 2D profile enters and leaves each ring along, in the waste
    /// beside it: Fusion's lead-in and lead-out. None for none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leads: Option<Leads>,
    /// How high the tool goes between cuts that are not made staying down.
    #[serde(default, skip_serializing_if = "is_default")]
    pub retraction: Retraction,
    /// How a pass is entered where it starts in material, for a strategy that
    /// offers a choice: the strategy's own when not given (a helix for a
    /// profile).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<Entry>,
    /// The diameter of a helical entry's circle, as the tool's centre goes, in
    /// millimetres: 0.9 × the tool's diameter when not given, smaller where the
    /// waste has no room for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub helix_diameter: Option<f64>,
    /// The most a helix or a ramp round a ring descends in one lap, in
    /// millimetres: Fusion's maximum ramp stepdown. The operation's stepdown
    /// when not given, or the tool's diameter without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp_stepdown: Option<f64>,
}

/// How high the tool goes between cuts, as Fusion's retraction policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Retraction {
    /// Up only as far as clears the material under the way across, and the safe
    /// distance above it, as the stock stood before the operation; never above
    /// the retract height unless the material is.
    #[default]
    Minimum,
    /// Up to the retract height, or higher where the material under the way
    /// across is.
    Full,
}

/// How a pass that starts in material is entered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Entry {
    /// Down a helix in the waste beside the lead, at the ramp angle: the
    /// profile's own.
    #[default]
    Helix,
    /// Straight down beside the lead at the plunge feed, as a 2D contour
    /// without a ramp does in Fusion.
    Plunge,
    /// Down along the pass itself, in the direction it cuts, at the ramp angle.
    Ramp,
}

/// How a profile comes onto a ring and off it, in the waste beside it, as
/// Fusion's lead-in and lead-out, the same both ways: down a vertical arc, along
/// a straight, and round a level arc onto the ring, tangent to it; and off it the
/// same, mirrored. Each is a tenth of the tool's diameter when not given, as
/// Fusion's.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Leads {
    /// The level arc's radius, in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
    /// How far round the level arc turns, in degrees: Fusion's sweep angle.
    #[serde(
        default = "default_lead_sweep",
        skip_serializing_if = "is_default_lead_sweep"
    )]
    pub sweep: f64,
    /// The straight before the level arc in and after it out, in millimetres:
    /// Fusion's linear lead-in distance; zero for none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub straight: Option<f64>,
    /// The vertical arc's radius, in millimetres: Fusion's vertical lead-in
    /// radius; zero for none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical: Option<f64>,
}

impl Default for Leads {
    /// Fusion's: every size a tenth of the tool, a quarter turn.
    fn default() -> Self {
        Self {
            radius: None,
            sweep: default_lead_sweep(),
            straight: None,
            vertical: None,
        }
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_lead_sweep(v: &f64) -> bool {
    v.to_bits() == default_lead_sweep().to_bits()
}

impl Leads {
    /// Fusion's leads for a tool `diameter` across: a quarter turn, and a tenth
    /// of the diameter each.
    pub fn sizes(&self, diameter: f64) -> LeadSizes {
        let tenth = 0.1 * diameter;
        LeadSizes {
            radius: self.radius.unwrap_or(tenth),
            sweep: self.sweep,
            straight: self.straight.unwrap_or(tenth),
            vertical: self.vertical.unwrap_or(tenth),
        }
    }
}

/// A profile's leads, sized for its tool.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeadSizes {
    /// The level arc's radius, in millimetres.
    pub radius: f64,
    /// How far round the level arc turns, in degrees.
    pub sweep: f64,
    /// The straight, in millimetres.
    pub straight: f64,
    /// The vertical arc's radius, in millimetres.
    pub vertical: f64,
}

fn default_lead_sweep() -> f64 {
    90.0
}

impl Default for Linking {
    fn default() -> Self {
        Self {
            retract_feed: default_retract_feed(),
            plunge_feed: default_plunge_feed(),
            reorder: true,
            allow_reverse: false,
            safe_distance: default_safe_distance(),
            stay_down: None,
            lead_radius: None,
            ramp_angle: default_ramp_angle(),
            ramp_feed: None,
            leads: None,
            retraction: Retraction::Minimum,
            entry: None,
            helix_diameter: None,
            ramp_stepdown: None,
        }
    }
}

fn default_safe_distance() -> f64 {
    2.0
}

// Serde's `skip_serializing_if` hands the field by reference.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_safe_distance(v: &f64) -> bool {
    v.to_bits() == default_safe_distance().to_bits()
}

fn default_ramp_angle() -> f64 {
    2.0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_ramp_angle(v: &f64) -> bool {
    v.to_bits() == default_ramp_angle().to_bits()
}

fn default_retract_feed() -> f64 {
    1500.0
}

fn default_plunge_feed() -> f64 {
    300.0
}

// Serde's `skip_serializing_if` hands the field by reference.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_yes(b: &bool) -> bool {
    *b
}

fn yes() -> bool {
    true
}
