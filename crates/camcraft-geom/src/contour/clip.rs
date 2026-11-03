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
        if polygons.len() > BATCH {
            return self.union(&union_in_strips(polygons)?);
        }
        let mut paths = self.to_paths();
        paths.extend(polygons.iter().map(counter_clockwise));
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
    ///
    /// Every point of a stretch is where it was given, though the clipper hands
    /// each back rounded to its tick: only where a line crosses the region's
    /// edge is a point new. So a point put exactly somewhere, as onto the part,
    /// stays there.
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
                // Wholly inside: the clipper hands back the line itself, each
                // point at its tick, so as long as the line is there.
                (Some(a), Some(b)) if a == b => {
                    let given = paths
                        .get(i)
                        .map_or(0.0, |p| polyline_length(&from_path(&to_path(p))));
                    let whole = pieces
                        .get(a)
                        .and_then(Option::as_ref)
                        .is_some_and(|p| (polyline_length(p) - given).abs() < 1e-6);
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

        // Each point given, at its tick, which the clipper hands it back at.
        let given: std::collections::HashMap<(i64, i64), DVec2> = lines
            .iter()
            .flat_map(|(points, _)| points.iter().map(|&p| (key(p), p)))
            .collect();
        for (points, _) in &mut out {
            for p in points {
                if let Some(&exact) = given.get(&key(*p)) {
                    *p = exact;
                }
            }
        }
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

/// How many polygons a strip holds, about, when a union of many is worked out
/// strip by strip. A pass of the clipper over many more slows down far faster
/// than their number grows: it sweeps every edge that crosses each line across
/// them, and a mesh's triangles, all overlapping their neighbours, put a great
/// many across every line. A part's 442 000 triangles took 24 s in one pass.
const BATCH: usize = 2048;

/// A polygon in ticks, wound counter-clockwise, so it counts once in a
/// non-zero union however it was wound.
fn counter_clockwise(c: &Contour) -> Path64 {
    let mut p = to_path(&c.points);
    if c.signed_area() < 0.0 {
        p.reverse();
    }
    p
}

/// Everything covered by any of `polygons`, worked out in strips side by side.
///
/// The plane is cut by lines of constant X, on whole ticks, into strips of
/// about [`BATCH`] polygons each, and each polygon is clipped exactly to the
/// strips it crosses ([`clip_to_strip`]). Each strip is unioned on its own, all
/// at once, and the strips, which only meet along the lines, are joined.
///
/// Where an edge crosses a line is worked out from its two ends in the same
/// order whichever polygon it belongs to, so two polygons sharing an edge share
/// the point it is cut at, and the two strips either side of a line hold the
/// very same points along it: nothing is left between them. What is not on a
/// line, the clipper works out as it would in one pass. Batches of whole
/// polygons would not do: each batch's union puts where its edges cross on
/// the nearest tick its own way, and two batches meeting along an edge would
/// leave a hairline gap between them.
fn union_in_strips(polygons: &[Contour]) -> Result<Region, RegionError> {
    use rayon::prelude::*;
    let paths: Vec<(i64, i64, Path64)> = polygons
        .iter()
        .map(counter_clockwise)
        .filter_map(|p| {
            let lo = p.iter().map(|q| q.x).min()?;
            let hi = p.iter().map(|q| q.x).max()?;
            Some((lo, hi, p))
        })
        .collect();
    // Lines where the polygons' left ends divide them evenly.
    let mut lows: Vec<i64> = paths.iter().map(|(lo, ..)| *lo).collect();
    lows.sort_unstable();
    let strips = lows.len().div_ceil(BATCH).max(1);
    let mut lines: Vec<i64> = (1..strips)
        .filter_map(|k| lows.get(k * lows.len() / strips).copied())
        .collect();
    lines.dedup();
    // Each polygon to every strip it reaches into: strip `k` runs from line
    // `k - 1` to line `k`, the first and last on out.
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); lines.len() + 1];
    for (i, (lo, hi, _)) in paths.iter().enumerate() {
        let first = lines.partition_point(|&x| x <= *lo);
        let last = lines.partition_point(|&x| x < *hi);
        for strip in members.get_mut(first..=last).into_iter().flatten() {
            strip.push(i);
        }
    }
    let unioned: Vec<Paths64> = members
        .par_iter()
        .enumerate()
        .map(|(k, mine)| {
            let lo = k.checked_sub(1).and_then(|j| lines.get(j)).copied();
            let hi = lines.get(k).copied();
            let clipped: Paths64 = mine
                .iter()
                .filter_map(|&i| paths.get(i))
                .filter_map(|(.., p)| clip_to_strip(p, lo, hi))
                .collect();
            if clipped.is_empty() {
                return Some(Paths64::new());
            }
            boolean(
                ClipType::Union,
                &clipped,
                &Paths64::new(),
                FillRule::NonZero,
            )
        })
        .collect::<Option<_>>()
        .ok_or(RegionError::Clip { op: "union" })?;
    // The strips overlap nowhere, so joining them is one quick pass.
    let all: Paths64 = unioned.into_iter().flatten().collect();
    if all.is_empty() {
        return Ok(Region::default());
    }
    boolean(ClipType::Union, &all, &Paths64::new(), FillRule::NonZero)
        .map(|p| Region::from_paths(&p))
        .ok_or(RegionError::Clip { op: "union" })
}

