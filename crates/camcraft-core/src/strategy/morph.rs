//! A spiral morphed from the boundary to a centre, laid out seen from above for
//! [`crate::strategy::projected`] to drop onto the part: Fusion's Morphed Spiral.
//!
//! # Rings from the boundary to the centre
//!
//! Over each piece of the area, its holes filled, two distances are worked out
//! across a grid by fast marching ([`camcraft_geom::geodesic`]), both within the
//! piece and seen from above: from its boundary, and from its centre. Their
//! blend, the distance from the boundary over the sum of the two, is 0 on the
//! boundary and 1 at the centre, and its lines of equal value are rings that
//! turn from the boundary's shape into a point. Measured within the piece, they
//! go round a concave corner rather than across it.
//!
//! The rings are that blend at even steps, as many as keep neighbouring rings no
//! more than a stepover apart anywhere. The grid places the rings and nothing
//! else, as scallop's does (decision 0009): the first is the boundary itself, and
//! every point is cut on the part exactly.
//!
//! # One spiral through them
//!
//! A ray from the centre cuts each ring at a seam. Each lap runs from one ring's
//! seam round to the next's, the same fraction of the way along both, and the
//! same fraction of the way from the one to the other, so it moves in steadily
//! and ends where the next lap starts. A piece whose blend has a level of more
//! than one ring, as a very concave one can, is cut in rings instead, one after
//! the other.

use camcraft_geom::contour::{Contour, Region};
use camcraft_geom::geodesic::{Seed, march};
use camcraft_geom::isoline::IsoLine;
use camcraft_geom::trigrid::TriGrid;
use glam::DVec2;
use rayon::prelude::*;

use crate::strategy::field::{self, AreaGrid, level_lines};
use crate::strategy::projected::Curve;
use crate::strategy::{StrategyError, check};

/// The most rings a piece may have: far past any real part.
const MAX_RINGS: u32 = 100_000;

/// How to lay out the spiral.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MorphParams {
    /// Distance between neighbouring laps, in millimetres.
    pub stepover: f64,
    /// Where the rings shrink to, if inside the piece; otherwise, and without
    /// one, the point of the piece furthest from its boundary.
    pub centre: Option<DVec2>,
    /// Outward from the centre, rather than in from the boundary.
    pub outward: bool,
    /// Turning anticlockwise, seen from above.
    pub ccw: bool,
    /// The most grid nodes a piece's field may have; past it the grid is
    /// coarsened.
    pub max_nodes: u32,
}

impl Default for MorphParams {
    fn default() -> Self {
        Self {
            stepover: 1.0,
            centre: None,
            outward: false,
            ccw: true,
            max_nodes: 1 << 23,
        }
    }
}

/// What happened while laying out the spiral.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MorphReport {
    /// The stepover the laps were laid out to be within.
    pub stepover: f64,
    /// Pieces of the area.
    pub pieces: u64,
    /// Rings, in all the pieces.
    pub rings: u64,
    /// Pieces cut in rings, not one spiral, as a level of theirs was more than
    /// one ring.
    pub in_rings: u64,
    /// The furthest apart neighbouring rings are anywhere, in millimetres: more
    /// than the stepover only where a piece needed more rings than it may have.
    pub widest: f64,
    /// Grid nodes the fields were worked out on, in all.
    pub nodes: u64,
    /// Whether a grid was coarsened to stay within its budget.
    pub coarsened: bool,
}

impl MorphReport {
    /// Whether its laps are more than the stepover apart anywhere: where a piece
    /// needed more rings than it may have.
    pub fn uneven(&self) -> bool {
        self.widest > self.stepover * (1.0 + 1e-6)
    }
}

/// What a morphed spiral operation reported: laying the spiral out, and cutting
/// it on the part.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MorphedSpiralReport {
    /// Laying it out.
    pub pattern: MorphReport,
    /// Cutting it on the part.
    pub cut: crate::strategy::projected::ProjectReport,
}

