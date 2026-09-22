//! The drop-cutter: the exact, authoritative cutter-location query.
//!
//! Drop a tool straight down at `(x, y)` until it first touches the mesh, and report
//! the tool-tip Z. Every emitted path point is sampled this way. The cached grid is
//! an optimisation layered on top; *this* is the ground truth.
//!
//! # Why not grid dilation
//!
//! Taking a maximum over a rasterised height field rounds sharp edges — the known
//! ZMap accuracy problem. The `step` fixture exists precisely to catch that: a flat
//! mill of radius `R` must report the full step height right up to `x = R` and the
//! floor immediately past it, with no ramp between.
//!
//! # Decomposition
//!
//! For a tool at `(x, y)`, contact happens at a triangle **vertex**, along an
//! **edge**, or on the **facet** interior. Taking the maximum over all three for
//! every candidate triangle is exact, because the tool stops at the *highest*
//! contact it meets on the way down.
//!
//! The maximum is associative and commutative over non-NaN floats, so the result
//! does not depend on the order candidates are visited. That is what makes this
//! deterministic under `rayon`.

use camcraft_geom::bvh::MeshBvh;
use camcraft_geom::mesh::TriMesh;
use glam::{DVec2, DVec3, dvec2, dvec3};

use crate::tool::{Ball, Flat, ToolProfile};

/// Triangles smaller than this in squared normal length are treated as degenerate
/// and contribute only through their vertices.
const DEGENERATE: f64 = 1.0e-24;

/// Slack for "is this point inside the triangle", as a fraction of barycentric span.
const IN_TRI_EPS: f64 = 1.0e-9;

/// A tool that can be dropped onto a triangle exactly.
///
/// Separate from [`ToolProfile`] because a profile is just geometry, whereas this is
/// a contact query with an analytic solution per cutter shape. A new cutter must
/// provide one; there is deliberately no silently-approximate default.
pub trait DropTest: ToolProfile {
    /// Lowest tool-tip Z at `(x, y)` that does not penetrate this triangle, or
    /// `None` when the tool footprint does not reach it.
    fn drop_triangle(&self, x: f64, y: f64, tri: &[DVec3; 3]) -> Option<f64>;
}

/// Exact cutter-location queries against a mesh.
#[derive(Debug)]
pub struct DropCutter<'a> {
    mesh: &'a TriMesh,
    bvh: MeshBvh,
}

impl<'a> DropCutter<'a> {
    /// Build a drop-cutter over `mesh`, constructing its spatial index.
    pub fn new(mesh: &'a TriMesh) -> Self {
        Self {
            bvh: MeshBvh::new(mesh),
            mesh,
        }
    }

    /// Tool-tip Z at `(x, y)`, or `None` when no geometry lies under the tool.
    ///
    /// "Nothing underneath" is a normal answer — over open stock there is simply no
    /// contact — so it is `None` rather than an error.
    pub fn drop_at<T: DropTest>(&self, tool: &T, x: f64, y: f64) -> Option<f64> {
        let mut scratch = Vec::new();
        self.drop_at_with(tool, x, y, &mut scratch)
    }

    /// As [`drop_at`](DropCutter::drop_at), reusing a caller-owned candidate buffer.
    ///
    /// A dense CL map makes millions of these calls; give each thread its own buffer
    /// so the hot loop does not allocate.
    pub fn drop_at_with<T: DropTest>(
        &self,
        tool: &T,
        x: f64,
        y: f64,
        scratch: &mut Vec<u32>,
    ) -> Option<f64> {
        self.bvh
            .candidates_in_column(x, y, tool.max_radius(), scratch);

        let mut best: Option<f64> = None;
        for index in scratch.iter() {
            let Some(tri) = self.mesh.triangle(*index as usize) else {
                continue;
            };
            if let Some(z) = tool.drop_triangle(x, y, &tri) {
                best = Some(match best {
                    Some(b) if b >= z => b,
                    _ => z,
                });
            }
        }
        best
    }
}

// ---------------------------------------------------------------------------
// Shared geometry helpers
// ---------------------------------------------------------------------------

