//! Command-line interface for camcraft.
//!
//! Every subcommand runs through one engine path, [`run_job`]. `inspect` and `post`
//! describe a single finishing operation with flags, and are turned into a
//! one-operation job before anything is computed; `job` reads the operations from a
//! file. What the inspector draws and what the post writes therefore come from the
//! same path, move for move. There used to be a second, single-operation path for
//! the picture, and a defect could have lived in only the one nobody looked at.
//!
//! `inspect` writes no G-code, which is deliberate: nothing that can reach a
//! machine comes out of it.

mod inspection;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use camcraft_core::job::{
    Geometry, Height, HeightRef, Heights, Job as JobSpec, Linking, Operation, Passes, Setup,
    StockSpec, StockToLeave, StrategySpec, ToolKind, ToolSpec,
};
use camcraft_core::run::{JobResult, OperationResult, run_job};
use camcraft_geom::mesh::{Aabb, TriMesh};
use camcraft_import::step::{FileCheck, load_step};
use camcraft_import::stl::{Units, load_stl};
use camcraft_post::op::{ProgramSpec, Spin, Units as PostUnits, lower};
use camcraft_post::profile::Profile;
use camcraft_post::render::render;
use clap::{ArgGroup, Parser, Subcommand, ValueEnum};

/// Generate 3-axis CNC toolpaths from a 3D model.
#[derive(Parser, Debug)]
#[command(name = "camcraft", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Compute a finishing pass and write part + path as JSON for inspection.
    ///
    /// Emits nothing a machine can read. Look at the result before cutting.
    Inspect(InspectArgs),
    /// Write a G-code program for a single finishing pass.
    ///
    /// Refuses unless the simulator verifies the result against the part, because a
    /// file that can reach a machine should have been checked against material and
    /// not merely against geometry.
    Post(PostArgs),
    /// Run a multi-operation job from a TOML file.
    ///
    /// Operations run in order against one block of material, each reading what the
    /// last one left. Writes one program per operation, and refuses any that did not
    /// verify. `--inspect` writes what will run as JSON for the inspector, and is
    /// written even when verification fails, because that is when it is needed.
    Job(JobArgs),
}

#[derive(Parser, Debug)]
#[command(group(ArgGroup::new("output").required(true).multiple(true).args(["out", "inspect"])))]
struct JobArgs {
    /// Model to machine (`.stl`, `.step` or `.stp`).
    model: PathBuf,

    /// Job description (`.toml`).
    job: PathBuf,

    /// What the numbers in the model mean. Required for STL, which carries no
    /// units. A STEP file states its own; given anyway, it must agree.
    #[arg(long, value_enum)]
    units: Option<UnitArg>,

    /// Machine profile: a builtin name, or a path to a TOML profile.
    #[arg(long, default_value = "masso")]
    profile: String,

    /// Directory to write programs into. One file per operation.
    #[arg(short, long)]
    out: Option<PathBuf>,

    /// Write every operation's path as JSON for the inspector.
    #[arg(long)]
    inspect: Option<PathBuf>,

    /// Write programs even though verification failed.
    #[arg(long)]
    force: bool,
}

#[derive(Parser, Debug)]
struct PostArgs {
    #[command(flatten)]
    job: InspectArgs,

    /// Machine profile: a builtin name, or a path to a TOML profile.
    #[arg(long, default_value = "masso")]
    profile: String,

    /// Spindle speed, rev/min.
    #[arg(long, default_value_t = 18000)]
    rpm: u32,

    /// Work offset, 54 for G54.
    #[arg(long, default_value_t = 54)]
    wcs: u8,

    /// Tool number.
    #[arg(long, default_value_t = 1)]
    tool_number: u32,

    /// Write the program even though verification failed.
    ///
    /// Prints what is wrong first. Reach for this only when you know why the
    /// simulator objects and have decided it is wrong.
    #[arg(long)]
    force: bool,
}

#[derive(Parser, Debug)]
struct InspectArgs {
    /// Model to machine (`.stl`, `.step` or `.stp`).
    model: PathBuf,

    /// What the numbers in the model mean. Required for STL, which carries no
    /// units. A STEP file states its own; given anyway, it must agree.
    #[arg(long, value_enum)]
    units: Option<UnitArg>,

    /// Cutter shape.
    #[arg(long, value_enum, default_value_t = ToolArg::Ball)]
    tool: ToolArg,

