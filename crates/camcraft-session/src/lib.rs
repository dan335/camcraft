//! One model and one job: loaded, run, checked and posted.
//!
//! Everything between files on disk and G-code that is not engine work. Both the
//! command line and the desktop app go through here, so they cannot drift apart.
//! 0.1 fixed exactly that defect between the inspector and the post, and the app
//! must not reintroduce it:
//!
//! - which reader a file gets, and what its numbers mean ([`load_model`]),
//! - the one engine path ([`run`]),
//! - the one verification policy ([`gate`]),
//! - and how a program is named and rendered ([`program`], [`program_name`]).
//!
//! Position in the dependency DAG: depends on every library crate, and only the two
//! binaries depend on it. It says nothing about how anything is shown. It returns
//! what happened as data (`notes` and `warnings`), and each front end presents it
//! in its own way.

pub mod fs;
pub mod inspection;
pub mod library;

use std::collections::BTreeMap;
use std::path::Path;

use camcraft_core::job::{
    Geometry, Height, HeightRef, Heights, Job, JobError, Linking, Operation, Passes, Setup,
    StockSpec, StockToLeave, StrategySpec, ToolKind, ToolSpec,
};
use camcraft_core::run::{JobResult, OperationResult, RunError, run_job};
use camcraft_geom::mesh::{Aabb, TriMesh};
use camcraft_import::step::{FileCheck, StepError, load_step};
use camcraft_import::stl::{ImportError, load_stl};
use camcraft_post::op::{LowerError, ProgramSpec, Spin, Units as PostUnits, lower};
use camcraft_post::profile::{Profile, ProfileError};
use camcraft_post::render::{RenderReport, render};

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
    /// The job did not verify, and nobody insisted.
    #[error("verification failed")]
    Refused,
}

