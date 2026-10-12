//! Closed 2D regions, and the operations on them that clearing, boundaries and the
//! 2D cuts need.
//!
//! This is the **second geometry channel**. The 3D strategies consume height fields;
//! everything areal — which region to clear at which depth, a boundary picked on a
//! model, the rings a profile or a pocket follows — consumes contours. Collapsing
//! the two would leave the entire 2D half of a CAM system with nowhere to live.
//!
//! # Grids are only for stock
//!
//! [`contour_above`] traces a height field by marching squares, and is for the
//! simulated **stock** only: where material still stands. Its rings are
//! grid-resolution staircases, so no path may run along them. Every boundary a
//! toolpath follows comes from the model's own geometry — see
//! [`crate::shadow`] — and a stock contour may only decide *whether* a path cuts
//! somewhere, by clipping, never *where* it runs.
//!
//! # Offsetting is Clipper2's job
//!
//! Polygon offsetting with correct self-intersection handling is a solved problem
//! and not one to re-solve. Clipper2 works in scaled integers, which is what makes
//! it robust; the conversion is hidden here so nothing above deals in ticks.

mod clip;
mod stock;

pub use stock::contour_above;

use clipper2_rust::{Path64, PointInPolygonResult, point_in_polygon};
use glam::{DVec2, dvec2};

/// A closed ring of points, in millimetres.
///
/// The closing edge is implicit: the last point joins the first.
#[derive(Debug, Clone, PartialEq)]
pub struct Contour {
    points: Vec<DVec2>,
}

/// A region operation that failed.
///
/// Returned rather than swallowed. An empty region reads as "nothing to cut", which
/// produces a program that moves nowhere — safe, but indistinguishable from a
/// correct answer, and that is exactly the silence these rules exist to prevent.
#[derive(Debug, thiserror::Error)]
pub enum RegionError {
    /// The clipping library reported that an operation failed.
    #[error("polygon {op} failed")]
    Clip {
        /// Which operation.
        op: &'static str,
    },
}

/// A set of rings forming one region, outers and holes together.
///
/// Outers wind counter-clockwise and holes clockwise, so the net signed area is the
/// region's area. Every constructor establishes that, because offsetting reads the
/// winding to tell inside from outside.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Region {
    rings: Vec<Contour>,
}

impl Contour {
    /// A ring from points. Fewer than three is not an area.
    pub fn new(points: Vec<DVec2>) -> Option<Self> {
        (points.len() >= 3 && points.iter().all(|p| p.is_finite())).then_some(Self { points })
    }

    /// The points, in order.
    pub fn points(&self) -> &[DVec2] {
        &self.points
    }

    /// How many points the ring has.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// Whether the ring is empty.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Its edges, in order, each from a point to the next: the closing one, from
    /// the last point back to the first, comes last.
    pub fn edges(&self) -> impl Iterator<Item = (DVec2, DVec2)> + '_ {
        let next = self.points.iter().cycle().skip(1);
        self.points.iter().zip(next).map(|(&a, &b)| (a, b))
    }

    /// Signed area. Positive is counter-clockwise; the sign distinguishes an outer
    /// boundary from a hole.
    pub fn signed_area(&self) -> f64 {
        if self.points.len() < 3 {
            return 0.0;
        }
        let sum = self
            .edges()
            .fold(0.0, |sum, (a, b)| sum + (a.x * b.y - b.x * a.y));
        sum / 2.0
    }
}

impl Region {
    /// The rings.
    pub fn rings(&self) -> &[Contour] {
        &self.rings
    }

    /// Its connected pieces: each outer ring with the holes directly inside it.
    pub fn pieces(&self) -> Vec<Region> {
        let areas: Vec<f64> = self.rings.iter().map(Contour::signed_area).collect();
        let outers: Vec<(&Contour, Path64, f64)> = self
            .rings
            .iter()
            .zip(&areas)
            .filter(|&(_, &a)| a > 0.0)
            .map(|(r, &a)| (r, clip::to_path(&r.points), a))
            .collect();
        let mut groups: Vec<Vec<Contour>> =
            outers.iter().map(|(r, ..)| vec![(*r).clone()]).collect();
        for (hole, _) in self.rings.iter().zip(&areas).filter(|&(_, &a)| a <= 0.0) {
            let ticks = clip::to_path(&hole.points);
            // The smallest outer round it is the one it is a hole of.
            let owner = outers
                .iter()
                .enumerate()
                .filter(|(_, (_, outer, _))| encloses(outer, &ticks))
                .min_by(|a, b| a.1.2.total_cmp(&b.1.2))
                .map(|(k, _)| k);
            if let Some(group) = owner.and_then(|k| groups.get_mut(k)) {
                group.push(hole.clone());
            }
        }
        groups.into_iter().map(Region::new).collect()
    }

