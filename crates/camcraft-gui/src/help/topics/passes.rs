//! An operation's Passes tab, and the preset fields that mean the same.

use crate::help::Figure;
use crate::help::figures::{Facing, LeaveOn, Tab};

topics! {
    PASS_ANGLE: "Pass angle",
        "The direction passes run, seen from above. 0° runs along X, 90° along Y.",
        Figure::PassAngle;
    SHORTEST_RING: "Shortest ring",
        "Rings shorter than this are skipped: too small to be worth entering.";
    DIRECTION_ADAPTIVE: "Direction",
        "Climb gives the better finish and tool life. Both ways cuts in either direction, for less linking.",
        Figure::Climb;
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
        "Adds a level at each flat face, so floors are left just the stock to leave, not a stepdown's terrace.";
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
        "How wide a cut each pass is steered to hold. About a tenth of the tool's diameter is a safe start.",
        Figure::OptimalLoad;
    DEEPEST_STEP: "Deepest step cut",
        "The biggest drop the tool may take while it stays down and cuts. A link that would fall further lifts clear.";
    MULTIPLE_DEPTHS: "Multiple depths",
        "Cuts down in several levels. Unticked, the whole depth is cut in one pass.",
        Figure::Stepdown;
    STEPDOWN: "Stepdown",
        "The deepest each level may be. Levels are spaced evenly down to the Bottom.",
        Figure::Stepdown;
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
}
