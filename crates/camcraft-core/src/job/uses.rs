//! Which of the settings every operation has its strategy uses.

use super::{Compensation, Entry, HeightRef, ProfileSide, StrategySpec};

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
            holes: None,
            curves: None,
            stepover: true,
            load: false,
            feed: true,
            stock_to_leave: Leave::Both,
            depths: Depths::Optional,
            fine_stepdown: false,
            tabs: false,
            finishing: false,
            links: Links::Level,
            stay_down: true,
            cut_step: true,
            reach: Reach::Tools(5),
            helix: HelixSize::Tool,
            lead_radius: false,
            ramp: true,
            entries: &[],
            leads: false,
            // Only parallel's passes are in no order of their own.
            reorder: false,
            top: Some(HeightRef::StockTop),
            bottom: Some(HeightRef::ModelBottom),
            rest: false,
            smoothing: true,
            feed_optimization: true,
        };
        match *self {
            // Down the surface in one pass, as Fusion's 3D finishing: it
            // follows the part all the way down, but for what of it is above a
            // Top or below a Bottom given it, which is trimmed away
            // (`path::trim`). Without them, it is all kept: the part's top and
            // bottom are only where the fields start from.
            StrategySpec::Parallel { .. } => Uses {
                depths: Depths::None,
                links: Links::OverPart,
                reach: Reach::Mm(50),
                lead_radius: true,
                // Into material standing over where a pass starts, such as a
                // roughing's terraces, down along the pass rather than
                // straight down, never below the pass itself; in the air, as
                // before, down its vertical lead.
                ramp: true,
                reorder: true,
                top: Some(HeightRef::ModelTop),
                bottom: Some(HeightRef::ModelBottom),
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
                // Fusion's: scallop's rings are closer than a pattern's laps.
                reach: if matches!(self, StrategySpec::Scallop { .. }) {
                    Reach::Tools(2)
                } else {
                    Reach::Tools(5)
                },
                lead_radius: true,
                // Ramped into material as parallel is.
                ramp: true,
                // Trimmed to them as parallel is.
                top: Some(HeightRef::ModelTop),
                bottom: Some(HeightRef::ModelBottom),
                ..cuts_loops
            },
            // A level each stepdown is what it is, going round, not over: it
            // has no stepover. Its order is its own. Its levels are the
            // part's, from its top, and each runs round the part beside its
            // walls, below where a tool from above would rest: linked at the
            // level, never over the part, whose way up from a wall's foot is
            // through the wall. Its ramp is its own, down the wall from level
            // to level (`waterline`); it comes down onto each level's lead
            // beside the wall, as before.
            StrategySpec::Contour { .. } => Uses {
                stepover: false,
                depths: Depths::Required,
                links: Links::Level,
                reach: Reach::Mm(50),
                helix: HelixSize::Flat,
                lead_radius: true,
                ramp: true,
                top: Some(HeightRef::ModelTop),
                ..cuts_loops
            },
            // Contour's rings, gone down wall by wall: no stepover, its
            // stepdown how far down each time round, its order and its way
            // down its own (`strategy::ramp`). Each wall starts above where
            // the last ended, and Fusion's programs lift between every two,
            // to the retract height, and come down onto each along a
            // vertical arc. Its levels start from the stock's top, as
            // Fusion's Top does.
            StrategySpec::Ramp { .. } => Uses {
                stepover: false,
                depths: Depths::Required,
                links: Links::Level,
                stay_down: false,
                reach: Reach::Mm(50),
                helix: HelixSize::Flat,
                lead_radius: true,
                ramp: true,
                ..cuts_loops
            },
            // Each flat at its own height, once: rings in from its edges, in
            // their own order, as a pocket clearing level's are. Only the
            // part's flats, from its top.
            StrategySpec::Flat { .. } => Uses {
                depths: Depths::None,
                reach: Reach::Tools(2),
                top: Some(HeightRef::ModelTop),
                ..cuts_loops
            },
            // Slice by slice is what it is. Its order is its own, each level's
            // rings from the middle out, and each the way it cuts. Into
            // material it goes down a helix, as Fusion's 3D pocket does by
            // default, or ramps along its ring, or plunges.
            // It clears what the operations before it left, ring by ring
            // round it, so it waits for them to be cut.
            StrategySpec::PocketClearing { .. } => Uses {
                rest: true,
                depths: Depths::Required,
                entries: &[Entry::Helix, Entry::Ramp, Entry::Plunge],
                helix: HelixSize::Flat,
                ..cuts_loops
            },
            // Its stepover is its optimal load. It goes down a helix, or
            // straight, into what it cannot reach from the side, as Fusion's
            // 3D Adaptive's ramp types (its Predrill aside): it has no ring to
            // ramp along. Its passes are in the order it steers them, linked at
            // the level where it has cleared the way.
            // Every pass of its stays on its level, so it has no drop of its own
            // to cut down: its links and the check have the default deepest
            // step cut ([`Operation::max_cut_step`](super::Operation::max_cut_step)).
            // With rest machining, it clears what the operations before it
            // left, so it waits for them to be cut.
            StrategySpec::Adaptive { rest_machining, .. } => Uses {
                load: true,
                cut_step: false,
                rest: rest_machining,
                depths: Depths::Required,
                // Fusion's 3D Adaptive's, between its levels.
                fine_stepdown: true,
                links: Links::Own,
                helix: HelixSize::Flat,
                entries: &[Entry::Helix, Entry::Plunge],
                // It holds the load at every step, round its tight loops too:
                // a corner of its never loads the tool more than its walls,
                // so slowing there would only lose time.
                feed_optimization: false,
                ..cuts_loops
            },
            // A 2D operation's order is its own: what is inside before what frees
            // it, a pocket's middle before its walls.
            StrategySpec::Pocket { .. } => Uses {
                boundary: false,
                outline: true,
                finishing: true,
                entries: &[Entry::Helix, Entry::Ramp, Entry::Plunge],
                // Fusion's 2D pocket's helix is its 3D strategies'.
                helix: HelixSize::Flat,
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
                holes: Some(Holes::Rims),
                curves: None,
                stepover: false,
                load: false,
                // Each hole is fed down at the plunge feed.
                feed: false,
                stock_to_leave: Leave::None,
                // Its pecks are its own.
                depths: Depths::None,
                fine_stepdown: false,
                tabs: false,
                finishing: false,
                // Up out of each hole, and across to the next.
                links: Links::Own,
                stay_down: false,
                cut_step: true,
                reach: Reach::Tools(0),
                helix: HelixSize::Tool,
                lead_radius: false,
                ramp: false,
                entries: &[],
                leads: false,
                // Holes are drilled in the order found, nearest first.
                reorder: false,
                // Each from its rim to its own bottom, its bottom edge, or the
                // Bottom where one is set; the part's bottom where no wall is found.
                top: None,
                bottom: Some(HeightRef::ModelBottom),
                rest: false,
                // A hole is straight down and up.
                smoothing: false,
                feed_optimization: false,
            },
            // Its picks are round walls, each hole or boss cut round its own
            // circle from its own top to its own bottom: no area, no levels,
            // its helix its own. In and out level along its leads, down and
            // up the vertical arcs the linker makes; lifted between passes,
            // each of which starts at the top.
            StrategySpec::Bore { .. } => Uses {
                boundary: false,
                holes: Some(Holes::Walls),
                depths: Depths::None,
                finishing: true,
                links: Links::Level,
                stay_down: false,
                cut_step: false,
                reach: Reach::Tools(0),
                ramp: false,
                leads: true,
                top: Some(HeightRef::ModelTop),
                bottom: Some(HeightRef::ModelBottom),
                // Its moves are arcs already, and every one goes round the
                // same circle.
                smoothing: false,
                feed_optimization: false,
                ..cuts_loops
            },
            // As a bore, but level by level, as Fusion's Circular: every pass
            // at every level comes down its vertical lead, into the material
            // where the level is still there, and lifts out after, never
            // ramping and never staying down, as Fusion's programs do.
            StrategySpec::Circular { .. } => Uses {
                boundary: false,
                holes: Some(Holes::Walls),
                stay_down: false,
                cut_step: false,
                reach: Reach::Tools(0),
                ramp: false,
                leads: true,
                top: Some(HeightRef::ModelTop),
                bottom: Some(HeightRef::ModelBottom),
                smoothing: false,
                feed_optimization: false,
                ..cuts_loops
            },
            // Its picks are round walls, as a bore's, each hole threaded from
            // its own bottom to its own top, or top to bottom: no levels, no
            // stock to leave, as Fusion's Thread has none with its Stock to
            // Leave off. Its stepover is between its passes out to the
            // thread. Each pass starts at the hole's centre, down at the
            // bottom: the linker comes down there and lifts out after, to the
            // retract height between holes, as Fusion's programs do.
            StrategySpec::Thread { .. } => Uses {
                boundary: false,
                holes: Some(Holes::Walls),
                stock_to_leave: Leave::None,
                depths: Depths::None,
                stay_down: false,
                cut_step: false,
                reach: Reach::Tools(0),
                ramp: false,
                leads: true,
                top: Some(HeightRef::ModelTop),
                bottom: Some(HeightRef::ModelBottom),
                smoothing: false,
                feed_optimization: false,
                ..cuts_loops
            },
            // Its picks are the edges round each slot's floor, each slot cut
            // along its middle from the stock's top down to its own floor:
            // no area, no stepover, only its floor to leave anything on. Its
            // ramp is its own, back and forth along the slot, as Fusion's
            // Slot's profile ramp: each slot is one pass. Between slots it
            // stays down within twice the tool, as Fusion's.
            StrategySpec::Slot { .. } => Uses {
                boundary: false,
                curves: Some(Curves::Slots),
                stepover: false,
                stock_to_leave: Leave::Axial,
                cut_step: false,
                reach: Reach::Tools(2),
                top: Some(HeightRef::StockTop),
                bottom: Some(HeightRef::Selection),
                ..cuts_loops
            },
            // Its picks are edges, open or closed, each followed at its own
            // height: no area, no stepover, its levels above the curve, its
            // ramp none, as Fusion's Trace plunges. Entered and left along
            // vertical arcs where asked; between passes it lifts, unless it
            // keeps the tool down, over the part as a finishing pass does.
            // Its stock to leave is beside it only with compensation.
            StrategySpec::Trace { compensation, .. } => Uses {
                boundary: false,
                curves: Some(Curves::Traced),
                stepover: false,
                stock_to_leave: if compensation == Compensation::Center {
                    Leave::Axial
                } else {
                    Leave::Both
                },
                links: Links::OverPart,
                cut_step: false,
                reach: Reach::Mm(50),
                lead_radius: true,
                ramp: false,
                top: None,
                bottom: None,
                ..cuts_loops
            },
            // Its picks are the edges it chamfers, or the faces whose edges
            // it chamfers, each at its own height: no area, no stepover, no
            // levels, nothing to leave, as Fusion's 2D Chamfer has none.
            // Onto each pass along a straight from the waste and off it so,
            // Fusion's lead; between them it lifts, as Fusion's keeps no tool
            // down, over the part as a finishing pass does.
            StrategySpec::Chamfer2d { .. } => Uses {
                boundary: false,
                curves: Some(Curves::Chamfered),
                stepover: false,
                stock_to_leave: Leave::None,
                depths: Depths::None,
                links: Links::OverPart,
                stay_down: false,
                cut_step: false,
                reach: Reach::Mm(0),
                ramp: false,
                top: None,
                bottom: None,
                ..cuts_loops
            },
            // Its picks are the shapes it engraves, by the edges round each or
            // its floor: no area to keep to, no stepover, nothing to leave, as
            // Fusion's Engrave has none; each shape from its own top down to
            // its own depth, its levels a stepdown apart where asked. Each
            // walk starts at a corner at the surface and goes round without
            // lifting, down through its levels; between walks it lifts, as
            // Fusion's does between shapes in every program of its compared:
            // staying down from one shape to the next would drag the point
            // across the surface between them.
            StrategySpec::Engrave { .. } => Uses {
                boundary: false,
                curves: Some(Curves::Engraved),
                stepover: false,
                stock_to_leave: Leave::None,
                stay_down: false,
                cut_step: false,
                ramp: false,
                top: None,
                bottom: None,
                // Fusion's has no feed optimisation.
                feed_optimization: false,
                ..cuts_loops
            },
            // A 2D pocket's areas and levels, each level cleared as adaptive
            // clearing clears one: its stepover is its optimal load, it goes
            // down a helix or straight into a closed pocket and from outside
            // into an open one, and its passes are in the order it steers
            // them, linked at the level where it has cleared the way. Its
            // levels are a 2D pocket's, down to each floor in one unless
            // asked; it has no flats to find and no fine stepdown.
            StrategySpec::Adaptive2d { .. } => Uses {
                boundary: false,
                outline: true,
                load: true,
                cut_step: false,
                links: Links::Own,
                helix: HelixSize::Flat,
                entries: &[Entry::Helix, Entry::Plunge],
                // It holds the load round its corners, as adaptive does.
                feed_optimization: false,
                ..cuts_loops
            },
        }
    }
}

