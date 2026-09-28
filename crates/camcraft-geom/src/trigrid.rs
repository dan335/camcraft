//! A surface sampled on a regular XY lattice and split into triangles: the lifted
//! grid that fields are measured along ([`crate::geodesic`]) and lines drawn across
//! ([`crate::isoline`]).
//!
//! # Nodes, cells and triangles
//!
//! Node `n` is `iy · nx + ix`, at `origin + (ix, iy) · pitch`, lifted to its own
//! height. A node is *defined* when it is active and has a finite height. Cell
//! `(cx, cy)` has the nodes `(cx, cy)` to `(cx + 1, cy + 1)` at its corners and is
//! split into triangles `2c` and `2c + 1`, `c = cy · (nx − 1) + cx`, each wound
//! anticlockwise seen from above. A triangle exists when all three of its nodes are
//! defined.
//!
//! # The diagonal
//!
//! Each cell is split on the diagonal that keeps the largest 3D angle of its two
//! triangles smallest. A front crossing a triangle is then updated from inside it,
//! and fast marching stays exact on a plane. A cell with one corner undefined is
//! split on the diagonal that leaves one whole triangle clear of it.

use glam::{DVec2, DVec3, dvec2, dvec3};

/// Which way a cell is split, seen from above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Diagonal {
    /// From the cell's lower-left corner to its upper-right.
    Rising,
    /// From its lower-right corner to its upper-left.
    Falling,
}

/// A lifted grid, split into triangles.
#[derive(Debug, Clone)]
pub struct TriGrid {
    origin: DVec2,
    pitch: f64,
    nx: u32,
    ny: u32,
    /// Each node's height; NaN where it is not defined, active or not.
    z: Vec<f32>,
    diagonal: Vec<Diagonal>,
}

impl TriGrid {
    /// The grid of `nx × ny` nodes from `origin`, `pitch` apart, with the height
    /// `z` of each (NaN where there is none) and whether it takes part.
    ///
    /// `None` unless there are at least two nodes each way, the pitch is finite and
    /// positive, and `z` and `active` have a value for every node.
    pub fn new(
        origin: DVec2,
        pitch: f64,
        nx: u32,
        ny: u32,
        mut z: Vec<f32>,
        active: Vec<bool>,
    ) -> Option<Self> {
        let nodes = usize::try_from(u64::from(nx) * u64::from(ny)).ok()?;
        if nx < 2
            || ny < 2
            || !(pitch.is_finite() && pitch > 0.0)
            || !origin.is_finite()
            || z.len() != nodes
            || active.len() != nodes
            || u32::try_from(nodes).is_err()
        {
            return None;
        }
        // A node not taking part has no height, so one test says both.
        for (z, _) in z.iter_mut().zip(&active).filter(|(_, a)| !**a) {
            *z = f32::NAN;
        }
        let mut grid = Self {
            origin,
            pitch,
            nx,
            ny,
            z,
            diagonal: Vec::new(),
        };
        let cells = (nx - 1) * (ny - 1);
        grid.diagonal = (0..cells).map(|c| grid.choose_diagonal(c)).collect();
        Some(grid)
    }

    /// Nodes across, in X.
    pub fn nx(&self) -> u32 {
        self.nx
    }

    /// Nodes up, in Y.
    pub fn ny(&self) -> u32 {
        self.ny
    }

    /// Distance between neighbouring nodes, in millimetres.
    pub fn pitch(&self) -> f64 {
        self.pitch
    }

    /// Where node `(0, 0)` is.
    pub fn origin(&self) -> DVec2 {
        self.origin
    }

    /// How many nodes there are.
    pub fn nodes(&self) -> u32 {
        self.nx * self.ny
    }

    /// How many triangles there could be, defined or not: triangle ids run below it.
    pub fn triangle_slots(&self) -> u32 {
        2 * (self.nx - 1) * (self.ny - 1)
    }

    /// Node `(ix, iy)`, if it is on the grid.
    pub fn node(&self, ix: u32, iy: u32) -> Option<u32> {
        (ix < self.nx && iy < self.ny).then(|| iy * self.nx + ix)
    }

    /// The column and row of node `n`.
    pub fn column_row(&self, n: u32) -> (u32, u32) {
        (n % self.nx, n / self.nx)
    }

    /// Where node `n` is, seen from above.
    pub fn xy(&self, n: u32) -> Option<DVec2> {
        (n < self.nodes()).then(|| {
            let (ix, iy) = self.column_row(n);
            self.origin + dvec2(f64::from(ix), f64::from(iy)) * self.pitch
        })
    }

    /// Node `n`'s height, if it is defined.
    pub fn z(&self, n: u32) -> Option<f64> {
        let z = f64::from(*self.z.get(usize::try_from(n).ok()?)?);
        z.is_finite().then_some(z)
    }

