//! Closed 2D regions, and the operations roughing needs on them.
//!
//! This is the **second geometry channel**. The 3D strategies consume height fields;
//! everything areal — which region to clear at which depth, and later every 2D
//! operation — consumes contours. Collapsing the two would leave the entire 2D half
//! of a CAM system with nowhere to live.
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

use crate::heightmap::HeightMap;
use clipper2_rust::{
    ClipType, Clipper64, ClipperOffset, EndType, FillRule, JoinType, Path64, Paths64, Point64,
};
use glam::{DVec2, dvec2};

/// Integer ticks per millimetre. A tick is 0.1 µm: rounding to it moves a point by
/// at most 0.07 µm, far inside any machining tolerance, while a 10 m part still
/// sits many orders of magnitude inside Clipper's coordinate range.
const TICKS_PER_MM: f64 = 10_000.0;

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
    /// A region from rings that do not cross one another, in any winding.
    ///
    /// Rings are read even-odd, so a ring inside another is a hole whichever way it
    /// winds, and the result is rewound to the convention the type keeps.
    pub fn new(rings: Vec<Contour>) -> Self {
        let raw = Self { rings };
        if raw.rings.is_empty() {
            return raw;
        }
        boolean(
            ClipType::Union,
            &raw.to_paths(),
            &Paths64::new(),
            FillRule::EvenOdd,
        )
        .map_or_else(Self::default, |p| Self::from_paths(&p))
    }

    /// Everything covered by any of `polygons`, which may overlap one another.
    ///
    /// Each polygon counts once however it winds, so a triangle seen from below
    /// cannot cancel one seen from above.
    pub fn union_of(polygons: &[Contour]) -> Result<Self, RegionError> {
        Self::default().with_polygons(polygons)
    }

    /// This region together with `polygons`, which may overlap it and one another.
    ///
    /// The polygons are rewound counter-clockwise; this region's own holes keep
    /// their winding, so a hole stays a hole unless a polygon covers it.
    pub fn with_polygons(&self, polygons: &[Contour]) -> Result<Self, RegionError> {
        let mut paths = self.to_paths();
        paths.extend(polygons.iter().map(|c| {
            let mut p = to_path(&c.points);
            if c.signed_area() < 0.0 {
                p.reverse();
            }
            p
        }));
        if paths.is_empty() {
            return Ok(Self::default());
        }
        boolean(ClipType::Union, &paths, &Paths64::new(), FillRule::NonZero)
            .map(|p| Self::from_paths(&p))
            .ok_or(RegionError::Clip { op: "union" })
    }

    /// The rings.
    pub fn rings(&self) -> &[Contour] {
        &self.rings
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
        for ring in &self.rings {
            let pts = ring.points();
            let n = pts.len();
            for i in 0..n {
                let (Some(a), Some(b)) = (pts.get(i), pts.get((i + 1) % n)) else {
                    continue;
                };
                if (a.y > p.y) != (b.y > p.y) {
                    let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                    if p.x < x {
                        inside = !inside;
                    }
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
        for ring in &self.rings {
            let pts = ring.points();
            let n = pts.len();
            for i in 0..n {
                let (Some(&c), Some(&d)) = (pts.get(i), pts.get((i + 1) % n)) else {
                    continue;
                };
                if c.max(d).cmplt(lo).any() || c.min(d).cmpgt(hi).any() {
                    continue;
                }
                let (d1, d2) = (cross(a, b, c), cross(a, b, d));
                let (d3, d4) = (cross(c, d, a), cross(c, d, b));
                if (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0) {
                    return false;
                }
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
        for ring in &self.rings {
            let pts = ring.points();
            let n = pts.len();
            for i in 0..n {
                let (Some(a), Some(b)) = (pts.get(i), pts.get((i + 1) % n)) else {
                    continue;
                };
                let (va, vb) = (a.dot(across), b.dot(across));
                if (va > v) != (vb > v) {
                    let t = (v - va) / (vb - va);
                    crossings.push((*a + (*b - *a) * t).dot(along));
                }
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

    /// Grow (positive) or shrink (negative) by `delta` millimetres, erring outward
    /// when growing and inward when shrinking.
    ///
    /// Round joins, because a cutter is round: a mitred corner would describe an
    /// offset no tool can actually produce. A round join is drawn as chords whose
    /// ends lie on the true arc, so the chords fall short of it by up to
    /// `arc_tolerance`. Offsetting by that much more than asked makes the error
    /// one-sided: a grown region always **contains** the exact offset, and a shrunk
    /// one always lies **within** it, each by at most `arc_tolerance`. That is the
    /// side a forbidden region and the room left inside it have to err on.
    pub fn offset(&self, delta: f64, arc_tolerance: f64) -> Self {
        if self.rings.is_empty() || !delta.is_finite() || !arc_tolerance.is_finite() {
            return Self::default();
        }
        if delta == 0.0 {
            return self.clone();
        }
        let tol = arc_tolerance.max(1.0 / TICKS_PER_MM);
        let padded = delta + tol.copysign(delta);
        let mut out = Paths64::new();
        let mut off = ClipperOffset::new(2.0, tol * TICKS_PER_MM, false, false);
        off.add_paths(&self.to_paths(), JoinType::Round, EndType::Polygon);
        off.execute(padded * TICKS_PER_MM, &mut out);
        Self::from_paths(&out)
    }

    /// Grown by at least `delta` everywhere, with mitred or squared corners.
    ///
    /// Contains the round offset, with far fewer points: for a region that only
    /// decides coverage, never the shape of a path — such as where stock stands.
    pub fn grow_covering(&self, delta: f64) -> Self {
        if self.rings.is_empty() || !(delta.is_finite() && delta > 0.0) {
            return self.clone();
        }
        let mut out = Paths64::new();
        let mut off = ClipperOffset::new(2.0, 0.0, false, false);
        off.add_paths(&self.to_paths(), JoinType::Miter, EndType::Polygon);
        off.execute(delta * TICKS_PER_MM, &mut out);
        Self::from_paths(&out)
    }

    /// Everything within `radius` of any of the polylines, with round ends, erring
    /// outward by at most `arc_tolerance` as [`offset`](Self::offset) does.
    pub fn around_polylines(lines: &[Vec<DVec2>], radius: f64, arc_tolerance: f64) -> Self {
        if lines.is_empty() || !(radius.is_finite() && radius > 0.0) {
            return Self::default();
        }
        let tol = arc_tolerance.max(1.0 / TICKS_PER_MM);
        let paths: Paths64 = lines.iter().map(|l| to_path(l)).collect();
        let mut out = Paths64::new();
        let mut off = ClipperOffset::new(2.0, tol * TICKS_PER_MM, false, false);
        off.add_paths(&paths, JoinType::Round, EndType::Round);
        off.execute((radius + tol) * TICKS_PER_MM, &mut out);
        Self::from_paths(&out)
    }

    /// Everything in any of `regions`, in one pass.
    ///
    /// Each region winds to 1 inside and 0 outside, holes included, so their sum is
    /// non-zero exactly where one of them covers.
    pub fn union_all(regions: &[Self]) -> Result<Self, RegionError> {
        let paths: Paths64 = regions.iter().flat_map(Self::to_paths).collect();
        if paths.is_empty() {
            return Ok(Self::default());
        }
        boolean(ClipType::Union, &paths, &Paths64::new(), FillRule::NonZero)
            .map(|p| Self::from_paths(&p))
            .ok_or(RegionError::Clip { op: "union" })
    }

    /// Everything in `self` that is not in `other`.
    pub fn difference(&self, other: &Self) -> Result<Self, RegionError> {
        if other.is_empty() {
            return Ok(self.clone());
        }
        self.op(ClipType::Difference, other, "difference")
    }

    /// Everything in both.
    pub fn intersection(&self, other: &Self) -> Result<Self, RegionError> {
        if self.is_empty() || other.is_empty() {
            return Ok(Self::default());
        }
        self.op(ClipType::Intersection, other, "intersection")
    }

    /// Everything in either, with overlaps merged.
    pub fn union(&self, other: &Self) -> Result<Self, RegionError> {
        if self.is_empty() {
            return Ok(other.clone());
        }
        if other.is_empty() {
            return Ok(self.clone());
        }
        self.op(ClipType::Union, other, "union")
    }

    /// The parts of open polylines that lie inside this region.
    ///
    /// A closed ring is passed with its first point repeated at the end. Where a
    /// polyline leaves and re-enters, it comes back as several pieces; the order of
    /// the pieces is the library's, not the input's.
    pub fn clip_polylines(&self, lines: &[Vec<DVec2>]) -> Result<Vec<Vec<DVec2>>, RegionError> {
        if self.is_empty() || lines.is_empty() {
            return Ok(Vec::new());
        }
        self.clip_open(ClipType::Intersection, lines)
    }

    /// The stretches of `lines` inside this region, each a line and whether it
    /// closes on itself.
    ///
    /// One clip for them all, since a region can be large. A closed line wholly
    /// inside comes back whole and closed, as it was given. Otherwise each is cut
    /// into the open stretches inside, running the way it runs; the stretch through
    /// a closed line's first point comes back from the clipper as two pieces, and
    /// is joined again.
    pub fn clip_lines(
        &self,
        lines: &[(Vec<DVec2>, bool)],
    ) -> Result<Vec<(Vec<DVec2>, bool)>, RegionError> {
        let paths: Vec<Vec<DVec2>> = lines
            .iter()
            .map(|(points, closed)| {
                let mut path = points.clone();
                if *closed && let Some(&first) = points.first() {
                    path.push(first);
                }
                path
            })
            .collect();
        let mut pieces: Vec<Option<Vec<DVec2>>> = self
            .clip_polylines(&paths)?
            .into_iter()
            .filter(|p| p.len() >= 2)
            .map(Some)
            .collect();

        // A closed line's first point, to the clipper's tick, which is where the
        // pieces through it come back to exactly.
        let key = |p: DVec2| (to_tick(p.x), to_tick(p.y));
        let mut starts: std::collections::HashMap<(i64, i64), usize> =
            std::collections::HashMap::new();
        for (i, (points, closed)) in lines.iter().enumerate() {
            if *closed && let Some(&first) = points.first() {
                starts.entry(key(first)).or_insert(i);
            }
        }
        let mut starting = std::collections::HashMap::new();
        let mut ending = std::collections::HashMap::new();
        for (k, piece) in pieces.iter().enumerate() {
            let Some(piece) = piece else { continue };
            if let Some(&i) = piece.first().and_then(|&p| starts.get(&key(p))) {
                starting.entry(i).or_insert(k);
            }
            if let Some(&i) = piece.last().and_then(|&p| starts.get(&key(p))) {
                ending.entry(i).or_insert(k);
            }
        }

        let mut out = Vec::new();
        for (i, (points, _)) in lines.iter().enumerate() {
            match (starting.get(&i).copied(), ending.get(&i).copied()) {
                // Wholly inside: the clipper hands back the line itself.
                (Some(a), Some(b)) if a == b => {
                    let whole = pieces.get(a).and_then(Option::as_ref).is_some_and(|p| {
                        (polyline_length(p)
                            - polyline_length(paths.get(i).map_or(&[][..], Vec::as_slice)))
                        .abs()
                            < 1e-6
                    });
                    if whole {
                        pieces.get_mut(a).map(Option::take);
                        out.push((points.clone(), true));
                    }
                }
                (Some(a), Some(b)) => {
                    let tail = pieces.get_mut(a).and_then(Option::take).unwrap_or_default();
                    if let Some(Some(head)) = pieces.get_mut(b) {
                        head.extend(tail.into_iter().skip(1));
                    }
                }
                _ => {}
            }
        }
        out.extend(pieces.into_iter().flatten().map(|p| (p, false)));
        Ok(out)
    }

    fn clip_open(
        &self,
        kind: ClipType,
        lines: &[Vec<DVec2>],
    ) -> Result<Vec<Vec<DVec2>>, RegionError> {
        let open: Paths64 = lines.iter().map(|l| to_path(l)).collect();
        let mut c = Clipper64::new();
        c.add_open_subject(&open);
        c.add_clip(&self.to_paths());
        let mut closed = Paths64::new();
        let mut pieces = Paths64::new();
        if !c.execute(kind, FillRule::NonZero, &mut closed, Some(&mut pieces)) {
            return Err(RegionError::Clip {
                op: "polyline clip",
            });
        }
        Ok(pieces.iter().map(from_path).collect())
    }

    /// The parts of open polylines that lie outside this region.
    pub fn polylines_outside(&self, lines: &[Vec<DVec2>]) -> Result<Vec<Vec<DVec2>>, RegionError> {
        if lines.is_empty() {
            return Ok(Vec::new());
        }
        if self.is_empty() {
            return Ok(lines.to_vec());
        }
        self.clip_open(ClipType::Difference, lines)
    }

    /// An axis-aligned rectangle.
    pub fn rectangle(min: DVec2, max: DVec2) -> Self {
        Contour::new(vec![min, dvec2(max.x, min.y), max, dvec2(min.x, max.y)])
            .map_or_else(Self::default, |c| Self { rings: vec![c] })
    }

    fn op(&self, kind: ClipType, other: &Self, name: &'static str) -> Result<Self, RegionError> {
        boolean(kind, &self.to_paths(), &other.to_paths(), FillRule::NonZero)
            .map(|p| Self::from_paths(&p))
            .ok_or(RegionError::Clip { op: name })
    }

    fn to_paths(&self) -> Paths64 {
        self.rings.iter().map(|c| to_path(&c.points)).collect()
    }

    fn from_paths(paths: &Paths64) -> Self {
        let rings = paths
            .iter()
            .filter_map(|p| Contour::new(from_path(p)))
            .collect();
        Self { rings }
    }
}

fn boolean(kind: ClipType, subject: &Paths64, clip: &Paths64, fill: FillRule) -> Option<Paths64> {
    let mut c = Clipper64::new();
    c.add_subject(subject);
    if !clip.is_empty() {
        c.add_clip(clip);
    }
    let mut out = Paths64::new();
    c.execute(kind, fill, &mut out, None).then_some(out)
}

fn to_tick(v: f64) -> i64 {
    // Clamped into range, so the narrowing is exact.
    #[allow(clippy::cast_possible_truncation)]
    let t = (v * TICKS_PER_MM).round().clamp(-4.0e18, 4.0e18) as i64;
    t
}

fn to_path(points: &[DVec2]) -> Path64 {
    points
        .iter()
        .map(|p| Point64::new(to_tick(p.x), to_tick(p.y)))
        .collect()
}

fn from_path(path: &Path64) -> Vec<DVec2> {
    #[allow(clippy::cast_precision_loss)]
    path.iter()
        .map(|p| dvec2(p.x as f64 / TICKS_PER_MM, p.y as f64 / TICKS_PER_MM))
        .collect()
}

/// Trace the region where a height field is at or above `level`.
///
/// Marching squares over the grid. Cells whose value is unset count as *below*: an
/// unknown height is not somewhere to claim material exists.
///
/// For **stock only**. The rings this produces are grid-resolution staircases:
/// right for "is there material here?", wrong for anything a path follows. See the
/// module notes.
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

            // The classic sixteen cases, each with what qualifies on its right.
            // Ambiguous saddles (5, 10) are resolved by joining the corners, which
            // keeps a diagonal neck connected rather than pinching it into two
            // regions the cutter cannot reach between: each cuts off the two
            // corners that do not qualify, as 7 and 13, or 11 and 14, do.
            match code {
                1 => segs.push((left, bottom)),
                2 => segs.push((bottom, right)),
                3 => segs.push((left, right)),
                4 => segs.push((right, top)),
                5 => {
                    segs.push((left, top));
                    segs.push((right, bottom));
                }
                6 => segs.push((bottom, top)),
                7 => segs.push((left, top)),
                8 => segs.push((top, left)),
                9 => segs.push((top, bottom)),
                10 => {
                    segs.push((top, right));
                    segs.push((bottom, left));
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
mod tests {

    /// A span's inside is `contains`' inside: scanning a square with a square hole
    /// in it, at many angles, every point strictly inside a span is inside the
    /// region, and every point strictly between spans is not.
    #[test]
    fn spans_agree_with_contains() {
        let outer = Contour::new(vec![
            dvec2(0.0, 0.0),
            dvec2(10.0, 0.0),
            dvec2(10.0, 10.0),
            dvec2(0.0, 10.0),
        ])
        .unwrap();
        let hole = Contour::new(vec![
            dvec2(3.0, 3.0),
            dvec2(3.0, 7.0),
            dvec2(7.0, 7.0),
            dvec2(7.0, 3.0),
        ])
        .unwrap();
        let region = Region::new(vec![outer, hole]);
        for deg in [0.0_f64, 17.0, 45.0, 90.0, 133.0] {
            let along = dvec2(deg.to_radians().cos(), deg.to_radians().sin());
            let across = along.perp();
            for k in -20..=160 {
                let v = f64::from(k) * 0.1;
                let spans = region.spans(along, v);
                for (u0, u1) in &spans {
                    assert!(u1 > u0);
                    let mid = along * ((u0 + u1) / 2.0) + across * v;
                    assert!(region.contains(mid), "{deg} {v}: {mid} should be inside");
                }
                for w in spans.windows(2) {
                    if let [(_, a), (b, _)] = w {
                        let mid = along * ((a + b) / 2.0) + across * v;
                        assert!(!region.contains(mid), "{deg} {v}: {mid} should be outside");
                    }
                }
            }
        }
        // Through the hole along X: two spans; below the square: none.
        let spans = region.spans(dvec2(1.0, 0.0), 5.0);
        assert_eq!(spans, vec![(0.0, 3.0), (7.0, 10.0)]);
        assert!(region.spans(dvec2(1.0, 0.0), -1.0).is_empty());
        assert_eq!(region.bounds(), Some((dvec2(0.0, 0.0), dvec2(10.0, 10.0))));
        assert_eq!(Region::default().bounds(), None);
    }
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
        let bigger = r.offset(1.0, 0.001);
        let smaller = r.offset(-1.0, 0.001);
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
        let gone = square(2.0).offset(-5.0, 0.001);
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
        assert!(e.offset(1.0, 0.001).is_empty());
    }

    /// Offsets err to the side a cutter needs: a grown region contains the exact
    /// offset, a shrunk one lies within it. A square's round-cornered offset has a
    /// known area, and the error each way is bounded by the arc tolerance.
    #[test]
    fn offsets_err_to_the_safe_side() {
        let r = square(5.0);
        let tol = 0.01;
        let exact_grown = 100.0 + 4.0 * 10.0 * 2.0 + std::f64::consts::PI * 4.0;
        let grown = r.offset(2.0, tol).area();
        assert!(grown >= exact_grown, "{grown} < {exact_grown}");
        assert!(grown <= exact_grown + 2.0 * tol * 60.0, "{grown}");

        // A frame shrunk inward: the outer square loses 2 each side and keeps its
        // corners; the hole grows round-cornered, exactly as `square` did above.
        // The room left must not exceed the exact room.
        let frame = Region::rectangle(dvec2(-10.0, -10.0), dvec2(10.0, 10.0))
            .difference(&r)
            .unwrap();
        let room = frame.offset(-2.0, tol).area();
        let exact = 16.0 * 16.0 - exact_grown;
        assert!(room <= exact + 1e-6, "{room} > {exact}");
        assert!(room >= exact - 2.0 * tol * 60.0, "{room}");
    }

    /// Growing for coverage contains the round offset, however the corners turn.
    #[test]
    fn growing_for_coverage_contains_the_round_offset() {
        // An L: convex corners and one reflex one.
        let l = Region::rectangle(dvec2(0.0, 0.0), dvec2(10.0, 2.0))
            .union(&Region::rectangle(dvec2(0.0, 0.0), dvec2(2.0, 10.0)))
            .unwrap();
        // Round, and inside the exact offset: offsets are padded by their arc
        // tolerance, so asking for that much less stays within the exact one.
        let round = l.offset(3.0 - 0.002, 0.001);
        let covering = l.grow_covering(3.0);
        assert!(
            round.difference(&covering).unwrap().area() < 1e-6,
            "the covering growth misses part of the round one"
        );
    }

    /// Adding polygons to a region keeps its holes, unless a polygon covers them,
    /// and polygons count once whichever way they wind.
    #[test]
    fn adding_polygons_keeps_holes() {
        let frame = Region::rectangle(dvec2(0.0, 0.0), dvec2(10.0, 10.0))
            .difference(&square_at(5.0, 5.0, 2.0))
            .unwrap();
        assert!((frame.area() - 84.0).abs() < 1e-6);

        let far = Contour::new(vec![dvec2(20.0, 0.0), dvec2(20.0, 1.0), dvec2(21.0, 0.0)]).unwrap();
        let kept = frame.with_polygons(&[far]).unwrap();
        assert!((kept.area() - 84.5).abs() < 1e-6, "{}", kept.area());

        // Clockwise, over the hole: fills it.
        let plug = Contour::new(vec![
            dvec2(2.0, 2.0),
            dvec2(2.0, 8.0),
            dvec2(8.0, 8.0),
            dvec2(8.0, 2.0),
        ])
        .unwrap();
        let filled = frame.with_polygons(&[plug]).unwrap();
        assert!((filled.area() - 100.0).abs() < 1e-6, "{}", filled.area());
    }

    fn square_at(cx: f64, cy: f64, half: f64) -> Region {
        Region::rectangle(dvec2(cx - half, cy - half), dvec2(cx + half, cy + half))
    }

    #[test]
    fn containment_respects_holes() {
        let frame = Region::rectangle(dvec2(0.0, 0.0), dvec2(10.0, 10.0))
            .difference(&square_at(5.0, 5.0, 2.0))
            .unwrap();
        assert!(frame.contains(dvec2(1.0, 1.0)));
        assert!(!frame.contains(dvec2(5.0, 5.0)), "inside the hole");
        assert!(!frame.contains(dvec2(11.0, 5.0)));
    }

    /// A line crossing a region comes back as the stretches inside it.
    #[test]
    fn polylines_are_clipped_to_the_stretches_inside() {
        let r = square_at(0.0, 0.0, 5.0);
        let line = vec![dvec2(-10.0, 0.0), dvec2(10.0, 0.0)];
        let pieces = r.clip_polylines(&[line]).unwrap();
        assert_eq!(pieces.len(), 1);
        let len: f64 = pieces[0].windows(2).map(|w| w[0].distance(w[1])).sum();
        assert!((len - 10.0).abs() < 1e-6, "{len}");

        // In and out twice: two pieces.
        let frame = r.difference(&square_at(0.0, 0.0, 2.0)).unwrap();
        let line = vec![dvec2(-10.0, 0.0), dvec2(10.0, 0.0)];
        assert_eq!(frame.clip_polylines(&[line]).unwrap().len(), 2);
        assert!(
            Region::default()
                .clip_polylines(&[vec![dvec2(0.0, 0.0), dvec2(1.0, 0.0)]])
                .unwrap()
                .is_empty()
        );
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

    /// A rib one cell wide running diagonally either way is one connected
    /// region of the same area: the saddles where its cells meet corner to
    /// corner are joined, wound as every other case is, so the rings close on
    /// themselves rather than across a chord.
    #[test]
    fn a_diagonal_rib_contours_the_same_either_way() {
        let rib = |up: bool| {
            let mut f = HeightMap::filled(dvec2(0.0, 0.0), 1.0, 12, 12, 0.0).unwrap();
            for k in 0..7 {
                let iy = if up { 2 + k } else { 9 - k };
                f.set(2 + k, iy, 10.0);
            }
            contour_above(&f, 5.0)
        };
        let (up, down) = (rib(true), rib(false));
        // Seven cells, each a diamond of half a cell, and six saddles joining
        // them, each half a cell less its two cut corners' eighths: 7 × 0.5 +
        // 6 × 0.5.
        for (r, which) in [(&up, "up"), (&down, "down")] {
            assert_eq!(r.rings().len(), 1, "{which}: one ring");
            assert!(
                (r.area() - 6.5).abs() < 0.01,
                "{which}: expected 6.5, got {}",
                r.area()
            );
        }
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

    /// A closed line inside comes back whole; one crossing the edge comes back as
    /// the stretches inside, the one through its start joined again, each running
    /// the way the line did.
    #[test]
    fn clip_lines_keeps_whole_rings_and_rejoins_the_seam() {
        let region = Region::rectangle(dvec2(0.0, 0.0), dvec2(10.0, 10.0));
        let inside = vec![
            dvec2(2.0, 2.0),
            dvec2(4.0, 2.0),
            dvec2(4.0, 4.0),
            dvec2(2.0, 4.0),
        ];
        // Starts inside, runs out past x = 10 and back.
        let across = vec![
            dvec2(8.0, 5.0),
            dvec2(8.0, 3.0),
            dvec2(12.0, 3.0),
            dvec2(12.0, 7.0),
            dvec2(8.0, 7.0),
        ];
        let open = vec![dvec2(-2.0, 1.0), dvec2(12.0, 1.0)];
        let out = region
            .clip_lines(&[(inside.clone(), true), (across, true), (open, false)])
            .unwrap();
        assert_eq!(out.len(), 3, "{out:?}");
        assert!(out.iter().any(|(p, closed)| *closed && *p == inside));
        let seam = out
            .iter()
            .find(|(p, _)| p.iter().any(|q| (q.y - 5.0).abs() < 1e-9))
            .unwrap();
        assert!(!seam.1);
        // From where it comes back in at y = 7, down through its start, out at y = 3.
        assert!((seam.0.first().unwrap().y - 7.0).abs() < 1e-6, "{seam:?}");
        assert!((seam.0.last().unwrap().y - 3.0).abs() < 1e-6, "{seam:?}");
        let cut = out
            .iter()
            .find(|(p, _)| p.iter().all(|q| (q.y - 1.0).abs() < 1e-9))
            .unwrap();
        assert!(cut.0.first().unwrap().x < cut.0.last().unwrap().x);
    }
}
