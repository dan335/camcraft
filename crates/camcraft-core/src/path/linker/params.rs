//! What linking is asked to do, and what it reports.

use glam::DVec2;

use crate::path::toolpath::ToolpathError;

/// When a link is made without lifting clear.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StayDown {
    /// Never: every link lifts clear.
    Never,
    /// A link up to `max` long, riding over the part: the straight line between
    /// the two cuts, lifted wherever the part rises above it. For finishing, which
    /// runs on the part.
    OverPart {
        /// The longest link made this way, in millimetres.
        max: f64,
    },
    /// A link up to `max` long between two cuts at the same height, straight and
    /// level, where the part does not reach up into it. For clearing at a level.
    Level {
        /// The longest link made this way, in millimetres.
        max: f64,
    },
}

/// How a pass that starts in material is entered, where [`LinkParams::ramp`]
/// gives a ramp angle and feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// Down along the pass itself, in the direction it cuts; a ring with leads
    /// is entered at its start, without its lead.
    Ramp,
    /// Down a helix beside where the pass starts, then along the vertical lead,
    /// if there is one; straight down where there is no room for one.
    Helix,
    /// Straight down at the plunge feed onto the vertical lead, or the pass.
    Plunge,
}

/// Ramping into material along the pass about to be cut.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ramp {
    /// How steeply the ramp descends, in radians from level.
    pub angle: f64,
    /// Feed rate along it, in mm/min.
    pub feed: f64,
}

/// How to resolve a link.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinkParams {
    /// Feed rate for coming up out of a cut, in mm/min.
    pub retract_feed: f64,
    /// Feed rate for descending into material, in mm/min.
    pub plunge_feed: f64,
    /// Tool radius, passed to the clearance source.
    pub tool_radius: f64,
    /// How far above the material the tool stops rapiding down and feeds, and up
    /// to which it feeds on the way out, in millimetres.
    pub safe_distance: f64,
    /// When a link stays down.
    pub stay_down: StayDown,
    /// The radius of the vertical arcs a pass is entered and left along, in
    /// millimetres; zero for none.
    pub lead_radius: f64,
    /// Ramping into material, where a pass starts in it; `None` plunges.
    pub ramp: Option<Ramp>,
    /// How a pass that starts in material is entered, with a ramp.
    pub entry: EntryKind,
    /// The radius of a helical entry, as the tool's centre goes, in millimetres.
    pub helix_radius: f64,
    /// The smallest a helical entry is made where there is no room for one of
    /// [`LinkParams::helix_radius`], in millimetres.
    pub helix_min_radius: f64,
    /// Whether a pass with no room for a helix is ramped into along itself, as
    /// Fusion's helix ramp type allows a pocket, rather than plunged.
    pub ramp_where_no_helix: bool,
    /// Whether a helix turns counter-clockwise seen from above, which climbs
    /// with a clockwise spindle.
    pub helix_ccw: bool,
    /// The most a helix, or a ramp round a closed pass, descends in one lap, in
    /// millimetres; `None` for no limit but the ramp angle.
    pub ramp_stepdown: Option<f64>,
    /// Whether the vertical lead may go down through material, from a plunge
    /// onto its top, as a 2D profile's does; otherwise it is left out wherever
    /// its top is in material, as a finishing pass's is.
    pub lead_in_material: bool,
    /// Deepest a descent at cutting feed may fall steeply, in millimetres: a
    /// stay-down link that would fall further lifts clear instead.
    pub max_cut_step: f64,
    /// Chordal tolerance for the arcs, in millimetres.
    pub tolerance: f64,
    /// The height the tool goes up to after the last cut, if any.
    pub end_z: Option<f64>,
    /// The height the tool arrives at, above the first pass, before it goes down
    /// to where it starts feeding, if any: the clearance height.
    pub start_z: Option<f64>,
}

impl LinkParams {
    /// Lift clear for every link and plunge straight in: nothing but the feeds and
    /// the tool, the rest at its plainest.
    pub fn plain(retract_feed: f64, plunge_feed: f64, tool_radius: f64) -> Self {
        Self {
            retract_feed,
            plunge_feed,
            tool_radius,
            safe_distance: 2.0,
            stay_down: StayDown::Never,
            lead_radius: 0.0,
            ramp: None,
            entry: EntryKind::Ramp,
            helix_radius: 0.0,
            helix_min_radius: 0.0,
            ramp_where_no_helix: false,
            helix_ccw: true,
            ramp_stepdown: None,
            lead_in_material: false,
            max_cut_step: 0.5,
            tolerance: 0.01,
            end_z: None,
            start_z: None,
        }
    }
}

/// Linking could not proceed.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum LinkError {
    /// A parameter was outside its usable range.
    #[error("{name} must be {must}, got {value}")]
    BadParameter {
        /// Which parameter.
        name: &'static str,
        /// What it must be.
        must: &'static str,
        /// What it was.
        value: f64,
    },
    /// The clearance source returned a height that is not usable.
    #[error("clearance height is not finite at {at:?}")]
    BadClearance {
        /// Where it was asked about.
        at: DVec2,
    },
    /// A move could not be added.
    #[error(transparent)]
    Toolpath(#[from] ToolpathError),
}

/// What linking did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LinkReport {
    /// How many links were resolved.
    pub links_resolved: u64,
    /// How many of them stayed down.
    pub stayed_down: u64,
    /// How many passes were ramped into.
    pub ramps: u64,
    /// How many passes were entered down a helix.
    pub helixes: u64,
    /// How many plunges start at or below the surface they descend into.
    ///
    /// Any clearance no higher than where a plunge ends makes one: a plane
    /// below the work as much as a field.
    pub suspicious_plunges: u64,
}
