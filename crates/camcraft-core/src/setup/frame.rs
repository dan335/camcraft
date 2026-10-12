//! How a setup holds the part: its orientation on the table, and where its work
//! zero is.
//!
//! Models are drawn in their own (design) coordinates. A setup turns them so that
//! a chosen model direction points up the spindle, then measures everything from
//! its work zero. Everything past that point (strategies, the linker, the post)
//! works in the setup frame and never sees a rotation.
//!
//! Orientations are limited to the 24 that map axes onto axes, the ways a block
//! can sit square on a 3-axis table. So a box stays a box, exactly, and turning a
//! point only swaps and negates its coordinates: no rounding, and the identity
//! orientation changes nothing bit for bit.

use glam::{DMat4, DVec3, DVec4};
use serde::{Deserialize, Serialize};

/// A model axis, either way along it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Axis {
    /// +X.
    #[serde(rename = "+x")]
    PosX,
    /// −X.
    #[serde(rename = "-x")]
    NegX,
    /// +Y.
    #[serde(rename = "+y")]
    PosY,
    /// −Y.
    #[serde(rename = "-y")]
    NegY,
    /// +Z.
    #[serde(rename = "+z")]
    PosZ,
    /// −Z.
    #[serde(rename = "-z")]
    NegZ,
}

impl Axis {
    /// Every axis, in the order the app lists them.
    pub const ALL: [Axis; 6] = [
        Axis::PosX,
        Axis::NegX,
        Axis::PosY,
        Axis::NegY,
        Axis::PosZ,
        Axis::NegZ,
    ];

    /// Which coordinate (0, 1 or 2) and which way.
    fn parts(self) -> (usize, f64) {
        match self {
            Axis::PosX => (0, 1.0),
            Axis::NegX => (0, -1.0),
            Axis::PosY => (1, 1.0),
            Axis::NegY => (1, -1.0),
            Axis::PosZ => (2, 1.0),
            Axis::NegZ => (2, -1.0),
        }
    }

    fn from_parts(i: usize, sign: f64) -> Axis {
        match (i, sign > 0.0) {
            (0, true) => Axis::PosX,
            (0, false) => Axis::NegX,
            (1, true) => Axis::PosY,
            (1, false) => Axis::NegY,
            (2, true) => Axis::PosZ,
            _ => Axis::NegZ,
        }
    }

    /// The unit vector.
    pub fn vector(self) -> DVec3 {
        let (i, s) = self.parts();
        let mut v = [0.0; 3];
        if let Some(slot) = v.get_mut(i) {
            *slot = s;
        }
        DVec3::from_array(v)
    }

    /// The same axis, the other way.
    pub fn flipped(self) -> Axis {
        let (i, s) = self.parts();
        Axis::from_parts(i, -s)
    }

    /// How the app names it.
    pub fn label(self) -> &'static str {
        match self {
            Axis::PosX => "+X",
            Axis::NegX => "−X",
            Axis::PosY => "+Y",
            Axis::NegY => "−Y",
            Axis::PosZ => "+Z",
            Axis::NegZ => "−Z",
        }
    }
}

/// The two setup axes given as model directions: `z` points up the spindle, away
/// from the table, and `x` along the machine's X. Y follows, as `z × x`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Orientation {
    /// The model direction that becomes the setup's +Z.
    pub z: Axis,
    /// The model direction that becomes the setup's +X.
    pub x: Axis,
}

impl Default for Orientation {
    /// As modelled.
    fn default() -> Self {
        Self {
            z: Axis::PosZ,
            x: Axis::PosX,
        }
    }
}

/// An orientation whose two axes are not square to each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the X axis ({x:?}) must be square to the Z axis ({z:?})")]
pub struct BadOrientation {
    /// The Z axis given.
    pub z: Axis,
    /// The X axis given.
    pub x: Axis,
}

impl Orientation {
    /// The part turned over about X, for machining its underside.
    pub fn flipped(self) -> Self {
        Self {
            z: self.z.flipped(),
            x: self.x,
        }
    }

    /// Whether the axes are square, as every orientation must be.
    pub fn check(self) -> Result<(), BadOrientation> {
        if self.z.parts().0 == self.x.parts().0 {
            Err(BadOrientation {
                z: self.z,
                x: self.x,
            })
        } else {
            Ok(())
        }
    }

    /// The setup's Y, as a model direction.
    pub fn y(self) -> Axis {
        let y = self.z.vector().cross(self.x.vector());
        let (i, s) = if y.x != 0.0 {
            (0, y.x)
        } else if y.y != 0.0 {
            (1, y.y)
        } else {
            (2, y.z)
        };
        Axis::from_parts(i, s)
    }

