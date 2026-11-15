//! A turn in the plane about the origin, as a part laid on a sheet is turned
//! about Z.
//!
//! A quarter turn is exact. Its sine and cosine are exactly 0 and ±1, and it
//! is applied by swapping and negating coordinates, never by multiplying:
//! `sin` and `cos` of a quarter of π, rounded as π is, are not exactly 0 and
//! 1, and multiplying even by exact ones turns a −0 into a 0 and an infinity
//! into a NaN. So a part turned by quarter turns stands exactly where its
//! outline says, on every machine, and no turn at all leaves every point as
//! it was, bit for bit.

use glam::{DVec2, dvec2};
use std::hash::{Hash, Hasher};

/// A turn anticlockwise about the origin, seen from above.
///
/// Two turns are equal when they turn by the same angle, taken from 0 up to
/// 360 degrees, so 450 degrees is 90. Equality and hashing go by that
/// angle's bits, so a turn can be part of a key.
#[derive(Debug, Clone, Copy)]
pub struct Turn {
    /// The angle in degrees, at least 0 and less than 360, never −0.
    degrees: f64,
    /// How many quarter turns, where it is a whole number of them.
    quarters: Option<u8>,
    sin: f64,
    cos: f64,
}

impl Turn {
    /// No turn.
    pub const NONE: Self = Self {
        degrees: 0.0,
        quarters: Some(0),
        sin: 0.0,
        cos: 1.0,
    };

    /// A turn by `degrees`, anticlockwise; `None` for one that is not finite.
    pub fn from_degrees(degrees: f64) -> Option<Self> {
        if !degrees.is_finite() {
            return None;
        }
        // The remainder is exact. A tiny negative angle comes back as 360, which
        // is no turn; and adding 0 makes a −0 a 0, so equal angles have equal bits.
        let mut d = degrees.rem_euclid(360.0) + 0.0;
        if d >= 360.0 {
            d = 0.0;
        }
        // Only an angle of exactly a whole number of quarters turns exactly.
        #[allow(clippy::float_cmp)]
        let quarters = [0.0, 90.0, 180.0, 270.0]
            .iter()
            .position(|&q| d == q)
            .and_then(|q| u8::try_from(q).ok());
        let (sin, cos) = match quarters {
            Some(0) => (0.0, 1.0),
            Some(1) => (1.0, 0.0),
            Some(2) => (0.0, -1.0),
            Some(3) => (-1.0, 0.0),
            _ => d.to_radians().sin_cos(),
        };
        Some(Self {
            degrees: d,
            quarters,
            sin,
            cos,
        })
    }

    /// The angle, in degrees: at least 0 and less than 360.
    pub fn degrees(&self) -> f64 {
        self.degrees
    }

    /// Its sine and cosine: exactly 0 and ±1 for a quarter turn.
    pub fn sin_cos(&self) -> (f64, f64) {
        (self.sin, self.cos)
    }

    /// Whether it is no turn at all.
    pub fn is_none(&self) -> bool {
        self.quarters == Some(0)
    }

    /// `p` turned. A whole number of quarter turns swaps and negates its
    /// coordinates, and no turn gives back `p` itself.
    pub fn apply(&self, p: DVec2) -> DVec2 {
        match self.quarters {
            Some(0) => p,
            Some(1) => dvec2(-p.y, p.x),
            Some(2) => -p,
            Some(3) => dvec2(p.y, -p.x),
            _ => dvec2(
                self.cos * p.x - self.sin * p.y,
                self.sin * p.x + self.cos * p.y,
            ),
        }
    }

    /// `p` turned back: what [`apply`](Self::apply) turns to `p`. A whole
    /// number of quarter turns swaps and negates back, and no turn gives back
    /// `p` itself.
    pub fn undo(&self, p: DVec2) -> DVec2 {
        match self.quarters {
            Some(0) => p,
            Some(1) => dvec2(p.y, -p.x),
            Some(2) => -p,
            Some(3) => dvec2(-p.y, p.x),
            _ => dvec2(
                self.cos * p.x + self.sin * p.y,
                -self.sin * p.x + self.cos * p.y,
            ),
        }
    }
}