    /// Whether the region has no rings.
    pub fn is_empty(&self) -> bool {
        self.rings.is_empty()
    }

    /// Whether `p` lies inside, by the even-odd rule over every ring.
    ///
    /// A point exactly on a boundary may go either way.
    pub fn contains(&self, p: DVec2) -> bool {
        let mut inside = false;
        for (a, b) in self.rings.iter().flat_map(Contour::edges) {
            if (a.y > p.y) != (b.y > p.y) {
                let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if p.x < x {
                    inside = !inside;
                }
            }
        }
        inside
    }

    /// Whether the whole segment `a`–`b` lies inside: both ends inside, and no
    /// edge crossing it.
    ///
    /// A segment touching the boundary may go either way, as a point on it does.
    pub fn contains_segment(&self, a: DVec2, b: DVec2) -> bool {
        if !self.contains(a) || !self.contains(b) {
            return false;
        }
        let (lo, hi) = (a.min(b), a.max(b));
        let cross = |o: DVec2, p: DVec2, q: DVec2| (p - o).perp_dot(q - o);
        for (c, d) in self.rings.iter().flat_map(Contour::edges) {
            if c.max(d).cmplt(lo).any() || c.min(d).cmpgt(hi).any() {
                continue;
            }
            let (d1, d2) = (cross(a, b, c), cross(a, b, d));
            let (d3, d4) = (cross(c, d, a), cross(c, d, b));
            if (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0) {
                return false;
            }
        }
        true
    }

    /// Where the line through the region at `v` runs inside it, as intervals of
    /// `u`, in order.
    ///
    /// The line is every point `p` with `p · across == v`, where `across` is
    /// `along` turned a quarter left; `u` is `p · along`. `along` must be a unit
    /// vector. Exact in `f64`, by the same half-open crossing rule as
    /// [`contains`](Self::contains), so a span's inside agrees with it. Intervals
    /// of no length are left out.
    pub fn spans(&self, along: DVec2, v: f64) -> Vec<(f64, f64)> {
        let across = along.perp();
        let mut crossings = Vec::new();
        for (a, b) in self.rings.iter().flat_map(Contour::edges) {
            let (va, vb) = (a.dot(across), b.dot(across));
            if (va > v) != (vb > v) {
                let t = (v - va) / (vb - va);
                crossings.push((a + (b - a) * t).dot(along));
            }
        }
        crossings.sort_by(f64::total_cmp);
        crossings
            .as_chunks::<2>()
            .0
            .iter()
            .filter(|[a, b]| b > a)
            .map(|&[a, b]| (a, b))
            .collect()
    }

    /// The smallest box holding every ring, or `None` for an empty region.
    pub fn bounds(&self) -> Option<(DVec2, DVec2)> {
        let mut pts = self.rings.iter().flat_map(|r| r.points().iter().copied());
        let first = pts.next()?;
        Some(pts.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }

    /// Net enclosed area: outers less holes.
    pub fn area(&self) -> f64 {
        self.rings
            .iter()
            .map(Contour::signed_area)
            .sum::<f64>()
            .abs()
    }

    /// An axis-aligned rectangle.
    pub fn rectangle(min: DVec2, max: DVec2) -> Self {
        Contour::new(vec![min, dvec2(max.x, min.y), max, dvec2(min.x, max.y)])
            .map_or_else(Self::default, |c| Self { rings: vec![c] })
    }
}

/// Whether the ring `inner`, which crosses no edge of `outer`, lies inside it, both
/// in ticks.
///
/// Told by the first of its points not on `outer`'s boundary, since the clipper's
/// rings may touch at a point. One lying wholly on the boundary is inside.
fn encloses(outer: &Path64, inner: &Path64) -> bool {
    inner
        .iter()
        .map(|&p| point_in_polygon(p, outer))
        .find(|&r| r != PointInPolygonResult::IsOn)
        .is_none_or(|r| r == PointInPolygonResult::IsInside)
}

/// The length of an open polyline.
pub fn polyline_length(points: &[DVec2]) -> f64 {
    points
        .iter()
        .zip(points.iter().skip(1))
        .map(|(a, b)| a.distance(*b))
        .sum()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests;