fn component(v: DVec3, axis: u8) -> f64 {
    match axis {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

/// Unnormalised triangle normal, or `None` if the triangle has no area.
fn normal(tri: &[DVec3; 3]) -> Option<DVec3> {
    let (a, b, c) = (tri[0], tri[1], tri[2]);
    let n = (b - a).cross(c - a);
    (n.length_squared() > DEGENERATE).then_some(n)
}

/// Is `p`, known to lie in the triangle's plane, inside the triangle?
///
/// Tested in the axis-aligned plane where the triangle projects largest, which keeps
/// the barycentric denominators well away from zero for steep faces.
fn point_in_triangle(p: DVec3, tri: &[DVec3; 3], n: DVec3) -> bool {
    let (ax, ay) = {
        let (nx, ny, nz) = (n.x.abs(), n.y.abs(), n.z.abs());
        if nx >= ny && nx >= nz {
            (1u8, 2u8)
        } else if ny >= nz {
            (2u8, 0u8)
        } else {
            (0u8, 1u8)
        }
    };
    let flat = |v: DVec3| dvec2(component(v, ax), component(v, ay));
    let (a, b, c, q) = (flat(tri[0]), flat(tri[1]), flat(tri[2]), flat(p));

    let area = (b - a).perp_dot(c - a);
    if area.abs() < DEGENERATE {
        return false;
    }
    let s = (b - a).perp_dot(q - a) / area;
    let t = (c - b).perp_dot(q - b) / area;
    let u = (a - c).perp_dot(q - c) / area;
    s >= -IN_TRI_EPS && t >= -IN_TRI_EPS && u >= -IN_TRI_EPS
}

/// Height of the plane through `p0` with normal `n` at XY position `(x, y)`.
fn plane_z(n: DVec3, p0: DVec3, x: f64, y: f64) -> f64 {
    p0.z - (n.x * (x - p0.x) + n.y * (y - p0.y)) / n.z
}

/// Parameter range of segment `q0 -> q1` lying within the disc of radius `r` about
/// `c`, clipped to `[0, 1]`.
fn segment_in_disc(q0: DVec2, q1: DVec2, c: DVec2, r: f64) -> Option<(f64, f64)> {
    let d = q1 - q0;
    let f = q0 - c;
    let a = d.length_squared();

    if a < DEGENERATE {
        // The segment projects to a point — a vertical edge.
        return (f.length_squared() <= r * r).then_some((0.0, 1.0));
    }

    let b = 2.0 * d.dot(f);
    let cc = f.length_squared() - r * r;
    let disc = b * b - 4.0 * a * cc;

    if disc < 0.0 {
        // No crossing: the segment is wholly inside the disc, or wholly outside.
        return (cc <= 0.0).then_some((0.0, 1.0));
    }
    let root = disc.sqrt();
    let lo = ((-b - root) / (2.0 * a)).max(0.0);
    let hi = ((-b + root) / (2.0 * a)).min(1.0);
    (lo <= hi).then_some((lo, hi))
}

fn edges(tri: &[DVec3; 3]) -> [(DVec3, DVec3); 3] {
    [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])]
}

fn raise(best: &mut Option<f64>, z: f64) {
    if z.is_finite() {
        *best = Some(match *best {
            Some(b) if b >= z => b,
            _ => z,
        });
    }
}

// ---------------------------------------------------------------------------
// Flat end mill
// ---------------------------------------------------------------------------

impl DropTest for Flat {
    /// The cutting face is a disc at the tip, so the tool rests at
    /// `max{ p.z : p in triangle, |p.xy - c| <= R }` — the maximum of a linear
    /// function over the triangle clipped to the disc.
    ///
    /// That maximum lies on the boundary of the clipped region, which is exactly:
    /// vertices inside the disc, edge segments inside the disc, and the disc rim
    /// where it passes through the triangle. The rim contributes its uphill point,
    /// since a plane increases fastest along its gradient.
    fn drop_triangle(&self, x: f64, y: f64, tri: &[DVec3; 3]) -> Option<f64> {
        let r = self.max_radius();
        let c = dvec2(x, y);
        let mut best: Option<f64> = None;

        // Vertices lying under the cutting face.
        for v in tri {
            if (v.truncate() - c).length_squared() <= r * r {
                raise(&mut best, v.z);
            }
        }

        // Facet: the highest point of the disc, if it falls inside the triangle.
        if let Some(n) = normal(tri)
            && n.z.abs() > 0.0
        {
            let grad = dvec2(-n.x / n.z, -n.y / n.z);
            let at = if grad.length_squared() > DEGENERATE {
                c + grad.normalize() * r
            } else {
                c // Horizontal facet: every point of the disc is equally high.
            };
            let z = plane_z(n, tri[0], at.x, at.y);
            if point_in_triangle(dvec3(at.x, at.y, z), tri, n) {
                raise(&mut best, z);
            }
        }

        // Edges clipped to the disc; z is linear along an edge, so the maximum is
        // at one end of the clipped interval.
        for (p0, p1) in edges(tri) {
            if let Some((lo, hi)) = segment_in_disc(p0.truncate(), p1.truncate(), c, r) {
                let dz = p1.z - p0.z;
                raise(&mut best, p0.z + lo * dz);
                raise(&mut best, p0.z + hi * dz);
            }
        }

        best
    }
}