/// The morphed spiral over each piece of `area`, in order, its laps no more than
/// a stepover apart.
pub fn morphed_spiral(
    area: &Region,
    params: &MorphParams,
) -> Result<(Vec<Curve>, MorphReport), StrategyError> {
    check("stepover", params.stepover)?;
    let mut report = MorphReport {
        stepover: params.stepover,
        ..Default::default()
    };
    let mut curves = Vec::new();
    for piece in area.pieces() {
        // The outer ring alone: the pattern runs over the holes, and is kept off
        // them when it is kept to the area.
        let Some(outer) = piece
            .rings()
            .iter()
            .max_by(|a, b| a.signed_area().abs().total_cmp(&b.signed_area().abs()))
        else {
            continue;
        };
        let filled = Region::new(vec![outer.clone()]);
        let Some(bounds) = filled.bounds() else {
            continue;
        };
        report.pieces += 1;
        curves.extend(morph_piece(&filled, outer, bounds, params, &mut report)?);
    }
    Ok((curves, report))
}

/// One piece's spiral, or its rings.
fn morph_piece(
    filled: &Region,
    outer: &Contour,
    bounds: (DVec2, DVec2),
    params: &MorphParams,
    report: &mut MorphReport,
) -> Result<Vec<Curve>, StrategyError> {
    let s = params.stepover;
    // A third of a stepover, with no tool to be fine enough for, coarsened if it
    // must be to half of one.
    let on = AreaGrid::over(
        filled,
        bounds,
        field::pitch(s, f64::INFINITY),
        s / 2.0,
        params.max_nodes,
        |_, _, _| Some(0.0),
    )?;
    report.nodes += u64::from(on.grid.nodes());
    report.coarsened |= on.coarsened;
    let grid = &on.grid;
    let flat = |n: u32| grid.xy(n).map(|p| p.extend(0.0));

    // From the boundary.
    let seeds: Vec<Seed> = on
        .boundary_feet(filled)
        .into_iter()
        .map(|(node, distance, _)| Seed { node, distance })
        .collect();
    let from_edge = march(grid, &seeds, flat).d;
    let inside = |n: usize| on.inside.get(n).copied().unwrap_or(false);

    // To the centre.
    let centre = params.centre.filter(|c| filled.contains(*c)).or_else(|| {
        let (far, _) = from_edge
            .iter()
            .enumerate()
            .filter(|&(n, d)| inside(n) && d.is_finite())
            .max_by(|a, b| a.1.total_cmp(b.1))?;
        grid.xy(u32::try_from(far).ok()?)
    });
    let Some(centre) = centre else {
        return Ok(Vec::new());
    };
    let near = 1.5 * grid.pitch();
    let seeds: Vec<Seed> = on
        .nodes_near(centre - DVec2::splat(near), centre + DVec2::splat(near))
        .filter_map(|n| {
            let p = grid.xy(n).filter(|_| grid.defined(n))?;
            let d = p.distance(centre);
            (d <= near).then_some(Seed {
                node: n,
                distance: d,
            })
        })
        .collect();
    let from_centre = march(grid, &seeds, flat).d;

    // The blend: 0 on the boundary, 1 at the centre, negative outside.
    let blend: Vec<f64> = from_edge
        .iter()
        .zip(&from_centre)
        .enumerate()
        .map(|(n, (&b, &c))| {
            let u = b / (b + c);
            if !u.is_finite() {
                f64::INFINITY
            } else if inside(n) {
                u
            } else {
                -u
            }
        })
        .collect();

    // As many rings as keep them a stepover apart: across each triangle inside,
    // a step of the blend is 1 / |gradient| long. At most enough for the longest
    // way from the boundary to the centre twenty times over.
    let longest = from_edge
        .iter()
        .zip(&from_centre)
        .enumerate()
        .filter(|&(n, _)| inside(n))
        .map(|(_, (b, c))| b + c)
        .filter(|v| v.is_finite())
        .fold(0.0, f64::max);
    let steepest_step = widest_step(grid, &blend, &on.inside);
    let cap = (20.0 * (longest / s).ceil()).min(f64::from(MAX_RINGS));
    let rings = (steepest_step / s).ceil().clamp(1.0, cap.max(1.0));
    // Clamped to a small whole number, so the narrowing is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let rings = rings as u32;
    report.rings += u64::from(rings);
    report.widest = report.widest.max(steepest_step / f64::from(rings));

    // Each level's lines, from 1 to one short of the centre.
    let mut levels = level_lines(
        grid,
        &blend,
        1.0 / f64::from(rings),
        rings.saturating_sub(1),
    );
    levels.resize_with(usize::try_from(rings).unwrap_or(0), Vec::new);
    let levels: Vec<Vec<IsoLine>> = levels.into_iter().skip(1).collect();

    // The boundary, then each level, from the outside in, each a ring turning
    // the way the spiral does before it is turned outward.
    let turning = params.ccw != params.outward;
    let whole = levels
        .iter()
        .all(|l| matches!(l.as_slice(), [one] if one.closed));
    if !whole {
        report.in_rings += 1;
        let mut out: Vec<Curve> = std::iter::once(outer.points().to_vec())
            .chain(
                levels
                    .into_iter()
                    .flatten()
                    .filter(|l| l.closed)
                    .map(|l| l.points),
            )
            .map(|points| Curve::closed(wound(points, params.ccw)))
            .collect();
        if params.outward {
            out.reverse();
        }
        return Ok(out);
    }
    let rings: Vec<Vec<DVec2>> = std::iter::once(outer.points().to_vec())
        .chain(levels.into_iter().flatten().map(|l| l.points))
        .map(|points| wound(points, turning))
        .collect();
    let mut points = spiral_through(&rings, centre, grid.pitch());
    if params.outward {
        points.reverse();
    }
    Ok(vec![Curve::open(points)])
}

