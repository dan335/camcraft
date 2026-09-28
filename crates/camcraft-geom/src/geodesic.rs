//! Distance along a lifted surface, from seeds: fast marching on a [`TriGrid`].
//!
//! # Why fast marching
//!
//! Dijkstra over a grid measures along its edges, so the distance from a point
//! grows as a polygon, not a circle: with sixteen neighbours, a sixteen-sided one.
//! Lines of equal distance then have kinks at its corners, and are spaced closer
//! across some directions than others. Fast marching updates a node from the
//! front crossing a whole triangle (Kimmel and Sethian, 1998), so a plane front
//! is carried exactly and a curved one to first order in the pitch.
//!
//! # Deterministic
//!
//! Nodes are settled in order of distance, ties by node number, so the same input
//! gives the same field on any machine.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use glam::DVec3;

use crate::trigrid::TriGrid;

/// A node whose distance is known to start with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seed {
    /// Which node.
    pub node: u32,
    /// Its distance, in millimetres; not negative.
    pub distance: f64,
}

/// A field of distances, one a node.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Distances {
    /// Each node's distance from the seeds; infinite where none was reached.
    pub d: Vec<f64>,
    /// Defined nodes no front reached.
    pub unreached: u64,
}

impl Distances {
    /// Node `n`'s distance, if it was reached.
    pub fn at(&self, n: u32) -> Option<f64> {
        let d = *self.d.get(usize::try_from(n).ok()?)?;
        d.is_finite().then_some(d)
    }
}

/// The distance of every defined node of `grid` from `seeds`, measured across its
/// triangles with each node at `position`.
///
/// `position` is where a node is in 3D: the grid's own lifted point, or somewhere
/// else the surface is measured at. A seed may be lowered by the march, if a
/// shorter way to it is found.
pub fn march(grid: &TriGrid, seeds: &[Seed], position: impl Fn(u32) -> Option<DVec3>) -> Distances {
    let count = usize::try_from(grid.nodes()).unwrap_or(0);
    let mut d = vec![f64::INFINITY; count];
    let mut known = vec![false; count];
    let mut heap: BinaryHeap<Reverse<(u64, u32)>> = BinaryHeap::new();

    for s in seeds {
        if !(s.distance.is_finite() && s.distance >= 0.0) || !grid.defined(s.node) {
            continue;
        }
        if let Some(slot) = usize::try_from(s.node).ok().and_then(|i| d.get_mut(i))
            && s.distance < *slot
        {
            *slot = s.distance;
            // Not negative, so the bits order as the values do.
            heap.push(Reverse((s.distance.to_bits(), s.node)));
        }
    }

    let value = |d: &[f64], n: u32| {
        usize::try_from(n)
            .ok()
            .and_then(|i| d.get(i))
            .copied()
            .unwrap_or(f64::INFINITY)
    };
    let is_known = |known: &[bool], n: u32| {
        usize::try_from(n)
            .ok()
            .and_then(|i| known.get(i))
            .copied()
            .unwrap_or(true)
    };

    while let Some(Reverse((bits, node))) = heap.pop() {
        if is_known(&known, node) || value(&d, node).to_bits() != bits {
            continue;
        }
        if let Some(k) = usize::try_from(node).ok().and_then(|i| known.get_mut(i)) {
            *k = true;
        }
        let Some(p) = position(node) else { continue };
        let t_node = value(&d, node);
        for t in grid.triangles_at(node) {
            let Some(nodes) = grid.triangle(t) else {
                continue;
            };
            for &c in &nodes {
                if c == node || is_known(&known, c) {
                    continue;
                }
                let Some(pc) = position(c) else { continue };
                let other = nodes.iter().copied().find(|&m| m != node && m != c);
                let candidate = match other {
                    Some(o) if is_known(&known, o) => match position(o) {
                        Some(po) => update((p, t_node), (po, value(&d, o)), pc),
                        None => t_node + p.distance(pc),
                    },
                    _ => t_node + p.distance(pc),
                };
                if let Some(slot) = usize::try_from(c).ok().and_then(|i| d.get_mut(i))
                    && candidate < *slot
                {
                    *slot = candidate;
                    heap.push(Reverse((candidate.to_bits(), c)));
                }
            }
        }
    }

    let unreached = (0..grid.nodes())
        .filter(|&n| grid.defined(n) && !value(&d, n).is_finite())
        .count() as u64;
    Distances { d, unreached }
}

