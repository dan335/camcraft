//! An operation's Passes tab, and the preset fields that mean the same.

use crate::help::Figure;
use crate::help::figures::{Facing, LeaveOn, Tab};

topics! {
    PASS_ANGLE: "Pass angle",
        "The direction passes run, seen from above. 0° runs along X, 90° along Y.",
        Figure::PassAngle;
    SHORTEST_RING: "Shortest ring",
        "Rings shorter than this are skipped: too small to be worth entering.";
    MIN_CUTTING_RADIUS: "Minimum cutting radius",
        "The tightest the tool turns in an inside corner. Sharper corners are blended, leaving material in them for a smaller tool.",
        Figure::MinCuttingRadius;
    MACHINE_CAVITIES: "Machine cavities",
        "Goes down a helix into pockets it cannot reach from the side. Unticked, it only cuts from the outside in and leaves them.",
        Figure::Cavities;
    ADAPTIVE_ORDER: "Order",
        "By depth cuts each level everywhere before the next down. By area clears each pocket or area all the way down first.";
    DIRECTION_ADAPTIVE: "Direction",
        "Climb gives the better finish and tool life. Both ways turns at the end of a pass and cuts back conventional, rather than linking round to climb.",
        Figure::BothWays;
    DIRECTION_2D: "Direction",
        "Climb gives the better finish on most machines. Conventional can suit a flexible machine.",
        Figure::Climb;
    DIRECTION_FACE: "Direction",
        "Both ways zigzags across. Climb or conventional cut every pass the same way, lifting back between.";
    DIRECTION_SCALLOP: "Direction",
        "Climb gives the better finish. Both ways keeps rings climbing and starts each open pass at its nearer end, for less linking.",
        Figure::Climb;
    SCALLOP_ORDER: "Order",
        "Outside in cuts from the boundary inward. Inside out cuts each area from its middle, ending on the boundary.";
    SLOPES: "Slopes",
        "Only finishes where the surface is between these angles from level, both included: 0° is flat, 90° a wall. Scallop leaves faces steeper than a plunge.";
    CONTOUR_ORDER: "Order",
        "By depth cuts each level all the way round before the next down. By area cuts each piece of the part all the way down first.";
    DIRECTION_SPIRAL: "Direction",
        "Which way it turns: climb gives the better finish, conventional can suit a flexible machine.",
        Figure::Climb;
    SPIRAL_ORDER: "Order",
        "Outside in ends at the centre. Inside out starts at the centre and ends on the outside.";
    CENTRE: "Centre",
        "Where the pattern turns about, from the work zero. Left empty, the middle of the boundary.";
    MORPH_CENTRE: "Centre",
        "Where the spiral turns into, from the work zero. Left empty, the point furthest inside the boundary.";
    INNER_RADIUS: "Inner radius",
        "Nothing is cut nearer the centre than this. Spokes crowd together there, so a little saves recutting the middle.";
    OUTER_RADIUS: "Outer radius",
        "Nothing is cut further out than this. Left empty, the pattern reaches all of the boundary.";
    RADIAL_ANGLES: "Angles",
        "Spokes are laid from the first angle round to the second, anticlockwise from X. 0° to 360° goes all the way round.";
    RADIAL_DIRECTION: "Direction",
        "Both ways cuts out along one spoke and back in along the next. Outward or inward cuts every spoke one way.";
    FLAT_AREAS: "Flat areas",
        "Adds a level at each flat face and peak, so floors are left just the stock to leave, not a stepdown's terrace.",
        Figure::FlatAreas;
    MIN_FLAT_STEPDOWN: "Minimum stepdown",
        "A flat or peak less than this below the level above gets no level of its own: that much is left on it. Empty for none.";
    MIN_AXIAL_ENGAGEMENT: "Minimum axial engagement",
        "Fine and shallow levels cutting less depth than this are left out, so the flutes never just skim. Empty for none.";
    SHALLOW_AREAS: "Shallow areas",
        "Adds levels between the stepdowns where the slope is shallow and their terraces wide, cutting only there.",
        Figure::ShallowAreas;
    MIN_SHALLOW_STEPDOWN: "Minimum shallow stepdown",
        "The closest an added level comes to another. It wins over the maximum stepover.",
        Figure::ShallowAreas;
    MAX_SHALLOW_STEPOVER: "Maximum shallow stepover",
        "Where a terrace between two levels is wider than this, a level is added halfway, then again, until none is.",
        Figure::ShallowAreas;
    SLOT_CLEARING: "Slot clearing",
        "A pocket gone down into is first cut a slot along its middle, then cleared out from it.",
        Figure::SlotClearing;
    SLOT_WIDTH: "Slot width",
        "How wide that slot is cut. Empty, or less than the tool, the tool's own width.",
        Figure::SlotClearing;
    PASS_EXTENSION: "Pass extension",
        "How far each pass runs on past the faced area before it turns.",
        Figure::Facing(Facing::PassExtension);
    PECK: "Peck",
        "How deep to drill before backing out to clear chips. Left empty, each hole is drilled in one go.",
        Figure::Peck;
    DWELL: "Dwell",
        "Seconds to pause at the bottom of each hole. Left empty, no pause.";
    STEPOVER: "Stepover",
        "The distance between side-by-side passes. Smaller leaves a smoother finish but takes longer.",
        Figure::Stepover;
    OPTIMAL_LOAD: "Optimal load",
        "The width of cut each pass is steered to hold, so the tool never slots. A tenth of the diameter is a safe start, with a deep stepdown.",
        Figure::OptimalLoad;
    DEEPEST_STEP: "Deepest step cut",
        "The tallest near-vertical drop the tool may cut straight down at the cutting feed. Taller ones lift, and plunge at the plunge feed.",
        Figure::CutStep;
    MULTIPLE_DEPTHS: "Multiple depths",
        "Cuts down in several levels. Unticked, the whole depth is cut in one pass.",
        Figure::Stepdown;
    STEPDOWN: "Stepdown",
        "The deepest each level may be. Levels are spaced evenly down to the Bottom.",
        Figure::Stepdown;
    FINE_STEPDOWN: "Fine stepdown",
        "Extra levels this far apart between the stepdowns, each cutting only what was left: a slope's terraces come down to steps this tall.",
        Figure::FineStepdown;
    STOCK_TO_LEAVE: "Stock to leave",
        "Leaves a skin of material for a finishing pass to clean up.",
        Figure::Leave(LeaveOn::Both);
    WALLS: "Walls",
        "Material left on walls for a finishing pass. Negative cuts past them.",
        Figure::Leave(LeaveOn::Walls);
    FLOORS: "Floors",
        "Material left on floors. It follows the walls until given its own. Negative cuts below them.",
        Figure::Leave(LeaveOn::Floors);
    TABS: "Tabs",
        "Leaves small bridges across the deepest cut, so a part cut free stays held in the sheet.",
        Figure::Tab(Tab::All);
    TAB_SHAPE: "Shape",
        "Rectangular tabs are square-sided. Triangular ones are ramped over, so the tool never plunges.",
        Figure::Tab(Tab::Shape);
    TAB_WIDTH: "Width",
        "How long each tab is along the cut, at its top.",
        Figure::Tab(Tab::Width);
    TAB_HEIGHT: "Height",
        "How far each tab's top stands above the bottom of the cut.",
        Figure::Tab(Tab::Height);
    TAB_SPACING: "Spacing",
        "About how far apart tabs are round each loop. There are always at least two to a loop.",
        Figure::Tab(Tab::Spacing);
    FINISHING: "Finishing passes",
        "Passes round the pocket's walls with the side of the tool, once each level is cleared. The clearing stops short of the walls to leave them their stepovers.";
    FINISHING_PASSES: "Passes",
        "How many finishing passes, each a stepover nearer the wall, the last at it.";
    FINISHING_STEPOVER: "Stepover",
        "The distance between finishing passes, and the most the last takes off the wall. Left empty, a tenth of the tool.";
    FINISHING_FEED: "Finishing feed",
        "How fast the last finishing pass goes. Left empty, the cutting feed.";
    FINISHING_REPEAT: "Repeat",
        "Cuts the last finishing pass twice, for a tool that springs away from the wall.";
}
