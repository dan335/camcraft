//! A job: its models loaded, its setups run, checked and posted.
//!
//! Everything between files on disk and G-code that is not engine work. Both the
//! command line and the desktop app go through here, so they cannot drift apart.
//! 0.1 fixed exactly that defect between the inspector and the post, and the app
//! must not reintroduce it:
//!
//! - which reader a file gets, and what its numbers mean ([`load_model`]),
//! - where a job's model files are ([`fs::resolve`]), and how a job is read and
//!   written ([`read_job`], [`write_job`]),
//! - the one engine path ([`run_setup`]),
//! - the one set of checks ([`checks`]) and the one verification policy
//!   ([`gate`]): no program from a setup with a failure among them,
//! - and how a program is named, rendered and written ([`plan_posting`],
//!   [`program_name`]).
//!
//! Position in the dependency DAG: depends on every library crate, and only the two
//! binaries depend on it. It says nothing about how anything is shown. It returns
//! what happened as data (`notes` and `warnings`), and each front end presents it
//! in its own way.

pub mod checks;
pub mod fs;
pub mod library;
mod post;

pub use post::{
    Posting, Program, builtin_posts, load_profile, plan_posting, program, program_name,
    write_posting,
};

use std::collections::BTreeMap;
use std::path::Path;

use camcraft_core::job::{
    Geometry, Height, HeightRef, Heights, JOB_VERSION, Job, JobError, LengthUnit, Linking,
    ModelDef, Operation, Passes, Setup, StockDef, StockSpec, StockToLeave, StrategySpec, ToolKind,
    ToolSpec,
};
use camcraft_core::run::{Made, Plan, RunError, SetupResult, Stage, run_prepared_planned};
use camcraft_core::setup::{Bodies, PrepareError, prepare};
use camcraft_geom::mesh::{Aabb, TriMesh};
use camcraft_geom::topology::{Body, FaceSource};
use camcraft_import::step::{FileCheck, StepError, load_step};
use camcraft_import::stl::{ImportError, load_stl};
use camcraft_post::op::LowerError;
use camcraft_post::profile::ProfileError;

use crate::checks::Checks;

pub use camcraft_import::stl::Units;

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
    /// The job file is from before jobs had setups.
    #[error(
        "{0} is in the old job format, from before jobs had models, stocks and \
         setups; see examples/rail-jig.camcraft for the new one"
    )]
    OldJob(String),
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
    /// The job did not verify, and nobody insisted.
    #[error("verification failed")]
    Refused,
}

/// A model, loaded and measured.
#[derive(Debug, Clone)]
pub struct Model {
    /// The triangles, in millimetres, and their faces, edges and vertices.
    pub body: Body,
    /// Their bounds.
    pub bounds: Aabb,
    /// Where it came from.
    pub path: String,
    /// What the model's numbers meant, as a person would say it.
    pub units: String,
    /// What loading did, in order, for a log.
    pub notes: Vec<String>,
    /// Anything about the import worth a person's attention.
    pub warnings: Vec<String>,
}

impl Model {
    /// The triangles, in millimetres.
    pub fn mesh(&self) -> &TriMesh {
        self.body.mesh()
    }
}

/// A label for units, as a person would write them.
pub fn units_label(units: Units) -> &'static str {
    match units {
        Units::Millimetres => "mm",
        Units::Inches => "inch",
    }
}

/// Tessellation tolerance for a B-rep, from the machining tolerance.
///
/// The simulator verifies against this mesh, so the gap between it and the true
/// surface is error nothing downstream can see. A quarter keeps it a small part of
/// the budget without making tessellation the slow step.
pub fn tessellation_tolerance(tolerance: f64) -> f64 {
    tolerance / 4.0
}

