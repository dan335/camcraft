//! The sheet as a grid of square cells, each part's outline as the cells it
//! covers, and which cells are taken.
//!
//! Cells are counted from the corner packing starts at: a column runs across
//! the sheet, and columns are counted along the direction it is used up in.
//! Every part is laid in those counts, so one search serves every corner and
//! direction; only turning a count back into a place on the sheet knows
//! which they are.

use camcraft_geom::contour::Region;
use camcraft_geom::turn::Turn;
use glam::{DVec2, dvec2};

use super::verify::ERROR;
use super::{Direction, Sheet, Stand, TICK_ERROR};

/// How much further apart than the spacing two parts can stand, at most, in
/// cells: each part's cells reach up to a cell's diagonal past the part grown
/// by half the spacing (half a diagonal grown, half a diagonal from a cell's
/// centre to its corner), and a part is moved a whole cell at a time, so the
/// next cell nearer might be the one that would have touched.
pub(super) const GAP_PER_CELL: f64 = 1.0 + 2.0 * std::f64::consts::SQRT_2;

/// The most two parts can stand further apart than the spacing, as a share of
/// it: what sets the cell. A quarter, by the benchmark in the tests: fifty
/// mixed parts at a 15° step on a 2440 by 1220 sheet, 10 apart, took 0.29 s
/// in all three orders on four threads at a half, 0.55 s at 0.35, 1.08 s at
/// a quarter and 3.1 s at 0.15, and placed 38, 40, 40 and 40 of them. A
/// quarter keeps the gap small, its typical part about half of it, and the
/// largest sheets laid out in about a second.
const EXTRA_GAP: f64 = 0.25;

/// The most cells a sheet is cut into; a sheet that would need more at the
/// spacing's cell has larger cells, and its parts a larger extra gap. A
/// 2440 by 1220 sheet, 10 apart, needs 6.9 million: the full extra gap
/// holds down to that spacing on the largest common sheet, and time holds
/// at about a second below it.
const MOST_CELLS: f64 = 8.0e6;

/// How much more than it needs each outline is grown for its cells, in
/// millimetres: what keeps the exact check from failing a layout the grid
/// made, which would turn a good layout into an error.
///
/// The clipper's rounding can leave the grown outline short of the exact
/// growth by [`TICK_ERROR`], so the cells cover the part grown by half the
/// spacing and `MARGIN - TICK_ERROR`, four times the check's [`ERROR`], more.
/// A part so stands at least the border and 4 `ERROR` inside the sheet, and
/// two at least the spacing and 8 `ERROR` apart. The check grows a part by
/// the border or the spacing and reaches at most `ERROR` past it, so it has
/// 3 `ERROR` to spare at the border and 7 between parts: 1.9 µm and 4.5 µm.
/// Where a cell is, worked out in floating point, is off by far less than a
/// tick.
const MARGIN: f64 = TICK_ERROR + 4.0 * ERROR;

/// The sheet's cells.
#[derive(Debug, Clone)]
pub(super) struct Grid {
    /// A cell's side.
    cell: f64,
    /// The front-left corner of the grid, on the sheet.
    origin: DVec2,
    /// Cells along X and along Y.
    nx: usize,
    ny: usize,
    /// Whether packing starts at the left and at the front.
    left: bool,
    front: bool,
    along: Direction,
}

impl Grid {
    /// The grid over `sheet` inset by its border less half its spacing,
    /// flush with that at the corner packing starts from: what is left over,
    /// less than a cell, is at the far sides.
    pub(super) fn new(sheet: &Sheet) -> Self {
        let inset = sheet.border - sheet.spacing / 2.0;
        let room = sheet.size - DVec2::splat(2.0 * inset);
        let cell =
            (sheet.spacing * EXTRA_GAP / GAP_PER_CELL).max((room.x * room.y / MOST_CELLS).sqrt());
        // A count of cells, of a room no larger than the sheet.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let count = |length: f64| (length / cell).floor() as usize;
        let (nx, ny) = (count(room.x), count(room.y));
        let lo = sheet.corner + DVec2::splat(inset);
        let (left, front) = (sheet.from.left(), sheet.from.front());
        #[allow(clippy::cast_precision_loss)]
        let span = dvec2(nx as f64, ny as f64) * cell;
        let origin = dvec2(
            if left { lo.x } else { lo.x + room.x - span.x },
            if front { lo.y } else { lo.y + room.y - span.y },
        );
        Self {
            cell,
            origin,
            nx,
            ny,
            left,
            front,
            along: sheet.along,
        }
    }