/// What a strategy that runs along what is picked, rather than over an area,
/// has picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curves {
    /// Slots, by the edges round their floors, or their floors: a slot's
    /// middle is what it runs along, as Fusion's Slot has them.
    Slots,
    /// Edges, open or closed, the tool follows, as Fusion's Trace has them.
    Traced,
    /// Edges to chamfer, or faces whose edges to chamfer, as Fusion's 2D
    /// Chamfer has them: its chains or its face contours.
    Chamfered,
    /// Shapes to engrave, by the edges round each, whole loops, or by each
    /// one's floor, whose outline it is: Fusion's Engrave's contours.
    Engraved,
}

/// How a strategy that cuts holes has them picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holes {
    /// By their rims, the round edges at their tops, as drilling has them.
    Rims,
    /// By their round walls, faces, as Fusion's Bore, Circular and Thread
    /// have them: a hole's or a boss's.
    Walls,
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
    /// Holes: its picks are the holes it cuts, not an area, and no height is
    /// measured from them, since each hole goes to its own bottom. How they
    /// are picked, where it has them.
    pub holes: Option<Holes>,
    /// Curves: its picks are what it runs along, slots or edges, not an area.
    /// Which, where it has them.
    pub curves: Option<Curves>,
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
    /// Finer levels between those, where they leave terraces: Fusion's fine
    /// stepdown ([`Passes::fine_stepdown`](super::Passes::fine_stepdown)).
    pub fine_stepdown: bool,
    /// Tabs across the deepest cut of the loops it cuts.
    pub tabs: bool,
    /// Finishing passes along a pocket's walls, as Fusion's 2D pocket has.
    pub finishing: bool,
    /// How the links it leaves to the linker are made.
    pub links: Links,
    /// Links made without lifting.
    pub stay_down: bool,
    /// A deepest step cut of its own
    /// ([`Passes::max_cut_step`](super::Passes::max_cut_step)): the tallest
    /// near-vertical drop it cuts down at the cutting feed. Without one, the
    /// default ([`Operation::max_cut_step`](super::Operation::max_cut_step)).
    pub cut_step: bool,
    /// The longest of them when not given: Fusion's Maximum Stay-Down
    /// Distance for the strategy.
    pub reach: Reach,
    /// How wide its helical entries are when not given.
    pub helix: HelixSize,
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
    /// A Top it cuts down from, and where that is without one. A strategy
    /// linked over the part ([`Links::OverPart`]) has it only when given: its
    /// passes are trimmed to it, and without one nothing is.
    pub top: Option<HeightRef>,
    /// A Bottom it cuts down to, and where that is without one; as `top`, for
    /// one linked over the part.
    pub bottom: Option<HeightRef>,
    /// Its passes are worked out from the material the operations before it
    /// left, so they wait for those to be cut; every other strategy's are
    /// worked out from the part alone, at once with the others'. Linking and
    /// cutting are always in order.
    pub rest: bool,
    /// Its passes may be smoothed, fitted with arcs and longer lines
    /// ([`Passes::smoothing`](super::Passes::smoothing)).
    pub smoothing: bool,
    /// Its cuts may be slowed at their corners
    /// ([`Operation::feed_optimization`](super::Operation::feed_optimization)).
    pub feed_optimization: bool,
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

