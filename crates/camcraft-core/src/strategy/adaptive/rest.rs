//! Rest machining: the material the operations before this one left, at each
//! level, from their moves swept exactly, each with the tool it was cut with.
//!
//! What a tool cuts at a level is, along every move whose tip is at or below
//! it, a disc as wide as the tool is that far above its tip. Its tool only
//! widens going up, so what is cut at a level is cut at every level above it
//! too: a move far enough below a level to cut it at the tool's full width
//! cuts every level above at that width, and is swept once, from the lowest
//! level up. Only the moves close below a level, where the tool is narrower
//! there, are swept for it alone, each piece at its narrowest, so what is
//! taken as cut never claims more than was.
//!
//! Rapids cut nothing: a rapid through material is the check's to refuse.

use camcraft_geom::steps::bounded_count;
use glam::DVec3;

use super::tiles::{TILE_RADII, Tiles};
use super::*;

/// An operation cut before this one: its path, and the real tool it was cut
/// with.
#[derive(Debug, Clone, PartialEq)]
pub struct Earlier {
    /// Its moves, as cut: shared with its operation's result.
    pub path: std::sync::Arc<Toolpath>,
    /// The tool it was cut with, without the stock it left: what took the
    /// material.
    pub cutter: Cutter,
}

/// A straight piece of an earlier path, between two places of the tool's tip.
#[derive(Debug, Clone, Copy)]
struct Piece {
    a: DVec3,
    b: DVec3,
}

/// An earlier operation's tool, and its moves in pieces.
struct Op<'a> {
    cutter: &'a Cutter,
    pieces: Vec<Piece>,
}

/// The most pieces a move close below a level is cut into, to take the tool's
/// width at each as it is there.
const MOST_SLICES: f64 = 32.0;

/// The material at each of `levels`, in any order: the blocks (`stocks`), less
/// what the operations `before` swept there, less what they left thinner than
/// `ignore`. Each with its level, from the lowest.
pub(super) fn material_at(
    stocks: &[(DVec2, DVec2, f64)],
    levels: &[f64],
    before: &[Earlier],
    ignore: f64,
    tolerance: f64,
    watch: &Watch,
) -> Result<Vec<(f64, Region)>, StrategyError> {
    let mut zs: Vec<f64> = levels.to_vec();
    zs.sort_by(f64::total_cmp);
    zs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let Some(&lowest) = zs.first() else {
        return Ok(Vec::new());
    };
    let arc = tolerance * 0.5;

    // Each earlier operation's pieces, and from which level each cuts at its
    // tool's full width, and from which at all.
    let ops: Vec<Op<'_>> = before
        .iter()
        .map(|e| Op {
            cutter: &e.cutter,
            pieces: pieces(&e.path, tolerance),
        })
        .collect();
    // full[k]: by operation, the pieces cutting level k at full width first.
    // close[k]: by operation, the pieces cutting level k narrower.
    let n = zs.len();
    let mut full: Vec<Vec<Vec<usize>>> = vec![vec![Vec::new(); ops.len()]; n];
    let mut close: Vec<Vec<Vec<usize>>> = vec![vec![Vec::new(); ops.len()]; n];
    let first_at = |z: f64| zs.partition_point(|l| *l < z - 1e-9);
    for (o, op) in ops.iter().enumerate() {
        let wide = op.cutter.max_radius();
        // How far above its tip the tool is its full width.
        let rise = op.cutter.height_at(wide).unwrap_or(f64::INFINITY);
        for (i, p) in op.pieces.iter().enumerate() {
            let (low, high) = (p.a.z.min(p.b.z), p.a.z.max(p.b.z));
            let from = first_at(low);
            let whole = first_at(high + rise).max(from);
            for k in from..whole.min(n) {
                if let Some(c) = close.get_mut(k).and_then(|c| c.get_mut(o)) {
                    c.push(i);
                }
            }
            if let Some(f) = full.get_mut(whole).and_then(|f| f.get_mut(o)) {
                f.push(i);
            }
        }
    }

    // From the lowest level up: the material standing at each as the full
    // width sweeps leave it, every level's own.
    let size = TILE_RADII
        * ops
            .iter()
            .map(|o| o.cutter.max_radius())
            .fold(1.0, f64::max);
    let mut blocks = material(stocks, lowest)?;
    let mut standing = Tiles::new(&blocks, size)?;
    let mut swept_full: Vec<Region> = Vec::with_capacity(n);
    for (k, &z) in zs.iter().enumerate() {
        watch.check()?;
        // A block whose top is below this level has none of it.
        let here = material(stocks, z)?;
        if here.area() < blocks.area() - 1e-9 {
            standing.subtract(&blocks.difference(&here)?)?;
            blocks = here;
        }
        for (o, op) in ops.iter().enumerate() {
            let Some(these) = full.get(k).and_then(|f| f.get(o)) else {
                continue;
            };
            for line in polylines(&op.pieces, these) {
                standing.subtract_swept(&line, op.cutter.max_radius(), arc)?;
            }
        }
        swept_full.push(standing.whole()?);
    }

    // Each level's own: less the pieces close below it, at their width there,
    // and what is too thin to be worth it. In parallel.
    zs.par_iter()
        .zip(swept_full.into_par_iter())
        .enumerate()
        .map(|(k, (&z, standing))| {
            let mut left = standing;
            let mut cuts: Vec<Region> = Vec::new();
            for (o, op) in ops.iter().enumerate() {
                let Some(these) = close.get(k).and_then(|c| c.get(o)) else {
                    continue;
                };
                for &i in these {
                    let Some(p) = op.pieces.get(i) else {
                        continue;
                    };
                    cuts.extend(sweep_below(*p, z, op.cutter, arc));
                }
            }
            if !cuts.is_empty() {
                left = left.difference(&Region::union_all(&cuts)?)?;
            }
            if ignore > 0.0 {
                // An opening by half of it takes what is thinner than it.
                left = left.offset(-ignore / 2.0, arc).offset(ignore / 2.0, arc);
            }
            Ok((z, left))
        })
        .collect()
}

