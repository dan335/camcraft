//! An operation's Geometry tab: where it may cut, and what it cuts round.

use crate::help::Figure;
use crate::help::figures::Facing;

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
    FACES: "Faces",
        "Faces picked in the view. Their outline seen from above is what the operation cuts or keeps to.";
    EDGES: "Edges",
        "Edge loops picked in the view. A click takes the whole loop the edge goes round.";
}
