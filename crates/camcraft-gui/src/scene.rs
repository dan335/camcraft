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

use camcraft_core::job::{Job, Setup, ToolSpec};
use camcraft_core::replay::Position;
use camcraft_core::run::{PLUNGE_RATIO, SetupResult};
use camcraft_core::toolpath::MoveRole;
use camcraft_geom::heightmap::narrow;
use camcraft_geom::mesh::Aabb;
use glam::{DVec3, Vec3};

use crate::theme;
use crate::tool_drawing::tip_profile;

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
    /// Machining time at the end of this move, in seconds from the start.
    pub ends_at: f64,
}

/// Rapid rate assumed for machining time until a machine profile supplies one, in
/// mm/min. Times are labelled as estimates for this reason.
pub const ASSUMED_RAPID: f64 = 5000.0;

/// A cutting move that drops further than its operation allows while barely
/// advancing: a plunge at cutting feed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Steep {
    /// Its index in [`Scene::segments`].
    pub index: usize,
    /// Which operation, from zero.
    pub op: usize,
    /// How far it drops in all, in millimetres.
    pub drop: f64,
    /// Where it ends.
    pub at: DVec3,
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
    /// Segments per role, indexed as [`theme::ROLES`].
    pub role_counts: [usize; 7],
    /// Continuous cutting descents steeper than [`PLUNGE_RATIO`] and deeper than
    /// their operation's `max_cut_step`, in machine order, each at its last move.
    pub steep: Vec<Steep>,
}

/// Where the program stands: `shown` whole segments made, and `fraction` of the
/// next one. The one position everything is drawn from: the paths up to it, the
/// cutter on it, the material as it stands there, and the clock.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Playhead {
    /// Segments made, in machine order.
    pub shown: usize,
    /// How far along segment `shown` the tool is, from 0 up to but not 1.
    pub fraction: f64,
}

