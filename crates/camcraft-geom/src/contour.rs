//! Closed 2D regions, and the operations roughing needs on them.
//!
//! This is the **second geometry channel**. The 3D strategies consume height fields;
//! everything areal — which region to clear at which depth, and later every 2D
//! operation — consumes contours. Collapsing the two would leave the entire 2D half
//! of a CAM system with nowhere to live.
//!
//! # Regions come out of a height field by marching squares
//!
//! "Where is the cutter-location surface above `z`?" is a contouring question. The
//! grid is exactly what it should be asked of: this is areal reasoning, not a path,
//! so grid resolution sets the accuracy and that is acceptable. Path points still
//! come from the exact query.
//!
//! # Offsetting is Clipper2's job
//!
//! Polygon offsetting with correct self-intersection handling is a solved problem
//! and not one to re-solve. Clipper2 works in scaled integers, which is what makes
//! it robust; the conversion is hidden here so nothing above deals in ticks.

use crate::heightmap::HeightMap;
use clipper2::{Centi, EndType, JoinType, Paths, difference, inflate, intersect, union};
use glam::{DVec2, dvec2};

/// A closed ring of points, in millimetres.
///
/// The closing edge is implicit: the last point joins the first.
#[derive(Debug, Clone, Default, PartialEq)]
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
    /// The clipping library rejected the operation.
    #[error("polygon {op} failed: {source}")]
    Clip {
        /// Which operation.
        op: &'static str,
        /// What the library said.
        source: clipper2::ClipperError,
    },
}

/// A set of rings forming one region, outers and holes together.
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

    /// Signed area. Positive is counter-clockwise; the sign distinguishes an outer
    /// boundary from a hole.
    pub fn signed_area(&self) -> f64 {
        let n = self.points.len();
        if n < 3 {
            return 0.0;
        }
        let mut sum = 0.0;
        for i in 0..n {
            let a = self.points.get(i).copied().unwrap_or(DVec2::ZERO);
            let b = self.points.get((i + 1) % n).copied().unwrap_or(DVec2::ZERO);
            sum += a.x * b.y - b.x * a.y;
        }
        sum / 2.0
    }

    /// Total length of the ring, including the closing edge.
    pub fn perimeter(&self) -> f64 {
        let n = self.points.len();
        if n < 2 {
            return 0.0;
        }
        (0..n)
            .filter_map(|i| {
                let a = self.points.get(i)?;
                let b = self.points.get((i + 1) % n)?;
                Some((*b - *a).length())
            })
            .sum()
    }
}

impl Region {
    /// A region from rings.
    pub fn new(rings: Vec<Contour>) -> Self {
        Self { rings }
    }

    /// The rings.
    pub fn rings(&self) -> &[Contour] {
        &self.rings
    }

    /// Whether the region has no rings.
    pub fn is_empty(&self) -> bool {
        self.rings.is_empty()
    }

    /// Net enclosed area: outers less holes.
    pub fn area(&self) -> f64 {
        self.rings
            .iter()
            .map(Contour::signed_area)
            .sum::<f64>()
            .abs()
    }

    /// Grow (positive) or shrink (negative) by `delta` millimetres.
    ///
    /// Round joins, because a cutter is round: a mitred corner would describe an
    /// offset no tool can actually produce.
    pub fn offset(&self, delta: f64) -> Self {
        if self.rings.is_empty() || !delta.is_finite() {
            return Self::default();
        }
        Self::from_paths(&inflate(
            self.to_paths(),
            delta,
            JoinType::Round,
            EndType::Polygon,
            2.0,
        ))
    }

    /// Everything in `self` that is not in `other`.
    pub fn difference(&self, other: &Self) -> Result<Self, RegionError> {
        if other.is_empty() {
            return Ok(self.clone());
        }
        difference(self.to_paths(), other.to_paths(), Default::default())
            .map(|p| Self::from_paths(&p))
            .map_err(|source| RegionError::Clip {
                op: "difference",
                source,
            })
    }

    /// Everything in both.
    pub fn intersection(&self, other: &Self) -> Result<Self, RegionError> {
        if self.is_empty() || other.is_empty() {
            return Ok(Self::default());
        }
        intersect(self.to_paths(), other.to_paths(), Default::default())
            .map(|p| Self::from_paths(&p))
            .map_err(|source| RegionError::Clip {
                op: "intersection",
                source,
            })
    }

