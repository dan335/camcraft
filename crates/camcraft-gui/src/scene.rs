//! What the viewport draws, built on the CPU from a finished run.
//!
//! Pure data, so the rules can be tested without a GPU:
//!
//! - **Segments are the program in machine order.** Each joins two consecutive
//!   moves of one operation, and its index in the list *is* its order, which is
//!   what the scrubber walks.
//! - **Segments never join operations.** Between two operations is a tool change,
//!   not a move, and drawing a line there would show a traverse that never happens.
//! - **The cutter is the job's own tool,** revolved from its real profile, with the
//!   tip at the origin because the tip is the point the engine controls.

use camcraft_core::job::{Job, ToolKind, ToolSpec};
use camcraft_core::run::JobResult;
use camcraft_core::toolpath::MoveRole;
use camcraft_geom::heightmap::narrow;
use glam::{DVec3, Vec3};

use crate::theme;

/// One move, drawn as a line from where the tool was to where it went.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    /// Where the tool was.
    pub a: DVec3,
    /// Where the move took it.
    pub b: DVec3,
    /// Why the move exists.
    pub role: MoveRole,
    /// Which operation, from zero.
    pub op: u32,
    /// Feed, for the readout; `None` for a rapid.
    pub feed: Option<f64>,
}

/// One operation's share of the program.
#[derive(Debug, Clone)]
pub struct OpSpan {
    /// Its name.
    pub name: String,
    /// Index of its first segment.
    pub first: usize,
    /// How many segments it has.
    pub count: usize,
    /// Its cutter, as triangles, tip at the origin.
    pub cutter: Vec<[Vec3; 3]>,
}

/// Everything drawn after a run, beyond the part itself.
#[derive(Debug, Clone, Default)]
pub struct Scene {
    /// Every move of every operation, in machine order.
    pub segments: Vec<Segment>,
    /// Each operation's range of `segments`.
    pub ops: Vec<OpSpan>,
    /// The block of material, as twelve edges.
    pub stock: Vec<(DVec3, DVec3)>,
    /// Segments per role, indexed as [`theme::ROLES`].
    pub role_counts: [usize; 7],
}

/// Where the scrub stands: the last move shown.
#[derive(Debug, Clone, PartialEq)]
pub struct AtMove<'a> {
    /// Which operation, from zero.
    pub op: usize,
    /// Its name.
    pub op_name: &'a str,
    /// The move.
    pub segment: &'a Segment,
}

impl Scene {
    /// The scene for a finished run of `job`.
    pub fn build(job: &Job, result: &JobResult) -> Self {
        let mut scene = Self::default();
        for (i, run) in result.operations.iter().enumerate() {
            let op = u32::try_from(i).unwrap_or(u32::MAX);
            let first = scene.segments.len();
            for pair in run.path.moves().windows(2) {
                let [from, to] = pair else { continue };
                scene.segments.push(Segment {
                    a: from.to,
                    b: to.to,
                    role: to.role,
                    op,
                    feed: to.feed,
                });
                let r = theme::role_index(to.role) as usize;
                if let Some(c) = scene.role_counts.get_mut(r) {
                    *c += 1;
                }
            }
            let cutter = job
                .operations
                .get(i)
                .and_then(|spec| job.tool_for(spec))
                .map(cutter_mesh)
                .unwrap_or_default();
            scene.ops.push(OpSpan {
                name: run.name.clone(),
                first,
                count: scene.segments.len() - first,
                cutter,
            });
        }
        scene.stock = stock_box(result);
        scene
    }

    /// The segment the scrub ends on, when `shown` segments are showing.
    pub fn at(&self, shown: usize) -> Option<AtMove<'_>> {
        let index = shown.checked_sub(1)?;
        let segment = self.segments.get(index)?;
        let op = segment.op as usize;
        let op_name = self.ops.get(op).map_or("", |o| o.name.as_str());
        Some(AtMove {
            op,
            op_name,
            segment,
        })
    }
}

/// The twelve edges of the block of material the job ran against.
pub fn stock_box(result: &JobResult) -> Vec<(DVec3, DVec3)> {
    let (lo, hi) = (result.stock_min, result.stock_max);
    let (z0, z1) = (result.heights.stock_bottom, result.heights.stock_top);
    let corner = |i: u8| {
        DVec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { z0 } else { z1 },
        )
    };
    let mut edges = Vec::with_capacity(12);
    for i in 0..8u8 {
        for bit in [1u8, 2, 4] {
            if i & bit == 0 {
                edges.push((corner(i), corner(i | bit)));
            }
        }
    }
    edges
}