/// Read a model, choosing the reader by extension.
///
/// An STL carries no units, so `units` is required for one. A STEP file states its
/// own, and `units`, if given, must agree with it. `tolerance` is the job's
/// machining tolerance; a STEP file is tessellated to a quarter of it.
pub fn load_model(
    path: &Path,
    units: Option<Units>,
    tolerance: f64,
) -> Result<Model, SessionError> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let mut notes = Vec::new();
    let mut warnings = Vec::new();
    let (body, label) = match ext.as_str() {
        "stl" => {
            let units = units.ok_or(SessionError::StlNeedsUnits)?;
            let (mesh, import) = load_stl(path, units)?;
            if import.degenerate > 0 {
                warnings.push(format!(
                    "{} degenerate triangle(s) in the model",
                    import.degenerate
                ));
            }
            notes.push(format!(
                "loaded {} triangles, {} vertices after welding",
                import.triangles, import.vertices
            ));
            // An STL file has no faces; they are found from the triangles.
            (Body::derived(mesh), units_label(units).to_string())
        }
        "step" | "stp" => {
            let started = std::time::Instant::now();
            let (body, report) = load_step(path, tessellation_tolerance(tolerance))?;
            if let Some(said) = units
                && (said.to_mm() - report.unit.mm).abs() > 1e-9 * said.to_mm()
            {
                return Err(SessionError::UnitsDisagree {
                    said: units_label(said),
                    file: report.unit.name,
                });
            }
            if report.degenerate > 0 {
                warnings.push(format!(
                    "{} degenerate triangle(s) from tessellation",
                    report.degenerate
                ));
            }
            if let FileCheck::Skipped(why) = report.check {
                warnings.push(format!(
                    "the mesh was not checked against the file's geometry: {why}"
                ));
            }
            notes.push(format!(
                "read STEP in {}: {} face(s) all meshed, {} triangles at {} mm, checked: {}",
                report.unit.name,
                report.faces,
                report.triangles,
                report.tolerance,
                match report.check {
                    FileCheck::Passed => "yes",
                    FileCheck::Skipped(_) => "NO",
                },
            ));
            notes.push(format!(
                "tessellated in {:.1} s",
                started.elapsed().as_secs_f64()
            ));
            (body, format!("{} (from file)", report.unit.name))
        }
        _ => return Err(SessionError::UnknownFormat(path.display().to_string())),
    };
    let t = body.topology();
    notes.push(format!(
        "topology: {} face(s) {}, {} edge(s), {} vertices",
        t.faces().len(),
        match t.source() {
            FaceSource::File => "from the file",
            FaceSource::Derived => "found from the triangles",
        },
        t.edges().len(),
        t.vertices().len()
    ));

    let bounds = body.mesh().bounds().ok_or(SessionError::EmptyModel)?;
    let extent = bounds.extent();
    notes.push(format!(
        "model is {:.1} x {:.1} x {:.1} mm",
        extent.x, extent.y, extent.z
    ));
    Ok(Model {
        body,
        bounds,
        path: path.display().to_string(),
        units: label,
        notes,
        warnings,
    })
}

/// Read a job file.
pub fn read_job(path: &Path) -> Result<Job, SessionError> {
    let name = path.display().to_string();
    let text = std::fs::read_to_string(path).map_err(|source| SessionError::JobIo {
        path: name.clone(),
        source,
    })?;
    parse_job(&text, &name)
}

/// Read a job from its text; `name` says where it came from, for errors.
pub fn parse_job(text: &str, name: &str) -> Result<Job, SessionError> {
    // The version first, so an old or newer file is named for what it is rather
    // than refused over whichever field it lacks.
    #[derive(serde::Deserialize)]
    struct Probe {
        version: Option<u32>,
    }
    let parse = |source| SessionError::JobParse {
        path: name.to_string(),
        source,
    };
    match toml::from_str::<Probe>(text).map_err(parse)?.version {
        None => return Err(SessionError::OldJob(name.to_string())),
        Some(v) if !(1..=JOB_VERSION).contains(&v) => {
            return Err(JobError::UnsupportedVersion(v).into());
        }
        Some(_) => {}
    }
    // Read into this build's format. What a file is written as is decided when it
    // is written (`job_text`), from what the job holds, not from what it was read as.
    let mut job: Job = toml::from_str(text).map_err(parse)?;
    job.version = JOB_VERSION;
    Ok(job)
}

/// A job as the text of its file, at the oldest version that holds it
/// ([`Job::required_version`]): a job an older build can read in full is written
/// so it can, and one it would misread is written so it refuses it.
pub fn job_text(job: &Job) -> Result<String, SessionError> {
    let mut job = job.clone();
    job.version = job.required_version();
    let body = toml::to_string(&job)?;
    Ok(format!("# camcraft job. Written by camcraft.\n\n{body}"))
}

