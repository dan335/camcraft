//! The inspection document: the part and every operation's path, as JSON.
//!
//! Built from a [`SetupResult`] and nothing else. Every subcommand runs through
//! [`run_setup`](crate::run_setup), so what the inspector draws is the
//! path the post lowers, move for move. There is no second, approximate path for
//! the picture to come from, and so no way for a defect to live only in the one
//! nobody looks at.

use camcraft_core::job::{Job, Operation, ToolKind};
use camcraft_core::run::{OperationResult, SetupResult, StrategyDetail};
use camcraft_core::toolpath::{MoveRole, Toolpath};
use camcraft_geom::heightmap::narrow;
use camcraft_geom::mesh::{Aabb, TriMesh};
use glam::DVec3;
use serde::Serialize;

/// Bumped whenever a field changes meaning, so the viewer can tell what it holds.
///
/// Format 1 was a single operation with `path` and `meta` at the top level.
pub const FORMAT: u32 = 2;

/// Everything the inspector shows.
#[derive(Serialize)]
pub struct Document {
    format: u32,
    part: Part,
    job: JobMeta,
    operations: Vec<OperationDoc>,
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
struct JobMeta {
    model: String,
    units: String,
    tolerance: f64,
    /// Which setup this is, everything in its frame.
    setup: String,
    /// Its post processor.
    post: String,
    wcs: u8,
    /// The block of material, corner to corner.
    stock_min: [f64; 3],
    stock_max: [f64; 3],
    /// Whether the whole job verified, every operation included.
    verified: bool,
    verify: VerifyDoc,
    /// Everything worth a person's attention, across every operation.
    warnings: Vec<String>,
}

#[derive(Serialize)]
struct VerifyDoc {
    compared: u64,
    gouged: u64,
    deepest_gouge_mm: f64,
    leftover: u64,
    tallest_leftover_mm: f64,
    budget_mm: f64,
}

#[derive(Serialize)]
struct OperationDoc {
    name: String,
    strategy: &'static str,
    tool: ToolDoc,
    stepover: f64,
    stock_radial: f64,
    stock_axial: f64,
    feed: f64,
    max_cut_step: f64,
    /// Traverses fly this far above the material as it stands.
    clearance_offset: f64,
    /// And never lower than this where the material is unknown.
    clearance_z: f64,
    passes: u64,
    travel_saved: f64,
    links_resolved: u64,
    suspicious_plunges: u64,
    detail: DetailDoc,
    sim: SimDoc,
    steep_cut_descents: u64,
    deepest_cut_descent_mm: f64,
    cutting_length_mm: f64,
    move_count: usize,
    /// Whether this operation is safe to run as it stands.
    verified: bool,
    warnings: Vec<String>,
    path: Vec<Step>,
}

#[derive(Serialize)]
struct ToolDoc {
    name: String,
    kind: &'static str,
    diameter: f64,
    corner: f64,
    number: u32,
    rpm: u32,
    comment: String,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum DetailDoc {
    Parallel {
        refinement_limited: u64,
        discontinuities: u64,
        coverage_breaks: u64,
        wall_breaks: u64,
    },
    ZlevelRough {
        levels: u64,
        empty_levels: u64,
        rings: u64,
        rings_too_short: u64,
        offsets_exhausted: u64,
    },
}

#[derive(Serialize)]
struct SimDoc {
    removed_mm3: f64,
    rapid_contacts: u64,
    deepest_rapid_contact_mm: f64,
}

#[derive(Serialize)]
struct Step {
    to: [f64; 3],
    role: &'static str,
    feed: Option<f64>,
}

/// Describe a finished setup for the inspector. `part` is the setup's part, in its
/// frame, as [`Ran::part`](crate::Ran::part) has it.
///
/// `warnings` is passed in rather than re-derived so the document says exactly
/// what the terminal said.
#[allow(clippy::too_many_arguments)]
pub fn document(
    model: &str,
    units: &str,
    part_mesh: &TriMesh,
    job: &Job,
    setup: usize,
    result: &SetupResult,
    warnings: &[String],
) -> Document {
    let spec = job.setups.get(setup);
    let bounds = part_mesh.bounds().unwrap_or(Aabb {
        min: DVec3::ZERO,
        max: DVec3::ZERO,
    });
    let operations = spec
        .map(|s| s.operations.as_slice())
        .unwrap_or_default()
        .iter()
        .zip(&result.operations)
        .map(|(spec, run)| operation(job, spec, run))
        .collect();

    Document {
        format: FORMAT,
        part: part(part_mesh, &bounds),
        job: JobMeta {
            model: model.to_string(),
            units: units.to_string(),
            tolerance: job.tolerance,
            setup: result.setup.clone(),
            post: spec.map_or_else(String::new, |s| s.post.clone()),
            wcs: spec.map_or(54, |s| s.wcs),
            stock_min: [
                result.stock_min.x,
                result.stock_min.y,
                result.heights.stock_bottom,
            ],
            stock_max: [
                result.stock_max.x,
                result.stock_max.y,
                result.heights.stock_top,
            ],
            verified: result.is_clean(),
            verify: VerifyDoc {
                compared: result.verify.compared,
                gouged: result.verify.gouged,
                deepest_gouge_mm: result.verify.deepest_gouge_mm,
                leftover: result.verify.leftover,
                tallest_leftover_mm: result.verify.tallest_leftover_mm,
                budget_mm: result.verify_budget,
            },
            warnings: warnings.to_vec(),
        },
        operations,
    }
}

fn operation(job: &Job, spec: &Operation, run: &OperationResult) -> OperationDoc {
    let tool = job.tool_for(spec);
    let start = run.path.moves().first().map_or(DVec3::ZERO, |m| m.to);
    OperationDoc {
        name: run.name.clone(),
        strategy: run.strategy,
        tool: ToolDoc {
            name: spec.tool.clone().unwrap_or_default(),
            kind: tool.map_or("unknown", |t| match t.kind {
                ToolKind::Flat => "flat",
                ToolKind::Ball => "ball",
                ToolKind::Bullnose => "bullnose",
            }),
            diameter: tool.map_or(0.0, |t| t.diameter),
            corner: tool.map_or(0.0, |t| t.corner),
            number: run.tool_number,
            rpm: run.rpm,
            comment: run.tool_comment.clone(),
        },
        stepover: spec.passes.stepover,
        stock_radial: spec.passes.stock_to_leave.radial,
        stock_axial: spec.passes.stock_to_leave.axial,
        feed: spec.passes.feed,
        max_cut_step: spec.passes.max_cut_step,
        clearance_offset: spec.heights.clearance.offset,
        clearance_z: run.clearance_z,
        passes: run.passes,
        travel_saved: run.travel_saved,
        links_resolved: run.links.links_resolved,
        suspicious_plunges: run.links.suspicious_plunges,
        detail: match run.detail {
            StrategyDetail::Parallel(r) => DetailDoc::Parallel {
                refinement_limited: r.refinement_limited,
                discontinuities: r.discontinuities,
                coverage_breaks: r.coverage_breaks,
                wall_breaks: r.wall_breaks,
            },
            StrategyDetail::ZlevelRough(r) => DetailDoc::ZlevelRough {
                levels: r.levels,
                empty_levels: r.empty_levels,
                rings: r.rings,
                rings_too_short: r.rings_too_short,
                offsets_exhausted: r.offsets_exhausted,
            },
        },
        sim: SimDoc {
            removed_mm3: run.sim.removed_mm3,
            rapid_contacts: run.sim.rapid_contacts,
            deepest_rapid_contact_mm: run.sim.deepest_rapid_contact_mm,
        },
        steep_cut_descents: run.steep_cut_descents,
        deepest_cut_descent_mm: run.deepest_cut_descent_mm,
        cutting_length_mm: run.path.cutting_length(start),
        move_count: run.path.len(),
        verified: run.is_clean(),
        warnings: run.warnings.clone(),
        path: path(&run.path),
    }
}

fn part(mesh: &TriMesh, bounds: &Aabb) -> Part {
    let mut triangles = Vec::with_capacity(mesh.len() * 9);
    for i in 0..mesh.len() {
        if let Some(t) = mesh.triangle(i) {
            for v in t {
                triangles.push(narrow(v.x));
                triangles.push(narrow(v.y));
                triangles.push(narrow(v.z));
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

fn path(path: &Toolpath) -> Vec<Step> {
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
