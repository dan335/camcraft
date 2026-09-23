//! What shape a face or an edge is: a plane, a line, a circle, or none of those.
//!
//! Each fit is accepted only when every point lies within the tolerance of it, so a
//! face called a plane *is* flat to that tolerance and a circle's centre is a centre
//! worth putting a work zero on.

use super::{Circle, EdgeKind, FaceKind};
use crate::mesh::{TriMesh, closest_point_on_segment};
use glam::DVec3;

/// The plane the triangles lie in, or [`FaceKind::Curved`].
///
/// The normal is the area-weighted sum of the triangles' normals, each first turned
/// to agree with the largest triangle's, so a face whose triangles are wound
/// inconsistently still finds its plane. It points the way the largest triangle is
/// wound.
pub(super) fn plane(mesh: &TriMesh, triangles: &[u32], tol: f64) -> FaceKind {
    let corners = || triangles.iter().filter_map(|&t| mesh.triangle(t as usize));
    // The first of the largest, so a tie goes the same way every time.
    let Some(reference) = corners()
        .map(|[a, b, c]| (b - a).cross(c - a))
        .reduce(|best, n| {
            if n.length_squared() > best.length_squared() {
                n
            } else {
                best
            }
        })
        .filter(|n| n.length_squared() > 0.0)
    else {
        return FaceKind::Curved;
    };
    let (mut sum, mut moment, mut area) = (DVec3::ZERO, DVec3::ZERO, 0.0);
    for [a, b, c] in corners() {
        let n = (b - a).cross(c - a);
        let n = if n.dot(reference) < 0.0 { -n } else { n };
        let w = n.length();
        sum += n;
        moment += (a + b + c) / 3.0 * w;
        area += w;
    }
    let normal = sum.normalize_or_zero();
    if normal == DVec3::ZERO || area.is_nan() || area <= 0.0 {
        return FaceKind::Curved;
    }
    let offset = normal.dot(moment / area);
    let flat = corners()
        .flatten()
        .all(|p| (normal.dot(p) - offset).abs() <= tol);
    if flat {
        FaceKind::Plane { normal, offset }
    } else {
        FaceKind::Curved
    }
}

/// What an edge's points trace: a straight line, a circle or an arc of one, or some
/// other curve.
///
/// A circle needs at least five points, all in one plane and all on it within `tol`.
/// Fewer fit a circle whatever they are: any three points do, and so do a
/// rectangle's corners.
pub(super) fn edge(points: &[DVec3], closed: bool, tol: f64) -> EdgeKind {
    if !closed
        && let (Some(&a), Some(&b)) = (points.first(), points.last())
        && a.distance(b) > tol
        && points
            .iter()
            .all(|&p| closest_point_on_segment(p, a, b).distance(p) <= tol)
    {
        return EdgeKind::Line;
    }
    match circle(points, closed, tol) {
        Some(c) => EdgeKind::Circle(c),
        None => EdgeKind::Curve,
    }
}

fn circle(points: &[DVec3], closed: bool, tol: f64) -> Option<Circle> {
    if points.len() < 5 {
        return None;
    }
    #[allow(clippy::cast_precision_loss)] // point counts are far below 2^52
    let count = points.len() as f64;
    let mid = points.iter().copied().sum::<DVec3>() / count;
    // Newell's normal, of the polygon the points make, closed by its chord if the
    // edge is open. Measured from their middle so it keeps its precision far from
    // the origin.
    let next = points.iter().skip(1).chain(points.first());
    let normal = points
        .iter()
        .zip(next)
        .map(|(p, q)| (*p - mid).cross(*q - mid))
        .sum::<DVec3>()
        .normalize_or_zero();
    if normal == DVec3::ZERO {
        return None;
    }
    let normal = canonical(normal);
    if points.iter().any(|p| normal.dot(*p - mid).abs() > tol) {
        return None;
    }
    // A basis in the plane.
    let first = *points.first()?;
    let u = (first - mid)
        .reject_from_normalized(normal)
        .normalize_or_zero();
    if u == DVec3::ZERO {
        return None;
    }
    let v = normal.cross(u);
    let flat: Vec<(f64, f64)> = points
        .iter()
        .map(|p| ((*p - mid).dot(u), (*p - mid).dot(v)))
        .collect();

    // Kasa's fit: least squares on x² + y² + Dx + Ey + F = 0, which is linear.
    let (mut sxx, mut sxy, mut syy, mut sx, mut sy) = (0.0, 0.0, 0.0, 0.0, 0.0);
    let (mut sxz, mut syz, mut sz) = (0.0, 0.0, 0.0);
    for &(x, y) in &flat {
        let z = x * x + y * y;
        sxx += x * x;
        sxy += x * y;
        syy += y * y;
        sx += x;
        sy += y;
        sxz += x * z;
        syz += y * z;
        sz += z;
    }
    let [d, e, f] = solve3(
        [[sxx, sxy, sx], [sxy, syy, sy], [sx, sy, count]],
        [-sxz, -syz, -sz],
    )?;
    let (cx, cy) = (-d / 2.0, -e / 2.0);
    let r2 = cx * cx + cy * cy - f;
    if r2.is_nan() || r2 <= 0.0 {
        return None;
    }
    let radius = r2.sqrt();
    let on = flat
        .iter()
        .all(|&(x, y)| ((x - cx).hypot(y - cy) - radius).abs() <= tol);
    on.then(|| Circle {
        center: mid + u * cx + v * cy,
        normal,
        radius,
        full: closed,
    })
}

