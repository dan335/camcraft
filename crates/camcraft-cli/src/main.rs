//! Command-line interface for camcraft.
//!
//! Argument parsing, logging and file writing. Everything else goes through
//! `camcraft-session`, which the desktop app shares, so the two cannot disagree
//! about what a file means, what runs or what is allowed to be posted.
//!
//! Every subcommand runs through one engine path. `inspect` and `post` describe a
//! single finishing operation with flags, and are turned into a one-operation job
//! before anything is computed; `job` reads a job file, with its models, stocks and
//! setups. What the
//! inspector draws and what the post writes therefore come from the same path, move
//! for move.
//!
//! `inspect` writes no G-code, which is deliberate: nothing that can reach a
//! machine comes out of it.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use std::collections::BTreeMap;

use camcraft_core::job::{Job as JobSpec, LengthUnit, ModelDef};
use camcraft_session::{
    FinishSpec, FinishTool, Model, Models, Passed, Ran, SessionError, Units, gate, inspection,
    load_job_model, load_model, load_profile, program, program_name, read_job, run_setup,
    single_operation,
};
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
    /// Run a job file: its setups, each with its own models, stock and post.
    ///
    /// A setup's operations run in order against one block of material, each
    /// reading what the last one left. Writes one program per operation, named
    /// after its setup, and refuses to write any unless every setup run verified.
    /// `--inspect` writes what will run as JSON for the inspector, and is written
    /// even when verification fails, because that is when it is needed.
    Job(JobArgs),
}

#[derive(Parser, Debug)]
#[command(group(ArgGroup::new("output").required(true).multiple(true).args(["out", "inspect"])))]
struct JobArgs {
    /// The job file (`.camcraft`). Its models are found relative to it.
    job: PathBuf,

    /// Run only this setup, by name. Give it more than once for several; without
    /// it, every setup runs.
    #[arg(long = "setup")]
    setups: Vec<String>,

    /// Machine profile for every setup, instead of each setup's own post: a
    /// builtin name, or a path to a TOML profile.
    #[arg(long)]
    profile: Option<String>,

    /// Directory to write programs into. One file per operation.
    #[arg(short, long)]
    out: Option<PathBuf>,

    /// Write every operation's path as JSON for the inspector. With more than one
    /// setup, one file each, named after the setup.
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

