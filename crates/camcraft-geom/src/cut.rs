//! A grid whose cells are cut where what lies under them changes, each side
//! triangulated right up to the change.
//!
//! # Classes, cuts and bands
//!
//! Each node of a [`TriGrid`] has a class: what it stands for to the caller,
//! as which face of the part the tool touches there, or that nothing is
//! under it; or [`UNKNOWN`]. Some classes make up *bands*, the pieces of
//! ground a field is measured over; the rest are none.
//!
//! Along every lattice edge whose ends differ in class, one of them a band's,
//! the caller finds where the class changes: a *cut*, a node of its own at
//! that place and height. A cell with a cut on its sides is split by them.
//! Each run of its corners of one band's class, with the cuts either side of
//! the run, is a convex polygon, its corners on the cell's sides, fanned into
//! triangles. So a band is triangulated right up to where it ends, not to the
//! last whole triangle of its nodes, which may be most of a cell short of it.
//! A cell with a corner of [`UNKNOWN`] class is never cut: of its lattice
//! triangles, those whose corners are all of one band's class are kept.
//!
//! A band is the nodes of one class joined through its triangles. Two runs of
//! a class in one cell, across a saddle, are not joined there.
//!
//! # Views
//!
//! [`CutGrid::view`] is one band's triangles, or every band's, as a
//! [`TriSurface`]: fields are measured along it and lines drawn across it as
//! on the grid itself. Lattice triangles keep their ids; the cut cells'
//! follow them.
//!
//! # Edges
//!
//! The cuts round a band, in order, are its edges ([`CutGrid::edges`]): each
//! cut cell's polygon closes from the cut it leaves by to the cut it came in
//! by, a straight piece of the edge with the band on its left.

use glam::{DVec2, DVec3};
use rayon::prelude::*;

use crate::trigrid::{TriGrid, TriSurface};

/// The class of a node nothing is known of: a cell with such a corner is
/// never cut.
pub const UNKNOWN: u32 = u32::MAX;

/// No band.
pub const NO_BAND: u32 = u32::MAX;

/// Where the class changes along a lattice edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cut {
    /// The place, at its height.
    pub at: DVec3,
    /// The lattice edge, its node of a band's class first.
    pub edge: (u32, u32),
    /// The band on each side: the first node's, then the second's;
    /// [`NO_BAND`] where its class makes none.
    pub bands: (u32, u32),
}

/// One run of a cut cell's corners of a band's class, fanned into triangles:
/// the cuts it comes in and leaves by.
#[derive(Debug, Clone, Copy)]
struct Polygon {
    band: u32,
    enter: u32,
    leave: u32,
}

/// A grid with its cells cut where the class of its nodes changes.
#[derive(Debug, Clone)]
pub struct CutGrid {
    grid: TriGrid,
    class: Vec<u32>,
    cuts: Vec<Cut>,
    /// The cut cells' triangles, cell by cell, and the band of each.
    tris: Vec<[u32; 3]>,
    tri_band: Vec<u32>,
    /// Where each cell's triangles start in `tris`: one more than the cells.
    cell_start: Vec<u32>,
    /// Whether each cell is cut.
    is_cut: Vec<bool>,
    polygons: Vec<Polygon>,
    node_band: Vec<u32>,
    bands: u32,
    /// Whether each band has a cell no edge cuts ([`CutGrid::resolved`]).
    whole: Vec<bool>,
}

