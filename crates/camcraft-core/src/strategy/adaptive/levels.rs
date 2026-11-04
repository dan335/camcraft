//! Which levels adaptive clearing cuts beyond those a stepdown apart: on the
//! part's peaks, in its shallow areas, and which of them are too thin to take.
//!
//! A slab is a level a stepdown down and the levels between it and the one
//! above, cut bottom up ([`crate::strategy::slabs`]). Nothing above a slab's
//! own levels is cut until the level above it is, so every level in a slab
//! cuts into material standing up to that level: how deep a level cuts is how
//! far below it it is, however close the levels between are.

use std::collections::HashMap;

use camcraft_geom::contour::Region;
use camcraft_geom::mesh::TriMesh;
use glam::{DVec2, DVec3};
use rayon::prelude::*;

use super::material;
use crate::strategy::reach::LevelReach;
use crate::strategy::{MATERIAL_THICKNESS, SAME_LEVEL, StrategyError};

/// The most levels shallow areas may add to a run, so a tiny minimum against a
/// tall part cannot run away.
const MOST_SHALLOW: usize = 10_000;

/// The heights of the part's peaks: each point no point of it within the
/// tool's `radius` across stands above, on a face that faces up, where Fusion's
/// flat area detection adds a level too. A level there takes the stock off
/// over the peak down to it, which the level below cannot reach.
///
/// Points are the mesh's corners, found by where they are, as a face's own
/// corners are numbered apart from its neighbours'. The tops of flat faces
/// come out too, at the heights [`super::flat_heights`] gives.
pub fn peak_heights(mesh: &TriMesh, tolerance: f64, radius: f64) -> Vec<f64> {
    let q = (tolerance * 0.01).max(1e-6);
    let key = |p: DVec3| {
        // Within any part's size in a 64-bit count of hundredths of the tolerance.
        #[allow(clippy::cast_possible_truncation)]
        let k = |v: f64| (v / q).round() as i64;
        (k(p.x), k(p.y), k(p.z))
    };
    // Each corner: where it is, the highest corner of the faces it is on, and
    // whether any of them faces up.
    let mut corners: HashMap<(i64, i64, i64), (DVec3, f64, bool)> = HashMap::new();
    for t in 0..mesh.len() {
        let Some(tri) = mesh.triangle(t) else {
            continue;
        };
        let [a, b, c] = tri;
        let up = (b - a).cross(c - a).z > 0.0;
        let top = a.z.max(b.z).max(c.z);
        for v in tri {
            let e = corners
                .entry(key(v))
                .or_insert((v, f64::NEG_INFINITY, false));
            e.1 = e.1.max(top);
            e.2 |= up;
        }
    }
    let rise = tolerance * 0.1;
    let peaks: Vec<DVec3> = corners
        .values()
        .filter(|(v, top, up)| *up && *top <= v.z + rise)
        .map(|(v, ..)| *v)
        .collect();
    if peaks.is_empty() {
        return Vec::new();
    }
    // Only a peak the tool could stand over: none higher within its width.
    let cell = radius.max(tolerance);
    let cell_of = |p: DVec3| {
        #[allow(clippy::cast_possible_truncation)]
        let k = |v: f64| (v / cell).floor() as i64;
        (k(p.x), k(p.y))
    };
    let mut grid: HashMap<(i64, i64), Vec<DVec3>> = HashMap::new();
    for &(v, ..) in corners.values() {
        grid.entry(cell_of(v)).or_default().push(v);
    }
    let mut zs: Vec<f64> = peaks
        .par_iter()
        .filter(|p| {
            let (cx, cy) = cell_of(**p);
            let near = (cx - 1..=cx + 1).flat_map(|x| (cy - 1..=cy + 1).map(move |y| (x, y)));
            !near
                .filter_map(|c| grid.get(&c))
                .flatten()
                .any(|v| v.z > p.z + rise && v.truncate().distance(p.truncate()) < radius)
        })
        .map(|p| p.z)
        .collect();
    zs.sort_by(f64::total_cmp);
    let mut out: Vec<f64> = Vec::new();
    for z in zs {
        match out.last_mut() {
            // Peaks at a rounding error's difference in height are one level,
            // the highest's, so none is cut into.
            Some(last) if z - *last <= rise => *last = z,
            _ => out.push(z),
        }
    }
    out
}

/// Leave out of each of `slabs` the levels between its own and the one above
/// that `thin` says may be left out, and that are less than `min` below that
/// level, or `top` for the first: they would cut no deeper than that.
pub(super) fn without_thin(
    slabs: &mut [(f64, Vec<f64>)],
    top: f64,
    min: f64,
    thin: impl Fn(f64) -> bool,
) {
    if min <= 0.0 {
        return;
    }
    let mut above = top;
    for (z, fills) in slabs.iter_mut() {
        fills.retain(|&f| !thin(f) || above - f >= min - SAME_LEVEL);
        above = *z;
    }
}

/// Fusion's Machine Shallow Areas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shallow {
    /// The closest two levels may be, in millimetres: no level is added nearer
    /// the levels either side than this.
    pub min_stepdown: f64,
    /// The widest a terrace between two levels may be, as the tool's centre
    /// goes, in millimetres: where one is wider, a level is added halfway.
    pub max_stepover: f64,
}