    fn length(self) -> LengthUnit {
        match self {
            UnitArg::Mm => LengthUnit::Mm,
            UnitArg::Inch => LengthUnit::Inch,
        }
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

/// Say a session error the way this command line would, naming its flags.
fn explain(e: SessionError) -> Box<dyn std::error::Error> {
    match e {
        SessionError::StlNeedsUnits => "an STL file carries no units: pass --units mm or \
                                        --units inch. Guessing would make an inch part \
                                        25.4 times too small."
            .into(),
        SessionError::UnitsDisagree { said, file } => format!(
            "--units {said} was given, but the file is in {file}. A STEP file states \
             its own units; drop --units, or check this is the right file."
        )
        .into(),
        SessionError::InModel { name, source }
            if matches!(*source, SessionError::StlNeedsUnits) =>
        {
            format!(
                "model '{name}' is an STL, which carries no units: give it \
                 units = \"mm\" or units = \"inch\" in the job. Guessing would make an \
                 inch part 25.4 times too small."
            )
            .into()
        }
        other => other.into(),
    }
}

fn load(path: &Path, units: Option<UnitArg>, tolerance: f64) -> Fallible<Model> {
    let model = load_model(path, units.map(UnitArg::units), tolerance).map_err(explain)?;
    for n in &model.notes {
        tracing::info!("{n}");
    }
    Ok(model)
}

fn execute(job: &JobSpec, setup: usize, models: &Models<'_>) -> Fallible<Ran> {
    let ran = run_setup(job, setup, models)?;
    for n in &ran.notes {
        tracing::info!("{n}");
    }
    Ok(ran)
}

fn write_inspection(
    path: &Path,
    model: &str,
    units: &str,
    job: &JobSpec,
    setup: usize,
    ran: &Ran,
) -> Fallible<()> {
    let doc = inspection::document(
        model,
        units,
        &ran.part,
        job,
        setup,
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

/// The shared verification policy, with this command line's explanation.
fn check_gate(ran: &Ran, force: bool, look_first: &str) -> Fallible<()> {
    match gate(ran, force) {
        Ok(Passed::Verified) => Ok(()),
        Ok(Passed::Forced) => {
            tracing::warn!("--force given: writing a program that did NOT verify");
            Ok(())
        }
        Err(e) => {
            // The caller never reaches its own warning report on this path, so say
            // what is wrong here, before refusing.
            for w in &ran.warnings {
                tracing::warn!("{w}");
            }
            tracing::error!(
                "refusing to write a program that did not verify. Look at it first:\n  \
                 {look_first}\n  bash scripts/view.sh\n\
                 If you understand why the simulator objects, re-run with --force."
            );
            Err(e.into())
        }
    }
}

fn write_program(
    op: &camcraft_core::run::OperationResult,
    wcs: u8,
    profile: &camcraft_post::profile::Profile,
    name: &str,
    path: &Path,
) -> Fallible<()> {
    let (text, report) = program(op, wcs, profile, name)?;
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
    let job = read_job(&args.job)?;
    let job_dir = args
        .job
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .to_path_buf();

    let chosen: Vec<usize> = if args.setups.is_empty() {
        (0..job.setups.len()).collect()
    } else {
        args.setups
            .iter()
            .map(|name| {
                job.setups
                    .iter()
                    .position(|s| s.name == *name)
                    .ok_or_else(|| format!("the job has no setup called '{name}'"))
            })
            .collect::<Result<_, _>>()?
    };
    if chosen.is_empty() {
        return Err("the job has no setups".into());
    }

    // Each model once, however many setups use it, at the job's tolerance: it
    // decides how finely a STEP model is tessellated.
    let mut loaded: BTreeMap<String, Model> = BTreeMap::new();
    for &i in &chosen {
        for m in job.models_for(i) {
            if loaded.contains_key(&m.name) {
                continue;
            }
            let model = load_job_model(&job_dir, m, job.tolerance).map_err(explain)?;
            for n in &model.notes {
                tracing::info!("{}: {n}", m.name);
            }
            loaded.insert(m.name.clone(), model);
        }
    }
    let models: Models<'_> = loaded.iter().map(|(n, m)| (n.as_str(), m)).collect();

    let mut runs = Vec::with_capacity(chosen.len());
    for &i in &chosen {
        runs.push((i, execute(&job, i, &models)?));
    }

    // Before the gate, not after it: a job that failed to verify is exactly the one
    // that needs looking at.
    if let Some(path) = &args.inspect {
        for (i, ran) in &runs {
            let Some(setup) = job.setups.get(*i) else {
                continue;
            };
            let names: Vec<&str> = setup.models.iter().map(String::as_str).collect();
            let units: Vec<&str> = setup
                .models
                .iter()
                .filter_map(|n| loaded.get(n))
                .map(|m| m.units.as_str())
                .collect();
            let target = if runs.len() == 1 {
                path.clone()
            } else {
                let stem = path
                    .file_stem()
                    .map_or_else(|| "inspect".into(), |s| s.to_string_lossy().to_string());
                path.with_file_name(format!("{stem}-{}.json", setup.name))
            };
            write_inspection(&target, &names.join(", "), &units.join(", "), &job, *i, ran)?;
        }
    }

    if let Some(dir) = &args.out {
        // Every setup passes, or no program is written: half a job's programs are
        // a trap on the machine.
        for (_, ran) in &runs {
            check_gate(
                ran,
                args.force,
                "camcraft job ... --inspect path.json, then drop it on the inspector",
            )?;
        }
        std::fs::create_dir_all(dir)?;
        for (i, ran) in &runs {
            let Some(setup) = job.setups.get(*i) else {
                continue;
            };
            let post = args.profile.as_deref().unwrap_or(&setup.post);
            let profile = load_profile(post, Some(&job_dir))?;
            for (k, op) in ran.result.operations.iter().enumerate() {
                let name = program_name(&setup.name, k, &op.name);
                let path = dir.join(format!("{name}.{}", profile.extension));
                write_program(op, setup.wcs, &profile, &name, &path)?;
            }
        }
    }

    let mut count = 0;
    for (_, ran) in &runs {
        for w in &ran.warnings {
            tracing::warn!("{w}");
        }
        count += ran.warnings.len();
    }
    Ok(count)
}

/// The job `inspect` and `post` describe with flags, and its model, loaded.
fn flag_job(args: &InspectArgs, wcs: u8, rpm: u32, number: u32) -> Fallible<(JobSpec, Model)> {
    let model = load(&args.model, args.units, args.tolerance)?;
    let def = ModelDef {
        name: "model".into(),
        path: args.model.display().to_string(),
        units: args.units.map(UnitArg::length),
    };
    let job = single_operation(&finish_spec(args, wcs, rpm, number), def);
    Ok((job, model))
}

fn post(args: PostArgs) -> Fallible<usize> {
    let (job, model) = flag_job(&args.job, args.wcs, args.rpm, args.tool_number)?;
    let models: Models<'_> = [("model", &model)].into_iter().collect();
    let ran = execute(&job, 0, &models)?;
    check_gate(
        &ran,
        args.force,
        "camcraft inspect ... --out path.json, then drop it on the inspector",
    )?;

    let profile = load_profile(&args.profile, None)?;
    let op = ran
        .result
        .operations
        .first()
        .ok_or("the job produced no operation")?;
    let out = &args.job.out;
    let name = out
        .file_stem()
        .map_or_else(|| "camcraft".into(), |s| s.to_string_lossy().to_string());
    write_program(op, args.wcs, &profile, &name, out)?;

    for w in &ran.warnings {
        tracing::warn!("{w}");
    }
    Ok(ran.warnings.len())
}

fn inspect(args: InspectArgs) -> Fallible<usize> {
    let (job, model) = flag_job(&args, 54, 18000, 1)?;
    let models: Models<'_> = [("model", &model)].into_iter().collect();
    let ran = execute(&job, 0, &models)?;
    write_inspection(&args.out, &model.path, &model.units, &job, 0, &ran)?;
    for w in &ran.warnings {
        tracing::warn!("{w}");
    }
    Ok(ran.warnings.len())
}

/// The finishing pass that `inspect` and `post` describe with flags.
fn finish_spec(args: &InspectArgs, wcs: u8, rpm: u32, number: u32) -> FinishSpec {
    FinishSpec {
        tool: match args.tool {
            ToolArg::Ball => FinishTool::Ball,
            ToolArg::Flat => FinishTool::Flat,
        },
        diameter: args.diameter,
        stepover: args.stepover,
        tolerance: args.tolerance,
        stock_radial: args.stock_radial,
        stock_axial: args.stock_axial,
        max_cut_step: args.max_cut_step,
        clearance: args.clearance,
        feed: args.feed,
        wcs,
        rpm,
        number,
    }
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