impl CutGrid {
    /// `grid` with each node of `class`, its cells cut where the class
    /// changes. A class makes up bands where `makes_band` says so. `find(a, b)`
    /// is where the class changes along the lattice edge from node `a`, of a
    /// band's class, to node `b`, at its height: asked once for each edge,
    /// in parallel.
    pub fn new(
        grid: TriGrid,
        class: Vec<u32>,
        makes_band: impl Fn(u32) -> bool + Sync,
        find: impl Fn(u32, u32) -> DVec3 + Sync,
    ) -> Self {
        let (nx, ny) = (grid.nx(), grid.ny());
        let cells = (nx - 1) * (ny - 1);
        let class_of = |n: u32| {
            usize::try_from(n)
                .ok()
                .and_then(|i| class.get(i))
                .copied()
                .unwrap_or(UNKNOWN)
        };
        let corners = |c: u32| {
            let (cx, cy) = (c % (nx - 1), c / (nx - 1));
            let a = cy * nx + cx;
            [a, a + 1, a + nx + 1, a + nx]
        };
        let is_cut: Vec<bool> = (0..cells)
            .map(|c| {
                let k = corners(c).map(class_of);
                k.iter().all(|&k| k != UNKNOWN)
                    && k.iter().any(|&x| x != k[0])
                    && k.iter().any(|&x| makes_band(x))
            })
            .collect();
        let cut_cell = |c: u32| {
            usize::try_from(c)
                .ok()
                .and_then(|i| is_cut.get(i))
                .copied()
                .unwrap_or(false)
        };

        // The lattice edges to cut, each once, in order: of a band's class on
        // one side at least, the node of a band's class first.
        let mut edges: Vec<(u32, u32)> = Vec::new();
        for c in (0..cells).filter(|&c| cut_cell(c)) {
            let k = corners(c);
            for (&a, &b) in k.iter().zip(k.iter().cycle().skip(1)) {
                let (ca, cb) = (class_of(a), class_of(b));
                if ca == cb || !(makes_band(ca) || makes_band(cb)) {
                    continue;
                }
                let first = if makes_band(ca) && (!makes_band(cb) || a < b) {
                    (a, b)
                } else {
                    (b, a)
                };
                edges.push(first);
            }
        }
        edges.sort_unstable_by_key(|&(a, b)| (a.min(b), a.max(b)));
        edges.dedup_by_key(|&mut (a, b)| (a.min(b), a.max(b)));
        let found: Vec<DVec3> = edges.par_iter().map(|&(a, b)| find(a, b)).collect();
        let lattice = grid.nodes();
        let cut_at = |a: u32, b: u32| {
            edges
                .binary_search_by_key(&(a.min(b), a.max(b)), |&(p, q)| (p.min(q), p.max(q)))
                .ok()
                .and_then(|i| u32::try_from(i).ok())
                .map(|i| lattice + i)
        };

        // Each cut cell's polygons, fanned from their first corner.
        let mut tris = Vec::new();
        let mut tri_first = Vec::new();
        let mut polygons_found = Vec::new();
        let mut cell_start = Vec::with_capacity(usize::try_from(cells).unwrap_or(0) + 1);
        for c in 0..cells {
            cell_start.push(u32::try_from(tris.len()).unwrap_or(u32::MAX));
            if !cut_cell(c) {
                continue;
            }
            let k = corners(c);
            // Corner `i` round the cell, and its class.
            let node = |i: usize| k.get(i % 4).copied().unwrap_or(u32::MAX);
            let class = |i: usize| class_of(node(i));
            for start in 0..4 {
                if class(start) == class(start + 3) || !makes_band(class(start)) {
                    continue;
                }
                let mut end = start;
                while class(end + 1) == class(start) {
                    end += 1;
                }
                let (Some(enter), Some(leave)) = (
                    cut_at(node(start + 3), node(start)),
                    cut_at(node(end), node(end + 1)),
                ) else {
                    continue;
                };
                let mut poly = vec![enter];
                poly.extend((start..=end).map(node));
                poly.push(leave);
                // Fanned from the first corner: every triangle has a lattice
                // node, which joins it to its band.
                let first = node(start);
                for w in poly.get(2..).unwrap_or_default().windows(2) {
                    if let [p, q] = *w {
                        tris.push([first, p, q]);
                        tri_first.push(first);
                    }
                }
                tris.push([first, leave, enter]);
                tri_first.push(first);
                polygons_found.push((first, enter, leave));
            }
        }
        cell_start.push(u32::try_from(tris.len()).unwrap_or(u32::MAX));

        // Bands: the nodes of one class joined through their triangles.
        let count = usize::try_from(lattice).unwrap_or(0);
        let mut sets = Union::new(count);
        let mut member = vec![false; count];
        let mut join = |nodes: &[u32], sets: &mut Union| {
            let Some(&first) = nodes.first() else { return };
            for &n in nodes.iter().filter(|&&n| n < lattice) {
                if let Some(m) = usize::try_from(n).ok().and_then(|i| member.get_mut(i)) {
                    *m = true;
                }
                sets.union(first, n);
            }
        };
        for c in (0..cells).filter(|&c| !cut_cell(c)) {
            for t in [2 * c, 2 * c + 1] {
                if let Some(nodes) = grid.triangle(t)
                    && kept(&nodes, &class_of, &makes_band)
                {
                    join(&nodes, &mut sets);
                }
            }
        }
        for (t, &first) in tris.iter().zip(&tri_first) {
            let lattice_corners: Vec<u32> = std::iter::once(first)
                .chain(t.iter().copied().filter(|&n| n < lattice))
                .collect();
            join(&lattice_corners, &mut sets);
        }
        let mut node_band = vec![NO_BAND; count];
        let mut renumber = vec![NO_BAND; count];
        let mut bands = 0u32;
        for n in 0..lattice {
            let i = usize::try_from(n).unwrap_or(usize::MAX);
            if !member.get(i).copied().unwrap_or(false) {
                continue;
            }
            let root = usize::try_from(sets.find(n)).unwrap_or(usize::MAX);
            let Some(slot) = renumber.get_mut(root) else {
                continue;
            };
            if *slot == NO_BAND {
                *slot = bands;
                bands += 1;
            }
            let band = *slot;
            if let Some(b) = node_band.get_mut(i) {
                *b = band;
            }
        }
        let band_of = |n: u32| {
            usize::try_from(n)
                .ok()
                .and_then(|i| node_band.get(i))
                .copied()
                .unwrap_or(NO_BAND)
        };
        // Which bands have a cell no edge cuts: one with none lies wholly in
        // cells its edges cut, finer than the grid resolves.
        let mut whole = vec![false; usize::try_from(bands).unwrap_or(0)];
        for c in (0..cells).filter(|&c| !cut_cell(c)) {
            let kept_band = [2 * c, 2 * c + 1].into_iter().find_map(|t| {
                let nodes = grid.triangle(t)?;
                kept(&nodes, &class_of, &makes_band).then(|| band_of(nodes[0]))
            });
            if let Some(w) = kept_band.and_then(|b| whole.get_mut(usize::try_from(b).ok()?)) {
                *w = true;
            }
        }
        let tri_band = tri_first.iter().map(|&n| band_of(n)).collect();
        let polygons = polygons_found
            .into_iter()
            .map(|(first, enter, leave)| Polygon {
                band: band_of(first),
                enter,
                leave,
            })
            .collect();
        let cuts = edges
            .iter()
            .zip(found)
            .map(|(&(a, b), at)| Cut {
                at,
                edge: (a, b),
                bands: (band_of(a), band_of(b)),
            })
            .collect();
        Self {
            grid,
            class,
            cuts,
            tris,
            tri_band,
            cell_start,
            is_cut,
            polygons,
            node_band,
            bands,
            whole,
        }
    }