/// The longest link a strategy makes without lifting when none is given, as
/// Fusion's Maximum Stay-Down Distance for it: so many millimetres, or so many
/// of the tool's diameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Millimetres.
    Mm(u32),
    /// The tool's diameters.
    Tools(u32),
}

impl Reach {
    /// In millimetres, with a tool `diameter` across.
    pub fn of(self, diameter: f64) -> f64 {
        match self {
            Reach::Mm(mm) => f64::from(mm),
            Reach::Tools(k) => f64::from(k) * diameter,
        }
    }
}

/// How wide a strategy's helical entries are when not given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelixSize {
    /// 0.9 × the tool's diameter, camcraft's for a 2D strategy, down to a
    /// quarter of that where it has no room.
    Tool,
    /// Fusion's 3D strategies' Helical Ramp Diameter: 0.95 × the tool's
    /// diameter less its corners, no less than a quarter of the tool; down to
    /// its Minimum Ramp Diameter, half the tool less its corners, no less than
    /// a quarter of the tool, where it has no room.
    Flat,
}

impl HelixSize {
    /// The helix's diameter, as the tool's centre goes, for a tool `diameter`
    /// across with corners of radius `corner`.
    pub fn diameter(self, diameter: f64, corner: f64) -> f64 {
        match self {
            HelixSize::Tool => 0.9 * diameter,
            HelixSize::Flat => (0.95 * (diameter - 2.0 * corner)).max(0.25 * diameter),
        }
    }

    /// The smallest a helix of `largest` across may be made where there is no
    /// room for it, for the same tool.
    pub fn least(self, largest: f64, diameter: f64, corner: f64) -> f64 {
        match self {
            HelixSize::Tool => 0.25 * largest,
            HelixSize::Flat => (0.5 * (diameter - 2.0 * corner))
                .max(0.25 * diameter)
                .min(largest),
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
