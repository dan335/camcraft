//! Keeping watch over what must not touch: a body that rides with the tool,
//! as a tool's shank and holder do, swept through the material without
//! cutting it, and every move where material stands within its reach told.
//!
//! The body is described as a cutter is ([`SweptTool`]): an underside, the
//! lowest it is at each distance from the axis. Each move is looked at before
//! it cuts, so what the flutes would take away along it still stands for the
//! shank to meet, as it does: flutes only cut as high as they reach.
//!
//! A body's underside only rises away from its axis, as any tool's does: a
//! piece of it reaches no further out lower down than a piece above it. So a
//! block of cells is no nearer the body than its highest cell is above the
//! underside at the block's nearest distance, and the field's maximum is kept
//! as a pyramid, each level the maximum of four below, lowered as the tool
//! cuts: a wide holder above the material is passed over in a few blocks, and
//! only where something could reach it are its cells looked at.

use camcraft_geom::mesh::distance_to_segment;
use glam::{DVec2, DVec3, dvec2};

use super::{CellRect, MoveClass, SimMove, Stock, Swept, SweptTool};

/// How far material must stand into the body to count, in millimetres:
/// clear of rounding.
const TOUCHING: f64 = 1.0e-6;

/// One move where material stood within the body's reach.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Collision {
    /// Which of the moves swept, by its place among them.
    pub at: usize,
    /// How far the material stood into the body, in millimetres: how much
    /// higher the body would have had to be.
    pub depth: f64,
    /// How far from the axis the material that stood deepest was, in
    /// millimetres: which part of the body met it.
    pub distance: f64,
}

/// The field's maxima: level 0 the cells, each level above the maximum of
/// the two by two below it.
struct Pyramid {
    levels: Vec<(u32, u32, Vec<f32>)>,
}

impl Pyramid {
    fn new(stock: &Stock) -> Self {
        let (nx, ny) = (stock.field.nx(), stock.field.ny());
        let cells: Vec<f32> = stock
            .field
            .data()
            .iter()
            .map(|v| if v.is_finite() { *v } else { f32::NEG_INFINITY })
            .collect();
        let mut levels = vec![(nx, ny, cells)];
        while let Some((w, h, _)) = levels.last()
            && (*w > 1 || *h > 1)
        {
            let (w, h) = (*w, *h);
            let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
            levels.push((cw, ch, vec![f32::NEG_INFINITY; (cw * ch) as usize]));
            let k = levels.len() - 1;
            Self::rebuild(&mut levels, k, 0, 0, cw - 1, ch - 1);
        }
        Self { levels }
    }

    /// Work out level `k`'s nodes `x0..=x1`, `y0..=y1` from the level below.
    fn rebuild(levels: &mut [(u32, u32, Vec<f32>)], k: usize, x0: u32, y0: u32, x1: u32, y1: u32) {
        let Some((below, here)) = k.checked_sub(1).map(|b| levels.split_at_mut(b + 1)) else {
            return;
        };
        let (Some((bw, bh, bv)), Some((w, _, v))) = (below.last(), here.first_mut()) else {
            return;
        };
        for y in y0..=y1 {
            for x in x0..=x1 {
                let mut top = f32::NEG_INFINITY;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (bx, by) = (2 * x + dx, 2 * y + dy);
                    if bx < *bw && by < *bh {
                        top = top.max(bv.get((by * bw + bx) as usize).copied().unwrap_or(top));
                    }
                }
                if let Some(slot) = v.get_mut((y * *w + x) as usize) {
                    *slot = top;
                }
            }
        }
    }

    /// Take in the cells `rect` of `stock`, lowered, and the levels above.
    fn refresh(&mut self, stock: &Stock, rect: CellRect) {
        let data = stock.field.data();
        if let Some((nx, _, cells)) = self.levels.first_mut() {
            for iy in rect.y0..=rect.y1 {
                for ix in rect.x0..=rect.x1 {
                    let i = (iy * *nx + ix) as usize;
                    if let (Some(slot), Some(v)) = (cells.get_mut(i), data.get(i)) {
                        *slot = if v.is_finite() { *v } else { f32::NEG_INFINITY };
                    }
                }
            }
        }
        let (mut x0, mut y0, mut x1, mut y1) = (rect.x0, rect.y0, rect.x1, rect.y1);
        for k in 1..self.levels.len() {
            (x0, y0, x1, y1) = (x0 / 2, y0 / 2, x1 / 2, y1 / 2);
            Self::rebuild(&mut self.levels, k, x0, y0, x1, y1);
        }
    }
}