    /// Whether band `b` has a cell of the grid no edge of it cuts: one that
    /// has none lies wholly in cells its edges cut, an island or a sliver
    /// finer than the grid resolves.
    pub fn resolved(&self, b: u32) -> bool {
        usize::try_from(b)
            .ok()
            .and_then(|i| self.whole.get(i))
            .copied()
            .unwrap_or(false)
    }

    /// The class across the most of band `b`'s cuts, the lowest of as many:
    /// what lies round it.
    pub fn class_round(&self, b: u32) -> Option<u32> {
        let mut across: Vec<u32> = self
            .cuts
            .iter()
            .filter_map(|c| {
                if c.bands.0 == b {
                    Some(self.class(c.edge.1))
                } else if c.bands.1 == b {
                    Some(self.class(c.edge.0))
                } else {
                    None
                }
            })
            .collect();
        across.sort_unstable();
        let mut best: Option<(usize, u32)> = None;
        for run in across.chunk_by(|a, b| a == b) {
            if let Some(&k) = run.first()
                && best.is_none_or(|(n, _)| run.len() > n)
            {
                best = Some((run.len(), k));
            }
        }
        best.map(|(_, k)| k)
    }

    /// The grid it was cut from.
    pub fn grid(&self) -> &TriGrid {
        &self.grid
    }