    /// A cell's side.
    #[cfg(test)]
    pub(super) fn cell(&self) -> f64 {
        self.cell
    }

    /// How far each outline is grown for its cells, over a spacing of
    /// `spacing`: half the spacing, half a cell's diagonal, and the
    /// [`MARGIN`] for the check.
    pub(super) fn growth(&self, spacing: f64) -> f64 {
        spacing / 2.0 + self.cell * std::f64::consts::FRAC_1_SQRT_2 + MARGIN
    }

    /// How far a round join's chords may fall inside its arc, growing an
    /// outline: the clipper grows by this much more, so it errs outward,
    /// and a fiftieth of a cell moves no cell's centre in or out by much.
    pub(super) fn arc_tolerance(&self) -> f64 {
        self.cell / 50.0
    }

    /// Columns along the direction the sheet is used up in.
    pub(super) fn long(&self) -> usize {
        match self.along {
            Direction::X => self.nx,
            Direction::Y => self.ny,
        }
    }

    /// Cells across, in each column.
    pub(super) fn wide(&self) -> usize {
        match self.along {
            Direction::X => self.ny,
            Direction::Y => self.nx,
        }
    }

    /// The column and the cell across of the cell `i` along X and `j` along Y
    /// from another, counted from that one.
    fn counted(&self, i: i64, j: i64) -> (i64, i64) {
        let x = if self.left { i } else { -i };
        let y = if self.front { j } else { -j };
        match self.along {
            Direction::X => (x, y),
            Direction::Y => (y, x),
        }
    }

    /// The cell along X and along Y of the sheet at column `p` and cell `q`
    /// across.
    fn on_sheet(&self, p: i64, q: i64) -> (i64, i64) {
        let (x, y) = match self.along {
            Direction::X => (p, q),
            Direction::Y => (q, p),
        };
        // Counts of cells, far inside an i64.
        let (nx, ny) = (self.nx as i64, self.ny as i64);
        (
            if self.left { x } else { nx - 1 - x },
            if self.front { y } else { ny - 1 - y },
        )
    }

    /// Where a part stands whose cells, `mask`, start at column `p` and cell
    /// `q` across.
    pub(super) fn stand(&self, mask: &Mask, p: usize, q: usize) -> Stand {
        // Counts of cells, far inside an i64.
        let (p, q) = (p as i64 + mask.origin.0, q as i64 + mask.origin.1);
        let (i, j) = self.on_sheet(p, q);
        // Counts of cells, far inside what an f64 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let at = dvec2(i as f64, j as f64) * self.cell;
        Stand {
            turn: mask.turn,
            offset: (self.origin + at).extend(0.0),
        }
    }
}

/// The cells across a column, from the first to the last of them.
#[derive(Debug, Clone, Copy)]
struct Run {
    column: usize,
    first: usize,
    last: usize,
}

/// The cells a part covers at one turn, as columns of runs, counted from its
/// first column and its first cell across.
#[derive(Debug, Clone)]
pub(super) struct Mask {
    turn: Turn,
    /// Columns, and cells across.
    long: usize,
    wide: usize,
    runs: Vec<Run>,
    /// The cell whose front-left corner is the part's origin, counted from
    /// the first: where the part stands follows from where its cells go.
    origin: (i64, i64),
}

