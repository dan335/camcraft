//! How the passes are laid out: Fusion's Passes tab.

use serde::{Deserialize, Serialize};

use super::{Depths, Leave, Operation, StrategySpec, ToolKind, ToolSpec, is_default, is_yes, yes};

/// How the passes are laid out.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Passes {
    /// Distance between neighbouring passes, in millimetres.
    #[serde(default = "default_stepover")]
    pub stepover: f64,
    /// Deepest each level may be, in millimetres: Fusion's multiple depths, for
    /// a strategy that cuts in levels ([`Uses::depths`](super::Uses::depths)).
    /// Straight to the bottom in one when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stepdown: Option<f64>,
    /// Fusion's fine stepdown, in millimetres, for a strategy that has one
    /// ([`Uses::fine_stepdown`](super::Uses::fine_stepdown)): levels this far
    /// apart up from each level and flat, between it and the level above, each
    /// cutting only what the ones below it left there. On a slope, that is the
    /// terraces a stepdown apart would leave, cut down to steps this tall. None
    /// when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fine_stepdown: Option<f64>,
    /// Cutting feed, in mm/min.
    #[serde(default = "default_feed")]
    pub feed: f64,
    /// Material to leave behind: Fusion's radial and axial stock to leave. What
    /// the operation cuts with is [`Operation::stock_to_leave`].
    #[serde(default, skip_serializing_if = "is_default")]
    pub stock_to_leave: StockToLeave,
    /// Largest Z drop that may be traversed as a cut: Fusion has none. What
    /// the operation cuts with is [`Operation::max_cut_step`].
    #[serde(default = "default_max_cut_step")]
    pub max_cut_step: f64,
    /// Spindle speed; the tool's own when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpm: Option<u32>,
    /// Tabs left across the deepest cut, to hold what it frees; none when not
    /// given. Only a profile has them ([`Uses::tabs`](super::Uses::tabs),
    /// [`crate::path::tabs`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tabs: Option<Tabs>,
    /// Passes along a pocket's walls with the side of the tool after each
    /// level is cleared, the clearing leaving them their stepovers; none when
    /// not given, as Fusion's 2D pocket has none until asked
    /// ([`Uses::finishing`](super::Uses::finishing)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finishing: Option<Finishing>,
    /// Fit the passes' runs of short moves with arcs and longer lines, for a
    /// strategy that may be ([`Uses::smoothing`](super::Uses::smoothing)):
    /// Fusion's Smoothing, off until asked ([`crate::path::fit`]).
    #[serde(default, skip_serializing_if = "is_default")]
    pub smoothing: bool,
    /// How far the fitted moves may stray from the passes, in millimetres:
    /// Fusion's Smoothing Tolerance. The job's tolerance when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smoothing_tolerance: Option<f64>,
    /// Cuts slowed into and out of their sharp corners and round their tight
    /// arcs, for a strategy that may be
    /// ([`Uses::feed_optimization`](super::Uses::feed_optimization)): Fusion's
    /// Feed Optimization, off until asked ([`crate::path::corners`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed_optimization: Option<FeedOptimization>,
}

impl Default for Passes {
    fn default() -> Self {
        Self {
            stepover: default_stepover(),
            stepdown: None,
            fine_stepdown: None,
            feed: default_feed(),
            stock_to_leave: StockToLeave::default(),
            max_cut_step: default_max_cut_step(),
            rpm: None,
            tabs: None,
            finishing: None,
            smoothing: false,
            smoothing_tolerance: None,
            feed_optimization: None,
        }
    }
}

/// A pocket's finishing passes along its walls: Fusion's 2D pocket's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Finishing {
    /// How many, each a stepover nearer the wall, the last at it: Fusion's
    /// Number of Finishing Passes, one when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passes: Option<u32>,
    /// The distance between them, and the most the last takes off the wall,
    /// in millimetres: a tenth of the tool when not given, Fusion's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stepover: Option<f64>,
    /// Feed along the last, in mm/min: Fusion's Finish Feedrate, the cutting
    /// feed when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed: Option<f64>,
    /// Whether the last is cut twice, for a tool that springs from the wall:
    /// Fusion's Repeat Finishing Pass.
    #[serde(default, skip_serializing_if = "is_default")]
    pub repeat: bool,
}

