//! What a setup keeps that is not in the job: its last run, how it is laid out,
//! and how it is being looked at and played.

use std::time::Instant;

use camcraft_core::job::Job;
use camcraft_core::run::Stage;
use camcraft_core::setup::Prepared;
use camcraft_session::Ran;

use super::next_key;
use super::run::SetupInputs;
use crate::stock::live::LiveStock;
use crate::stock::stock_view::{ColorMode, StockImage};
use crate::view::camera::Camera;
use crate::view::scene::{Playhead, Scene};
use crate::view::viewport::{Outline, View};

/// Which material to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StockMode {
    /// None.
    Off,
    /// As it stands at the playhead, cut as the program plays.
    Follow,
    /// A fixed stage: the block before everything is 0, after operation `n` is
    /// `n + 1`.
    Stage(usize),
}

/// A setup's last run, and what is drawn from it.
#[derive(Debug, Clone)]
pub struct RanSetup {
    /// What it was run from.
    pub inputs: SetupInputs,
    /// The job as it was run.
    pub job: Job,
    /// What running it produced.
    pub ran: Ran,
    /// Its toolpaths and cutters, for drawing.
    pub scene: Scene,
    /// Tells this scene's upload from any other.
    pub scene_key: u64,
}

/// A setup laid out for drawing, before or without a run.
#[derive(Debug, Clone)]
pub struct Layout {
    /// What it was laid out from, and which of its models had loaded.
    pub(super) inputs: (SetupInputs, Vec<String>),
    /// The part and stock in the setup's frame, or why not.
    pub prepared: Result<Prepared, String>,
    /// Stock edges and the work-zero axes.
    pub outline: Vec<Outline>,
    /// Tells this layout's uploads from any other.
    pub key: u64,
}

/// How a setup is being looked at: its own camera, playhead and choices, kept
/// while another is shown.
#[derive(Debug, Clone)]
pub struct SetupView {
    /// The camera, once it has been shown.
    pub camera: Option<Camera>,
    /// Frames the camera once, on the first layout that has a part.
    pub framing: u64,
    /// Where the program stands: what the timeline, the keys and playback move.
    pub at: Playhead,
    /// Playing, and how.
    pub transport: Transport,
    /// The cutter at the playhead.
    pub show_cutter: bool,
    /// All toolpaths at once, over the per-role choices.
    pub show_paths: bool,
    /// Which material to draw.
    pub stock_mode: StockMode,
    /// The material covers the part, so the part is hidden while it shows.
    pub hide_part_with_stock: bool,
    /// What to draw.
    pub view: View,
    /// The material at each stage, classified and coloured, built when first shown,
    /// each with a key for its upload.
    pub stock_images: Vec<Option<(u64, StockImage)>>,
}

/// Playback: whether it plays, which way, how fast, and where it stops by itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transport {
    /// Playing.
    pub playing: bool,
    /// Playing backwards.
    pub reverse: bool,
    /// Machining seconds per second.
    pub speed: f64,
    /// Pause on reaching a failed check or a steep descent.
    pub stop_at_problems: bool,
    /// Pause at the end of each operation.
    pub stop_at_op_end: bool,
    /// How the material is coloured.
    pub color: ColorMode,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            playing: false,
            reverse: false,
            speed: 10.0,
            stop_at_problems: true,
            stop_at_op_end: false,
            color: ColorMode::Material,
        }
    }
}

impl Default for SetupView {
    fn default() -> Self {
        Self {
            camera: None,
            framing: next_key(),
            at: Playhead::default(),
            transport: Transport::default(),
            show_cutter: true,
            show_paths: true,
            stock_mode: StockMode::Off,
            hide_part_with_stock: true,
            view: View {
                stock: true,
                ..View::default()
            },
            stock_images: Vec::new(),
        }
    }
}

/// Everything about one setup that is not in the job.
#[derive(Debug, Default)]
pub struct SetupState {
    /// Its last run.
    pub ran: Option<RanSetup>,
    /// The run in flight, if any, and the job as it was sent.
    pub running: Option<(u64, Job)>,
    /// Where the run in flight has got to, and since when.
    pub progress: Option<(Stage, Instant)>,
    /// How it is laid out now.
    pub layout: Option<Layout>,
    /// How it is being looked at.
    pub view: SetupView,
    /// Its material during playback, once it has been followed.
    pub live: Option<LiveStock>,
}

impl SetupState {
    /// Forget its last run and everything drawn from it, the playhead and the
    /// material among them, as before it ever ran. The camera and what is ticked
    /// under Show are kept.
    pub fn forget_run(&mut self) {
        self.ran = None;
        self.live = None;
        let view = &mut self.view;
        view.at = Playhead::default();
        view.transport.playing = false;
        view.stock_images.clear();
    }
}
