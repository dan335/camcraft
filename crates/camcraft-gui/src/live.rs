//! The material during playback, cut on its own thread.
//!
//! Sweeping is the slow part of playback, so the UI thread never does it. Each
//! setup being played has a thread that owns a [`Live`] stock and an image of it.
//! The UI sends it where the program stands whenever that changes; the thread
//! works only on the newest position it has been sent, a few milliseconds at a
//! time, and sends back the cells that changed, recoloured, for the UI to copy into
//! its own image and the viewport to upload. So the window keeps its frame rate
//! while the material catches up, and a fast scrub costs no more than a slow one.

use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use camcraft_core::replay::{Changed, Live, Position, Replay};
use camcraft_sim::stock::{CellRect, PartSurface};
use eframe::egui;

use crate::document::{RanSetup, next_key};
use crate::stock_view::StockImage;
use crate::tiles::Tiles;
use crate::viewport::StockChange;

/// Memory the checkpoints of one setup's playback may take. Going back costs at
/// most sweeping the moves between two of them.
const CHECKPOINT_BYTES: usize = 256 << 20;

/// How long the thread sweeps before sending what it has, so the material is
/// seen to move during a long seek.
const STEP: Duration = Duration::from_millis(12);

/// Moves swept between looks at the clock, and between noting which tiles
/// changed: few, so that what is noted is close to what was cut.
const MOVES_PER_LOOK: usize = 4;

/// Cells a frame uploads at most, about 20 MB: after a long seek the rest follow
/// over the next frames, so no frame waits for a whole sheet.
const UPLOAD_CELLS: u64 = 4_000_000;

/// Where the program stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    /// Whole moves made.
    pub at: Position,
    /// How far along the next.
    pub fraction: f64,
}

/// What the thread tells the UI: what changed in the image they share, if
/// anything, on the way to a target, and whether it got there.
#[derive(Debug)]
struct Update {
    target: Target,
    reached: bool,
    change: Option<Change>,
}

/// What changed in the shared image.
#[derive(Debug)]
enum Change {
    /// All of it: it is a new image.
    Full,
    /// These cells.
    Cells(Vec<CellRect>),
}

/// The image of the material the thread writes and the UI draws: one, shared,
/// so a sheet's worth of cells is not held twice. Empty until the thread first
/// fills it.
type Shared = Arc<Mutex<StockImage>>;

fn lock(shared: &Shared) -> MutexGuard<'_, StockImage> {
    // A panic elsewhere leaves the image as it was: still fit to draw.
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One setup's playback material, as the UI holds it.
#[derive(Debug)]
pub struct LiveStock {
    /// Which run it replays.
    pub scene_key: u64,
    to_thread: Sender<Target>,
    from_thread: Receiver<Update>,
    /// The last target sent.
    sent: Option<Target>,
    /// The material, as the thread last left it.
    image: Shared,
    /// Tells one whole image from the next, for the viewport.
    key: u64,
    /// What changed since the viewport last uploaded: all of it, or these tiles.
    pending_all: bool,
    pending: Option<Tiles>,
    /// The last target the thread reached.
    reached: Option<Target>,
}

impl LiveStock {
    /// Start replaying `ran`, or `None` when it cannot be.
    pub fn start(ctx: &egui::Context, ran: &RanSetup) -> Option<Self> {
        let result = &ran.ran.result;
        let setup = ran.job.setups.iter().find(|s| s.name == result.setup)?;
        let replay = Replay::new(&ran.job, setup, result).ok()?;
        let part = Arc::clone(&result.part);
        let budget = result.verify_budget;
        let (to_thread, rx) = channel();
        let (tx, from_thread) = channel();
        let image: Shared = Arc::new(Mutex::new(StockImage::empty()));
        let shared = Arc::clone(&image);
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("camcraft playback".into())
            .spawn(move || run(&replay, &part, budget, &shared, &rx, &tx, &ctx))
            .ok()?;
        Some(Self {
            scene_key: ran.scene_key,
            to_thread,
            from_thread,
            sent: None,
            image,
            key: next_key(),
            pending_all: false,
            pending: None,
            reached: None,
        })
    }

    /// Ask for the material at `target`, unless that is what was last asked for.
    pub fn seek(&mut self, target: Target) {
        if self.sent != Some(target) && self.to_thread.send(target).is_ok() {
            self.sent = Some(target);
        }
    }

    /// Take in what the thread has said.
    pub fn poll(&mut self) {
        while let Ok(update) = self.from_thread.try_recv() {
            match update.change {
                Some(Change::Full) => {
                    let (nx, ny) = {
                        let image = lock(&self.image);
                        (image.nx, image.ny)
                    };
                    self.pending = Some(Tiles::new(nx, ny));
                    self.key = next_key();
                    self.pending_all = true;
                }
                Some(Change::Cells(cells)) => {
                    if let Some(pending) = &mut self.pending {
                        for rect in cells {
                            pending.mark(rect);
                        }
                    }
                }
                None => {}
            }
            if update.reached {
                self.reached = Some(update.target);
            }
        }
    }

    /// Whether the material stands where it was last asked to.
    pub fn caught_up(&self) -> bool {
        self.sent.is_some() && self.reached == self.sent
    }

