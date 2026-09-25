//! Why a job cannot be run as written.

use super::{JOB_VERSION, ToolFieldError};
use crate::frame::BadOrientation;
use crate::tool::ToolError;

/// A job that cannot be run as written.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum JobError {
    /// The file is a format this build does not read.
    #[error("job format {0} is not one this version reads (it reads 1 to {JOB_VERSION})")]
    UnsupportedVersion(u32),
    /// Something was left without a name.
    #[error("a {what} has no name")]
    NoName {
        /// What kind of thing.
        what: &'static str,
    },
    /// Two things of one kind share a name, so a reference to it is ambiguous.
    #[error("there are two {what}s called '{name}'")]
    DuplicateName {
        /// What kind of thing.
        what: &'static str,
        /// The name.
        name: String,
    },
    /// A setup or a stock names a model the job does not have.
    #[error("{owner} uses model '{model}', which is not in the job")]
    UnknownModel {
        /// Who asked for it.
        owner: String,
        /// Which model.
        model: String,
    },
    /// A setup names a stock the job does not have.
    #[error("setup '{setup}' uses stock '{stock}', which is not in the job")]
    UnknownStock {
        /// Which setup.
        setup: String,
        /// Which stock.
        stock: String,
    },
    /// A setup machines no model.
    #[error("setup '{0}' has no model")]
    NoModels(String),
    /// A setup has no stock.
    #[error("setup '{0}' has no stock")]
    NoStock(String),
    /// The job has no setups.
    #[error("job has no setups")]
    NoSetups,
    /// A setup would do nothing.
    #[error("setup '{0}' has no operations")]
    NoOperations(String),
    /// A setup's axes are not square to each other.
    #[error("setup '{setup}': {source}")]
    BadOrientation {
        /// Which setup.
        setup: String,
        /// Why.
        source: BadOrientation,
    },
    /// An operation's tool cannot do what the operation asks of it.
    #[error("operation '{op}' cannot use tool '{tool}': {why}")]
    WrongTool {
        /// Which operation, as `setup/operation`.
        op: String,
        /// The tool.
        tool: String,
        /// Why not.
        why: String,
    },
    /// An operation has no tool yet.
    #[error("operation '{op}' has no tool")]
    NoTool {
        /// Which operation, as `setup/operation`.
        op: String,
    },
    /// An operation named a tool the job does not have.
    #[error("operation '{op}' uses tool '{tool}', which is not in the job's tools")]
    UnknownTool {
        /// Which operation, as `setup/operation`.
        op: String,
        /// Which tool it asked for.
        tool: String,
    },
    /// A tool dimension was not physically meaningful.
    #[error("tool '{name}': {source}")]
    BadTool {
        /// Which tool.
        name: String,
        /// Why.
        source: ToolError,
    },
    /// A value was outside its usable range.
    #[error("{field} must be finite and greater than zero, got {value}")]
    BadValue {
        /// Which field.
        field: String,
        /// What it was.
        value: f64,
    },
    /// A work offset outside the usual range.
    #[error("setup '{setup}': work offset {wcs} is not in the range 54..=59")]
    BadWorkOffset {
        /// Which setup.
        setup: String,
        /// The offset.
        wcs: u8,
    },
    /// A tool field that is not physically meaningful.
    #[error("tool '{name}', {source}")]
    BadToolField {
        /// Which tool.
        name: String,
        /// What is wrong.
        source: ToolFieldError,
    },
    /// A pick names a model the setup does not machine.
    #[error("{owner} was picked on model '{model}', which setup '{setup}' does not machine")]
    PickedFromOtherModel {
        /// Which setup.
        setup: String,
        /// What it was picked for.
        owner: String,
        /// The model it names.
        model: String,
    },
    /// A pick's point is not a point.
    #[error("{owner}: a picked point is not finite: {at:?}")]
    BadPick {
        /// What it was picked for.
        owner: String,
        /// The point.
        at: [f64; 3],
    },
    /// Two different tools the job uses share one tool number. The machine would
    /// load one cutter and run the other's program.
    #[error(
        "tools '{first}' and '{second}' are both T{number}; the machine would load one \
         and cut with the other's program"
    )]
    DuplicateToolNumber {
        /// The number.
        number: u32,
        /// One tool.
        first: String,
        /// The other.
        second: String,
    },
}