/// Write a job file, whole or not at all.
pub fn write_job(path: &Path, job: &Job) -> Result<(), SessionError> {
    let text = job_text(job)?;
    fs::write_atomically(path, &text).map_err(|source| SessionError::JobWrite {
        path: path.display().to_string(),
        source,
    })
}

/// What a model's units, as a job states them, mean to the reader.
pub fn units_of(unit: Option<LengthUnit>) -> Option<Units> {
    unit.map(|u| match u {
        LengthUnit::Mm => Units::Millimetres,
        LengthUnit::Inch => Units::Inches,
    })
}

/// Load one of a job's models, found relative to `job_dir`, at the job's
/// tolerance.
pub fn load_job_model(
    job_dir: &Path,
    model: &ModelDef,
    tolerance: f64,
) -> Result<Model, SessionError> {
    let path = fs::resolve(job_dir, &model.path);
    if !path.exists() {
        return Err(SessionError::ModelMissing {
            name: model.name.clone(),
            path: path.display().to_string(),
        });
    }
    load_model(&path, units_of(model.units), tolerance).map_err(|e| SessionError::InModel {
        name: model.name.clone(),
        source: Box::new(e),
    })
}

/// A job's models, loaded, by name.
pub type Models<'a> = BTreeMap<&'a str, &'a Model>;

/// Load every model the setups at `setups` use, each once however many use it,
/// found relative to `job_dir` at the job's tolerance, by name.
pub fn load_models(
    job: &Job,
    job_dir: &Path,
    setups: &[usize],
) -> Result<BTreeMap<String, Model>, SessionError> {
    let mut loaded = BTreeMap::new();
    for &i in setups {
        for m in job.models_for(i) {
            if !loaded.contains_key(&m.name) {
                loaded.insert(m.name.clone(), load_job_model(job_dir, m, job.tolerance)?);
            }
        }
    }
    Ok(loaded)
}

/// Loaded models as a run takes them.
pub fn models_of(loaded: &BTreeMap<String, Model>) -> Models<'_> {
    loaded.iter().map(|(n, m)| (n.as_str(), m)).collect()
}

/// A setup, run.
#[derive(Debug, Clone)]
pub struct Ran {
    /// What the engine produced.
    pub result: SetupResult,
    /// The part as the setup machined it: its models, merged, turned and measured
    /// from its work zero.
    pub part: TriMesh,
    /// What happened, in order, for a log.
    pub notes: Vec<String>,
    /// Everything worth a person's attention: the setup's and its models'.
    pub warnings: Vec<String>,
    /// What the checks found. The gate is "no failures here".
    pub checks: Checks,
}

/// The one engine path: setup `index` of `job`, with its models.
pub fn run_setup(job: &Job, index: usize, models: &Models<'_>) -> Result<Ran, SessionError> {
    run_setup_watched(job, index, models, &mut |_| {})
}

/// As [`run_setup`], telling `progress` each [`Stage`] as it starts.
pub fn run_setup_watched(
    job: &Job,
    index: usize,
    models: &Models<'_>,
    progress: &mut dyn FnMut(Stage),
) -> Result<Ran, SessionError> {
    run_setup_planned(job, index, models, &[], progress)
}

/// As [`run_setup_watched`], doing with each operation what `plans` says, by
/// its place: see [`run_prepared_planned`]. Only the operations that will run
/// are checked beforehand.
pub fn run_setup_planned(
    job: &Job,
    index: usize,
    models: &Models<'_>,
    plans: &[Plan],
    progress: &mut dyn FnMut(Stage),
) -> Result<Ran, SessionError> {
    // Validation happens before anything is computed, so a bad job fails on its own
    // terms rather than part-way through an operation.
    job.check_common()?;
    job.check_setup_where(index, &|i| !matches!(plans.get(i), Some(Plan::Skip)))?;
    let setup = job.setups.get(index).ok_or(JobError::NoSetups)?;
    let mut notes = vec![format!(
        "setup '{}': {} operation(s), tolerance {} mm",
        setup.name,
        setup.operations.len(),
        job.tolerance
    )];
    let bodies: Bodies<'_> = models.iter().map(|(n, m)| (*n, &m.body)).collect();
    progress(Stage::Preparing);
    let prepared = prepare(job, index, &bodies)?;
    let result = run_prepared_planned(job, index, &prepared, &bodies, plans, progress)?;
    notes.extend(summary(&result));

    let mut model_warnings = Vec::new();
    for m in job.models_for(index) {
        if let Some(model) = models.get(m.name.as_str()) {
            model_warnings.extend(model.warnings.iter().map(|w| format!("{}: {w}", m.name)));
        }
    }
    progress(Stage::Checking);
    let checks = checks::check(
        job,
        index,
        &prepared.part,
        &result.operations,
        &result.verify,
        result.verify_budget,
        &model_warnings,
    );
    let mut warnings = result.warnings();
    warnings.extend(model_warnings);
    Ok(Ran {
        result,
        part: prepared.part,
        notes,
        warnings,
        checks,
    })
}

