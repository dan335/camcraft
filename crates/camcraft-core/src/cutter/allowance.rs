//! Stock to leave, and the effective tool it implies.
//!
//! # Leaving stock is a Minkowski sum
//!
//! Machining a part while leaving stock `S` is the same as machining the part
//! *offset* by `S`, and that in turn is the same as machining the original part with
//! the tool *dilated* by `S`. The drop-cutter therefore never sees the raw cutter —
//! it sees an effective one.
//!
//! This is not a matter of shifting a profile. A ball mill of radius `r` leaving
//! isotropic stock `s` behaves as a ball of radius `r + s`, and a *flat* mill leaving
//! stock behaves as a **bullnose**, because a cylinder dilated by a ball has a
//! rounded corner. Treating stock as "the same tool, raised" would quietly cut
//! through the corner it is supposed to protect.
//!
//! # The shape of the stock
//!
//! Radial and axial stock are separate numbers, so the stock solid is not a ball.
//! Here it is a **rounded cylinder** — a disc, a ball and a vertical segment summed
//! together:
//!
//! ```text
//! S(b, a) = disc(b - m) + ball(m) + segment(a - m),   m = min(a, b)
//! ```
//!
//! This is a modelling choice and worth stating plainly. An ellipsoid with semi-axes
//! `(b, b, a)` is the other natural reading. The two agree **exactly** wherever it is
//! unambiguous — pure radial, pure axial, and isotropic stock — and differ only in
//! how the corner blends on a slope when the two amounts differ. The rounded cylinder
//! is chosen because it composes: the bullnose family is closed under it, so the
//! effective tool stays a tool this engine can machine with exactly, rather than
//! requiring a new elliptical-corner cutter and a numerical contact solve.
//!
//! # The result
//!
//! Summing a bullnose `(R, c)` with `S(b, a)` gives
//!
//! ```text
//! effective = bullnose(R + b, c + min(a, b)),   tip offset = a
//! ```
//!
//! which reduces correctly at every limit: zero stock is the tool itself, isotropic
//! `s` gives `(R + s, c + s)` raised by `s`, pure axial leaves the shape alone and
//! only raises it, and pure radial widens it without raising it at all.
//!
//! # Negative stock
//!
//! Negative stock cuts past the part, as Fusion's does: the same sum with the
//! stock taken away, so the tool shrinks rather than grows and its tip goes
//! below the contact rather than above it. A ball shrunk by `s` is a ball of
//! `r - s`, and a flat mill shrunk stays flat, its corner already as sharp as it
//! goes: the corner stops at zero,
//!
//! ```text
//! effective = bullnose(R + b, max(c + min(a, b), 0)),   tip offset = a
//! ```
//!
//! Past a round end's corner radius, down, the shrunk end is flat where the real
//! one is round, so a slope is cut past by less than asked; the job refuses that
//! in 3D, as Fusion does ([`crate::job::Operation::cannot_use`]). Beside, the
//! tool cannot shrink past nothing: `R + b` must stay above zero.
//!
//! # The tool that cuts
//!
//! The grown tool is what the drop-cutter contacts, and every path point is where
//! the *real* tip goes. So the material is cut by the real tool, at those points:
//! the simulator sweeps [`EffectiveTool::base`], or the stock left on a wall would
//! be swept away with the grown tool's extra width.

use crate::cutter::dropcutter::DropTest;
use crate::cutter::tool::{Bullnose, BullnoseFamily, Cutter, ToolError, ToolProfile};
use glam::DVec3;

/// How much material to leave behind, measured perpendicular to the tool axis
/// (`radial`) and along it (`axial`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Allowance {
    radial: f64,
    axial: f64,
}

/// Stock to leave that was not usable.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum AllowanceError {
    /// Stock to leave must be a number.
    #[error("stock to leave must be a number, got radial {radial}, axial {axial}")]
    NotANumber {
        /// The radial amount given.
        radial: f64,
        /// The axial amount given.
        axial: f64,
    },
}

impl Allowance {
    /// Leave `radial` perpendicular to the axis and `axial` along it; either
    /// negative to cut past the part.
    pub fn new(radial: f64, axial: f64) -> Result<Self, AllowanceError> {
        if !radial.is_finite() || !axial.is_finite() {
            return Err(AllowanceError::NotANumber { radial, axial });
        }
        Ok(Self { radial, axial })
    }