/// `path`'s feeding moves as straight pieces, its arcs and helixes in chords
/// within `tolerance` of them.
fn pieces(path: &Toolpath, tolerance: f64) -> Vec<Piece> {
    let mut out = Vec::new();
    for (from, m) in path.steps() {
        let (Some(from), Some(_)) = (from, m.feed) else {
            continue;
        };
        // An arc by chords that fall short of it by at most the tolerance.
        let mut prev = from;
        for p in m.chords(from, tolerance) {
            out.push(Piece { a: prev, b: p });
            prev = p;
        }
    }
    out
}

/// The pieces `which` of `pieces`, joined where one starts where the one before
/// it ended, as lines seen from above.
fn polylines(pieces: &[Piece], which: &[usize]) -> Vec<Vec<DVec2>> {
    let mut lines: Vec<Vec<DVec2>> = Vec::new();
    let mut last: Option<(usize, DVec3)> = None;
    for &i in which {
        let Some(p) = pieces.get(i) else {
            continue;
        };
        let (a, b) = (p.a.truncate(), p.b.truncate());
        // Joined only where the path ran on without a rapid between.
        let joined = last.is_some_and(|(l, end)| l + 1 == i && end == p.a);
        match lines.last_mut() {
            Some(line) if joined => line.push(b),
            _ => lines.push(vec![a, b]),
        }
        last = Some((i, p.b));
    }
    lines
}

/// What `cutter` cuts at level `z` along `p`, a piece whose tip comes within
/// the tool's rounded end below it: in slices, each as wide as the tool is
/// at its highest, where it is narrowest at the level.
fn sweep_below(p: Piece, z: f64, cutter: &Cutter, arc: f64) -> Vec<Region> {
    let (low, high) = (p.a.z.min(p.b.z), p.a.z.max(p.b.z));
    if low > z {
        return Vec::new();
    }
    // Only the part of it at or below the level cuts it.
    let (a, b) = if high <= z {
        (p.a, p.b)
    } else {
        let t = (z - p.a.z) / (p.b.z - p.a.z);
        let cross = p.a.lerp(p.b, t.clamp(0.0, 1.0));
        if p.a.z <= z {
            (p.a, cross)
        } else {
            (cross, p.b)
        }
    };
    let rise = (a.z - b.z).abs();
    let step = 0.05 * cutter.max_radius();
    let n = bounded_count((rise / step.max(1e-6)).ceil().clamp(1.0, MOST_SLICES));
    let mut out = Vec::new();
    for k in 0..n {
        let (s, t) = (f64::from(k) / f64::from(n), f64::from(k + 1) / f64::from(n));
        let (from, to) = (a.lerp(b, s), a.lerp(b, t));
        let width = cutter.radius_at(z - from.z.max(to.z));
        if width > 0.0 {
            out.push(Region::around_polylines(
                &[vec![from.truncate(), to.truncate()]],
                width,
                arc,
            ));
        }
    }
    out
}
