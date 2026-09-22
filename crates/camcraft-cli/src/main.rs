//! Command-line interface for camcraft.
//!
//! `inspect` is the verification path: it computes a toolpath and writes the part
//! and the path out together as JSON, so both can be looked at before anything is
//! cut. It writes no G-code, which is deliberate — nothing that can reach a machine
//! comes out of this subcommand.

use std::path::PathBuf;
use std::process::ExitCode;

use camcraft_core::dropcutter::DropCutter;
use camcraft_core::linker::{ClearancePlane, LinkParams, link};
use camcraft_core::parallel::{ParallelParams, parallel_finish};
use camcraft_core::stock::{EffectiveTool, Stock};
use camcraft_core::tool::{Ball, BullnoseFamily, Flat, ToolProfile};
use camcraft_core::toolpath::{MoveRole, Toolpath};
use camcraft_geom::mesh::{Aabb, TriMesh};
use camcraft_import::stl::{Units, load_stl};
use clap::{Parser, Subcommand, ValueEnum};
use glam::{DVec3, dvec2};
use serde::Serialize;

/// Generate 3-axis CNC toolpaths from a 3D model.
#[derive(Parser, Debug)]
#[command(name = "camcraft", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Compute a toolpath and write part + path as JSON for inspection.
    ///
    /// Emits nothing a machine can read. Look at the result before cutting.
    Inspect(InspectArgs),
}

#[derive(Parser, Debug)]
struct InspectArgs {
    /// Model to machine (`.stl`).
    model: PathBuf,

    /// What the numbers in the file mean. STL carries no units.
    #[arg(long, value_enum)]
    units: UnitArg,

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

    /// Traverse height above the top of the part, in millimetres.
    #[arg(long, default_value_t = 10.0)]
    clearance: f64,

    /// Cutting feed, in mm/min.
    #[arg(long, default_value_t = 1200.0)]
    feed: f64,