/// A level added in a shallow area.
pub(super) struct Added {
    /// Its height.
    pub(super) z: f64,
    /// Where the terrace it was added for is wider than the most, as the
    /// tool's centre goes: where it cuts.
    pub(super) zone: Region,
    /// Where the tool may not be at it.
    pub(super) forbidden: Region,
}

/// Two levels one above the other, and where the tool may not be at each: the
/// upper's a hair below it, so a flat face at it is the part's, not a terrace.
struct Gap {
    below: f64,
    above: f64,
    lower: usize,
    upper: usize,
}

/// The levels to add in shallow areas between every two of `slabs`, the
/// first's up to `top`: halfway between two levels wherever the terrace
/// between them is wider than `shallow`'s most, as the tool's centre goes, and
/// then halfway between each of those and the levels either side, and so on,
/// until none is so wide or the levels would come closer than its minimum.
/// Each with where it cuts: where its terrace is that wide. `known` gives where
/// the tool may not be at each of the levels; `stocks` are the blocks of
/// material, and only terraces of them are counted.
pub(super) fn shallow_levels(
    slabs: &[(f64, Vec<f64>)],
    top: f64,
    shallow: Shallow,
    reach: &mut LevelReach<'_>,
    known: &[(f64, Region)],
    stocks: &[(DVec2, DVec2, f64)],
    arc: f64,
) -> Result<Vec<Added>, StrategyError> {
    let half = shallow.max_stepover * 0.5;
    if !(half > 0.0 && shallow.min_stepdown > 0.0) {
        return Ok(Vec::new());
    }
    // Every gap between two levels in a slab, and between its highest and the
    // level above.
    let mut gaps: Vec<(f64, f64)> = Vec::new();
    let mut above = top;
    for (z, fills) in slabs {
        let mut levels: Vec<f64> = std::iter::once(*z).chain(fills.iter().copied()).collect();
        levels.sort_by(f64::total_cmp);
        levels.push(above);
        gaps.extend(levels.windows(2).filter_map(|w| match w {
            [a, b] => Some((*a, *b)),
            _ => None,
        }));
        above = *z;
    }
    // Where the tool may not be: at each level, and a hair below each upper
    // one, from the top down as the shadows are cheapest asked for.
    let mut regions: Vec<Region> = known.iter().map(|(_, r)| r.clone()).collect();
    let find = |z: f64| known.iter().position(|(at, _)| (at - z).abs() < SAME_LEVEL);
    let mut uppers: Vec<f64> = gaps.iter().map(|&(_, b)| b - MATERIAL_THICKNESS).collect();
    uppers.sort_by(|a, b| b.total_cmp(a));
    uppers.dedup_by(|a, b| (*a - *b).abs() < SAME_LEVEL);
    let first = regions.len();
    regions.extend(reach.forbidden_all(&uppers)?);
    let upper_of = |b: f64| {
        uppers
            .iter()
            .position(|u| (u - (b - MATERIAL_THICKNESS)).abs() < SAME_LEVEL)
            .map(|k| first + k)
    };
    let mut pending: Vec<Gap> = gaps
        .iter()
        .filter_map(|&(a, b)| {
            Some(Gap {
                below: a,
                above: b,
                lower: find(a)?,
                upper: upper_of(b)?,
            })
        })
        .collect();
    let mut added: Vec<(f64, Region, usize)> = Vec::new();
    while !pending.is_empty() && added.len() < MOST_SHALLOW {
        // Each gap's terrace, and where it is too wide, in parallel.
        let split: Vec<Option<(f64, Region)>> = pending
            .par_iter()
            .map(|g| {
                if g.above - g.below < 2.0 * shallow.min_stepdown - SAME_LEVEL {
                    return Ok(None);
                }
                let (Some(lower), Some(upper)) = (regions.get(g.lower), regions.get(g.upper))
                else {
                    return Ok(None);
                };
                let terrace = lower
                    .difference(upper)?
                    .intersection(&material(stocks, g.below)?)?;
                let core = terrace.offset(-half, arc);
                if core.is_empty() {
                    return Ok(None);
                }
                let zone = core.offset(half, arc).intersection(&terrace)?;
                Ok(Some(((g.below + g.above) * 0.5, zone)))
            })
            .collect::<Result<_, StrategyError>>()?;
        let mut mids: Vec<f64> = split.iter().flatten().map(|(m, _)| *m).collect();
        if mids.is_empty() {
            break;
        }
        mids.sort_by(|a, b| b.total_cmp(a));
        let at = regions.len();
        regions.extend(reach.forbidden_all(&mids)?);
        let index_of = |m: f64| {
            mids.iter()
                .position(|x| x.to_bits() == m.to_bits())
                .map(|k| at + k)
        };
        let mut next = Vec::new();
        for (g, s) in pending.iter().zip(split) {
            let Some((m, zone)) = s else {
                continue;
            };
            let Some(k) = index_of(m) else {
                continue;
            };
            next.push(Gap {
                below: g.below,
                above: m,
                lower: g.lower,
                upper: k,
            });
            next.push(Gap {
                below: m,
                above: g.above,
                lower: k,
                upper: g.upper,
            });
            added.push((m, zone, k));
        }
        pending = next;
    }
    added.truncate(MOST_SHALLOW);
    Ok(added
        .into_iter()
        .filter_map(|(z, zone, k)| {
            Some(Added {
                z,
                zone,
                forbidden: regions.get(k)?.clone(),
            })
        })
        .collect())
}