/// The longest a step of `blend` is across any triangle of `grid` inside the
/// area: the reciprocal of its gradient there.
fn widest_step(grid: &TriGrid, blend: &[f64], inside: &[bool]) -> f64 {
    let step = |[a, b, c]: [u32; 3]| {
        let all_inside = [a, b, c]
            .iter()
            .all(|&n| usize::try_from(n).ok().and_then(|i| inside.get(i)) == Some(&true));
        if !all_inside {
            return None;
        }
        let at = |n: u32| {
            let v = *blend.get(usize::try_from(n).ok()?)?;
            Some((grid.xy(n)?, v))
        };
        let ((pa, ua), (pb, ub), (pc, uc)) = (at(a)?, at(b)?, at(c)?);
        if !(ua.is_finite() && ub.is_finite() && uc.is_finite()) {
            return None;
        }
        // The plane through the three: u = ua + g · (p − pa).
        let (e1, e2) = (pb - pa, pc - pa);
        let det = e1.perp_dot(e2);
        if det.abs() < 1e-18 {
            return None;
        }
        let (d1, d2) = (ub - ua, uc - ua);
        let g = DVec2::new(d1 * e2.y - d2 * e1.y, e1.x * d2 - e2.x * d1) / det;
        let length = g.length();
        (length > 1e-12).then(|| 1.0 / length)
    };
    // The largest of them, in parallel: in any order, the same.
    (0..grid.triangle_slots())
        .into_par_iter()
        .filter_map(|t| step(grid.triangle(t)?))
        .reduce(|| 0.0, f64::max)
}

/// `points`, a closed ring, wound anticlockwise if `ccw`, else clockwise.
fn wound(mut points: Vec<DVec2>, ccw: bool) -> Vec<DVec2> {
    let area = Contour::new(points.clone()).map_or(0.0, |c| c.signed_area());
    if (area > 0.0) != ccw {
        points.reverse();
    }
    points
}

