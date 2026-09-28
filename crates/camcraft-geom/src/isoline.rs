//! Lines of equal value across a field on a [`TriGrid`], and how the regions they
//! bound nest.
//!
//! # Marching triangles
//!
//! Each triangle a level crosses holds one straight piece of its line, between the
//! two edges it crosses, placed by interpolating along each edge. Never at an
//! edge's midpoint: that is the staircase decision 0003 rules out. The triangles are
//! the field's own, so the interpolation is the one the field was built with, and
//! there is no saddle to resolve. A node exactly at the level counts as above it,
//! so a level through a node is crossed once.
//!
//! Pieces are joined by the edge they share, an exact integer key, so lines join
//! the same way on any machine. Each runs with the higher side on its left.
//!
//! # Nesting
//!
//! Where the field is at least a level falls into separate regions as the level
//! rises: [`nest`] finds them level by level with one union–find sweep down the
//! nodes, and which region of the level below each lies in. A region with nothing
//! above it rising more than half a step over its level gets one more line halfway
//! up: its top-up.

use std::collections::HashMap;

use glam::DVec2;

use crate::trigrid::TriGrid;

/// A line of equal value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IsoLine {
    /// Its points; a closed line does not repeat its first.
    pub points: Vec<DVec2>,
    /// Whether it closes on itself.
    pub closed: bool,
    /// The triangle each segment crosses, one a segment: `points[i]` to the next.
    pub triangles: Vec<u32>,
    /// A node on its higher side.
    pub high: u32,
}

/// Node `n`'s value in `d`, if it has one.
fn value(d: &[f64], n: u32) -> Option<f64> {
    let v = *d.get(usize::try_from(n).ok()?)?;
    v.is_finite().then_some(v)
}

/// The triangles each level `k · step` crosses, `k` from 1: index `k` holds level
/// `k`'s, index 0 none. At most `max_levels` levels.
pub fn crossings(grid: &TriGrid, d: &[f64], step: f64, max_levels: u32) -> Vec<Vec<u32>> {
    let mut levels: Vec<Vec<u32>> = Vec::new();
    if !(step.is_finite() && step > 0.0) {
        return levels;
    }
    for t in grid.triangles() {
        let Some(values) = grid.triangle(t).and_then(|nodes| {
            nodes
                .map(|n| value(d, n))
                .into_iter()
                .collect::<Option<Vec<f64>>>()
        }) else {
            continue;
        };
        let lo = values.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        // Crossed where lo < k·step ≤ hi; a little wide either side, since a level
        // the triangle does not cross yields nothing.
        let first = ((lo / step) - 1e-9).floor() + 1.0;
        let last = ((hi / step) + 1e-9).floor().min(f64::from(max_levels));
        if !(first.is_finite() && last.is_finite()) || last < 1.0 {
            continue;
        }
        let first = first.max(1.0);
        // Both in 1..=max_levels, so the narrowing is exact.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (first, last) = (first as u32, last as u32);
        for k in first..=last {
            let Ok(k) = usize::try_from(k) else { continue };
            if levels.len() <= k {
                levels.resize_with(k + 1, Vec::new);
            }
            if let Some(level) = levels.get_mut(k) {
                level.push(t);
            }
        }
    }
    levels
}

/// One straight piece of a line, in one triangle.
struct Piece {
    from_edge: u64,
    to_edge: u64,
    from: DVec2,
    to: DVec2,
    triangle: u32,
    high: u32,
}

/// The key of the edge between nodes `a` and `b`.
fn edge(a: u32, b: u32) -> u64 {
    (u64::from(a.min(b)) << 32) | u64::from(a.max(b))
}

/// The lines where the field `d` is `level`, across `triangles`.
pub fn lines(
    grid: &TriGrid,
    d: &[f64],
    level: f64,
    triangles: impl IntoIterator<Item = u32>,
) -> Vec<IsoLine> {
    let mut pieces: Vec<Piece> = Vec::new();
    for t in triangles {
        if let Some(piece) = piece(grid, d, level, t) {
            pieces.push(piece);
        }
    }
    chain(pieces)
}