/// Segments around the cutter's axis.
const AROUND: u32 = 36;
/// Steps along a rounded corner.
const ARC_STEPS: u32 = 8;

/// The cutter as triangles: its profile revolved about +Z, tip at the origin.
///
/// Flat, ball and bullnose are all one profile: a flat bottom out to
/// `radius - corner`, a quarter-circle corner, then the side up to a shank three
/// diameters long.
pub fn cutter_mesh(tool: &ToolSpec) -> Vec<[Vec3; 3]> {
    let radius = narrow(tool.diameter * 0.5).max(1e-3);
    let corner = match tool.kind {
        ToolKind::Flat => 0.0,
        ToolKind::Ball => radius,
        ToolKind::Bullnose => narrow(tool.corner).clamp(0.0, radius),
    };
    let height = radius * 6.0;

    // (distance from axis, height), from the tip up and round the top.
    let mut profile = vec![(0.0_f32, 0.0_f32)];
    if corner < radius {
        profile.push((radius - corner, 0.0));
    }
    if corner > 0.0 {
        for s in 1..=ARC_STEPS {
            let t = std::f32::consts::FRAC_PI_2 * f32_from(s) / f32_from(ARC_STEPS);
            profile.push((radius - corner + corner * t.sin(), corner * (1.0 - t.cos())));
        }
    }
    profile.push((radius, height));
    profile.push((0.0, height));

    let ring = |(r, z): (f32, f32), k: u32| {
        let a = std::f32::consts::TAU * f32_from(k) / f32_from(AROUND);
        Vec3::new(r * a.cos(), r * a.sin(), z)
    };
    let mut out = Vec::new();
    for pair in profile.windows(2) {
        let [p0, p1] = pair else { continue };
        for k in 0..AROUND {
            let (a0, a1) = (ring(*p0, k), ring(*p0, k + 1));
            let (b0, b1) = (ring(*p1, k), ring(*p1, k + 1));
            // On the axis a quad collapses to one triangle; skip the empty half.
            if p0.0 > 0.0 {
                out.push([a0, a1, b1]);
            }
            if p1.0 > 0.0 {
                out.push([a0, b1, b0]);
            }
        }
    }
    out
}