/// The distance at `c` from a front known at `a` and `b`, the other corners of a
/// triangle: across the triangle where the front comes through it, else along the
/// nearer edge.
fn update(a: (DVec3, f64), b: (DVec3, f64), c: DVec3) -> f64 {
    // A the nearer to the front.
    let ((pa, ta), (pb, tb)) = if a.1 <= b.1 { (a, b) } else { (b, a) };
    let along_edges = (ta + pa.distance(c)).min(tb + pb.distance(c));
    let u = tb - ta;
    let (ca, cb) = (pa - c, pb - c);
    let (edge_b, edge_a) = (ca.length(), cb.length());
    if edge_a <= 0.0 || edge_b <= 0.0 {
        return along_edges;
    }
    let cos = (ca.dot(cb) / (edge_a * edge_b)).clamp(-1.0, 1.0);
    let sin2 = 1.0 - cos * cos;
    // Kimmel and Sethian's quadratic in t, the time past A the front reaches C.
    let qa = edge_a * edge_a + edge_b * edge_b - 2.0 * edge_a * edge_b * cos;
    let qb = 2.0 * edge_b * u * (edge_a * cos - edge_b);
    let qc = edge_b * edge_b * (u * u - edge_a * edge_a * sin2);
    let disc = qb * qb - 4.0 * qa * qc;
    if qa <= 0.0 || disc < 0.0 {
        return along_edges;
    }
    let t = (-qb + disc.sqrt()) / (2.0 * qa);
    // The front must come through the triangle: arrive after it passed B, from a
    // direction between the two edges.
    let upper = if cos > 1e-12 {
        edge_a / cos
    } else if cos > -1e-12 {
        f64::INFINITY
    } else {
        f64::NEG_INFINITY
    };
    if t > u && t > 0.0 {
        let ratio = edge_b * (t - u) / t;
        if edge_a * cos < ratio && ratio < upper {
            return (ta + t).min(along_edges);
        }
    }
    along_edges
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]
mod tests {
    use super::*;
    use glam::{DVec2, dvec2};

    /// A grid of `n × n` nodes `pitch` apart with heights from `height`.
    fn grid(n: u32, pitch: f64, height: impl Fn(DVec2) -> f64) -> TriGrid {
        let mut z = Vec::new();
        for iy in 0..n {
            for ix in 0..n {
                z.push(height(dvec2(f64::from(ix), f64::from(iy)) * pitch) as f32);
            }
        }
        let count = z.len();
        TriGrid::new(DVec2::ZERO, pitch, n, n, z, vec![true; count]).unwrap()
    }

