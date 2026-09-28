//! Patterns about a centre, laid out seen from above for [`crate::projected`] to
//! drop onto the part: Fusion's Spiral and Radial.
//!
//! - A spiral turns about its centre a stepover further out each lap, from its
//!   inner radius to its outer. It is one line, cut outward or inward, and turns
//!   whichever way climbs, or cuts conventional, as asked: going outward what is
//!   still to finish lies outside, going inward inside.
//! - Spokes run straight out from the centre, from the inner radius to the outer,
//!   at angles close enough that they are no more than a stepover apart at the
//!   outer radius. Back and forth, or all one way.

use std::f64::consts::TAU;

use camcraft_geom::steps::bounded_count;
use glam::DVec2;

use crate::projected::Curve;
use crate::strategy::{StrategyError, check};

/// Where a pattern about a centre lies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Centred {
    /// Its centre.
    pub centre: DVec2,
    /// Nothing nearer the centre than this.
    pub inner: f64,
    /// Nothing further out than this.
    pub outer: f64,
}

impl Centred {
    fn check(&self) -> Result<(), StrategyError> {
        check("outer radius", self.outer)?;
        if !(self.centre.is_finite() && self.inner >= 0.0 && self.inner < self.outer) {
            return Err(StrategyError::BadParameter {
                name: "inner radius",
                value: self.inner,
            });
        }
        Ok(())
    }

    /// The point `r` from the centre at angle `a`.
    fn at(&self, r: f64, a: f64) -> DVec2 {
        let (sin, cos) = a.sin_cos();
        self.centre + DVec2::new(cos, sin) * r
    }
}

/// How far the furthest corner of `bounds` is from `centre`: the outer radius
/// that reaches all of it.
pub fn reaching(centre: DVec2, (lo, hi): (DVec2, DVec2)) -> f64 {
    [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)]
        .iter()
        .map(|c| c.distance(centre))
        .fold(0.0, f64::max)
}

/// A spiral about `on`'s centre, `stepover` further out each lap, its chords
/// within `tolerance` of it: outward from the inner radius if `outward`, else
/// inward from the outer; anticlockwise if `ccw`.
pub fn spiral(
    on: &Centred,
    stepover: f64,
    tolerance: f64,
    outward: bool,
    ccw: bool,
) -> Result<Vec<Curve>, StrategyError> {
    on.check()?;
    check("stepover", stepover)?;
    check("tolerance", tolerance)?;
    // Each step turned so its chord is within a quarter of the tolerance of the
    // spiral: a chord of length √(8ρ·sag) for its radius of curvature ρ, which
    // is half the step's growth per radian at the centre and the radius far out.
    // Never more than a sixteenth of a lap.
    let sag = 0.25 * tolerance;
    let b = stepover / TAU;
    let step = |r: f64| {
        let along = r.hypot(b);
        let rho = along.powi(3) / (r * r + 2.0 * b * b);
        ((8.0 * rho * sag).sqrt() / along).min(TAU / 16.0)
    };
    let turn = (on.outer - on.inner) / stepover * TAU;
    let sense = if ccw { 1.0 } else { -1.0 };
    // Bounded: the steps are shortest at the outer radius.
    let least = step(on.outer).max(1e-9);
    let most = bounded_count(turn / least) + 2;
    let mut points = Vec::new();
    let mut a = 0.0;
    for _ in 0..most {
        let r = on.inner + b * a;
        points.push(on.at(r, sense * a));
        if a >= turn {
            break;
        }
        a = (a + step(r).max(least)).min(turn);
    }
    if !outward {
        points.reverse();
    }
    Ok(vec![Curve::open(points)])
}

/// Which way spokes are cut.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Spokes {
    /// Out and back in, one after the other.
    #[default]
    BothWays,
    /// Every one from the inner radius out.
    Outward,
    /// Every one from the outer radius in.
    Inward,
}