/// One spiral through `rings`, the boundary first, each lap from one ring to the
/// next and the last into `centre`: its points no more than `spacing` apart.
fn spiral_through(rings: &[Vec<DVec2>], centre: DVec2, spacing: f64) -> Vec<DVec2> {
    let Some(first) = rings.first() else {
        return vec![centre];
    };
    // The seam: the ray from the centre through the boundary's nearest point.
    let toward = first
        .iter()
        .copied()
        .min_by(|a, b| a.distance(centre).total_cmp(&b.distance(centre)))
        .and_then(|p| (p - centre).try_normalize())
        .unwrap_or(DVec2::X);
    let mut seam_at = f64::INFINITY;
    let mut laps: Vec<Lap> = Vec::new();
    for ring in rings {
        let lap = Lap::from(ring, centre, toward, &mut seam_at);
        laps.push(lap);
    }
    let mut out = Vec::new();
    for (k, lap) in laps.iter().enumerate() {
        let next = laps.get(k + 1);
        let longest = lap.length.max(next.map_or(0.0, |n| n.length));
        let steps = camcraft_geom::steps::bounded_count((longest / spacing).ceil()).max(8);
        for i in 0..steps {
            let t = f64::from(i) / f64::from(steps);
            let here = lap.at(t);
            let there = next.map_or(centre, |n| n.at(t));
            out.push(here.lerp(there, t));
        }
    }
    out.push(centre);
    out
}

/// A ring cut at its seam, measured along from there.
struct Lap {
    points: Vec<DVec2>,
    /// How far along each point is.
    along: Vec<f64>,
    length: f64,
}

impl Lap {
    /// `ring`, cut where the ray from `centre` towards `toward` crosses it: of
    /// the places it does, the one nearest `seam_at`'s distance along the ray,
    /// which is then its own.
    fn from(ring: &[DVec2], centre: DVec2, toward: DVec2, seam_at: &mut f64) -> Self {
        let n = ring.len();
        let mut best: Option<(usize, f64, DVec2)> = None;
        for i in 0..n {
            let (Some(&a), Some(&b)) = (ring.get(i), ring.get((i + 1) % n)) else {
                continue;
            };
            let (da, db) = ((a - centre).perp_dot(toward), (b - centre).perp_dot(toward));
            if (da > 0.0) == (db > 0.0) {
                continue;
            }
            let p = a.lerp(b, da / (da - db));
            let t = (p - centre).dot(toward);
            if t < 0.0 {
                continue;
            }
            let better = best.is_none_or(|(_, bt, _)| {
                if seam_at.is_finite() {
                    (t - *seam_at).abs() < (bt - *seam_at).abs()
                } else {
                    t < bt
                }
            });
            if better {
                best = Some((i, t, p));
            }
        }
        let (start, seam) = match best {
            Some((i, t, p)) => {
                *seam_at = t;
                (i + 1, Some(p))
            }
            None => (0, None),
        };
        let mut points: Vec<DVec2> = seam.into_iter().collect();
        points.extend((0..n).filter_map(|k| ring.get((start + k) % n).copied()));
        if let Some(&first) = points.first() {
            points.push(first);
        }
        let mut along = Vec::with_capacity(points.len());
        let mut length = 0.0;
        for (i, p) in points.iter().enumerate() {
            if i > 0 {
                length += points.get(i - 1).map_or(0.0, |q| q.distance(*p));
            }
            along.push(length);
        }
        Self {
            points,
            along,
            length,
        }
    }

    /// The point `t` of the way round.
    fn at(&self, t: f64) -> DVec2 {
        let want = t * self.length;
        let i = self.along.partition_point(|&a| a < want).max(1);
        let (Some(&a), Some(&b), Some(&la), Some(&lb)) = (
            self.points.get(i - 1),
            self.points.get(i),
            self.along.get(i - 1),
            self.along.get(i),
        ) else {
            return self.points.last().copied().unwrap_or(DVec2::ZERO);
        };
        if lb - la < 1e-15 {
            b
        } else {
            a.lerp(b, (want - la) / (lb - la))
        }
    }
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
    use glam::dvec2;

    fn disc(r: f64) -> Region {
        let points = (0..720)
            .map(|i| {
                let a = f64::from(i) * 0.5f64.to_radians();
                dvec2(a.cos(), a.sin()) * r
            })
            .collect();
        Region::new(vec![Contour::new(points).unwrap()])
    }