    /// The material to draw: its key, the image (shared with the thread, to be
    /// locked only while it is read), and what changed in it since the viewport
    /// last uploaded it.
    pub fn shown(&self) -> Option<(u64, &Mutex<StockImage>, StockChange)> {
        if lock(&self.image).nx == 0 {
            return None;
        }
        let change = match &self.pending {
            _ if self.pending_all => StockChange::All,
            Some(tiles) if !tiles.is_empty() => StockChange::Cells(tiles.first(UPLOAD_CELLS)),
            _ => StockChange::Nothing,
        };
        Some((self.key, &*self.image, change))
    }

    /// The viewport has uploaded what [`LiveStock::shown`] gave it.
    pub fn uploaded(&mut self) {
        if self.pending_all {
            self.pending_all = false;
            if let Some(t) = &mut self.pending {
                t.clear();
            }
        } else if let Some(t) = &mut self.pending {
            for r in t.first(UPLOAD_CELLS) {
                t.unmark(r);
            }
        }
    }

    /// Whether there is more to upload than one frame takes.
    pub fn uploading(&self) -> bool {
        self.pending_all || self.pending.as_ref().is_some_and(|t| !t.is_empty())
    }
}

/// One stretch of the thread's work: sweep toward `t` until `until` or it is
/// there, and classify again the tiles that changed, into the shared image.
/// Returns what changed, and whether it got there.
fn work(
    live: &mut Live,
    replay: &Replay,
    (part, budget): (&PartSurface, f64),
    shared: &Shared,
    t: Target,
    until: Instant,
) -> (Option<Change>, bool) {
    let (nx, ny) = (live.stock().field().nx(), live.stock().field().ny());
    let mut tiles = Tiles::new(nx, ny);
    let mut all = false;
    let mut reached = false;
    while !reached && Instant::now() < until {
        let (changed, r) = live.seek(replay, t.at, t.fraction, MOVES_PER_LOOK);
        match changed {
            Changed::Cells(rect) => tiles.mark(rect),
            Changed::All => all = true,
            Changed::Nothing => {}
        }
        reached = r;
    }
    let fresh = {
        let image = lock(shared);
        image.nx != nx || image.ny != ny
    };
    if all || fresh {
        // Built unlocked, so the UI can draw the old one meanwhile.
        let image = StockImage::build(live.stock(), part, budget);
        *lock(shared) = image;
        return (Some(Change::Full), reached);
    }
    if tiles.is_empty() {
        return (None, reached);
    }
    let rects = tiles.rects();
    lock(shared).patch_many(live.stock(), part, budget, &rects);
    (Some(Change::Cells(rects)), reached)
}

/// The thread: seek toward the newest target, and say what changed.
fn run(
    replay: &Replay,
    part: &PartSurface,
    budget: f64,
    shared: &Shared,
    rx: &Receiver<Target>,
    tx: &Sender<Update>,
    ctx: &egui::Context,
) {
    let Some(mut live) = Live::new(replay, CHECKPOINT_BYTES) else {
        return;
    };
    // The stock swept ahead below drops the checkpoints; this one only seeks.
    live.set_dropping(false);
    // A second stock, swept through the whole program while there is nothing else
    // to do, dropping checkpoints for the first: then going back anywhere costs at
    // most one spacing of sweeping, even to where playback has never been.
    let mut ahead = Live::new(replay, CHECKPOINT_BYTES);
    let mut target: Option<Target> = None;
    loop {
        // Idle: sweep ahead, or wait once that is done. Otherwise only look for a
        // newer target.
        while target.is_none() {
            if let Some(a) = &mut ahead {
                match rx.try_recv() {
                    Ok(t) => target = Some(t),
                    Err(TryRecvError::Disconnected) => return,
                    Err(TryRecvError::Empty) => {
                        let started = Instant::now();
                        let mut done = false;
                        while !done && started.elapsed() < STEP {
                            done = a.sweep_ahead(replay, 32);
                        }
                        live.adopt_checkpoints(a);
                        if done {
                            ahead = None;
                        }
                    }
                }
            } else {
                match rx.recv() {
                    Ok(t) => target = Some(t),
                    Err(_) => return,
                }
            }
        }
        loop {
            match rx.try_recv() {
                Ok(t) => target = Some(t),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        let Some(t) = target else { continue };

        let until = Instant::now() + STEP;
        let (change, reached) = work(&mut live, replay, (part, budget), shared, t, until);
        if change.is_some() || reached {
            let update = Update {
                target: t,
                reached,
                change,
            };
            if tx.send(update).is_err() {
                return;
            }
            ctx.request_repaint();
        }
        if reached {
            target = None;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::print_stdout, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::scene::{Playhead, Scene};
    use crate::test_jobs::run_setup_zero;
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
        let lego: Job = toml::from_str(
            r#"
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
strategy = "zlevel-rough"
stepdown = 2.0
min_ring_length = 3.0
tool = "rough-6-flat"
"#,
        )
        .unwrap();
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
            crate::scene::clock(scene.total_time()),
            live.stock().field().nx(),
            live.stock().field().ny()
        );

        let frame = |live: &mut Live, to: Playhead| {
            let started = Instant::now();
            let (at, fraction) = scene.target(to);
            let target = Target { at, fraction };
            // As the thread does, but to the end rather than for a while.
            let far = started + Duration::from_secs(3600);
            while !work(live, &replay, (part, budget), &image, target, far).1 {}
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
        let finished = result.stages.last().unwrap().unpack().unwrap();
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
}
