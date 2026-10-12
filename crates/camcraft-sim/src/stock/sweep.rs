//! Sweeping a tool through the material: every move lowers the cells its
//! underside passes over.

use camcraft_geom::heightmap::{cell_index, narrow};
use camcraft_geom::mesh::distance_to_segment;
use camcraft_geom::steps::bounded_count;
use glam::{DVec2, DVec3, dvec2};
use rayon::prelude::*;

use super::{CellRect, MoveClass, SimMove, SimReport, Stock, Swept, SweptTool};

/// How far a move may climb and still be level, or wander and still be straight
/// up or down, in millimetres: both are then swept exactly.
const STRAIGHT: f64 = 1.0e-9;

/// How far an arc's chords may stray from it, as a fraction of a cell: an arc is
/// swept as its chords, which cut this much inside it at most.
const ARC_CHORD: f64 = 0.1;

/// Cells a sweep's footprint must span before its rows are cut in parallel.
const PARALLEL_CELLS: u64 = 100_000;

/// Where the row at `y` crosses the points within `r` of the segment `a`-`b`, as
/// `(lowest x, highest x)`. The region is convex, so its crossing is one span:
/// the widest of the span across either end's disc and across the band between.
fn capsule_row(a: DVec2, b: DVec2, r: f64, y: f64) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for end in [a, b] {
        let dy = y - end.y;
        if dy.abs() <= r {
            let w = (r * r - dy * dy).max(0.0).sqrt();
            lo = lo.min(end.x - w);
            hi = hi.max(end.x + w);
        }
    }
    let ab = b - a;
    let len = ab.length();
    if len > 0.0 {
        let u = ab / len;
        let n = dvec2(-u.y, u.x);
        // Along the row, x moves the point `coef` per unit across the band and
        // along the segment; each must stay in its range.
        let mut span = (f64::NEG_INFINITY, f64::INFINITY);
        for (coef, at, min, max) in [
            (n.x, (y - a.y) * n.y, -r, r),
            (u.x, (y - a.y) * u.y, 0.0, len),
        ] {
            if coef.abs() > 1.0e-12 {
                let (p, q) = ((min - at) / coef, (max - at) / coef);
                span = (span.0.max(p.min(q)), span.1.min(p.max(q)));
            } else if !(min..=max).contains(&at) {
                span = (f64::INFINITY, f64::NEG_INFINITY);
            }
        }
        if span.0 <= span.1 {
            lo = lo.min(a.x + span.0);
            hi = hi.max(a.x + span.1);
        }
    }
    (lo <= hi).then_some((lo, hi))
}

/// A tool's underside as it cuts: the tool, the height of its tip, and the floor
/// no cut goes below.
struct Underside<'t, T> {
    tool: &'t T,
    z: f64,
    floor: f64,
}

/// Lower the cells `i0..=i1` of row `iy`, `cells` being the whole row, to the
/// underside of a tool whose axis is `distance` from each cell's position, on a
/// grid from `origin` at `pitch`. Returns how deep it cut and the first and last
/// columns it lowered, or `None` when it lowered none.
///
/// Every sweep lowers cells this way, so a level move and a stamp at a point
/// agree cell for cell.
fn lower_row<T: SweptTool>(
    cells: &mut [f32],
    iy: u32,
    (i0, i1): (u32, u32),
    origin: DVec2,
    pitch: f64,
    under: &Underside<'_, T>,
    distance: impl Fn(DVec2) -> f64,
) -> Option<(f64, u32, u32)> {
    let mut cut: Option<(f64, u32, u32)> = None;
    for ix in i0..=i1 {
        let p = origin + dvec2(f64::from(ix) * pitch, f64::from(iy) * pitch);
        let Some(h) = under.tool.height_at(distance(p)) else {
            continue;
        };
        let depth = (under.z + h).max(under.floor);
        let Some(slot) = cells.get_mut(ix as usize) else {
            continue;
        };
        let cur = f64::from(*slot);
        if cur.is_finite() && depth < cur {
            *slot = narrow(depth);
            let (d, lo, _) = cut.unwrap_or((0.0, ix, ix));
            cut = Some((d.max(cur - depth), lo, ix));
        }
    }
    cut
}