    /// Whether node `n` is defined: active, with a height.
    pub fn defined(&self, n: u32) -> bool {
        self.z(n).is_some()
    }

    /// Node `n`, lifted to its height, if it is defined.
    pub fn point(&self, n: u32) -> Option<DVec3> {
        let p = self.xy(n)?;
        self.z(n).map(|z| dvec3(p.x, p.y, z))
    }

    /// Triangle `t`'s nodes, anticlockwise seen from above, if all three are
    /// defined.
    pub fn triangle(&self, t: u32) -> Option<[u32; 3]> {
        let nodes = self.triangle_nodes(t)?;
        nodes.iter().all(|&n| self.defined(n)).then_some(nodes)
    }

    /// Every triangle that exists, in id order, with its nodes.
    pub fn triangles(&self) -> impl Iterator<Item = (u32, [u32; 3])> + '_ {
        (0..self.triangle_slots()).filter_map(|t| self.triangle(t).map(|nodes| (t, nodes)))
    }

    /// The triangles that exist with node `n` a corner of them, with their nodes:
    /// at most six.
    pub fn triangles_at(&self, n: u32) -> impl Iterator<Item = (u32, [u32; 3])> + '_ {
        let (ix, iy) = self.column_row(n);
        let cells = [(0, 0), (1, 0), (0, 1), (1, 1)]
            .into_iter()
            .filter_map(move |(dx, dy)| {
                let cx = ix.checked_sub(dx)?;
                let cy = iy.checked_sub(dy)?;
                (cx + 1 < self.nx && cy + 1 < self.ny).then(|| cy * (self.nx - 1) + cx)
            });
        cells.flat_map(|c| [2 * c, 2 * c + 1]).filter_map(move |t| {
            self.triangle(t)
                .filter(|nodes| nodes.contains(&n))
                .map(|nodes| (t, nodes))
        })
    }

    /// The nodes joined to `n` by an edge of a triangle that exists.
    pub fn neighbours(&self, n: u32) -> Vec<u32> {
        let mut out: Vec<u32> = self
            .triangles_at(n)
            .flat_map(|(_, nodes)| nodes)
            .filter(|&m| m != n)
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The triangle that exists over `p`, if any.
    pub fn locate(&self, p: DVec2) -> Option<u32> {
        let local = (p - self.origin) / self.pitch;
        let far = dvec2(f64::from(self.nx - 1), f64::from(self.ny - 1));
        if !(local.cmpge(DVec2::ZERO).all() && local.cmple(far).all()) {
            return None;
        }
        let (fx, fy) = (local.x.floor(), local.y.floor());
        // In range, so the narrowing is exact.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (cx, cy) = (
            (fx as u32).min(self.nx.saturating_sub(2)),
            (fy as u32).min(self.ny.saturating_sub(2)),
        );
        let (u, v) = (local.x - f64::from(cx), local.y - f64::from(cy));
        let c = cy * (self.nx - 1) + cx;
        let second = match self.diagonal.get(usize::try_from(c).ok()?)? {
            Diagonal::Rising => v > u,
            Diagonal::Falling => u + v > 1.0,
        };
        let t = 2 * c + u32::from(second);
        self.triangle(t).map(|_| t)
    }

    /// How steep triangle `t` is, lifted: the angle of its plane from level, in
    /// radians.
    pub fn slope(&self, t: u32) -> Option<f64> {
        let [a, b, c] = self.triangle(t)?.map(|n| self.point(n));
        let n = (b? - a?).cross(c? - a?);
        let len = n.length();
        (len > 0.0).then(|| (n.z.abs() / len).clamp(0.0, 1.0).acos())
    }

    /// Triangle `t`'s nodes, whether they exist or not.
    fn triangle_nodes(&self, t: u32) -> Option<[u32; 3]> {
        if t >= self.triangle_slots() {
            return None;
        }
        let c = t / 2;
        let (cx, cy) = (c % (self.nx - 1), c / (self.nx - 1));
        let a = cy * self.nx + cx;
        let (b, cc, d) = (a + 1, a + self.nx + 1, a + self.nx);
        let second = t % 2 == 1;
        Some(
            match (self.diagonal.get(usize::try_from(c).ok()?)?, second) {
                (Diagonal::Rising, false) => [a, b, cc],
                (Diagonal::Rising, true) => [a, cc, d],
                (Diagonal::Falling, false) => [a, b, d],
                (Diagonal::Falling, true) => [b, cc, d],
            },
        )
    }

    /// The diagonal for cell `c`: the one leaving whole triangles clear of an
    /// undefined corner, else the one whose largest angle is smallest.
    fn choose_diagonal(&self, c: u32) -> Diagonal {
        let (cx, cy) = (c % (self.nx - 1), c / (self.nx - 1));
        let a = cy * self.nx + cx;
        let corners = [a, a + 1, a + self.nx + 1, a + self.nx].map(|n| self.point(n));
        let [pa, pb, pc, pd] = corners;
        match (pa, pb, pc, pd) {
            (Some(pa), Some(pb), Some(pc), Some(pd)) => {
                // The smallest cosine of either triangle is its largest angle.
                let rising = least_cosine(pa, pb, pc).min(least_cosine(pa, pc, pd));
                let falling = least_cosine(pa, pb, pd).min(least_cosine(pb, pc, pd));
                if falling > rising + 1e-12 {
                    Diagonal::Falling
                } else {
                    Diagonal::Rising
                }
            }
            // With the lower-left or upper-right corner missing, the falling
            // diagonal keeps a triangle clear of it; with either other, the rising.
            (None, ..) | (_, _, None, _) => Diagonal::Falling,
            _ => Diagonal::Rising,
        }
    }
}