/// The body watched for, and the field's maxima to look at the material
/// quickly.
pub struct Guard<'b, B> {
    body: &'b B,
    pyramid: Pyramid,
    /// How far a sloping move may drop before it is looked at again: each
    /// piece of it is taken at its lowest, so the body is checked no more
    /// than this lower than it is.
    drop: f64,
}

impl<'b, B: SweptTool> Guard<'b, B> {
    /// Watch for `body` in `stock` as it stands, a sloping move looked at in
    /// pieces that drop no more than `drop`.
    pub fn new(stock: &Stock, body: &'b B, drop: f64) -> Self {
        Self {
            body,
            pyramid: Pyramid::new(stock),
            drop: drop.max(1.0e-3),
        }
    }

    /// How far the tip at `at` must rise for the body to clear `stock`, the
    /// material the guard was made over or kept to: nothing where it is
    /// clear already.
    pub fn rise_at(&self, stock: &Stock, at: DVec3) -> f64 {
        let p = at.truncate();
        self.rise_along(stock, p, p, at.z)
    }

    /// How far the tip, at `z` all along the segment `a`-`b`, must rise for
    /// the body to clear `stock` anywhere along it: nothing where it is clear
    /// already.
    pub fn rise_along(&self, stock: &Stock, a: DVec2, b: DVec2, z: f64) -> f64 {
        self.intrusion(stock, a, b, z)
            .map_or(0.0, |(depth, _)| depth.max(0.0))
    }

    /// How far the material stands into the body, its axis on the segment
    /// `a`-`b` with its tip at `z`, and how far from the axis the deepest
    /// was: `None` where nothing does.
    fn intrusion(&self, stock: &Stock, a: DVec2, b: DVec2, z: f64) -> Option<(f64, f64)> {
        let reach = self.body.max_radius();
        let (origin, pitch) = (stock.field.origin(), stock.field.pitch());
        let top = self.pyramid.levels.len().checked_sub(1)?;
        let mut best: Option<(f64, f64)> = None;
        // Nodes to look at: a level and a place in it.
        let mut stack = vec![(top, 0u32, 0u32)];
        while let Some((k, x, y)) = stack.pop() {
            let Some((w, h, v)) = self.pyramid.levels.get(k) else {
                continue;
            };
            if x >= *w || y >= *h {
                continue;
            }
            let Some(&max) = v.get((y * *w + x) as usize) else {
                continue;
            };
            if !max.is_finite() {
                continue;
            }
            // The node's cells, as a box in the plane.
            let span = f64::from(1u32 << k);
            let lo = origin + dvec2(f64::from(x) * span * pitch, f64::from(y) * span * pitch);
            let hi = lo + dvec2((span - 1.0) * pitch, (span - 1.0) * pitch);
            let nearest = box_to_segment(lo, hi, a, b);
            if nearest > reach {
                continue;
            }
            let Some(under) = self.body.height_at(nearest) else {
                continue;
            };
            let bound = f64::from(max) - (z + under);
            if bound <= best.map_or(TOUCHING, |(d, _)| d.max(TOUCHING)) {
                continue;
            }
            if k == 0 {
                // A cell: as it is, at its own distance.
                best = Some((bound, nearest));
                continue;
            }
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                stack.push((k - 1, 2 * x + dx, 2 * y + dy));
            }
        }
        best
    }
}

