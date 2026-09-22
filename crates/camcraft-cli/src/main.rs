//! Command-line interface for camcraft: model + job.toml to G-code.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

/// Generate 3-axis CNC toolpaths from a 3D model.
#[derive(Parser, Debug)]
#[command(name = "camcraft", version, about)]
struct Cli {
    /// Model to machine (`.stl` or `.step`).
    model: PathBuf,

    /// Job description (`.toml`): setup, stock, tools and operations.
    job: PathBuf,

    /// Where to write the G-code program.
    #[arg(short, long)]
    out: PathBuf,

    /// Skip simulator verification before writing the program.
    ///
    /// Verification is on by default and gates release: no cut should come from a
    /// toolpath the simulator has not checked for gouges.
    #[arg(long)]
    no_verify: bool,
}

fn main() -> ExitCode {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    tracing::error!(
        model = %cli.model.display(),
        job   = %cli.job.display(),
        out   = %cli.out.display(),
        verify = !cli.no_verify,
        "camcraft 0.1-a scaffold: the engine is not wired up yet",
    );

    ExitCode::FAILURE
}
