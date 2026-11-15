//! Arranging parts on a sheet: where each copy of each part stands, so that
//! all of them that can fit do, none nearer another than the spacing nor the
//! sheet's edge than its border.
//!
//! # A stand
//!
//! A copy is its part as it stands, turned about Z and moved: a [`Stand`].
//! Re-arranging changes stands only, never the part, so a part is worked out
//! once however many copies of it there are.
//!
//! # Packing on a grid
//!
//! [`pack`] lays the parts one at a time on a grid of square cells over the
//! sheet. Each part's outline is grown by half the spacing and by half a
//! cell's diagonal more, and covers every cell whose centre lies inside that:
//! every point within half the spacing of the part is in a cell whose centre
//! is within half a diagonal of it, so the cells cover the part grown by half
//! the spacing. Two parts whose cells are apart are therefore at least the
//! spacing apart, and a part whose cells are all on a grid inset by the
//! border less half the spacing is at least the border inside the sheet.
//! Cells inside a part's hole are free, so a smaller part can stand in it.
//!
//! Each part goes where it uses the least of the sheet's length, then nearest
//! the start along it and across it, then at the earliest turn it may take.
//! Its turns are searched at once; that total order picks among what they
//! find, so the layout is the same on any number of threads. Several orders
//! of the parts are laid, and the best layout kept.
//!
//! The grid errs to the safe side only: parts can stand a little further
//! apart than asked, never nearer. Every layout is then checked exactly, with
//! the outlines themselves ([`verify`]), before it is given back; one that
//! fails is an error, never a layout.

mod grid;
mod pack;
mod verify;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests;

pub use pack::pack;
pub use verify::verify;

use std::hash::{Hash, Hasher};

use camcraft_geom::contour::{Region, RegionError, TICK};
use camcraft_geom::turn::Turn;
use glam::{DVec2, DVec3};

use crate::control::Stopped;
use crate::must;

/// How far the clipper's rounding to its tick can move a grown outline's
/// edge from where it would be exactly: half a tick's diagonal for each point
/// it is given, and half for each it makes.
const TICK_ERROR: f64 = std::f64::consts::SQRT_2 * TICK;

/// Where a copy of a part stands: its part turned about Z by `turn`, about
/// its own origin, then moved by `offset`.
///
/// No stand at all ([`Stand::NONE`], the default) leaves every point as it
/// was, bit for bit, and does no arithmetic. A quarter turn moves each point
/// exactly, by swapping and negating ([`Turn`]).
///
/// Two stands are equal, and hash alike, when their turns are and their
/// offsets have the same bits, so a stand can be part of a key.
#[derive(Debug, Clone, Copy, Default)]
pub struct Stand {
    /// The turn about Z.
    pub turn: Turn,
    /// Where the part's origin goes once turned: across the sheet, and up or
    /// down to seat it.
    pub offset: DVec3,
}

impl Stand {
    /// Standing where it is: no turn and no move.
    pub const NONE: Self = Self {
        turn: Turn::NONE,
        offset: DVec3::ZERO,
    };

    /// Whether it stands where it is.
    pub fn is_none(&self) -> bool {
        self.turn.is_none() && self.offset == DVec3::ZERO
    }

    /// The point `p` of the part, where this copy has it.
    pub fn point(&self, p: DVec3) -> DVec3 {
        if self.is_none() {
            return p;
        }
        self.turn.apply(p.truncate()).extend(p.z) + self.offset
    }

    /// The direction `v` of the part, as this copy has it: turned, not moved.
    pub fn direction(&self, v: DVec3) -> DVec3 {
        if self.turn.is_none() {
            return v;
        }
        self.turn.apply(v.truncate()).extend(v.z)
    }

    /// The outline `r` of the part, seen from above, where this copy has it:
    /// its move up or down changes nothing seen from above.
    pub fn region(&self, r: &Region) -> Region {
        r.rotated(self.turn).translated(self.offset.truncate())
    }

    /// The offset's bits, for equality and hashing.
    fn offset_bits(&self) -> [u64; 3] {
        self.offset.to_array().map(f64::to_bits)
    }
}

impl PartialEq for Stand {
    fn eq(&self, other: &Self) -> bool {
        self.turn == other.turn && self.offset_bits() == other.offset_bits()
    }
}

impl Eq for Stand {}

impl Hash for Stand {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.turn.hash(state);
        self.offset_bits().hash(state);
    }
}

/// The corner of the sheet packing starts from, seen from above: the front
/// is towards −Y, the left towards −X.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Corner {
    /// −X, −Y.
    #[default]
    FrontLeft,
    /// +X, −Y.
    FrontRight,
    /// −X, +Y.
    BackLeft,
    /// +X, +Y.
    BackRight,
}

impl Corner {
    /// Whether it is on the left, towards −X.
    fn left(self) -> bool {
        matches!(self, Self::FrontLeft | Self::BackLeft)
    }

    /// Whether it is at the front, towards −Y.
    fn front(self) -> bool {
        matches!(self, Self::FrontLeft | Self::FrontRight)
    }
}

