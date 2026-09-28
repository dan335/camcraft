//! A setup's panel: which way the part sits, its work zero, and how it posts.

topics! {
    UP_Z: "Up (Z)",
        "Which of the model's directions points up, towards the spindle.";
    ALONG_X: "Along X",
        "Which of the model's directions runs along the machine's X.";
    WORK_ZERO_ON: "On",
        "What the work zero is measured from: the stock, the models' box, or the model's own zero.";
    WORK_ZERO_POINT: "Work zero",
        "Where X0 Y0 sits, seen from above. Touch off at the same place on the machine.";
    WORK_ZERO_HEIGHT: "Height",
        "Where Z0 sits: the box's top, middle or bottom. Touch off at the same height on the machine.";
    WORK_ZERO_OFFSET: "Offset",
        "Moves the work zero from the chosen point by this much along each axis.";
    WCS: "Work offset",
        "Which of the machine's work offsets the program uses. Touch off into the same one.";
    POST: "Post",
        "The machine the program is written for. File… loads a machine profile of your own.";
}
