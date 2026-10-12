//! The drop-cutter: the exact, authoritative cutter-location query.
//!
//! Drop a tool straight down at `(x, y)` until it first touches the mesh, and report
//! the tool-tip Z. Every emitted path point is sampled this way: *this* is the
//! ground truth, and no grid stands in for it.
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

use std::cell::RefCell;

use camcraft_geom::bvh::MeshBvh;
use camcraft_geom::mesh::TriMesh;
use glam::{DVec2, DVec3, dvec2, dvec3};

use crate::cutter::tool::{Ball, Bullnose, Cone, Cutter, Flat, ToolProfile};

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

    /// The most a drop can come to rest above the highest point it touches. A
    /// tool's tip is its lowest point, so nothing, unless the tool is held off
    /// the part.
    fn above_contact(&self) -> f64 {
        0.0
    }
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
        thread_local! {
            /// The triangles a drop looks at: one list a thread, kept between
            /// drops, so a drop does not allocate.
            static SCRATCH: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
        }
        SCRATCH.with(|scratch| match scratch.try_borrow_mut() {
            Ok(mut scratch) => self.drop_at_in(tool, x, y, &mut scratch),
            // A drop made while another is under way on this thread.
            Err(_) => self.drop_at_in(tool, x, y, &mut Vec::new()),
        })
    }

    /// As [`drop_at`](Self::drop_at), with `scratch` for the triangles it looks
    /// at: for a caller making many drops, which then allocates once.
    pub fn drop_at_in<T: DropTest>(
        &self,
        tool: &T,
        x: f64,
        y: f64,
        scratch: &mut Vec<u32>,
    ) -> Option<f64> {
        self.bvh
            .candidates_in_column(x, y, tool.max_radius(), scratch);

        let lift = tool.above_contact();
        let mut best: Option<f64> = None;
        for index in scratch.iter() {
            let Some(tri) = self.mesh.triangle(*index as usize) else {
                continue;
            };
            // The tip is the tool's lowest point, so it never rests above the
            // highest point it touches: a triangle wholly below the best so far
            // cannot raise it.
            if best.is_some_and(|b| tri.iter().all(|v| v.z + lift <= b)) {
                continue;
            }
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
/// `c`, clipped to `[0, 1]`: where it goes in and where it comes out, if it
/// passes within `r` at all.
///
/// A segment whose length squared is under `degenerate` projects to a point, so
/// it is wholly in or wholly out, as its start is.
pub(crate) fn segment_in_disc(
    q0: DVec2,
    q1: DVec2,
    c: DVec2,
    r: f64,
    degenerate: f64,
) -> Option<(f64, f64)> {
    let d = q1 - q0;
    let f = q0 - c;
    let a = d.length_squared();

    if a < degenerate {
        // The segment projects to a point: a vertical edge, or a move straight
        // up or down.
        return (f.length_squared() <= r * r).then_some((0.0, 1.0));
    }

    let b = 2.0 * d.dot(f);
    let cc = f.length_squared() - r * r;
    let disc = b * b - 4.0 * a * cc;

    if disc < 0.0 {
        // The line misses the circle, so the segment is wholly outside it.
        return None;
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
        flat_drop(self.max_radius(), x, y, tri)
    }
}

/// [`Flat::drop_triangle`] for a flat mill of radius `r`.
fn flat_drop(r: f64, x: f64, y: f64, tri: &[DVec3; 3]) -> Option<f64> {
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
        if let Some((lo, hi)) = segment_in_disc(p0.truncate(), p1.truncate(), c, r, DEGENERATE) {
            let dz = p1.z - p0.z;
            raise(&mut best, p0.z + lo * dz);
            raise(&mut best, p0.z + hi * dz);
        }
    }

    best
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
        ball_drop(self.max_radius(), x, y, tri)
    }
}

