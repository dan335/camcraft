//! The surfaces a face can lie on that distance to is exact and cheap: planes,
//! cylinders, cones, spheres and tori.

use std::collections::BTreeMap;

use glam::{DAffine3, DVec3};

use crate::part21::{kind, numbers, placement, refs};

/// A surface from the file, where the file puts it, in millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Surface {
    /// Through `origin`, square to `normal`.
    Plane {
        /// A point on it.
        origin: DVec3,
        /// Its unit normal.
        normal: DVec3,
    },
    /// `radius` from the line through `origin` along `axis`.
    Cylinder {
        /// A point on the axis.
        origin: DVec3,
        /// The axis, a unit vector.
        axis: DVec3,
        /// The radius.
        radius: f64,
    },
    /// `radius` from the axis at `origin`, widening along `axis` at `half_angle`.
    Cone {
        /// The point on the axis where it is `radius` across.
        origin: DVec3,
        /// The axis, a unit vector, pointing the way the cone widens.
        axis: DVec3,
        /// Its radius at `origin`.
        radius: f64,
        /// The angle between its side and its axis, radians.
        half_angle: f64,
    },
    /// `radius` from `center`.
    Sphere {
        /// The centre.
        center: DVec3,
        /// The radius.
        radius: f64,
    },
    /// A tube `minor` in radius round a circle `major` in radius about `axis`.
    Torus {
        /// The centre.
        origin: DVec3,
        /// The axis, a unit vector.
        axis: DVec3,
        /// The radius of the circle the tube follows.
        major: f64,
        /// The radius of the tube.
        minor: f64,
    },
}

impl Surface {
    /// The surface entity `body`, if it is one of these, in the frame it is
    /// written in. `s` is millimetres per file length unit, `radians` radians per
    /// file angle unit.
    pub(crate) fn read(
        entities: &BTreeMap<u64, String>,
        body: &str,
        s: f64,
        radians: f64,
    ) -> Option<Self> {
        let frame = placement(entities, *refs(body).first()?, s)?;
        let (origin, axis) = (frame.translation, frame.matrix3.z_axis);
        let n = numbers(body);
        Some(match (kind(body), n.as_slice()) {
            ("PLANE", _) => Surface::Plane {
                origin,
                normal: axis,
            },
            ("CYLINDRICAL_SURFACE", [r, ..]) => Surface::Cylinder {
                origin,
                axis,
                radius: r * s,
            },
            ("CONICAL_SURFACE", [r, angle, ..]) => Surface::Cone {
                origin,
                axis,
                radius: r * s,
                half_angle: angle * radians,
            },
            ("SPHERICAL_SURFACE", [r, ..]) => Surface::Sphere {
                center: origin,
                radius: r * s,
            },
            ("TOROIDAL_SURFACE", [major, minor, ..]) => Surface::Torus {
                origin,
                axis,
                major: major * s,
                minor: minor * s,
            },
            _ => return None,
        })
    }

    /// The same surface moved by `w`, a rigid placement.
    pub(crate) fn moved(self, w: &DAffine3) -> Self {
        let p = |v: DVec3| w.transform_point3(v);
        let d = |v: DVec3| w.transform_vector3(v).normalize();
        match self {
            Surface::Plane { origin, normal } => Surface::Plane {
                origin: p(origin),
                normal: d(normal),
            },
            Surface::Cylinder {
                origin,
                axis,
                radius,
            } => Surface::Cylinder {
                origin: p(origin),
                axis: d(axis),
                radius,
            },
            Surface::Cone {
                origin,
                axis,
                radius,
                half_angle,
            } => Surface::Cone {
                origin: p(origin),
                axis: d(axis),
                radius,
                half_angle,
            },
            Surface::Sphere { center, radius } => Surface::Sphere {
                center: p(center),
                radius,
            },
            Surface::Torus {
                origin,
                axis,
                major,
                minor,
            } => Surface::Torus {
                origin: p(origin),
                axis: d(axis),
                major,
                minor,
            },
        }
    }

    /// The apex of a cone, which a face can reach with no point in the file to
    /// mark it.
    pub(crate) fn apex(&self) -> Option<DVec3> {
        match *self {
            Surface::Cone {
                origin,
                axis,
                radius,
                half_angle,
            } if half_angle.tan().abs() > 1e-12 => Some(origin - radius / half_angle.tan() * axis),
            _ => None,
        }
    }

    /// How far `q` is from the surface, taken whole: every surface here is
    /// unbounded, so a face is held to its box as well.
    pub fn distance(&self, q: DVec3) -> f64 {
        // Along the axis and out from it.
        let meridian = |origin: DVec3, axis: DVec3| {
            let d = q - origin;
            let z = d.dot(axis);
            (z, (d - z * axis).length())
        };
        match *self {
            Surface::Plane { origin, normal } => (q - origin).dot(normal).abs(),
            Surface::Cylinder {
                origin,
                axis,
                radius,
            } => (meridian(origin, axis).1 - radius).abs(),
            Surface::Cone {
                origin,
                axis,
                radius,
                half_angle,
            } => {
                // The side is the line r = radius + z tan(half_angle) in the plane
                // through the axis; past the apex it is the same line's mirror.
                let (z, r) = meridian(origin, axis);
                let (sin, cos) = half_angle.sin_cos();
                ((r - radius) * cos - z * sin)
                    .abs()
                    .min(((r + radius) * cos + z * sin).abs())
            }
            Surface::Sphere { center, radius } => (q.distance(center) - radius).abs(),
            Surface::Torus {
                origin,
                axis,
                major,
                minor,
            } => {
                let (z, r) = meridian(origin, axis);
                ((r - major).hypot(z) - minor).abs()
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use glam::dvec3;

    use super::*;

    #[test]
    fn distances_are_exact() {
        let cylinder = Surface::Cylinder {
            origin: DVec3::ZERO,
            axis: DVec3::Z,
            radius: 10.0,
        };
        assert!((cylinder.distance(dvec3(13.0, 0.0, 50.0)) - 3.0).abs() < 1e-12);
        assert!((cylinder.distance(dvec3(0.0, 4.0, -2.0)) - 6.0).abs() < 1e-12);

        // 10 across at the base, to a point 30 up: placed at the base, widening
        // downwards.
        let cone = Surface::Cone {
            origin: DVec3::ZERO,
            axis: -DVec3::Z,
            radius: 10.0,
            half_angle: (10.0f64 / 30.0).atan(),
        };
        assert!(
            cone.distance(dvec3(5.0, 0.0, 15.0)) < 1e-12,
            "half way up the side"
        );
        assert!(cone.apex().unwrap().distance(dvec3(0.0, 0.0, 30.0)) < 1e-12);
        // Up the side from the base, and out from it.
        let along = DVec3::new(-10.0, 0.0, 30.0).normalize();
        let out = DVec3::new(30.0, 0.0, 10.0).normalize();
        let p = dvec3(10.0, 0.0, 0.0) + 5.0 * along + 2.0 * out;
        assert!((cone.distance(p) - 2.0).abs() < 1e-9);

        let torus = Surface::Torus {
            origin: DVec3::ZERO,
            axis: DVec3::Z,
            major: 20.0,
            minor: 5.0,
        };
        assert!((torus.distance(dvec3(20.0, 0.0, 7.0)) - 2.0).abs() < 1e-12);
        assert!((torus.distance(DVec3::ZERO) - 15.0).abs() < 1e-12);
    }
}
