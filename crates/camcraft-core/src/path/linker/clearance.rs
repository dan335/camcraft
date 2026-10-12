//! How high the tool crosses between cuts.

use camcraft_geom::heightmap::{HeightMap, cell_index};
use camcraft_geom::mesh::distance_to_segment;
use glam::DVec2;

use super::{LinkError, NOT_NEGATIVE};
use crate::is_positive;

/// What a height must be.
const FINITE: &str = "finite";

/// How high the tool must be to cross safely.
pub trait Clearance {
    /// Safe tool-tip Z for a traverse from `from` to `to` with a tool of the given
    /// radius.
    ///
    /// The radius is supplied so an implementation can account for the tool's
    /// footprint sweeping past an obstacle, not just its axis.
    fn safe_z(&self, from: DVec2, to: DVec2, tool_radius: f64) -> f64;
}

/// A single horizontal plane, high enough to clear everything.
///
/// Sufficient when the only thing to avoid is the part itself on a flat bed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClearancePlane {
    z: f64,
}

impl ClearancePlane {
    /// A plane at absolute height `z`.
    pub fn new(z: f64) -> Result<Self, LinkError> {
        z.is_finite()
            .then_some(Self { z })
            .ok_or(LinkError::BadParameter {
                name: "clearance",
                must: FINITE,
                value: z,
            })
    }

    /// The plane height.
    pub fn z(&self) -> f64 {
        self.z
    }
}

impl Clearance for ClearancePlane {
    fn safe_z(&self, _from: DVec2, _to: DVec2, _tool_radius: f64) -> f64 {
        self.z
    }
}

/// Clearance taken from a height field, plus a margin.
///
/// This is what [`Clearance`] existed for. A plane must clear the tallest thing on
/// the table everywhere, so every traverse flies at the height of the worst obstacle
/// on the part. A field answers per traverse, so the tool stays low where it can.
///
/// The height is sampled **along** the traverse and across the tool's own width,
/// because the cutter is not a point: a rapid whose axis misses a boss can still put
/// the cutter's shoulder through it.
#[derive(Debug, Clone)]
pub struct ClearanceField {
    field: HeightMap,
    margin: f64,
    fallback: f64,
    floor: f64,
}

impl ClearanceField {
    /// Clear `field` by `margin`, falling back to `fallback` where the field has no
    /// data.
    ///
    /// The fallback matters: an unsampled column is an unknown height, and flying
    /// low over an unknown is exactly the wrong guess.
    pub fn new(field: HeightMap, margin: f64, fallback: f64) -> Result<Self, LinkError> {
        if !margin.is_finite() || margin < 0.0 {
            return Err(LinkError::BadParameter {
                name: "margin",
                must: NOT_NEGATIVE,
                value: margin,
            });
        }
        if !fallback.is_finite() {
            return Err(LinkError::BadParameter {
                name: "fallback",
                must: FINITE,
                value: fallback,
            });
        }
        Ok(Self {
            field,
            margin,
            fallback,
            floor: f64::MIN,
        })
    }

    /// Never lower than `z`: a full retraction, to the retract height, or
    /// higher where the material is.
    pub fn at_least(self, z: f64) -> Self {
        Self { floor: z, ..self }
    }
}

impl Clearance for ClearanceField {
    /// The highest material under everything the cutter sweeps: every cell whose
    /// column the tool's disc can reach anywhere along the traverse, not a handful of
    /// probes. A rapid running tangent to a terrace passes material on its diagonal,
    /// and probes on the axis and the four compass points never see it.
    fn safe_z(&self, from: DVec2, to: DVec2, tool_radius: f64) -> f64 {
        // A cell's value stands for its whole square, so the disc reaches it if it
        // reaches its farthest corner.
        let r = if is_positive(tool_radius) {
            tool_radius
        } else {
            0.0
        };
        let reach = r + self.field.pitch() * std::f64::consts::FRAC_1_SQRT_2;
        let under = highest_under(&self.field, from, to, reach);
        let from_field = under.top.map_or(f64::MIN, |t| t + self.margin);
        // Anywhere the field is silent, the fallback governs. Never fly under an
        // unknown.
        let z = if under.unknown || from_field <= f64::MIN {
            from_field.max(self.fallback)
        } else {
            from_field
        };
        z.max(self.floor)
    }
}

/// What a height field has under a disc swept along a line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Under {
    /// The highest height anywhere under it, if it is over any cell with one.
    pub top: Option<f64>,
    /// Whether any of it is off the field, or over a cell with no height.
    pub unknown: bool,
}

