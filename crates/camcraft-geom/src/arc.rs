//! Circular arcs in space, helices among them: the one curve a path follows
//! besides a straight line.
//!
//! An [`Arc`] turns about an axis through `centre`, from wherever it starts to
//! where it ends, through `sweep` radians, counter-clockwise about `axis` (seen
//! from its tip) when positive. It moves along the axis at a steady rate as it
//! turns, so an arc whose ends are at different heights along its axis is a
//! helix. Its ends are not part of it: a path's move carries where it goes, and
//! starts where the move before it ended, so the two can never disagree.
//!
//! A level arc turns about Z; a lead in a vertical plane turns about a level axis.
//! Nothing here knows which planes a controller can cut in: that is the post's to
//! decide.

use std::f64::consts::{FRAC_PI_2, TAU};

use glam::DVec3;

/// A circular arc or a helix, less its ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arc {
    /// A point on the axis it turns about.
    pub centre: DVec3,
    /// The axis, a unit vector.
    pub axis: DVec3,
    /// How far it turns, in radians: counter-clockwise about `axis` when positive.
    /// At most a full turn either way.
    pub sweep: f64,
}

/// `v` turned `angle` about the unit `axis`; `v` must be square to it.
fn turned(v: DVec3, axis: DVec3, angle: f64) -> DVec3 {
    let (s, c) = angle.sin_cos();
    v * c + axis.cross(v) * s
}

/// How many chords, none turning more than a quarter turn nor straying more than
/// `tolerance` from the circle of `radius`, span a turn of `sweep` radians: at
/// least one, at most 100 000, and one for a sweep that is not finite.
pub(crate) fn chord_count(radius: f64, sweep: f64, tolerance: f64) -> u32 {
    // The largest angle whose chord strays no more than the tolerance.
    let widest = if tolerance < radius {
        2.0 * (1.0 - tolerance / radius).acos()
    } else {
        FRAC_PI_2
    };
    let widest = widest.clamp(1.0e-3, FRAC_PI_2);
    let n = (sweep.abs() / widest).ceil().max(1.0);
    let n = if n.is_finite() { n.min(1.0e5) } else { 1.0 };
    // At least one and at most 100 000, so the narrowing is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = n as u32;
    n
}

impl Arc {
    /// The arc about `axis` through `centre` from `from` to `to`, turning
    /// counter-clockwise about the axis when `ccw`, the short way or the long way
    /// round as that direction requires. Ends that coincide, seen along the axis,
    /// make a full turn.
    ///
    /// `None` where the ends are not the same distance from the axis, within
    /// `tolerance`, or are on it.
    pub fn about(
        centre: DVec3,
        axis: DVec3,
        from: DVec3,
        to: DVec3,
        ccw: bool,
        tolerance: f64,
    ) -> Option<Self> {
        let axis = axis.try_normalize()?;
        let radial = |p: DVec3| {
            let r = p - centre;
            r - axis * r.dot(axis)
        };
        let (a, b) = (radial(from), radial(to));
        let (ra, rb) = (a.length(), b.length());
        if ra <= tolerance || (ra - rb).abs() > tolerance {
            return None;
        }
        let mut angle = axis.dot(a.cross(b)).atan2(a.dot(b));
        if a.distance(b) <= tolerance {
            angle = 0.0;
        }
        let sweep = if ccw {
            if angle <= 0.0 { angle + TAU } else { angle }
        } else if angle >= 0.0 {
            angle - TAU
        } else {
            angle
        };
        Some(Self {
            centre,
            axis,
            sweep,
        })
    }

    /// Where `p` is from the axis, square to it, and how far along it.
    fn frame(&self, p: DVec3) -> (DVec3, f64) {
        let r = p - self.centre;
        let along = r.dot(self.axis);
        (r - self.axis * along, along)
    }

    /// Its radius, starting at `from`.
    pub fn radius(&self, from: DVec3) -> f64 {
        self.frame(from).0.length()
    }

    /// The point a fraction `t` of the way along it, from `from` to `to`.
    pub fn point_at(&self, from: DVec3, to: DVec3, t: f64) -> DVec3 {
        let (r0, a0) = self.frame(from);
        let (_, a1) = self.frame(to);
        self.point_in(r0, a0, a1, t)
    }