/// Spokes about `on`'s centre from angle `from` to `to`, in radians
/// anticlockwise from +X, no more than `stepover` apart at the outer radius; the
/// whole way round if they span a lap or more.
pub fn radial(
    on: &Centred,
    stepover: f64,
    (from, to): (f64, f64),
    spokes: Spokes,
) -> Result<Vec<Curve>, StrategyError> {
    on.check()?;
    check("stepover", stepover)?;
    if !(from.is_finite() && to.is_finite() && from < to) {
        return Err(StrategyError::BadParameter {
            name: "angle range",
            value: to,
        });
    }
    let sweep = (to - from).min(TAU);
    let whole = sweep >= TAU - 1e-9;
    let gaps = bounded_count((sweep * on.outer / stepover).ceil());
    // A lap's last spoke would be its first again.
    let count = if whole { gaps } else { gaps + 1 };
    let step = sweep / f64::from(gaps);
    Ok((0..count)
        .map(|k| {
            let a = from + f64::from(k) * step;
            let (inner, outer) = (on.at(on.inner, a), on.at(on.outer, a));
            let outward = match spokes {
                Spokes::BothWays => k % 2 == 0,
                Spokes::Outward => true,
                Spokes::Inward => false,
            };
            Curve::open(if outward {
                vec![inner, outer]
            } else {
                vec![outer, inner]
            })
        })
        .collect())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use camcraft_geom::mesh::distance_to_segment;

    fn at_origin(inner: f64, outer: f64) -> Centred {
        Centred {
            centre: DVec2::ZERO,
            inner,
            outer,
        }
    }

    #[test]
    fn a_spiral_is_a_stepover_further_out_each_lap() {
        let curves = spiral(&at_origin(0.0, 10.0), 1.5, 0.01, true, true).unwrap();
        let points = &curves[0].points;
        assert_eq!(points[0], DVec2::ZERO);
        assert!((points.last().unwrap().length() - 10.0).abs() < 1e-9);
        // Along +X, each crossing a stepover past the last.
        let mut crossings: Vec<f64> = points
            .windows(2)
            .filter(|w| w[0].y < 0.0 && w[1].y >= 0.0 && w[1].x > 0.0)
            .map(|w| w[0].x + (w[1].x - w[0].x) * (-w[0].y / (w[1].y - w[0].y)))
            .collect();
        crossings.sort_by(f64::total_cmp);
        for w in crossings.windows(2) {
            assert!((w[1] - w[0] - 1.5).abs() < 0.01, "{crossings:?}");
        }
        // Its chords within a quarter of the tolerance of it: the spiral's point
        // halfway round each, whose radius is halfway too, is that near the chord.
        for w in points.windows(2) {
            let turn = w[0].perp_dot(w[1]).atan2(w[0].dot(w[1]));
            let r = 0.5 * (w[0].length() + w[1].length());
            let a = w[0].y.atan2(w[0].x) + 0.5 * turn;
            let on = DVec2::new(a.cos(), a.sin()) * r;
            if w[0].length() > 1e-9 {
                // Curvature changes a little along a step near the centre.
                assert!(distance_to_segment(on, w[0], w[1]) <= 0.0026, "{w:?}");
            }
        }
    }

    #[test]
    fn a_spiral_turns_anticlockwise_outward_when_asked() {
        let out = &spiral(&at_origin(1.0, 5.0), 1.0, 0.01, true, true).unwrap()[0].points;
        let cross = out[0].perp_dot(out[1]);
        assert!(cross > 0.0);
        let inward = &spiral(&at_origin(1.0, 5.0), 1.0, 0.01, false, true).unwrap()[0].points;
        assert!((inward[0].length() - 5.0).abs() < 1e-9);
        assert!((inward.last().unwrap().length() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn spokes_are_no_more_than_a_stepover_apart_at_the_outer_radius() {
        let curves = radial(&at_origin(2.0, 20.0), 1.0, (0.0, TAU), Spokes::BothWays).unwrap();
        let ends: Vec<DVec2> = curves
            .iter()
            .map(|c| {
                c.points
                    .iter()
                    .copied()
                    .max_by(|a, b| a.length().total_cmp(&b.length()))
                    .unwrap()
            })
            .collect();
        for (a, b) in ends.iter().zip(ends.iter().cycle().skip(1)) {
            assert!(a.distance(*b) <= 1.0 + 1e-9);
        }
        // Back and forth.
        assert!((curves[0].points[0].length() - 2.0).abs() < 1e-9);
        assert!((curves[1].points[0].length() - 20.0).abs() < 1e-9);
        // None repeated round the lap.
        assert!(ends[0].distance(*ends.last().unwrap()) > 1e-6);
    }

    #[test]
    fn spokes_keep_to_their_angles() {
        let curves = radial(
            &at_origin(0.0, 10.0),
            1.0,
            (0.0, std::f64::consts::FRAC_PI_2),
            Spokes::Outward,
        )
        .unwrap();
        let first = curves[0].points[1];
        let last = curves.last().unwrap().points[1];
        assert!(distance_to_segment(first, DVec2::ZERO, DVec2::X * 10.0) < 1e-9);
        assert!(distance_to_segment(last, DVec2::ZERO, DVec2::Y * 10.0) < 1e-9);
        for c in &curves {
            assert_eq!(c.points[0], DVec2::ZERO);
        }
    }

    #[test]
    fn reaching_covers_the_far_corner() {
        let r = reaching(DVec2::new(1.0, 1.0), (DVec2::ZERO, DVec2::new(4.0, 5.0)));
        assert!((r - 5.0).abs() < 1e-12);
    }
}