impl Default for Turn {
    fn default() -> Self {
        Self::NONE
    }
}

impl PartialEq for Turn {
    fn eq(&self, other: &Self) -> bool {
        self.degrees.to_bits() == other.degrees.to_bits()
    }
}

impl Eq for Turn {}

impl Hash for Turn {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.degrees.to_bits().hash(state);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn turn(degrees: f64) -> Turn {
        Turn::from_degrees(degrees).unwrap()
    }

    /// Every quarter turn has an exact sine and cosine, and turns a point by
    /// swapping and negating it, whatever way round the angle was given.
    #[test]
    fn a_quarter_turn_is_exact() {
        let p = dvec2(0.1, -7.3);
        for (degrees, sin_cos, to) in [
            (0.0, (0.0, 1.0), p),
            (90.0, (1.0, 0.0), dvec2(7.3, 0.1)),
            (180.0, (0.0, -1.0), dvec2(-0.1, 7.3)),
            (270.0, (-1.0, 0.0), dvec2(-7.3, -0.1)),
            (-90.0, (-1.0, 0.0), dvec2(-7.3, -0.1)),
            (450.0, (1.0, 0.0), dvec2(7.3, 0.1)),
            (-720.0, (0.0, 1.0), p),
        ] {
            let t = turn(degrees);
            assert_eq!(t.sin_cos(), sin_cos, "{degrees}");
            assert_eq!(t.apply(p), to, "{degrees}");
        }
    }

    /// No turn gives back the very point, bits and all: a −0 stays −0 and an
    /// infinity stays one, as no arithmetic would leave them.
    #[test]
    fn no_turn_leaves_a_point_as_it_was() {
        let p = dvec2(-0.0, f64::INFINITY);
        let back = Turn::NONE.apply(p);
        assert_eq!(back.x.to_bits(), p.x.to_bits());
        assert_eq!(back.y, f64::INFINITY);
        assert!(Turn::NONE.is_none() && turn(360.0).is_none() && turn(-0.0).is_none());
        assert_eq!(Turn::default(), Turn::NONE);
    }

    /// Any other angle turns by its sine and cosine, keeping the distance
    /// from the origin.
    #[test]
    fn a_free_angle_turns_by_its_sine_and_cosine() {
        let t = turn(30.0);
        let q = t.apply(dvec2(2.0, 0.0));
        assert!((q - dvec2(3f64.sqrt(), 1.0)).length() < 1e-12, "{q}");
        assert_eq!(t.degrees(), 30.0);
        assert_eq!(turn(-30.0).degrees(), 330.0);
    }

    /// Undoing a turn brings a point back: exactly for a quarter turn, and to
    /// within rounding for any other.
    #[test]
    fn a_turn_undone_brings_a_point_back() {
        let p = dvec2(0.1, -7.3);
        for degrees in [0.0, 90.0, 180.0, 270.0] {
            let t = turn(degrees);
            assert_eq!(t.undo(t.apply(p)), p, "{degrees}");
            assert_eq!(t.apply(t.undo(p)), p, "{degrees}");
        }
        let t = turn(37.5);
        assert!((t.undo(t.apply(p)) - p).length() < 1e-12);
        let back = Turn::NONE.undo(dvec2(-0.0, 1.0));
        assert_eq!(back.x.to_bits(), (-0.0f64).to_bits());
    }

    /// Turns are equal, and hash alike, by their angle from 0 to 360.
    #[test]
    fn turns_are_keys_by_their_angle() {
        let set: HashSet<Turn> = [turn(90.0), turn(450.0), turn(-270.0), turn(15.0)]
            .into_iter()
            .collect();
        assert_eq!(set.len(), 2);
        assert_eq!(turn(-0.0), turn(0.0));
        assert_ne!(turn(15.0), turn(15.000_000_1));
        // A tiny negative angle is a whole turn back round: no turn.
        assert_eq!(turn(-1e-300), Turn::NONE);
        assert!(Turn::from_degrees(f64::NAN).is_none());
        assert!(Turn::from_degrees(f64::INFINITY).is_none());
    }
}