/// A convex polygon, in ticks, cut to where `lo <= x <= hi`, either open when
/// `None`; `None` when nothing is left with any area. It keeps its winding.
///
/// Where an edge crosses a line is worked out from the edge's two ends taken in
/// a fixed order, in whole numbers, so every polygon with that edge has it cut
/// at the same tick.
fn clip_to_strip(path: &Path64, lo: Option<i64>, hi: Option<i64>) -> Option<Path64> {
    let mut out = path.clone();
    if let Some(lo) = lo {
        out = clip_half(&out, lo, |x| x >= lo);
    }
    if let Some(hi) = hi {
        out = clip_half(&out, hi, |x| x <= hi);
    }
    // Twice its area, exactly: none left is no polygon.
    let twice: i128 = out
        .iter()
        .zip(out.iter().cycle().skip(1))
        .map(|(a, b)| i128::from(a.x) * i128::from(b.y) - i128::from(b.x) * i128::from(a.y))
        .sum();
    (out.len() >= 3 && twice != 0).then_some(out)
}

/// What of a polygon is on the side of the line `x = at` that `keep` says, as
/// Sutherland and Hodgman clip it.
fn clip_half(path: &Path64, at: i64, keep: impl Fn(i64) -> bool) -> Path64 {
    let mut out = Path64::with_capacity(path.len() + 1);
    for (a, b) in path.iter().zip(path.iter().cycle().skip(1)) {
        let (a_in, b_in) = (keep(a.x), keep(b.x));
        if a_in {
            out.push(*a);
        }
        if a_in != b_in {
            out.push(crossing(*a, *b, at));
        }
    }
    out
}