/// Lower each row of `rect` in `data`, a field `nx` cells across, with `row`,
/// given the row's index and its cells, which says how deep it cut and the first
/// and last columns it lowered. Returns how deep the deepest cell was cut and
/// which cells were lowered.
///
/// Rows are independent, and each cell is worked out alone, so a sweep over many
/// cells (a long pass on a sheet, a large tool) is cut in parallel; a small one
/// is not worth handing out. Either way every cell comes out the same.
fn lower_rows(
    data: &mut [f32],
    nx: usize,
    rect: CellRect,
    row: impl Fn(u32, &mut [f32]) -> Option<(f64, u32, u32)> + Sync,
) -> (f64, Option<CellRect>) {
    let rows = data
        .get_mut(rect.y0 as usize * nx..(rect.y1 as usize + 1) * nx)
        .unwrap_or_default();
    let cells = u64::from(rect.width()) * u64::from(rect.height());
    let cuts: Vec<Option<(f64, u32, u32)>> = if cells >= PARALLEL_CELLS {
        rows.par_chunks_mut(nx)
            .enumerate()
            .map(|(i, cells)| row(rect.y0 + u32::try_from(i).unwrap_or(0), cells))
            .collect()
    } else {
        rows.chunks_mut(nx)
            .zip(rect.y0..=rect.y1)
            .map(|(cells, iy)| row(iy, cells))
            .collect()
    };
    let mut deepest = 0.0f64;
    let mut touched: Option<CellRect> = None;
    for (iy, cut) in (rect.y0..).zip(cuts) {
        if let Some((d, lo, hi)) = cut {
            deepest = deepest.max(d);
            let r = CellRect {
                x0: lo,
                y0: iy,
                x1: hi,
                y1: iy,
            };
            touched = CellRect::join(touched, Some(r));
        }
    }
    (deepest, touched)
}

impl Stock {
    /// Sweep a path through the block, removing material.
    ///
    /// Level moves, and moves straight up or down, are swept exactly. A move that
    /// climbs or descends as it goes is sampled at no more than half a cell, so
    /// consecutive stamps overlap heavily; its residual error is bounded by how
    /// much `z + h(d)` can change over half a cell, which is well under a cell for
    /// any real cutter.
    pub fn apply<T: SweptTool>(&mut self, moves: &[SimMove], tool: &T) -> SimReport {
        let before = self.volume();
        let swept = self.sweep(moves, tool);
        SimReport {
            removed_mm3: (before - self.volume()).max(0.0),
            rapid_contacts: swept.rapid_contacts,
            deepest_rapid_contact_mm: swept.deepest_rapid_contact_mm,
        }
    }

    /// Sweep a path through the block exactly as [`Stock::apply`] does, without
    /// measuring the volume removed, and say which cells it lowered.
    ///
    /// For whatever sweeps a few moves at a time, as playback does and as a run
    /// does while it links each operation over the material: two passes over the
    /// whole grid for the volume would cost more than the sweep.
    pub fn sweep<T: SweptTool>(&mut self, moves: &[SimMove], tool: &T) -> Swept {
        let mut report = Swept::default();
        let pitch = self.field.pitch();
        let step = (pitch * 0.5).max(1.0e-6);

        let mut at: Option<DVec3> = None;
        let mut first = true;
        for m in moves {
            let from = match at {
                Some(p) => p,
                None => {
                    at = Some(m.to);
                    continue;
                }
            };
            // An arc is swept as chords within a tenth of a cell of it; a
            // straight move is its one chord.
            let chords;
            let ends: &[DVec3] = match m.arc {
                None => std::slice::from_ref(&m.to),
                Some(arc) => {
                    chords = arc.chords(from, m.to, pitch * ARC_CHORD);
                    &chords
                }
            };
            let mut worst = 0.0f64;
            let mut touched = None;
            let mut a = from;
            for &b in ends {
                let (w, t) = self.stamp_segment(a, b, tool, step, first);
                worst = worst.max(w);
                touched = CellRect::join(touched, t);
                first = false;
                a = b;
            }
            report.touched = CellRect::join(report.touched, touched);
            if m.class == MoveClass::Rapid && worst > pitch * 0.01 {
                report.rapid_contacts += 1;
                report.deepest_rapid_contact_mm = report.deepest_rapid_contact_mm.max(worst);
            }
            at = Some(m.to);
        }
        report
    }

    /// Sweep one straight piece of a move, from `from` to `to`.
    fn stamp_segment<T: SweptTool>(
        &mut self,
        from: DVec3,
        to: DVec3,
        tool: &T,
        step: f64,
        first: bool,
    ) -> (f64, Option<CellRect>) {
        let rise = (to.z - from.z).abs();
        let run = (to - from).truncate().length();
        if rise <= STRAIGHT {
            // Level: exact, however long the move.
            self.stamp_level(from, to, tool)
        } else if run <= STRAIGHT {
            // Straight up or down: the tool is lowest at one end, and that end
            // cuts everything the rest of the move could.
            self.stamp(if from.z < to.z { from } else { to }, tool)
        } else {
            self.stamp_along(from, to, tool, step, first)
        }
    }

    /// Sweep a move that climbs or descends as it goes, sampled at no more than
    /// `step` along it.
    pub(super) fn stamp_along<T: SweptTool>(
        &mut self,
        from: DVec3,
        to: DVec3,
        tool: &T,
        step: f64,
        first: bool,
    ) -> (f64, Option<CellRect>) {
        let span = (to - from).length();
        // Bounded, so a pathological coordinate cannot turn one move into an
        // unbounded loop.
        let steps = bounded_count((span / step).ceil());
        let mut worst = 0.0f64;
        let mut touched = None;
        // The start was stamped as the end of the move before, unless this is the
        // first move swept; stamping lowers to a minimum, so doing it again would
        // change nothing, and leaving it out changes nothing either.
        for k in u32::from(!first)..=steps {
            let t = f64::from(k) / f64::from(steps);
            let (cut, cells) = self.stamp(from + (to - from) * t, tool);
            worst = worst.max(cut);
            touched = CellRect::join(touched, cells);
        }
        (worst, touched)
    }

