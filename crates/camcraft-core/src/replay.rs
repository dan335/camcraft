//! The material at any point in a finished setup.
//!
//! A run keeps the stock only at its keyframes: before the first operation and after
//! each. [`Replay`] recovers it anywhere between, by starting from the nearest
//! keyframe at or before the position and sweeping the moves forward. The tools are
//! the ones the run cut with, built by [`cutting_tool`], so what is replayed is what
//! was verified rather than a second opinion about it.
//!
//! # Why this is exact
//!
//! [`Stock::apply`] treats the first move of a slice as a starting point and
//! sweeps each later move from the one before. So a slice started one move early
//! sweeps exactly the moves a whole run would, and the stock after moves in pieces
//! is bit-identical to the stock after them all at once. Playback, seeking and
//! stepping backwards all rest on that, and the tests below hold it.
//!
//! A position part-way along a move is not a state to continue from. Sweeping to a
//! midpoint and on to the end samples different points than sweeping the move whole,
//! so the stock would drift. The state a replay carries forward advances only by
//! whole moves; a part-way position is a sweep on a copy, for showing.
//!
//! # Playing it
//!
//! [`Live`] is the material as playback shows it: one stock that seeks to any
//! position, forward by sweeping, back by restoring the nearest checkpoint it
//! dropped on the way forward. Every step says which cells changed, so a viewer
//! redraws only those. A part-way position sweeps the rest of the way onto the live
//! stock and remembers the cells it covered, to put them back before the next step:
//! the committed state still only ever advances by whole moves.

use std::collections::BTreeMap;
use std::sync::Arc;

use camcraft_sim::stock::{CellRect, PackedStock, SimMove, Stock, SweptTool};

use crate::cutter::allowance::EffectiveTool;
use crate::job::{Job, Setup};
use crate::path::simadapt::sim_moves;
use crate::run::{RunError, SetupResult, cutting_tool};

/// A place in the program: the moves of one operation applied so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    /// Which operation, from zero.
    pub op: usize,
    /// How many of its moves have been made. Zero is the operation not started;
    /// its move count is the operation finished.
    pub moves: usize,
}

/// What a replay needs to sweep one operation again.
#[derive(Debug, Clone)]
struct OpSweep {
    /// None for an operation the run did not make (turned off, or left out),
    /// which has no moves and may have no tool.
    tool: Option<EffectiveTool>,
    moves: Vec<SimMove>,
}

/// A finished setup, ready to have its material recovered at any position.
#[derive(Debug, Clone)]
pub struct Replay {
    sweeps: Vec<OpSweep>,
    /// The stock before each operation, packed; one more than there are
    /// operations. Shared with the run's result, not copied.
    keyframes: Vec<Arc<PackedStock>>,
}

impl Replay {
    /// Everything needed to replay `result`, which must be `setup`'s, in `job`.
    pub fn new(job: &Job, setup: &Setup, result: &SetupResult) -> Result<Self, RunError> {
        let sweeps = setup
            .operations
            .iter()
            .zip(&result.operations)
            .map(|(op, ran)| {
                Ok(OpSweep {
                    tool: if ran.made.has_paths() {
                        Some(cutting_tool(job, op)?)
                    } else {
                        None
                    },
                    moves: sim_moves(&ran.path),
                })
            })
            .collect::<Result<Vec<_>, RunError>>()?;
        Ok(Self {
            sweeps,
            keyframes: result.keyframes.clone(),
        })
    }

    /// How many moves operation `op` has.
    pub fn moves_in(&self, op: usize) -> usize {
        self.sweeps.get(op).map_or(0, |p| p.moves.len())
    }

    /// The stock as it stood at `position`, or `None` past the end of the job.
    pub fn stock_at(&self, position: Position) -> Option<Stock> {
        let mut stock = self.keyframes.get(position.op)?.unpack().ok()?;
        self.advance(&mut stock, position.op, 0, position.moves);
        Some(stock)
    }

