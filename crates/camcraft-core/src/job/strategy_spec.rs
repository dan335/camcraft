//! Which strategy an operation uses, and the parameters only it has, with the
//! choices they are made from.

use serde::de::value::MapDeserializer;
use serde::{Deserialize, Serialize};

use super::{Containment, Depths, StockToLeave, is_default, is_ninety, is_yes, ninety, yes};

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
        /// Go down into what cannot be reached from the side, a closed pocket:
        /// Fusion's Machine Cavities. Off, it only cuts from the outside in,
        /// and leaves them.
        #[serde(default = "yes", skip_serializing_if = "is_yes")]
        machine_cavities: bool,
        /// The tightest the tool's centre turns in an inside corner, in
        /// millimetres: Fusion's Minimum Cutting Radius. Every corner sharper
        /// is blended, leaving material in it for a smaller tool; nothing
        /// narrower than the tool and twice this is cut. None when zero.
        #[serde(default, skip_serializing_if = "is_default")]
        min_cutting_radius: f64,
        /// Level by level, or each area cleared all the way down before the
        /// next: Fusion's Order by Depth and Order by Area.
        #[serde(default, skip_serializing_if = "is_default")]
        order: ContourOrder,
        /// Clear only what the operations before it in the setup left: Fusion's
        /// Rest Machining from previous operations.
        #[serde(default, skip_serializing_if = "is_default")]
        rest_machining: bool,
        /// With rest machining, material the operations before left thinner
        /// than this is let be, in millimetres: Fusion's Ignore Stock Less
        /// Than.
        #[serde(default, skip_serializing_if = "is_default")]
        ignore_stock_less_than: f64,
        /// With flat areas, a level on a flat face or a peak less than this
        /// below the level above it is left out, leaving that much on it, in
        /// millimetres: Fusion's flat area Minimum Stepdown. None when zero.
        #[serde(default, skip_serializing_if = "is_default")]
        min_flat_stepdown: f64,
        /// A level between two a stepdown apart, a fine or a shallow one,
        /// cutting less deep than this is left out, in millimetres: Fusion's
        /// Minimum Axial Engagement. None when zero.
        #[serde(default, skip_serializing_if = "is_default")]
        min_axial_engagement: f64,
        /// Add levels where the terraces between two are wide: Fusion's
        /// Machine Shallow Areas.
        #[serde(default, skip_serializing_if = "is_default")]
        shallow_areas: bool,
        /// The closest the levels shallow areas add come to another, in
        /// millimetres: Fusion's Minimum Shallow Stepdown.
        #[serde(
            default = "default_min_shallow_stepdown",
            skip_serializing_if = "is_default_min_shallow_stepdown"
        )]
        min_shallow_stepdown: f64,
        /// The widest a terrace between two levels may be before shallow
        /// areas add one between, in millimetres: Fusion's Maximum Shallow
        /// Stepover.
        #[serde(
            default = "default_max_shallow_stepover",
            skip_serializing_if = "is_default_max_shallow_stepover"
        )]
        max_shallow_stepover: f64,
        /// A closed pocket gone down into is first cut a slot along its
        /// middle, and cleared out from it: Fusion's Use Slot Clearing.
        #[serde(default, skip_serializing_if = "is_default")]
        slot_clearing: bool,
        /// How wide that slot is, in millimetres: the tool's diameter when
        /// zero, and never narrower. Fusion's Slot Clearing Width.
        #[serde(default, skip_serializing_if = "is_default")]
        slot_width: f64,
        /// How hard a link tries to stay down round what stands between, in
        /// percent: none, 0, goes only straight; 100 goes round as far as the
        /// maximum stay-down distance allows. Fusion's Stay-Down Level.
        #[serde(
            default = "default_stay_down_level",
            skip_serializing_if = "is_default_stay_down_level"
        )]
        stay_down_level: f64,
        /// How far the tool keeps off the material along a link it stays
        /// down for, but where it leaves a pass and comes to the next, in
        /// millimetres: Fusion's Minimum Stay-Down Clearance.
        #[serde(default, skip_serializing_if = "is_default")]
        stay_down_clearance: f64,
        /// How far the tool rises for a link it stays down for, in
        /// millimetres: Fusion's Lift Height.
        #[serde(default, skip_serializing_if = "is_default")]
        lift_height: f64,
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

    /// How a new operation of it keeps to the part's silhouette, as Fusion
    /// starts one: finishing by a pattern dropped onto the part keeps the
    /// tool's centre inside it; pocket and adaptive clearing, contour and flat
    /// finishing reach in from outside, since kept inside the part's outline
    /// the tool would never reach its walls. `None` for a 2D operation, whose
    /// outline starts as the silhouette already, and for facing and drilling,
    /// which have no boundary.
    pub fn starting_containment(&self) -> Option<Containment> {
        match *self {
            StrategySpec::Parallel { .. }
            | StrategySpec::Scallop { .. }
            | StrategySpec::Spiral { .. }
            | StrategySpec::Radial { .. }
            | StrategySpec::MorphedSpiral { .. } => Some(Containment::Center),
            StrategySpec::PocketClearing { .. }
            | StrategySpec::Adaptive { .. }
            | StrategySpec::Contour { .. }
            | StrategySpec::Flat { .. } => Some(Containment::Outside),
            StrategySpec::Profile { .. }
            | StrategySpec::Pocket { .. }
            | StrategySpec::Face { .. }
            | StrategySpec::Drill { .. } => None,
        }
    }

    /// Which way its passes cut, where that is a setting of its own: a 2D
    /// operation's or pocket clearing's one way or the other, or the
    /// direction it is given. `None` where it is not a setting, as parallel's,
    /// radial's and facing's passes go their own ways.
    pub fn direction(&self) -> Option<CutDirection> {
        let one_way = |conventional: bool| {
            if conventional {
                CutDirection::Conventional
            } else {
                CutDirection::Climb
            }
        };
        match *self {
            StrategySpec::Profile { conventional, .. }
            | StrategySpec::Pocket { conventional }
            | StrategySpec::PocketClearing { conventional, .. } => Some(one_way(conventional)),
            StrategySpec::Adaptive { direction, .. }
            | StrategySpec::Scallop { direction, .. }
            | StrategySpec::Contour { direction, .. }
            | StrategySpec::Flat { direction, .. }
            | StrategySpec::Spiral { direction, .. }
            | StrategySpec::MorphedSpiral { direction, .. } => Some(direction),
            StrategySpec::Parallel { .. }
            | StrategySpec::Radial { .. }
            | StrategySpec::Face { .. }
            | StrategySpec::Drill { .. } => None,
        }
    }

    /// Whether it cuts conventional rather than climb, where that is a setting of
    /// its own. Facing's direction is its passes' own, and not asked here.
    pub fn conventional(&self) -> bool {
        self.direction() == Some(CutDirection::Conventional)
    }

    /// Whether every pass cuts the same way, climbing or conventional, so the
    /// material is on the same side of each: not where they cut both ways, or
    /// where which way is not a setting.
    pub fn one_way(&self) -> bool {
        matches!(
            self.direction(),
            Some(CutDirection::Climb | CutDirection::Conventional)
        )
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

    /// The expressions a new operation of this strategy works its settings out
    /// from, so they follow its tool as Fusion's defaults do: Fusion's own,
    /// from a fresh operation of each strategy and Autodesk's note of the
    /// defaults it changed in 2022 ([decision 0020]).
    ///
    /// - **Stepover:** for parallel finishing and the patterns dropped onto
    ///   the part (scallop, spiral, radial, morphed spiral), the spacing that
    ///   leaves a cusp no taller than the tolerance with a round end, a quarter
    ///   of the diameter with a square one, to the micron; for 3D and 2D
    ///   pockets, 0.6 of the diameter; adaptive's optimal load, 0.4 of it;
    ///   flat finishing, 0.3 of it; facing, 0.7 of the flat of the tool's end.
    /// - **Stepdown:** adaptive's maximum roughing stepdown,
    ///   `Math.min(tool_fluteLength * 0.75; tool_diameter * 2.5)`; a pocket's,
    ///   3D or 2D, `Math.min(tool_fluteLength * 0.75; tool_diameter * 0.1)`; a
    ///   profile's, three quarters of the flutes. Contour finishing's is a
    ///   millimetre, [`StrategySpec::default_stepdown`].
    /// - **Feeds:** plunging at a third of the cutting feed, and ramping at
    ///   the plunge feed, as a new tool of Fusion's does; drilling's own.
    /// - **Adaptive's own:** a minimum cutting radius of a tenth of the tool,
    ///   a stay-down clearance of a tenth of it between 2 and 15 mm, and a slot
    ///   1.25 tools wide when slot clearing is on.
    /// - **Facing's own:** passes running half the tool and the tolerance past
    ///   the stock.
    ///
    /// [decision 0020]: ../../../../docs/decisions/0020-defaults.md
    pub fn default_expressions(&self) -> Vec<(&'static str, &'static str)> {
        const CUSP: &str = "tool_cornerRadius > tolerance \
            ? Math.roundto(Math.sqrt(2 * (tool_cornerRadius^2 - (tool_cornerRadius - tolerance)^2)); 0.001) \
            : Math.roundto(tool_diameter / 4; 0.001)";
        const ROUGHING_STEPDOWN: &str = "Math.min(tool_fluteLength * 0.75; tool_diameter * 2.5)";
        const POCKET_STEPDOWN: &str = "Math.min(tool_fluteLength * 0.75; tool_diameter * 0.1)";
        let mut own: Vec<(&'static str, &'static str)> = match self {
            // A scallop pass is parallel's cusp, kept on a slope too.
            StrategySpec::Parallel { .. }
            | StrategySpec::Scallop { .. }
            | StrategySpec::Spiral { .. }
            | StrategySpec::Radial { .. }
            | StrategySpec::MorphedSpiral { .. } => vec![("passes.stepover", CUSP)],
            StrategySpec::PocketClearing { .. } | StrategySpec::Pocket { .. } => vec![
                ("passes.stepover", "tool_diameter * 0.6"),
                ("passes.stepdown", POCKET_STEPDOWN),
            ],
            StrategySpec::Flat { .. } => vec![("passes.stepover", "tool_diameter * 0.3")],
            StrategySpec::Adaptive { .. } => vec![
                ("passes.stepover", "tool_diameter * 0.4"),
                ("passes.stepdown", ROUGHING_STEPDOWN),
                ("min_cutting_radius", "tool_diameter * 0.1"),
                (
                    "stay_down_clearance",
                    "Math.max(Math.min(tool_diameter * 0.1; 15mm); 2mm)",
                ),
                ("slot_width", "tool_diameter * 1.25"),
            ],
            StrategySpec::Face { .. } => vec![
                (
                    "passes.stepover",
                    "Math.max((tool_diameter - 2 * tool_cornerRadius) * 0.7; tool_diameter * 0.05)",
                ),
                ("pass_extension", "tool_diameter / 2 + tolerance"),
            ],
            StrategySpec::Profile { .. } => vec![("passes.stepdown", "tool_fluteLength * 0.75")],
            StrategySpec::Contour { .. } | StrategySpec::Drill { .. } => Vec::new(),
        };
        // A drill's feed down is its cutting feed: it has no plunge of its own.
        if !matches!(self, StrategySpec::Drill { .. }) {
            own.push(("linking.plunge_feed", "tool_feedCutting / 3"));
        }
        if self.uses().ramp {
            own.push(("linking.ramp_feed", "tool_feedPlunge"));
        }
        own
    }

    /// What a new operation of this strategy starts with that is not worked
    /// out from its tool, where it is not what a job file that leaves the
    /// setting out reads: adaptive lifts a fifth of a millimetre off the
    /// floor between passes, as Fusion's has since 2022.
    pub fn fresh(mut self) -> Self {
        if let StrategySpec::Adaptive { lift_height, .. } = &mut self {
            *lift_height = 0.2;
        }
        self
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

/// Which order a contour's levels are cut in, or adaptive clearing's.
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

default_number!(full_turn, is_full_turn = 360.0);

fn climb() -> CutDirection {
    CutDirection::Climb
}

// Serde's `skip_serializing_if` hands the field by reference.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_climb(d: &CutDirection) -> bool {
    *d == CutDirection::Climb
}

default_number!(
    /// Fusion's Minimum Shallow Stepdown, in millimetres.
    default_min_shallow_stepdown,
    is_default_min_shallow_stepdown = 0.5
);

default_number!(
    /// Fusion's Maximum Shallow Stepover, in millimetres.
    default_max_shallow_stepover,
    is_default_max_shallow_stepover = 2.0
);

default_number!(
    /// Staying down round what stands between as far as the maximum
    /// stay-down distance allows, in percent.
    default_stay_down_level,
    is_default_stay_down_level = 100.0
);

fn default_min_ring() -> f64 {
    3.0
}