/// The cosine of a triangle's largest angle.
fn least_cosine(a: DVec3, b: DVec3, c: DVec3) -> f64 {
    let cosine = |p: DVec3, q: DVec3, r: DVec3| {
        let (u, v) = (q - p, r - p);
        let len = u.length() * v.length();
        if len > 0.0 { u.dot(v) / len } else { -1.0 }
    };
    cosine(a, b, c).min(cosine(b, c, a)).min(cosine(c, a, b))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn flat(nx: u32, ny: u32) -> TriGrid {
        let n = usize::try_from(nx * ny).unwrap();
        TriGrid::new(DVec2::ZERO, 1.0, nx, ny, vec![0.0; n], vec![true; n]).unwrap()
    }

    #[test]
    fn refuses_a_grid_with_no_cells() {
        assert!(TriGrid::new(DVec2::ZERO, 1.0, 1, 3, vec![0.0; 3], vec![true; 3]).is_none());
        assert!(TriGrid::new(DVec2::ZERO, 0.0, 2, 2, vec![0.0; 4], vec![true; 4]).is_none());
        assert!(TriGrid::new(DVec2::ZERO, 1.0, 2, 2, vec![0.0; 3], vec![true; 4]).is_none());
    }

    #[test]
    fn triangles_wind_anticlockwise_and_cover_each_cell() {
        let g = flat(4, 3);
        let mut area = 0.0;
        for (t, nodes) in g.triangles() {
            let [a, b, c] = nodes.map(|n| g.xy(n).unwrap());
            let signed = (b - a).perp_dot(c - a) / 2.0;
            assert!(signed > 0.0, "triangle {t} winds clockwise");
            area += signed;
        }
        assert!((area - 6.0).abs() < 1e-12);
    }

    #[test]
    fn an_undefined_corner_leaves_one_triangle() {
        let mut z = vec![0.0f32; 4];
        z[0] = f32::NAN;
        let g = TriGrid::new(DVec2::ZERO, 1.0, 2, 2, z, vec![true; 4]).unwrap();
        let tris: Vec<(u32, [u32; 3])> = g.triangles().collect();
        assert_eq!(tris.len(), 1);
        assert!(!tris[0].1.contains(&0));
    }

    #[test]
    fn locate_finds_the_triangle_under_a_point() {
        let g = flat(5, 5);
        for p in [
            dvec2(0.2, 0.1),
            dvec2(0.1, 0.9),
            dvec2(3.5, 2.25),
            dvec2(4.0, 4.0),
        ] {
            let t = g.locate(p).unwrap();
            let [a, b, c] = g.triangle(t).unwrap().map(|n| g.xy(n).unwrap());
            let inside = |u: DVec2, v: DVec2| (v - u).perp_dot(p - u) >= -1e-12;
            assert!(
                inside(a, b) && inside(b, c) && inside(c, a),
                "{p:?} not in {t}"
            );
        }
        assert!(g.locate(dvec2(-0.1, 1.0)).is_none());
        assert!(g.locate(dvec2(4.5, 1.0)).is_none());
    }

    #[test]
    fn a_node_has_its_six_neighbours() {
        let g = flat(3, 3);
        let middle = g.node(1, 1).unwrap();
        assert_eq!(g.triangles_at(middle).count(), 6);
        assert_eq!(g.neighbours(middle).len(), 6);
    }

    #[test]
    fn slope_is_the_lifted_planes() {
        let n = 9;
        let z: Vec<f32> = (0..n).map(|i| (i % 3) as f32).collect();
        let g = TriGrid::new(DVec2::ZERO, 1.0, 3, 3, z, vec![true; 9]).unwrap();
        for (t, _) in g.triangles() {
            assert!((g.slope(t).unwrap() - std::f64::consts::FRAC_PI_4).abs() < 1e-6);
        }
    }
}