    fn polygon(points: &[(f64, f64)]) -> Region {
        Region::new(vec![
            Contour::new(points.iter().map(|&(x, y)| dvec2(x, y)).collect()).unwrap(),
        ])
    }

    #[test]
    fn a_disc_morphs_into_an_even_spiral() {
        let (curves, report) = morphed_spiral(
            &disc(10.0),
            &MorphParams {
                stepover: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(report.in_rings, 0, "{report:?}");
        assert!(report.widest <= 1.0 + 1e-9, "{report:?}");
        let points = &curves[0].points;
        assert!((points[0].length() - 10.0).abs() < 0.01);
        assert!(points.last().unwrap().length() < 0.4);
        // Every lap a little further in, never out.
        let mut crossings: Vec<f64> = points
            .windows(2)
            .filter(|w| w[0].y < 0.0 && w[1].y >= 0.0 && w[1].x > 0.0)
            .map(|w| w[0].x + (w[1].x - w[0].x) * (-w[0].y / (w[1].y - w[0].y)))
            .collect();
        crossings.sort_by(f64::total_cmp);
        assert!(crossings.len() >= 9, "{crossings:?}");
        for w in crossings.windows(2) {
            let gap = w[1] - w[0];
            assert!(gap > 0.5 && gap <= 1.05, "{crossings:?}");
        }
    }

    #[test]
    fn it_stays_in_the_piece_and_turns_the_way_asked() {
        let square = polygon(&[(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)]);
        for ccw in [true, false] {
            let (curves, _) = morphed_spiral(
                &square,
                &MorphParams {
                    stepover: 1.5,
                    ccw,
                    ..Default::default()
                },
            )
            .unwrap();
            let points = &curves[0].points;
            for p in points {
                assert!(p.x > -1e-6 && p.x < 20.0 + 1e-6 && p.y > -1e-6 && p.y < 20.0 + 1e-6);
            }
            // Ends in the middle.
            assert!(points.last().unwrap().distance(dvec2(10.0, 10.0)) < 1.0);
            // Turning about it the way asked.
            let c = dvec2(10.0, 10.0);
            let turned: f64 = points
                .windows(2)
                .map(|w| (w[0] - c).perp_dot(w[1] - c))
                .sum();
            assert_eq!(turned > 0.0, ccw);
        }
    }

    #[test]
    fn outward_is_the_same_spiral_the_other_way() {
        let square = polygon(&[(0.0, 0.0), (12.0, 0.0), (12.0, 8.0), (0.0, 8.0)]);
        let params = MorphParams {
            stepover: 1.0,
            ..Default::default()
        };
        let inward = morphed_spiral(&square, &params).unwrap().0;
        let outward = morphed_spiral(
            &square,
            &MorphParams {
                outward: true,
                ccw: false,
                ..params
            },
        )
        .unwrap()
        .0;
        let mut back = outward[0].points.clone();
        back.reverse();
        assert_eq!(back, inward[0].points);
    }

    #[test]
    fn an_l_shape_is_covered_to_the_stepover() {
        let l = polygon(&[
            (0.0, 0.0),
            (30.0, 0.0),
            (30.0, 10.0),
            (10.0, 10.0),
            (10.0, 30.0),
            (0.0, 30.0),
        ]);
        let (curves, report) = morphed_spiral(
            &l,
            &MorphParams {
                stepover: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!curves.is_empty());
        assert!(report.widest <= 1.0 + 1e-9, "{report:?}");
        for c in &curves {
            for p in &c.points {
                assert!(l.offset(1e-3, 1e-4).contains(*p), "{p}");
            }
        }
    }

    #[test]
    fn is_deterministic() {
        let square = polygon(&[(0.0, 0.0), (15.0, 0.0), (15.0, 9.0), (0.0, 9.0)]);
        let params = MorphParams {
            stepover: 0.8,
            ..Default::default()
        };
        assert_eq!(
            morphed_spiral(&square, &params).unwrap(),
            morphed_spiral(&square, &params).unwrap()
        );
    }
}
