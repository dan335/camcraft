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

use crate::is_positive;

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
}

/// Cutters expressible in the bullnose family.
///
/// Flat, ball and corner-radius mills are each a flat face of radius `R - c`
/// surrounded by a corner of radius `c`. Stating that as a trait is what lets
/// stock-to-leave be computed once, for all of them, instead of per shape — the
/// family is closed under dilation by the stock solid. A [`Cone`] is not in it.
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

/// A cone: a V-bit, chamfer mill or drill point. A flat tip of radius `tip`
/// widening at half-angle `α` to the full radius `R`, then a shank of radius `R`.
///
/// ```text
/// h(d) = 0                     for d <= tip
///        (d - tip) / tan(α)    for tip < d <= R
/// ```
///
/// `h` is convex, which is what makes its drop-cutter contacts exact or bounded
/// solves, as the bullnose's are. A cone is not closed under stock to leave: grown
/// by a ball, its tip rounds, so a cone cuts to size only.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cone {
    radius: f64,
    tip: f64,
    /// `1 / tan(α)`: how far the flank rises for each unit out.
    rise: f64,
}

impl Cone {
    /// A cone of overall radius `radius`, tip radius `tip` and half-angle
    /// `half_angle` radians from the axis: 45° for a 90° V-bit.
    pub fn new(radius: f64, tip: f64, half_angle: f64) -> Result<Self, ToolError> {
        let radius = check_radius(radius)?;
        if !(half_angle.is_finite() && half_angle > 0.0 && half_angle < std::f64::consts::FRAC_PI_2)
        {
            return Err(ToolError::BadAngle(half_angle.to_degrees() * 2.0));
        }
        if !(tip.is_finite() && tip >= 0.0 && tip < radius) {
            return Err(ToolError::BadTip { tip, radius });
        }
        Ok(Self {
            radius,
            tip,
            rise: 1.0 / half_angle.tan(),
        })
    }

    /// The flat tip's radius.
    pub fn tip(&self) -> f64 {
        self.tip
    }

    /// Height of the flank per unit of radius: `1 / tan(α)`.
    pub fn rise(&self) -> f64 {
        self.rise
    }

    /// Height above the tip where the flank reaches full radius.
    #[cfg(test)]
    pub(crate) fn flank_height(&self) -> f64 {
        (self.radius - self.tip) * self.rise
    }
}

impl ToolProfile for Cone {
    fn radius_at(&self, z: f64) -> f64 {
        if z < 0.0 {
            0.0
        } else {
            (self.tip + z / self.rise).min(self.radius)
        }
    }

    fn max_radius(&self) -> f64 {
        self.radius
    }

    fn height_at(&self, d: f64) -> Option<f64> {
        if d > self.radius {
            return None;
        }
        Some((d - self.tip).max(0.0) * self.rise)
    }
}

/// Any cutter the engine cuts with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cutter {
    /// Flat, ball and corner-radius end mills.
    Bullnose(Bullnose),
    /// V-bits, chamfer mills and drill points.
    Cone(Cone),
}

impl Cutter {
    /// The cutter as a bullnose, when it is one.
    #[cfg(test)]
    pub(crate) fn bullnose(&self) -> Option<&Bullnose> {
        match self {
            Cutter::Bullnose(b) => Some(b),
            Cutter::Cone(_) => None,
        }
    }
}

impl From<Bullnose> for Cutter {
    fn from(b: Bullnose) -> Self {
        Cutter::Bullnose(b)
    }
}

impl From<Flat> for Cutter {
    fn from(f: Flat) -> Self {
        Cutter::Bullnose(f.as_bullnose())
    }
}

impl From<Ball> for Cutter {
    fn from(b: Ball) -> Self {
        Cutter::Bullnose(b.as_bullnose())
    }
}

impl From<Cone> for Cutter {
    fn from(c: Cone) -> Self {
        Cutter::Cone(c)
    }
}

impl ToolProfile for Cutter {
    fn radius_at(&self, z: f64) -> f64 {
        match self {
            Cutter::Bullnose(t) => t.radius_at(z),
            Cutter::Cone(t) => t.radius_at(z),
        }
    }

    fn max_radius(&self) -> f64 {
        match self {
            Cutter::Bullnose(t) => t.max_radius(),
            Cutter::Cone(t) => t.max_radius(),
        }
    }

    fn height_at(&self, d: f64) -> Option<f64> {
        match self {
            Cutter::Bullnose(t) => t.height_at(d),
            Cutter::Cone(t) => t.height_at(d),
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
    /// A cone's included angle was not between 0° and 180°.
    #[error("the angle must be between 0 and 180 degrees, got {0}")]
    BadAngle(f64),
    /// A cone's tip did not fit within the tool.
    #[error(
        "the tip must be at least zero and narrower than the tool ({radius} radius), got {tip}"
    )]
    BadTip {
        /// The offending tip radius.
        tip: f64,
        /// The tool radius it had to fit inside.
        radius: f64,
    },
    /// Stock to leave was asked of a cone, which cannot be grown and stay a cone.
    #[error("a V-bit, chamfer mill or drill cuts to size: it cannot leave stock")]
    ConeStock,
}

fn check_radius(radius: f64) -> Result<f64, ToolError> {
    if is_positive(radius) {
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
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests;
