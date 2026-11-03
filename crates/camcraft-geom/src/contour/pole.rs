//! The point deepest inside a region: furthest from every edge of it, its pole
//! of inaccessibility. The centre of the largest circle that fits.
//!
//! Found by searching square cells, best first, as Mapbox's polylabel does: a
//! cell whose centre is `d` inside can hold no point deeper than `d` plus half
//! its diagonal, so a cell that cannot beat the best found by more than the
//! precision asked for is never split. The distances are exact, to the edges
//! themselves, so nothing is offset and nothing rounded to ticks: the answer is
//! within the precision of the true deepest point's depth, whatever the shape.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use glam::DVec2;

use super::{Contour, Region};

/// The most cells searched, so a pathological shape asked for a tiny precision
/// cannot run away: the best found by then is given.
const MAX_CELLS: usize = 1 << 20;

/// A square cell: its centre, half its side, how far inside its centre is
/// (negative outside), and the most any point of it could be.
#[derive(Debug, Clone, Copy)]
struct Cell {
    centre: DVec2,
    half: f64,
    depth: f64,
    most: f64,
}

impl Cell {
    fn new(centre: DVec2, half: f64, edges: &Indexed) -> Self {
        let depth = edges.depth_at(centre);
        Self {
            centre,
            half,
            depth,
            most: depth + half * std::f64::consts::SQRT_2,
        }
    }
}

impl PartialEq for Cell {
    fn eq(&self, other: &Self) -> bool {
        self.most.total_cmp(&other.most) == Ordering::Equal
    }
}

impl Eq for Cell {}

impl PartialOrd for Cell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Cell {
    /// The cell that could hold the deepest point first.
    fn cmp(&self, other: &Self) -> Ordering {
        self.most.total_cmp(&other.most)
    }
}

impl Region {
    /// How far `p` is inside: the distance to the nearest edge of any ring,
    /// negative outside.
    pub fn depth_at(&self, p: DVec2) -> f64 {
        let nearest = self
            .rings
            .iter()
            .flat_map(Contour::edges)
            .map(|(a, b)| distance_to_segment(p, a, b))
            .fold(f64::INFINITY, f64::min);
        if self.contains(p) { nearest } else { -nearest }
    }

    /// The point deepest inside, and how deep it is: no shallower than the
    /// deepest point there is by more than `precision`. `None` for a region
    /// with no area, or a precision that is not a positive number.
    pub fn deepest_point(&self, precision: f64) -> Option<(DVec2, f64)> {
        if !(precision.is_finite() && precision > 0.0) {
            return None;
        }
        let (lo, hi) = self.bounds()?;
        let size = hi - lo;
        let side = size.x.min(size.y);
        // Not a number is no side either.
        if side.is_nan() || side <= 0.0 {
            return None;
        }
        let half = side / 2.0;
        let edges = Indexed::of(self);
        let mut queue = BinaryHeap::new();
        let mut y = lo.y;
        while y < hi.y {
            let mut x = lo.x;
            while x < hi.x {
                queue.push(Cell::new(DVec2::new(x + half, y + half), half, &edges));
                x += side;
            }
            y += side;
        }
        // A first guess: the middle of the bounds.
        let mut best = Cell::new((lo + hi) / 2.0, 0.0, &edges);
        let mut searched = queue.len();
        while let Some(cell) = queue.pop() {
            if cell.depth > best.depth {
                best = cell;
            }
            if cell.most - best.depth <= precision || searched >= MAX_CELLS {
                continue;
            }
            let h = cell.half / 2.0;
            for d in [
                DVec2::new(-h, -h),
                DVec2::new(h, -h),
                DVec2::new(-h, h),
                DVec2::new(h, h),
            ] {
                queue.push(Cell::new(cell.centre + d, h, &edges));
            }
            searched += 4;
        }
        (best.depth > 0.0).then_some((best.centre, best.depth))
    }
}

/// A region's edges, indexed for asking how far inside a point is: in square
/// cells, each holding the edges that cross it, so the nearest is found by
/// looking outwards from the point a ring of cells at a time; and in rows, each
/// holding the edges whose heights span it, so inside or out is counted from
/// those alone. The same answers as looking at every edge.
pub struct Indexed {
    edges: Vec<(DVec2, DVec2)>,
    lo: DVec2,
    cell: f64,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<u32>>,
    rows: Vec<Vec<u32>>,
}

