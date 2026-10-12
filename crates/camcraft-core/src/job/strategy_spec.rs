//! Which strategy an operation uses, and the parameters only it has, with the
//! choices they are made from.

use serde::de::value::MapDeserializer;
use serde::{Deserialize, Serialize};

use super::{Depths, StockToLeave, is_default, is_yes, yes};

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
    /// Finishing passes a constant distance apart along the surface, offset in
    /// from the boundary, so the cusp between them is the same on a slope as on
    /// the flat. Fusion's Scallop.
    Scallop {
        /// Which way each pass cuts.
        #[serde(default, skip_serializing_if = "is_default")]
        direction: CutDirection,
        /// From the boundary in, or from the middle out.
        #[serde(default, skip_serializing_if = "is_default")]
        order: PassOrder,
        /// Only where the surface is at least this steep, in degrees from level.
        #[serde(default, skip_serializing_if = "is_default")]
        slope_from: f64,
        /// And at most this steep, in degrees from level.
        #[serde(default = "ninety", skip_serializing_if = "is_ninety")]
        slope_to: f64,
    },
    /// Passes round the part at levels a stepdown apart ([`Depths::Required`]),
    /// each where the tool touches the walls at its height, and one at the foot
    /// of every wall standing on a flat. Fusion's Contour: a waterline.
    Contour {
        /// Which way each pass cuts; climbing unless asked.
        #[serde(default = "climb", skip_serializing_if = "is_climb")]
        direction: CutDirection,
        /// Level by level, or each piece of the part all the way down.
        #[serde(default, skip_serializing_if = "is_default")]
        order: ContourOrder,
        /// Only where the surface is at least this steep, in degrees from level.
        #[serde(default, skip_serializing_if = "is_default")]
        slope_from: f64,
        /// And at most this steep, in degrees from level.
        #[serde(default = "ninety", skip_serializing_if = "is_ninety")]
        slope_to: f64,
    },
    /// Each flat face of the part cleared at its own height, rings a stepover
    /// apart in from its edges, and nothing else. Fusion's Flat.
    Flat {
        /// Which way each ring cuts; climbing unless asked.
        #[serde(default = "climb", skip_serializing_if = "is_climb")]
        direction: CutDirection,
        /// From the edges in, or from the middle out.
        #[serde(default, skip_serializing_if = "is_default")]
        order: PassOrder,
    },
    /// A spiral about a centre, a stepover further out each lap, dropped onto
    /// the part. Fusion's Spiral.
    Spiral {
        /// Its centre, in the setup's frame; the boundary's middle unless given.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        centre: Option<[f64; 2]>,
        /// Nothing nearer the centre than this, in millimetres.
        #[serde(default, skip_serializing_if = "is_default")]
        inner_radius: f64,
        /// Nothing further out than this, in millimetres; out to the whole
        /// boundary if zero.
        #[serde(default, skip_serializing_if = "is_default")]
        outer_radius: f64,
        /// Which way it turns: to climb, or to cut conventional.
        #[serde(default = "climb", skip_serializing_if = "is_climb")]
        direction: CutDirection,
        /// In from the outer radius, or out from the inner.
        #[serde(default, skip_serializing_if = "is_default")]
        order: PassOrder,
        /// Only where the surface is at least this steep, in degrees from level.
        #[serde(default, skip_serializing_if = "is_default")]
        slope_from: f64,
        /// And at most this steep, in degrees from level.
        #[serde(default = "ninety", skip_serializing_if = "is_ninety")]
        slope_to: f64,
    },
    /// Spokes out from a centre, no more than a stepover apart at the outer
    /// radius, dropped onto the part. Fusion's Radial.
    Radial {
        /// Its centre, in the setup's frame; the boundary's middle unless given.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        centre: Option<[f64; 2]>,
        /// Nothing nearer the centre than this, in millimetres.
        #[serde(default, skip_serializing_if = "is_default")]
        inner_radius: f64,
        /// Nothing further out than this, in millimetres; out to the whole
        /// boundary if zero.
        #[serde(default, skip_serializing_if = "is_default")]
        outer_radius: f64,
        /// The first spoke's angle, in degrees anticlockwise from +X.
        #[serde(default, skip_serializing_if = "is_default")]
        angle_from: f64,
        /// The last's: a whole turn round unless asked.
        #[serde(default = "full_turn", skip_serializing_if = "is_full_turn")]
        angle_to: f64,
        /// Out and back, or every spoke one way.
        #[serde(default, skip_serializing_if = "is_default")]
        direction: RadialDirection,
        /// Only where the surface is at least this steep, in degrees from level.
        #[serde(default, skip_serializing_if = "is_default")]
        slope_from: f64,
        /// And at most this steep, in degrees from level.
        #[serde(default = "ninety", skip_serializing_if = "is_ninety")]
        slope_to: f64,
    },
    /// A spiral that turns from the boundary's shape into a centre, its laps a
    /// stepover apart, dropped onto the part. Fusion's Morphed Spiral.
    MorphedSpiral {
        /// Where it turns into, in the setup's frame; the point furthest inside
        /// the boundary unless given.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        centre: Option<[f64; 2]>,
        /// Which way it turns: to climb, or to cut conventional.
        #[serde(default = "climb", skip_serializing_if = "is_climb")]
        direction: CutDirection,
        /// In from the boundary, or out from the centre.
        #[serde(default, skip_serializing_if = "is_default")]
        order: PassOrder,
        /// Only where the surface is at least this steep, in degrees from level.
        #[serde(default, skip_serializing_if = "is_default")]
        slope_from: f64,
        /// And at most this steep, in degrees from level.
        #[serde(default = "ninety", skip_serializing_if = "is_ninety")]
        slope_to: f64,
    },
    /// Z-level clearing by offsets, a level each stepdown ([`Depths::Required`]):
    /// each level's rings a stepover apart in from its edges, cut from the middle
    /// out, the one along the walls last. Fusion's 3D Pocket Clearing.
    PocketClearing {
        /// Rings shorter than this are not worth the entry.
        #[serde(default = "default_min_ring")]
        min_ring_length: f64,
        /// Cut conventional rather than climb.
        #[serde(default, skip_serializing_if = "is_default")]
        conventional: bool,
        /// Add a level on each flat face, so floors are left just the stock to
        /// leave down. Fusion's flat area detection.
        #[serde(default = "yes", skip_serializing_if = "is_yes")]
        flat_areas: bool,
    },
    /// Z-level clearing with the tool's load held constant: each pass steered
    /// to keep the arc of the tool in material at the optimal load, the
    /// stepover ([`Depths::Required`]). Fusion's 3D Adaptive Clearing.
    Adaptive {
        /// Which way the passes cut; climbing unless asked.
        #[serde(default = "climb", skip_serializing_if = "is_climb")]
        direction: CutDirection,
        /// Add a level on each flat face, so floors are left just the stock to
        /// leave down. Fusion's flat area detection.
        #[serde(default = "yes", skip_serializing_if = "is_yes")]
        flat_areas: bool,
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
    /// The strategy a job file calls `name`, each of its parameters as a file
    /// that gives none has it: what a new operation of it starts from.
    pub fn named(name: &str) -> Option<Self> {
        let only = std::iter::once(("strategy", name));
        Self::deserialize(MapDeserializer::<_, serde::de::value::Error>::new(only)).ok()
    }

    /// The strategy's name as a job file writes it, which is also how a program's
    /// section comment names it.
    pub fn name(&self) -> &'static str {
        match self {
            StrategySpec::Parallel { .. } => "parallel",
            StrategySpec::Scallop { .. } => "scallop",
            StrategySpec::Contour { .. } => "contour",
            StrategySpec::Flat { .. } => "flat",
            StrategySpec::Spiral { .. } => "spiral",
            StrategySpec::Radial { .. } => "radial",
            StrategySpec::MorphedSpiral { .. } => "morphed-spiral",
            StrategySpec::PocketClearing { .. } => "pocket-clearing",
            StrategySpec::Adaptive { .. } => "adaptive",
            StrategySpec::Profile { .. } => "profile",
            StrategySpec::Pocket { .. } => "pocket",
            StrategySpec::Face { .. } => "face",
            StrategySpec::Drill { .. } => "drill",
        }
    }

    /// Whether it cuts conventional rather than climb, where that is a setting of
    /// its own. Facing's direction is its passes' own, and not asked here.
    pub fn conventional(&self) -> bool {
        match *self {
            StrategySpec::Profile { conventional, .. }
            | StrategySpec::Pocket { conventional }
            | StrategySpec::PocketClearing { conventional, .. } => conventional,
            StrategySpec::Adaptive { direction, .. }
            | StrategySpec::Scallop { direction, .. }
            | StrategySpec::Contour { direction, .. }
            | StrategySpec::Flat { direction, .. }
            | StrategySpec::Spiral { direction, .. }
            | StrategySpec::MorphedSpiral { direction, .. } => {
                direction == CutDirection::Conventional
            }
            StrategySpec::Parallel { .. }
            | StrategySpec::Radial { .. }
            | StrategySpec::Face { .. }
            | StrategySpec::Drill { .. } => false,
        }
    }

    /// The range of slopes it keeps to, in degrees from level, where it has one.
    pub fn slopes(&self) -> Option<(f64, f64)> {
        match *self {
            StrategySpec::Scallop {
                slope_from,
                slope_to,
                ..
            }
            | StrategySpec::Contour {
                slope_from,
                slope_to,
                ..
            }
            | StrategySpec::Spiral {
                slope_from,
                slope_to,
                ..
            }
            | StrategySpec::Radial {
                slope_from,
                slope_to,
                ..
            }
            | StrategySpec::MorphedSpiral {
                slope_from,
                slope_to,
                ..
            } => Some((slope_from, slope_to)),
            _ => None,
        }
    }

    /// The stock to leave Fusion starts this strategy with: roughing leaves half
    /// a millimetre each way for the finish, and the rest cut to size.
    pub fn default_stock_to_leave(&self) -> StockToLeave {
        match self {
            StrategySpec::PocketClearing { .. } | StrategySpec::Adaptive { .. } => StockToLeave {
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
            // A finishing waterline's, as Fusion's Contour.
            _ if matches!(self, StrategySpec::Contour { .. }) => Some(1.0),
            _ => Some(2.0),
        }
    }
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

/// Which way a strategy's passes cut, where it has the choice: scallop, contour,
/// flat, spiral and morphed spiral finishing, and adaptive clearing.
///
/// Each strategy says what its default is: scallop goes both ways, as Fusion's
/// does, and the rest climb.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CutDirection {
    /// Both ways. A finishing pass: closed rings climbing, and an open pass
    /// started at whichever end is nearer. Adaptive clearing: climbing, and
    /// cutting back conventional where a pass ends beside what is left, rather
    /// than going back to climb it.
    #[default]
    BothWays,
    /// Every pass climbing.
    Climb,
    /// Every pass conventional.
    Conventional,
}