/// The piece of the level's line in triangle `t`, higher side on its left.
fn piece(grid: &TriGrid, d: &[f64], level: f64, t: u32) -> Option<Piece> {
    let nodes = grid.triangle(t)?;
    let values = nodes.map(|n| value(d, n));
    let [va, vb, vc] = values;
    let values = [va?, vb?, vc?];
    let above = values.map(|v| v >= level);
    let count = above.iter().filter(|&&a| a).count();
    if count == 0 || count == 3 {
        return None;
    }
    // Round the triangle anticlockwise, the piece runs from the edge that goes from
    // above to below to the one that comes back up: so the higher side is on its
    // left, and a triangle and its neighbour, which go along the edge they share
    // opposite ways, agree which way the line runs through it. Placed on each edge
    // by interpolation.
    let mut from = None;
    let mut to = None;
    for (i, j) in [(0usize, 1usize), (1, 2), (2, 0)] {
        let (Some(&ni), Some(&nj), Some(&vi), Some(&vj), Some(&ai), Some(&aj)) = (
            nodes.get(i),
            nodes.get(j),
            values.get(i),
            values.get(j),
            above.get(i),
            above.get(j),
        ) else {
            continue;
        };
        if ai == aj {
            continue;
        }
        let (pi, pj) = (grid.xy(ni)?, grid.xy(nj)?);
        let s = ((level - vi) / (vj - vi)).clamp(0.0, 1.0);
        let crossing = (edge(ni, nj), pi + (pj - pi) * s);
        if ai {
            from = Some(crossing);
        } else {
            to = Some(crossing);
        }
    }
    let ((from_edge, from), (to_edge, to)) = (from?, to?);
    let high = nodes
        .iter()
        .zip(values)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(&n, _)| n)?;
    Some(Piece {
        from_edge,
        to_edge,
        from,
        to,
        triangle: t,
        high,
    })
}

/// Pieces joined into lines by the edges they share, in the order found.
fn chain(pieces: Vec<Piece>) -> Vec<IsoLine> {
    let by_start: HashMap<u64, usize> = pieces
        .iter()
        .enumerate()
        .map(|(i, p)| (p.from_edge, i))
        .collect();
    let ends: std::collections::HashSet<u64> = pieces.iter().map(|p| p.to_edge).collect();
    let mut used = vec![false; pieces.len()];
    let mut out = Vec::new();
    // Open lines first, from where they start: a piece nothing ends at.
    let starts: Vec<usize> = (0..pieces.len())
        .filter(|&i| pieces.get(i).is_some_and(|p| !ends.contains(&p.from_edge)))
        .chain(0..pieces.len())
        .collect();
    for start in starts {
        if used.get(start).copied().unwrap_or(true) {
            continue;
        }
        let mut run: Vec<usize> = Vec::new();
        let mut at = Some(start);
        let mut closed = false;
        while let Some(i) = at {
            match used.get_mut(i) {
                Some(u) if !*u => *u = true,
                _ => {
                    closed = i == start;
                    break;
                }
            }
            run.push(i);
            at = pieces
                .get(i)
                .and_then(|p| by_start.get(&p.to_edge))
                .copied();
        }
        if let Some(line) = line(&pieces, &run, closed) {
            out.push(line);
        }
    }
    out
}

/// The line through `run`'s pieces, without the ones of no length.
fn line(pieces: &[Piece], run: &[usize], closed: bool) -> Option<IsoLine> {
    let kept: Vec<&Piece> = run
        .iter()
        .filter_map(|&i| pieces.get(i))
        .filter(|p| p.from.distance(p.to) > 1e-12)
        .collect();
    let first = kept.first()?;
    let mut points = vec![first.from];
    let mut triangles = Vec::with_capacity(kept.len());
    for p in &kept {
        points.push(p.to);
        triangles.push(p.triangle);
    }
    if closed {
        // The last piece ends where the first began.
        points.pop();
        if points.len() < 3 {
            return None;
        }
    }
    Some(IsoLine {
        points,
        closed,
        triangles,
        high: first.high,
    })
}

/// A region where the field is at least its level: one of the pieces `{d ≥ k·step}`
/// falls into.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Region {
    /// Its level, `k`.
    pub level: u32,
    /// The lines of its level that bound it, by index into that level's lines.
    pub lines: Vec<usize>,
    /// The region of the level below it lies in.
    pub parent: Option<usize>,
    /// The regions of the level above lying in it.
    pub children: Vec<usize>,
    /// The highest value in it.
    pub top: f64,
    /// A node in it.
    pub rep: u32,
    /// With nothing above it, and its top more than half a step over its level:
    /// the lines halfway up to its top.
    pub top_up: Vec<IsoLine>,
}

