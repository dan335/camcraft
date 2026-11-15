//! A stock's panel: its shape, and its size and place round the models; for
//! a sheet, how it lays its parts out.

use crate::help::Figure;
use crate::help::figures::{Gap, Margin, Packing};

topics! {
    SHAPE: "Shape",
        "Around models grows a box round the parts by margins. A fixed box is a size you give, placed against them. A sheet lays its parts out on itself.";
    SHEET_SIZE: "Size",
        "The sheet's length along X, width along Y and thickness, as bought.";
    CORNER: "Corner",
        "Where the sheet's front-left bottom corner is, whatever its parts.";
    SPACING: "Spacing",
        "The least gap between two parts, seen from above. Leave at least the cutter's width to cut between them.",
        Figure::SheetGap(Gap::Spacing);
    BORDER: "Border",
        "The least gap between a part and the sheet's edge, seen from above.",
        Figure::SheetGap(Gap::Border);
    ROTATION: "Rotation",
        "The turns about Z a part may be laid at to fit. None keeps every part as it stands, for grain that must run one way.";
    STEP: "Step",
        "A part may be turned by any whole number of this many degrees.";
    FROM: "Pack from",
        "The corner parts are laid from first, seen from above: the front is towards −Y, the left towards −X.",
        Figure::Packing(Packing::From);
    ALONG: "Along",
        "The direction the sheet is used up in: parts fill across it first, so an offcut is left whole at the far end.",
        Figure::Packing(Packing::Along);
    SEAT: "Seat",
        "Bottom stands each part on the sheet's bottom face, to be cut out of it. Top puts its highest point on the sheet's top.";
    PARTS: "Parts",
        "The models the sheet lays out, each where it fits. Its sheet places a part, so it keeps no position of its own. A model goes on one sheet only.";
    COPIES: "Copies",
        "How many of this part the sheet lays. What is picked on one copy is picked on every copy. Copies with no room are left out; the rest are cut.";
    GRAIN: "Grain",
        "The turns this part may take instead of the sheet's. None, or a half turn, keeps its grain running the way the part is drawn.";
    GRAIN_STEP: "Grain step",
        "This part's copies may be turned by any whole number of this many degrees.";
    SIZE: "Size",
        "How long the block is along each axis.";
    POSITION: "Position",
        "Where the block sits along each axis: centred on the models, past one side of them, or at a coordinate.";
    OFFSET: "Offset",
        "How far the block's side is past the models', or the coordinate its low side is at.";
    MARGIN_X: "X margin",
        "Material beyond the models on each side along X.",
        Figure::Margin(Margin::X);
    MARGIN_Y: "Y margin",
        "Material beyond the models on each side along Y.",
        Figure::Margin(Margin::Y);
    MARGIN_TOP: "Top margin",
        "Material above the models' top, for facing off.",
        Figure::Margin(Margin::Top);
    MARGIN_UNDER: "Margin underneath",
        "Material below the models' bottom, so cutting through leaves a clean edge.",
        Figure::Margin(Margin::Under);
}