    /// Cutter diameter in millimetres.
    #[arg(long, default_value_t = 6.0)]
    diameter: f64,

    /// Distance between passes, in millimetres.
    #[arg(long, default_value_t = 1.0)]
    stepover: f64,

    /// Chordal tolerance, in millimetres.
    #[arg(long, default_value_t = 0.01)]
    tolerance: f64,

    /// Stock to leave perpendicular to the tool axis, in millimetres.
    #[arg(long, default_value_t = 0.0)]
    stock_radial: f64,

    /// Stock to leave along the tool axis, in millimetres.
    #[arg(long, default_value_t = 0.0)]
    stock_axial: f64,

    /// Largest Z drop that may be traversed as a cutting move, in millimetres.
    #[arg(long, default_value_t = 0.5)]
    max_cut_step: f64,

    /// Traverse height above the material, in millimetres.
    #[arg(long, default_value_t = 10.0)]
    clearance: f64,

    /// Cutting feed, in mm/min.
    #[arg(long, default_value_t = 1200.0)]
    feed: f64,

    /// Where to write the output.
    #[arg(short, long)]
    out: PathBuf,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum UnitArg {
    /// Millimetres.
    Mm,
    /// Inches.
    Inch,
}

impl UnitArg {
    fn units(self) -> Units {
        match self {
            UnitArg::Mm => Units::Millimetres,
            UnitArg::Inch => Units::Inches,
        }
    }

    fn label(self) -> &'static str {
        match self {
            UnitArg::Mm => "mm",
            UnitArg::Inch => "inch",
        }
    }