    /// Leave the same amount in both directions.
    pub fn isotropic(amount: f64) -> Result<Self, AllowanceError> {
        Self::new(amount, amount)
    }

    /// Leave nothing — a finishing pass.
    pub fn none() -> Self {
        Self {
            radial: 0.0,
            axial: 0.0,
        }
    }

    /// The radial amount.
    pub fn radial(&self) -> f64 {
        self.radial
    }

    /// The axial amount.
    pub fn axial(&self) -> f64 {
        self.axial
    }

    /// Whether this leaves nothing at all.
    pub fn is_none(&self) -> bool {
        self.radial == 0.0 && self.axial == 0.0
    }
}

/// A cutter dilated by its stock to leave, together with the offset back to the real
/// tool tip.
///
/// The drop-cutter always works with one of these, never a raw cutter. Contact is
/// computed for the dilated shape and then raised by the axial stock, which puts the
/// *real* tool tip exactly the requested distance clear of the part.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectiveTool {
    shape: Cutter,
    base: Cutter,
    allowance: Allowance,
}

impl EffectiveTool {
    /// Dilate `tool` by `stock`, or shrink it by negative stock.
    ///
    /// Takes anything convertible into the bullnose family: every cutter but a
    /// cone, which [`EffectiveTool::of`] takes as it is.
    pub fn new(tool: &(impl BullnoseFamily + ?Sized), stock: Allowance) -> Result<Self, ToolError> {
        let base = tool.as_bullnose();
        let m = stock.radial.min(stock.axial);
        let shape = Bullnose::new(
            base.max_radius() + stock.radial,
            (base.corner() + m).max(0.0),
        )?;
        Ok(Self {
            shape: Cutter::Bullnose(shape),
            base: Cutter::Bullnose(base),
            allowance: stock,
        })
    }

    /// Dilate any cutter by `stock`. A cone grown by a ball is no longer a cone,
    /// so a cone is taken only as it is, with no stock to leave.
    pub fn of(tool: &Cutter, stock: Allowance) -> Result<Self, ToolError> {
        match tool {
            Cutter::Bullnose(b) => Self::new(b, stock),
            Cutter::Cone(_) if stock.is_none() => Ok(Self {
                shape: *tool,
                base: *tool,
                allowance: stock,
            }),
            Cutter::Cone(_) => Err(ToolError::ConeStock),
        }
    }

    /// The dilated shape the drop-cutter actually contacts with.
    #[cfg(test)]
    pub(crate) fn shape(&self) -> &Cutter {
        &self.shape
    }

    /// The real tool, which cuts the material at the points the grown one finds.
    pub fn base(&self) -> &Cutter {
        &self.base
    }

    /// The stock to leave this was built for.
    pub fn allowance(&self) -> &Allowance {
        &self.allowance
    }

    /// The real tool's widest radius, without the stock to leave: what a
    /// machining boundary's containment keeps in or out, since stock is left on
    /// the part, not on the boundary; and what the material is cut with.
    pub fn cutter_radius(&self) -> f64 {
        self.base.max_radius()
    }

    /// How far the real tool tip sits above the dilated shape's contact.
    pub fn tip_offset(&self) -> f64 {
        self.allowance.axial
    }
}

/// The geometry of an effective tool is the *dilated* shape, because that is what
/// determines the footprint the drop-cutter must search and the contact it computes.
/// The offset back to the real tip is applied by [`DropTest::drop_triangle`].
impl ToolProfile for EffectiveTool {
    fn radius_at(&self, z: f64) -> f64 {
        self.shape.radius_at(z)
    }

    fn max_radius(&self) -> f64 {
        self.shape.max_radius()
    }

    fn height_at(&self, d: f64) -> Option<f64> {
        self.shape.height_at(d)
    }
}

impl DropTest for EffectiveTool {
    /// Contact the dilated shape, then raise by the axial stock so the answer is the
    /// position of the *real* tool tip.
    fn drop_triangle(&self, x: f64, y: f64, tri: &[DVec3; 3]) -> Option<f64> {
        self.shape
            .drop_triangle(x, y, tri)
            .map(|z| z + self.allowance.axial)
    }

    fn above_contact(&self) -> f64 {
        self.shape.above_contact() + self.allowance.axial
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests;