/// Where the edge from `a` to `b` crosses `x = at`, which it does: from its
/// lower end, so the same edge either way round gives the same point, rounded
/// to the nearest tick.
fn crossing(a: Point64, b: Point64, at: i64) -> Point64 {
    let (p, q) = if (a.x, a.y) <= (b.x, b.y) {
        (a, b)
    } else {
        (b, a)
    };
    let (dx, dy) = (
        i128::from(q.x) - i128::from(p.x),
        i128::from(q.y) - i128::from(p.y),
    );
    let num = (i128::from(at) - i128::from(p.x)) * dy;
    // Rounded half away from zero, in whole numbers; `dx` is not zero, since
    // the ends are either side of the line.
    let half = dx.abs() / 2;
    let step = if (num >= 0) == (dx > 0) {
        (num.abs() + half) / dx.abs()
    } else {
        -((num.abs() + half) / dx.abs())
    };
    let y = i128::from(p.y) + step;
    #[allow(clippy::cast_possible_truncation)]
    let y = y.clamp(i128::from(MIN_COORD), i128::from(MAX_COORD)) as i64;
    Point64::new(at, y)
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

    /// A sphere's triangles from above, each sharing its edges with its
    /// neighbours as a part's do, in strips: one disc, as in one pass, with no
    /// hairline gap left where the strips meet, and nothing filled to hide one.
    #[test]
    fn a_mesh_in_strips_leaves_no_seams() {
        let mesh = crate::synthetic::uv_sphere(30.0, 64, 128);
        let tris: Vec<Contour> = (0..mesh.len())
            .filter_map(|i| mesh.triangle(i))
            .filter_map(|t| Contour::new(t.iter().map(|p| dvec2(p.x, p.y)).collect()))
            .filter(|c| c.signed_area().abs() > 1e-9)
            .collect();
        assert!(tris.len() > 4 * BATCH, "{}", tris.len());
        let strips = Region::default().with_polygons(&tris).unwrap();
        let paths: Paths64 = tris.iter().map(counter_clockwise).collect();
        let one = Region::from_paths(
            &boolean(ClipType::Union, &paths, &Paths64::new(), FillRule::NonZero).unwrap(),
        );
        assert_eq!(one.rings().len(), 1);
        assert_eq!(strips.rings().len(), 1, "seams left as rings");
        assert_within_rounding(&strips, &one);
    }

    /// Each pass rounds where edges cross to the tick, so neither `a` nor `b` is
    /// the exact union: each is within a rounding of it all along its edge, as
    /// [`TICKS_PER_MM`] allows, and so within two of the other, either way.
    fn assert_within_rounding(a: &Region, b: &Region) {
        let edge: f64 = b
            .rings()
            .iter()
            .flat_map(Contour::edges)
            .map(|(p, q)| p.distance(q))
            .sum();
        let rounding = 0.5 * std::f64::consts::SQRT_2 / TICKS_PER_MM;
        let apart = 2.0 * rounding * edge;
        for (more, than) in [(a, b), (b, a)] {
            let extra = more.difference(than).unwrap().area();
            assert!(extra <= apart, "{extra} past {apart}");
        }
    }

    /// A triangle cut at a line that crosses it is cut where its edge crosses,
    /// to the tick, and both sides of the line have that very point; a strip it
    /// only touches has nothing of it.
    #[test]
    fn a_polygon_is_cut_exactly_at_a_strip_line() {
        let tri: Path64 = vec![
            Point64::new(0, 0),
            Point64::new(30, 10),
            Point64::new(0, 20),
        ];
        let left = clip_to_strip(&tri, None, Some(10)).unwrap();
        let right = clip_to_strip(&tri, Some(10), None).unwrap();
        let on_line = |p: &Path64| {
            let mut ys: Vec<i64> = p.iter().filter(|q| q.x == 10).map(|q| q.y).collect();
            ys.sort_unstable();
            ys.dedup();
            ys
        };
        // From (0, 0) to (30, 10): 3.33 at x = 10, rounded to 3; and 17.
        assert_eq!(on_line(&left), [3, 17]);
        assert_eq!(on_line(&right), on_line(&left));
        // The same edge the other way round is cut at the same point.
        let reversed: Path64 = tri.iter().rev().copied().collect();
        assert_eq!(
            on_line(&clip_to_strip(&reversed, Some(10), None).unwrap()),
            [3, 17]
        );
        assert!(
            clip_to_strip(&tri, Some(30), None).is_none(),
            "touched at a point"
        );
        assert!(
            clip_to_strip(&tri, None, Some(0)).is_none(),
            "touched along an edge"
        );
    }

    /// Many overlapping triangles, round a ring so there is a hole in what they
    /// cover, either way wound: unioned in strips, they cover what one pass over
    /// them all covers, hole and all.
    #[test]
    fn a_union_in_strips_is_the_union_in_one() {
        let mut tris = Vec::new();
        let n = BATCH * 3 + 17;
        for i in 0..n {
            #[allow(clippy::cast_precision_loss)]
            let a = i as f64 / n as f64 * std::f64::consts::TAU;
            let at = |r: f64, da: f64| dvec2(r * (a + da).cos(), r * (a + da).sin());
            let mut pts = vec![at(40.0, 0.0), at(60.0, 0.002), at(50.0, 0.01)];
            if i % 2 == 0 {
                pts.reverse();
            }
            tris.push(Contour::new(pts).unwrap());
        }
        let strips = Region::default().with_polygons(&tris).unwrap();
        let paths: Paths64 = tris.iter().map(counter_clockwise).collect();
        let one = Region::from_paths(
            &boolean(ClipType::Union, &paths, &Paths64::new(), FillRule::NonZero).unwrap(),
        );
        assert_eq!(one.rings().len(), 2, "a ring and its hole");
        assert_eq!(strips.rings().len(), one.rings().len());
        assert_within_rounding(&strips, &one);
    }
}