impl Mask {
    /// The cells whose centres lie in `grown`, the part's outline grown for
    /// its cells ([`Grid::growth`]) and turned by `turn`, with its origin at
    /// a cell's front-left corner; `None` if there are none.
    ///
    /// Each line of cell centres across the sheet is cut by the outline
    /// ([`Region::spans`]), and every centre in a span is a cell covered, on
    /// its edge included.
    pub(super) fn new(grid: &Grid, grown: &Region, turn: Turn) -> Option<Self> {
        let (lo, hi) = grown.bounds()?;
        let c = grid.cell;
        // The first and last cell whose centre is within `lo..=hi`.
        #[allow(clippy::cast_possible_truncation)]
        let cells = |lo: f64, hi: f64| {
            let first = (lo / c - 0.5).ceil() as i64;
            let last = (hi / c - 0.5).floor() as i64;
            first..=last
        };
        // Lines of centres along the sheet's length, each cut across it.
        let lengthwise_x = grid.along == Direction::X;
        let (along, across) = if lengthwise_x {
            ((lo.x, hi.x), DVec2::Y)
        } else {
            ((lo.y, hi.y), DVec2::X)
        };
        let mut found: Vec<(i64, i64, i64)> = Vec::new();
        for k in cells(along.0, along.1) {
            // A count of cells, far inside what an f64 holds exactly.
            #[allow(clippy::cast_precision_loss)]
            let at = (k as f64 + 0.5) * c;
            // Across Y the line is `x = at`, which `spans` takes as `-x`.
            let v = if lengthwise_x { -at } else { at };
            for (u0, u1) in grown.spans(across, v) {
                let range = cells(u0, u1);
                let (first, last) = (*range.start(), *range.end());
                if first > last {
                    continue;
                }
                let (i0, j0, i1, j1) = if lengthwise_x {
                    (k, first, k, last)
                } else {
                    (first, k, last, k)
                };
                let (p, q0) = grid.counted(i0, j0);
                let (_, q1) = grid.counted(i1, j1);
                found.push((p, q0.min(q1), q0.max(q1)));
            }
        }
        let p0 = found.iter().map(|r| r.0).min()?;
        let p1 = found.iter().map(|r| r.0).max()?;
        let q0 = found.iter().map(|r| r.1).min()?;
        let q1 = found.iter().map(|r| r.2).max()?;
        let from = |v: i64, first: i64| usize::try_from(v - first).ok();
        let runs = found
            .iter()
            .map(|&(p, a, b)| {
                Some(Run {
                    column: from(p, p0)?,
                    first: from(a, q0)?,
                    last: from(b, q0)?,
                })
            })
            .collect::<Option<Vec<Run>>>()?;
        Some(Self {
            turn,
            long: from(p1 + 1, p0)?,
            wide: from(q1 + 1, q0)?,
            runs,
            origin: (-p0, -q0),
        })
    }

    /// Columns it spans.
    pub(super) fn long(&self) -> usize {
        self.long
    }
}

/// Which cells of the sheet are taken: one bit each, column by column.
#[derive(Debug, Clone)]
pub(super) struct Board {
    words: Vec<u64>,
    /// Words to a column.
    stride: usize,
    long: usize,
    wide: usize,
}

impl Board {
    /// A sheet of `long` columns, each `wide` cells across, all free.
    pub(super) fn new(long: usize, wide: usize) -> Self {
        let stride = wide.div_ceil(64);
        Self {
            words: vec![0; stride * long],
            stride,
            long,
            wide,
        }
    }

    /// Whether `mask` fits on the sheet at all, with nothing else on it.
    pub(super) fn holds(&self, mask: &Mask) -> bool {
        mask.long <= self.long && mask.wide <= self.wide
    }

    fn column(&self, p: usize) -> &[u64] {
        let at = p * self.stride;
        self.words.get(at..at + self.stride).unwrap_or(&[])
    }

    /// The first cell taken in column `p` from `first` to `last`, if any is.
    fn first_taken(&self, p: usize, first: usize, last: usize) -> Option<usize> {
        let words = self.column(p);
        let mut w = first / 64;
        let end = last / 64;
        let mut bits = words.get(w)? & (u64::MAX << (first % 64));
        loop {
            if w == end {
                bits &= u64::MAX >> (63 - last % 64);
                return (bits != 0).then(|| w * 64 + bits.trailing_zeros() as usize);
            }
            if bits != 0 {
                return Some(w * 64 + bits.trailing_zeros() as usize);
            }
            w += 1;
            bits = *words.get(w)?;
        }
    }

