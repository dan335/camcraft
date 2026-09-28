//! The tool library: a tool's description, shape, holder, speeds and post.

use crate::help::Figure;
use crate::help::figures::ToolDim;

topics! {
    DESCRIPTION: "Description",
        "A name for the tool, shown wherever tools are listed.";
    VENDOR: "Vendor",
        "Who makes the tool.";
    PRODUCT: "Product",
        "The maker's part number. An import uses it to find a tool the library already has.";
    MATERIAL: "Material",
        "What the cutter is made of. For your records: speeds and feeds come from the presets.";
    KIND: "Type",
        "The cutter's shape: a flat, ball or bullnose end mill, a V-bit or chamfer mill, or a drill.",
        Figure::ToolKinds;
    DIAMETER: "Diameter",
        "The diameter the tool cuts.",
        Figure::ToolDim(ToolDim::Diameter);
    CORNER: "Corner radius",
        "The rounding at a bullnose's corners.",
        Figure::ToolDim(ToolDim::Corner);
    POINT_ANGLE: "Point angle",
        "The included angle of the drill's point. 118° is the most common.",
        Figure::ToolDim(ToolDim::Angle);
    ANGLE: "Angle",
        "The V's included angle: a 90° bit cuts a 45° chamfer.",
        Figure::ToolDim(ToolDim::Angle);
    TIP_DIAMETER: "Tip diameter",
        "The width of the flat at the V's tip. 0 for a sharp point.",
        Figure::ToolDim(ToolDim::Tip);
    FLUTES: "Flutes",
        "How many cutting edges the tool has. Chip load needs it.";
    FLUTE_LENGTH: "Flute length",
        "How much of the tool cuts, from the tip.",
        Figure::ToolDim(ToolDim::Flutes);
    SHOULDER: "Shoulder length",
        "From the tip to where the tool reaches its full shank diameter.",
        Figure::ToolDim(ToolDim::Shoulder);
    SHANK: "Shank diameter",
        "The diameter of the plain part the collet holds.",
        Figure::ToolDim(ToolDim::Shank);
    STICKOUT: "Stickout",
        "How far the tool reaches out of the holder. Shorter is stiffer and chatters less.",
        Figure::ToolDim(ToolDim::Stickout);
    OVERALL: "Overall length",
        "The whole tool, from its tip to the end of its shank.",
        Figure::ToolDim(ToolDim::Overall);
    HOLDER: "Holder",
        "What holds the tool, such as an ER20 collet, as segments from the collet up.",
        Figure::ToolDim(ToolDim::Holder);
    HOLDER_SEGMENT: "Holder segment",
        "One step of the holder: its diameter (Ø) and length (L), counted from the collet up.",
        Figure::ToolDim(ToolDim::Holder);
    TOOL_SPINDLE: "Spindle",
        "The speed operations with this tool turn at, unless their preset gives one.";
    NUMBER: "Tool number",
        "The T number the program calls the tool by. Each tool in a job needs its own.";
    SPINDLE_TURNS: "Spindle turns",
        "Which way the spindle runs: clockwise (M3) for almost every cutter, counter-clockwise (M4) for a left-hand one.";
    COOLANT: "Coolant",
        "The coolant the program turns on for this tool: off, flood (M8) or mist (M7).";
    PRESET_NAME: "Name",
        "What the preset is called, such as Roughing or MDF.";
    NOTE: "Note",
        "Anything worth remembering about the preset, such as where its numbers came from.";
    PRESET_SPINDLE: "Spindle",
        "The spindle speed this preset gives. Left empty, the tool's own.";
    PRESET_RAMP_FEED: "Ramp feed",
        "How fast the tool feeds along a ramp or helix into material.";
    PRESET_STOCK_TO_LEAVE: "Stock to leave",
        "Material this preset leaves on walls and floors for a finishing pass.";
}
