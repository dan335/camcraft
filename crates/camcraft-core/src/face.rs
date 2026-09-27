//! Facing: the stock's top cut flat, straight passes across it side by side,
//! level by level down to the bottom. Fusion's Face.
//!
//! # From the stock, not the model
//!
//! What is faced is the stock's footprint, grown by the stock offset, and nothing
//! of the model: every pass is a straight line across it, so nothing here needs
//! the part's geometry. Its bottom is the model's top unless another is asked
//! for; one asked for lower cuts into the part, and the verifier says so.
//!
//! # Coverage
//!
//! The outermost passes put the tool's centre on the faced area's edges, so the
//! tool reaches a radius past them, and every pass runs on until the tool is clear
//! of the area, then on by the pass extension: nothing of the area is left under
//! the tool's reach, and the tool enters and leaves off the stock. The passes are
//! spaced evenly, no more than the stepover apart.
//!
//! # Direction
//!
//! Both ways by default, as Fusion's: back and forth, stepping over off the stock
//! at each end without lifting. Climb or conventional cut every pass the same way,
//! and the linker brings the tool back between them. Each level starts from the
//! side the one above finished on.

use crate::job::FaceDirection;
use crate::planar::{PlanarReport, levels};
use crate::strategy::{StrategyError, check};
use crate::toolpath::{MoveRole, Toolpath};
use glam::{DVec2, DVec3, dvec2, dvec3};

/// How to face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceParams {
    /// The area faced, lower and upper corner: the stock's footprint and its
    /// offset.
    pub area: (DVec2, DVec2),
    /// Pass direction in radians; zero runs along +X.
    pub angle: f64,
    /// Distance between passes, in millimetres.
    pub stepover: f64,
    /// How far each pass runs on once the tool is clear of the area.
    pub extension: f64,
    /// The tool's radius.
    pub radius: f64,
    /// The levels cut, from the top down; the last is the bottom.
    pub levels: LevelsAsked,
    /// Which way the passes cut.
    pub direction: FaceDirection,
    /// Whether climbing has the material on the tool's right: a clockwise
    /// spindle.
    pub clockwise: bool,
    /// Cutting feed, in mm/min.
    pub feed: f64,
}

/// The levels facing cuts at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelsAsked {
    /// Highest level cut from.
    pub top: f64,
    /// The faced level.
    pub bottom: f64,
    /// Deepest a level may be below the one above: Fusion's multiple depths.
    /// `None` cuts the bottom alone.
    pub stepdown: Option<f64>,
}

/// The passes facing `params.area`, at each level.
pub fn face(params: &FaceParams) -> Result<(Toolpath, PlanarReport), StrategyError> {
    check("stepover", params.stepover)?;
    check("feed", params.feed)?;
    check("tool radius", params.radius)?;
    if let Some(d) = params.levels.stepdown {
        check("stepdown", d)?;
    }
    for (name, value) in [
        ("pass extension", params.extension),
        ("pass angle", params.angle),
        ("top", params.levels.top),
    ] {
        let fine = value.is_finite() && (name != "pass extension" || value >= 0.0);
        if !fine {
            return Err(StrategyError::BadParameter { name, value });
        }
    }

    let mut report = PlanarReport::default();
    let asked = params.levels;
    let zs = levels(asked.top, asked.bottom, asked.stepdown);
    let lines = passes(params);
    if zs.is_empty() {
        report.too_shallow = 1;
        return Ok((Toolpath::new(), report));
    }
    if lines.is_empty() {
        return Ok((Toolpath::new(), report));
    }
    report.areas = 1;
    report.rings = lines.len() as u64;
    report.levels = zs.len() as u64;

    let mut path = Toolpath::new();
    // Both ways turns at the end of every pass, from one level into the next
    // too, so each level starts where the one above finished.
    let mut turn = 0usize;
    for (k, &z) in zs.iter().enumerate() {
        let ascending = k % 2 == 0;
        let order: Vec<usize> = if ascending {
            (0..lines.len()).collect()
        } else {
            (0..lines.len()).rev().collect()
        };
        for (n, &i) in order.iter().enumerate() {
            let Some(&(a, b)) = lines.get(i) else {
                continue;
            };
            // Each pass is `a` to `b` along +u. Going +u, the tool's right is -v;
            // the side not yet cut is the one the next passes are toward, +v
            // while they ascend. Climbing with a clockwise spindle has it on the
            // right.
            let forward = match params.direction {
                FaceDirection::BothWays => turn.is_multiple_of(2),
                FaceDirection::Climb => params.clockwise != ascending,
                FaceDirection::Conventional => params.clockwise == ascending,
            };
            turn += 1;
            let (from, to) = if forward { (a, b) } else { (b, a) };
            // Both ways steps over at the end without lifting, off the stock;
            // otherwise, and onto each level, the linker brings the tool.
            let role = if params.direction == FaceDirection::BothWays && n > 0 {
                MoveRole::Cut
            } else {
                MoveRole::Link
            };
            path.feed_to(at(from, z), role, params.feed)?;
            path.feed_to(at(to, z), MoveRole::Cut, params.feed)?;
            report.passes += 1;
        }
    }
    Ok((path, report))
}

