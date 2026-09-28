//! An operation's Heights tab. Each is an offset from what is chosen beside it.

use crate::help::Figure;
use crate::view::viewport::PlaneKind;

topics! {
    CLEARANCE: "Clearance",
        "The height the tool arrives at and leaves from. Keep it above clamps and anything else in the way.",
        Figure::Height(PlaneKind::Clearance);
    RETRACT: "Retract",
        "The height the tool lifts to between cuts. Lower saves time, but keep it clear of the material.",
        Figure::Height(PlaneKind::Retract);
    TOP: "Top",
        "The highest level cut. Left empty, cutting starts at the stock's top.",
        Figure::Height(PlaneKind::Top);
    BOTTOM: "Bottom",
        "The deepest level cut. Left empty, the strategy's own: usually the part's bottom.",
        Figure::Height(PlaneKind::Bottom);
}
