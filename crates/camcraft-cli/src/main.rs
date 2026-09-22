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
use camcraft_core::simadapt::sim_moves;
use camcraft_core::stock::{EffectiveTool, Stock};
use camcraft_core::tool::{Ball, BullnoseFamily, Flat, ToolProfile};
use camcraft_core::toolpath::{MoveRole, Toolpath};
use camcraft_geom::mesh::{Aabb, TriMesh};
use camcraft_import::stl::{Units, load_stl};
use camcraft_post::op::{ProgramSpec, Spin, Units as PostUnits, lower};
use camcraft_post::profile::Profile;
use camcraft_post::render::render;
use camcraft_sim::stock::{PartSurface, Stock as SimStock};
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
    /// Write a G-code program.
    ///
    /// Refuses unless the simulator verifies the result against the part, because a
    /// file that can reach a machine should have been checked against material and
    /// not merely against geometry.
    Post(PostArgs),
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
    /// Largest Z drop that may be traversed as a cutting move, in millimetres.
    #[arg(long, default_value_t = 0.5)]
    max_cut_step: f64,

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
    max_cut_step: f64,
    wall_breaks: u64,
    coverage_breaks: u64,
    sim_rapid_contacts: u64,
    sim_deepest_rapid_contact_mm: f64,
    sim_gouged_columns: u64,
    sim_deepest_gouge_mm: f64,
    sim_leftover_columns: u64,
    sim_removed_mm3: f64,
    verified: bool,
    steep_cut_descents: u64,
    deepest_cut_descent_mm: f64,
    cutting_length_mm: f64,
    move_count: usize,
    /// Anything the caller should look at before trusting this.
    warnings: Vec<String>,
}

/// Descent-to-advance ratio past which a cutting move is really a plunge.
const PLUNGE_RATIO: f64 = 10.0;