    /// Everything in either, with overlaps merged.
    pub fn union(&self, other: &Self) -> Result<Self, RegionError> {
        if self.is_empty() {
            return Ok(other.clone());
        }
        if other.is_empty() {
            return Ok(self.clone());
        }
        union(self.to_paths(), other.to_paths(), Default::default())
            .map(|p| Self::from_paths(&p))
            .map_err(|source| RegionError::Clip {
                op: "union",
                source,
            })
    }

    /// An axis-aligned rectangle.
    pub fn rectangle(min: DVec2, max: DVec2) -> Self {
        Contour::new(vec![min, dvec2(max.x, min.y), max, dvec2(min.x, max.y)])
            .map_or_else(Self::default, |c| Self::new(vec![c]))
    }

    fn to_paths(&self) -> Paths<Centi> {
        let v: Vec<Vec<(f64, f64)>> = self
            .rings
            .iter()
            .map(|c| c.points.iter().map(|p| (p.x, p.y)).collect())
            .collect();
        Paths::<Centi>::from(v)
    }

    fn from_paths(paths: &Paths<Centi>) -> Self {
        let rings = paths
            .iter()
            .filter_map(|p| Contour::new(p.iter().map(|pt| dvec2(pt.x(), pt.y())).collect()))
            .collect();
        Self::new(rings)
    }
}

/// Trace the region where a height field is at or above `level`.
///
/// Marching squares over the grid. Cells whose value is unset count as *below*: an
/// unknown height is not somewhere to claim material exists.
///
/// The rings this produces are grid-resolution, which is right for an areal
/// question. Nothing here should ever become a path point.
pub fn contour_above(field: &HeightMap, level: f64) -> Region {
    let (nx, ny) = (field.nx(), field.ny());
    if nx == 0 || ny == 0 || !level.is_finite() {
        return Region::default();
    }
    // MAX_CELLS bounds the grid well inside i32, so these conversions are exact.
    let (nxi, nyi) = (
        i32::try_from(nx).unwrap_or(i32::MAX),
        i32::try_from(ny).unwrap_or(i32::MAX),
    );
    let pitch = field.pitch();
    let origin = field.origin();

    // The marching window runs one cell outside the grid on every side, and anything
    // outside counts as below. Without that border a field uniformly above the level
    // has no boundary anywhere inside the grid, so nothing is emitted at all — even
    // though the whole grid qualifies. That is the difference between "clear the
    // entire block" and "clear nothing".
    let pos = |ix: i32, iy: i32| -> DVec2 {
        origin + dvec2(f64::from(ix) * pitch, f64::from(iy) * pitch)
    };
    let inside = |ix: i32, iy: i32| -> bool {
        if ix < 0 || iy < 0 || ix >= nxi || iy >= nyi {
            return false;
        }
        let (ux, uy) = (
            u32::try_from(ix).unwrap_or(0),
            u32::try_from(iy).unwrap_or(0),
        );
        field.get(ux, uy).is_some_and(|v| v >= level)
    };

    let mut segs: Vec<(DVec2, DVec2)> = Vec::new();
    for iy in -1..nyi {
        for ix in -1..nxi {
            let (p00, p10, p01, p11) = (
                pos(ix, iy),
                pos(ix + 1, iy),
                pos(ix, iy + 1),
                pos(ix + 1, iy + 1),
            );
            let code = u8::from(inside(ix, iy))
                | u8::from(inside(ix + 1, iy)) << 1
                | u8::from(inside(ix + 1, iy + 1)) << 2
                | u8::from(inside(ix, iy + 1)) << 3;
            if code == 0 || code == 15 {
                continue;
            }

            let bottom = (p00 + p10) * 0.5;
            let right = (p10 + p11) * 0.5;
            let top = (p01 + p11) * 0.5;
            let left = (p00 + p01) * 0.5;

            // The classic sixteen cases. Ambiguous saddles (5, 10) are resolved by
            // joining the corners, which keeps a diagonal neck connected rather than
            // pinching it into two regions the cutter cannot reach between.
            match code {
                1 => segs.push((left, bottom)),
                2 => segs.push((bottom, right)),
                3 => segs.push((left, right)),
                4 => segs.push((right, top)),
                5 => {
                    segs.push((left, top));
                    segs.push((bottom, right));
                }
                6 => segs.push((bottom, top)),
                7 => segs.push((left, top)),
                8 => segs.push((top, left)),
                9 => segs.push((top, bottom)),
                10 => {
                    segs.push((top, right));
                    segs.push((left, bottom));
                }
                11 => segs.push((top, right)),
                12 => segs.push((right, left)),
                13 => segs.push((right, bottom)),
                14 => segs.push((bottom, left)),
                _ => {}
            }
        }
    }
    Region::new(chain(segs, pitch * 0.25))
}

