//! Why a setup could not be run.

use camcraft_geom::contour::RegionError;
use camcraft_sim::stock::StockError;

use crate::cutter::allowance::AllowanceError;
use crate::cutter::tool::ToolError;
use crate::job::{JobError, Operation};
use crate::path::linker::LinkError;
use crate::path::toolpath::ToolpathError;
use crate::setup::PrepareError;
use crate::setup::features::FeatureError;
use crate::strategy::StrategyError;
use crate::strategy::chamfer::ChamferError;
use crate::strategy::drilling::HoleError;
use crate::strategy::engrave::EngraveError;
use crate::strategy::reach::ReachError;
use crate::strategy::round::RoundError;
use crate::strategy::slot::SlotError;
use crate::strategy::trace::TraceError;

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
    /// A bore's or a circular's picks are not round walls, or one cannot be
    /// cut.
    #[error("operation '{op}': {source}")]
    Rounds {
        /// Which operation.
        op: String,
        /// Why.
        source: RoundError,
    },
    /// A slot's picks are not slots, or one cannot be cut.
    #[error("operation '{op}': {source}")]
    Slots {
        /// Which operation.
        op: String,
        /// Why.
        source: SlotError,
    },
    /// A trace's picks cannot be followed.
    #[error("operation '{op}': {source}")]
    Trace {
        /// Which operation.
        op: String,
        /// Why.
        source: TraceError,
    },
    /// A 2D chamfer's picks cannot be chamfered.
    #[error("operation '{op}': {source}")]
    Chamfer {
        /// Which operation.
        op: String,
        /// Why.
        source: ChamferError,
    },
    /// An engraving's picks are not shapes, or cannot be engraved.
    #[error("operation '{op}': {source}")]
    Engrave {
        /// Which operation.
        op: String,
        /// Why.
        source: EngraveError,
    },
    /// The rest of the tool would meet the part, and the operation was to
    /// fail where it would: Fusion's Shaft and Holder's Fail.
    #[error(
        "operation '{op}': the shank or holder would meet the part at X{:.3} Y{:.3} Z{:.3}",
        at.x,
        at.y,
        at.z
    )]
    Held {
        /// Which operation.
        op: String,
        /// Where the tool tip was.
        at: glam::DVec3,
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
    /// The run was stopped ([`crate::control`]): what was made before stands,
    /// and was told as it was made.
    #[error("stopped")]
    Stopped,
    /// An operation's work ended without an answer: it panicked.
    #[error("operation '{op}': its work ended without an answer")]
    Lost {
        /// Which operation.
        op: String,
    },
}

impl RunError {
    /// Whether this is the run being stopped, however deep in an operation's
    /// work it was found, rather than anything gone wrong.
    pub fn is_stopped(&self) -> bool {
        matches!(
            self,
            RunError::Stopped
                | RunError::Strategy {
                    source: StrategyError::Stopped(_),
                    ..
                }
                | RunError::Link {
                    source: LinkError::Stopped(_),
                    ..
                }
        )
    }
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

impl OperationError for RoundError {
    fn named(self, op: String) -> RunError {
        match self {
            // Stopped, or failed making the path, as any strategy does.
            RoundError::Strategy(e) => e.named(op),
            source => RunError::Rounds { op, source },
        }
    }
}

impl OperationError for SlotError {
    fn named(self, op: String) -> RunError {
        match self {
            // Stopped, or failed making the path, as any strategy does.
            SlotError::Strategy(e) => e.named(op),
            source => RunError::Slots { op, source },
        }
    }
}

impl OperationError for TraceError {
    fn named(self, op: String) -> RunError {
        match self {
            TraceError::Strategy(e) => e.named(op),
            source => RunError::Trace { op, source },
        }
    }
}

impl OperationError for ChamferError {
    fn named(self, op: String) -> RunError {
        match self {
            ChamferError::Strategy(e) => e.named(op),
            source => RunError::Chamfer { op, source },
        }
    }
}

impl OperationError for EngraveError {
    fn named(self, op: String) -> RunError {
        match self {
            EngraveError::Strategy(e) => e.named(op),
            source => RunError::Engrave { op, source },
        }
    }
}

impl OperationError for RegionError {
    fn named(self, op: String) -> RunError {
        StrategyError::from(self).named(op)
    }
}

impl OperationError for ToolpathError {
    fn named(self, op: String) -> RunError {
        StrategyError::from(self).named(op)
    }
}

/// Name `op` in whatever its work failed with, for `map_err`.
pub(super) fn in_op<E: OperationError>(op: &Operation) -> impl Fn(E) -> RunError + '_ {
    move |source| source.named(op.name.clone())
}
