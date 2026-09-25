//! Why a setup could not be run.

use camcraft_sim::stock::StockError;

use crate::allowance::AllowanceError;
use crate::drilling::HoleError;
use crate::features::FeatureError;
use crate::job::{JobError, Operation};
use crate::linker::LinkError;
use crate::reach::ReachError;
use crate::setup::PrepareError;
use crate::strategy::StrategyError;
use crate::tool::ToolError;
use crate::toolpath::ToolpathError;

/// A setup that could not be run.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The job was not valid.
    #[error(transparent)]
    Job(#[from] JobError),
    /// The setup's models and stock could not be laid out.
    #[error(transparent)]
    Prepare(#[from] PrepareError),
    /// A strategy failed.
    #[error("operation '{op}': {source}")]
    Strategy {
        /// Which operation.
        op: String,
        /// Why.
        source: StrategyError,
    },
    /// Linking failed.
    #[error("operation '{op}': {source}")]
    Link {
        /// Which operation.
        op: String,
        /// Why.
        source: LinkError,
    },
    /// Where the cutter may not go could not be worked out from the model.
    #[error("operation '{op}': {source}")]
    Reach {
        /// Which operation.
        op: String,
        /// Why.
        source: ReachError,
    },
    /// What the operation was kept to could not be found or used.
    #[error("operation '{op}': {source}")]
    Feature {
        /// Which operation.
        op: String,
        /// Why.
        source: FeatureError,
    },
    /// A drilling operation's picks are not holes.
    #[error("operation '{op}': {source}")]
    Holes {
        /// Which operation.
        op: String,
        /// Why.
        source: HoleError,
    },
    /// The stock could not be built.
    #[error("cannot build stock: {0}")]
    Stock(#[from] StockError),
    /// A tool dimension was not usable.
    #[error(transparent)]
    Tool(#[from] ToolError),
    /// Stock to leave was not usable.
    #[error(transparent)]
    Allowance(#[from] AllowanceError),
    /// A move could not be added.
    #[error(transparent)]
    Toolpath(#[from] ToolpathError),
}

/// What an operation's own work can fail with, which a [`RunError`] carries
/// together with the operation's name.
pub(super) trait OperationError {
    /// This error, as operation `op`'s.
    fn named(self, op: String) -> RunError;
}

impl OperationError for StrategyError {
    fn named(self, op: String) -> RunError {
        RunError::Strategy { op, source: self }
    }
}

impl OperationError for LinkError {
    fn named(self, op: String) -> RunError {
        RunError::Link { op, source: self }
    }
}

impl OperationError for ReachError {
    fn named(self, op: String) -> RunError {
        RunError::Reach { op, source: self }
    }
}

impl OperationError for FeatureError {
    fn named(self, op: String) -> RunError {
        RunError::Feature { op, source: self }
    }
}

impl OperationError for HoleError {
    fn named(self, op: String) -> RunError {
        RunError::Holes { op, source: self }
    }
}

/// Name `op` in whatever its work failed with, for `map_err`.
pub(super) fn in_op<E: OperationError>(op: &Operation) -> impl Fn(E) -> RunError + '_ {
    move |source| source.named(op.name.clone())
}
