use super::*;
use crate::test_jobs::run_setup_zero;
use crate::view::scene::{Playhead, Scene};
use camcraft_core::job::Job;

/// How playback keeps up on the router jig, and on the lego brick at its
/// fine pitch: play each through at 1000x in 60 Hz frames, doing each
/// frame's sweeping and patching as the thread does, then drag back. Prints
/// the worst frame; a frame's work runs off the UI thread, so a slow one
/// delays the material, not the window.
/// `cargo test --release -p camcraft-gui -- --ignored keeps_up --nocapture`.
#[test]
#[ignore = "runs the router jig and the lego brick: tens of seconds"]
fn keeps_up() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = repo.join("examples/rail-jig.camcraft");
    let jig: Job = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    println!("== router jig");
    keeps_up_on(&jig, &repo.join("examples"));
    let lego: Job = toml::from_str(LEGO).unwrap();
    println!("== lego brick");
    keeps_up_on(&lego, &repo);
    let sheet = std::env::var("CAMCRAFT_SHEET_JOB").ok();
    if let Some(path) = sheet {
        let path = std::path::PathBuf::from(path);
        let job: Job = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        println!("== sheet");
        keeps_up_on(&job, path.parent().unwrap());
    }
}

/// The lego brick, roughed with a quarter-inch flat.
const LEGO: &str = r#"
version = 1
tolerance = 0.05
[tools.rough-6-flat]
kind = "flat"
diameter = 6.35
number = 1
[[model]]
name = "lego"
path = "fixtures/step/lego_4x2_01.step"
[[stock]]
name = "block"
kind = "around-models"
margin = [2.0, 2.0, 2.0]
[[setup]]
name = "top"
models = ["lego"]
stocks = ["block"]
[[setup.operation]]
name = "rough"
strategy = "pocket-clearing"
passes = { stepdown = 2.0 }
min_ring_length = 3.0
tool = "rough-6-flat"
"#;

/// The UI knows whether there is material to draw, what changed of it and
/// what the legend counts without locking the image the thread writes: all
/// of it here with the image held locked, as the thread holds it working.
#[test]
fn the_ui_reads_the_material_without_locking_it() {
    use crate::stock::stock_view::Class;
    let (to_thread, _to) = channel();
    let (tx, from_thread) = channel();
    let mut live = LiveStock {
        scene_key: 0,
        to_thread,
        from_thread,
        sent: None,
        image: Arc::new(Mutex::new(StockImage::empty())),
        key: next_key(),
        pending_all: false,
        pending: None,
        counts: [0; 4],
        reached: None,
        shaded: false,
    };
    let held = Arc::clone(&live.image);
    let _working = lock(&held);
    assert!(live.shown().is_none() && !live.filled());
    let target = Goal {
        at: Position { op: 0, moves: 3 },
        fraction: 0.0,
    };
    let update = |change, counts| Update {
        target,
        reached: true,
        change: Some(change),
        shaded: false,
        counts,
    };
    tx.send(update(Change::Full(64, 40), Some([1, 2, 3, 4])))
        .unwrap();
    live.poll();
    let (key, _, change) = live.shown().unwrap();
    assert_eq!(change, StockChange::All);
    assert_eq!(live.count(Class::Leftover), 2);
    live.uploaded();
    assert_eq!(live.shown().unwrap().2, StockChange::Nothing);
    let cut = CellRect {
        x0: 3,
        y0: 5,
        x1: 9,
        y1: 6,
    };
    tx.send(update(Change::Cells(vec![cut]), None)).unwrap();
    live.poll();
    let (again, _, change) = live.shown().unwrap();
    assert_eq!(again, key, "the same image");
    assert!(matches!(change, StockChange::Cells(r) if r.iter().any(|r| r.contains(3, 5))));
    assert_eq!(live.count(Class::Leftover), 2, "kept");
}

/// A cut is shaded as it is cut. Playing, frame after frame with no time to
/// catch up on the shading round the cuts, the cells a frame cut still carry
/// the shade of the material as it now stands, not of what was cut away.
#[test]
fn a_cut_is_shaded_as_it_is_cut() {
    // The router jig, whose shade reaches far enough for a pocket's floor to
    // be darker by its walls than the top it was cut from.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let text = std::fs::read_to_string(dir.join("rail-jig.camcraft")).unwrap();
    let text = text.replace("sled_base.step", "hinge_jig.step");
    let job = camcraft_session::parse_job(&text, "rail-jig").unwrap();
    let ran = run_setup_zero(&job, &dir);
    let result = &ran.result;
    let setup = &job.setups[0];
    let scene = Scene::build(&job, setup, result);
    let replay = Replay::new(&job, setup, result).unwrap();
    let mut live = Live::new(&replay, CHECKPOINT_BYTES).unwrap();
    let (part, budget) = (&result.part, result.verify_budget);
    let image: Shared = Arc::new(Mutex::new(StockImage::empty()));
    let mut shading = Shading::default();
    let mut frame = |live: &mut Live, to: Playhead| {
        let (at, fraction) = scene.target(to);
        let far = Instant::now() + Duration::from_secs(3600);
        let mut change = None;
        loop {
            let (c, reached) = work(
                live,
                &replay,
                (part, budget),
                (&image, &mut shading),
                Goal { at, fraction },
                far,
            );
            change = merge(change, c);
            if reached {
                return change;
            }
        }
    };
    let mut at = scene.locate(scene.total_time() * 0.4);
    frame(&mut live, at);
    let mut cut = Vec::new();
    for _ in 0..30 {
        at = scene.advance(at, 20.0 / 60.0);
        if let Some(Change::Cells(rects)) = frame(&mut live, at) {
            cut = rects;
        }
    }
    assert!(!cut.is_empty(), "the last frame cut nothing");
    let (fresh, _) = StockImage::build_shaded(live.stock(), part, budget);
    let image = lock(&image);
    let (mut cells, mut off) = (0, 0);
    for r in &cut {
        for y in r.y0..=r.y1 {
            for x in r.x0..=r.x1 {
                let i = y as usize * image.nx as usize + x as usize;
                cells += 1;
                off += usize::from(image.ao[i] != fresh.ao[i]);
            }
        }
    }
    assert_eq!(
        off, 0,
        "{off} of {cells} cells just cut shaded as they were"
    );
}