/// A small count as a float, for angles.
fn f32_from(n: u32) -> f32 {
    // Counts here are tens, far inside f32's exact integer range.
    #[allow(clippy::cast_precision_loss)]
    let f = n as f32;
    f
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
    use camcraft_session::{load_model, read_job, run};

    fn repo(path: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path)
    }

    /// A real two-operation run on a real part, shared by the tests below.
    fn scene() -> (Job, JobResult, Scene) {
        let dir = std::env::temp_dir().join(format!("camcraft-gui-scene-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let job_path = dir.join("job.toml");
        std::fs::write(
            &job_path,
            r#"
tolerance = 0.1
[setup]
stock = { kind = "box", margin = [2.0, 2.0, 2.0] }
[tools.flat]
kind = "flat"
diameter = 4.0
[tools.ball]
kind = "ball"
diameter = 3.0
number = 2
[[operation]]
name = "rough"
strategy = "zlevel-rough"
tool = "flat"
stepdown = 4.0
passes = { stepover = 2.0, feed = 1800.0, stock_to_leave = { radial = 0.3, axial = 0.3 } }
[[operation]]
name = "finish"
strategy = "parallel"
tool = "ball"
passes = { stepover = 2.0, feed = 2000.0 }
"#,
        )
        .unwrap();
        let job = read_job(&job_path).unwrap();
        let model = load_model(&repo("fixtures/step/hinge_jig.step"), None, job.tolerance).unwrap();
        let ran = run(&model, &job).unwrap();
        let scene = Scene::build(&job, &ran.result);
        (job, ran.result, scene)
    }

    /// What the viewport draws for each operation is exactly its toolpath, move for
    /// move and in order: the viewport's sibling of the CLI's one-path test.
    #[test]
    fn segments_are_each_operations_moves_in_machine_order() {
        let (_, result, scene) = scene();
        assert_eq!(scene.ops.len(), 2);
        let mut next = 0;
        for (i, (span, run)) in scene.ops.iter().zip(&result.operations).enumerate() {
            let moves = run.path.moves();
            assert!(
                moves.len() > 10,
                "{}: too few moves to mean anything",
                run.name
            );
            assert_eq!(span.first, next, "operations are contiguous, in order");
            assert_eq!(
                span.count,
                moves.len() - 1,
                "one segment per move after the first"
            );
            for (k, s) in scene.segments[span.first..span.first + span.count]
                .iter()
                .enumerate()
            {
                assert_eq!(s.a, moves[k].to);
                assert_eq!(s.b, moves[k + 1].to);
                assert_eq!(s.role, moves[k + 1].role);
                assert_eq!(s.feed, moves[k + 1].feed);
                assert_eq!(s.op as usize, i);
            }
            next += span.count;
        }
        assert_eq!(next, scene.segments.len());
        assert_eq!(
            scene.role_counts.iter().sum::<usize>(),
            scene.segments.len()
        );
    }

    /// No segment runs from the last move of one operation to the first of the next.
    #[test]
    fn no_segment_joins_two_operations() {
        let (_, result, scene) = scene();
        let end_of_rough = result.operations[0].path.moves().last().unwrap().to;
        let start_of_finish = result.operations[1].path.moves().first().unwrap().to;
        assert!(
            !scene
                .segments
                .iter()
                .any(|s| s.a == end_of_rough && s.b == start_of_finish),
            "a line was drawn across the tool change"
        );
    }

    #[test]
    fn the_scrub_names_the_move_it_stands_on() {
        let (_, _, scene) = scene();
        assert!(scene.at(0).is_none(), "nothing shown, nothing to name");
        let first = scene.at(1).unwrap();
        assert_eq!((first.op, first.op_name), (0, "rough"));
        assert_eq!(first.segment, &scene.segments[0]);

        let boundary = scene.ops[1].first;
        assert_eq!(scene.at(boundary).unwrap().op_name, "rough");
        assert_eq!(scene.at(boundary + 1).unwrap().op_name, "finish");
        let last = scene.at(scene.segments.len()).unwrap();
        assert_eq!(last.segment, scene.segments.last().unwrap());
        assert!(scene.at(scene.segments.len() + 1).is_none());
    }

    #[test]
    fn the_stock_box_is_the_block_the_job_ran_against() {
        let (_, result, scene) = scene();
        assert_eq!(scene.stock.len(), 12);
        for (a, b) in &scene.stock {
            // Every edge runs along exactly one axis.
            let d = (*b - *a).abs();
            assert_eq!(
                [d.x > 0.0, d.y > 0.0, d.z > 0.0]
                    .iter()
                    .filter(|x| **x)
                    .count(),
                1
            );
        }
        let top = scene
            .stock
            .iter()
            .map(|(a, b)| a.z.max(b.z))
            .fold(f64::MIN, f64::max);
        assert_eq!(top, result.heights.stock_top);
    }

    fn tool(kind: ToolKind, diameter: f64, corner: f64) -> ToolSpec {
        ToolSpec {
            kind,
            diameter,
            corner,
            number: 1,
            rpm: 18000,
            comment: String::new(),
        }
    }

    /// The cutter's tip is the controlled point: it must sit at the origin, and the
    /// cutter must be as wide as the tool.
    #[test]
    fn cutters_have_their_tip_at_the_origin_and_their_real_width() {
        for t in [
            tool(ToolKind::Flat, 6.0, 0.0),
            tool(ToolKind::Ball, 6.0, 0.0),
            tool(ToolKind::Bullnose, 6.0, 1.0),
        ] {
            let mesh = cutter_mesh(&t);
            let points = || mesh.iter().flatten();
            let lowest = points().map(|p| p.z).fold(f32::MAX, f32::min);
            assert!(lowest.abs() < 1e-6, "{:?}: tip at {lowest}", t.kind);
            let widest = points().map(|p| p.truncate().length()).fold(0.0, f32::max);
            assert!((widest - 3.0).abs() < 1e-4, "{:?}: radius {widest}", t.kind);
            // Only the very tip sits on the axis at Z=0 for a ball; a flat's whole
            // bottom face is at Z=0.
            let on_floor = points()
                .filter(|p| p.z.abs() < 1e-6)
                .map(|p| p.truncate().length());
            let floor = on_floor.fold(0.0, f32::max);
            match t.kind {
                ToolKind::Flat => assert!((floor - 3.0).abs() < 1e-4),
                ToolKind::Ball => assert!(floor < 1e-4),
                ToolKind::Bullnose => assert!((floor - 2.0).abs() < 1e-4),
            }
        }
    }

    #[test]
    fn a_run_gives_each_operation_its_own_cutter() {
        let (_, _, scene) = scene();
        let width = |i: usize| {
            scene.ops[i]
                .cutter
                .iter()
                .flatten()
                .map(|p| p.truncate().length())
                .fold(0.0, f32::max)
        };
        assert!((width(0) - 2.0).abs() < 1e-4, "rough is a 4 mm flat");
        assert!((width(1) - 1.5).abs() < 1e-4, "finish is a 3 mm ball");
    }
}