    /// How many bands there are: band ids run below it.
    pub fn bands(&self) -> u32 {
        self.bands
    }

    /// Lattice node `n`'s class.
    pub fn class(&self, n: u32) -> u32 {
        usize::try_from(n)
            .ok()
            .and_then(|i| self.class.get(i))
            .copied()
            .unwrap_or(UNKNOWN)
    }

    /// The band lattice node `n` is in, or [`NO_BAND`].
    pub fn band_of(&self, n: u32) -> u32 {
        usize::try_from(n)
            .ok()
            .and_then(|i| self.node_band.get(i))
            .copied()
            .unwrap_or(NO_BAND)
    }

    /// The cuts, in node order: cut `i` is node `grid().nodes() + i`.
    pub fn cuts(&self) -> &[Cut] {
        &self.cuts
    }

    /// The cut that node `n` is, if it is one.
    pub fn cut(&self, n: u32) -> Option<&Cut> {
        let i = n.checked_sub(self.grid.nodes())?;
        self.cuts.get(usize::try_from(i).ok()?)
    }

    /// Band `band`'s triangles, or every band's with `None`, as a surface.
    pub fn view(&self, band: Option<u32>) -> View<'_> {
        View { cut: self, band }
    }

    /// The edges round `band`, each its cuts in order with the band on its
    /// left, and whether it closes on itself.
    pub fn edges(&self, band: u32) -> Vec<(Vec<u32>, bool)> {
        let lattice = self.grid.nodes();
        let slot = |n: u32| usize::try_from(n.checked_sub(lattice)?).ok();
        let count = self.cuts.len();
        // From each cut, the next along the edge, and whether one comes in.
        let mut next = vec![u32::MAX; count];
        let mut entered = vec![false; count];
        for p in self.polygons.iter().filter(|p| p.band == band) {
            if let (Some(l), Some(e)) = (slot(p.leave), slot(p.enter)) {
                if let Some(n) = next.get_mut(l) {
                    *n = p.enter;
                }
                if let Some(x) = entered.get_mut(e) {
                    *x = true;
                }
            }
        }
        let mut used = vec![false; count];
        let mut out = Vec::new();
        let leads = |i: usize| next.get(i).is_some_and(|&n| n != u32::MAX);
        let comes_in = |i: usize| entered.get(i).copied().unwrap_or(false);
        // Open edges first, from where they start: a cut nothing comes in to.
        let starts = (0..count)
            .filter(|&i| leads(i) && !comes_in(i))
            .chain(0..count);
        for start in starts {
            if used.get(start).copied().unwrap_or(true) || !leads(start) {
                continue;
            }
            let mut line = Vec::new();
            let mut at = start;
            let mut closed = false;
            while let Some(u) = used.get_mut(at) {
                if *u {
                    closed = at == start;
                    break;
                }
                *u = true;
                line.push(lattice + u32::try_from(at).unwrap_or(0));
                let n = next.get(at).copied().unwrap_or(u32::MAX);
                if n == u32::MAX {
                    break;
                }
                at = n
                    .checked_sub(lattice)
                    .and_then(|i| usize::try_from(i).ok())
                    .unwrap_or(usize::MAX);
            }
            if line.len() > 1 {
                out.push((line, closed));
            }
        }
        out
    }

    /// The cell `p` is over, if it is over the grid.
    fn cell_at(&self, p: DVec2) -> Option<u32> {
        let g = &self.grid;
        let local = (p - g.origin()) / g.pitch();
        let far = DVec2::new(f64::from(g.nx() - 1), f64::from(g.ny() - 1));
        if !(local.cmpge(DVec2::ZERO).all() && local.cmple(far).all()) {
            return None;
        }
        // In range, so the narrowing is exact.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (cx, cy) = (
            (local.x.floor() as u32).min(g.nx() - 2),
            (local.y.floor() as u32).min(g.ny() - 2),
        );
        Some(cy * (g.nx() - 1) + cx)
    }

    fn cut_cell(&self, c: u32) -> bool {
        usize::try_from(c)
            .ok()
            .and_then(|i| self.is_cut.get(i))
            .copied()
            .unwrap_or(false)
    }

    /// Cell `c`'s cut triangles, by id.
    fn cell_tris(&self, c: u32) -> std::ops::Range<u32> {
        let at = |i: u32| {
            usize::try_from(i)
                .ok()
                .and_then(|i| self.cell_start.get(i))
                .copied()
                .unwrap_or(0)
        };
        let slots = self.grid.triangle_slots();
        slots + at(c)..slots + at(c + 1)
    }

    /// The lattice triangle `t`, if it is kept, with its band.
    fn lattice_triangle(&self, t: u32) -> Option<([u32; 3], u32)> {
        if self.cut_cell(t / 2) {
            return None;
        }
        // Kept where its corners are all of one class and in a band: only a
        // band's class has nodes in one.
        let nodes = self.grid.triangle(t)?;
        let [a, b, c] = nodes;
        let band = self.band_of(a);
        let one = self.class(a) == self.class(b) && self.class(a) == self.class(c);
        (one && band != NO_BAND).then_some((nodes, band))
    }

    /// Triangle `t`, lattice or cut, with its band.
    fn any_triangle(&self, t: u32) -> Option<([u32; 3], u32)> {
        let slots = self.grid.triangle_slots();
        if t < slots {
            return self.lattice_triangle(t);
        }
        let i = usize::try_from(t - slots).ok()?;
        Some((*self.tris.get(i)?, *self.tri_band.get(i)?))
    }

    /// The cells round lattice node `n`, or either side of cut `n`'s edge.
    fn cells_by(&self, n: u32) -> Vec<u32> {
        let (nx, ny) = (self.grid.nx(), self.grid.ny());
        let cell = |cx: i64, cy: i64| {
            (cx >= 0 && cy >= 0 && cx < i64::from(nx - 1) && cy < i64::from(ny - 1))
                .then(|| u32::try_from(cy * i64::from(nx - 1) + cx).ok())
                .flatten()
        };
        let at = |m: u32| (i64::from(m % nx), i64::from(m / nx));
        match self.cut(n) {
            Some(cut) => {
                let (a, b) = (cut.edge.0.min(cut.edge.1), cut.edge.0.max(cut.edge.1));
                let (ax, ay) = at(a);
                if b == a + 1 {
                    [cell(ax, ay), cell(ax, ay - 1)]
                        .into_iter()
                        .flatten()
                        .collect()
                } else {
                    [cell(ax, ay), cell(ax - 1, ay)]
                        .into_iter()
                        .flatten()
                        .collect()
                }
            }
            None => {
                let (x, y) = at(n);
                [(x - 1, y - 1), (x, y - 1), (x - 1, y), (x, y)]
                    .into_iter()
                    .filter_map(|(cx, cy)| cell(cx, cy))
                    .collect()
            }
        }
    }
}