    /// Sweep operation `op`'s moves `from..to` into `stock`, which must be the
    /// material as it stood after its first `from` moves.
    ///
    /// The slice starts one move early, so the first move swept is swept from where
    /// the tool really was. That overlap is what makes pieces equal the whole.
    ///
    /// Returns the cells it lowered.
    pub fn advance(
        &self,
        stock: &mut Stock,
        op: usize,
        from: usize,
        to: usize,
    ) -> Option<CellRect> {
        let sweep = self.sweeps.get(op)?;
        let to = to.min(sweep.moves.len());
        if to <= from {
            return None;
        }
        let slice = sweep.moves.get(from.saturating_sub(1)..to)?;
        stock.sweep(slice, sweep.tool.as_ref()?).touched
    }

    /// How many operations there are.
    pub fn ops(&self) -> usize {
        self.sweeps.len()
    }

    /// Sweep a `fraction` of operation `op`'s move after its first `made` into
    /// `stock`, which must stand after those moves. Returns the cells that could
    /// have changed, with what they held before, to be put back.
    fn partial(
        &self,
        stock: &mut Stock,
        op: usize,
        made: usize,
        fraction: f64,
    ) -> Option<(CellRect, Vec<f32>)> {
        let sweep = self.sweeps.get(op)?;
        // The first move is where the tool starts, so there is nothing before it
        // to sweep from.
        let from = sweep.moves.get(made.checked_sub(1)?)?;
        let next = sweep.moves.get(made)?;
        let tool = sweep.tool.as_ref()?;
        let r = tool.max_radius();
        let part = next.part(from.to, fraction.clamp(0.0, 1.0));
        // An arc bulges past its ends, so its footprint is its chords'.
        let rect = part
            .chords(from.to, stock.field().pitch())
            .into_iter()
            .fold(stock.footprint(from.to, r), |rect, p| {
                CellRect::join(rect, stock.footprint(p, r))
            })?;
        let saved = stock.read_cells(rect);
        stock.sweep(&[*from, part], tool);
        Some((rect, saved))
    }
}

/// Which cells a step of [`Live`] changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Changed {
    /// None.
    Nothing,
    /// Some, all inside this rectangle.
    Cells(CellRect),
    /// Any of them: the stock was replaced.
    All,
}

impl Changed {
    /// What either changed.
    #[must_use]
    pub fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::All, _) | (_, Self::All) => Self::All,
            (Self::Nothing, x) | (x, Self::Nothing) => x,
            (Self::Cells(a), Self::Cells(b)) => Self::Cells(a.union(b)),
        }
    }

    fn cells(rect: Option<CellRect>) -> Self {
        rect.map_or(Self::Nothing, Self::Cells)
    }
}

/// The material as playback shows it, at any position, reached by seeking.
#[derive(Debug, Clone)]
pub struct Live {
    stock: Stock,
    /// Whole moves made: what `stock` stands after, part-way sweep aside.
    at: Position,
    /// A part-way sweep on top: the cells it may have changed and what they held.
    overlay: Option<(CellRect, Vec<f32>)>,
    /// Stock dropped on the way forward, packed, every `spacing` moves of an
    /// operation, while they fit `budget` bytes.
    checkpoints: BTreeMap<Position, PackedStock>,
    spacing: usize,
    budget: usize,
    held: usize,
    /// Whether it drops checkpoints as it goes.
    dropping: bool,
}

impl Live {
    /// The block before anything, keeping checkpoints within `budget` bytes.
    ///
    /// The checkpoints are spread evenly through the program, [`CHECKPOINTS`] of
    /// them, so going back costs at most one spacing of sweeping, whatever the
    /// position. They are packed, and machined material packs small, so they
    /// usually all fit; where they do not, the later ones are not kept.
    pub fn new(replay: &Replay, budget: usize) -> Option<Self> {
        let stock = replay.keyframes.first()?.unpack().ok()?;
        let total: usize = (0..replay.ops()).map(|op| replay.moves_in(op)).sum();
        Some(Self {
            stock,
            at: Position { op: 0, moves: 0 },
            overlay: None,
            checkpoints: BTreeMap::new(),
            spacing: total.div_ceil(CHECKPOINTS).max(MIN_SPACING),
            budget,
            held: 0,
            dropping: true,
        })
    }