    /// The point a fraction `t` of the way along it, from where it starts at `r0`
    /// from the axis and `a0` along it, to `a1` along it.
    fn point_in(&self, r0: DVec3, a0: f64, a1: f64, t: f64) -> DVec3 {
        self.centre + turned(r0, self.axis, self.sweep * t) + self.axis * (a0 + (a1 - a0) * t)
    }

    /// The direction it runs in a fraction `t` of the way along, a unit vector.
    pub fn tangent_at(&self, from: DVec3, to: DVec3, t: f64) -> DVec3 {
        let (r0, a0) = self.frame(from);
        let (_, a1) = self.frame(to);
        let r = turned(r0, self.axis, self.sweep * t);
        let d = self.axis.cross(r) * self.sweep + self.axis * (a1 - a0);
        d.try_normalize().unwrap_or(self.axis)
    }

    /// Its length, from `from` to `to`.
    pub fn length(&self, from: DVec3, to: DVec3) -> f64 {
        let (r0, a0) = self.frame(from);
        let (_, a1) = self.frame(to);
        (r0.length() * self.sweep).hypot(a1 - a0)
    }

    /// Whether `to` is where it ends, from `from`, within `tolerance`.
    pub fn ends_at(&self, from: DVec3, to: DVec3, tolerance: f64) -> bool {
        self.point_at(from, to, 1.0).distance(to) <= tolerance
    }

    /// Points along it within `tolerance` of it, from `from`: the start left out,
    /// `to` last, exactly. No chord spans more than a quarter turn.
    pub fn chords(&self, from: DVec3, to: DVec3, tolerance: f64) -> Vec<DVec3> {
        let (r0, a0) = self.frame(from);
        let (_, a1) = self.frame(to);
        let n = chord_count(r0.length(), self.sweep, tolerance);
        let mut out: Vec<DVec3> = (1..n)
            .map(|k| self.point_in(r0, a0, a1, f64::from(k) / f64::from(n)))
            .collect();
        out.push(to);
        out
    }

    /// The same arc run backwards: from `to` back to `from`.
    pub fn reversed(&self) -> Self {
        Self {
            sweep: -self.sweep,
            ..*self
        }
    }

    /// The two arcs it is cut into a fraction `t` of the way along, from `from` to
    /// `to`, and the point between them.
    pub fn split(&self, from: DVec3, to: DVec3, t: f64) -> (Self, DVec3, Self) {
        let mid = self.point_at(from, to, t);
        (
            Self {
                sweep: self.sweep * t,
                ..*self
            },
            mid,
            Self {
                sweep: self.sweep * (1.0 - t),
                ..*self
            },
        )
    }

    /// The same arc lifted `dz` along Z: its centre moves with it.
    pub fn raised(&self, dz: f64) -> Self {
        Self {
            centre: self.centre + DVec3::Z * dz,
            ..*self
        }
    }