/// Whether a lattice triangle of `nodes` is kept uncut: all of one class,
/// a band's.
fn kept(nodes: &[u32; 3], class: &impl Fn(u32) -> u32, makes_band: &impl Fn(u32) -> bool) -> bool {
    let k = class(nodes[0]);
    k != UNKNOWN && makes_band(k) && nodes.iter().all(|&n| class(n) == k)
}

/// One band's triangles of a [`CutGrid`], or every band's.
#[derive(Debug, Clone, Copy)]
pub struct View<'a> {
    cut: &'a CutGrid,
    band: Option<u32>,
}

impl View<'_> {
    fn ours(&self, band: u32) -> bool {
        self.band.is_none_or(|b| b == band)
    }

    /// The grid it is a view of.
    pub fn cut_grid(&self) -> &CutGrid {
        self.cut
    }
}

impl TriSurface for View<'_> {
    fn nodes(&self) -> u32 {
        let cuts = u32::try_from(self.cut.cuts.len()).unwrap_or(0);
        self.cut.grid.nodes() + cuts
    }

    fn xy(&self, n: u32) -> Option<DVec2> {
        match self.cut.cut(n) {
            Some(c) => Some(c.at.truncate()),
            None => self.cut.grid.xy(n),
        }
    }

    fn z(&self, n: u32) -> Option<f64> {
        match self.cut.cut(n) {
            Some(c) => c.at.z.is_finite().then_some(c.at.z),
            None => self.cut.grid.z(n),
        }
    }

    fn triangle(&self, t: u32) -> Option<[u32; 3]> {
        let (nodes, band) = self.cut.any_triangle(t)?;
        self.ours(band).then_some(nodes)
    }

    fn triangles(&self) -> impl Iterator<Item = (u32, [u32; 3])> + '_ {
        let total =
            self.cut.grid.triangle_slots() + u32::try_from(self.cut.tris.len()).unwrap_or(0);
        (0..total).filter_map(|t| self.triangle(t).map(|nodes| (t, nodes)))
    }

    fn triangles_at(&self, n: u32) -> impl Iterator<Item = (u32, [u32; 3])> + '_ {
        self.cut.cells_by(n).into_iter().flat_map(move |c| {
            let lattice = (!self.cut.cut_cell(c)).then_some([2 * c, 2 * c + 1]);
            lattice
                .into_iter()
                .flatten()
                .chain(self.cut.cell_tris(c))
                .filter_map(move |t| {
                    self.triangle(t)
                        .filter(|nodes| nodes.contains(&n))
                        .map(|nodes| (t, nodes))
                })
        })
    }

    fn locate(&self, p: DVec2) -> Option<u32> {
        let c = self.cut.cell_at(p)?;
        if !self.cut.cut_cell(c) {
            let t = self.cut.grid.locate(p)?;
            return self.triangle(t).map(|_| t);
        }
        let eps = 1e-12 * self.cut.grid.pitch();
        self.cut.cell_tris(c).find(|&t| {
            self.triangle(t).is_some_and(|nodes| {
                let [a, b, q] = nodes.map(|n| self.xy(n).unwrap_or(DVec2::NAN));
                let inside =
                    |u: DVec2, v: DVec2| (v - u).perp_dot(p - u) >= -eps * (v - u).length();
                inside(a, b) && inside(b, q) && inside(q, a)
            })
        })
    }

    fn pitch(&self) -> f64 {
        self.cut.grid.pitch()
    }
}

