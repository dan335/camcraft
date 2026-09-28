//! An operation's Tool tab: its tool, preset, speeds and feeds.

use crate::help::Figure;

topics! {
    TOOL: "Tool",
        "The cutter this operation uses. Choose one from the tool library, where its shape is set.";
    PRESET: "Preset",
        "A set of speeds and feeds saved with the tool. Choosing one copies its values into this operation.";
    SPINDLE: "Spindle",
        "How fast the tool turns. Left empty, the tool's own speed is used.";
    SURFACE_SPEED: "Surface speed",
        "How fast the cutting edge moves through the material. Makers chart cutters in it. Setting it sets the spindle speed.";
    CUTTING_FEED: "Cutting feed",
        "How fast the tool moves while it cuts.";
    CHIP_LOAD: "Chip load",
        "How thick a chip each flute takes per turn. Too thin rubs and burns. Setting it sets the cutting feed.",
        Figure::ChipLoad;
    PLUNGE_FEED: "Plunge feed",
        "How fast the tool feeds straight down into material. Usually slower than the cutting feed.";
}