impl Finishing {
    /// Every value its own, or Fusion's for a tool `diameter` across cutting
    /// at `feed`.
    pub fn sizes(&self, diameter: f64, feed: f64) -> FinishingSizes {
        FinishingSizes {
            passes: self.passes.unwrap_or(1),
            stepover: self.stepover.unwrap_or(0.1 * diameter),
            feed: self.feed.unwrap_or(feed),
            repeat: self.repeat,
        }
    }
}

/// A pocket's finishing passes, every value its own or its default for the
/// tool.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FinishingSizes {
    /// How many.
    pub passes: u32,
    /// The distance between them, in millimetres.
    pub stepover: f64,
    /// Feed along the last, in mm/min.
    pub feed: f64,
    /// Whether the last is cut twice.
    pub repeat: bool,
}

/// Fusion's Feed Optimization: where cuts slow, and to what. Fusion's
/// reference gives no defaults; these are camcraft's.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FeedOptimization {
    /// The most a cut may turn into the next without slowing, in degrees:
    /// Fusion's Maximum Directional Change. 45 degrees when not given.
    #[serde(
        default = "default_max_directional_change",
        skip_serializing_if = "is_default_max_directional_change"
    )]
    pub max_directional_change: f64,
    /// An arc tighter than this is slowed along, in millimetres: Fusion's
    /// Reduced Feed Radius. The tool's radius when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduced_feed_radius: Option<f64>,
    /// How far before a corner the cut slows, and after it speeds up again,
    /// in millimetres: Fusion's Reduced Feed Distance. The tool's radius when
    /// not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduced_feed_distance: Option<f64>,
    /// The feed slowed to, in mm/min: Fusion's Reduced Feedrate. Half the
    /// cutting feed when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduced_feed: Option<f64>,
    /// Only the corners that turn into the material: Fusion's Only Inner
    /// Corners, on unless turned off.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub only_inner_corners: bool,
}

impl Default for FeedOptimization {
    fn default() -> Self {
        Self {
            max_directional_change: default_max_directional_change(),
            reduced_feed_radius: None,
            reduced_feed_distance: None,
            reduced_feed: None,
            only_inner_corners: true,
        }
    }
}

impl FeedOptimization {
    /// Every value its own, or its default for a tool of `radius` cutting at
    /// `feed`.
    pub fn sizes(&self, radius: f64, feed: f64) -> FeedOptimizationSizes {
        FeedOptimizationSizes {
            max_directional_change: self.max_directional_change,
            radius: self.reduced_feed_radius.unwrap_or(radius),
            distance: self.reduced_feed_distance.unwrap_or(radius),
            feed: self.reduced_feed.unwrap_or(0.5 * feed),
            only_inner_corners: self.only_inner_corners,
        }
    }
}

/// Fusion's Feed Optimization, every value its own or its default for the
/// tool and the cutting feed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeedOptimizationSizes {
    /// The most a cut may turn without slowing, in degrees.
    pub max_directional_change: f64,
    /// An arc tighter than this is slowed along, in millimetres.
    pub radius: f64,
    /// How far either side of a corner the cut is slowed, in millimetres.
    pub distance: f64,
    /// The feed slowed to, in mm/min.
    pub feed: f64,
    /// Only corners that turn into the material.
    pub only_inner_corners: bool,
}

default_number!(
    default_max_directional_change,
    is_default_max_directional_change = 45.0
);

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
}

impl Operation {
    /// Its finishing passes along the walls, sized for a tool `diameter`
    /// across, when they are asked for and its strategy has them.
    pub fn finishing(&self, diameter: f64) -> Option<FinishingSizes> {
        let f = self
            .passes
            .finishing
            .filter(|_| self.strategy.uses().finishing)?;
        Some(f.sizes(diameter, self.passes.feed))
    }

    /// The tallest near-vertical drop it may cut down at the cutting feed: its
    /// own, unless its strategy has none ([`Uses::cut_step`](super::Uses::cut_step)),
    /// when it is the default.
    pub fn max_cut_step(&self) -> f64 {
        if self.strategy.uses().cut_step {
            self.passes.max_cut_step
        } else {
            default_max_cut_step()
        }
    }