/// The axis a sheet is used up along: parts fill across it first, and the
/// length used is measured along it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Along X: parts stack across Y, and the sheet is used from one end of X.
    #[default]
    X,
    /// Along Y: parts line up across X, and the sheet is used from one end of Y.
    Y,
}

/// A sheet and how parts are arranged on it.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    /// Its front-left corner, seen from above.
    pub corner: DVec2,
    /// Its size along X and Y.
    pub size: DVec2,
    /// The least distance between two parts.
    pub spacing: f64,
    /// The least distance between a part and the sheet's edge.
    pub border: f64,
    /// The corner packing starts from.
    pub from: Corner,
    /// The axis the sheet is used up along.
    pub along: Direction,
}

/// A part to arrange: which outline it has, and the turns it may take.
///
/// Copies of a part are simply parts naming the same outline; each may take
/// any of its turns, and the earlier turns are preferred where two would do
/// equally well.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// Its outline, by its place among the shapes.
    pub shape: usize,
    /// The turns it may take: at least one.
    pub turns: Vec<Turn>,
}

/// What became of one part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// It stands there.
    Placed(Stand),
    /// It does not fit on the sheet, inside its border, at any turn it may
    /// take, even alone.
    TooLarge,
    /// It would fit alone, but there is no room left for it among the others.
    NoRoom,
}

/// Why parts could not be arranged.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ArrangeError {
    /// A size of the sheet is not one there can be.
    #[error("the sheet's {what} must be {must}")]
    Sheet {
        /// Which.
        what: &'static str,
        /// What it must be.
        must: &'static str,
    },
    /// The border takes the whole sheet.
    #[error("the border leaves no room on the sheet")]
    NoRoom,
    /// A part names an outline there is not.
    #[error("part {part} has shape {shape}, and there are {shapes}")]
    NoSuchShape {
        /// The part, by its place.
        part: usize,
        /// The shape it names.
        shape: usize,
        /// How many there are.
        shapes: usize,
    },
    /// A part may take no turn at all.
    #[error("part {part} may take no turn")]
    NoTurns {
        /// The part, by its place.
        part: usize,
    },
    /// An outline encloses nothing.
    #[error("shape {shape} has no area")]
    EmptyShape {
        /// The shape, by its place.
        shape: usize,
    },
    /// A polygon operation failed.
    #[error("region operation failed: {0}")]
    Region(String),
    /// The layout failed its exact check. The grid errs to the safe side, so
    /// this is a fault in the packing, never a layout to use.
    #[error("the layout failed its check: {0}")]
    Unverified(Violation),
    /// It was stopped while it worked ([`crate::control`]).
    #[error(transparent)]
    Stopped(#[from] Stopped),
}

impl From<RegionError> for ArrangeError {
    fn from(e: RegionError) -> Self {
        Self::Region(e.to_string())
    }
}

/// How a layout breaks the sheet's rules, as [`verify`] finds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Violation {
    /// Not one outcome for each part.
    #[error("{outcomes} outcomes for {parts} parts")]
    Outcomes {
        /// How many parts.
        parts: usize,
        /// How many outcomes.
        outcomes: usize,
    },
    /// A part stands at a turn it may not take.
    #[error("part {part} stands at a turn it may not take")]
    Turn {
        /// The part, by its place.
        part: usize,
    },
    /// A part is nearer the sheet's edge than the border, or off it.
    #[error("part {part} is nearer the sheet's edge than the border")]
    Border {
        /// The part, by its place.
        part: usize,
    },
    /// Two parts are nearer each other than the spacing.
    #[error("parts {a} and {b} are nearer each other than the spacing")]
    Spacing {
        /// The first, by its place.
        a: usize,
        /// The second, by its place.
        b: usize,
    },
}

/// Whether the sheet, the shapes and the parts are ones there can be.
fn check(sheet: &Sheet, shapes: &[Region], parts: &[Part]) -> Result<(), ArrangeError> {
    let positive = |what, v: f64| {
        crate::is_positive(v)
            .then_some(())
            .ok_or(ArrangeError::Sheet {
                what,
                must: must::POSITIVE,
            })
    };
    let at_least_zero = |what, v: f64| {
        (v.is_finite() && v >= 0.0)
            .then_some(())
            .ok_or(ArrangeError::Sheet {
                what,
                must: must::AT_LEAST_ZERO,
            })
    };
    positive("length", sheet.size.x)?;
    positive("width", sheet.size.y)?;
    at_least_zero("spacing", sheet.spacing)?;
    at_least_zero("border", sheet.border)?;
    if !sheet.corner.is_finite() {
        return Err(ArrangeError::Sheet {
            what: "corner",
            must: must::FINITE,
        });
    }
    if 2.0 * sheet.border >= sheet.size.min_element() {
        return Err(ArrangeError::NoRoom);
    }
    for (part, p) in parts.iter().enumerate() {
        let Some(shape) = shapes.get(p.shape) else {
            return Err(ArrangeError::NoSuchShape {
                part,
                shape: p.shape,
                shapes: shapes.len(),
            });
        };
        if p.turns.is_empty() {
            return Err(ArrangeError::NoTurns { part });
        }
        if shape.area() <= 0.0 {
            return Err(ArrangeError::EmptyShape { shape: p.shape });
        }
    }
    Ok(())
}