/// The least distance from the box `lo`-`hi` to the segment `a`-`b`: zero
/// where they meet.
fn box_to_segment(lo: DVec2, hi: DVec2, a: DVec2, b: DVec2) -> f64 {
    let clamp = |p: DVec2| p.clamp(lo, hi);
    // Of the segment's points, the nearest the box is at an end, at a corner's
    // foot on it, or where it crosses the box; sampling those bounds it below
    // well enough: the box's distance to the nearest of them, less nothing.
    let mut best = (clamp(a) - a).length().min((clamp(b) - b).length());
    for corner in [lo, hi, dvec2(lo.x, hi.y), dvec2(hi.x, lo.y)] {
        best = best.min(distance_to_segment(corner, a, b));
    }
    // Crossing: the segment enters the box.
    let ab = b - a;
    for (axis, edge) in [(0, lo.x), (0, hi.x), (1, lo.y), (1, hi.y)] {
        let (pa, pd) = if axis == 0 { (a.x, ab.x) } else { (a.y, ab.y) };
        if pd.abs() > 1e-12 {
            let t = (edge - pa) / pd;
            if (0.0..=1.0).contains(&t) {
                let p = a + ab * t;
                best = best.min((clamp(p) - p).length());
            }
        }
    }
    best
}

impl Stock {
    /// Sweep `moves` through the material with `tool`, as [`Stock::sweep`]
    /// does, watching `guard`'s body ride with it: every move where material
    /// stood within the body's reach is told, with how far into it.
    pub fn sweep_guarded<T: SweptTool, B: SweptTool>(
        &mut self,
        moves: &[SimMove],
        tool: &T,
        guard: &mut Guard<'_, B>,
    ) -> (Swept, Vec<Collision>) {
        let mut swept = Swept::default();
        let mut collisions = Vec::new();
        let mut at: Option<DVec3> = None;
        for (i, m) in moves.iter().enumerate() {
            let Some(from) = at else {
                at = Some(m.to);
                continue;
            };
            let worst = self.guarded_move(from, m, guard);
            // The move cut as the plain sweep cuts it, from where it starts.
            let start = SimMove {
                to: from,
                class: MoveClass::Rapid,
                arc: None,
            };
            let one = self.sweep(&[start, *m], tool);
            swept.rapid_contacts += one.rapid_contacts;
            swept.deepest_rapid_contact_mm = swept
                .deepest_rapid_contact_mm
                .max(one.deepest_rapid_contact_mm);
            swept.touched = CellRect::join(swept.touched, one.touched);
            if let Some(t) = one.touched {
                guard.pyramid.refresh(self, t);
            }
            if let Some((depth, distance)) = worst {
                collisions.push(Collision {
                    at: i,
                    depth,
                    distance,
                });
            }
            at = Some(m.to);
        }
        (swept, collisions)
    }