/// Disjoint sets of nodes.
struct Union {
    parent: Vec<u32>,
}

impl Union {
    fn new(count: usize) -> Self {
        Self {
            parent: (0..u32::try_from(count).unwrap_or(0)).collect(),
        }
    }

    fn find(&mut self, n: u32) -> u32 {
        let slot = |n: u32| usize::try_from(n).unwrap_or(usize::MAX);
        let mut root = n;
        while let Some(&p) = self.parent.get(slot(root)) {
            if p == root {
                break;
            }
            root = p;
        }
        let mut at = n;
        while at != root {
            let Some(&p) = self.parent.get(slot(at)) else {
                break;
            };
            if let Some(s) = self.parent.get_mut(slot(at)) {
                *s = root;
            }
            at = p;
        }
        root
    }

    fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        // The lower root keeps, for the same sets every time.
        let (keep, gone) = (ra.min(rb), ra.max(rb));
        if let Some(p) = usize::try_from(gone)
            .ok()
            .and_then(|i| self.parent.get_mut(i))
        {
            *p = keep;
        }
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
    use crate::geodesic::{Seed, march};
    use crate::isoline::lines;
    use glam::dvec2;

    /// A flat grid `n × n`, `pitch` apart, each node of the class `class`
    /// gives where it is; cut where the class changes at `edge` along the
    /// lattice edge, by halving on `class`.
    fn cut(n: u32, pitch: f64, class: impl Fn(DVec2) -> u32 + Sync) -> CutGrid {
        let count = usize::try_from(n * n).unwrap();
        let grid = TriGrid::new(DVec2::ZERO, pitch, n, n, vec![0.0; count]).unwrap();
        let classes: Vec<u32> = (0..grid.nodes())
            .map(|m| class(grid.xy(m).unwrap()))
            .collect();
        let g = grid.clone();
        CutGrid::new(
            grid,
            classes,
            |k| k < 10,
            |a, b| {
                let (mut lo, mut hi) = (g.xy(a).unwrap(), g.xy(b).unwrap());
                let ka = class(lo);
                for _ in 0..50 {
                    let mid = lo.midpoint(hi);
                    if class(mid) == ka {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                lo.extend(0.0)
            },
        )
    }

    /// A disc of one class in a field of another: two bands, the disc's
    /// triangulated right out to its circle, its edge one closed line round
    /// it, the band on its left, every cut on the circle.
    #[test]
    fn a_disc_is_cut_out_to_its_circle() {
        let centre = dvec2(5.0, 5.0);
        let g = cut(41, 0.25, |p| u32::from(p.distance(centre) > 3.1));
        assert_eq!(g.bands(), 2);
        let inner = g.band_of(g.grid().node(20, 20).unwrap());
        let view = g.view(Some(inner));
        let area: f64 = view
            .triangles()
            .map(|(_, [a, b, c])| {
                let [a, b, c] = [a, b, c].map(|n| view.xy(n).unwrap());
                (b - a).perp_dot(c - a) / 2.0
            })
            .sum();
        // The circle's polygon through cuts a pitch or less apart.
        let exact = std::f64::consts::PI * 3.1 * 3.1;
        assert!((area - exact).abs() < 0.01 * exact, "{area} vs {exact}");
        let edges = g.edges(inner);
        assert_eq!(edges.len(), 1);
        let (line, closed) = &edges[0];
        assert!(closed);
        let pts: Vec<DVec2> = line.iter().map(|&n| view.xy(n).unwrap()).collect();
        for p in &pts {
            assert!((p.distance(centre) - 3.1).abs() < 1e-9, "{p:?}");
        }
        let turning: f64 = pts
            .iter()
            .zip(pts.iter().cycle().skip(1))
            .map(|(a, b)| a.perp_dot(*b) / 2.0)
            .sum();
        assert!(turning > 0.0, "the disc on the edge's left");
        // The outer band's edge is the same cuts, the other way round.
        let outer = 1 - inner;
        let (out_line, _) = &g.edges(outer)[0];
        let mut a = line.clone();
        let mut b = out_line.clone();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b);
    }

    /// Measured out from the circle, the distance in the outer band at a
    /// node is its distance to the circle, to the first order of the march,
    /// and a line of the field half a pitch out is drawn right round.
    #[test]
    fn a_field_runs_from_the_cuts() {
        let centre = dvec2(5.0, 5.0);
        let g = cut(41, 0.25, |p| u32::from(p.distance(centre) > 3.1));
        let outer = g.band_of(g.grid().node(0, 0).unwrap());
        let view = g.view(Some(outer));
        let seeds: Vec<Seed> = (0..g.cuts().len())
            .map(|i| Seed {
                node: g.grid().nodes() + i as u32,
                distance: 0.0,
            })
            .collect();
        let d = march(&view, &seeds, |n| view.point(n)).d;
        let n = g.grid().node(36, 20).unwrap();
        let exact = view.xy(n).unwrap().distance(centre) - 3.1;
        assert!(
            (d[n as usize] - exact).abs() < 0.05,
            "{} vs {exact}",
            d[n as usize]
        );
        let tris: Vec<u32> = view.triangles().map(|(t, _)| t).collect();
        let found = lines(&view, &d, 0.125, tris);
        assert_eq!(found.len(), 1);
        assert!(found[0].closed);
        for p in &found[0].points {
            assert!((p.distance(centre) - 3.225).abs() < 0.03, "{p:?}");
        }
    }

    /// Locating a point finds the triangle of the band it is in, either side
    /// of a cut through a cell.
    #[test]
    fn a_point_is_found_in_its_bands_triangle() {
        let g = cut(11, 1.0, |p| u32::from(p.x + 0.3 * p.y > 4.62));
        let (left, right) = (g.band_of(0), g.band_of(10));
        assert_ne!(left, right);
        let p = dvec2(4.0, 1.5);
        let (l, r) = (g.view(Some(left)), g.view(Some(right)));
        assert!(l.locate(p).is_some());
        assert!(r.locate(p).is_none());
        assert!(r.locate(dvec2(4.4, 1.5)).is_some());
        assert!(g.view(None).locate(p).is_some());
    }
}
