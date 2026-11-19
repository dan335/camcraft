//! The part as an operation's tool meets it.
//!
//! Every 3D strategy drops its tool onto the part, and those that cut at
//! levels ask where at a level it may not be. Each asks it of [`PartSurfaces`],
//! not of the part's mesh, so what an operation meets is said in one place,
//! and no strategy needs to know what it is. For now it is always the whole
//! part, met by the operation's tool.
//!
//! A tool is held over it once, as a [`Seat`], before anything is dropped:
//! whatever the part asks of the tool is worked out then, and a drop is the
//! drop-cutter's own, exact, with nothing worked out again for it.

use crate::cutter::allowance::EffectiveTool;
use crate::cutter::dropcutter::DropCutter;
use crate::cutter::tool::ToolError;
use crate::strategy::reach::{LevelReach, ReachError};

/// The part as an operation's tool meets it: what the tool comes to rest on,
/// dropped anywhere, and what keeps it off at a level.
#[derive(Debug)]
pub struct PartSurfaces<'a> {
    form: Form<'a>,
}

/// What the part is, to the operation.
#[derive(Debug)]
enum Form<'a> {
    /// All of it, as it is, met by the tool itself.
    Whole(DropCutter<'a>),
}

impl<'a> PartSurfaces<'a> {
    /// The whole part, as `cutter` drops onto it: the part's own drop-cutter,
    /// sharing its index with every other.
    pub fn whole(cutter: DropCutter<'a>) -> Self {
        Self {
            form: Form::Whole(cutter),
        }
    }

    /// `tool` held over the part, to be dropped onto it anywhere.
    pub fn under(&self, tool: &EffectiveTool) -> Result<Seat<'_>, ToolError> {
        let on = match &self.form {
            Form::Whole(cutter) => On::Whole(cutter),
        };
        Ok(Seat { tool: *tool, on })
    }

    /// Where at a level `tool` may not be, against the part, to `tolerance`
    /// ([`LevelReach::new`]).
    pub fn reach(
        &self,
        tool: &EffectiveTool,
        tolerance: f64,
    ) -> Result<LevelReach<'_>, ReachError> {
        match &self.form {
            Form::Whole(cutter) => LevelReach::new(cutter.mesh(), tool, tolerance),
        }
    }
}

/// A tool held over the part as an operation meets it
/// ([`PartSurfaces::under`]): where it comes to rest, dropped at any point.
#[derive(Debug)]
pub struct Seat<'s> {
    tool: EffectiveTool,
    on: On<'s>,
}

/// What a held tool is dropped onto.
#[derive(Debug)]
enum On<'s> {
    /// The whole part, by its drop-cutter.
    Whole(&'s DropCutter<'s>),
}

impl Seat<'_> {
    /// The tool held, as it was given.
    pub fn tool(&self) -> &EffectiveTool {
        &self.tool
    }

    /// Where the tool's tip comes to rest dropped at `(x, y)`, or `None` where
    /// nothing is under it ([`DropCutter::drop_at`]).
    #[inline]
    pub fn drop_at(&self, x: f64, y: f64) -> Option<f64> {
        match self.on {
            On::Whole(cutter) => cutter.drop_at(&self.tool, x, y),
        }
    }

    /// As [`drop_at`](Self::drop_at), with `scratch` for the triangles it
    /// looks at: for a caller making many drops, which then allocates once.
    #[inline]
    pub fn drop_at_in(&self, x: f64, y: f64, scratch: &mut Vec<u32>) -> Option<f64> {
        match self.on {
            On::Whole(cutter) => cutter.drop_at_in(&self.tool, x, y, scratch),
        }
    }
}
