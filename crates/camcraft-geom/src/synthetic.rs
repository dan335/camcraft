//! Synthetic primitives whose exact cutter-location surface is known in closed form.
//!
//! These exist so tests can assert against *real answers* rather than against
//! committed values that merely lock in whatever the code did first. They are part
//! of the public API because the drop-cutter, the simulator and the benchmarks all
//! need them, and they are cheap.
//!
//! # Tessellation is not the shape
//!
//! A tessellated sphere is not a sphere: every chord lies inside the true surface,
//! so a drop-cutter run against the mesh sits slightly *below* the closed-form
//! answer for the ideal solid. Tests must therefore compare within that bound, not
//! for equality. [`uv_sphere_max_sag`] returns it, so the tolerance is derived
//! rather than guessed.
//!
//! [`plane`] and [`step`] have no such error — they are exactly planar, which is
//! what makes `step` the sharp-edge fixture that a dilation-based implementation
//! cannot pass.

use crate::mesh::TriMesh;
use glam::{DVec3, dvec3};

/// A flat plane at height `z`, spanning `[-half, half]` in X and Y.
///
/// Closed form: for any tool, `Zcl = z` everywhere the tool is fully over the plane.
pub fn plane(z: f64, half: f64) -> TriMesh {
    let v = vec![
        dvec3(-half, -half, z),
        dvec3(half, -half, z),
        dvec3(half, half, z),
        dvec3(-half, half, z),
    ];
    // Wound counter-clockwise seen from +Z, so the normal points up.
    TriMesh::new(v, vec![[0, 1, 2], [0, 2, 3]]).unwrap_or_default()
}

/// A sharp step: height `h` over `x <= 0`, height `0` over `x >= 0`, joined by a
/// vertical wall, spanning `[-half, half]` in both axes.
///
/// Closed form for a flat end mill of radius `R`, whose disc touches the upper
/// level exactly while `x < R`:
///
/// ```text
/// Zcl(x, y) = h   for x < R
///             0   for x >= R
/// ```
///
/// Exactly planar, so there is no tessellation error to allow for. This is the
/// fixture that catches edge rounding.
pub fn step(h: f64, half: f64) -> TriMesh {
    let v = vec![
        // upper plateau, x in [-half, 0]
        dvec3(-half, -half, h),
        dvec3(0.0, -half, h),
        dvec3(0.0, half, h),
        dvec3(-half, half, h),
        // lower shelf, x in [0, half]
        dvec3(0.0, -half, 0.0),
        dvec3(half, -half, 0.0),
        dvec3(half, half, 0.0),
        dvec3(0.0, half, 0.0),
    ];
    let t = vec![
        [0, 1, 2],
        [0, 2, 3], // plateau
        [4, 5, 6],
        [4, 6, 7], // shelf
        [1, 4, 7],
        [1, 7, 2], // vertical wall at x = 0
    ];
    TriMesh::new(v, t).unwrap_or_default()
}

/// A UV sphere of radius `r` centred at the origin, with `lat` stacks and `lon`
/// sectors.
///
/// Closed form for a ball cutter of radius `br` against the ideal sphere, remembering
/// that the controlled point is the **tool tip** and not the ball centre: the centre
/// traces a sphere of radius `r + br`, so
///
/// ```text
/// Zcl(x, y) = sqrt((r + br)^2 - x^2 - y^2) - br
/// ```
///
/// Getting that `- br` wrong would bake an error of one ball radius into everything
/// downstream, which is precisely why this fixture exists.
///
/// `lat` and `lon` are clamped to `[2, MAX_SUBDIVISION]` and `[3, MAX_SUBDIVISION]`
/// respectively, which keeps every index well inside `u32` and makes overflow
/// impossible rather than merely unlikely.
pub fn uv_sphere(r: f64, lat: u32, lon: u32) -> TriMesh {
    let (lat, lon) = clamp_subdivision(lat, lon);
    let row = lon + 1;
    let mut v: Vec<DVec3> = Vec::with_capacity((row as usize) * (lat as usize + 1));
    for i in 0..=lat {
        let theta = std::f64::consts::PI * f64::from(i) / f64::from(lat);
        let (st, ct) = theta.sin_cos();
        for j in 0..=lon {
            let phi = std::f64::consts::TAU * f64::from(j) / f64::from(lon);
            let (sp, cp) = phi.sin_cos();
            v.push(dvec3(r * st * cp, r * st * sp, r * ct));
        }
    }
    let mut t = Vec::with_capacity(lat as usize * lon as usize * 2);
    for i in 0..lat {
        for j in 0..lon {
            let a = i * row + j;
            let c = (i + 1) * row + j;
            t.push([a, c, a + 1]);
            t.push([a + 1, c, c + 1]);
        }
    }
    TriMesh::new(v, t).unwrap_or_default()
}