impl Indexed {
    /// `region`'s edges, indexed.
    pub fn of(region: &Region) -> Self {
        let edges: Vec<(DVec2, DVec2)> = region.rings.iter().flat_map(Contour::edges).collect();
        let (lo, hi) = region.bounds().unwrap_or((DVec2::ZERO, DVec2::ONE));
        let size = (hi - lo).max(DVec2::splat(1e-9));
        // About two edges to a cell, and no more than 4096 cells across.
        #[allow(clippy::cast_precision_loss)]
        let n = edges.len().max(1) as f64;
        let cell = (size.x * size.y / n)
            .sqrt()
            .max(size.x.max(size.y) / 4096.0)
            * 1.5;
        let count = |l: f64| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let c = (l / cell).ceil().clamp(1.0, 4096.0) as usize;
            c
        };
        let (nx, ny) = (count(size.x), count(size.y));
        let mut cells = vec![Vec::new(); nx * ny];
        let mut rows = vec![Vec::new(); ny];
        let index = |v: f64, n: usize| {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let i = (v / cell).floor().clamp(0.0, (n - 1) as f64) as usize;
            i
        };
        for (k, &(a, b)) in edges.iter().enumerate() {
            let Ok(k) = u32::try_from(k) else {
                break;
            };
            let (x0, x1) = (
                index(a.x.min(b.x) - lo.x, nx),
                index(a.x.max(b.x) - lo.x, nx),
            );
            let (y0, y1) = (
                index(a.y.min(b.y) - lo.y, ny),
                index(a.y.max(b.y) - lo.y, ny),
            );
            for y in y0..=y1 {
                if let Some(row) = rows.get_mut(y) {
                    row.push(k);
                }
                for x in x0..=x1 {
                    if let Some(c) = cells.get_mut(y * nx + x) {
                        c.push(k);
                    }
                }
            }
        }
        Self {
            edges,
            lo,
            cell,
            nx,
            ny,
            cells,
            rows,
        }
    }

    /// How far `p` is inside, as [`Region::depth_at`] says.
    pub fn depth_at(&self, p: DVec2) -> f64 {
        let nearest = self.nearest(p);
        if self.contains(p) { nearest } else { -nearest }
    }

    /// Whether `p` is inside, by the even-odd rule, as [`Region::contains`]
    /// counts it: from the edges spanning its row, the only ones a line
    /// across from it can cross.
    pub fn contains(&self, p: DVec2) -> bool {
        let t = (p.y - self.lo.y) / self.cell;
        #[allow(clippy::cast_precision_loss)]
        if !(t >= 0.0 && t < self.ny as f64) {
            return false;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let Some(row) = self.rows.get(t.floor() as usize) else {
            return false;
        };
        let mut inside = false;
        for &k in row {
            let Some(&(a, b)) = self.edges.get(k as usize) else {
                continue;
            };
            if (a.y > p.y) != (b.y > p.y) {
                let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if p.x < x {
                    inside = !inside;
                }
            }
        }
        inside
    }

    /// How far `p` is from the nearest edge: the cells round it a ring at a
    /// time, until no edge in a further ring could be nearer.
    fn nearest(&self, p: DVec2) -> f64 {
        let (nx, ny) = (
            i64::try_from(self.nx).unwrap_or(i64::MAX),
            i64::try_from(self.ny).unwrap_or(i64::MAX),
        );
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        let at = |v: f64, n: i64| ((v / self.cell).floor() as i64).clamp(-1, n);
        let (cx, cy) = (at(p.x - self.lo.x, nx), at(p.y - self.lo.y, ny));
        let mut best = f64::INFINITY;
        for ring in 0..=nx.max(ny) + 1 {
            for y in (cy - ring)..=(cy + ring) {
                for x in (cx - ring)..=(cx + ring) {
                    let on_ring = (y - cy).abs() == ring || (x - cx).abs() == ring;
                    if !on_ring || x < 0 || y < 0 || x >= nx || y >= ny {
                        continue;
                    }
                    let Some(cell) = usize::try_from(y * nx + x)
                        .ok()
                        .and_then(|i| self.cells.get(i))
                    else {
                        continue;
                    };
                    for &k in cell {
                        if let Some(&(a, b)) = self.edges.get(k as usize) {
                            best = best.min(distance_to_segment(p, a, b));
                        }
                    }
                }
            }
            // Anything in a ring further out is at least this far away.
            #[allow(clippy::cast_precision_loss)]
            if best <= ring as f64 * self.cell {
                break;
            }
        }
        best
    }
}