    /// The deepest the material stands into the guard's body along a move
    /// from `from`, before the move cuts anything: each straight piece of it
    /// taken at its lowest, a sloping one in pieces that drop no more than
    /// the guard allows.
    fn guarded_move<B: SweptTool>(
        &self,
        from: DVec3,
        m: &SimMove,
        guard: &Guard<'_, B>,
    ) -> Option<(f64, f64)> {
        let deeper = |a: Option<(f64, f64)>, b: Option<(f64, f64)>| match (a, b) {
            (Some(x), Some(y)) => Some(if y.0 > x.0 { y } else { x }),
            (x, y) => x.or(y),
        };
        let pitch = self.field.pitch();
        let chords;
        let ends: &[DVec3] = match m.arc {
            None => std::slice::from_ref(&m.to),
            Some(arc) => {
                chords = arc.chords(from, m.to, pitch * 0.1);
                &chords
            }
        };
        let mut worst = None;
        let mut a = from;
        for &b in ends {
            // Straight up or down, the body is lowest at one end, and that end
            // meets all the rest of it could.
            let straight_down = (b - a).truncate().length() <= 1.0e-9;
            let pieces = if straight_down {
                1
            } else {
                camcraft_geom::steps::bounded_count(((b.z - a.z).abs() / guard.drop).ceil())
            };
            for k in 0..pieces.max(1) {
                let t0 = f64::from(k) / f64::from(pieces.max(1));
                let t1 = f64::from(k + 1) / f64::from(pieces.max(1));
                let (p, q) = (a + (b - a) * t0, a + (b - a) * t1);
                worst = deeper(
                    worst,
                    guard.intrusion(self, p.truncate(), q.truncate(), p.z.min(q.z)),
                );
            }
            a = b;
        }
        worst
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use glam::{dvec2, dvec3};

    /// A flat cutter of radius `0`.
    struct Flat(f64);
    impl SweptTool for Flat {
        fn max_radius(&self) -> f64 {
            self.0
        }
        fn height_at(&self, d: f64) -> Option<f64> {
            (d <= self.0).then_some(0.0)
        }
    }

    /// A shank `radius` across from `start` above the tip, and a holder
    /// `wide` across from `holder` above it.
    struct Body {
        radius: f64,
        start: f64,
        wide: f64,
        holder: f64,
    }
    impl SweptTool for Body {
        fn max_radius(&self) -> f64 {
            self.wide
        }
        fn height_at(&self, d: f64) -> Option<f64> {
            if d <= self.radius {
                Some(self.start)
            } else {
                (d <= self.wide).then_some(self.holder)
            }
        }
    }

    fn rapid(to: DVec3) -> SimMove {
        SimMove {
            to,
            class: MoveClass::Rapid,
            arc: None,
        }
    }

    fn cut(to: DVec3) -> SimMove {
        SimMove {
            to,
            class: MoveClass::MayCut,
            arc: None,
        }
    }

    /// A slot cut deeper than the flutes reach meets the shank in what stands
    /// above them; one within their reach meets nothing; and a holder low
    /// enough meets the top of the block beside the slot.
    #[test]
    fn what_stands_above_the_flutes_meets_the_body() {
        // A block 20 mm tall, its top at 0.
        let fresh =
            || Stock::block(dvec2(-30.0, -30.0), dvec2(30.0, 30.0), 0.0, -20.0, 0.25).unwrap();
        let tool = Flat(3.0);
        let body = Body {
            radius: 3.0,
            start: 10.0,
            wide: 15.0,
            holder: 30.0,
        };
        let slot = |depth: f64| {
            [
                rapid(dvec3(-25.0, 0.0, 5.0)),
                cut(dvec3(-25.0, 0.0, -depth)),
                cut(dvec3(25.0, 0.0, -depth)),
                rapid(dvec3(25.0, 0.0, 5.0)),
            ]
        };

        let mut stock = fresh();
        let mut guard = Guard::new(&stock, &body, 0.05);
        let (_, collisions) = stock.sweep_guarded(&slot(6.0), &tool, &mut guard);
        assert!(collisions.is_empty(), "within the flutes: {collisions:?}");

        let mut stock = fresh();
        let mut guard = Guard::new(&stock, &body, 0.05);
        let (_, collisions) = stock.sweep_guarded(&slot(14.0), &tool, &mut guard);
        // Into the block, and along it: the shank 4 mm into what the flutes
        // leave standing above them.
        assert_eq!(collisions.iter().map(|c| c.at).collect::<Vec<_>>(), [1, 2]);
        assert!((collisions[1].depth - 4.0).abs() < 1e-6, "{collisions:?}");
        assert!(collisions[1].distance <= 3.0);

        // Deep enough for the holder to reach the top beside the slot.
        let mut stock = fresh();
        let mut guard = Guard::new(&stock, &body, 0.05);
        let (_, collisions) = stock.sweep_guarded(&slot(19.0), &tool, &mut guard);
        let along = collisions.iter().find(|c| c.at == 2).unwrap();
        assert!(along.depth >= 9.0 - 1e-6, "{along:?}");
    }

    /// The sweep cuts what the plain sweep cuts, cell for cell.
    #[test]
    fn a_guarded_sweep_cuts_as_the_plain_one() {
        let moves = [
            rapid(dvec3(-20.0, -5.0, 5.0)),
            cut(dvec3(-20.0, -5.0, -3.0)),
            cut(dvec3(20.0, 5.0, -4.0)),
            cut(dvec3(0.0, 15.0, -2.0)),
            rapid(dvec3(0.0, 15.0, 5.0)),
        ];
        let block =
            || Stock::block(dvec2(-30.0, -30.0), dvec2(30.0, 30.0), 0.0, -20.0, 0.5).unwrap();
        let mut plain = block();
        plain.sweep(&moves, &Flat(3.0));
        let mut guarded = block();
        let body = Body {
            radius: 3.0,
            start: 10.0,
            wide: 15.0,
            holder: 30.0,
        };
        let mut guard = Guard::new(&guarded, &body, 0.05);
        guarded.sweep_guarded(&moves, &Flat(3.0), &mut guard);
        assert_eq!(plain.field.data(), guarded.field.data());
    }
}
