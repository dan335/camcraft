//! What a setup keeps that is not in the job: its last run, how it is laid out,
//! and how it is being looked at and played.

use std::sync::Arc;
use std::time::Instant;

use camcraft_core::control::Control;
use camcraft_core::job::Job;
use camcraft_core::run::Stage;
use camcraft_core::setup::Prepared;
use camcraft_geom::contour::Region;
use camcraft_session::Ran;

use super::next_key;
use super::passes::PassCache;
use super::run::{SetupInputs, ShadowKey};
use crate::stock::live::LiveStock;
use crate::stock::stock_view::ColorMode;
use crate::transport::{Mark, marks};
use crate::ui::theme;
use crate::view::camera::Camera;
use crate::view::scene::{Playhead, Scene};
use crate::view::viewport::{Highlight, Outline, View};

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
    /// The part and stock in the setup's frame, or why not. Shared with the
    /// worker that casts its shadow ([`Document::shadows_wanted`]).
    ///
    /// [`Document::shadows_wanted`]: super::Document::shadows_wanted
    pub prepared: Result<Arc<Prepared>, String>,
    /// Whether it is laid out as the job has it: not where its work zero was
    /// picked on something no longer there, and is drawn from the model's
    /// own zero instead. Only then may a run take it.
    pub(super) exact: bool,
    /// Why the part's shadow could not be cast, once the worker has tried.
    pub(super) shadow_failed: Option<String>,
    /// What its part's shadow is cast from, to keep it by and find it again.
    pub(super) shadow_key: ShadowKey,
    /// Stock edges and the work-zero axes.
    pub outline: Vec<Outline>,
    /// Tells this layout's uploads from any other.
    pub key: u64,
}

impl Layout {
    /// The part's silhouette, its shadow from above: `Ok(None)` while it is
    /// being cast, off the window's thread, since a large part takes a while.
    /// Never casts it here.
    pub fn silhouette(&self) -> Result<Option<&Region>, String> {
        let prepared = self.prepared.as_ref().map_err(Clone::clone)?;
        match (prepared.cast_shadow(), &self.shadow_failed) {
            (Some(shadow), _) => Ok(Some(shadow.area())),
            (None, Some(why)) => Err(format!("the part's silhouette: {why}")),
            (None, None) => Ok(None),
        }
    }
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

/// A setup's run in flight.
#[derive(Debug)]
pub struct Running {
    /// Which run it is, to tell its word from a run's since superseded.
    pub seq: u64,
    /// The job as it was sent.
    pub job: Job,
    /// What it was sent from, to tell what has changed since.
    pub inputs: SetupInputs,
    /// To stop it, and to see how far each operation has got.
    pub control: Control,
    /// What it has started on of its own, not an operation's: laying out,
    /// verifying or checking; and since when.
    pub stage: (Stage, Instant),
    /// Each operation's work, by its place, once started, and since when.
    pub ops: Vec<Option<(OpWork, Instant)>>,
    /// Each operation it keeps from before, by its place.
    pub kept: Vec<bool>,
    /// How many operations it has made so far, from the first.
    pub made: usize,
    /// Each operation's passes shown while being worked out
    /// ([`camcraft_core::control::Watch::show`]), as lines to draw, by its
    /// place; and how many passes of each have been taken.
    pub(super) shown: Vec<(Vec<Outline>, usize)>,
    /// Those of the operations not yet made, together, as drawn.
    pub preview: Highlight,
    /// Tells this preview's upload from any other.
    pub preview_key: u64,
    /// When the preview was last made again.
    pub(super) previewed: Instant,
}

impl Running {
    /// Draw again what the operations not yet made have shown.
    pub(super) fn preview_again(&mut self) {
        let made = self.made;
        self.preview.lines = self
            .shown
            .iter()
            .enumerate()
            .filter(|(op, _)| *op >= made)
            .flat_map(|(_, (lines, _))| lines.iter().copied())
            .collect();
        self.preview_key = next_key();
    }
}

/// What an operation's work is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpWork {
    /// Working out its passes: at once with the others where it can be.
    Planning,
    /// Linking them over the material, and cutting them into it, in turn.
    Cutting,
}

/// Why a setup is left as it is, not generated by itself, until something it
/// is made from changes.
#[derive(Debug, Clone, PartialEq)]
pub enum Halt {
    /// It was stopped.
    Stopped,
    /// Its run failed, at operation `at`, for `why`.
    Failed {
        /// The operation it failed at, by its place: the first it had not
        /// made. Past the last, it failed after them all.
        at: usize,
        /// Why, to read.
        why: String,
    },
}

/// Everything about one setup that is not in the job.
#[derive(Debug, Default)]
pub struct SetupState {
    /// Its last run: as far as it has got, while one is in flight.
    pub ran: Option<RanSetup>,
    /// The run in flight, if any.
    pub running: Option<Running>,
    /// Why it is left as it is, and what it was made from then: while it is
    /// still made from that, it is not generated by itself.
    pub halted: Option<(SetupInputs, Halt)>,
    /// How it is laid out now.
    pub layout: Option<Layout>,
    /// The layout, by its key, whose part's shadow a worker is casting: one at
    /// a time, so a layout made again and again while one is cast (a stock's
    /// size typed in) waits for it rather than starting more.
    pub(super) casting: Option<(u64, ShadowKey)>,
    /// How it is being looked at.
    pub view: SetupView,
    /// Its material during playback, once it has been followed.
    pub live: Option<LiveStock>,
    /// Its operations' passes, kept from its runs, to link again.
    pub(super) passes: PassCache,
}

impl SetupState {
    /// Show `ran` as its run: what a run has made so far, or all of it. A
    /// setup shown for the first time plays back from its first move, every
    /// operation visible; one shown again, as each change is generated, keeps
    /// its playhead where it was, as far as the new run goes.
    pub fn show_run(&mut self, ran: RanSetup) {
        self.passes.keep(&ran.inputs, &ran.ran.result);
        let first = self.ran.is_none();
        let end = ran.scene.segments.len();
        self.ran = Some(ran);
        self.live = None;
        let view = &mut self.view;
        if first {
            view.at = Playhead::default();
            view.transport.playing = false;
            view.view.ops = u32::MAX;
        } else if view.at.shown > end {
            view.at = Playhead::at(end);
        }
    }

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
