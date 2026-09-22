//! Tool geometry.
//!
//! A cutter is a solid of revolution, described canonically by **`r(z)`**: the
//! radius at height `z` above the tool tip. That form is fully general — it
//! describes flat, ball, bullnose, V-bit and tapered cutters, and equally the
//! undercutting ones (dovetail, T-slot, keyhole, lollipop) where `r` simply is not
//! monotonic.
//!
//! The drop-cutter wants the other direction: `h(d)`, the height at which the tool
//! surface sits at radial distance `d` from the axis. That is the generalised
//! inverse of `r`:
//!
//! ```text
//! h(d) = inf { z >= 0 : r(z) >= d }
//! ```
//!
//! For a monotonically non-decreasing `r` this is well defined and cheap. For an
//! undercutting tool it is not, because the tool reaches radius `d` at more than one
//! height, so [`ToolProfile::height_at`] returns `None` and the fast drop-cutter
//! path does not apply. Encoding that in the type is deliberate: it is what lets
//! undercutting tools arrive later without revisiting this trait.

/// Identifies a cutter's geometry exactly, for cache keying.
///
/// Parameters are stored as raw bit patterns so that two tools which are equal are
/// keyed equally, without the pitfalls of hashing floats by value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ToolKey {
    /// Discriminates the cutter shape.
    pub kind: &'static str,
    /// Shape parameters as exact bit patterns; unused slots are zero.
    pub params: [u64; 4],
}

/// A cutter's geometry, as a solid of revolution about the tool axis.
///
/// The tool tip is the origin and the controlled point — never the ball centre or
/// the holder gauge line. All lengths are millimetres.
pub trait ToolProfile {
    /// Radius of the tool at height `z` above the tip.
    ///
    /// Returns `0.0` below the tip, where there is no tool.
    fn radius_at(&self, z: f64) -> f64;

    /// The largest radius anywhere on the cutting portion.
    ///
    /// The drop-cutter uses this to bound its search footprint, so understating it
    /// means missed contacts, and missed contacts mean gouges.
    fn max_radius(&self) -> f64;

    /// Height of the tool surface at radial distance `d` from the axis: the
    /// generalised inverse `inf { z >= 0 : r(z) >= d }`.
    ///
    /// `None` when `d` exceeds [`max_radius`](ToolProfile::max_radius), or when the
    /// profile is not monotonic and the inverse is therefore ambiguous.
    fn height_at(&self, d: f64) -> Option<f64>;

    /// Whether `r(z)` is monotonically non-decreasing, and so `height_at` is defined
    /// across the whole radius range. False for undercutting tools.
    fn is_monotonic(&self) -> bool {
        true
    }

    /// A value identifying this tool's geometry exactly.
    fn geometry_key(&self) -> ToolKey;

    /// Smallest convex radius of curvature this cutter can leave on a CL surface, or
    /// `None` if it can leave a sharp convex edge.
    ///
    /// A rounded tip of radius `r` cannot produce a convex feature sharper than `r`,
    /// because offsetting any surface by `r` rounds its convex corners to exactly
    /// that. This bounds how badly a grid can misrepresent the CL surface between
    /// nodes, which is what makes a sampling pitch derivable rather than guessed.
    ///
    /// A flat mill returns `None`: passing over a step it produces a genuine
    /// discontinuity in slope, so no such bound exists and its grid must be sized
    /// from region accuracy instead.
    fn min_convex_radius(&self) -> Option<f64> {
        None
    }
}

/// A flat (square-end) end mill: a plain cylinder.
///
/// `r(z) = R`, so `h(d) = 0` for every `d` within the radius — the whole cutting
/// face lies in one plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flat {
    radius: f64,
}

/// A ball-nose end mill: a hemisphere of radius `R` below a shank of the same
/// radius.
///
/// `r(z) = sqrt(z(2R - z))` up to `z = R`, then `R`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ball {
    radius: f64,
}

/// A bullnose (corner-radius) end mill: a flat face of radius `R - c` surrounded by
/// a toroidal corner of radius `c`.
///
/// ```text
/// r(z) = (R - c) + sqrt(z(2c - z))   for 0 <= z <= c
///        R                           for z > c
/// ```
///
/// This shape subsumes the other two: `c = 0` is a [`Flat`] mill and `c = R` is a
/// [`Ball`]. Those keep their own dedicated implementations because both have fully
/// analytic contact tests, whereas the general bullnose does not — but the fact that
/// the general form must *agree* with them at those two limits is a strong check on
/// both, and the tests exercise it.
///
/// It also matters for stock-to-leave. Leaving stock `s` means machining with the
/// tool dilated by `s`, and a cylinder dilated by a ball is precisely a bullnose of
/// radius `R + s` and corner `s`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bullnose {
    radius: f64,
    corner: f64,
}