/// The highest cell of `field` whose centre is within `reach` of the line from
/// `from` to `to`.
///
/// Within the tool's radius is the cells the simulator cuts and checks a rapid
/// against; a traverse that must clear every cell it could touch asks for more.
pub fn highest_under(field: &HeightMap, from: DVec2, to: DVec2, reach: f64) -> Under {
    let reach = if reach.is_finite() && reach > 0.0 {
        reach
    } else {
        0.0
    };
    let f = field;
    let pitch = f.pitch();
    let origin = f.origin();

    let lo = from.min(to) - DVec2::splat(reach);
    let hi = from.max(to) + DVec2::splat(reach);
    let first = ((lo - origin) / pitch).floor();
    let last = ((hi - origin) / pitch).ceil();
    let (nx, ny) = (f64::from(f.nx()), f64::from(f.ny()));
    let finite = first.is_finite() && last.is_finite();
    let mut unknown = !finite || first.x < 0.0 || first.y < 0.0 || last.x >= nx || last.y >= ny;

    let mut top: Option<f64> = None;
    if finite {
        let (x0, y0, x1, y1) = (
            cell_index(first.x, f.nx()),
            cell_index(first.y, f.ny()),
            cell_index(last.x, f.nx()),
            cell_index(last.y, f.ny()),
        );
        for iy in y0..=y1 {
            for ix in x0..=x1 {
                let Some(c) = f.position(ix, iy) else {
                    continue;
                };
                if distance_to_segment(c, from, to) > reach {
                    continue;
                }
                match f.get(ix, iy) {
                    Some(h) => top = Some(top.map_or(h, |t: f64| t.max(h))),
                    None => unknown = true,
                }
            }
        }
    }
    Under { top, unknown }
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
    use crate::path::linker::tests::stepped_field;
    use glam::dvec2;

    #[test]
    fn rejects_an_unusable_field() {
        let f = stepped_field();
        assert!(ClearanceField::new(f.clone(), -1.0, 0.0).is_err());
        assert!(ClearanceField::new(f, 5.0, f64::NAN).is_err());
    }

    /// The whole point of a field over a plane: stay low where it is safe to.
    #[test]
    fn a_traverse_clear_of_the_boss_flies_low() {
        let c = ClearanceField::new(stepped_field(), 5.0, 0.0).unwrap();
        let away = c.safe_z(dvec2(-9.0, -9.0), dvec2(-7.0, -9.0), 0.5);
        assert!(
            (away - 5.0).abs() < 1e-9,
            "clear of the boss it should be flat + margin, got {away}"
        );
    }

    /// And rise where it is not.
    #[test]
    fn a_traverse_over_the_boss_rises() {
        let c = ClearanceField::new(stepped_field(), 5.0, 0.0).unwrap();
        let over = c.safe_z(dvec2(-9.0, 0.0), dvec2(9.0, 0.0), 0.5);
        assert!(
            (over - 25.0).abs() < 1e-9,
            "crossing the boss it should be 20 + margin, got {over}"
        );
    }

    /// The cutter is not a point. A rapid whose axis misses a boss can still put
    /// the tool's shoulder through it.
    #[test]
    fn the_tool_width_is_accounted_for() {
        let c = ClearanceField::new(stepped_field(), 5.0, 0.0).unwrap();
        // The boss spans cell centres -2..2. A traverse at y = -4 clears it by 2 mm
        // on the axis, but a 3 mm cutter does not.
        let thin = c.safe_z(dvec2(-9.0, -4.0), dvec2(9.0, -4.0), 0.1);
        let wide = c.safe_z(dvec2(-9.0, -4.0), dvec2(9.0, -4.0), 3.0);
        assert!(
            thin < 25.0,
            "a thin tool passes beside the boss, got {thin}"
        );
        assert!(
            (wide - 25.0).abs() < 1e-9,
            "a wide tool must clear it, got {wide}"
        );
    }

    /// An unsampled column is an unknown height. Flying low over an unknown is
    /// exactly the wrong guess.
    #[test]
    fn unknown_ground_falls_back_rather_than_flying_low() {
        let c = ClearanceField::new(stepped_field(), 5.0, 99.0).unwrap();
        let outside = c.safe_z(dvec2(500.0, 500.0), dvec2(520.0, 500.0), 1.0);
        assert_eq!(outside, 99.0, "off the field, the fallback governs");
    }
}
