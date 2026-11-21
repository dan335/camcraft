//! Why a job cannot be run as written.

use super::{JOB_VERSION, MOST_COPIES, ToolFieldError};
use crate::setup::frame::BadOrientation;

/// A job that cannot be run as written.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum JobError {
    /// The file is a format this build does not read.
    #[error("job format {0} is not this build's ({JOB_VERSION})")]
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
    /// A setup names a machine the job does not have.
    #[error("setup '{setup}' is cut on machine '{machine}', which is not in the job")]
    UnknownMachine {
        /// Which setup.
        setup: String,
        /// Which machine.
        machine: String,
    },
    /// A machine's spindle nose has a segment that is not physically meaningful.
    #[error("machine '{machine}': nose segment {segment} needs positive diameters and a length")]
    BadNose {
        /// Which machine.
        machine: String,
        /// Which segment, from one.
        segment: usize,
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
    /// An operation's height is measured from something it cannot be.
    #[error("operation '{op}': {why}")]
    BadHeight {
        /// Which operation, as `setup/operation`.
        op: String,
        /// Why not.
        why: &'static str,
    },
    /// An operation has no tool yet.
    #[error("operation '{op}' has no tool")]
    NoTool {
        /// Which operation, as `setup/operation`.
        op: String,
    },
    /// An operation's strategy cuts in levels, and it has no stepdown.
    #[error("operation '{op}' cuts in levels, and has no stepdown")]
    NoStepdown {
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
    /// A setting's expression could not be worked out.
    #[error("operation '{operation}': {setting}'s expression cannot be worked out: {message}")]
    BadExpression {
        /// Which operation, as `setup/operation`.
        operation: String,
        /// The setting, by its place (`passes.stepover`).
        setting: String,
        /// Why.
        message: String,
    },
    /// A value was outside its usable range.
    #[error("{field} must be {range}, got {value}")]
    BadValue {
        /// Which field.
        field: String,
        /// What it was.
        value: f64,
        /// What it must be: finite and greater than zero, say.
        range: &'static str,
    },
    /// A choice the operation's strategy does not offer.
    #[error("{field} is '{value}', which {strategy} does not offer")]
    NotOffered {
        /// Which field.
        field: String,
        /// What it was given.
        value: &'static str,
        /// The strategy, as a job file names it.
        strategy: &'static str,
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
    /// A surface group with no faces machines or avoids nothing, and is
    /// surely a group still being made.
    #[error("operation '{op}': surface group {group} has no faces")]
    EmptyGroup {
        /// Which operation, as `setup/operation`.
        op: String,
        /// Which group, from one.
        group: usize,
    },
    /// The model is avoided and no group is machined: there is nothing to
    /// cut.
    #[error(
        "operation '{op}': the model is avoided and no surface group is machined, so there is \
         nothing to cut"
    )]
    NothingToMachine {
        /// Which operation, as `setup/operation`.
        op: String,
    },
    /// A sheet lists one model twice: it is one part, which says how many
    /// copies of it to lay.
    #[error("stock '{stock}' lists model '{model}' twice: give the one part more copies")]
    RepeatedPart {
        /// The sheet.
        stock: String,
        /// The model.
        model: String,
    },
    /// A sheet asks for no copy of a part, or for more than a sheet lays.
    #[error(
        "stock '{stock}' asks for {copies} copies of '{model}': a part is from 1 to {} copies",
        MOST_COPIES
    )]
    Copies {
        /// The sheet.
        stock: String,
        /// The model.
        model: String,
        /// How many it asks for.
        copies: u32,
    },
    /// A setup's work zero is picked on a model its sheet lays more than
    /// once: the pick is on every copy, so it is no one place, and which copy
    /// the packer laid first is no reason to choose it.
    #[error(
        "setup '{setup}': the work zero is picked on '{model}', which stock '{stock}' lays \
         {copies} copies of, so it is no one place: pick it on the stock, or on a model that \
         stands once"
    )]
    ZeroOnCopies {
        /// The setup.
        setup: String,
        /// The model.
        model: String,
        /// How many copies its sheet lays.
        copies: u32,
        /// The sheet.
        stock: String,
    },
    /// A Morph's curves are picked on a model its sheet lays more than once:
    /// a Morph lays its passes between two curves, and picked on several
    /// copies they are two on each.
    #[error(
        "operation '{op}': Morph cannot yet morph across copies, and its curves are picked on \
         '{model}', which stock '{stock}' lays {copies} copies of: give the part one copy, or \
         make each copy a model of its own with a Morph of its own"
    )]
    MorphOnCopies {
        /// Which operation, as `setup/operation`.
        op: String,
        /// The model.
        model: String,
        /// How many copies its sheet lays.
        copies: u32,
        /// The sheet.
        stock: String,
    },
    /// Two sheets arrange one model, which can stand on only one.
    #[error(
        "model '{model}' is on stock '{first}' and on stock '{second}': it can be on one sheet"
    )]
    OnTwoSheets {
        /// The model.
        model: String,
        /// The sheet that lists it first.
        first: String,
        /// The other.
        second: String,
    },
    /// A model a sheet arranges has a position of its own, which would do
    /// nothing: the sheet places it.
    #[error(
        "model '{model}' has a position, {position:?}, which does nothing: stock '{stock}' \
         places it; its position must be zero"
    )]
    PlacedBySheet {
        /// The model.
        model: String,
        /// Its position.
        position: [f64; 3],
        /// The sheet that arranges it.
        stock: String,
    },
    /// A setup machines a model a sheet arranges, without that sheet: the
    /// model stands only on it.
    #[error(
        "setup '{setup}' machines '{model}', which stock '{stock}' arranges, and does not use \
         that stock"
    )]
    SheetNotUsed {
        /// The setup.
        setup: String,
        /// The model.
        model: String,
        /// The sheet.
        stock: String,
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
