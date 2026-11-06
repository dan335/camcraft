//! Help beyond the panels' own: names, the job, settings and playback.

topics! {
    NAME: "Name",
        "What it is called in the tree. No two of the same kind may share a name.";
    TOLERANCE: "Tolerance",
        "How closely toolpaths follow the part. Smaller is more accurate but slower to work out.";
    UNITS: "Units",
        "How the app shows lengths and feeds. Jobs and programs stay in millimetres.";
    THEME: "Theme",
        "The window's colours: the desktop's theme, camcraft's own, one Omarchy ships, or a colors.toml palette of your own in the themes folder.";
    GENERATE: "Generate",
        "Works out each setup's toolpaths by itself as soon as an edit settles, keeping what still stands. Off, a setup is generated only when asked.";
    STOP_AT_PROBLEMS: "Stop at problems",
        "Pauses playback on reaching a failed check, a warning or a steep descent.";
    STOP_AT_OPERATION_END: "Stop at each operation's end",
        "Pauses playback as each operation finishes.";
    SPEED: "Speed",
        "How much machining time plays each second. [ and ] change it.";
    SHOW_MATERIAL: "Part or cut stock",
        "Shows the stock as cut by the selected operation, or by the whole setup. M switches.";
    SHOW_PART: "Part",
        "Shows the part being machined.";
    SHOW_STOCK: "Stock outline",
        "Shows the edges of the stock block.";
    SHOW_WORK_ZERO: "Work zero",
        "Shows where the setup's X0 Y0 Z0 is.";
    SHOW_CUTTER: "Cutter",
        "Shows the tool at the playhead.";
    SHOW_TOOLPATHS: "Toolpaths",
        "Shows the toolpaths, coloured by what each move does.";
}