/// [`Ball::drop_triangle`] for a ball mill of radius `r`.
fn ball_drop(r: f64, x: f64, y: f64, tri: &[DVec3; 3]) -> Option<f64> {
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

// ---------------------------------------------------------------------------
// Bullnose (corner-radius) end mill
// ---------------------------------------------------------------------------

/// Iterations of the ternary search used for a bullnose edge contact.
///
/// Fixed, so the result cannot vary with input or thread. Each step shrinks the
/// bracket by a third, so 80 steps reduce it by a factor of about `1e-14` — past
/// double precision for any edge of a real part.
const EDGE_SEARCH_STEPS: u32 = 80;

impl DropTest for Bullnose {
    /// The cutting end is a flat disc of radius `R - c` swept by a ball of radius
    /// `c` — that is, `disc(R - c) ⊕ Ball(c)`. So "the tool touches X" means "the
    /// disc is exactly `c` away from X", and the distance from a point to a
    /// horizontal disc has a closed form. Vertex and facet follow directly; the edge
    /// does not, and is handled below.
    ///
    /// A flat mill (no corner) and a ball mill (no flat) are bullnoses too, and
    /// are dropped as their own shapes, whose edge contacts are exact and need no
    /// search.
    fn drop_triangle(&self, x: f64, y: f64, tri: &[DVec3; 3]) -> Option<f64> {
        let c = self.corner();
        let rho = self.flat_radius();
        if c == 0.0 {
            return flat_drop(rho, x, y, tri);
        }
        if rho == 0.0 {
            return ball_drop(c, x, y, tri);
        }
        let centre = dvec2(x, y);
        let mut best: Option<f64> = None;

        // Vertex: horizontal gap to the disc rim, then the ball reaches the rest.
        for v in tri {
            if let Some(z) = disc_ball_contact(v.z, (v.truncate() - centre).length(), rho, c) {
                raise(&mut best, z);
            }
        }

        // Facet: the disc sits `c` from the plane. Its lowest point relative to the
        // plane is displaced `rho` down the in-plane component of the normal.
        if let Some(n) = normal(tri) {
            let n = if n.z < 0.0 { -n } else { n };
            let n = n.normalize();
            if n.z > 0.0 {
                let nh = dvec2(n.x, n.y).length();
                let disc_z = (c + rho * nh + n.dot(tri[0]) - n.x * x - n.y * y) / n.z;
                let rim = if nh > 0.0 {
                    dvec3(x - rho * n.x / nh, y - rho * n.y / nh, disc_z)
                } else {
                    dvec3(x, y, disc_z)
                };
                if point_in_triangle(rim - n * c, tri, n) {
                    raise(&mut best, disc_z - c);
                }
            }
        }

        // Edge: see `bullnose_edge_contact`.
        for (p0, p1) in edges(tri) {
            if let Some(z) = bullnose_edge_contact(p0, p1, centre, rho, c) {
                raise(&mut best, z);
            }
        }

        best
    }
}

/// Tip height at which `disc(rho) ⊕ Ball(c)` touches a point whose height is `pz`
/// and whose horizontal distance from the tool axis is `dxy`.
///
/// The disc sits `c` above the tip, so the horizontal gap the ball must span is
/// `max(0, dxy - rho)`; beyond `c` there is no contact.
///
/// Reduces correctly at both limits: with `c = 0` contact needs `dxy <= rho` and
/// gives `pz`, the flat-mill answer; with `rho = 0` it gives
/// `pz - c + sqrt(c^2 - dxy^2)`, the ball answer.
fn disc_ball_contact(pz: f64, dxy: f64, rho: f64, c: f64) -> Option<f64> {
    let gap = (dxy - rho).max(0.0);
    if gap > c {
        return None;
    }
    Some(pz - c + (c * c - gap * gap).max(0.0).sqrt())
}

/// Tip height at which a bullnose touches the segment `p0 -> p1`.
///
/// Along the segment, the tip height that would touch point `p(t)` is
///
/// ```text
/// f(t) = p(t).z - c + sqrt(c^2 - gap(t)^2),   gap(t) = max(0, D(t) - rho)
/// ```
///
/// and the tool rests at the maximum of `f`, since it stops at the highest contact
/// on the way down. `D(t)` is a distance and therefore convex, `max(0, ·)` preserves
/// convexity, and `sqrt(c^2 - ·^2)` is concave and decreasing, so the composition is
/// concave and the sum with a linear term stays concave. A concave function on an
/// interval is unimodal, so a ternary search converges — this is a bounded solve of a
/// known-shaped problem, not a hopeful approximation.
///
/// The feasible domain `D(t) <= rho + c` is itself an interval, found analytically.
/// Both endpoints are always evaluated, so the linear case `c = 0` lands exactly.
fn bullnose_edge_contact(p0: DVec3, p1: DVec3, centre: DVec2, rho: f64, c: f64) -> Option<f64> {
    edge_contact(p0, p1, centre, rho + c, |pz, d| {
        disc_ball_contact(pz, d, rho, c)
    })
}

/// Tip height at which a tool touches the segment `p0 -> p1`, for any tool whose
/// surface height `h` over horizontal distance is convex and increasing out to
/// `reach`: `contact(pz, d)` is `pz - h(d)`, the tip height touching one point.
///
/// `f(t) = contact(p(t).z, D(t))` is then concave, as for the bullnose, and its
/// maximum is found the same way: both ends of the feasible interval, and a
/// fixed-length ternary search between.
fn edge_contact(
    p0: DVec3,
    p1: DVec3,
    centre: DVec2,
    reach: f64,
    contact: impl Fn(f64, f64) -> Option<f64>,
) -> Option<f64> {
    let e = p1 - p0;
    let a = p0.truncate() - centre;
    let exy = e.truncate();

    // Feasible range of t, from |a + t*exy|^2 <= reach^2.
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    let qa = exy.length_squared();
    if qa < DEGENERATE {
        // Vertical edge: horizontal distance does not vary along it.
        if a.length() > reach {
            return None;
        }
    } else {
        let qb = 2.0 * a.dot(exy);
        let qc = a.length_squared() - reach * reach;
        let disc = qb * qb - 4.0 * qa * qc;
        if disc < 0.0 {
            return None;
        }
        let root = disc.sqrt();
        lo = lo.max((-qb - root) / (2.0 * qa));
        hi = hi.min((-qb + root) / (2.0 * qa));
        if lo > hi {
            return None;
        }
    }

    let f = |t: f64| -> Option<f64> {
        let d = (a + exy * t).length();
        contact(p0.z + e.z * t, d)
    };

    let mut best: Option<f64> = None;
    if let Some(v) = f(lo) {
        raise(&mut best, v);
    }
    if let Some(v) = f(hi) {
        raise(&mut best, v);
    }

    // Ternary search for an interior maximum of the concave f.
    let (mut left, mut right) = (lo, hi);
    for _ in 0..EDGE_SEARCH_STEPS {
        let third = (right - left) / 3.0;
        let (m1, m2) = (left + third, right - third);
        let (v1, v2) = (f(m1), f(m2));
        match (v1, v2) {
            (Some(a1), Some(a2)) if a1 < a2 => left = m1,
            (Some(_), Some(_)) => right = m2,
            _ => break,
        }
    }
    if let Some(v) = f((left + right) / 2.0) {
        raise(&mut best, v);
    }
    best
}

// ---------------------------------------------------------------------------
// Cone: V-bit, chamfer mill, drill point
// ---------------------------------------------------------------------------

impl DropTest for Cone {
    /// The surface over horizontal distance `d` is `h(d) = max(0, d - tip) * k`,
    /// with `k = 1 / tan(α)`: flat out to the tip, then a straight flank. A point
    /// at height `pz` and distance `d` is touched with the tip at `pz - h(d)`.
    ///
    /// Vertex: that directly. Edge: `h` is convex, so the shared concave search.
    /// Facet: over a plane rising at slope `s` along its gradient, the tip height
    /// touching the point at distance `r` along it is `z0 + s r - h(r)`, which
    /// peaks at the tip's rim when the plane is shallower than the flank (`s < k`)
    /// and at the full radius when steeper. Both are tried, each only where it
    /// lands inside the triangle; anywhere else the peak is on an edge or the rim,
    /// which the edge test and the other candidate cover.
    fn drop_triangle(&self, x: f64, y: f64, tri: &[DVec3; 3]) -> Option<f64> {
        let (tip, k, reach) = (self.tip(), self.rise(), self.max_radius());
        let centre = dvec2(x, y);
        let contact = |pz: f64, d: f64| (d <= reach).then(|| pz - (d - tip).max(0.0) * k);
        let mut best: Option<f64> = None;

        for v in tri {
            if let Some(z) = contact(v.z, (v.truncate() - centre).length()) {
                raise(&mut best, z);
            }
        }

        if let Some(n) = normal(tri)
            && n.z.abs() > 0.0
        {
            let grad = dvec2(-n.x / n.z, -n.y / n.z);
            let dir = if grad.length_squared() > DEGENERATE {
                grad.normalize()
            } else {
                DVec2::ZERO // Horizontal: every direction is as high.
            };
            for r in [tip, reach] {
                let at = centre + dir * r;
                let z = plane_z(n, tri[0], at.x, at.y);
                if point_in_triangle(dvec3(at.x, at.y, z), tri, n)
                    && let Some(tz) = contact(z, r)
                {
                    raise(&mut best, tz);
                }
            }
        }

        for (p0, p1) in edges(tri) {
            if let Some(z) = edge_contact(p0, p1, centre, reach, contact) {
                raise(&mut best, z);
            }
        }
        best
    }
}

impl DropTest for Cutter {
    fn drop_triangle(&self, x: f64, y: f64, tri: &[DVec3; 3]) -> Option<f64> {
        match self {
            Cutter::Bullnose(t) => t.drop_triangle(x, y, tri),
            Cutter::Cone(t) => t.drop_triangle(x, y, tri),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests;