    /// A model point in the orientation's axes. Only swaps and negates, so it is
    /// exact. Meaningless unless [`check`](Self::check) passes.
    pub fn apply(self, p: DVec3) -> DVec3 {
        let pick = |a: Axis| {
            let (i, s) = a.parts();
            let v = match i {
                0 => p.x,
                1 => p.y,
                _ => p.z,
            };
            if s > 0.0 { v } else { -v }
        };
        DVec3::new(pick(self.x), pick(self.y()), pick(self.z))
    }

    /// Undo [`apply`](Self::apply).
    pub fn inverse(self, q: DVec3) -> DVec3 {
        let mut p = [0.0; 3];
        for (a, v) in [(self.x, q.x), (self.y(), q.y), (self.z, q.z)] {
            let (i, s) = a.parts();
            if let Some(slot) = p.get_mut(i) {
                *slot = if s > 0.0 { v } else { -v };
            }
        }
        DVec3::from_array(p)
    }
}

/// A setup's frame: model coordinates turned by `orientation`, then measured from
/// `origin` (a point in the turned coordinates).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// How the part sits.
    pub orientation: Orientation,
    /// The work zero, in turned model coordinates.
    pub origin: DVec3,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            orientation: Orientation::default(),
            origin: DVec3::ZERO,
        }
    }
}

impl Frame {
    /// A model point in the setup frame.
    pub fn to_setup(&self, p: DVec3) -> DVec3 {
        self.orientation.apply(p) - self.origin
    }

    /// A setup point in model coordinates.
    pub fn to_model(&self, q: DVec3) -> DVec3 {
        self.orientation.inverse(q + self.origin)
    }

    /// [`to_setup`](Self::to_setup) as a matrix, for drawing.
    pub fn matrix(&self) -> DMat4 {
        let o = self.orientation;
        let row = |a: Axis| a.vector();
        let (x, y, z) = (row(o.x), row(o.y()), row(o.z));
        // Rows are the setup axes; columns are what glam stores.
        DMat4::from_cols(
            DVec4::new(x.x, y.x, z.x, 0.0),
            DVec4::new(x.y, y.y, z.y, 0.0),
            DVec4::new(x.z, y.z, z.z, 0.0),
            (-self.origin).extend(1.0),
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;
    use glam::dvec3;

    fn all() -> Vec<Orientation> {
        let mut out = Vec::new();
        for z in Axis::ALL {
            for x in Axis::ALL {
                let o = Orientation { z, x };
                if o.check().is_ok() {
                    out.push(o);
                }
            }
        }
        out
    }

    #[test]
    fn there_are_24_proper_orientations() {
        let all = all();
        assert_eq!(all.len(), 24);
        for o in all {
            let m = Frame {
                orientation: o,
                origin: DVec3::ZERO,
            }
            .matrix();
            assert_eq!(m.determinant(), 1.0, "{o:?} is not a rotation");
        }
    }

    #[test]
    fn apply_matches_the_matrix_and_inverse_undoes_it() {
        let p = dvec3(1.25, -7.5, 3.0625);
        for o in all() {
            let f = Frame {
                orientation: o,
                origin: dvec3(0.5, -2.0, 4.0),
            };
            let q = f.to_setup(p);
            assert_eq!(f.matrix().transform_point3(p), q, "{o:?}");
            assert_eq!(f.to_model(q), p, "{o:?}");
        }
    }

    #[test]
    fn the_default_changes_nothing() {
        let p = dvec3(0.1, 0.2, 0.3);
        assert_eq!(Orientation::default().apply(p), p);
        assert_eq!(Frame::default().to_setup(p), p);
    }

    #[test]
    fn flipped_puts_the_underside_up() {
        let o = Orientation::default().flipped();
        assert!(o.check().is_ok());
        let q = o.apply(dvec3(1.0, 2.0, 3.0));
        assert_eq!(q, dvec3(1.0, -2.0, -3.0));
    }

    #[test]
    fn parallel_axes_are_refused() {
        for (z, x) in [(Axis::PosZ, Axis::NegZ), (Axis::PosX, Axis::PosX)] {
            assert!(Orientation { z, x }.check().is_err());
        }
    }

    #[test]
    fn orientations_read_and_write_as_signed_axes() {
        let o = Orientation {
            z: Axis::NegZ,
            x: Axis::PosY,
        };
        let text = toml::to_string(&o).unwrap();
        assert_eq!(text, "z = \"-z\"\nx = \"+y\"\n");
        assert_eq!(toml::from_str::<Orientation>(&text).unwrap(), o);
    }
}
