//! A stock's panel: its shape, and its size and place round the models.

use crate::help::Figure;
use crate::help::figures::Margin;

topics! {
    SHAPE: "Shape",
        "Around models grows a box round the parts by margins. A fixed box is a size you give, placed against them.";
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
