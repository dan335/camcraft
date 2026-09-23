//! The material at any point in a finished job.
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

use camcraft_sim::stock::{SimMove, Stock};

use crate::job::Job;
use crate::run::{JobResult, RunError, cutting_tool};
use crate::simadapt::sim_moves;
use crate::stock::EffectiveTool;

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
struct Pass {
    tool: EffectiveTool,
    moves: Vec<SimMove>,
}

/// A finished job, ready to have its material recovered at any position.
#[derive(Debug, Clone)]
pub struct Replay {
    passes: Vec<Pass>,
    /// The stock before each operation; one more than there are operations.
    keyframes: Vec<Stock>,
}

impl Replay {
    /// Everything needed to replay `result`, which must be `job`'s.
    pub fn new(job: &Job, result: &JobResult) -> Result<Self, RunError> {
        let passes = job
            .operations
            .iter()
            .zip(&result.operations)
            .map(|(op, ran)| {
                Ok(Pass {
                    tool: cutting_tool(job, op)?,
                    moves: sim_moves(&ran.path),
                })
            })
            .collect::<Result<Vec<_>, RunError>>()?;
        Ok(Self {
            passes,
            keyframes: result.stages.clone(),
        })
    }

    /// How many moves operation `op` has.
    pub fn moves_in(&self, op: usize) -> usize {
        self.passes.get(op).map_or(0, |p| p.moves.len())
    }

    /// The stock as it stood at `position`, or `None` past the end of the job.
    pub fn stock_at(&self, position: Position) -> Option<Stock> {
        let mut stock = self.keyframes.get(position.op)?.clone();
        self.advance(&mut stock, position.op, 0, position.moves);
        Some(stock)
    }

    /// Sweep operation `op`'s moves `from..to` into `stock`, which must be the
    /// material as it stood after its first `from` moves.
    ///
    /// The slice starts one move early, so the first move swept is swept from where
    /// the tool really was. That overlap is what makes pieces equal the whole.
    pub fn advance(&self, stock: &mut Stock, op: usize, from: usize, to: usize) {
        let Some(pass) = self.passes.get(op) else {
            return;
        };
        let to = to.min(pass.moves.len());
        if to <= from {
            return;
        }
        if let Some(slice) = pass.moves.get(from.saturating_sub(1)..to) {
            stock.apply(slice, &pass.tool);
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::run::run_job;
    use camcraft_geom::synthetic;

    const JOB: &str = r#"
tolerance = 0.05

[setup]
stock = { kind = "box", margin = [1.0, 1.0, 2.0] }

[tools.rough]
kind = "flat"
diameter = 6.0
number = 1

[tools.finish]
kind = "ball"
diameter = 4.0
number = 2

[[operation]]
name = "rough"
strategy = "zlevel-rough"
tool = "rough"
stepdown = 2.5
passes = { stepover = 3.0, feed = 1800.0, stock_to_leave = { radial = 0.5, axial = 0.5 } }

[[operation]]
name = "finish"
strategy = "parallel"
tool = "finish"
passes = { stepover = 0.8, feed = 2000.0 }
"#;

    fn run() -> (Job, JobResult) {
        let job: Job = toml::from_str(JOB).unwrap();
        let result = run_job(&job, &synthetic::uv_sphere(8.0, 60, 84)).unwrap();
        (job, result)
    }

    fn bits(stock: &Stock) -> Vec<u32> {
        stock.field().data().iter().map(|v| v.to_bits()).collect()
    }

    /// The keyframes are what the run passed through: the untouched block first, the
    /// finished stock last, and material only ever removed in between.
    #[test]
    fn stages_are_the_keyframes_of_the_run() {
        let (job, result) = run();
        assert_eq!(result.stages.len(), job.operations.len() + 1);
        let block = result.stages.first().unwrap();
        let top = result.heights.stock_top;
        assert!(
            block
                .field()
                .data()
                .iter()
                .all(|h| *h == camcraft_geom::heightmap::narrow(top))
        );
        assert_eq!(bits(result.stages.last().unwrap()), bits(&result.stock));
        for pair in result.stages.windows(2) {
            let [a, b] = pair else { continue };
            for (before, after) in a.field().data().iter().zip(b.field().data()) {
                assert!(after <= before, "material appeared: {before} -> {after}");
            }
        }
    }

    /// Replaying each operation from its keyframe lands exactly on the next one.
    #[test]
    fn replay_reproduces_every_stage() {
        let (job, result) = run();
        let replay = Replay::new(&job, &result).unwrap();
        for op in 0..job.operations.len() {
            let n = replay.moves_in(op);
            let got = replay.stock_at(Position { op, moves: n }).unwrap();
            assert_eq!(
                bits(&got),
                bits(&result.stages[op + 1]),
                "operation {op} replayed differently from how it ran"
            );
        }
        assert!(replay.stock_at(Position { op: 9, moves: 0 }).is_none());
    }

    /// The property playback rests on: sweeping in pieces of any size equals
    /// sweeping whole, bit for bit.
    #[test]
    fn pieces_equal_the_whole() {
        let (job, result) = run();
        let replay = Replay::new(&job, &result).unwrap();
        // A fixed pseudo-random sequence: reproducible, and no dependency.
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = |max: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            1 + usize::try_from(seed % max as u64).unwrap()
        };
        for op in 0..job.operations.len() {
            let n = replay.moves_in(op);
            assert!(n > 50, "operation {op} is too short to prove anything");
            let whole = replay.stock_at(Position { op, moves: n }).unwrap();

            let mut pieces = result.stages[op].clone();
            let mut at = 0;
            let mut chunks = 0;
            while at < n {
                // Pieces of up to a twentieth of the operation: always many.
                let to = (at + next((n / 20).max(2))).min(n);
                replay.advance(&mut pieces, op, at, to);
                at = to;
                chunks += 1;
            }
            assert!(chunks > 10);
            assert_eq!(
                bits(&pieces),
                bits(&whole),
                "operation {op} drifted in pieces"
            );

            // And one move at a time, the finest a step can be.
            let mut single = result.stages[op].clone();
            for k in 0..n {
                replay.advance(&mut single, op, k, k + 1);
            }
            assert_eq!(
                bits(&single),
                bits(&whole),
                "operation {op} drifted move by move"
            );
        }
    }

    /// Part-way through an operation the stock lies between its keyframes.
    #[test]
    fn midway_is_between_the_keyframes() {
        let (job, result) = run();
        let replay = Replay::new(&job, &result).unwrap();
        let n = replay.moves_in(0);
        let mid = replay
            .stock_at(Position {
                op: 0,
                moves: n / 2,
            })
            .unwrap();
        let (before, after) = (&result.stages[0], &result.stages[1]);
        let volume = |s: &Stock| s.volume();
        assert!(
            volume(&mid) < volume(before),
            "half the roughing removed nothing"
        );
        assert!(
            volume(&mid) > volume(after),
            "half the roughing removed everything"
        );
    }
}