    /// Stop or start dropping checkpoints on the way forward. A stock another
    /// sweeps ahead for ([`Live::adopt_checkpoints`]) need not: packing one takes a
    /// moment, and a seek someone is waiting on should not spend it.
    pub fn set_dropping(&mut self, dropping: bool) {
        self.dropping = dropping;
    }

    /// The material, as the last step left it.
    pub fn stock(&self) -> &Stock {
        &self.stock
    }

    /// The whole moves made.
    pub fn at(&self) -> Position {
        self.at
    }

    /// Checkpoints held.
    pub fn checkpoints(&self) -> usize {
        self.checkpoints.len()
    }

    /// Bytes the checkpoints hold.
    pub fn held(&self) -> usize {
        self.held
    }

    /// Take the checkpoints `other` has that this has not, so a second stock swept
    /// ahead in idle time makes seeking back fast everywhere. Both must replay the
    /// same program with the same budget.
    pub fn adopt_checkpoints(&mut self, other: &mut Self) {
        if other.spacing != self.spacing {
            return;
        }
        other.held = 0;
        for (at, stock) in std::mem::take(&mut other.checkpoints) {
            if self.held + stock.bytes() <= self.budget && !self.checkpoints.contains_key(&at) {
                self.held += stock.bytes();
                self.checkpoints.insert(at, stock);
            }
        }
    }

    /// Sweep on toward the end of the program through every operation, dropping
    /// checkpoints, about `budget` moves at a time. Returns whether it is there.
    ///
    /// A seek to the end would restore the finished stock and sweep nothing;
    /// this sweeps through each operation in turn, from wherever it stands.
    pub fn sweep_ahead(&mut self, replay: &Replay, budget: usize) -> bool {
        let Some(last) = replay.ops().checked_sub(1) else {
            return true;
        };
        let op = if self.at.moves >= replay.moves_in(self.at.op) {
            self.at.op + 1
        } else {
            self.at.op
        };
        if op > last {
            return true;
        }
        let end = Position {
            op,
            moves: replay.moves_in(op),
        };
        self.lift_overlay();
        self.sweep_toward(replay, end, budget);
        op == last && self.at == end
    }

    /// Move toward `target`, and `fraction` of the way along the move after it,
    /// sweeping at most about `budget` moves. Returns what changed, and whether
    /// the target was reached; call again until it was.
    pub fn seek(
        &mut self,
        replay: &Replay,
        target: Position,
        fraction: f64,
        budget: usize,
    ) -> (Changed, bool) {
        let mut changed = self.lift_overlay();
        let Some(target) = clamp(replay, target) else {
            return (changed, true);
        };
        match self.restore_near(replay, target) {
            Some(restored) => changed = changed.and(restored),
            None => return (changed, true),
        }
        changed = changed.and(self.sweep_toward(replay, target, budget));
        let reached = self.at == target;
        if reached
            && fraction > 0.0
            && let Some((rect, saved)) =
                replay.partial(&mut self.stock, target.op, target.moves, fraction)
        {
            changed = changed.and(Changed::Cells(rect));
            self.overlay = Some((rect, saved));
        }
        (changed, reached)
    }

    /// Put back the cells a part-way sweep covered, and say which.
    fn lift_overlay(&mut self) -> Changed {
        match self.overlay.take() {
            Some((rect, saved)) => {
                self.stock.write_cells(rect, &saved);
                Changed::Cells(rect)
            }
            None => Changed::Nothing,
        }
    }