/// Upper bound on [`uv_sphere`] subdivision, chosen so the largest possible vertex
/// index stays far below `u32::MAX`.
pub const MAX_SUBDIVISION: u32 = 4096;

fn clamp_subdivision(lat: u32, lon: u32) -> (u32, u32) {
    (lat.clamp(2, MAX_SUBDIVISION), lon.clamp(3, MAX_SUBDIVISION))
}

/// The largest distance by which a [`uv_sphere`] chord falls inside the true sphere.
///
/// A chord subtending central angle `d` on radius `r` sags by `r * (1 - cos(d / 2))`,
/// so the bound is set by the *longest* edge in the mesh. That is not simply the
/// larger of the two grid steps: the quads are split diagonally, and a diagonal spans
/// a latitude step and a longitude step at once, subtending more than either alone.
///
/// Rather than approximate, this walks the stacks and takes the exact maximum over
/// meridian, parallel and diagonal edges, using the spherical law of cosines
///
/// ```text
/// cos d = cos t0 cos t1 + sin t0 sin t1 cos dphi
/// ```
///
/// Tests size their tolerance from this instead of guessing, so a genuinely wrong
/// answer still fails.
pub fn uv_sphere_max_sag(r: f64, lat: u32, lon: u32) -> f64 {
    let (lat, lon) = clamp_subdivision(lat, lon);
    let dphi = std::f64::consts::TAU / f64::from(lon);
    let stack = |i: u32| std::f64::consts::PI * f64::from(i) / f64::from(lat);

    let separation = |t0: f64, t1: f64| -> f64 {
        let c = t0.cos() * t1.cos() + t0.sin() * t1.sin() * dphi.cos();
        c.clamp(-1.0, 1.0).acos()
    };

    let mut max_angle: f64 = 0.0;
    for i in 0..lat {
        let (t0, t1) = (stack(i), stack(i + 1));
        // Meridian edge: pure latitude step.
        max_angle = max_angle.max(t1 - t0);
        // Parallel edges at both stack boundaries.
        max_angle = max_angle.max(separation(t0, t0));
        max_angle = max_angle.max(separation(t1, t1));
        // The diagonal that splits the quad — usually the longest of the three.
        max_angle = max_angle.max(separation(t0, t1));
    }
    r * (1.0 - (max_angle / 2.0).cos())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn plane_is_flat_and_sized() {
        let m = plane(3.0, 10.0);
        assert_eq!(m.len(), 2);
        let b = m.bounds().unwrap();
        assert_eq!(b.min, dvec3(-10.0, -10.0, 3.0));
        assert_eq!(b.max, dvec3(10.0, 10.0, 3.0));
    }

    #[test]
    fn step_spans_both_levels() {
        let m = step(5.0, 10.0);
        let b = m.bounds().unwrap();
        assert_eq!(b.min.z, 0.0);
        assert_eq!(b.max.z, 5.0);
        // Plateau, shelf, and the wall joining them.
        assert_eq!(m.len(), 6);
    }

    #[test]
    fn sphere_vertices_lie_on_the_sphere() {
        let r = 7.0;
        let m = uv_sphere(r, 16, 24);
        for v in m.vertices() {
            assert!(
                (v.length() - r).abs() < 1e-9,
                "vertex off the sphere: {v:?}"
            );
        }
        let b = m.bounds().unwrap();
        // Poles are exact, so Z reaches the full radius.
        assert!((b.max.z - r).abs() < 1e-9);
        assert!((b.min.z + r).abs() < 1e-9);
    }

    /// The sag bound must actually bound the sag, or every test using it is a lie.
    #[test]
    fn sag_bound_holds_for_edge_midpoints() {
        let (r, lat, lon) = (7.0, 12, 18);
        let m = uv_sphere(r, lat, lon);
        let bound = uv_sphere_max_sag(r, lat, lon);
        assert!(bound > 0.0);
        for i in 0..m.len() {
            let t = m.triangle(i).unwrap();
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let mid = (a + b) * 0.5;
                if mid.length() < 1e-12 {
                    continue;
                }
                let sag = r - mid.length();
                assert!(
                    sag <= bound + 1e-12,
                    "chord sag {sag} exceeded the stated bound {bound}"
                );
            }
        }
    }

    #[test]
    fn degenerate_parameters_are_clamped_not_panicked() {
        assert!(!uv_sphere(1.0, 0, 0).is_empty());
        assert!(uv_sphere_max_sag(1.0, 0, 0).is_finite());
    }
}