fn main() -> ExitCode {
    tracing_subscriber::fmt().with_target(false).init();
    let cli = Cli::parse();
    match cli.command {
        Command::Post(args) => match post(args) {
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

struct Built {
    output: Output,
    path: Toolpath,
    warnings: Vec<String>,
    verified: bool,
}

fn post(args: PostArgs) -> Result<usize, Box<dyn std::error::Error>> {
    let out_path = args.job.out.clone();
    let built = build(&args.job)?;
    for w in &built.warnings {
        tracing::warn!("{w}");
    }

    if !built.verified && !args.force {
        tracing::error!(
            "refusing to write a program that did not verify. Inspect it first:
               camcraft inspect ... --out path.json
               bash scripts/view.sh
             If you understand why the simulator objects, re-run with --force."
        );
        return Err("verification failed".into());
    }
    if !built.verified {
        tracing::warn!("--force given: writing a program that did NOT verify");
    }

    let profile = match Profile::builtin(&args.profile) {
        Some(p) => p?,
        None => Profile::load(&args.profile)?,
    };
    let spec = ProgramSpec {
        name: out_path
            .file_stem()
            .map_or_else(|| "camcraft".into(), |s| s.to_string_lossy().to_string()),
        op_name: "finish".into(),
        strategy: "parallel".into(),
        tool: args.tool_number,
        tool_comment: format!("{:?} d{:.2}", args.job.tool, args.job.diameter),
        wcs: args.wcs,
        rpm: args.rpm,
        spin: Spin::Clockwise,
        coolant: None,
        units: PostUnits::Millimetres,
    };
    let ops = lower(&built.path, &spec)?;
    let (text, report) = render(&ops, &profile, &spec.name);
    std::fs::write(&out_path, &text)?;
    tracing::info!(
        "wrote {} lines ({} motion blocks) for {} to {}",
        report.lines,
        report.motion_blocks,
        profile.name,
        out_path.display()
    );
    Ok(built.warnings.len())
}

fn inspect(args: InspectArgs) -> Result<usize, Box<dyn std::error::Error>> {
    let built = build(&args)?;
    std::fs::write(&args.out, serde_json::to_string(&built.output)?)?;
    let size = std::fs::metadata(&args.out).map(|m| m.len()).unwrap_or(0);
    tracing::info!(
        "wrote {} moves in {} passes ({}) to {}",
        built.path.len(),
        built.output.meta.passes,
        human_size(size),
        args.out.display()
    );
    for w in &built.warnings {
        tracing::warn!("{w}");
    }
    Ok(built.warnings.len())
}

fn build(args: &InspectArgs) -> Result<Built, Box<dyn std::error::Error>> {
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
        max_cut_step: args.max_cut_step,
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

    // The oracle. Every other check reasons about the path; this one removes
    // material and looks at what is left, which is the only way to answer "did a
    // rapid touch stock" against real geometry rather than against a plane.
    let sim_pitch = (args.tolerance * 8.0).clamp(0.05, 1.0);
    let top = bounds.max.z;
    let mut block = SimStock::block(lo, hi, top, bounds.min.z - 1.0, sim_pitch)?;
    let sim = block.apply(&sim_moves(&path), &tool);
    let part = PartSurface::from_mesh(&mesh, lo, hi, sim_pitch)?;
    // The simulator resolves the surface on a grid, so it cannot be held finer than
    // one cell plus the chordal tolerance the path was built to.
    let gouge_budget = args.tolerance + sim_pitch + args.stock_axial;
    let check = block.verify(&part, gouge_budget, 1.0);

    if sim.rapid_contacts > 0 {
        warnings.push(format!(
            "{} rapid move(s) pass through material, deepest {:.2} mm -- a full-speed              traverse is not supposed to be cutting",
            sim.rapid_contacts, sim.deepest_rapid_contact_mm
        ));
    }
    if check.gouged > 0 {
        warnings.push(format!(
            "simulated result is below the part in {} column(s), deepest {:.3} mm              against a {:.3} mm budget",
            check.gouged, check.deepest_gouge_mm, gouge_budget
        ));
    }
    let verified = sim.is_clean() && check.is_clean();
    tracing::info!(
        "simulated: removed {:.0} mm3, {} column(s) compared, gouges {}, rapid contacts {}",
        sim.removed_mm3,
        check.compared,
        check.gouged,
        sim.rapid_contacts
    );

    let start = path.moves().first().map_or(DVec3::ZERO, |m| m.to);
    // A cut that descends faster than it advances is a plunge at cutting feed.
    // Two conditions, not one. Steepness alone is safe: a ball riding down a
    // near-vertical but *smooth* face descends faster than it advances and is fully
    // supported. Depth alone is safe too: a long gentle descent is just a cut. The
    // hazard is both together -- dropping a long way while barely advancing, which
    // engages the end of the cutter rather than its side. PLUNGE_RATIO is about 84
    // degrees; real surfaces measured on this part top out near 3.
    let (steep, deepest) = path.steep_cut_descents(start, PLUNGE_RATIO);
    if deepest > args.max_cut_step {
        warnings.push(format!(
            "deepest cutting descent {deepest:.2} mm exceeds the {:.2} mm allowance              ({steep} steep move(s)) -- a plunge at cutting feed will break cutters",
            args.max_cut_step
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
            max_cut_step: args.max_cut_step,
            wall_breaks: strategy.wall_breaks,
            coverage_breaks: strategy.coverage_breaks,
            sim_rapid_contacts: sim.rapid_contacts,
            sim_deepest_rapid_contact_mm: sim.deepest_rapid_contact_mm,
            sim_gouged_columns: check.gouged,
            sim_deepest_gouge_mm: check.deepest_gouge_mm,
            sim_leftover_columns: check.leftover,
            sim_removed_mm3: sim.removed_mm3,
            verified,
            steep_cut_descents: steep,
            deepest_cut_descent_mm: deepest,
            cutting_length_mm: path.cutting_length(start),
            move_count: path.len(),
            warnings: warnings.clone(),
        },
    };

    Ok(Built {
        output,
        path,
        warnings,
        verified,
    })
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