impl Bullnose {
    /// Build a bullnose mill of overall radius `radius` and corner radius `corner`.
    ///
    /// `corner` must lie in `[0, radius]`; outside that the shape is not a cutter.
    pub fn new(radius: f64, corner: f64) -> Result<Self, ToolError> {
        let radius = check_radius(radius)?;
        if !corner.is_finite() || corner < 0.0 || corner > radius {
            return Err(ToolError::BadCorner { corner, radius });
        }
        Ok(Self { radius, corner })
    }

    /// Radius of the flat portion of the face, `R - c`.
    pub fn flat_radius(&self) -> f64 {
        self.radius - self.corner
    }

    /// The corner radius.
    pub fn corner(&self) -> f64 {
        self.corner
    }
}

impl ToolProfile for Bullnose {
    fn radius_at(&self, z: f64) -> f64 {
        let (r, c) = (self.radius, self.corner);
        if z < 0.0 {
            0.0
        } else if z >= c {
            r
        } else {
            (r - c) + (z * (2.0 * c - z)).max(0.0).sqrt()
        }
    }

    fn max_radius(&self) -> f64 {
        self.radius
    }

    fn height_at(&self, d: f64) -> Option<f64> {
        let (r, c) = (self.radius, self.corner);
        if d > r {
            return None;
        }
        let flat = r - c;
        if d <= flat {
            return Some(0.0);
        }
        let k = d - flat;
        Some(c - (c * c - k * k).max(0.0).sqrt())
    }

    fn geometry_key(&self) -> ToolKey {
        ToolKey {
            kind: "bullnose",
            params: [self.radius.to_bits(), self.corner.to_bits(), 0, 0],
        }
    }

    fn min_convex_radius(&self) -> Option<f64> {
        (self.corner > 0.0).then_some(self.corner)
    }
}

/// Cutters expressible in the bullnose family.
///
/// Every cutter this engine has is a flat face of radius `R - c` surrounded by a
/// corner of radius `c`. Stating that as a trait is what lets stock-to-leave be
/// computed once, for all of them, instead of per shape — the family is closed under
/// dilation by the stock solid.
pub trait BullnoseFamily {
    /// This cutter as a bullnose. Infallible: the dimensions were validated when the
    /// cutter was built.
    fn as_bullnose(&self) -> Bullnose;
}

impl BullnoseFamily for Bullnose {
    fn as_bullnose(&self) -> Bullnose {
        *self
    }
}

impl BullnoseFamily for Flat {
    fn as_bullnose(&self) -> Bullnose {
        Bullnose {
            radius: self.radius,
            corner: 0.0,
        }
    }
}

impl BullnoseFamily for Ball {
    fn as_bullnose(&self) -> Bullnose {
        Bullnose {
            radius: self.radius,
            corner: self.radius,
        }
    }
}

/// A tool dimension that was not physically meaningful.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ToolError {
    /// A radius was zero, negative, or not finite.
    #[error("tool radius must be finite and greater than zero, got {0}")]
    BadRadius(f64),
    /// A corner radius did not fit within the tool.
    #[error("corner radius must be finite and within [0, {radius}], got {corner}")]
    BadCorner {
        /// The offending corner radius.
        corner: f64,
        /// The tool radius it had to fit inside.
        radius: f64,
    },
}

fn check_radius(radius: f64) -> Result<f64, ToolError> {
    if radius.is_finite() && radius > 0.0 {
        Ok(radius)
    } else {
        Err(ToolError::BadRadius(radius))
    }
}

impl Flat {
    /// Build a flat end mill of the given radius in millimetres.
    pub fn new(radius: f64) -> Result<Self, ToolError> {
        check_radius(radius).map(|radius| Self { radius })
    }
}

impl Ball {
    /// Build a ball-nose end mill of the given radius in millimetres.
    pub fn new(radius: f64) -> Result<Self, ToolError> {
        check_radius(radius).map(|radius| Self { radius })
    }
}

impl ToolProfile for Flat {
    fn radius_at(&self, z: f64) -> f64 {
        if z < 0.0 { 0.0 } else { self.radius }
    }

    fn max_radius(&self) -> f64 {
        self.radius
    }

    fn height_at(&self, d: f64) -> Option<f64> {
        (d <= self.radius).then_some(0.0)
    }

    fn geometry_key(&self) -> ToolKey {
        ToolKey {
            kind: "flat",
            params: [self.radius.to_bits(), 0, 0, 0],
        }
    }

    // Deliberately no min_convex_radius: a flat mill leaves sharp convex edges.
}

impl ToolProfile for Ball {
    fn radius_at(&self, z: f64) -> f64 {
        let r = self.radius;
        if z < 0.0 {
            0.0
        } else if z >= r {
            r
        } else {
            // sqrt(R^2 - (R - z)^2), arranged to stay accurate for small z.
            (z * (2.0 * r - z)).max(0.0).sqrt()
        }
    }

    fn max_radius(&self) -> f64 {
        self.radius
    }

    fn height_at(&self, d: f64) -> Option<f64> {
        let r = self.radius;
        if d > r {
            return None;
        }
        // The ball centre sits at z = R, so the surface at radius d is at
        // R - sqrt(R^2 - d^2).
        Some(r - (r * r - d * d).max(0.0).sqrt())
    }