impl CutDirection {
    /// The generators' own.
    pub fn engine(self) -> crate::strategy::adaptive::Direction {
        match self {
            CutDirection::BothWays => crate::strategy::adaptive::Direction::BothWays,
            CutDirection::Climb => crate::strategy::adaptive::Direction::Climb,
            CutDirection::Conventional => crate::strategy::adaptive::Direction::Conventional,
        }
    }
}

/// Which way round passes nested one in another are cut: scallop's, a flat's,
/// a spiral's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PassOrder {
    /// From the boundary in. Fusion's default.
    #[default]
    OutsideIn,
    /// Each region's innermost passes first, out to the boundary.
    InsideOut,
}

impl PassOrder {
    /// The generators' own.
    pub fn engine(self) -> crate::strategy::nesting::Order {
        match self {
            PassOrder::OutsideIn => crate::strategy::nesting::Order::OutsideIn,
            PassOrder::InsideOut => crate::strategy::nesting::Order::InsideOut,
        }
    }
}

/// Which order a contour's levels are cut in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContourOrder {
    /// Each level all the way round before the next down. Fusion's default.
    #[default]
    ByDepth,
    /// Each piece of the part all the way down before the next.
    ByArea,
}

impl ContourOrder {
    /// The generator's own.
    pub fn engine(self) -> crate::strategy::waterline::Order {
        match self {
            ContourOrder::ByDepth => crate::strategy::waterline::Order::ByDepth,
            ContourOrder::ByArea => crate::strategy::waterline::Order::ByArea,
        }
    }
}