fn at(p: DVec2, z: f64) -> DVec3 {
    dvec3(p.x, p.y, z)
}

/// The pass direction and the one across it, as unit vectors.
fn frame(angle: f64) -> (DVec2, DVec2) {
    let u = dvec2(angle.cos(), angle.sin());
    (u, u.perp())
}

/// Each pass's two ends, in the setup's frame, ordered across from -v to +v and
/// each running +u: spaced evenly from one edge of the area to the other, and
/// running across the area grown by the tool's radius, then the extension.
fn passes(params: &FaceParams) -> Vec<(DVec2, DVec2)> {
    let (lo, hi) = params.area;
    if !(hi.x > lo.x && hi.y > lo.y) {
        return Vec::new();
    }
    let (u, v) = frame(params.angle);
    let corners = |lo: DVec2, hi: DVec2| {
        [
            dvec2(lo.x, lo.y),
            dvec2(hi.x, lo.y),
            dvec2(hi.x, hi.y),
            dvec2(lo.x, hi.y),
        ]
    };
    let across: Vec<f64> = corners(lo, hi).iter().map(|c| c.dot(v)).collect();
    let (v0, v1) = across
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &c| {
            (a.min(c), b.max(c))
        });
    let span = v1 - v0;
    // Bounded, so a tiny stepover cannot run away.
    let steps = (span / params.stepover - 1e-9).ceil().clamp(0.0, 100_000.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let steps = steps as u32;
    let r = params.radius;
    let grown = corners(lo - DVec2::splat(r), hi + DVec2::splat(r));
    (0..=steps)
        .filter_map(|i| {
            let c = if steps == 0 {
                v0 + span / 2.0
            } else {
                v0 + span * f64::from(i) / f64::from(steps)
            };
            let (a, b) = crossing(&grown, u, v, c)?;
            let e = params.extension;
            Some((u * (a - e) + v * c, u * (b + e) + v * c))
        })
        .collect()
}