    /// The deepest each of its levels may be, when it cuts in levels: its own,
    /// unless its strategy has none. `None` is straight to the bottom in one.
    pub fn stepdown(&self) -> Option<f64> {
        self.passes
            .stepdown
            .filter(|_| self.strategy.uses().depths != Depths::None)
    }

    /// How far its passes' fitted moves may stray from them, in millimetres,
    /// with smoothing on and a strategy that may be smoothed: its own, or the
    /// job's `tolerance`. `None` for no smoothing.
    pub fn smoothing(&self, tolerance: f64) -> Option<f64> {
        (self.passes.smoothing && self.strategy.uses().smoothing)
            .then(|| self.passes.smoothing_tolerance.unwrap_or(tolerance))
    }

    /// Its feed optimisation, sized for a tool of `radius`, when it is asked
    /// for and its strategy may have it.
    pub fn feed_optimization(&self, radius: f64) -> Option<FeedOptimizationSizes> {
        let f = self
            .passes
            .feed_optimization
            .filter(|_| self.strategy.uses().feed_optimization)?;
        Some(f.sizes(radius, self.passes.feed))
    }

    /// Its fine stepdown, when its strategy has one and it cuts in levels: its
    /// own, if given and less than its stepdown, which leaves no room for one.
    pub fn fine_stepdown(&self) -> Option<f64> {
        let fine = self
            .passes
            .fine_stepdown
            .filter(|_| self.strategy.uses().fine_stepdown)?;
        self.stepdown().filter(|s| fine < *s).map(|_| fine)
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
        let corner = tool.corner_radius();
        StockToLeave {
            radial: -tool.diameter / 2.0,
            axial: if !self.strategy.uses().outline && corner > 0.0 {
                -corner
            } else {
                f64::NEG_INFINITY
            },
        }
    }

    /// The stepover Fusion gives this operation with `tool`, at `tolerance`: for
    /// parallel finishing and the patterns dropped onto the part (scallop,
    /// spiral, radial and morphed spiral), a quarter of the tool's diameter with
    /// a square end, and with a round one the spacing that leaves a cusp no
    /// taller than the tolerance, `2·√(r² − (r − tol)²) / √2`; for 3D pocket
    /// clearing, a 2D pocket and flat finishing, 0.6 of the diameter; for
    /// adaptive clearing, its optimal load, a tenth of the diameter; for facing,
    /// 95% of the flat of the tool's end. `None` for contour finishing, a profile
    /// or drilling, which have none, and for facing with a tool that has no flat.
    pub fn default_stepover(&self, tool: &ToolSpec, tolerance: f64) -> Option<f64> {
        let d = tool.diameter;
        match self.strategy {
            // A scallop pass is parallel's cusp, kept on a slope too.
            StrategySpec::Parallel { .. }
            | StrategySpec::Scallop { .. }
            | StrategySpec::Spiral { .. }
            | StrategySpec::Radial { .. }
            | StrategySpec::MorphedSpiral { .. } => {
                let r = tool.corner_radius();
                let over = if r > tolerance && r > 0.0 {
                    2.0 * (r * r - (r - tolerance).powi(2)).sqrt() / 2f64.sqrt()
                } else {
                    d / 4.0
                };
                // To the micron, as Fusion rounds it.
                Some((over * 1000.0).round() / 1000.0)
            }
            StrategySpec::PocketClearing { .. }
            | StrategySpec::Pocket { .. }
            | StrategySpec::Flat { .. } => Some(0.6 * d),
            // Fusion's optimal load: a tenth of the tool, to the micron.
            StrategySpec::Adaptive { .. } => Some((0.1 * d * 1000.0).round() / 1000.0),
            // Fusion's: 95% of the flat of the tool's end, less its corners.
            StrategySpec::Face { .. } => match tool.kind {
                ToolKind::Flat => Some(0.95 * d),
                ToolKind::Bullnose => Some(0.95 * (d - 2.0 * tool.corner)),
                ToolKind::Ball | ToolKind::Cone | ToolKind::Drill => None,
            },
            StrategySpec::Profile { .. }
            | StrategySpec::Contour { .. }
            | StrategySpec::Drill { .. } => None,
        }
    }
}
