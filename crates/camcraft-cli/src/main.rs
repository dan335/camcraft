//! Command-line interface for camcraft.
//!
//! Argument parsing, logging and file writing. Everything else goes through
//! `camcraft-session`, which the desktop app shares, so the two cannot disagree
//! about what a file means, what runs or what is allowed to be posted.
//!
//! Every subcommand runs through one engine path, the one the desktop app runs.
//! `post` describes a single finishing operation with flags, and is turned into a
//! one-operation job before anything is computed; `job` reads a job file, with its
//! models, stocks and setups. What the app draws and what the post writes
//! therefore come from the same path, move for move.
//!
//! Every run is checked ([`camcraft_session::checks`]), and the list is printed.
//! `job` without `--out` stops there: it runs and checks, and writes nothing.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use camcraft_core::job::{
    Job as JobSpec, LengthUnit, ModelDef, Passes, Setup, StrategySpec, ToolKind,
};
use camcraft_session::checks::{Checks, Level};
use camcraft_session::fs::{job_dir, write_atomically};
use camcraft_session::{
    FinishSpec, Model, Models, Passed, Program, Ran, SessionError, gate, load_model, load_models,
    load_profile, models_of, plan_posting, program, read_job, run_setup, setup_profile,
    single_operation, units_of, write_posting,
};
use clap::{Parser, Subcommand, ValueEnum};

/// Generate 3-axis CNC toolpaths from a 3D model.
#[derive(Parser, Debug)]
#[command(name = "camcraft", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Write a G-code program for a single finishing operation.
    ///
    /// Refuses unless the result passes its checks, the simulator's against the
    /// part among them, because a file that can reach a machine should have been
    /// checked against material and not merely against geometry.
    Post(PostArgs),
    /// Run a job file: its setups, each with its own models, stock and post.
    ///
    /// A setup's operations run in order against one block of material, each
    /// reading what the last one left. Every setup is checked and the findings
    /// printed. With `--out`, writes one program per operation, named after its
    /// setup, and refuses to write any unless every setup passed its checks.
    Job(JobArgs),
}

#[derive(Parser, Debug)]
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

    /// Directory to write programs into. One file per operation. Without it, the
    /// setups are run and checked, and nothing is written.
    #[arg(short, long)]
    out: Option<PathBuf>,

    /// Write programs even though checks failed.
    #[arg(long)]
    force: bool,
}

/// A single finishing operation, described with flags.
#[derive(Parser, Debug)]
struct PostArgs {
    /// Model to machine (`.stl`, `.step` or `.stp`).
    model: PathBuf,

    /// Finishing strategy, named as a job file names it, with its own defaults.
    #[arg(long, value_enum, default_value_t = StrategyArg::Parallel)]
    strategy: StrategyArg,

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

    /// Corner radius of a bullnose cutter, in millimetres.
    #[arg(long, default_value_t = 0.5)]
    corner: f64,

    /// Distance between passes, in millimetres.
    #[arg(long, default_value_t = Passes::default().stepover)]
    stepover: f64,

    /// Distance between levels, in millimetres, for contour: 1 mm unless given.
    #[arg(long)]
    stepdown: Option<f64>,

    /// Direction of parallel's passes, in degrees from +X.
    #[arg(long, default_value_t = 0.0)]
    angle: f64,

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
    #[arg(long, default_value_t = Passes::default().max_cut_step)]
    max_cut_step: f64,

    /// How far above the model's top the tool arrives and leaves, in
    /// millimetres. Between cuts it lifts only clear of the material.
    #[arg(long, default_value_t = 10.0)]
    clearance: f64,

    /// Cutting feed, in mm/min. Retracts run at it, plunges at a quarter of it.
    #[arg(long, default_value_t = Passes::default().feed)]
    feed: f64,

    /// Where to write the program.
    #[arg(short, long)]
    out: PathBuf,

    /// Machine profile: a builtin name, or a path to a TOML profile.
    #[arg(long, default_value = "masso")]
    profile: String,

    /// Spindle speed, rev/min.
    #[arg(long, default_value_t = 18000)]
    rpm: u32,

    /// Work offset, 54 for G54.
    #[arg(long, default_value_t = 54)]
    wcs: u8,

    /// Tool number, from 1: T0 is no tool on most controllers.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    tool_number: u32,