/// `n` or `-n`, whichever points up, or failing that towards +Y, or +X: a circle's
/// normal has no side, so it gets the same one however its points run.
fn canonical(n: DVec3) -> DVec3 {
    let key = [n.z, n.y, n.x];
    match key.iter().find(|c| c.abs() > 1e-9) {
        Some(c) if *c < 0.0 => -n,
        _ => n,
    }
}

/// Cramer's rule, or `None` when the system has no single answer.
fn solve3(m: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = |m: [[f64; 3]; 3]| {
        let [[a, b, c], [d, e, f], [g, h, i]] = m;
        a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
    };
    let whole = det(m);
    let scale = m
        .iter()
        .flatten()
        .fold(0.0_f64, |s, x| s.max(x.abs()))
        .powi(3);
    if whole.is_nan() || whole.abs() <= 1e-14 * scale {
        return None;
    }
    let column = |k: usize| {
        let mut c = m;
        for (row, v) in c.iter_mut().zip(b) {
            if let Some(slot) = row.get_mut(k) {
                *slot = v;
            }
        }
        det(c) / whole
    };
    Some([column(0), column(1), column(2)])
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use glam::dvec3;

    fn ring(center: DVec3, r: f64, n: u32, arc: f64) -> Vec<DVec3> {
        (0..n)
            .map(|i| {
                let a = arc * f64::from(i) / f64::from(n);
                center + dvec3(r * a.cos(), 0.0, r * a.sin())
            })
            .collect()
    }

    #[test]
    fn a_ring_of_points_is_a_circle_with_its_centre() {
        let c = dvec3(10.0, -4.0, 7.0);
        let EdgeKind::Circle(fit) = edge(&ring(c, 3.0, 24, std::f64::consts::TAU), true, 1e-6)
        else {
            unreachable!("not a circle");
        };
        assert!(fit.center.distance(c) < 1e-9, "{:?}", fit.center);
        assert!((fit.radius - 3.0).abs() < 1e-9);
        assert!((fit.normal - dvec3(0.0, 1.0, 0.0)).length() < 1e-9);
        assert!(fit.full);
    }

    #[test]
    fn an_open_arc_is_a_circle_too_and_a_straight_run_a_line() {
        let c = dvec3(1.0, 2.0, 3.0);
        let arc = ring(c, 5.0, 9, std::f64::consts::FRAC_PI_2);
        let EdgeKind::Circle(fit) = edge(&arc, false, 1e-6) else {
            unreachable!("not an arc");
        };
        assert!(fit.center.distance(c) < 1e-9);
        assert!(!fit.full);
        let line: Vec<_> = (0..6).map(|i| dvec3(f64::from(i), 2.0, 0.0)).collect();
        assert_eq!(edge(&line, false, 1e-6), EdgeKind::Line);
        // Too few points to say, and points off any circle.
        assert_eq!(edge(arc.get(..4).unwrap(), false, 1e-6), EdgeKind::Curve);
        let mut bent = arc.clone();
        if let Some(p) = bent.get_mut(4) {
            *p += dvec3(0.3, 0.0, 0.0);
        }
        assert_eq!(edge(&bent, false, 1e-6), EdgeKind::Curve);
    }

    #[test]
    fn a_flat_face_is_a_plane_whatever_its_winding() {
        let mesh = TriMesh::new(
            vec![
                dvec3(0.0, 0.0, 2.0),
                dvec3(4.0, 0.0, 2.0),
                dvec3(4.0, 3.0, 2.0),
                dvec3(0.0, 3.0, 2.0),
            ],
            // The second triangle wound the other way.
            vec![[0, 1, 2], [0, 3, 2]],
        )
        .unwrap();
        let FaceKind::Plane { normal, offset } = plane(&mesh, &[0, 1], 1e-9) else {
            unreachable!("not a plane");
        };
        assert!((normal - dvec3(0.0, 0.0, 1.0)).length() < 1e-12);
        assert!((offset - 2.0).abs() < 1e-12);
    }
}