/// A line per operation and one for the verdict.
pub fn summary(result: &SetupResult) -> Vec<String> {
    let mut out: Vec<String> = result
        .operations
        .iter()
        .map(|op| match op.made {
            Made::Skipped => format!("{}: not generated", op.name),
            Made::Disabled => format!("{}: turned off", op.name),
            Made::Generated | Made::Kept => format!(
                "{}: {} passes, removed {:.0} mm3, travel saved {:.0}%, rapid contacts {}{}",
                op.name,
                op.passes,
                op.sim.removed_mm3,
                op.travel_saved * 100.0,
                op.sim.rapid_contacts,
                if op.made == Made::Kept {
                    ", kept from before"
                } else {
                    ""
                }
            ),
        })
        .collect();
    out.push(format!(
        "verified: {} column(s) compared, gouges {}, deepest {:.3} mm, budget {:.3} mm",
        result.verify.compared,
        result.verify.gouged,
        result.verify.deepest_gouge_mm,
        result.verify_budget
    ));
    out
}

/// How a setup got past the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Passed {
    /// It verified.
    Verified,
    /// It did not, and the caller insisted.
    Forced,
}

impl Ran {
    /// Whether it may be posted: it verified, and no check fails.
    pub fn passed(&self) -> bool {
        self.result.is_clean() && self.checks.passed()
    }
}

/// The one verification policy: no program from a setup that did not verify, or
/// that any check fails, unless the caller insists.
pub fn gate(ran: &Ran, force: bool) -> Result<Passed, SessionError> {
    if ran.passed() {
        Ok(Passed::Verified)
    } else if force {
        Ok(Passed::Forced)
    } else {
        Err(SessionError::Refused)
    }
}

/// Cutter shape for a single finishing pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishTool {
    /// Ball nose.
    Ball,
    /// Square end.
    Flat,
}

/// A single parallel finishing pass, described by its numbers rather than a file.
#[derive(Debug, Clone, Copy)]
pub struct FinishSpec {
    /// Cutter shape.
    pub tool: FinishTool,
    /// Cutter diameter, in millimetres.
    pub diameter: f64,
    /// Distance between passes, in millimetres.
    pub stepover: f64,
    /// Chordal tolerance, in millimetres.
    pub tolerance: f64,
    /// Stock to leave perpendicular to the tool axis, in millimetres.
    pub stock_radial: f64,
    /// Stock to leave along the tool axis, in millimetres.
    pub stock_axial: f64,
    /// Largest Z drop that may be cut, in millimetres.
    pub max_cut_step: f64,
    /// Traverse height above the material, in millimetres.
    pub clearance: f64,
    /// Cutting feed, in mm/min.
    pub feed: f64,
    /// Work offset, 54 for G54.
    pub wcs: u8,
    /// Spindle speed.
    pub rpm: u32,
    /// Tool number.
    pub number: u32,
}