/// How far `p` is from the segment `a`–`b`.
fn distance_to_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let length = ab.length_squared();
    let t = if length > 0.0 {
        ((p - a).dot(ab) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// A rectangle's deepest point is its middle, half its short side in.
    #[test]
    fn a_rectangle_is_deepest_in_the_middle() {
        let r = Region::rectangle(DVec2::new(0.0, 0.0), DVec2::new(40.0, 10.0));
        let (p, depth) = r.deepest_point(1e-3).unwrap();
        assert!((depth - 5.0).abs() < 1e-3, "{depth}");
        assert!((p.y - 5.0).abs() < 1e-2, "{p}");
        assert!((5.0..=35.0).contains(&p.x), "{p}");
    }

    /// An L: the deepest point is in its wider arm, not where the middle of its
    /// bounds or its points fall, which is outside it.
    #[test]
    fn an_l_is_deepest_in_its_wide_arm() {
        // Wide arm 0..30 x 0..20; narrow arm up to 0..6 x 20..60.
        let l = Contour::new(vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(30.0, 0.0),
            DVec2::new(30.0, 20.0),
            DVec2::new(6.0, 20.0),
            DVec2::new(6.0, 60.0),
            DVec2::new(0.0, 60.0),
        ])
        .unwrap();
        let r = Region::new(vec![l]);
        assert!(
            !r.contains(DVec2::new(15.0, 30.0)),
            "the bounds' middle is outside"
        );
        let (p, depth) = r.deepest_point(1e-3).unwrap();
        assert!((depth - 10.0).abs() < 1e-3, "{depth} at {p}");
        assert!(r.contains(p));
        assert!((p.y - 10.0).abs() < 1e-2, "{p}");
    }

    /// A square band round a square hole: deepest in a corner of the band, as
    /// far from the outer corner's two sides as from the hole's corner, and
    /// never in the hole.
    #[test]
    fn a_ring_is_deepest_across_its_band() {
        let outer = Region::rectangle(DVec2::new(-20.0, -20.0), DVec2::new(20.0, 20.0));
        let hole = Region::rectangle(DVec2::new(-12.0, -12.0), DVec2::new(12.0, 12.0));
        let band = outer.difference(&hole).unwrap();
        let (p, depth) = band.deepest_point(1e-3).unwrap();
        // On the diagonal at c: 20 - c = √2 (c - 12).
        let s = std::f64::consts::SQRT_2;
        let c = (20.0 + 12.0 * s) / (1.0 + s);
        assert!((depth - (20.0 - c)).abs() < 1e-3, "{depth} at {p}");
        assert!(
            (p.x.abs() - c).abs() < 1e-2 && (p.y.abs() - c).abs() < 1e-2,
            "{p}"
        );
        assert!(!hole.contains(p));
        assert!(band.depth_at(DVec2::ZERO) < 0.0, "the hole is outside");
    }

    /// Indexed, a region's depths are what every edge says, inside and out,
    /// near and far from it.
    #[test]
    fn the_index_answers_as_every_edge_does() {
        let outer = Region::rectangle(DVec2::new(-20.0, -20.0), DVec2::new(20.0, 20.0));
        let hole = Region::rectangle(DVec2::new(-12.0, -3.0), DVec2::new(9.0, 12.0));
        let band = outer.difference(&hole).unwrap().offset(-0.7, 0.01);
        let edges = Indexed::of(&band);
        for i in 0..400 {
            let t = f64::from(i);
            let p = DVec2::new((t * 0.37).sin() * 30.0, (t * 0.61).cos() * 30.0);
            let (want, got) = (band.depth_at(p), edges.depth_at(p));
            assert!((want - got).abs() < 1e-9, "{p}: {want} against {got}");
        }
    }

    /// Nothing to be inside, or no precision to find it to: none.
    #[test]
    fn no_area_has_no_deepest_point() {
        assert!(Region::default().deepest_point(1e-3).is_none());
        let r = Region::rectangle(DVec2::ZERO, DVec2::ONE);
        assert!(r.deepest_point(0.0).is_none());
        assert!(r.deepest_point(f64::NAN).is_none());
    }
}
