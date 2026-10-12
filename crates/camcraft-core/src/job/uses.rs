//! Which of the settings every operation has its strategy uses.

use super::{Entry, HeightRef, ProfileSide, StrategySpec};

impl StrategySpec {
    /// Which of the settings every operation has this strategy uses.
    pub fn uses(&self) -> Uses {
        // Tabs go only where every ring is beside what is kept: a profile's, as
        // Fusion's go only on a 2D contour. Put on a pocket's or a roughing
        // level's clearing rings, they would leave pillars standing in the
        // waste.
        let cuts_loops = Uses {
            boundary: true,
            outline: false,
            holes: false,
            stepover: true,
            load: false,
            feed: true,
            stock_to_leave: Leave::Both,
            depths: Depths::Optional,
            tabs: false,
            links: Links::Level,
            stay_down: true,
            lead_radius: false,
            ramp: true,
            entries: &[],
            leads: false,
            // Only parallel's passes are in no order of their own.
            reorder: false,
            top: Some(HeightRef::StockTop),
            bottom: Some(HeightRef::ModelBottom),
        };
        match *self {
            // Down the surface in one pass, as Fusion's 3D finishing: it
            // follows the part all the way down, with no top or bottom.
            StrategySpec::Parallel { .. } => Uses {
                depths: Depths::None,
                links: Links::OverPart,
                lead_radius: true,
                ramp: false,
                reorder: true,
                top: None,
                bottom: None,
                ..cuts_loops
            },
            // As parallel, but in its own order: reordering would shuffle and
            // turn round the rings on any flat, which all start level. A
            // pattern dropped onto the part is cut in the order it is laid out.
            StrategySpec::Scallop { .. }
            | StrategySpec::Spiral { .. }
            | StrategySpec::Radial { .. }
            | StrategySpec::MorphedSpiral { .. } => Uses {
                depths: Depths::None,
                links: Links::OverPart,
                lead_radius: true,
                ramp: false,
                top: None,
                bottom: None,
                ..cuts_loops
            },
            // A level each stepdown is what it is, going round, not over: it
            // has no stepover. Its order is its own. Its levels are the
            // part's, from its top, and each runs round the part beside its
            // walls, below where a tool from above would rest: linked at the
            // level, never over the part, whose way up from a wall's foot is
            // through the wall.
            StrategySpec::Contour { .. } => Uses {
                stepover: false,
                depths: Depths::Required,
                links: Links::Level,
                lead_radius: true,
                ramp: false,
                top: Some(HeightRef::ModelTop),
                ..cuts_loops
            },
            // Each flat at its own height, once: rings in from its edges, in
            // their own order, as a pocket clearing level's are. Only the
            // part's flats, from its top.
            StrategySpec::Flat { .. } => Uses {
                depths: Depths::None,
                top: Some(HeightRef::ModelTop),
                ..cuts_loops
            },
            // Slice by slice is what it is. Its order is its own, each level's
            // rings from the middle out, and each the way it cuts.
            StrategySpec::PocketClearing { .. } => Uses {
                depths: Depths::Required,
                ..cuts_loops
            },
            // Its stepover is its optimal load. It goes down a helix, or
            // straight, into what it cannot reach from the side, as Fusion's
            // 3D Adaptive's ramp types (its Predrill aside): it has no ring to
            // ramp along. Its passes are in the order it steers them, linked at
            // the level where it has cleared the way.
            StrategySpec::Adaptive { .. } => Uses {
                load: true,
                depths: Depths::Required,
                links: Links::Own,
                entries: &[Entry::Helix, Entry::Plunge],
                ..cuts_loops
            },
            // A 2D operation's order is its own: what is inside before what frees
            // it, a pocket's middle before its walls.
            StrategySpec::Pocket { .. } => Uses {
                boundary: false,
                outline: true,
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
                entries: &[Entry::Helix, Entry::Plunge, Entry::Ramp],
                ..cuts_loops
            },
            // Straight passes across the stock, in its own order, with only the
            // floor to leave anything on, down to the part's top.
            StrategySpec::Face { .. } => Uses {
                boundary: false,
                stock_to_leave: Leave::Axial,
                bottom: Some(HeightRef::ModelTop),
                ..cuts_loops
            },
            // Its picks are the holes: no area to keep to or cut round.
            StrategySpec::Drill { .. } => Uses {
                boundary: false,
                outline: false,
                holes: true,
                stepover: false,
                load: false,
                // Each hole is fed down at the plunge feed.
                feed: false,
                stock_to_leave: Leave::None,
                // Its pecks are its own.
                depths: Depths::None,
                tabs: false,
                // Up out of each hole, and across to the next.
                links: Links::Own,
                stay_down: false,
                lead_radius: false,
                ramp: false,
                entries: &[],
                leads: false,
                // Holes are drilled in the order found, nearest first.
                reorder: false,
                // Each from its rim, to its bottom edge or the Bottom.
                top: None,
                bottom: Some(HeightRef::ModelBottom),
            },
        }
    }
}

/// Which of the settings every operation has (its boundary or outline,
/// [`Passes`](super::Passes), [`Linking`](super::Linking)) its strategy uses,
/// as [`StrategySpec::uses`] says.
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
    /// Holes: its picks are what it drills, not an area, and no height is
    /// measured from them, since each hole goes to its own bottom.
    pub holes: bool,
    /// The distance between passes.
    pub stepover: bool,
    /// Its stepover is the tool's load, the width of cut it holds, rather than
    /// the distance between passes: adaptive clearing's.
    pub load: bool,
    /// A cutting feed. Without one it cuts at the plunge feed, as a drill does.
    pub feed: bool,
    /// Which stock to leave it cuts with.
    pub stock_to_leave: Leave,
    /// Whether it cuts in levels a stepdown apart.
    pub depths: Depths,
    /// Tabs across the deepest cut of the loops it cuts.
    pub tabs: bool,
    /// How the links it leaves to the linker are made.
    pub links: Links,
    /// Links made without lifting.
    pub stay_down: bool,
    /// Vertical arcs into and out of each pass.
    pub lead_radius: bool,
    /// Ramping into material.
    pub ramp: bool,
    /// The ways it offers to enter each pass, the default helix first: a helix,
    /// a plunge or a ramp. None when it has no choice.
    pub entries: &'static [Entry],
    /// Level arcs into and out of each ring.
    pub leads: bool,
    /// Passes reordered, and maybe reversed, to shorten travel.
    pub reorder: bool,
    /// A Top it cuts down from, and where that is without one.
    pub top: Option<HeightRef>,
    /// A Bottom it cuts down to, and where that is without one.
    pub bottom: Option<HeightRef>,
}

/// How a strategy's passes are linked, where the linker links them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Links {
    /// Staying down over the part: its passes follow the part down, as 3D
    /// finishing's do, and are entered and left along vertical arcs.
    OverPart,
    /// At the level, ramping into material: it cuts level by level.
    Level,
    /// Its own way: it makes the links it stays down for itself, adaptive
    /// clearing's at the level and a drill's up out of each hole, and any the
    /// linker makes are lifted for and go straight down.
    Own,
}

impl Links {
    /// The longest link made without lifting clear, in millimetres, when none
    /// is given, with a tool `diameter` across: 50 mm over the part, 10 × the
    /// tool for adaptive clearing's own links at the level, and 5 × for the
    /// rest.
    pub fn stay_down(self, diameter: f64) -> f64 {
        match self {
            Links::OverPart => 50.0,
            // Adaptive clearing's, where it has cleared the way.
            Links::Own => 10.0 * diameter,
            Links::Level => 5.0 * diameter,
        }
    }
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