/// A model, loaded and measured.
#[derive(Debug, Clone)]
pub struct Model {
    /// The triangles, in millimetres.
    pub mesh: TriMesh,
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
    let (mesh, label) = match ext.as_str() {
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
            (mesh, units_label(units).to_string())
        }
        "step" | "stp" => {
            let started = std::time::Instant::now();
            let (mesh, report) = load_step(path, tessellation_tolerance(tolerance))?;
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
            (mesh, format!("{} (from file)", report.unit.name))
        }
        _ => return Err(SessionError::UnknownFormat(path.display().to_string())),
    };

    let bounds = mesh.bounds().ok_or(SessionError::EmptyModel)?;
    let extent = bounds.extent();
    notes.push(format!(
        "model is {:.1} x {:.1} x {:.1} mm",
        extent.x, extent.y, extent.z
    ));
    Ok(Model {
        mesh,
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
    toml::from_str(&text).map_err(|source| SessionError::JobParse { path: name, source })
}

/// A job, run.
#[derive(Debug, Clone)]
pub struct Ran {
    /// What the engine produced.
    pub result: JobResult,
    /// What happened, in order, for a log.
    pub notes: Vec<String>,
    /// Everything worth a person's attention: the job's and the model's.
    pub warnings: Vec<String>,
}

/// The one engine path.
pub fn run(model: &Model, job: &Job) -> Result<Ran, SessionError> {
    // Validation happens before anything is computed, so a bad job fails on its own
    // terms rather than part-way through an operation.
    job.validate()?;
    let mut notes = vec![format!(
        "job: {} operation(s), tolerance {} mm",
        job.operations.len(),
        job.tolerance
    )];
    let result = run_job(job, &model.mesh)?;
    notes.extend(summary(&result));

    let mut warnings = result.warnings();
    warnings.extend(model.warnings.iter().cloned());
    Ok(Ran {
        result,
        notes,
        warnings,
    })
}

/// A line per operation and one for the verdict.
pub fn summary(result: &JobResult) -> Vec<String> {
    let mut out: Vec<String> = result
        .operations
        .iter()
        .map(|op| {
            format!(
                "{}: {} passes, removed {:.0} mm3, travel saved {:.0}%, rapid contacts {}",
                op.name,
                op.passes,
                op.sim.removed_mm3,
                op.travel_saved * 100.0,
                op.sim.rapid_contacts
            )
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

/// How a job got past the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Passed {
    /// It verified.
    Verified,
    /// It did not, and the caller insisted.
    Forced,
}

/// The one verification policy: no program from a job that did not verify, unless
/// the caller insists.
pub fn gate(ran: &Ran, force: bool) -> Result<Passed, SessionError> {
    if ran.result.is_clean() {
        Ok(Passed::Verified)
    } else if force {
        Ok(Passed::Forced)
    } else {
        Err(SessionError::Refused)
    }
}

/// A builtin machine profile by name, or a TOML profile by path.
pub fn load_profile(name: &str) -> Result<Profile, SessionError> {
    Ok(match Profile::builtin(name) {
        Some(p) => p?,
        None => Profile::load(name)?,
    })
}

/// The program name for the `index`-th operation (from zero): `01-rough`.
///
/// The name is kept usable as a file name.
pub fn program_name(index: usize, op_name: &str) -> String {
    let clean: String = op_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("{:02}-{clean}", index + 1)
}

/// Render one operation as a program for `profile`.
pub fn program(
    op: &OperationResult,
    wcs: u8,
    profile: &Profile,
    name: &str,
) -> Result<(String, RenderReport), SessionError> {
    let spec = ProgramSpec {
        name: name.to_string(),
        op_name: op.name.clone(),
        strategy: op.strategy.to_string(),
        tool: op.tool_number,
        tool_comment: op.tool_comment.clone(),
        wcs,
        rpm: op.rpm,
        spin: Spin::Clockwise,
        coolant: None,
        units: PostUnits::Millimetres,
    };
    let ops = lower(&op.path, &spec)?;
    Ok(render(&ops, profile, name))
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

/// The one-operation job a [`FinishSpec`] describes.
///
/// The block is the model's own bounding box, and traverses clear the material by
/// `clearance`. Retracts run at the cutting feed and plunges at a quarter of it.
pub fn single_operation(spec: &FinishSpec) -> Job {
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
    Job {
        tolerance: spec.tolerance,
        setup: Setup {
            stock: StockSpec::Box { margin: [0.0; 3] },
            wcs: spec.wcs,
        },
        tools: BTreeMap::from([(TOOL.to_string(), tool)]),
        operations: vec![Operation {
            name: "finish".into(),
            strategy: StrategySpec::Parallel { angle: 0.0 },
            tool: TOOL.into(),
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
            },
            linking: Linking {
                retract_feed: spec.feed,
                plunge_feed: spec.feed / 4.0,
                ..Linking::default()
            },
        }],
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::float_cmp)]
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

    #[test]
    fn program_names_are_numbered_and_file_safe() {
        assert_eq!(program_name(0, "rough"), "01-rough");
        assert_eq!(program_name(11, "finish / walls"), "12-finish---walls");
    }

    fn small_job() -> Job {
        single_operation(&FinishSpec {
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
        })
    }

    /// The gate is the one policy both front ends share: a clean job passes, and
    /// an unclean one passes only when forced.
    #[test]
    fn the_gate_refuses_an_unverified_job_unless_forced() {
        let model = load_model(&fixture("hinge_jig.step"), None, 0.05).unwrap();
        let mut ran = run(&model, &small_job()).unwrap();
        assert_eq!(gate(&ran, false).unwrap(), Passed::Verified);

        // Make it unclean the way the simulator would: a rapid through material.
        let op = ran.result.operations.first_mut().unwrap();
        op.sim.rapid_contacts = 1;
        assert!(matches!(gate(&ran, false), Err(SessionError::Refused)));
        assert_eq!(gate(&ran, true).unwrap(), Passed::Forced);
    }

    #[test]
    fn a_program_renders_from_a_run() {
        let model = load_model(&fixture("hinge_jig.step"), None, 0.05).unwrap();
        let ran = run(&model, &small_job()).unwrap();
        let profile = load_profile("masso").unwrap();
        let op = ran.result.operations.first().unwrap();
        let (text, report) = program(op, 54, &profile, "01-finish").unwrap();
        assert!(text.contains("G54"));
        assert!(report.motion_blocks > 10);
    }
}