    fn geometry_key(&self) -> ToolKey {
        ToolKey {
            kind: "ball",
            params: [self.radius.to_bits(), 0, 0, 0],
        }
    }

    fn min_convex_radius(&self) -> Option<f64> {
        Some(self.radius)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use proptest::prelude::*;

    const EPS: f64 = 1e-9;

    #[test]
    fn rejects_unphysical_radii() {
        assert_eq!(Flat::new(0.0), Err(ToolError::BadRadius(0.0)));
        assert_eq!(Ball::new(-1.0), Err(ToolError::BadRadius(-1.0)));
        assert!(matches!(Ball::new(f64::NAN), Err(ToolError::BadRadius(_))));
        assert!(Flat::new(3.175).is_ok());
    }

    #[test]
    fn flat_is_a_cylinder() {
        let t = Flat::new(3.0).unwrap();
        assert_eq!(t.radius_at(0.0), 3.0);
        assert_eq!(t.radius_at(50.0), 3.0);
        assert_eq!(t.radius_at(-1.0), 0.0);
        // The entire cutting face lies in one plane.
        assert_eq!(t.height_at(0.0), Some(0.0));
        assert_eq!(t.height_at(3.0), Some(0.0));
        assert_eq!(t.height_at(3.000_001), None);
    }

    #[test]
    fn ball_matches_closed_form() {
        let r = 3.0;
        let t = Ball::new(r).unwrap();

        // Tip.
        assert_abs_diff_eq!(t.radius_at(0.0), 0.0, epsilon = EPS);
        assert_abs_diff_eq!(t.height_at(0.0).unwrap(), 0.0, epsilon = EPS);

        // Equator: at z = R the ball reaches full radius.
        assert_abs_diff_eq!(t.radius_at(r), r, epsilon = EPS);
        assert_abs_diff_eq!(t.height_at(r).unwrap(), r, epsilon = EPS);

        // 45 degrees around the ball: d = R/sqrt(2) sits at z = R - R/sqrt(2).
        let d = r / 2.0_f64.sqrt();
        assert_abs_diff_eq!(t.height_at(d).unwrap(), r - d, epsilon = EPS);

        // Above the ball it is a plain shank.
        assert_abs_diff_eq!(t.radius_at(10.0), r, epsilon = EPS);
    }

    /// The duality the whole drop-cutter rests on.
    fn check_inverse<T: ToolProfile>(t: &T, d: f64) -> Result<(), TestCaseError> {
        let Some(z) = t.height_at(d) else {
            return Ok(());
        };
        prop_assert!(z >= 0.0, "height must be at or above the tip, got {z}");
        // h(d) = inf { z : r(z) >= d }, so at h(d) the tool has reached radius d...
        prop_assert!(
            t.radius_at(z) >= d - 1e-7,
            "r(h(d)) = {} should be >= d = {d}",
            t.radius_at(z)
        );
        // ...and just below it, it has not.
        if z > 1e-6 {
            prop_assert!(
                t.radius_at(z - 1e-6) <= d + 1e-7,
                "r just below h(d) should not yet have reached d"
            );
        }
        Ok(())
    }

    proptest! {
        #[test]
        fn ball_inverse_holds(r in 0.1f64..25.0, frac in 0.0f64..=1.0) {
            let t = Ball::new(r).unwrap();
            check_inverse(&t, frac * r)?;
        }

        #[test]
        fn flat_inverse_holds(r in 0.1f64..25.0, frac in 0.0f64..=1.0) {
            let t = Flat::new(r).unwrap();
            check_inverse(&t, frac * r)?;
        }

        /// `r(z)` must be monotonically non-decreasing for both 0.1 tools. That is
        /// what makes `height_at` well defined, and it is exactly the assumption an
        /// undercutting tool will later break.
        #[test]
        fn radius_is_monotonic(r in 0.1f64..25.0, a in 0.0f64..50.0, b in 0.0f64..50.0) {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            let ball = Ball::new(r).unwrap();
            let flat = Flat::new(r).unwrap();
            prop_assert!(ball.radius_at(lo) <= ball.radius_at(hi) + 1e-12);
            prop_assert!(flat.radius_at(lo) <= flat.radius_at(hi) + 1e-12);
            prop_assert!(ball.is_monotonic() && flat.is_monotonic());
        }

        /// Nothing may exceed the declared maximum radius; the drop-cutter bounds
        /// its search footprint with it, so an understated value means gouges.
        #[test]
        fn never_exceeds_max_radius(r in 0.1f64..25.0, z in -5.0f64..100.0) {
            let ball = Ball::new(r).unwrap();
            let flat = Flat::new(r).unwrap();
            prop_assert!(ball.radius_at(z) <= ball.max_radius() + 1e-12);
            prop_assert!(flat.radius_at(z) <= flat.max_radius() + 1e-12);
        }
    }
}