/// Which way radial spokes are cut.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RadialDirection {
    /// Out and back in, one after the other. Fusion's default.
    #[default]
    BothWays,
    /// Every spoke from the centre out.
    Outward,
    /// Every spoke in to the centre.
    Inward,
}

impl RadialDirection {
    /// The generator's own.
    pub fn engine(self) -> crate::strategy::patterns::Spokes {
        match self {
            RadialDirection::BothWays => crate::strategy::patterns::Spokes::BothWays,
            RadialDirection::Outward => crate::strategy::patterns::Spokes::Outward,
            RadialDirection::Inward => crate::strategy::patterns::Spokes::Inward,
        }
    }
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

fn ninety() -> f64 {
    90.0
}

fn full_turn() -> f64 {
    360.0
}

// A signature serde's `skip_serializing_if` requires.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_full_turn(v: &f64) -> bool {
    (*v - 360.0).abs() < f64::EPSILON
}

fn climb() -> CutDirection {
    CutDirection::Climb
}

// A signature serde's `skip_serializing_if` requires.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_climb(d: &CutDirection) -> bool {
    *d == CutDirection::Climb
}

// A signature serde's `skip_serializing_if` requires.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_ninety(v: &f64) -> bool {
    (*v - 90.0).abs() < f64::EPSILON
}

fn default_min_ring() -> f64 {
    3.0
}