/// Join loose segments into closed rings.
///
/// Bounded: every iteration consumes a segment, so the loop cannot outlive the
/// input however badly the segments are formed.
fn chain(segs: Vec<(DVec2, DVec2)>, weld: f64) -> Vec<Contour> {
    // Quantise endpoints so segments that meet are recognised as meeting. The grid
    // is fine relative to a cell, so distinct corners never collide.
    let q = (1.0 / weld.max(1.0e-9)).min(1.0e9);
    let key = |p: DVec2| -> (i64, i64) {
        let snap = |v: f64| -> i64 {
            let t = (v * q).round();
            if !t.is_finite() {
                return 0;
            }
            // Clamped into range, so the narrowing is exact.
            #[allow(clippy::cast_possible_truncation)]
            let t = t.clamp(-9.0e15, 9.0e15) as i64;
            t
        };
        (snap(p.x), snap(p.y))
    };

    let mut starts: std::collections::HashMap<(i64, i64), Vec<usize>> =
        std::collections::HashMap::new();
    for (i, s) in segs.iter().enumerate() {
        starts.entry(key(s.0)).or_default().push(i);
    }

    let mut used = vec![false; segs.len()];
    let mut rings = Vec::new();
    for seed in 0..segs.len() {
        if used.get(seed).copied().unwrap_or(true) {
            continue;
        }
        if let Some(u) = used.get_mut(seed) {
            *u = true;
        }
        let Some(first) = segs.get(seed) else {
            continue;
        };
        let origin = first.0;
        let mut pts = vec![first.0, first.1];
        let mut cursor = first.1;

        // Bounded: every step consumes a segment, so the walk cannot outlive the
        // input however badly the segments are formed.
        for _ in 0..segs.len() {
            let Some(cands) = starts.get(&key(cursor)) else {
                break;
            };
            let Some(&next) = cands
                .iter()
                .find(|i| !used.get(**i).copied().unwrap_or(true))
            else {
                break;
            };
            if let Some(u) = used.get_mut(next) {
                *u = true;
            }
            let Some(seg) = segs.get(next) else { break };
            cursor = seg.1;
            if key(cursor) == key(origin) {
                break;
            }
            pts.push(cursor);
        }
        if let Some(c) = Contour::new(pts) {
            rings.push(c);
        }
    }
    rings
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

    fn square(half: f64) -> Region {
        Region::rectangle(dvec2(-half, -half), dvec2(half, half))
    }

    #[test]
    fn a_ring_needs_three_points() {
        assert!(Contour::new(vec![dvec2(0.0, 0.0), dvec2(1.0, 0.0)]).is_none());
        assert!(Contour::new(vec![dvec2(0.0, 0.0), dvec2(1.0, 0.0), dvec2(0.0, 1.0)]).is_some());
        assert!(
            Contour::new(vec![dvec2(f64::NAN, 0.0), dvec2(1.0, 0.0), dvec2(0.0, 1.0)]).is_none()
        );
    }

    #[test]
    fn area_and_perimeter_are_right() {
        let r = square(5.0);
        assert!((r.area() - 100.0).abs() < 1e-9);
        assert!((r.rings()[0].perimeter() - 40.0).abs() < 1e-9);
    }

    /// The sign is how an outer boundary is told from a hole.
    #[test]
    fn winding_shows_in_the_sign() {
        let ccw = Contour::new(vec![
            dvec2(0.0, 0.0),
            dvec2(2.0, 0.0),
            dvec2(2.0, 2.0),
            dvec2(0.0, 2.0),
        ])
        .unwrap();
        let mut cw_pts = ccw.points().to_vec();
        cw_pts.reverse();
        let cw = Contour::new(cw_pts).unwrap();
        assert!(ccw.signed_area() > 0.0);
        assert!(cw.signed_area() < 0.0);
    }

    #[test]
    fn offsetting_grows_and_shrinks() {
        let r = square(5.0);
        let bigger = r.offset(1.0);
        let smaller = r.offset(-1.0);
        assert!(bigger.area() > r.area(), "outward offset must grow");
        assert!(smaller.area() < r.area(), "inward offset must shrink");
        // 10x10 grown by 1 with round corners: 144 minus the corner shortfall.
        assert!((bigger.area() - 143.1).abs() < 1.0, "got {}", bigger.area());
        assert!(
            (smaller.area() - 64.0).abs() < 0.5,
            "got {}",
            smaller.area()
        );
    }

    /// Shrinking past the middle must vanish, not invert.
    #[test]
    fn over_shrinking_produces_nothing() {
        let gone = square(2.0).offset(-5.0);
        assert!(
            gone.is_empty() || gone.area() < 1e-6,
            "area {}",
            gone.area()
        );
    }

    #[test]
    fn booleans_behave() {
        let a = Region::rectangle(dvec2(0.0, 0.0), dvec2(10.0, 10.0));
        let b = Region::rectangle(dvec2(5.0, 0.0), dvec2(15.0, 10.0));
        assert!((a.intersection(&b).unwrap().area() - 50.0).abs() < 0.1);
        assert!((a.union(&b).unwrap().area() - 150.0).abs() < 0.1);
        assert!((a.difference(&b).unwrap().area() - 50.0).abs() < 0.1);
        assert!(a.difference(&a).unwrap().area() < 0.1);
    }

    #[test]
    fn empty_operands_are_handled() {
        let a = square(3.0);
        let e = Region::default();
        assert_eq!(a.difference(&e).unwrap().area(), a.area());
        assert!(a.intersection(&e).unwrap().is_empty());
        assert!((a.union(&e).unwrap().area() - a.area()).abs() < 1e-9);
        assert!(e.offset(1.0).is_empty());
    }

    /// The question roughing actually asks: where is the surface above this level?
    #[test]
    fn contours_a_plateau_out_of_a_field() {
        // A 20x20 field, everything at 0 except a 6x6 block at 10.
        let mut f = HeightMap::filled(dvec2(0.0, 0.0), 1.0, 20, 20, 0.0).unwrap();
        for iy in 7..13 {
            for ix in 7..13 {
                f.set(ix, iy, 10.0);
            }
        }
        let r = contour_above(&f, 5.0);
        assert!(!r.is_empty(), "the plateau should produce a ring");
        // The boundary runs through the midpoints between inside and outside cell
        // centres, so six cells at pitch 1 enclose a 6 x 6 region: area 36. The four
        // corners are cut diagonally, each removing half of a 0.5 x 0.5 square, so
        // the exact answer is 36 - 4 * 0.125 = 35.5.
        assert!(
            (r.area() - 35.5).abs() < 0.01,
            "expected 35.5 from the marching-squares geometry, got {}",
            r.area()
        );
    }

    #[test]
    fn a_level_above_everything_contours_nothing() {
        let f = HeightMap::filled(dvec2(0.0, 0.0), 1.0, 10, 10, 1.0).unwrap();
        assert!(contour_above(&f, 5.0).is_empty());
    }

    /// An unset cell means the height is unknown. Claiming material there would let
    /// roughing cut where nothing has been established.
    #[test]
    fn unset_cells_count_as_below() {
        let mut f = HeightMap::filled(dvec2(0.0, 0.0), 1.0, 10, 10, f64::NAN).unwrap();
        for iy in 3..7 {
            for ix in 3..7 {
                f.set(ix, iy, 10.0);
            }
        }
        let r = contour_above(&f, 1.0);
        assert!(!r.is_empty());
        assert!(r.area() < 30.0, "only the known block, got {}", r.area());
    }

    /// A field entirely above the level has no boundary *inside* the grid. Without
    /// a border of "outside", marching squares emits nothing and the caller reads
    /// that as "nothing to cut" -- the difference between clearing the whole block
    /// and clearing none of it.
    #[test]
    fn a_uniformly_high_field_contours_its_whole_extent() {
        let f = HeightMap::filled(dvec2(0.0, 0.0), 1.0, 10, 10, 10.0).unwrap();
        let r = contour_above(&f, 5.0);
        assert!(!r.is_empty(), "a full field must contour to its own extent");
        // Cell centres span 0..9, and the boundary sits half a cell beyond each
        // edge, so the ring encloses 10 x 10 less the four cut corners.
        assert!(
            (r.area() - 99.5).abs() < 0.01,
            "expected the full extent, got {}",
            r.area()
        );
    }

    #[test]
    fn degenerate_fields_do_not_panic() {
        // One cell above the level is a real, if tiny, region: a single cell
        // surrounded by outside. It has a boundary, so it contours.
        let tiny = HeightMap::filled(dvec2(0.0, 0.0), 1.0, 1, 1, 1.0).unwrap();
        assert!(!contour_above(&tiny, 0.0).is_empty());
        assert!(
            contour_above(&tiny, 5.0).is_empty(),
            "below the level: nothing"
        );

        let f = HeightMap::filled(dvec2(0.0, 0.0), 1.0, 4, 4, 1.0).unwrap();
        assert!(contour_above(&f, f64::NAN).is_empty());
    }
}