/// How the regions `{d ≥ k·step}` nest, for `k` from the highest level with lines
/// down to 0. `lines[k]` are level `k`'s lines (`lines[0]` is not read: level 0 is
/// everywhere the field is not negative).
///
/// Regions come out top level first; every region of level `k > 0` has a parent at
/// `k − 1`.
pub fn nest(grid: &TriGrid, d: &[f64], step: f64, lines: &[Vec<IsoLine>]) -> Vec<Region> {
    let mut regions: Vec<Region> = Vec::new();
    let count = grid.nodes();
    // Every node on or above level 0, highest first, ties by number.
    let mut order: Vec<(f64, u32)> = (0..count)
        .filter_map(|n| value(d, n).filter(|&v| v >= 0.0).map(|v| (v, n)))
        .collect();
    order.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));

    let mut sets = Sets::new(count);
    let mut next = 0usize;
    let top_level = u32::try_from(lines.len().saturating_sub(1)).unwrap_or(0);
    // The last level's regions, by the node each was found from.
    let mut above: Vec<(u32, usize)> = Vec::new();
    for k in (0..=top_level).rev() {
        let level = f64::from(k) * step;
        while let Some(&(v, n)) = order.get(next) {
            if v < level {
                break;
            }
            sets.add(n, v);
            for m in grid.neighbours(n) {
                if sets.has(m) {
                    sets.union(n, m);
                }
            }
            next += 1;
        }

        let mut here = Found {
            level: k,
            by_root: HashMap::new(),
            found: Vec::new(),
        };
        if k > 0 {
            for (i, line) in lines
                .get(usize::try_from(k).unwrap_or(usize::MAX))
                .into_iter()
                .flatten()
                .enumerate()
            {
                if !sets.has(line.high) {
                    continue;
                }
                let root = sets.find(line.high);
                let r = here.region(root, sets.top(root), &mut regions);
                if let Some(region) = regions.get_mut(r) {
                    region.lines.push(i);
                }
            }
        }
        // Each region of the level above lies in one of this level's, which has a
        // line round it unless it runs off the field; then it stands alone.
        for &(root, child) in &above {
            let root = sets.find(root);
            let parent = here.region(root, sets.top(root), &mut regions);
            if let Some(c) = regions.get_mut(child) {
                c.parent = Some(parent);
            }
            if let Some(p) = regions.get_mut(parent) {
                p.children.push(child);
            }
        }
        if k == 0 {
            // Level 0 is everywhere the field is not negative: every set is a
            // region, those with nothing above them too.
            for &(_, n) in &order {
                let root = sets.find(n);
                here.region(root, sets.top(root), &mut regions);
            }
        }
        for &(root, r) in &here.found {
            let Some(region) = regions.get(r) else {
                continue;
            };
            let crest = (level + region.top) / 2.0;
            if !region.children.is_empty() || region.top - level <= step / 2.0 {
                continue;
            }
            let root = sets.find(root);
            let mut triangles: Vec<u32> = sets
                .members(root)
                .filter(|&n| value(d, n).is_some_and(|v| v >= crest))
                .flat_map(|n| grid.triangles_at(n).collect::<Vec<u32>>())
                .collect();
            triangles.sort_unstable();
            triangles.dedup();
            let top_up = self::lines(grid, d, crest, triangles);
            if let Some(region) = regions.get_mut(r) {
                region.top_up = top_up;
            }
        }
        above = here.found;
    }
    regions
}

/// The regions found at one level, by the root of their set.
struct Found {
    level: u32,
    by_root: HashMap<u32, usize>,
    found: Vec<(u32, usize)>,
}

impl Found {
    /// The region whose set `root` heads, made the first time it is asked for.
    fn region(&mut self, root: u32, top: f64, regions: &mut Vec<Region>) -> usize {
        if let Some(&r) = self.by_root.get(&root) {
            return r;
        }
        regions.push(Region {
            level: self.level,
            top,
            rep: root,
            ..Region::default()
        });
        let r = regions.len() - 1;
        self.by_root.insert(root, r);
        self.found.push((root, r));
        r
    }
}

