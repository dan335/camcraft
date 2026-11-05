//! Each operation's passes as a run worked them out, kept by what they were
//! worked out from: the operation, its tool, and what shapes the setup's cuts.
//! A run of the setup after something before an operation has changed, or
//! after an edit to it is undone, or with it moved, links them again over the
//! material as it then stands ([`Plan::Relink`]) rather than working them out
//! afresh.
//!
//! Passes are found from the part alone for every strategy but one that
//! clears what the operations before it left
//! ([`Uses::rest`](camcraft_core::job::Uses::rest)), whose passes are kept by
//! those operations too: found again only where every one before it, and its
//! tool, is as it was.

use std::sync::Arc;

use camcraft_core::job::{Operation, ToolSpec};
use camcraft_core::run::{Planned, SetupResult};

use super::run::SetupInputs;

/// How many operations' passes a setup keeps beyond one for each of its
/// operations: enough for an edit or two undone, and never without bound.
const SPARE: usize = 8;

/// One operation's passes, and what they were worked out from.
#[derive(Debug, Clone)]
struct KeptPasses {
    /// What the setup's cuts were shaped by, and the operation and its tool,
    /// as they were.
    inputs: Arc<SetupInputs>,
    /// Which of its operations.
    index: usize,
    /// The passes.
    planned: Arc<Planned>,
}

impl KeptPasses {
    /// The operation, and its tool, as the passes were worked out for them.
    fn operation(&self) -> Option<(&Operation, Option<&ToolSpec>)> {
        self.inputs.operation(self.index)
    }

    /// Whether these are the passes of operation `index` as `inputs` has it:
    /// the same operation and tool, the cuts shaped alike, and, for one that
    /// clears what the operations before it left, the same operations before
    /// it.
    fn stand_for(&self, inputs: &SetupInputs, index: usize) -> bool {
        let Some((op, tool)) = inputs.operation(index) else {
            return false;
        };
        let after_the_same = || {
            self.index == index
                && (0..index).all(|j| self.inputs.operation(j) == inputs.operation(j))
        };
        self.operation() == Some((op, tool))
            && self.inputs.cuts_alike(inputs)
            && (!op.strategy.uses().rest || after_the_same())
    }
}

/// A setup's operations' passes, kept from its runs.
#[derive(Debug, Clone, Default)]
pub struct PassCache {
    /// Oldest first.
    kept: Vec<KeptPasses>,
}

impl PassCache {
    /// Keep the passes of every operation of `result` that has them, run from
    /// `inputs`, in place of any kept for the same operation worked out from
    /// the same.
    pub fn keep(&mut self, inputs: &SetupInputs, result: &SetupResult) {
        let inputs = Arc::new(inputs.clone());
        for (index, made) in result.operations.iter().enumerate() {
            let Some(planned) = &made.planned else {
                continue;
            };
            self.kept.retain(|k| !k.stand_for(&inputs, index));
            self.kept.push(KeptPasses {
                inputs: Arc::clone(&inputs),
                index,
                planned: Arc::clone(planned),
            });
        }
        let most = inputs.operations() + SPARE;
        if self.kept.len() > most {
            self.kept.drain(..self.kept.len() - most);
        }
    }

    /// The passes kept for operation `index` as `now` has it, worked out
    /// where the cuts were shaped as `now` shapes them; and, for one that
    /// clears what the operations before it left, after the same operations.
    pub fn find(&self, now: &SetupInputs, index: usize) -> Option<Arc<Planned>> {
        self.kept
            .iter()
            .rev()
            .find(|k| k.stand_for(now, index))
            .map(|k| Arc::clone(&k.planned))
    }
}
