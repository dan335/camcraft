//! Where the simulated stock stands: a height field traced by marching squares.
//!
//! For stock only. See the [module notes](super).

use super::{Contour, Region};
use crate::heightmap::HeightMap;
use glam::{DVec2, dvec2};

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
    // Which nodes of row `iy` qualify, from one before the grid to one past it.
    let fill = |row: &mut Vec<bool>, iy: i32| {
        row.clear();
        row.extend((-1..=nxi).map(|ix| inside(ix, iy)));
    };

    let mut segs: Vec<(DVec2, DVec2)> = Vec::new();
    let (mut lower, mut upper) = (Vec::new(), Vec::new());
    fill(&mut lower, -1);
    for iy in -1..nyi {
        fill(&mut upper, iy + 1);
        for ((l, u), ix) in lower.windows(2).zip(upper.windows(2)).zip(-1..nxi) {
            let (&[l0, l1], &[u0, u1]) = (l, u) else {
                continue;
            };
            let code = u8::from(l0) | u8::from(l1) << 1 | u8::from(u1) << 2 | u8::from(u0) << 3;
            if code == 0 || code == 15 {
                continue;
            }

            let (p00, p10, p01, p11) = (
                pos(ix, iy),
                pos(ix + 1, iy),
                pos(ix, iy + 1),
                pos(ix + 1, iy + 1),
            );
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
        std::mem::swap(&mut lower, &mut upper);
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