impl Playhead {
    /// After `shown` whole segments.
    pub fn at(shown: usize) -> Self {
        Self {
            shown,
            fraction: 0.0,
        }
    }
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
    /// The scene for a finished run of `setup`, in `job`.
    pub fn build(job: &Job, setup: &Setup, result: &SetupResult) -> Self {
        let mut scene = Self::default();
        let mut clock = 0.0;
        for (i, run) in result.operations.iter().enumerate() {
            let op = u32::try_from(i).unwrap_or(u32::MAX);
            let first = scene.segments.len();
            let allowance = setup
                .operations
                .get(i)
                .map_or(f64::INFINITY, |o| o.passes.max_cut_step);
            for pair in run.path.moves().windows(2) {
                let [from, to] = pair else { continue };
                let (a, b) = (from.to, to.to);
                clock += duration(a, b, to.feed);
                scene.segments.push(Segment {
                    a,
                    b,
                    role: to.role,
                    op,
                    feed: to.feed,
                    ends_at: clock,
                });
                let r = theme::role_index(to.role) as usize;
                if let Some(c) = scene.role_counts.get_mut(r) {
                    *c += 1;
                }
            }
            // The core's own finding, by the core's rule: continuous descents
            // steeper than the plunge ratio, too deep for the allowance. Segment `k`
            // of this operation ends at move `k + 1`.
            let moves = run.path.moves();
            let start = moves.first().map_or(DVec3::ZERO, |m| m.to);
            for d in run.path.steep_descents(start, PLUNGE_RATIO) {
                if d.depth > allowance
                    && let Some(end) = moves.get(d.last)
                {
                    scene.steep.push(Steep {
                        index: first + d.last.saturating_sub(1),
                        op: i,
                        drop: d.depth,
                        at: end.to,
                    });
                }
            }
            let cutter = setup
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
        scene
    }

    /// Estimated machining time for the whole program, in seconds.
    pub fn total_time(&self) -> f64 {
        self.segments.last().map_or(0.0, |s| s.ends_at)
    }

    /// Where the program stands when `shown` segments are showing, as a replay
    /// position: an operation, and how many of its moves have been made.
    ///
    /// At the end of an operation this is that operation complete, not the next one
    /// unstarted: the material is the same either way, and the operation the scrub
    /// is on is the one to name.
    pub fn position(&self, shown: usize) -> Position {
        match self.at(shown) {
            // A segment is a move after the operation's first, so showing k of an
            // operation's segments means k + 1 of its moves are made.
            Some(at) => {
                let first = self.ops.get(at.op).map_or(0, |o| o.first);
                Position {
                    op: at.op,
                    moves: shown - first + 1,
                }
            }
            None => Position { op: 0, moves: 0 },
        }
    }

    /// Machining time at `p`, in seconds from the start.
    pub fn time_of(&self, p: Playhead) -> f64 {
        let start = p
            .shown
            .checked_sub(1)
            .and_then(|i| self.segments.get(i))
            .map_or(0.0, |s| s.ends_at);
        match self.segments.get(p.shown) {
            Some(s) => start + (s.ends_at - start) * p.fraction,
            None => start,
        }
    }

    /// Where the program stands after `seconds` of machining.
    pub fn locate(&self, seconds: f64) -> Playhead {
        let shown = self.segments.partition_point(|s| s.ends_at <= seconds);
        let Some(s) = self.segments.get(shown) else {
            return Playhead::at(self.segments.len());
        };
        let start = shown
            .checked_sub(1)
            .and_then(|i| self.segments.get(i))
            .map_or(0.0, |s| s.ends_at);
        let span = s.ends_at - start;
        let fraction = if span > 0.0 {
            ((seconds - start) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        // Numerically at the end is the end: the next segment, not all of this one.
        if fraction >= 1.0 {
            return Playhead::at(shown + 1);
        }
        Playhead { shown, fraction }
    }

    /// `seconds` of machining on from `p`, or back for a negative number, held to
    /// the program.
    pub fn advance(&self, p: Playhead, seconds: f64) -> Playhead {
        let t = (self.time_of(p) + seconds).clamp(0.0, self.total_time());
        if seconds > 0.0 && t >= self.total_time() {
            return Playhead::at(self.segments.len());
        }
        self.locate(t)
    }

    /// The cutter at `p`: its operation and where its tip is.
    pub fn cutter_at(&self, p: Playhead) -> Option<(usize, DVec3)> {
        match self.segments.get(p.shown) {
            Some(s) => Some((s.op as usize, s.a.lerp(s.b, p.fraction))),
            None => self.segments.last().map(|s| (s.op as usize, s.b)),
        }
    }

    /// `p` as a replay position, and how far along the move after it.
    pub fn target(&self, p: Playhead) -> (Position, f64) {
        if p.fraction > 0.0
            && let Some(s) = self.segments.get(p.shown)
        {
            let op = s.op as usize;
            let first = self.ops.get(op).map_or(0, |o| o.first);
            // Segment `k` of an operation joins its moves `k` and `k + 1`: part-way
            // along it, `k + 1` moves are made.
            return (
                Position {
                    op,
                    moves: p.shown - first + 1,
                },
                p.fraction,
            );
        }
        (self.position(p.shown), 0.0)
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

/// How long a move takes, in seconds: at its feed, or the assumed rapid rate.
fn duration(a: DVec3, b: DVec3, feed: Option<f64>) -> f64 {
    let rate = feed.filter(|f| *f > 0.0).unwrap_or(ASSUMED_RAPID);
    (b - a).length() / rate * 60.0
}

/// Seconds as `m:ss`, or `h:mm:ss` past an hour.
pub fn clock(seconds: f64) -> String {
    // Whole seconds of a machining program: far inside u64.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let s = seconds.max(0.0).round() as u64;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// The twelve edges of a box.
pub fn box_edges(b: &Aabb) -> Vec<(DVec3, DVec3)> {
    let (lo, hi) = (b.min, b.max);
    let corner = |i: u8| {
        DVec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
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
/// The cutter as triangles: its profile revolved about +Z, tip at the origin, with
/// a shank three diameters long, or longer for a V-bit's long flank.
pub fn cutter_mesh(tool: &ToolSpec) -> Vec<[Vec3; 3]> {
    let mut profile: Vec<(f32, f32)> = tip_profile(tool)
        .into_iter()
        .map(|(r, z)| (narrow(r), narrow(z)))
        .collect();
    let (radius, end) = profile.last().copied().unwrap_or((0.5, 0.0));
    let height = (radius * 6.0).max(end + radius * 2.0);
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
    use crate::test_jobs::run_setup_zero;
    use camcraft_core::job::ToolKind;

    fn repo(path: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path)
    }

    const JOB: &str = r#"
version = 1
tolerance = 0.1
[tools.flat]
kind = "flat"
diameter = 4.0
[tools.ball]
kind = "ball"
diameter = 3.0
number = 2
[[model]]
name = "hinge"
path = "fixtures/step/hinge_jig.step"
[[stock]]
name = "block"
kind = "around-models"
margin = [2.0, 2.0, 2.0]
[[setup]]
name = "top"
models = ["hinge"]
stocks = ["block"]
[[setup.operation]]
name = "rough"
strategy = "zlevel-rough"
tool = "flat"
stepdown = 4.0
passes = { stepover = 2.0, feed = 1800.0, stock_to_leave = { radial = 0.3, axial = 0.3 } }
[[setup.operation]]
name = "finish"
strategy = "parallel"
tool = "ball"
passes = { stepover = 2.0, feed = 2000.0 }
"#;

    /// A real two-operation run on a real part, built once and shared by the tests
    /// below. Parsed from a string, not a file: tests run in parallel, and a shared
    /// temporary file is a race between one test writing it and another reading it.
    fn scene() -> (Job, SetupResult, Scene) {
        static RUN: std::sync::OnceLock<(Job, SetupResult, Scene)> = std::sync::OnceLock::new();
        RUN.get_or_init(|| {
            let job: Job = toml::from_str(JOB).unwrap();
            let ran = run_setup_zero(&job, &repo(""));
            let scene = Scene::build(&job, &job.setups[0], &ran.result);
            (job, ran.result, scene)
        })
        .clone()
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

    /// The steep list is the core's own finding: its deepest drop per operation is
    /// the one the run reported, and the ratio-steep count matches the core's.
    #[test]
    fn steep_descents_are_the_ones_the_core_found() {
        let (job, result, scene) = scene();
        for (i, run) in result.operations.iter().enumerate() {
            let allowance = job.setups[0].operations[i].passes.max_cut_step;
            let mine: Vec<&Steep> = scene.steep.iter().filter(|s| s.op == i).collect();
            if run.deepest_cut_descent_mm > allowance {
                let deepest = mine.iter().map(|s| s.drop).fold(0.0, f64::max);
                assert!(
                    (deepest - run.deepest_cut_descent_mm).abs() < 1e-9,
                    "{}",
                    run.name
                );
            } else {
                assert!(
                    mine.is_empty(),
                    "{}: listed moves the core did not flag",
                    run.name
                );
            }
            for s in mine {
                let seg = &scene.segments[s.index];
                assert_eq!(seg.role, MoveRole::Cut);
                assert_eq!(seg.b, s.at);
                assert!(s.drop > allowance);
            }
            // Every continuous ratio-steep descent, whatever its depth, is what the
            // core counted: runs of steep cutting segments, counted here afresh.
            let span = &scene.ops[i];
            let mut runs = 0;
            let mut inside = false;
            for g in &scene.segments[span.first..span.first + span.count] {
                let fall = g.a.z - g.b.z;
                let steep = g.role == MoveRole::Cut
                    && fall > 0.0
                    && fall > (g.b.truncate() - g.a.truncate()).length() * PLUNGE_RATIO;
                if steep && !inside {
                    runs += 1;
                }
                inside = steep;
            }
            assert_eq!(runs, run.steep_cut_descents, "{}", run.name);
        }
    }

    #[test]
    fn machining_time_is_length_over_feed() {
        assert!(
            (duration(DVec3::ZERO, DVec3::new(100.0, 0.0, 0.0), Some(1000.0)) - 6.0).abs() < 1e-12
        );
        let rapid = duration(DVec3::ZERO, DVec3::new(0.0, 0.0, 50.0), None);
        assert!((rapid - 50.0 / ASSUMED_RAPID * 60.0).abs() < 1e-12);
        let (_, _, scene) = scene();
        let mut last = 0.0;
        for s in &scene.segments {
            assert!(s.ends_at >= last, "time runs forwards");
            last = s.ends_at;
        }
        assert!(scene.total_time() > 0.0);
        assert_eq!(clock(59.6), "1:00");
        assert_eq!(clock(3725.0), "1:02:05");
    }

    /// The scrub, as a replay position, counts the moves made in the operation it is
    /// on, and lands exactly on each operation's end.
    #[test]
    fn scrub_positions_are_replay_positions() {
        let (_, result, scene) = scene();
        assert_eq!(scene.position(0), Position { op: 0, moves: 0 });
        assert_eq!(scene.position(1), Position { op: 0, moves: 2 });
        for (i, span) in scene.ops.iter().enumerate() {
            let end = scene.position(span.first + span.count);
            assert_eq!(end.op, i);
            assert_eq!(end.moves, result.operations[i].path.moves().len());
        }
    }

    /// Time and position are one: every playhead survives the round trip, and time
    /// runs on across operations.
    #[test]
    fn a_playhead_is_a_time() {
        let (_, _, scene) = scene();
        let total = scene.segments.len();
        for shown in [
            0,
            1,
            7,
            scene.ops[1].first,
            scene.ops[1].first + 3,
            total - 1,
        ] {
            for fraction in [0.0, 0.25, 0.9] {
                let p = Playhead { shown, fraction };
                let back = scene.locate(scene.time_of(p));
                // A zero-length move has no time of its own to find again.
                let dur =
                    scene.time_of(Playhead::at(shown + 1)) - scene.time_of(Playhead::at(shown));
                if dur > 1e-9 {
                    assert_eq!(back.shown, shown, "{p:?}");
                    assert!((back.fraction - fraction).abs() < 1e-6, "{p:?} -> {back:?}");
                }
            }
        }
        assert_eq!(scene.locate(-1.0), Playhead::at(0));
        assert_eq!(scene.locate(scene.total_time() + 1.0), Playhead::at(total));
        assert_eq!(
            scene.advance(Playhead::at(0), scene.total_time() * 2.0),
            Playhead::at(total)
        );
        assert_eq!(scene.advance(Playhead::at(total), -1e9), Playhead::at(0));
        // Across the tool change: from the end of roughing into finishing.
        let end = Playhead::at(scene.ops[1].first);
        let on = scene.advance(end, 1.0);
        assert!(scene.segments[on.shown].op == 1, "{on:?}");
        let back = scene.advance(on, -1.0);
        assert!((scene.time_of(back) - scene.time_of(end)).abs() < 1e-6);
    }

    /// Part-way along a segment is one more move made than the segment starts at,
    /// in the operation the segment belongs to, and the cutter is on the segment.
    #[test]
    fn a_playhead_part_way_is_a_replay_position() {
        let (_, _, scene) = scene();
        for shown in [0, 5, scene.ops[1].first] {
            assert_eq!(
                scene.target(Playhead::at(shown)),
                (scene.position(shown), 0.0)
            );
            let (at, f) = scene.target(Playhead {
                shown,
                fraction: 0.5,
            });
            assert_eq!(f, 0.5);
            // Finishing that segment makes the move it is heading for.
            let done = scene.position(shown + 1);
            assert_eq!((at.op, at.moves + 1), (done.op, done.moves));
            let (op, tip) = scene
                .cutter_at(Playhead {
                    shown,
                    fraction: 0.5,
                })
                .unwrap();
            let s = &scene.segments[shown];
            assert_eq!(op, s.op as usize);
            assert!((tip - (s.a + s.b) * 0.5).length() < 1e-9);
        }
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
    fn a_box_is_twelve_edges_along_the_axes() {
        let (_, result, _) = scene();
        let edges = box_edges(&result.stocks[0]);
        assert_eq!(edges.len(), 12);
        for (a, b) in &edges {
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
        let top = edges
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
            ..ToolSpec::default()
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
            ToolSpec {
                angle: Some(90.0),
                tip_diameter: 1.0,
                ..tool(ToolKind::Cone, 6.0, 0.0)
            },
            tool(ToolKind::Drill, 6.0, 0.0),
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
                ToolKind::Cone => assert!((floor - 0.5).abs() < 1e-4, "{floor}"),
                ToolKind::Drill => assert!(floor < 1e-4, "{floor}"),
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