fn keeps_up_on(job: &Job, dir: &std::path::Path) {
    let job = job.clone();
    let ran = run_setup_zero(&job, dir);
    let result = &ran.result;
    let setup = &job.setups[0];
    let scene = Scene::build(&job, setup, result);
    let replay = Replay::new(&job, setup, result).unwrap();
    let mut live = Live::new(&replay, CHECKPOINT_BYTES).unwrap();
    let (part, budget) = (&result.part, result.verify_budget);
    let image: Shared = Arc::new(Mutex::new(StockImage::empty()));
    println!(
        "{} moves, {} of machining, {} x {} cells",
        scene.segments.len(),
        crate::view::scene::clock(scene.total_time()),
        live.stock().field().nx(),
        live.stock().field().ny()
    );

    let mut shading = Shading::default();
    let mut frame = |live: &mut Live, to: Playhead| {
        let started = Instant::now();
        let (at, fraction) = scene.target(to);
        let target = Goal { at, fraction };
        // As the thread does, but to the end rather than for a while, with
        // its few milliseconds of catching up on the shading.
        let far = started + Duration::from_secs(3600);
        while !work(
            live,
            &replay,
            (part, budget),
            (&image, &mut shading),
            target,
            far,
        )
        .1
        {}
        shading.catch_up(&image, Instant::now() + SHADE_STEP);
        started.elapsed()
    };

    let (mut worst, mut sum, mut frames) = (Duration::ZERO, Duration::ZERO, 0u32);
    let mut at = Playhead::default();
    while at.shown < scene.segments.len() {
        at = scene.advance(at, 1000.0 / 60.0);
        let took = frame(&mut live, at);
        worst = worst.max(took);
        sum += took;
        frames += 1;
    }
    println!(
        "1000x: {frames} frames, mean {:.2} ms, worst {:.2} ms, {} checkpoints",
        sum.as_secs_f64() * 1e3 / f64::from(frames),
        worst.as_secs_f64() * 1e3,
        live.checkpoints()
    );
    let finished = result.keyframes.last().unwrap().unpack().unwrap();
    let whole = &finished.field().data();
    assert_eq!(
        live.stock().field().data(),
        *whole,
        "played to the end is the end"
    );

    let middle = scene.locate(scene.total_time() / 2.0);
    let back = frame(&mut live, middle);
    println!("back to the middle: {:.2} ms", back.as_secs_f64() * 1e3);
    let start = frame(&mut live, Playhead::default());
    println!("back to the start: {:.2} ms", start.as_secs_f64() * 1e3);

    // Straight to the end on opening, then back into roughing: cold, and after
    // the idle sweep ahead has dropped its checkpoints.
    let rough_end = scene.ops.get(1).map_or(scene.segments.len(), |o| o.first);
    let into_rough = scene.locate(scene.time_of(Playhead::at(rough_end)) * 0.8);
    let mut cold = Live::new(&replay, CHECKPOINT_BYTES).unwrap();
    frame(&mut cold, Playhead::at(scene.segments.len()));
    let took = frame(&mut cold, into_rough);
    println!(
        "cold, end to 80% of roughing: {:.0} ms",
        took.as_secs_f64() * 1e3
    );
    let mut warm = Live::new(&replay, CHECKPOINT_BYTES).unwrap();
    frame(&mut warm, Playhead::at(scene.segments.len()));
    let mut ahead = Live::new(&replay, CHECKPOINT_BYTES).unwrap();
    let started = Instant::now();
    while !ahead.sweep_ahead(&replay, MOVES_PER_LOOK) {}
    warm.adopt_checkpoints(&mut ahead);
    println!(
        "sweeping ahead: {:.1} s idle, {} checkpoints",
        started.elapsed().as_secs_f64(),
        warm.checkpoints()
    );
    let took = frame(&mut warm, into_rough);
    println!(
        "warm, end to 80% of roughing: {:.1} ms",
        took.as_secs_f64() * 1e3
    );
}