    /// The first cell free in column `p` from `from` on: `wide` if none is.
    fn first_free(&self, p: usize, from: usize) -> usize {
        let words = self.column(p);
        let mut w = from / 64;
        let mut free = !words.get(w).copied().unwrap_or(u64::MAX) & (u64::MAX << (from % 64));
        loop {
            if free != 0 {
                return (w * 64 + free.trailing_zeros() as usize).min(self.wide);
            }
            w += 1;
            match words.get(w) {
                Some(&taken) => free = !taken,
                None => return self.wide,
            }
        }
    }

    /// The first cell across at which `mask` fits with its first column at
    /// `p`, if it does anywhere in that column.
    ///
    /// Each run is tried in turn; one that meets a taken cell moves the mask
    /// just past the stretch of taken cells it met, which no place before
    /// could clear. The mask fits once every run has been tried in a row
    /// without moving it.
    fn lowest(&self, mask: &Mask, p: usize) -> Option<usize> {
        let most = self.wide.checked_sub(mask.wide)?;
        let mut q = 0;
        let (mut clear, mut k) = (0, 0);
        while clear < mask.runs.len() {
            let run = mask.runs.get(k)?;
            let column = p + run.column;
            match self.first_taken(column, q + run.first, q + run.last) {
                Some(taken) => {
                    q = self.first_free(column, taken) - run.first;
                    if q > most {
                        return None;
                    }
                    clear = 0;
                }
                None => {
                    clear += 1;
                    k = (k + 1) % mask.runs.len();
                }
            }
        }
        Some(q)
    }

    /// The first place `mask` fits, nearest the start along the sheet and
    /// then across it, as its first column and its first cell across: unless
    /// every place it could fit is worse than `bound`, or the work is to stop.
    ///
    /// `bound` is the best of the places found so far, at any turn, as the
    /// length of sheet used and the column ([`place_key`]); finding one
    /// better lowers it. A place is passed over only when it is strictly
    /// worse than one already found, so what is kept in the end is the same
    /// whichever turn finds its place first.
    pub(super) fn first_fit(
        &self,
        mask: &Mask,
        used: usize,
        bound: &std::sync::atomic::AtomicU64,
        stopped: impl Fn() -> bool,
    ) -> Option<(usize, usize)> {
        use std::sync::atomic::Ordering;
        if !self.holds(mask) {
            return None;
        }
        for p in 0..=self.long - mask.long {
            let reach = used.max(p + mask.long);
            if place_key(reach, p) > bound.load(Ordering::Relaxed) || stopped() {
                return None;
            }
            if let Some(q) = self.lowest(mask, p) {
                bound.fetch_min(place_key(reach, p), Ordering::Relaxed);
                return Some((p, q));
            }
        }
        None
    }

    /// Take the cells of `mask` with its first column at `p` and its first
    /// cell across at `q`.
    pub(super) fn take(&mut self, mask: &Mask, p: usize, q: usize) {
        for run in &mask.runs {
            let at = (p + run.column) * self.stride;
            let Some(words) = self.words.get_mut(at..at + self.stride) else {
                continue;
            };
            let (first, last) = (q + run.first, q + run.last);
            let (from, to) = (first / 64, last / 64);
            for (w, word) in words.iter_mut().enumerate().take(to + 1).skip(from) {
                let lo = if w == from { first % 64 } else { 0 };
                let hi = if w == to { last % 64 } else { 63 };
                *word |= (u64::MAX << lo) & (u64::MAX >> (63 - hi));
            }
        }
    }
}

/// The length of sheet used and the column a place starts at, as one number
/// that orders as the two do in turn.
pub(super) fn place_key(used: usize, column: usize) -> u64 {
    // Counts of cells, far below 2³², so neither spills into the other.
    ((used as u64) << 32) | column as u64
}