/// The one-operation job a [`FinishSpec`] describes, for `model`.
///
/// The block is the model's own bounding box, and traverses clear the material by
/// `clearance`. Retracts run at the cutting feed and plunges at a quarter of it.
/// Its one setup is called `finish`.
pub fn single_operation(spec: &FinishSpec, model: ModelDef) -> Job {
    const TOOL: &str = "cutter";
    let (kind, shape) = match spec.tool {
        FinishTool::Ball => (ToolKind::Ball, "ball"),
        FinishTool::Flat => (ToolKind::Flat, "flat"),
    };
    let tool = ToolSpec {
        kind,
        diameter: spec.diameter,
        corner: 0.0,
        number: spec.number,
        rpm: spec.rpm,
        comment: format!("{}mm {shape}", spec.diameter),
        ..ToolSpec::default()
    };
    let mut setup = Setup::new("finish");
    setup.models = vec![model.name.clone()];
    setup.stocks = vec!["block".into()];
    setup.wcs = spec.wcs;
    setup.operations = vec![Operation {
        name: "finish".into(),
        strategy: StrategySpec::Parallel { angle: 0.0 },
        tool: Some(TOOL.into()),
        preset: None,
        geometry: Geometry::default(),
        heights: Heights {
            clearance: Height {
                reference: HeightRef::ModelTop,
                offset: spec.clearance,
            },
        },
        passes: Passes {
            stepover: spec.stepover,
            feed: spec.feed,
            stock_to_leave: StockToLeave {
                radial: spec.stock_radial,
                axial: spec.stock_axial,
            },
            max_cut_step: spec.max_cut_step,
            rpm: None,
        },
        linking: Linking {
            retract_feed: spec.feed,
            plunge_feed: spec.feed / 4.0,
            ..Linking::default()
        },
        enabled: true,
    }];
    Job {
        tolerance: spec.tolerance,
        tools: BTreeMap::from([(TOOL.to_string(), tool)]),
        models: vec![model],
        stocks: vec![StockDef {
            name: "block".into(),
            spec: StockSpec::AroundModels {
                models: Vec::new(),
                margin: [0.0; 3],
                below: 0.0,
            },
        }],
        setups: vec![setup],
        ..Job::default()
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

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/step")
            .join(name)
    }

    #[test]
    fn the_reader_is_chosen_by_extension() {
        let err = load_model(Path::new("part.obj"), Some(Units::Millimetres), 0.05);
        assert!(matches!(err, Err(SessionError::UnknownFormat(_))));
    }

    #[test]
    fn an_stl_without_units_is_refused_before_it_is_read() {
        // The file does not exist: refusing must not depend on reading it.
        let err = load_model(Path::new("does-not-exist.stl"), None, 0.05);
        assert!(matches!(err, Err(SessionError::StlNeedsUnits)));
    }

    #[test]
    fn step_units_that_contradict_the_file_are_refused() {
        let err = load_model(&fixture("hinge_jig.step"), Some(Units::Millimetres), 0.05);
        match err {
            Err(SessionError::UnitsDisagree { said, file }) => {
                assert_eq!(said, "mm");
                assert_eq!(file, "inch");
            }
            other => panic!("expected a disagreement, got {other:?}"),
        }
    }

    #[test]
    fn step_units_that_agree_are_accepted_and_the_file_decides() {
        let m = load_model(&fixture("hinge_jig.step"), Some(Units::Inches), 0.05).unwrap();
        assert_eq!(m.units, "inch (from file)");
        let m = load_model(&fixture("hinge_jig.step"), None, 0.05).unwrap();
        assert!((m.bounds.extent().x - 25.4).abs() < 0.1);
        assert!(m.notes.iter().any(|n| n.starts_with("read STEP in inch")));
    }

    fn hinge() -> ModelDef {
        ModelDef {
            name: "hinge".into(),
            path: fixture("hinge_jig.step").display().to_string(),
            units: None,
        }
    }

    fn small_job() -> Job {
        single_operation(
            &FinishSpec {
                tool: FinishTool::Ball,
                diameter: 6.0,
                stepover: 2.0,
                tolerance: 0.05,
                stock_radial: 0.0,
                stock_axial: 0.0,
                max_cut_step: 0.5,
                clearance: 5.0,
                feed: 1200.0,
                wcs: 54,
                rpm: 18000,
                number: 1,
            },
            hinge(),
        )
    }

    pub(crate) fn run_small() -> Ran {
        let job = small_job();
        let model = load_job_model(Path::new("."), &job.models[0], job.tolerance).unwrap();
        let models: Models<'_> = [("hinge", &model)].into_iter().collect();
        run_setup(&job, 0, &models).unwrap()
    }

    /// One operation generated alone, the other kept: the kept one is cut into
    /// the material again, so the result is what generating both gives. One
    /// skipped or turned off has no paths and cuts nothing, and only what runs
    /// is checked, so a skipped operation may be unfinished.
    #[test]
    fn operations_are_kept_skipped_and_turned_off() {
        let mut job = small_job();
        let mut second = job.setups[0].operations[0].clone();
        second.name = "across".into();
        second.strategy = StrategySpec::Parallel { angle: 90.0 };
        job.setups[0].operations.push(second);
        let model = load_job_model(Path::new("."), &job.models[0], job.tolerance).unwrap();
        let models: Models<'_> = [("hinge", &model)].into_iter().collect();
        let final_heights = |ran: &Ran| {
            let last = ran.result.keyframes.last().unwrap().unpack().unwrap();
            last.field().data().to_vec()
        };

        let both = run_setup(&job, 0, &models).unwrap();
        assert!(
            both.result
                .operations
                .iter()
                .all(|o| o.made == Made::Generated)
        );

        let first = Box::new(both.result.operations[0].clone());
        let plans = [Plan::Keep(first), Plan::Generate];
        let kept = run_setup_planned(&job, 0, &models, &plans, &mut |_| {}).unwrap();
        assert_eq!(kept.result.operations[0].made, Made::Kept);
        assert_eq!(kept.result.operations[1].made, Made::Generated);
        assert_eq!(
            kept.result.operations[1].path.moves(),
            both.result.operations[1].path.moves()
        );
        assert_eq!(final_heights(&kept), final_heights(&both));

        // Skipped: nothing of it, and it need not be finished to run the other.
        job.setups[0].operations[0].tool = None;
        let plans = [Plan::Skip, Plan::Generate];
        let skipped = run_setup_planned(&job, 0, &models, &plans, &mut |_| {}).unwrap();
        assert_eq!(skipped.result.operations[0].made, Made::Skipped);
        assert!(skipped.result.operations[0].path.is_empty());
        assert_eq!(skipped.result.operations[0].sim.removed_mm3, 0.0);
        assert!(skipped.checks.groups.iter().all(|g| g.op != Some(0)));
        assert!(
            run_setup(&job, 0, &models).is_err(),
            "unfinished, it cannot run"
        );

        // Turned off: left out of any run, whatever the plan.
        job.setups[0].operations[0].enabled = false;
        let off = run_setup(&job, 0, &models).unwrap();
        assert_eq!(off.result.operations[0].made, Made::Disabled);
        assert!(off.result.operations[0].path.is_empty());
        assert_eq!(off.result.operations[1].made, Made::Generated);
    }

    /// A real part, finished only on its highest flat face, with its work zero on
    /// a corner of that face: it runs and verifies clean, every cut keeps the
    /// tool's centre over the face, the corner is the setup's zero, and the job
    /// is written as version 2 and read back the same.
    #[test]
    fn a_real_part_kept_to_a_picked_face() {
        use camcraft_core::job::{FeatureRef, Origin, PointRef, Snap};
        use camcraft_core::toolpath::MoveRole;
        use camcraft_geom::topology::FaceKind;
        let mut job = small_job();
        let model = load_job_model(Path::new("."), &job.models[0], job.tolerance).unwrap();
        let (mesh, t) = (model.mesh(), model.body.topology());
        let (face, _) = t
            .faces()
            .iter()
            .enumerate()
            .filter_map(|(i, f)| match f.kind {
                FaceKind::Plane { normal, offset } if normal.z > 0.99 && f.area > 20.0 => {
                    Some((u32::try_from(i).ok()?, offset))
                }
                _ => None,
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        let f = t.face(face).unwrap();
        let tri = mesh.triangle(f.triangles[0] as usize).unwrap();
        let at = (tri[0] + tri[1] + tri[2]) / 3.0;
        job.setups[0].operations[0].geometry.faces.push(FeatureRef {
            model: "hinge".into(),
            index: face,
            at: at.to_array(),
        });
        let corner = t
            .edge(f.edges[0])
            .and_then(|e| e.ends)
            .map(|[a, _]| a)
            .unwrap();
        let position = t.vertex(corner).unwrap().position;
        job.setups[0].origin = Origin::Point {
            point: PointRef {
                model: "hinge".into(),
                snap: Snap::Vertex,
                index: corner,
                at: position.to_array(),
            },
            offset: [0.0; 3],
        };
        job.validate().unwrap();

        let models: Models<'_> = [("hinge", &model)].into_iter().collect();
        let ran = run_setup(&job, 0, &models).unwrap();
        assert!(ran.result.is_clean(), "{:?}", ran.warnings);
        let frame = ran.result.frame;
        assert!(frame.to_setup(position).length() < 1e-9);

        // The face's extent in the setup, which every cut must keep over.
        let (lo, hi) = f
            .triangles
            .iter()
            .filter_map(|&i| mesh.triangle(i as usize))
            .flatten()
            .map(|p| frame.to_setup(p).truncate())
            .fold(
                (glam::DVec2::splat(f64::MAX), glam::DVec2::splat(f64::MIN)),
                |(lo, hi), p| (lo.min(p), hi.max(p)),
            );
        let cuts: Vec<_> = ran.result.operations[0]
            .path
            .moves()
            .iter()
            .filter(|m| m.role == MoveRole::Cut)
            .map(|m| m.to.truncate())
            .collect();
        assert!(cuts.len() > 10, "only {} cuts", cuts.len());
        for p in cuts {
            assert!(
                p.x >= lo.x - 0.03
                    && p.x <= hi.x + 0.03
                    && p.y >= lo.y - 0.03
                    && p.y <= hi.y + 0.03,
                "a cut at {p} leaves the face ({lo} to {hi})"
            );
        }

        let text = job_text(&job).unwrap();
        assert!(
            text.contains(
                "version = 2
"
            ),
            "{text}"
        );
        assert_eq!(parse_job(&text, "picked").unwrap(), job);
    }

    /// The gate is the one policy both front ends share: a clean job passes, and
    /// an unclean one passes only when forced.
    #[test]
    fn the_gate_refuses_an_unverified_job_unless_forced() {
        let mut ran = run_small();
        assert_eq!(gate(&ran, false).unwrap(), Passed::Verified);

        // Make it unclean the way the simulator would: a rapid through material.
        let op = ran.result.operations.first_mut().unwrap();
        op.sim.rapid_contacts = 1;
        assert!(matches!(gate(&ran, false), Err(SessionError::Refused)));
        assert_eq!(gate(&ran, true).unwrap(), Passed::Forced);
    }

    /// Re-check a run after its first operation's path has been tampered with.
    fn recheck(ran: &Ran, tamper: impl FnOnce(&mut camcraft_core::toolpath::Toolpath)) -> Checks {
        let job = small_job();
        let mut ops = ran.result.operations.clone();
        tamper(&mut ops[0].path);
        checks::check(
            &job,
            0,
            &ran.part,
            &ops,
            &ran.result.verify,
            ran.result.verify_budget,
            &[],
        )
    }

    fn failed(checks: &Checks) -> Vec<&'static str> {
        checks.failures().map(|(_, f)| f.code).collect()
    }

    /// A clean run passes every failure check, and says so check by check.
    #[test]
    fn a_clean_run_passes_its_checks() {
        let ran = run_small();
        assert!(ran.checks.passed(), "{:?}", ran.checks.failure_lines());
        let passed: Vec<&str> = ran
            .checks
            .groups
            .iter()
            .flat_map(|g| &g.findings)
            .filter(|f| f.level == checks::Level::Pass)
            .map(|f| f.code)
            .collect();
        for code in [
            "CC101", "CC102", "CC103", "CC104", "CC105", "CC106", "CC107",
        ] {
            assert!(passed.contains(&code), "{code} not reported as passed");
        }
    }

    /// Each failure the checks exist for is found, and found at its move, from the
    /// path alone: the engine's own reports still say all is well.
    #[test]
    fn the_checks_find_what_is_wrong_with_a_path() {
        use camcraft_core::toolpath::MoveRole;
        use glam::dvec3;
        let ran = run_small();
        let b = ran.part.bounds().unwrap();
        let mid = b.center();

        // A rapid straight through the middle of the part.
        let c = recheck(&ran, |p| {
            p.rapid_to(dvec3(b.min.x - 10.0, mid.y, mid.z)).unwrap();
            p.rapid_to(dvec3(b.max.x + 10.0, mid.y, mid.z)).unwrap();
        });
        assert_eq!(failed(&c), ["CC102"]);
        // At one of the two added rapids: the first also cuts through the part on
        // its way down.
        let at = c.failures().next().unwrap().1.at_move.unwrap();
        assert!(at >= ran.result.operations[0].path.len(), "{at}");

        // A plunge that travels sideways.
        let c = recheck(&ran, |p| {
            let at = p.moves().last().unwrap().to;
            p.feed_to(at + dvec3(3.0, 0.0, -1.0), MoveRole::Plunge, 300.0)
                .unwrap();
        });
        assert_eq!(failed(&c), ["CC103"]);

        // A hole the linker never filled.
        let c = recheck(&ran, |p| {
            let at = p.moves().last().unwrap().to;
            p.feed_to(at + dvec3(1.0, 0.0, 0.0), MoveRole::Link, 300.0)
                .unwrap();
        });
        assert_eq!(failed(&c), ["CC101"]);

        // A plunge at cutting feed, in short moves.
        let c = recheck(&ran, |p| {
            let at = p.moves().last().unwrap().to;
            for k in 1..=5 {
                let k = f64::from(k);
                p.feed_to(at + dvec3(0.001 * k, 0.0, -0.3 * k), MoveRole::Cut, 300.0)
                    .unwrap();
            }
        });
        assert_eq!(failed(&c), ["CC105"]);
    }

    /// The gate is "no failures": a path the engine is happy with, but the checks
    /// are not, is refused.
    #[test]
    fn a_failed_check_closes_the_gate() {
        use camcraft_core::toolpath::MoveRole;
        use glam::dvec3;
        let mut ran = run_small();
        assert!(ran.result.is_clean());
        ran.checks = recheck(&ran, |p| {
            let at = p.moves().last().unwrap().to;
            p.feed_to(at + dvec3(3.0, 0.0, -1.0), MoveRole::Plunge, 300.0)
                .unwrap();
        });
        assert!(matches!(gate(&ran, false), Err(SessionError::Refused)));
        assert_eq!(gate(&ran, true).unwrap(), Passed::Forced);
    }

    /// The example job reads, and a job written and read back is the same job.
    #[test]
    fn jobs_read_and_write() {
        let example =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/rail-jig.camcraft");
        let job = read_job(&example).unwrap();
        job.validate().unwrap();
        let dir = std::env::temp_dir().join(format!("camcraft-session-job-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("again.camcraft");
        write_job(&path, &job).unwrap();
        assert_eq!(read_job(&path).unwrap(), job);
    }

    /// A job from before setups is named as that, not refused over a field.
    #[test]
    fn an_old_job_is_named_as_one() {
        let old = "tolerance = 0.05\n[setup]\nstock = { kind = \"box\" }\n";
        assert!(matches!(
            parse_job(old, "old.toml"),
            Err(SessionError::OldJob(_))
        ));
        assert!(matches!(
            parse_job("version = 9\n", "new.camcraft"),
            Err(SessionError::Job(JobError::UnsupportedVersion(9)))
        ));
    }

    /// A job is written at the oldest version that holds it: an older build reads
    /// a job without picks as before, and refuses one with picks by name rather
    /// than cutting the whole part where one face was asked for.
    #[test]
    fn a_job_is_written_at_the_version_it_needs() {
        let example =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/rail-jig.camcraft");
        let mut job = read_job(&example).unwrap();
        let text = job_text(&job).unwrap();
        assert!(
            text.contains(
                "version = 1
"
            ),
            "{text}"
        );
        assert_eq!(parse_job(&text, "one").unwrap(), job);

        let model = job.setups[0].models[0].clone();
        job.setups[0].operations[0]
            .geometry
            .faces
            .push(camcraft_core::job::FeatureRef {
                model,
                index: 0,
                at: [0.0, 0.0, 0.0],
            });
        let text = job_text(&job).unwrap();
        assert!(
            text.contains(
                "version = 2
"
            ),
            "{text}"
        );
        assert_eq!(parse_job(&text, "two").unwrap(), job);
    }

    /// A model that is not where the job says is named, with where it was looked
    /// for.
    #[test]
    fn a_missing_model_is_named() {
        let m = ModelDef {
            name: "gone".into(),
            path: "nowhere/part.step".into(),
            units: None,
        };
        match load_job_model(Path::new("/tmp"), &m, 0.05) {
            Err(SessionError::ModelMissing { name, path }) => {
                assert_eq!(name, "gone");
                assert!(path.ends_with("nowhere/part.step"), "{path}");
            }
            other => panic!("expected a missing model, got {other:?}"),
        }
    }
}
