//! An operation's Geometry tab: where it may cut, what it cuts round, and the
//! faces it machines and keeps clear of.

use crate::help::Figure;
use crate::help::figures::{Clearance, Facing, LeaveOn};

topics! {
    BOUNDARY: "Boundary",
        "Where the tool may cut, seen from above: the whole stock, around the part, or faces and edges you pick.";
    CONTAINMENT: "Tool",
        "Where the tool sits against the boundary: its centre on it, all of it inside, or reaching in from outside.",
        Figure::Containment;
    ADDITIONAL_OFFSET: "Additional offset",
        "Moves the boundary out, or in when negative. With all of the tool kept inside, positive moves it in.",
        Figure::BoundaryOffset;
    CONTOUR: "Contour",
        "What the tool runs round: the part's outline seen from above, or faces and edges you pick.";
    POCKET: "Pocket",
        "What is cleared: the part's outline seen from above, or faces and edges you pick.";
    SIDE: "Cut",
        "Which side of the contour the tool runs on: outside to cut a part free, inside to cut a hole, on it for a groove.",
        Figure::ProfileSide;
    STOCK_OFFSET: "Stock offset",
        "How far past the stock's edges facing reaches, so the edges are faced clean.",
        Figure::Facing(Facing::StockOffset);
    REST_MACHINING: "Rest machining",
        "Clears only what the operations before it in the setup left: rough with a big tool, then follow with a smaller one.";
    IGNORE_LESS_THAN: "Ignore stock less than",
        "Material the operations before left thinner than this is let be, for a finish to take, rather than chased.";
    FACES: "Faces",
        "Faces picked in the view. Their outline seen from above is what the operation cuts or keeps to.";
    EDGES: "Edges",
        "Edge loops picked in the view. A click takes the whole loop the edge goes round.";
    CURVES: "Edges",
        "Edges picked in the view, open or closed. A click takes the edge and the edges running on smoothly from it.";
    CHAMFER_EDGES: "Edges",
        "Edges to chamfer. A click takes the loop the edge goes round, or where it closes none, the edges running on smoothly from it.";
    CHAMFER_FACES: "Faces",
        "Faces whose edges are all chamfered: the outline and every hole's rim.";
    ENGRAVE_EDGES: "Edges",
        "The rim of each shape to engrave, and of any island in it. A click takes the whole loop, engraved from its own height.";
    ENGRAVE_FACES: "Faces",
        "The floor of each shape to engrave: its outline, islands too, engraved from the top of the walls round it.";
    SURFACES_MODEL: "Model",
        "What is done to every face in no group: machined, or avoided. Roughing and 2D operations take no groups: they machine the whole part.";
    SURFACE_GROUP: "Group",
        "Faces picked in the view, machined or avoided with offsets of their own. A face is in one group: picking it into another moves it there.";
    SURFACE_ACTION: "Action",
        "Machined faces are cut; avoided ones are kept clear of, and on Flat get no passes at all. Avoiding any rules out a V-bit or chamfer mill.";
    EXTRA_WALLS: "Extra on walls",
        "More stock left on these faces' walls, added to the operation's stock to leave.",
        Figure::Leave(LeaveOn::Walls);
    EXTRA_FLOORS: "Extra on floors",
        "More stock left on these faces' floors, added to the operation's stock to leave.",
        Figure::Leave(LeaveOn::Floors);
    CLEARANCE_BESIDE: "Clearance beside",
        "How far the tool keeps from these faces sideways, added to the operation's stock to leave.",
        Figure::Clearance(Clearance::Beside);
    CLEARANCE_ABOVE: "Clearance above",
        "How far above these faces the tool keeps, added to the operation's stock to leave.",
        Figure::Clearance(Clearance::Above);
}
