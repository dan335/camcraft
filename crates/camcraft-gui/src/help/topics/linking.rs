//! An operation's Linking tab: how it moves between cuts.

use crate::help::Figure;
use crate::help::figures::{Helix, Lead};

topics! {
    RETRACT_FEED: "Retract feed",
        "How fast the tool feeds up out of a cut before it rapids away.";
    RETRACTION: "Retraction",
        "How high the tool lifts between cuts: just clear of the material, or all the way to the Retract height.",
        Figure::Retraction;
    SAFE_DISTANCE: "Safe distance",
        "How far above the material the tool changes from rapid to feed on the way down, and back on the way out.",
        Figure::SafeDistance;
    STAY_DOWN: "Stay down",
        "Links this long or shorter are made without lifting. 0 always lifts. Left empty, the strategy's own.",
        Figure::StayDown;
    STAY_DOWN_LEVEL: "Stay-down level",
        "How far round what stands between a link stays down. None goes only straight; most, as far as stay down allows.";
    STAY_DOWN_CLEARANCE: "Stay-down clearance",
        "How far a link that stays down keeps off the material, but where it leaves one pass and comes to the next.",
        Figure::StayDownClearance;
    LIFT_HEIGHT: "Lift height",
        "How far a link that stays down lifts off the floor, so it does not rub. No higher than the level above.",
        Figure::LiftHeight;
    LEAD_RADIUS: "Lead radius",
        "The vertical arcs the tool rolls into and out of each pass on. 0 for none. Left empty, a tenth of the tool.",
        Figure::LeadRadius;
    RAMP_ANGLE: "Ramp angle",
        "How steeply the tool goes down into material. Lower is gentler on the tool but takes longer.",
        Figure::RampAngle;
    RAMP_FEED: "Ramp feed",
        "How fast the tool feeds along a ramp or helix. Left empty, 60% of the cutting feed.";
    MOST_PER_LAP: "Most per lap",
        "The most a helix, or a ramp round a ring, goes down in one lap.",
        Figure::Helix(Helix::PerLap);
    ENTRY: "Entry",
        "How the tool goes down into material to start each ring: a helix, a straight plunge, or a ramp along the ring.",
        Figure::Entry;
    HELIX_DIAMETER: "Helix diameter",
        "The circle the tool's centre follows down the helix. Make it smaller where the waste has no room.",
        Figure::Helix(Helix::Diameter);
    LEADS: "Leads",
        "Arcs the tool comes onto each ring and leaves it along, in the waste, so the wall shows no mark.",
        Figure::Lead(Lead::All);
    LEAD_VERTICAL: "Vertical radius",
        "The arc down onto the lead and up off it. 0 for none.",
        Figure::Lead(Lead::Vertical);
    LEAD_STRAIGHT: "Straight",
        "A straight run before the level arc in, and after the arc out. 0 for none.",
        Figure::Lead(Lead::Straight);
    LEAD_ARC: "Arc radius",
        "The level arc onto the ring. Make it smaller where the waste has no room.",
        Figure::Lead(Lead::Arc);
    LEAD_SWEEP: "Arc sweep",
        "How far round the level arc turns.",
        Figure::Lead(Lead::Sweep);
    LEAD_TO_CENTER: "Lead to center",
        "Leads onto each pass from the hole's centre and back to it, a half turn each way: safer in a small hole.";
    REORDER: "Reorder passes",
        "Cuts passes in the order that travels least between them.";
    REVERSE: "Reverse passes",
        "Lets reordering cut a pass backwards. That turns climb into conventional, so it is off unless asked for.";
    KEEP_TOOL_DOWN: "Keep tool down",
        "Goes from one pass to the next without lifting where it is no further than the stay-down distance.",
        Figure::StayDown;
    LEAD_IN: "Lead-in",
        "Comes down onto each pass along a vertical arc of the lead radius, rather than straight down.",
        Figure::LeadRadius;
    LEAD_OUT: "Lead-out",
        "Goes up off each pass along a vertical arc of the lead radius, rather than straight up.",
        Figure::LeadRadius;
    MOST_PER_LEG: "Maximum ramp stepdown",
        "The most the ramp goes down there and back along a slot; each leg half. Left empty, the stepdown, or a tenth of the tool.",
        Figure::RampAngle;
    RAMP_CLEARANCE: "Ramp clearance height",
        "How far above the Top the ramp down along each slot starts.";
}