    /// Whether the arc's shape is usable: finite, a unit axis, a real turn of at
    /// most one revolution, and a radius at `from` over `tolerance`.
    pub fn is_usable(&self, from: DVec3, tolerance: f64) -> bool {
        self.centre.is_finite()
            && self.axis.is_finite()
            && (self.axis.length() - 1.0).abs() < 1.0e-9
            && self.sweep.is_finite()
            && self.sweep != 0.0
            && self.sweep.abs() <= TAU + 1.0e-9
            && self.radius(from) > tolerance
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;
    use glam::dvec3;
    use std::f64::consts::PI;

    fn close(a: DVec3, b: DVec3) -> bool {
        a.distance(b) < 1.0e-9
    }

    /// A quarter turn about Z runs counter-clockwise from +X to +Y, and back
    /// the long way round when clockwise.
    #[test]
    fn a_level_arc_turns_the_way_asked() {
        let (from, to) = (dvec3(1.0, 0.0, 0.0), dvec3(0.0, 1.0, 0.0));
        let ccw = Arc::about(DVec3::ZERO, DVec3::Z, from, to, true, 1e-9).unwrap();
        assert!((ccw.sweep - PI / 2.0).abs() < 1e-12);
        let cw = Arc::about(DVec3::ZERO, DVec3::Z, from, to, false, 1e-9).unwrap();
        assert!((cw.sweep + 3.0 * PI / 2.0).abs() < 1e-12);
        assert!(close(
            ccw.point_at(from, to, 0.5),
            dvec3(0.5f64.sqrt(), 0.5f64.sqrt(), 0.0)
        ));
        assert!(close(
            cw.point_at(from, to, 1.0 / 3.0),
            dvec3(0.0, -1.0, 0.0)
        ));
        assert!(ccw.ends_at(from, to, 1e-9) && cw.ends_at(from, to, 1e-9));
        assert!(close(ccw.tangent_at(from, to, 0.0), DVec3::Y));
        assert!((ccw.length(from, to) - PI / 2.0).abs() < 1e-12);
    }

    /// Ends that coincide make a full turn; ends at different distances from the
    /// axis make no arc.
    #[test]
    fn full_turns_and_mismatched_ends() {
        let p = dvec3(2.0, 0.0, 0.0);
        let full = Arc::about(DVec3::ZERO, DVec3::Z, p, p, false, 1e-9).unwrap();
        assert!((full.sweep + TAU).abs() < 1e-12);
        assert!(close(full.point_at(p, p, 0.5), dvec3(-2.0, 0.0, 0.0)));
        assert!(Arc::about(DVec3::ZERO, DVec3::Z, p, dvec3(0.0, 3.0, 0.0), true, 1e-6).is_none());
    }

    /// A helix climbs steadily along its axis as it turns, and is longer than its
    /// plan by the climb.
    #[test]
    fn a_helix_climbs_as_it_turns() {
        let (from, to) = (dvec3(1.0, 0.0, 0.0), dvec3(1.0, 0.0, -2.0));
        let h = Arc {
            centre: DVec3::ZERO,
            axis: DVec3::Z,
            sweep: TAU,
        };
        assert!(close(h.point_at(from, to, 0.25), dvec3(0.0, 1.0, -0.5)));
        assert!((h.length(from, to) - TAU.hypot(2.0)).abs() < 1e-12);
        let t = h.tangent_at(from, to, 0.0);
        assert!(t.z < 0.0 && t.y > 0.0, "{t}");
    }

    /// An arc in a vertical plane, as a lead: about a level axis.
    #[test]
    fn a_vertical_arc() {
        // Down from above-and-behind onto the origin, heading +X: a quarter turn
        // clockwise about +Y, which turns +X down towards -Z.
        let (from, to) = (dvec3(-1.0, 0.0, 1.0), DVec3::ZERO);
        let a = Arc::about(dvec3(0.0, 0.0, 1.0), DVec3::Y, from, to, false, 1e-9).unwrap();
        assert!((a.sweep + PI / 2.0).abs() < 1e-12, "{}", a.sweep);
        assert!(close(a.tangent_at(from, to, 1.0), DVec3::X));
        assert!(close(a.tangent_at(from, to, 0.0), -DVec3::Z));
    }

    /// Chords stay within the tolerance of the arc and end exactly at its end.
    #[test]
    fn chords_stay_within_the_tolerance() {
        let (from, to) = (dvec3(10.0, 0.0, 0.0), dvec3(-10.0, 0.0, 0.0));
        let a = Arc::about(DVec3::ZERO, DVec3::Z, from, to, true, 1e-9).unwrap();
        let pts = a.chords(from, to, 0.01);
        assert_eq!(*pts.last().unwrap(), to);
        let mut at = from;
        for p in pts {
            let mid = (at + p) * 0.5;
            assert!(10.0 - mid.length() <= 0.01 + 1e-12);
            assert!((p.length() - 10.0).abs() < 1e-9);
            at = p;
        }
    }

    /// Split in two, the halves meet where the whole passes, and reversed it
    /// runs back over itself.
    #[test]
    fn splitting_and_reversing() {
        let (from, to) = (dvec3(1.0, 0.0, 0.0), dvec3(-1.0, 0.0, 1.0));
        let a = Arc {
            centre: DVec3::ZERO,
            axis: DVec3::Z,
            sweep: PI,
        };
        let (first, mid, second) = a.split(from, to, 0.25);
        assert!(close(mid, a.point_at(from, to, 0.25)));
        assert!(first.ends_at(from, mid, 1e-9));
        assert!(second.ends_at(mid, to, 1e-9));
        assert!(close(
            second.point_at(mid, to, 0.5),
            a.point_at(from, to, 0.625)
        ));
        let back = a.reversed();
        assert!(back.ends_at(to, from, 1e-9));
        assert!(close(
            back.point_at(to, from, 0.5),
            a.point_at(from, to, 0.5)
        ));
    }
}
