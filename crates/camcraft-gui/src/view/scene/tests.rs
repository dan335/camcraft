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
strategy = "pocket-clearing"
tool = "flat"
passes = { stepdown = 4.0, stepover = 2.0, feed = 1800.0, stock_to_leave = { radial = 0.3, axial = 0.3 } }
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
    assert!((duration(100.0, Some(1000.0)) - 6.0).abs() < 1e-12);
    let rapid = duration(50.0, None);
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

/// The playhead, as a replay position, counts the moves made in the operation
/// it is on, and lands exactly on each operation's end.
#[test]
fn playhead_positions_are_replay_positions() {
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
            let dur = scene.time_of(Playhead::at(shown + 1)) - scene.time_of(Playhead::at(shown));
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
fn the_playhead_names_the_move_it_stands_on() {
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