/// Where the line `p·v = c` crosses the convex polygon `ring`, as the least and
/// greatest `p·u` on it.
fn crossing(ring: &[DVec2], u: DVec2, v: DVec2, c: f64) -> Option<(f64, f64)> {
    let mut hits = Vec::new();
    for (i, &p) in ring.iter().enumerate() {
        let q = *ring.get((i + 1) % ring.len())?;
        let (dp, dq) = (p.dot(v) - c, q.dot(v) - c);
        if (dp - dq).abs() < 1e-12 {
            // Along the line, or beside it: its ends are all it can add.
            if dp.abs() < 1e-12 {
                hits.extend([p.dot(u), q.dot(u)]);
            }
        } else if (dp <= 0.0) != (dq <= 0.0) || dp.abs() < 1e-12 || dq.abs() < 1e-12 {
            hits.push((p + (q - p) * (dp / (dp - dq))).dot(u));
        }
    }
    let lo = hits.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = hits.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    (lo <= hi).then_some((lo, hi))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn params() -> FaceParams {
        FaceParams {
            area: (dvec2(0.0, 0.0), dvec2(100.0, 50.0)),
            angle: 0.0,
            stepover: 9.5,
            extension: 2.0,
            radius: 5.0,
            levels: LevelsAsked {
                top: 3.0,
                bottom: 0.0,
                stepdown: None,
            },
            direction: FaceDirection::BothWays,
            clockwise: true,
            feed: 1000.0,
        }
    }

    fn cuts(path: &Toolpath) -> Vec<DVec3> {
        path.moves()
            .iter()
            .filter(|m| m.role == MoveRole::Cut)
            .map(|m| m.to)
            .collect()
    }

    /// Passes cover the area edge to edge, evenly no more than the stepover
    /// apart, and each runs off it by the tool's radius and the extension, at
    /// the bottom alone.
    #[test]
    fn passes_cover_the_area_and_run_off_it() {
        let (path, report) = face(&params()).unwrap();
        // 50 wide at 9.5 apart: 6 steps, 7 passes.
        assert_eq!(report.rings, 7);
        assert_eq!(report.levels, 1);
        assert_eq!(report.passes, 7);
        let moves = path.moves();
        let ys: Vec<f64> = moves.iter().map(|m| m.to.y).collect();
        assert_eq!(ys.first(), Some(&0.0));
        assert_eq!(ys.last(), Some(&50.0));
        for m in moves {
            assert_eq!(m.to.z, 0.0);
            assert!(m.to.x == -7.0 || m.to.x == 107.0, "{}", m.to);
        }
        // Both ways: one link in, and every step over is cut without lifting.
        assert_eq!(moves[0].role, MoveRole::Link);
        assert!(moves[1..].iter().all(|m| m.role == MoveRole::Cut));
        let gap = 50.0 / 6.0;
        let mut rows: Vec<f64> = cuts(&path).iter().map(|p| p.y).collect();
        rows.dedup();
        for w in rows.windows(2) {
            assert!(((w[1] - w[0]) - gap).abs() < 1e-9, "{w:?}");
        }
    }

    /// Multiple depths are evenly spaced down to the bottom, each level starting
    /// where the one above finished; with none, the bottom alone; with the
    /// bottom not below the top, nothing, and said.
    #[test]
    fn levels_step_down_to_the_bottom() {
        let mut p = params();
        p.levels.stepdown = Some(1.0);
        let (path, report) = face(&p).unwrap();
        assert_eq!(report.levels, 3);
        let moves = path.moves();
        let links: Vec<&DVec3> = moves
            .iter()
            .filter(|m| m.role == MoveRole::Link)
            .map(|m| &m.to)
            .collect();
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].z, 2.0);
        assert_eq!(links[2].z, 0.0);
        // The second level starts on the side the first ended on.
        assert_eq!(links[1].y, 50.0);

        p.levels.bottom = 3.0;
        let (path, report) = face(&p).unwrap();
        assert!(path.moves().is_empty());
        assert_eq!(report.too_shallow, 1);
    }

    /// Climb and conventional cut every pass the same way, each the opposite of
    /// the other, the linker bringing the tool back between; climb with a
    /// clockwise spindle keeps the uncut side on the right.
    #[test]
    fn one_way_passes_all_go_the_same_way() {
        for (direction, clockwise, way) in [
            (FaceDirection::Climb, true, -1.0),
            (FaceDirection::Conventional, true, 1.0),
            (FaceDirection::Climb, false, 1.0),
        ] {
            let p = FaceParams {
                direction,
                clockwise,
                ..params()
            };
            let (path, _) = face(&p).unwrap();
            let moves = path.moves();
            assert_eq!(moves.iter().filter(|m| m.role == MoveRole::Link).count(), 7);
            for pair in moves.chunks(2) {
                assert_eq!(pair[0].role, MoveRole::Link);
                let dx = pair[1].to.x - pair[0].to.x;
                assert_eq!(dx.signum(), way, "{direction:?} {clockwise}");
            }
        }
    }

    /// At an angle, every pass still runs off the area grown by the tool's
    /// radius at both ends, and the passes reach its far corners.
    #[test]
    fn angled_passes_run_off_the_area() {
        let p = FaceParams {
            angle: 30f64.to_radians(),
            ..params()
        };
        let (path, _) = face(&p).unwrap();
        let (lo, hi) = (dvec2(-5.0, -5.0), dvec2(105.0, 55.0));
        let inside = |q: DVec3| q.x > lo.x && q.x < hi.x && q.y > lo.y && q.y < hi.y;
        for m in path.moves() {
            // Each end of a pass is at least the extension off the grown area.
            assert!(!inside(m.to), "{} is on the stock", m.to);
        }
        let (_, v) = frame(p.angle);
        let across: Vec<f64> = cuts(&path).iter().map(|q| q.truncate().dot(v)).collect();
        let most = across.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let least = across.iter().copied().fold(f64::INFINITY, f64::min);
        let far = [
            dvec2(0.0, 0.0),
            dvec2(100.0, 0.0),
            dvec2(100.0, 50.0),
            dvec2(0.0, 50.0),
        ]
        .map(|c| c.dot(v));
        let (want_lo, want_hi) = (
            far.iter().copied().fold(f64::INFINITY, f64::min),
            far.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        );
        assert!((most - want_hi).abs() < 1e-9 && (least - want_lo).abs() < 1e-9);
    }

    /// A pass extension below zero, or no area, is refused or faces nothing.
    #[test]
    fn a_bad_extension_is_refused() {
        let p = FaceParams {
            extension: -1.0,
            ..params()
        };
        assert!(face(&p).is_err());
        let p = FaceParams {
            area: (dvec2(0.0, 0.0), dvec2(0.0, 10.0)),
            ..params()
        };
        assert!(face(&p).unwrap().0.moves().is_empty());
    }
}
