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

use crate::dropcutter::DropTest;
use crate::tool::{Bullnose, BullnoseFamily, Cutter, ToolError, ToolProfile};
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
    /// Stock to leave must be finite and not negative.
    #[error("stock to leave must be finite and at least zero, got radial {radial}, axial {axial}")]
    Negative {
        /// The radial amount given.
        radial: f64,
        /// The axial amount given.
        axial: f64,
    },
}

impl Allowance {
    /// Leave `radial` perpendicular to the axis and `axial` along it.
    pub fn new(radial: f64, axial: f64) -> Result<Self, AllowanceError> {
        if !radial.is_finite() || !axial.is_finite() || radial < 0.0 || axial < 0.0 {
            return Err(AllowanceError::Negative { radial, axial });
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
    allowance: Allowance,
}

impl EffectiveTool {
    /// Dilate `tool` by `stock`.
    ///
    /// Takes anything convertible into the bullnose family: every cutter but a
    /// cone, which [`EffectiveTool::of`] takes as it is.
    pub fn new(tool: &(impl BullnoseFamily + ?Sized), stock: Allowance) -> Result<Self, ToolError> {
        let base = tool.as_bullnose();
        let m = stock.radial.min(stock.axial);
        let shape = Bullnose::new(base.max_radius() + stock.radial, base.corner() + m)?;
        Ok(Self {
            shape: Cutter::Bullnose(shape),
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
                allowance: stock,
            }),
            Cutter::Cone(_) => Err(ToolError::ConeStock),
        }
    }

    /// The dilated shape the drop-cutter actually contacts with.
    pub fn shape(&self) -> &Cutter {
        &self.shape
    }

    /// The stock to leave this was built for.
    pub fn allowance(&self) -> &Allowance {
        &self.allowance
    }

    /// The real tool's widest radius, without the stock to leave: what a
    /// machining boundary's containment keeps in or out, since stock is left on
    /// the part, not on the boundary.
    pub fn cutter_radius(&self) -> f64 {
        self.shape.max_radius() - self.allowance.radial
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
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests {
    use super::*;
    use crate::dropcutter::DropCutter;
    use crate::tool::{Ball, Flat};
    use camcraft_geom::synthetic;

    #[test]
    fn rejects_unusable_stock() {
        assert!(matches!(
            Allowance::new(-0.1, 0.0),
            Err(AllowanceError::Negative { .. })
        ));
        assert!(matches!(
            Allowance::new(0.0, f64::NAN),
            Err(AllowanceError::Negative { .. })
        ));
        assert!(Allowance::new(0.0, 0.0).is_ok());
        assert!(Allowance::none().is_none());
    }

    /// A flat mill leaving stock becomes a bullnose. Getting this wrong -- treating
    /// stock as the same tool raised -- would cut through the corner it exists to
    /// protect.
    #[test]
    fn isotropic_stock_rounds_a_flat_mill() {
        let e = EffectiveTool::new(&Flat::new(5.0).unwrap(), Allowance::isotropic(0.5).unwrap())
            .unwrap();
        assert_eq!(e.shape().max_radius(), 5.5);
        assert_eq!(e.shape().bullnose().unwrap().corner(), 0.5);
        assert_eq!(e.tip_offset(), 0.5);
    }

    #[test]
    fn isotropic_stock_keeps_a_ball_a_ball() {
        let e = EffectiveTool::new(&Ball::new(3.0).unwrap(), Allowance::isotropic(0.5).unwrap())
            .unwrap();
        // corner == radius is exactly a ball.
        assert_eq!(e.shape().max_radius(), 3.5);
        assert_eq!(e.shape().bullnose().unwrap().corner(), 3.5);
    }

    #[test]
    fn pure_axial_widens_nothing_and_pure_radial_raises_nothing() {
        let flat = Flat::new(4.0).unwrap();

        let axial = EffectiveTool::new(&flat, Allowance::new(0.0, 0.7).unwrap()).unwrap();
        assert_eq!(
            axial.shape().max_radius(),
            4.0,
            "axial stock must not widen"
        );
        assert_eq!(
            axial.shape().bullnose().unwrap().corner(),
            0.0,
            "axial stock must not round"
        );
        assert_eq!(axial.tip_offset(), 0.7);

        let radial = EffectiveTool::new(&flat, Allowance::new(0.7, 0.0).unwrap()).unwrap();
        assert_eq!(radial.shape().max_radius(), 4.7, "radial stock must widen");
        assert_eq!(radial.tip_offset(), 0.0, "radial stock must not raise");
    }

    /// Zero stock must be indistinguishable from the bare cutter, or the seam has
    /// introduced an error at the very point it should be transparent.
    #[test]
    fn zero_stock_matches_the_bare_tool_exactly() {
        let mesh = synthetic::uv_sphere(9.0, 40, 60);
        let dc = DropCutter::new(&mesh);
        let ball = Ball::new(2.0).unwrap();
        let effective = EffectiveTool::new(&ball, Allowance::none()).unwrap();

        let mut compared = 0;
        for i in -7..=7 {
            for j in -7..=7 {
                let (x, y) = (f64::from(i) * 0.9, f64::from(j) * 0.8);
                match (dc.drop_at(&ball, x, y), dc.drop_at(&effective, x, y)) {
                    (Some(a), Some(b)) => {
                        assert!((a - b).abs() < 1e-7, "at ({x},{y}) {a} vs {b}");
                        compared += 1;
                    }
                    (None, None) => {}
                    (a, b) => panic!("coverage differs at ({x},{y}): {a:?} vs {b:?}"),
                }
            }
        }
        assert!(compared > 80, "expected real overlap, compared {compared}");
    }

    /// On a level floor the tool must end up exactly the axial stock above it,
    /// whatever the radial amount does.
    #[test]
    fn leaves_exactly_the_axial_stock_on_a_floor() {
        let mesh = synthetic::plane(2.0, 30.0);
        let dc = DropCutter::new(&mesh);
        for (radial, axial) in [(0.0, 0.5), (0.5, 0.5), (1.0, 0.25), (0.8, 0.0)] {
            let stock = Allowance::new(radial, axial).unwrap();
            let tool = EffectiveTool::new(&Flat::new(4.0).unwrap(), stock).unwrap();
            let z = dc.drop_at(&tool, 0.0, 0.0).unwrap();
            assert!(
                (z - (2.0 + axial)).abs() < 1e-9,
                "radial {radial}, axial {axial}: got {z}, want {}",
                2.0 + axial
            );
        }
    }

    /// More stock can never put the tool lower. Monotonicity is the cheap sanity
    /// check that catches a sign error anywhere in the dilation.
    #[test]
    fn more_stock_never_cuts_deeper() {
        let mesh = synthetic::uv_sphere(10.0, 50, 70);
        let dc = DropCutter::new(&mesh);
        let base = Ball::new(2.0).unwrap();

        for (x, y) in [(0.0, 0.0), (3.0, 2.0), (-5.0, 1.0), (6.0, -4.0)] {
            let mut previous = f64::NEG_INFINITY;
            for step in 0..5 {
                let s = f64::from(step) * 0.25;
                let tool = EffectiveTool::new(&base, Allowance::isotropic(s).unwrap()).unwrap();
                if let Some(z) = dc.drop_at(&tool, x, y) {
                    assert!(
                        z >= previous - 1e-9,
                        "at ({x},{y}) stock {s} gave {z}, below the previous {previous}"
                    );
                    previous = z;
                }
            }
        }
    }
}