    /// Write the program even though checks failed.
    ///
    /// Prints what is wrong first. Reach for this only when you know why the
    /// checks object and have decided they are wrong.
    #[arg(long)]
    force: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum UnitArg {
    /// Millimetres.
    Mm,
    /// Inches.
    Inch,
}

impl UnitArg {
    /// As a job states it; what that means to the reader is the session's to say
    /// ([`units_of`]).
    fn length(self) -> LengthUnit {
        match self {
            UnitArg::Mm => LengthUnit::Mm,
            UnitArg::Inch => LengthUnit::Inch,
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum StrategyArg {
    /// Raster passes side by side. Fusion's Parallel.
    Parallel,
    /// Passes a constant distance apart along the surface. Fusion's Scallop.
    Scallop,
    /// Passes round the part at levels a stepdown apart. Fusion's Contour.
    Contour,
    /// Each flat face cleared at its own height. Fusion's Flat.
    Flat,
    /// A spiral about the middle, dropped onto the part. Fusion's Spiral.
    Spiral,
    /// Spokes out from the middle, dropped onto the part. Fusion's Radial.
    Radial,
    /// A spiral from the boundary into the middle. Fusion's Morphed Spiral.
    MorphedSpiral,
}

impl StrategyArg {
    /// The strategy as a new one starts, by the name a job file gives it, which
    /// is the flag's.
    fn spec(self) -> Fallible<StrategySpec> {
        let name = self.to_possible_value().map(|v| v.get_name().to_string());
        name.as_deref()
            .and_then(StrategySpec::named)
            .ok_or_else(|| format!("no strategy called {name:?}").into())
    }
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum ToolArg {
    /// Ball-nose end mill.
    Ball,
    /// Flat (square-end) end mill.
    Flat,
    /// Flat end mill with rounded corners, of `--corner`.
    Bullnose,
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
    };
    match outcome {
        Ok(0) => ExitCode::SUCCESS,
        Ok(n) => {
            tracing::warn!("{n} warning(s) or failed check(s); review before cutting");
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
    let units = units_of(units.map(UnitArg::length));
    let model = load_model(path, units, tolerance).map_err(explain)?;
    log_notes(None, &model);
    Ok(model)
}

/// Log what loading `model` did, each line after `name` when there are several.
fn log_notes(name: Option<&str>, model: &Model) {
    for n in &model.notes {
        match name {
            Some(name) => tracing::info!("{name}: {n}"),
            None => tracing::info!("{n}"),
        }
    }
}

fn execute(job: &JobSpec, setup: usize, models: &Models<'_>) -> Fallible<Ran> {
    let ran = run_setup(job, setup, models)?;
    for n in &ran.notes {
        tracing::info!("{n}");
    }
    for w in &ran.warnings {
        tracing::warn!("{w}");
    }
    Ok(ran)
}

/// The shared verification policy, with this command line's explanation.
///
/// The failures themselves were printed with the checks.
fn check_gate(ran: &Ran, force: bool, look_first: &str) -> Fallible<()> {
    match gate(ran, force) {
        Ok(Passed::Verified) => Ok(()),
        Ok(Passed::Forced) => {
            tracing::warn!("--force given: writing a program that did NOT pass its checks");
            Ok(())
        }
        Err(e) => {
            tracing::error!(
                "refusing to write a program that did not pass its checks. Look at it \
                 first:\n  {look_first}\n\
                 If you understand why the checks object, re-run with --force."
            );
            Err(e.into())
        }
    }
}

/// Print what the checks found: failures as errors, advisories as warnings and
/// notes, and a line for what passed.
fn report_checks(title: &str, checks: &Checks) {
    let mut passed = 0;
    for group in &checks.groups {
        for f in &group.findings {
            let line = f.line(group);
            match f.level {
                Level::Fail => tracing::error!("{line}"),
                Level::Warn => tracing::warn!("{line}"),
                Level::Note => tracing::info!("{line}"),
                Level::Pass => passed += 1,
            }
        }
    }
    let failed = checks.failures().count();
    if failed == 0 {
        tracing::info!("{title}: all {passed} checks passed");
    } else {
        tracing::error!("{title}: {failed} check(s) FAILED. Do not run this program.");
    }
}

/// Say what was written.
fn report_program(p: &Program) {
    tracing::info!(
        "{}: {} lines, {} motion blocks for {} -> {}",
        p.op,
        p.report.lines,
        p.report.motion_blocks,
        p.profile,
        p.path.display()
    );
}

fn job(args: JobArgs) -> Fallible<usize> {
    let job = read_job(&args.job)?;
    let job_dir = job_dir(&args.job);

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

    // The machine profiles first, when writing: a wrong one should not cost a
    // whole run.
    if args.out.is_some() {
        for setup in chosen.iter().filter_map(|&i| job.setups.get(i)) {
            setup_profile(setup, &job_dir, args.profile.as_deref())?;
        }
    }

    // Each model once, however many setups use it, at the job's tolerance: it
    // decides how finely a STEP model is tessellated.
    let loaded = load_models(&job, &job_dir, &chosen).map_err(explain)?;
    for (name, model) in &loaded {
        log_notes(Some(name), model);
    }
    let models = models_of(&loaded);

    let mut runs = Vec::with_capacity(chosen.len());
    for &i in &chosen {
        runs.push((i, execute(&job, i, &models)?));
    }

    for (i, ran) in &runs {
        let name = job.setups.get(*i).map_or("setup", |s| s.name.as_str());
        report_checks(&format!("setup {name}"), &ran.checks);
    }

    if let Some(dir) = &args.out {
        // Every setup passes, or no program is written: half a job's programs are
        // a trap on the machine.
        for (_, ran) in &runs {
            check_gate(
                ran,
                args.force,
                "bash scripts/run.sh JOB, to open it in the app",
            )?;
        }
        // Everything is rendered before anything is written.
        let setups: Vec<(&Setup, &Ran)> = runs
            .iter()
            .filter_map(|(i, ran)| Some((job.setups.get(*i)?, ran)))
            .collect();
        let posting = plan_posting(&setups, &job_dir, dir, args.profile.as_deref())?;
        write_posting(&posting)?;
        posting.programs.iter().for_each(report_program);
    }

    let failed: usize = runs.iter().map(|(_, r)| r.checks.failures().count()).sum();
    if args.out.is_none() && failed > 0 {
        return Err(format!("{failed} check(s) failed").into());
    }
    // Written anyway with --force, a failed check is still worth reading.
    Ok(runs.iter().map(|(_, r)| r.warnings.len()).sum::<usize>() + failed)
}

/// The job `post` describes with flags, and its model, loaded.
fn flag_job(args: &PostArgs) -> Fallible<(JobSpec, Model)> {
    let model = load(&args.model, args.units, args.tolerance)?;
    let def = ModelDef {
        name: "model".into(),
        path: args.model.display().to_string(),
        units: args.units.map(UnitArg::length),
        transform: Default::default(),
    };
    let job = single_operation(&finish_spec(args)?, def);
    Ok((job, model))
}

fn post(args: PostArgs) -> Fallible<usize> {
    // The profile first: a wrong name should not cost a whole run.
    let profile = load_profile(&args.profile, None)?;
    let (job, model) = flag_job(&args)?;
    let models: Models<'_> = [("model", &model)].into_iter().collect();
    let ran = execute(&job, 0, &models)?;
    report_checks("post", &ran.checks);
    check_gate(
        &ran,
        args.force,
        "camcraft job with a job file and no --out, or open it in the app",
    )?;

    let op = ran
        .result
        .operations
        .first()
        .ok_or("the job produced no operation")?;
    let out = &args.out;
    let name = out
        .file_stem()
        .map_or_else(|| "camcraft".into(), |s| s.to_string_lossy().to_string());
    let (text, report) = program(op, args.wcs, &profile, &name)?;
    write_atomically(out, &text)?;
    tracing::info!(
        "{}: {} lines, {} motion blocks for {} -> {}",
        op.name,
        report.lines,
        report.motion_blocks,
        profile.name,
        out.display()
    );
    Ok(ran.warnings.len() + ran.checks.failures().count())
}

/// The finishing operation that `post` describes with flags.
fn finish_spec(args: &PostArgs) -> Fallible<FinishSpec> {
    let mut strategy = args.strategy.spec()?;
    if let StrategySpec::Parallel { angle } = &mut strategy {
        *angle = args.angle;
    }
    Ok(FinishSpec {
        strategy,
        tool: match args.tool {
            ToolArg::Ball => ToolKind::Ball,
            ToolArg::Flat => ToolKind::Flat,
            ToolArg::Bullnose => ToolKind::Bullnose,
        },
        corner: args.corner,
        diameter: args.diameter,
        stepover: args.stepover,
        stepdown: args.stepdown,
        tolerance: args.tolerance,
        stock_radial: args.stock_radial,
        stock_axial: args.stock_axial,
        max_cut_step: args.max_cut_step,
        clearance: args.clearance,
        feed: args.feed,
        wcs: args.wcs,
        rpm: args.rpm,
        number: args.tool_number,
    })
}
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    /// Every strategy `--strategy` offers is the 3D finishing strategy a job
    /// file calls by the same name: kept to a boundary, and cutting to size.
    #[test]
    fn every_strategy_flag_is_a_finishing_strategy() {
        for arg in StrategyArg::value_variants() {
            let spec = arg.spec().unwrap();
            assert_eq!(spec.name(), arg.to_possible_value().unwrap().get_name());
            assert!(spec.uses().boundary);
            assert_eq!(spec.default_stock_to_leave().radial, 0.0);
        }
    }
}