// ---------------------------------------------------------------------------
// Ball-nose end mill
// ---------------------------------------------------------------------------

impl DropTest for Ball {
    /// The ball centre sits `R` above the tip, so contact means the centre is exactly
    /// `R` from the triangle. Solve for the centre height at each feature and return
    /// the highest, less `R` to get back to the tip.
    ///
    /// The `- R` is why the sphere fixture exists: the *centre* traces the offset
    /// surface, the controlled point does not.
    fn drop_triangle(&self, x: f64, y: f64, tri: &[DVec3; 3]) -> Option<f64> {
        let r = self.max_radius();
        let c = dvec2(x, y);
        let mut centre: Option<f64> = None;

        // Vertex: |centre - v| = R.
        for v in tri {
            let d2 = (v.truncate() - c).length_squared();
            if d2 <= r * r {
                raise(&mut centre, v.z + (r * r - d2).sqrt());
            }
        }

        // Facet: the centre lies on the plane offset by R, and the contact point —
        // the centre projected back down the normal — must be inside the triangle.
        if let Some(n) = normal(tri) {
            let n = if n.z < 0.0 { -n } else { n }; // We always approach from above.
            let n = n.normalize();
            if n.z > 0.0 {
                let zc = (r + n.dot(tri[0]) - n.x * x - n.y * y) / n.z;
                let contact = dvec3(x, y, zc) - n * r;
                if point_in_triangle(contact, tri, n) {
                    raise(&mut centre, zc);
                }
            }
        }

        // Edge: distance from the centre to the segment equals R.
        for (p0, p1) in edges(tri) {
            if let Some(zc) = ball_edge_contact(p0, p1, x, y, r) {
                raise(&mut centre, zc);
            }
        }

        centre.map(|zc| zc - r)
    }
}