    /// Sweep a level move exactly: every cell within the tool's reach of the
    /// move's line is lowered to the tool's underside at its distance from the
    /// line, since a tool's profile only rises away from its axis.
    fn stamp_level<T: SweptTool>(
        &mut self,
        from: DVec3,
        to: DVec3,
        tool: &T,
    ) -> (f64, Option<CellRect>) {
        let r = tool.max_radius();
        let Some(rect) = CellRect::join(self.footprint(from, r), self.footprint(to, r)) else {
            return (0.0, None);
        };
        let (a, b) = (from.truncate(), to.truncate());
        let under = Underside {
            tool,
            z: from.z.min(to.z),
            floor: self.floor,
        };
        let pitch = self.field.pitch();
        let origin = self.field.origin();
        let nx = self.field.nx() as usize;
        // One row: how deep it cut, and the first and last columns it lowered.
        let row = |iy: u32, cells: &mut [f32]| -> Option<(f64, u32, u32)> {
            let y = origin.y + f64::from(iy) * pitch;
            let (lo, hi) = capsule_row(a, b, r, y)?;
            // A cell either side of the span, so rounding cannot drop the rim; the
            // distance test below decides.
            let first = ((lo - origin.x) / pitch).floor() - 1.0;
            let last = ((hi - origin.x) / pitch).ceil() + 1.0;
            let (x0, x1) = (f64::from(rect.x0), f64::from(rect.x1));
            if !first.is_finite() || !last.is_finite() || last < x0 || first > x1 {
                return None;
            }
            // Clamped into the rectangle, so the narrowing is exact.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let (i0, i1) = (first.max(x0) as u32, last.min(x1) as u32);
            lower_row(cells, iy, (i0, i1), origin, pitch, &under, |p| {
                distance_to_segment(p, a, b)
            })
        };
        lower_rows(self.field.data_mut(), nx, rect, row)
    }

    /// The cells a tool of `radius` at `tip` could reach, clamped to the grid,
    /// inclusive: the rim's own cell is in it.
    pub fn footprint(&self, tip: DVec3, radius: f64) -> Option<CellRect> {
        if !tip.is_finite() || !radius.is_finite() || radius <= 0.0 {
            return None;
        }
        let pitch = self.field.pitch();
        let origin = self.field.origin();
        let (nx, ny) = (self.field.nx(), self.field.ny());
        Some(CellRect {
            x0: cell_index(((tip.x - radius - origin.x) / pitch).floor(), nx),
            x1: cell_index(((tip.x + radius - origin.x) / pitch).ceil(), nx),
            y0: cell_index(((tip.y - radius - origin.y) / pitch).floor(), ny),
            y1: cell_index(((tip.y + radius - origin.y) / pitch).ceil(), ny),
        })
    }

    /// Lower the field to the underside of a tool at `tip`, returning how far the
    /// deepest cell was cut and which cells were lowered.
    pub(super) fn stamp<T: SweptTool>(&mut self, tip: DVec3, tool: &T) -> (f64, Option<CellRect>) {
        let r = tool.max_radius();
        if !tip.is_finite() || !r.is_finite() || r <= 0.0 {
            return (0.0, None);
        }
        // The footprint's bounds are inclusive. An exclusive upper bound leaves
        // the cell exactly at the cutter's rim unstamped, which under-removes
        // material -- and under-removal is the dangerous direction, because it can
        // hide a gouge.
        let Some(rect) = self.footprint(tip, r) else {
            return (0.0, None);
        };

        let under = Underside {
            tool,
            z: tip.z,
            floor: self.floor,
        };
        let (origin, pitch, nx) = (self.field.origin(), self.field.pitch(), self.field.nx());
        let axis = tip.truncate();
        // A hair over the radius squared: a cell this rejects, the tool's own
        // profile would have too.
        let reach2 = r * r * (1.0 + 1.0e-9);
        // One row: how deep it cut, and the first and last columns it lowered.
        let row = |iy: u32, cells: &mut [f32]| -> Option<(f64, u32, u32)> {
            let py = origin.y + f64::from(iy) * pitch;
            let dy = py - tip.y;
            let across = reach2 - dy * dy;
            if across < 0.0 {
                return None;
            }
            // Only the columns the tool's circle spans on this row, and one more
            // each side for rounding; the profile decides.
            let w = across.sqrt();
            let first = (((tip.x - w - origin.x) / pitch).floor() - 1.0).max(f64::from(rect.x0));
            let last = (((tip.x + w - origin.x) / pitch).ceil() + 1.0).min(f64::from(rect.x1));
            if first.is_nan() || last.is_nan() || first > last {
                return None;
            }
            // Within the footprint, so the narrowing is exact.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let (first, last) = (first as u32, last as u32);
            lower_row(cells, iy, (first, last), origin, pitch, &under, |p| {
                (p - axis).length()
            })
        };
        lower_rows(self.field.data_mut(), nx as usize, rect, row)
    }
}