    /// Where to write the JSON.
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

#[derive(ValueEnum, Clone, Copy, Debug)]
enum ToolArg {
    /// Ball-nose end mill.
    Ball,
    /// Flat (square-end) end mill.
    Flat,
}

#[derive(Serialize)]
struct Output {
    part: Part,
    path: Vec<Step>,
    meta: Meta,
}

#[derive(Serialize)]
struct Part {
    /// Flat triangle corners: nine values per triangle.
    triangles: Vec<f32>,
    min: [f64; 3],
    max: [f64; 3],
    triangle_count: usize,
}

#[derive(Serialize)]
struct Step {
    to: [f64; 3],
    role: &'static str,
    feed: Option<f64>,
}

#[derive(Serialize)]
struct Meta {
    model: String,
    units: &'static str,
    tool: String,
    tool_radius: f64,
    stock_radial: f64,
    stock_axial: f64,
    stepover: f64,
    tolerance: f64,
    clearance_z: f64,
    passes: u64,
    links_resolved: u64,
    refinement_limited: u64,
    discontinuities: u64,
    suspicious_plunges: u64,
    steep_cut_descents: u64,
    deepest_cut_descent_mm: f64,
    cutting_length_mm: f64,
    move_count: usize,
    /// Anything the caller should look at before trusting this.
    warnings: Vec<String>,
}

fn main() -> ExitCode {
    tracing_subscriber::fmt().with_target(false).init();
    let cli = Cli::parse();
    match cli.command {
        Command::Inspect(args) => match inspect(args) {
            Ok(0) => ExitCode::SUCCESS,
            Ok(n) => {
                tracing::warn!("{n} warning(s); review before cutting");
                ExitCode::from(2)
            }
            Err(e) => {
                tracing::error!("{e}");
                ExitCode::FAILURE
            }
        },
    }
}

fn inspect(args: InspectArgs) -> Result<usize, Box<dyn std::error::Error>> {
    let units = match args.units {
        UnitArg::Mm => Units::Millimetres,
        UnitArg::Inch => Units::Inches,
    };
    let (mesh, import) = load_stl(&args.model, units)?;
    let bounds = mesh.bounds().ok_or("model has no geometry")?;
    let extent = bounds.extent();
    tracing::info!(
        "loaded {} triangles, {} vertices after welding, {:.1} x {:.1} x {:.1} mm",
        import.triangles,
        import.vertices,
        extent.x,
        extent.y,
        extent.z
    );

    let mut warnings = Vec::new();
    if import.degenerate > 0 {
        warnings.push(format!(
            "{} degenerate triangle(s) in the model",
            import.degenerate
        ));
    }

    let radius = args.diameter / 2.0;
    let stock = Stock::new(args.stock_radial, args.stock_axial)?;
    let base = match args.tool {
        ToolArg::Ball => Ball::new(radius)?.as_bullnose(),
        ToolArg::Flat => Flat::new(radius)?.as_bullnose(),
    };
    let tool = EffectiveTool::new(&base, stock)?;

    let cutter = DropCutter::new(&mesh);
    let margin = tool.max_radius();
    let (lo, hi) = (
        dvec2(bounds.min.x - margin, bounds.min.y - margin),
        dvec2(bounds.max.x + margin, bounds.max.y + margin),
    );

    let sweep = ParallelParams {
        stepover: args.stepover,
        tolerance: args.tolerance,
        feed: args.feed,
        ..Default::default()
    };
    let (raw, strategy) = parallel_finish(&cutter, &tool, lo, hi, &sweep)?;
    if !strategy.is_clean() {
        warnings.push(format!(
            "{} span(s) hit the refinement limit; the path is coarser than requested",
            strategy.refinement_limited
        ));
    }

    let clearance_z = bounds.max.z + args.clearance;
    let plane = ClearancePlane::new(clearance_z)?;
    let (path, links) = link(
        &raw,
        &plane,
        &LinkParams {
            retract_feed: args.feed,
            plunge_feed: args.feed / 4.0,
            tool_radius: tool.max_radius(),
        },
    )?;
    if links.suspicious_plunges > 0 {
        warnings.push(format!(
            "{} plunge(s) do not start above the surface",
            links.suspicious_plunges
        ));
    }
    if path.has_unplanned_links() {
        warnings.push("path still contains unresolved links and is NOT machinable".into());
    }

    let start = path.moves().first().map_or(DVec3::ZERO, |m| m.to);
    // A cut that descends faster than it advances is a plunge at cutting feed.
    let (steep, deepest) = path.steep_cut_descents(start, 1.0);
    if steep > 0 {
        warnings.push(format!(
            "{steep} cutting move(s) descend more steeply than 45 degrees, deepest {deepest:.2} mm              -- these are full-depth plunges at cutting feed and will break cutters"
        ));
    }
    let output = Output {
        part: part_json(&mesh, &bounds),
        path: path_json(&path),
        meta: Meta {
            model: args.model.display().to_string(),
            units: match args.units {
                UnitArg::Mm => "mm",
                UnitArg::Inch => "inch",
            },
            tool: format!("{:?}", args.tool),
            tool_radius: radius,
            stock_radial: args.stock_radial,
            stock_axial: args.stock_axial,
            stepover: args.stepover,
            tolerance: args.tolerance,
            clearance_z,
            passes: strategy.passes,
            links_resolved: links.links_resolved,
            refinement_limited: strategy.refinement_limited,
            discontinuities: strategy.discontinuities,
            suspicious_plunges: links.suspicious_plunges,
            steep_cut_descents: steep,
            deepest_cut_descent_mm: deepest,
            cutting_length_mm: path.cutting_length(start),
            move_count: path.len(),
            warnings: warnings.clone(),
        },
    };

    std::fs::write(&args.out, serde_json::to_string(&output)?)?;
    let size = std::fs::metadata(&args.out).map(|m| m.len()).unwrap_or(0);
    tracing::info!(
        "wrote {} moves in {} passes ({}) to {}",
        path.len(),
        strategy.passes,
        human_size(size),
        args.out.display()
    );
    for w in &warnings {
        tracing::warn!("{w}");
    }
    Ok(warnings.len())
}

fn part_json(mesh: &TriMesh, bounds: &Aabb) -> Part {
    let mut triangles = Vec::with_capacity(mesh.len() * 9);
    for i in 0..mesh.len() {
        if let Some(t) = mesh.triangle(i) {
            for v in t {
                triangles.push(camcraft_geom::heightmap::narrow(v.x));
                triangles.push(camcraft_geom::heightmap::narrow(v.y));
                triangles.push(camcraft_geom::heightmap::narrow(v.z));
            }
        }
    }
    Part {
        triangles,
        min: bounds.min.to_array(),
        max: bounds.max.to_array(),
        triangle_count: mesh.len(),
    }
}

fn path_json(path: &Toolpath) -> Vec<Step> {
    path.moves()
        .iter()
        .map(|m| Step {
            to: m.to.to_array(),
            role: role_name(m.role),
            feed: m.feed,
        })
        .collect()
}

fn role_name(r: MoveRole) -> &'static str {
    match r {
        MoveRole::Rapid => "rapid",
        MoveRole::Cut => "cut",
        MoveRole::Lead => "lead",
        MoveRole::Ramp => "ramp",
        MoveRole::Plunge => "plunge",
        MoveRole::Retract => "retract",
        MoveRole::Link => "link",
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
