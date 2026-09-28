//! What stops a session before it produces anything.

use camcraft_core::job::JobError;
use camcraft_core::run::RunError;
use camcraft_core::setup::PrepareError;
use camcraft_import::step::StepError;
use camcraft_import::stl::ImportError;
use camcraft_post::op::LowerError;
use camcraft_post::profile::ProfileError;

/// Something that stopped a session before it produced anything.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// An STL file was given without saying what its numbers mean.
    #[error(
        "an STL file carries no units: say whether it is in millimetres or inches. \
         Guessing would make an inch part 25.4 times too small."
    )]
    StlNeedsUnits,
    /// The caller stated units that contradict what a STEP file declares.
    #[error(
        "{said} was given, but the file is in {file}. A STEP file states its own \
         units; check this is the right file."
    )]
    UnitsDisagree {
        /// What the caller said, as a label.
        said: &'static str,
        /// What the file declares.
        file: String,
    },
    /// Not a format this can read.
    #[error("{0}: not a model this can read; expected .stl, .step or .stp")]
    UnknownFormat(String),
    /// The STL could not be read.
    #[error(transparent)]
    Stl(#[from] ImportError),
    /// The STEP file could not be read or did not check out.
    #[error(transparent)]
    Step(#[from] StepError),
    /// The model had nothing in it.
    #[error("model has no geometry")]
    EmptyModel,
    /// The model could not be placed where the job says.
    #[error("the model cannot be placed there: {0}")]
    Placement(#[from] camcraft_geom::mesh::MeshError),
    /// The job file could not be read.
    #[error("cannot read job {path}: {source}")]
    JobIo {
        /// Which file.
        path: String,
        /// Why.
        source: std::io::Error,
    },
    /// The job file was not valid TOML for a job.
    #[error("job {path} is not valid: {source}")]
    JobParse {
        /// Which file.
        path: String,
        /// Why.
        source: toml::de::Error,
    },
    /// The job could not be written.
    #[error("cannot write job {path}: {source}")]
    JobWrite {
        /// Which file.
        path: String,
        /// Why.
        source: std::io::Error,
    },
    /// The job could not be put into words.
    #[error("cannot write the job: {0}")]
    JobSerialize(#[from] toml::ser::Error),
    /// A job's model file is not where the job says.
    #[error("model '{name}' is not at {path}")]
    ModelMissing {
        /// The model's name in the job.
        name: String,
        /// Where it was looked for.
        path: String,
    },
    /// A job's model could not be loaded.
    #[error("model '{name}': {source}")]
    InModel {
        /// The model's name in the job.
        name: String,
        /// Why.
        source: Box<SessionError>,
    },
    /// A setup's models and stock could not be laid out.
    #[error(transparent)]
    Prepare(#[from] PrepareError),
    /// The job described something that cannot be run.
    #[error(transparent)]
    Job(#[from] JobError),
    /// The engine could not run the job.
    #[error(transparent)]
    Run(#[from] RunError),
    /// The machine profile could not be loaded.
    #[error(transparent)]
    Profile(#[from] ProfileError),
    /// A path could not be lowered to instructions.
    #[error(transparent)]
    Lower(#[from] LowerError),
    /// Something went wrong posting one setup.
    #[error("setup '{setup}': {source}")]
    InSetup {
        /// The setup's name.
        setup: String,
        /// Why.
        source: Box<SessionError>,
    },
    /// A program, or the folder for it, could not be written.
    #[error("cannot write {path}: {source}")]
    ProgramWrite {
        /// Which file or folder.
        path: String,
        /// Why.
        source: std::io::Error,
    },
    /// Two programs of one posting would be written to the same file: their
    /// names differ only in what a file name cannot hold, or in case.
    #[error("{first} and {second} would both be written to {path}: rename one")]
    SameProgram {
        /// The file.
        path: String,
        /// The one written first.
        first: String,
        /// The one that would replace it.
        second: String,
    },
    /// The job did not verify, and nobody insisted.
    #[error("verification failed")]
    Refused,
}