/// Disjoint sets of nodes, each knowing its highest value and its members.
struct Sets {
    parent: Vec<u32>,
    size: Vec<u32>,
    top: Vec<f64>,
    /// The next member of a node's set, in a list each set's root heads.
    next: Vec<u32>,
    /// The last member of the list a root heads.
    last: Vec<u32>,
    added: Vec<bool>,
}

impl Sets {
    fn new(count: u32) -> Self {
        let n = usize::try_from(count).unwrap_or(0);
        Self {
            parent: (0..count).collect(),
            size: vec![1; n],
            top: vec![f64::NEG_INFINITY; n],
            next: vec![u32::MAX; n],
            last: (0..count).collect(),
            added: vec![false; n],
        }
    }

    fn slot(n: u32) -> usize {
        usize::try_from(n).unwrap_or(usize::MAX)
    }

    fn has(&self, n: u32) -> bool {
        self.added.get(Self::slot(n)).copied().unwrap_or(false)
    }

    fn add(&mut self, n: u32, value: f64) {
        if let Some(a) = self.added.get_mut(Self::slot(n)) {
            *a = true;
        }
        if let Some(t) = self.top.get_mut(Self::slot(n)) {
            *t = value;
        }
    }

    fn find(&mut self, n: u32) -> u32 {
        let mut root = n;
        while let Some(&p) = self.parent.get(Self::slot(root)) {
            if p == root {
                break;
            }
            root = p;
        }
        // Halve the path behind.
        let mut at = n;
        while at != root {
            let Some(&p) = self.parent.get(Self::slot(at)) else {
                break;
            };
            if let Some(slot) = self.parent.get_mut(Self::slot(at)) {
                *slot = root;
            }
            at = p;
        }
        root
    }

    fn top(&self, root: u32) -> f64 {
        self.top
            .get(Self::slot(root))
            .copied()
            .unwrap_or(f64::NEG_INFINITY)
    }

    fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        let size = |s: &Self, r: u32| s.size.get(Self::slot(r)).copied().unwrap_or(0);
        // The larger keeps its root; ties to the lower node, for the same result
        // every time.
        let (keep, gone) = match size(self, ra).cmp(&size(self, rb)) {
            std::cmp::Ordering::Greater => (ra, rb),
            std::cmp::Ordering::Less => (rb, ra),
            std::cmp::Ordering::Equal => (ra.min(rb), ra.max(rb)),
        };
        let merged = size(self, keep) + size(self, gone);
        let top = self.top(keep).max(self.top(gone));
        if let Some(p) = self.parent.get_mut(Self::slot(gone)) {
            *p = keep;
        }
        if let Some(s) = self.size.get_mut(Self::slot(keep)) {
            *s = merged;
        }
        if let Some(t) = self.top.get_mut(Self::slot(keep)) {
            *t = top;
        }
        // Splice the gone set's members onto the end of the kept one's.
        let tail = self.last.get(Self::slot(keep)).copied().unwrap_or(keep);
        let gone_tail = self.last.get(Self::slot(gone)).copied().unwrap_or(gone);
        if let Some(n) = self.next.get_mut(Self::slot(tail)) {
            *n = gone;
        }
        if let Some(l) = self.last.get_mut(Self::slot(keep)) {
            *l = gone_tail;
        }
    }

    /// The members of the set `root` heads.
    fn members(&self, root: u32) -> impl Iterator<Item = u32> + '_ {
        let mut at = Some(root);
        std::iter::from_fn(move || {
            let n = at?;
            let next = self.next.get(Self::slot(n)).copied().unwrap_or(u32::MAX);
            at = (next != u32::MAX).then_some(next);
            Some(n)
        })
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use glam::dvec2;

    /// A flat grid of `n × n` nodes `pitch` apart, and a field from `f`.
    fn field(n: u32, pitch: f64, f: impl Fn(DVec2) -> f64) -> (TriGrid, Vec<f64>) {
        let count = usize::try_from(n * n).unwrap();
        let g = TriGrid::new(
            DVec2::ZERO,
            pitch,
            n,
            n,
            vec![0.0; count],
            vec![true; count],
        )
        .unwrap();
        let d = (0..g.nodes()).map(|m| f(g.xy(m).unwrap())).collect();
        (g, d)
    }

    fn all(g: &TriGrid) -> Vec<u32> {
        g.triangles().collect()
    }

    /// A linear field's line is exactly where it should be, runs with the higher
    /// side on its left, and has no point twice even where it passes through nodes.
    #[test]
    fn a_linear_fields_line_is_exact() {
        let normal = dvec2(0.8, 0.6);
        let (g, d) = field(21, 0.5, |p| p.dot(normal));
        for level in [3.3, 4.0] {
            let found = lines(&g, &d, level, all(&g));
            assert_eq!(found.len(), 1, "level {level}");
            let line = &found[0];
            assert!(!line.closed);
            for p in &line.points {
                assert!((p.dot(normal) - level).abs() < 1e-12, "{p:?}");
            }
            for w in line.points.windows(2) {
                assert!(w[0].distance(w[1]) > 1e-12, "a point twice");
                // Higher to the left: the direction turned left points up the field.
                assert!((w[1] - w[0]).perp().dot(normal) > 0.0);
            }
            assert_eq!(line.triangles.len(), line.points.len() - 1);
        }
    }

    /// Round a peak the line closes, anticlockwise, the peak on its left.
    #[test]
    fn a_line_round_a_peak_closes() {
        let centre = dvec2(5.0, 5.0);
        let (g, d) = field(41, 0.25, |p| 5.0 - p.distance(centre));
        let found = lines(&g, &d, 2.0, all(&g));
        assert_eq!(found.len(), 1);
        let line = &found[0];
        assert!(line.closed);
        let area: f64 = line
            .points
            .iter()
            .zip(line.points.iter().cycle().skip(1))
            .map(|(a, b)| a.perp_dot(*b) / 2.0)
            .sum();
        assert!(area > 0.0);
        for p in &line.points {
            assert!((p.distance(centre) - 3.0).abs() < 0.02);
        }
    }

    /// A saddle: two lines that never cross.
    #[test]
    fn a_saddle_splits_cleanly() {
        let (g, d) = field(21, 0.5, |p| (p.x - 5.0) * (p.y - 5.0));
        let found = lines(&g, &d, 0.3, all(&g));
        assert_eq!(found.len(), 2);
        for line in &found {
            assert!(!line.closed);
        }
    }

    /// Where the field falls away on both sides of a ring, the ring and the hole
    /// inside it are one region; two peaks are two, in one below them; a region
    /// rising more than half a step over its level gets a line halfway up.
    #[test]
    fn regions_nest() {
        // Two peaks, 4 high, over a floor at 1, the whole ringed by a fall to -1.
        let peaks = [dvec2(3.0, 5.0), dvec2(7.0, 5.0)];
        let (g, d) = field(41, 0.25, |p| {
            let bump = peaks
                .iter()
                .map(|c| 3.0 - 2.0 * p.distance(*c))
                .fold(f64::MIN, f64::max);
            let edge = p.x.min(p.y).min(10.0 - p.x).min(10.0 - p.y);
            bump.max(1.0).min(edge * 2.0 - 1.0)
        });
        let step = 1.0;
        let buckets = crossings(&g, &d, step, 100);
        let levels: Vec<Vec<IsoLine>> = buckets
            .iter()
            .enumerate()
            .map(|(k, tris)| lines(&g, &d, k as f64 * step, tris.iter().copied()))
            .collect();
        let regions = nest(&g, &d, step, &levels);
        let at = |k: u32| regions.iter().filter(|r| r.level == k).count();
        assert_eq!(at(0), 1);
        assert_eq!(at(1), 1);
        assert_eq!(at(2), 2, "a region for each peak");
        for r in regions.iter().filter(|r| r.level > 0) {
            let parent = &regions[r.parent.unwrap()];
            assert_eq!(parent.level, r.level - 1);
        }
        // Each peak tops out at 3, a step over level 2: a top-up at 2.5.
        for r in regions.iter().filter(|r| r.level == 2) {
            assert!((r.top - 3.0).abs() < 0.05);
            assert!(r.children.is_empty());
            assert_eq!(r.top_up.len(), 1, "{r:?}");
            assert!(r.top_up[0].closed);
        }
    }
}