    /// Restore the nearest stock at or before `target` that is not behind where
    /// this is, when that is worth it: to go back, or to save a whole spacing of
    /// sweeping forward. Returns what changed, or `None` when the stock could not
    /// be restored.
    fn restore_near(&mut self, replay: &Replay, target: Position) -> Option<Changed> {
        // The end of an operation is the keyframe after it, so going there needs
        // no sweeping at all: at the end of the program, the finished stock.
        let finished = replay
            .keyframes
            .get(target.op + 1)
            .filter(|_| target.moves >= replay.moves_in(target.op));
        // Else the nearest stock at or before the target that is not behind where
        // we are: the operation's keyframe, or a checkpoint in it.
        let start = if finished.is_some() {
            target
        } else {
            self.checkpoints
                .range(
                    Position {
                        op: target.op,
                        moves: 0,
                    }..=target,
                )
                .next_back()
                .map_or(
                    Position {
                        op: target.op,
                        moves: 0,
                    },
                    |(p, _)| *p,
                )
        };
        let behind = target < self.at;
        // Worth restoring ahead only when it saves a whole spacing of sweeping.
        let ahead = start > self.at
            && (start.op > self.at.op || start.moves >= self.at.moves + self.spacing);
        if !(behind || ahead) {
            return Some(Changed::Nothing);
        }
        let from = finished.map(|k| &**k).or_else(|| {
            self.checkpoints
                .get(&start)
                .or_else(|| replay.keyframes.get(start.op).map(|k| &**k))
        });
        let Some(from) = from else {
            return Some(Changed::Nothing);
        };
        // Unpacked into the stock in place, and only what differs needs drawing
        // again, which after a short step back is a small part of a fine stock.
        let changed = match from.restore(&mut self.stock) {
            Some(Some(rect)) => Changed::Cells(rect),
            Some(None) => Changed::Nothing,
            None => match from.unpack() {
                Ok(stock) => {
                    self.stock = stock;
                    Changed::All
                }
                Err(_) => return None,
            },
        };
        self.at = start;
        Some(changed)
    }

    /// Sweep whole moves toward `target`, at most about `budget` of them,
    /// dropping checkpoints on the way. Returns what changed.
    fn sweep_toward(&mut self, replay: &Replay, target: Position, budget: usize) -> Changed {
        let mut changed = Changed::Nothing;
        let mut left = budget.max(1);
        while self.at < target && left > 0 {
            let op = self.at.op;
            let n = replay.moves_in(op);
            let end = if op < target.op { n } else { target.moves };
            // Stop at the next checkpoint, to drop one there.
            let next_mark = (self.at.moves / self.spacing + 1) * self.spacing;
            let to = end.min(next_mark).min(self.at.moves + left);
            if to > self.at.moves {
                let touched = replay.advance(&mut self.stock, op, self.at.moves, to);
                changed = changed.and(Changed::cells(touched));
                left = left.saturating_sub(to - self.at.moves);
                self.at.moves = to;
                if self.dropping
                    && to % self.spacing == 0
                    && to < n
                    && !self.checkpoints.contains_key(&self.at)
                {
                    let packed = PackedStock::pack(&self.stock);
                    if self.held + packed.bytes() <= self.budget {
                        self.held += packed.bytes();
                        self.checkpoints.insert(self.at, packed);
                    }
                }
            }
            if self.at.moves >= n && op < target.op {
                self.at = Position {
                    op: op + 1,
                    moves: 0,
                };
            }
        }
        changed
    }
}

/// Fewest moves between checkpoints: closer, and restoring costs more than
/// sweeping would.
const MIN_SPACING: usize = 64;

/// Checkpoints spread through a program, when they fit.
pub const CHECKPOINTS: usize = 64;

/// `target`, inside the program: past the end is the end.
fn clamp(replay: &Replay, target: Position) -> Option<Position> {
    let last = replay.ops().checked_sub(1)?;
    if target.op > last {
        return Some(Position {
            op: last,
            moves: replay.moves_in(last),
        });
    }
    Some(Position {
        op: target.op,
        moves: target.moves.min(replay.moves_in(target.op)),
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::indexing_slicing
)]
mod tests;
