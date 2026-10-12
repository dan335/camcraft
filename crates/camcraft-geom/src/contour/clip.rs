//! The bridge to Clipper2: millimetres to ticks and back, and every operation that
//! goes through the clipper.

use super::{Contour, Region, RegionError, polyline_length};
use clipper2_rust::{
    ClipType, Clipper64, ClipperOffset, EndType, FillRule, JoinType, Path64, Paths64, Point64,
    constants::{MAX_COORD, MIN_COORD},
};
use glam::{DVec2, dvec2};

/// Integer ticks per millimetre. A tick is 0.1 µm: rounding to it moves a point by
/// at most 0.07 µm, far inside any machining tolerance, while a 10 m part still
/// sits many orders of magnitude inside Clipper's coordinate range.
const TICKS_PER_MM: f64 = 10_000.0;

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
    pub(crate) fn with_polygons(&self, polygons: &[Contour]) -> Result<Self, RegionError> {
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
        inflate(
            &self.to_paths(),
            JoinType::Round,
            EndType::Polygon,
            padded,
            tol,
        )
    }

    /// Grown by at least `delta` everywhere, with mitred or squared corners.
    ///
    /// Contains the round offset, with far fewer points: for a region that only
    /// decides coverage, never the shape of a path — such as where stock stands.
    pub fn grow_covering(&self, delta: f64) -> Self {
        if self.rings.is_empty() || !(delta.is_finite() && delta > 0.0) {
            return self.clone();
        }
        inflate(
            &self.to_paths(),
            JoinType::Miter,
            EndType::Polygon,
            delta,
            0.0,
        )
    }

    /// Everything within `radius` of any of the polylines, with round ends, erring
    /// outward by at most `arc_tolerance` as [`offset`](Self::offset) does.
    pub fn around_polylines(lines: &[Vec<DVec2>], radius: f64, arc_tolerance: f64) -> Self {
        if lines.is_empty() || !(radius.is_finite() && radius > 0.0) {
            return Self::default();
        }
        let tol = arc_tolerance.max(1.0 / TICKS_PER_MM);
        let paths: Paths64 = lines.iter().map(|l| to_path(l)).collect();
        inflate(&paths, JoinType::Round, EndType::Round, radius + tol, tol)
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
    pub(crate) fn polylines_outside(
        &self,
        lines: &[Vec<DVec2>],
    ) -> Result<Vec<Vec<DVec2>>, RegionError> {
        if lines.is_empty() {
            return Ok(Vec::new());
        }
        if self.is_empty() {
            return Ok(lines.to_vec());
        }
        self.clip_open(ClipType::Difference, lines)
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

/// `paths` grown by `delta` millimetres, joined as `join` says and ended as `end`
/// does, a round join's chords ending within `arc` millimetres of its arc.
fn inflate(paths: &Paths64, join: JoinType, end: EndType, delta: f64, arc: f64) -> Region {
    let mut out = Paths64::new();
    let mut off = ClipperOffset::new(2.0, arc * TICKS_PER_MM, false, false);
    off.add_paths(paths, join, end);
    off.execute(delta * TICKS_PER_MM, &mut out);
    Region::from_paths(&out)
}

/// `v` millimetres, to the nearest tick inside Clipper's range; a NaN is 0.
fn to_tick(v: f64) -> i64 {
    // Saturating, then clamped into the range Clipper works in.
    #[allow(clippy::cast_possible_truncation)]
    let t = ((v * TICKS_PER_MM).round() as i64).clamp(MIN_COORD, MAX_COORD);
    t
}

/// A ring or a polyline in ticks.
pub(super) fn to_path(points: &[DVec2]) -> Path64 {
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// A coordinate past what the clipper can hold is held at its edge, not past it.
    #[test]
    fn ticks_stay_in_the_clippers_range() {
        assert_eq!(to_tick(1.25), 12_500);
        assert_eq!(to_tick(1.0e300), MAX_COORD);
        assert_eq!(to_tick(-1.0e300), MIN_COORD);
        assert_eq!(to_tick(f64::INFINITY), MAX_COORD);
        assert_eq!(to_tick(f64::NAN), 0);
    }
}
