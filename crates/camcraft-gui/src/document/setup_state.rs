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
use crate::stock::stock_view::ColorMode;
use crate::transport::{Mark, marks};
use crate::ui::theme;
use crate::view::camera::Camera;
use crate::view::scene::{Playhead, Scene};
use crate::view::viewport::{Outline, View};

/// What the view is for: setting the setup up, or playing its program back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// The part, the stock's outline, and the selection's toolpaths in full.
    #[default]
    Setup,
    /// The material cut as far as the playhead, the cutter there, and the
    /// transport.
    Playback,
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
    /// Where the checks found something and the steep descents, as the timeline
    /// marks them and playing stops at them ([`marks`]): found once, not every
    /// frame they are drawn.
    pub marks: Vec<(usize, Mark)>,
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
    /// Setting up, or playing back.
    pub mode: ViewMode,
    /// The cutter at the playhead.
    pub show_cutter: bool,
    /// Toolpaths while setting up, over the per-role choices.
    pub show_paths: bool,
    /// Toolpaths while playing back, as far as the playhead: off until asked for,
    /// the material being what playback is for.
    pub paths_in_playback: bool,
    /// The part while playing back: off until asked for, the material covering it.
    pub part_in_playback: bool,
    /// What to draw.
    pub view: View,
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
            mode: ViewMode::Setup,
            show_cutter: true,
            show_paths: true,
            paths_in_playback: false,
            part_in_playback: false,
            view: View {
                stock: true,
                ..View::default()
            },
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
    }

    /// Keep only the first `kept` operations of its last run: their toolpaths
    /// and material. The playhead stops and comes back to their end if
    /// it was past it. None kept is the run forgotten.
    pub fn cut_run_back(&mut self, kept: usize) {
        let Some(ran) = self.ran.as_mut() else {
            return;
        };
        if kept == 0 {
            self.forget_run();
            return;
        }
        let result = &mut ran.ran.result;
        result.operations.truncate(kept);
        result.keyframes.truncate(kept + 1);
        keep_first(&mut ran.scene, kept);
        ran.marks = marks(&ran.ran, &ran.scene);
        ran.scene_key = next_key();
        self.live = None;
        let view = &mut self.view;
        let end = ran.scene.segments.len();
        if view.at.shown >= end {
            view.at = Playhead::at(end);
        }
        view.transport.playing = false;
    }
}

/// Cut `scene` back to its first `kept` operations: what [`Scene::build`] makes
/// of them alone. Each operation's segments, their times and its steep descents
/// come from it and the ones before, so they stand as they are; only the counts
/// of what is left are made again.
pub(super) fn keep_first(scene: &mut Scene, kept: usize) {
    let Some(end) = scene.ops.get(kept).map(|o| o.first) else {
        return;
    };
    scene.segments.truncate(end);
    scene.ops.truncate(kept);
    // Deepest first, in machine order among equals, as they were.
    scene.steep.retain(|s| s.op < kept);
    scene.role_counts.fill(0);
    for s in &scene.segments {
        if let Some(c) = scene
            .role_counts
            .get_mut(theme::role_index(s.role) as usize)
        {
            *c += 1;
        }
    }
}