    /// A plane front, on a flat grid and a sloping one, at an angle to the grid:
    /// each node's distance is exact, whatever the diagonals. On a plane the
    /// distance along the surface from a line on it is the straight 3D distance.
    #[test]
    fn a_plane_wave_is_carried_exactly() {
        let angle = 17f64.to_radians();
        let normal = dvec2(angle.cos(), angle.sin());
        let along = normal.perp();
        for (sx, sy) in [(0.0, 0.0), (0.5, 0.3)] {
            let g = grid(41, 0.25, |p| sx * p.x + sy * p.y);
            // The front: the line through the middle along `along`, on the plane.
            let centre = dvec2(5.0, 5.0);
            let origin = glam::dvec3(centre.x, centre.y, sx * centre.x + sy * centre.y);
            let dir = glam::dvec3(along.x, along.y, sx * along.x + sy * along.y).normalize();
            let side = |n: u32| (g.xy(n).unwrap() - centre).dot(normal);
            // Signed, and shifted so every seed is positive: the whole side behind
            // the front is seeded, so nothing reaches ahead of it but the front.
            let exact = |n: u32| {
                let p = g.point(n).unwrap() - origin;
                (p - p.dot(dir) * dir).length().copysign(side(n)) + 20.0
            };
            let seeds: Vec<Seed> = (0..g.nodes())
                .filter(|&n| side(n) < 0.6)
                .map(|n| Seed {
                    node: n,
                    distance: exact(n),
                })
                .collect();
            let field = march(&g, &seeds, |n| g.point(n));
            assert_eq!(field.unreached, 0);
            // Only where the way back to the front stays well inside the grid: a node
            // on its edge is reached from outside it, which no triangle carries, and
            // what it gets wrong is carried a row or two in.
            let inland = |n: u32| {
                let foot = g.xy(n).unwrap() - side(n) * normal;
                foot.cmpge(DVec2::splat(1.5)).all() && foot.cmple(DVec2::splat(8.5)).all()
            };
            let worst = (0..g.nodes())
                .filter(|&n| side(n) >= 1.0 && inland(n))
                .map(|n| (field.at(n).unwrap() - exact(n)).abs())
                .fold(0.0, f64::max);
            // Exact but for what the grid's edges carry in, which dies away fast:
            // nanometres.
            assert!(worst < 1e-5, "slope ({sx}, {sy}): {worst}");
        }
    }

    /// Distance from a point grows as a circle, to first order in the pitch, and
    /// the error falls as the pitch does. A point is the hardest case: the front
    /// is most curved where it starts.
    #[test]
    fn a_point_source_is_first_order() {
        let error = |pitch: f64| {
            let n = (10.0 / pitch) as u32 + 1;
            let g = grid(n, pitch, |_| 0.0);
            let centre = dvec2(5.0, 5.0);
            // A seeded disc two pitches wide, exactly.
            let seeds: Vec<Seed> = (0..g.nodes())
                .filter(|&m| g.xy(m).unwrap().distance(centre) <= 2.0 * pitch + 1e-9)
                .map(|m| Seed {
                    node: m,
                    distance: g.xy(m).unwrap().distance(centre),
                })
                .collect();
            let field = march(&g, &seeds, |m| g.point(m));
            (0..g.nodes())
                .filter(|&m| {
                    let r = g.xy(m).unwrap().distance(centre);
                    (3.0..4.5).contains(&r)
                })
                .map(|m| (field.at(m).unwrap() - g.xy(m).unwrap().distance(centre)).abs())
                .fold(0.0, f64::max)
        };
        let coarse = error(0.1);
        let fine = error(0.05);
        // First order: within about a pitch and a half, and falling with it.
        assert!(coarse < 0.15, "coarse {coarse}");
        assert!(fine < coarse * 0.75, "fine {fine}, coarse {coarse}");
    }

    /// Down a step, the distance grows by the step's height: the front walks the
    /// wall.
    #[test]
    fn the_front_walks_down_a_wall() {
        let g = grid(41, 0.25, |p| if p.x < 5.0 { 3.0 } else { 0.0 });
        let seeds: Vec<Seed> = (0..g.nodes())
            .filter(|&n| g.xy(n).unwrap().x < 1e-9)
            .map(|n| Seed {
                node: n,
                distance: 0.0,
            })
            .collect();
        let field = march(&g, &seeds, |n| g.point(n));
        let n = g.node(32, 20).unwrap();
        let x = g.xy(n).unwrap().x;
        let expected = x + 3.0 - 0.25;
        assert!(
            (field.at(n).unwrap() - expected).abs() < 0.25 + 1e-9,
            "{} vs {expected}",
            field.at(n).unwrap()
        );
    }

    #[test]
    fn nodes_no_front_reaches_are_counted() {
        let g = grid(5, 1.0, |_| 0.0);
        let field = march(&g, &[], |n| g.point(n));
        assert_eq!(field.unreached, 25);
        assert!(field.at(0).is_none());
    }
}