    fn mm(self) -> f64 {
        self.units().to_mm()
    }
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum ToolArg {
    /// Ball-nose end mill.
    Ball,
    /// Flat (square-end) end mill.
    Flat,
}

type Fallible<T> = Result<T, Box<dyn std::error::Error>>;

fn main() -> ExitCode {
    // Logs and errors go to stderr, as a command-line tool's should: stdout is kept
    // clean for anything a caller might pipe.
    tracing_subscriber::fmt()
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    let outcome = match cli.command {
        Command::Job(args) => job(args),
        Command::Post(args) => post(args),
        Command::Inspect(args) => inspect(args),
    };
    match outcome {
        Ok(0) => ExitCode::SUCCESS,
        Ok(n) => {
            tracing::warn!("{n} warning(s); review before cutting");
            ExitCode::from(2)
        }
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// A model, loaded and measured.
struct Model {
    mesh: TriMesh,
    bounds: Aabb,
    path: String,
    /// What the model's numbers meant, as a person would say it.
    units: String,
    /// Anything about the import worth a person's attention.
    warnings: Vec<String>,
}

/// Tessellation tolerance for a B-rep, from the machining tolerance.
///
/// The simulator verifies against this mesh, so the gap between it and the true
/// surface is error nothing downstream can see. A quarter keeps it a small part of
/// the budget without making tessellation the slow step.
fn tessellation_tolerance(tolerance: f64) -> f64 {
    tolerance / 4.0
}

fn load(path: &Path, units: Option<UnitArg>, tolerance: f64) -> Fallible<Model> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let model = match ext.as_str() {
        "stl" => {
            let units = units.ok_or(
                "an STL file carries no units: pass --units mm or --units inch. \
                 Guessing would make an inch part 25.4 times too small.",
            )?;
            let (mesh, import) = load_stl(path, units.units())?;
            let mut warnings = Vec::new();
            if import.degenerate > 0 {
                warnings.push(format!(
                    "{} degenerate triangle(s) in the model",
                    import.degenerate
                ));
            }
            tracing::info!(
                "loaded {} triangles, {} vertices after welding",
                import.triangles,
                import.vertices
            );
            finish(path, mesh, units.label().to_string(), warnings)?
        }
        "step" | "stp" => {
            let started = std::time::Instant::now();
            let (mesh, report) = load_step(path, tessellation_tolerance(tolerance))?;
            if let Some(said) = units
                && (said.mm() - report.unit.mm).abs() > 1e-9 * said.mm()
            {
                return Err(format!(
                    "--units {} was given, but the file is in {}. A STEP file states \
                     its own units; drop --units, or check this is the right file.",
                    said.label(),
                    report.unit.name
                )
                .into());
            }
            let mut warnings = Vec::new();
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
            tracing::info!(
                "read STEP in {}: {} face(s) all meshed, {} triangles at {} mm, checked: {}",
                report.unit.name,
                report.faces,
                report.triangles,
                report.tolerance,
                match report.check {
                    FileCheck::Passed => "yes",
                    FileCheck::Skipped(_) => "NO",
                },
            );
            tracing::info!("tessellated in {:.1} s", started.elapsed().as_secs_f64());
            finish(
                path,
                mesh,
                format!("{} (from file)", report.unit.name),
                warnings,
            )?
        }
        _ => {
            return Err(format!(
                "{}: not a model this can read; expected .stl, .step or .stp",
                path.display()
            )
            .into());
        }
    };
    Ok(model)
}

fn finish(path: &Path, mesh: TriMesh, units: String, warnings: Vec<String>) -> Fallible<Model> {
    let bounds = mesh.bounds().ok_or("model has no geometry")?;
    let extent = bounds.extent();
    tracing::info!(
        "model is {:.1} x {:.1} x {:.1} mm",
        extent.x,
        extent.y,
        extent.z
    );
    Ok(Model {
        mesh,
        bounds,
        path: path.display().to_string(),
        units,
        warnings,
    })
}

/// A job, run.
struct Ran {
    result: JobResult,
    warnings: Vec<String>,
}

/// The one engine path every subcommand takes.
fn run(model: &Model, job: &JobSpec) -> Fallible<Ran> {
    // Validation happens before anything is computed, so a bad job fails on its own
    // terms rather than part-way through an operation.
    job.validate()?;
    tracing::info!(
        "job: {} operation(s), tolerance {} mm",
        job.operations.len(),
        job.tolerance
    );
    let result = run_job(job, &model.mesh)?;
    report(&result);

    let mut warnings = result.warnings();
    warnings.extend(model.warnings.iter().cloned());
    Ok(Ran { result, warnings })
}

fn report(result: &JobResult) {
    for op in &result.operations {
        tracing::info!(
            "{}: {} passes, removed {:.0} mm3, travel saved {:.0}%, rapid contacts {}",
            op.name,
            op.passes,
            op.sim.removed_mm3,
            op.travel_saved * 100.0,
            op.sim.rapid_contacts
        );
    }
    tracing::info!(
        "verified: {} column(s) compared, gouges {}, deepest {:.3} mm, budget {:.3} mm",
        result.verify.compared,
        result.verify.gouged,
        result.verify.deepest_gouge_mm,
        result.verify_budget
    );
}

fn write_inspection(path: &Path, model: &Model, job: &JobSpec, ran: &Ran) -> Fallible<()> {
    let doc = inspection::document(
        &model.path,
        &model.units,
        &model.mesh,
        &model.bounds,
        job,
        &ran.result,
        &ran.warnings,
    );
    std::fs::write(path, serde_json::to_string(&doc)?)?;
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let moves: usize = ran.result.operations.iter().map(|o| o.path.len()).sum();
    tracing::info!(
        "wrote {moves} moves in {} operation(s) ({}) to {}",
        ran.result.operations.len(),
        human_size(size),
        path.display()
    );
    Ok(())
}

/// Refuse to go further unless the job verified, or the caller insisted.
fn gate(ran: &Ran, force: bool, look_first: &str) -> Fallible<()> {
    if ran.result.is_clean() {
        return Ok(());
    }
    if !force {
        // The caller never reaches its own warning report on this path, so say what
        // is wrong here, before refusing.
        for w in &ran.warnings {
            tracing::warn!("{w}");
        }
        tracing::error!(
            "refusing to write a program that did not verify. Look at it first:\n  \
             {look_first}\n  bash scripts/view.sh\n\
             If you understand why the simulator objects, re-run with --force."
        );
        return Err("verification failed".into());
    }
    tracing::warn!("--force given: writing a program that did NOT verify");
    Ok(())
}

fn load_profile(name: &str) -> Fallible<Profile> {
    Ok(match Profile::builtin(name) {
        Some(p) => p?,
        None => Profile::load(name)?,
    })
}

fn write_program(
    op: &OperationResult,
    wcs: u8,
    profile: &Profile,
    name: &str,
    path: &Path,
) -> Fallible<()> {
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
    let (text, report) = render(&ops, profile, name);
    std::fs::write(path, &text)?;
    tracing::info!(
        "{}: {} lines, {} motion blocks for {} -> {}",
        op.name,
        report.lines,
        report.motion_blocks,
        profile.name,
        path.display()
    );
    Ok(())
}

fn job(args: JobArgs) -> Fallible<usize> {
    // The job first: its tolerance decides how finely a STEP model is tessellated.
    let job: JobSpec = toml::from_str(&std::fs::read_to_string(&args.job)?)?;
    let model = load(&args.model, args.units, job.tolerance)?;
    let ran = run(&model, &job)?;

    // Before the gate, not after it: a job that failed to verify is exactly the one
    // that needs looking at.
    if let Some(path) = &args.inspect {
        write_inspection(path, &model, &job, &ran)?;
    }

    if let Some(dir) = &args.out {
        gate(
            &ran,
            args.force,
            "camcraft job ... --inspect path.json, then drop it on the inspector",
        )?;
        let profile = load_profile(&args.profile)?;
        std::fs::create_dir_all(dir)?;
        for (i, op) in ran.result.operations.iter().enumerate() {
            let name = format!("{:02}-{}", i + 1, sanitise(&op.name));
            let path = dir.join(format!("{name}.{}", profile.extension));
            write_program(op, job.setup.wcs, &profile, &name, &path)?;
        }
    }

    for w in &ran.warnings {
        tracing::warn!("{w}");
    }
    Ok(ran.warnings.len())
}

fn post(args: PostArgs) -> Fallible<usize> {
    let model = load(&args.job.model, args.job.units, args.job.tolerance)?;
    let job = single_operation(&args.job, args.wcs, args.rpm, args.tool_number);
    let ran = run(&model, &job)?;
    gate(
        &ran,
        args.force,
        "camcraft inspect ... --out path.json, then drop it on the inspector",
    )?;

    let profile = load_profile(&args.profile)?;
    let op = ran
        .result
        .operations
        .first()
        .ok_or("the job produced no operation")?;
    let out = &args.job.out;
    let name = out
        .file_stem()
        .map_or_else(|| "camcraft".into(), |s| s.to_string_lossy().to_string());
    write_program(op, job.setup.wcs, &profile, &name, out)?;

    for w in &ran.warnings {
        tracing::warn!("{w}");
    }
    Ok(ran.warnings.len())
}

fn inspect(args: InspectArgs) -> Fallible<usize> {
    let model = load(&args.model, args.units, args.tolerance)?;
    let job = single_operation(&args, 54, 18000, 1);
    let ran = run(&model, &job)?;
    write_inspection(&args.out, &model, &job, &ran)?;
    for w in &ran.warnings {
        tracing::warn!("{w}");
    }
    Ok(ran.warnings.len())
}

/// The job that `inspect` and `post` describe with flags.
///
/// The block is the model's own bounding box, and traverses clear the material by
/// `--clearance`. Retracts run at the cutting feed and plunges at a quarter of it.
fn single_operation(args: &InspectArgs, wcs: u8, rpm: u32, number: u32) -> JobSpec {
    const TOOL: &str = "cutter";
    let (kind, shape) = match args.tool {
        ToolArg::Ball => (ToolKind::Ball, "ball"),
        ToolArg::Flat => (ToolKind::Flat, "flat"),
    };
    let tool = ToolSpec {
        kind,
        diameter: args.diameter,
        corner: 0.0,
        number,
        rpm,
        comment: format!("{}mm {shape}", args.diameter),
    };
    JobSpec {
        tolerance: args.tolerance,
        setup: Setup {
            stock: StockSpec::Box { margin: [0.0; 3] },
            wcs,
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
                    offset: args.clearance,
                },
            },
            passes: Passes {
                stepover: args.stepover,
                feed: args.feed,
                stock_to_leave: StockToLeave {
                    radial: args.stock_radial,
                    axial: args.stock_axial,
                },
                max_cut_step: args.max_cut_step,
            },
            linking: Linking {
                retract_feed: args.feed,
                plunge_feed: args.feed / 4.0,
                ..Linking::default()
            },
        }],
    }
}

/// Keep an operation name usable as a file name.
fn sanitise(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn human_size(bytes: u64) -> String {
    // Only for a log line; sizes past 2^53 bytes are not a real concern.
    #[allow(clippy::cast_precision_loss)]
    let kb = bytes as f64 / 1024.0;
    if kb > 1024.0 {
        format!("{:.1} MB", kb / 1024.0)
    } else {
        format!("{kb:.0} KB")
    }
}