/// Height of a ball centre of radius `r` on the vertical line through `(x, y)` at
/// which it touches the segment `p0 -> p1`.
///
/// With `w = C - p0`, `e = p1 - p0` and `u = z_c - p0.z`, requiring the
/// point-to-line distance to equal `r` gives
///
/// ```text
/// u^2 (E - ez^2) - 2 A ez u + (E|a|^2 - A^2 - r^2 E) = 0
/// ```
///
/// where `a` is the horizontal part of `w`, `A = a . e` and `E = |e|^2`. A vertical
/// edge makes the leading coefficient vanish; such an edge can only be touched at its
/// endpoints, which the vertex test already covers.
fn ball_edge_contact(p0: DVec3, p1: DVec3, x: f64, y: f64, r: f64) -> Option<f64> {
    let e = p1 - p0;
    let ee = e.length_squared();
    let qa = ee - e.z * e.z; // horizontal length squared
    if qa < DEGENERATE || ee < DEGENERATE {
        return None;
    }

    let a = dvec3(x - p0.x, y - p0.y, 0.0);
    let aa = a.length_squared();
    let ae = a.dot(e);

    let qb = -2.0 * ae * e.z;
    let qc = ee * aa - ae * ae - r * r * ee;
    let disc = qb * qb - 4.0 * qa * qc;
    if disc < 0.0 {
        return None;
    }
    let root = disc.sqrt();

    // Prefer the higher contact, but fall back to the lower root if only that one
    // lands on the segment rather than past an end.
    let mut best: Option<f64> = None;
    for u in [(-qb + root) / (2.0 * qa), (-qb - root) / (2.0 * qa)] {
        let t = (ae + u * e.z) / ee;
        if (0.0..=1.0).contains(&t) {
            raise(&mut best, p0.z + u);
        }
    }
    best
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;
    use camcraft_geom::synthetic;
    use proptest::prelude::*;

    const R: f64 = 3.0;

    #[test]
    fn flat_on_a_plane_is_exact() {
        let mesh = synthetic::plane(2.5, 20.0);
        let dc = DropCutter::new(&mesh);
        let tool = Flat::new(R).unwrap();
        for (x, y) in [(0.0, 0.0), (5.0, -7.0), (-12.0, 3.0)] {
            let z = dc.drop_at(&tool, x, y).unwrap();
            assert!((z - 2.5).abs() < 1e-12, "at ({x},{y}) got {z}, want 2.5");
        }
    }

    #[test]
    fn ball_on_a_plane_is_exact() {
        let mesh = synthetic::plane(-1.25, 20.0);
        let dc = DropCutter::new(&mesh);
        let tool = Ball::new(R).unwrap();
        for (x, y) in [(0.0, 0.0), (4.0, 4.0), (-9.0, 2.0)] {
            let z = dc.drop_at(&tool, x, y).unwrap();
            assert!((z + 1.25).abs() < 1e-12, "at ({x},{y}) got {z}, want -1.25");
        }
    }

    /// The fixture that a grid-dilation implementation cannot pass: the step must be
    /// full height right up to `x = R`, and the floor immediately past it.
    #[test]
    fn flat_on_a_sharp_step_does_not_round_the_edge() {
        let h = 4.0;
        let mesh = synthetic::step(h, 20.0);
        let dc = DropCutter::new(&mesh);
        let tool = Flat::new(R).unwrap();

        for x in [-10.0, -1.0, 0.0, R - 1e-6] {
            let z = dc.drop_at(&tool, x, 0.0).unwrap();
            assert!(
                (z - h).abs() < 1e-9,
                "at x={x} the disc still overlaps the plateau; got {z}, want {h}"
            );
        }
        for x in [R + 1e-6, R + 0.5, 10.0] {
            let z = dc.drop_at(&tool, x, 0.0).unwrap();
            assert!(
                z.abs() < 1e-9,
                "at x={x} the disc has cleared the plateau; got {z}, want 0"
            );
        }
    }

    /// The closed form, with the tip-versus-centre offset that the fixture exists to
    /// protect. Compared within the tessellation sag, so a genuinely wrong answer —
    /// notably one off by a whole ball radius — still fails.
    #[test]
    fn ball_on_a_sphere_matches_the_closed_form() {
        let (sphere_r, lat, lon) = (10.0, 200, 300);
        let mesh = synthetic::uv_sphere(sphere_r, lat, lon);
        let sag = synthetic::uv_sphere_max_sag(sphere_r, lat, lon);
        let dc = DropCutter::new(&mesh);
        let br = 2.0;
        let tool = Ball::new(br).unwrap();

        for (x, y) in [(0.0, 0.0), (2.0, 0.0), (0.0, -3.0), (4.0, 3.0)] {
            let got = dc.drop_at(&tool, x, y).unwrap();
            let sum = sphere_r + br;
            let want = (sum * sum - x * x - y * y).sqrt() - br;
            assert!(
                (got - want).abs() <= sag + 1e-9,
                "at ({x},{y}) got {got}, want {want}, sag budget {sag}"
            );
            assert!(
                got <= want + 1e-9,
                "a chord mesh may sit below the ideal surface, never above"
            );
        }
    }

    /// Exercises the facet rim. On a slope a flat mill rests on the *uphill edge* of
    /// its disc, `R * |grad|` above the plane beneath its centre. The horizontal
    /// fixtures cannot catch a missing rim term, because there the rim and the centre
    /// sit at the same height.
    #[test]
    fn flat_on_an_incline_rests_on_the_uphill_rim() {
        let (a, b, z0) = (0.35, -0.2, 1.5);
        let mesh = synthetic::inclined_plane(a, b, z0, 40.0);
        let dc = DropCutter::new(&mesh);
        let tool = Flat::new(R).unwrap();
        let lift = R * (a * a + b * b).sqrt();
        assert!(lift > 0.1, "fixture must actually be sloped");

        for (x, y) in [(0.0, 0.0), (6.0, -4.0), (-8.0, 5.0)] {
            let got = dc.drop_at(&tool, x, y).unwrap();
            let want = z0 + a * x + b * y + lift;
            assert!(
                (got - want).abs() < 1e-9,
                "at ({x},{y}) got {got}, want {want} (rim lift {lift})"
            );
        }
    }

    /// The ball's facet contact on a slope: the centre sits `r / n_z` above the
    /// plane, so the tip sits `r * (sqrt(a^2 + b^2 + 1) - 1)` above it.
    #[test]
    fn ball_on_an_incline_matches_the_closed_form() {
        let (a, b, z0) = (0.4, 0.25, -2.0);
        let mesh = synthetic::inclined_plane(a, b, z0, 40.0);
        let dc = DropCutter::new(&mesh);
        let br = 2.5;
        let tool = Ball::new(br).unwrap();
        let lift = br * ((a * a + b * b + 1.0).sqrt() - 1.0);
        assert!(lift > 0.1, "fixture must actually be sloped");

        for (x, y) in [(0.0, 0.0), (5.0, 5.0), (-7.0, 2.0)] {
            let got = dc.drop_at(&tool, x, y).unwrap();
            let want = z0 + a * x + b * y + lift;
            assert!(
                (got - want).abs() < 1e-9,
                "at ({x},{y}) got {got}, want {want} (lift {lift})"
            );
        }
    }

    #[test]
    fn reports_nothing_where_there_is_no_geometry() {
        let mesh = synthetic::plane(0.0, 1.0);
        let dc = DropCutter::new(&mesh);
        assert_eq!(dc.drop_at(&Flat::new(0.5).unwrap(), 50.0, 50.0), None);
    }

    #[test]
    fn survives_degenerate_input() {
        let mesh = TriMesh::new(
            vec![dvec3(0.0, 0.0, 1.0), dvec3(1.0, 0.0, 1.0)],
            vec![[0, 0, 0], [0, 1, 1]],
        )
        .unwrap();
        let dc = DropCutter::new(&mesh);
        // Zero-area triangles must contribute their vertices and nothing worse.
        let z = dc.drop_at(&Flat::new(2.0).unwrap(), 0.5, 0.0).unwrap();
        assert!((z - 1.0).abs() < 1e-12);
    }

    /// The invariant the whole system exists to hold: the tool never ends up below
    /// the surface it is cutting.
    ///
    /// Sampled across whole triangles rather than at vertices alone. A vertex-only
    /// check is far too weak — it cannot see the tool slicing through the middle of
    /// an edge or a face, which is exactly what a missing edge or facet test does.
    fn assert_no_penetration<T: DropTest>(
        mesh: &TriMesh,
        dc: &DropCutter<'_>,
        tool: &T,
        x: f64,
        y: f64,
    ) -> Result<(), TestCaseError> {
        let Some(z) = dc.drop_at(tool, x, y) else {
            return Ok(());
        };
        let r = tool.max_radius();
        const N: u32 = 8;

        for i in 0..mesh.len() {
            let Some(tri) = mesh.triangle(i) else {
                continue;
            };
            for u in 0..=N {
                for v in 0..=(N - u) {
                    let (bu, bv) = (f64::from(u) / f64::from(N), f64::from(v) / f64::from(N));
                    let p = tri[0] + (tri[1] - tri[0]) * bu + (tri[2] - tri[0]) * bv;
                    let d = (p.truncate() - dvec2(x, y)).length();
                    if d > r {
                        continue;
                    }
                    let Some(h) = tool.height_at(d) else { continue };
                    prop_assert!(
                        z + h >= p.z - 1e-7,
                        "tool at z={z} cuts through {p:?}: surface {} above tool {}",
                        p.z,
                        z + h
                    );
                }
            }
        }
        Ok(())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn never_penetrates_the_sphere(x in -8.0f64..8.0, y in -8.0f64..8.0, br in 0.5f64..4.0) {
            let mesh = synthetic::uv_sphere(10.0, 40, 60);
            let dc = DropCutter::new(&mesh);
            let tool = Ball::new(br).unwrap();
            assert_no_penetration(&mesh, &dc, &tool, x, y)?;
        }

        #[test]
        fn never_penetrates_the_step(x in -8.0f64..8.0, y in -8.0f64..8.0, fr in 0.5f64..4.0) {
            let mesh = synthetic::step(4.0, 10.0);
            let dc = DropCutter::new(&mesh);
            let tool = Flat::new(fr).unwrap();
            assert_no_penetration(&mesh, &dc, &tool, x, y)?;
        }

        /// A larger tool cannot reach deeper than a smaller one.
        #[test]
        fn monotonic_in_tool_radius(x in -8.0f64..8.0, y in -8.0f64..8.0) {
            let mesh = synthetic::uv_sphere(10.0, 40, 60);
            let dc = DropCutter::new(&mesh);
            let small = dc.drop_at(&Ball::new(1.0).unwrap(), x, y);
            let large = dc.drop_at(&Ball::new(3.0).unwrap(), x, y);
            if let (Some(s), Some(l)) = (small, large) {
                prop_assert!(l >= s - 1e-7, "radius 3 gave {l}, radius 1 gave {s}");
            }
        }

        /// Contact is a maximum over candidates, so visiting order cannot matter.
        #[test]
        fn is_deterministic(x in -8.0f64..8.0, y in -8.0f64..8.0) {
            let mesh = synthetic::uv_sphere(10.0, 30, 40);
            let dc = DropCutter::new(&mesh);
            let tool = Ball::new(2.0).unwrap();
            let first = dc.drop_at(&tool, x, y);
            for _ in 0..4 {
                prop_assert_eq!(dc.drop_at(&tool, x, y), first);
            }
        }
    }
}
