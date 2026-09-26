//! Looking at a setup, in one of two modes. Setting up: the part, the stock's
//! outline and the selection's toolpaths in full. Playing back: the playhead
//! through its program in machine order, the material cut as far as it and the
//! cutter there. And in both, what its last run did and its steep descents.
//!
//! All of it is per setup, kept in its [`SetupState`], so each setup of each open
//! job keeps its own mode and place.
//!
//! Here is what is kept up each frame and what the viewport is given; the panel on
//! the left is [`show_panel`], and the checks window [`checks_window`].

mod checks_window;
mod show_panel;

use camcraft_session::checks::Finding;
use eframe::egui;
use glam::DVec3;

use crate::document::{Selection, SetupState, ViewMode};
use crate::stock::live::{Goal, LiveStock};
use crate::transport;
use crate::view::scene::{Playhead, Scene, Steep};
use crate::view::viewport::{Content, StockRef, StockShow, View};

pub use checks_window::checks;
pub use show_panel::{controls, result};

/// `12345` as `12,345`, as counts are printed.
pub fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Whether the last run was laid out as the setup is now, so its paths belong on
/// the part as drawn.
pub fn paths_fit(state: &SetupState) -> bool {
    match (&state.ran, &state.layout) {
        (Some(ran), Some(layout)) => layout
            .prepared
            .as_ref()
            .is_ok_and(|p| p.frame == ran.ran.result.frame && p.stocks == ran.ran.result.stocks),
        (Some(_), None) => true,
        _ => false,
    }
}

/// Whether the setup is being played back: asked for, and its last run laid out
/// as the setup is now. Otherwise it is being set up.
pub fn playing_back(state: &SetupState) -> bool {
    state.view.mode == ViewMode::Playback && paths_fit(state)
}

/// Why it cannot be played back, if it cannot.
pub fn cannot_play_back(state: &SetupState) -> Option<&'static str> {
    if state.ran.is_none() {
        Some("Generate the setup to play it back.")
    } else if !paths_fit(state) {
        Some("Out of date: generate it again to play it back.")
    } else {
        None
    }
}

/// Back to setting up, once there is nothing that fits to play back: a run
/// forgotten, or the setup laid out otherwise since.
pub fn settle_mode(state: &mut SetupState) {
    if state.view.mode == ViewMode::Playback && !playing_back(state) {
        set_mode(state, ViewMode::Setup);
    }
}

/// Set it up, or play it back; setting up pauses playing.
pub fn set_mode(state: &mut SetupState, mode: ViewMode) {
    state.view.mode = mode;
    if mode == ViewMode::Setup {
        state.view.transport.playing = false;
    }
}

/// Keep the playback material following the playhead, while it is shown.
pub fn follow_stock(ctx: &egui::Context, state: &mut SetupState) {
    if !playing_back(state) {
        return;
    }
    let Some(ran) = &state.ran else {
        return;
    };
    if state
        .live
        .as_ref()
        .is_none_or(|l| l.scene_key != ran.scene_key)
    {
        state.live = LiveStock::start(ctx, ran);
    }
    let (at, fraction) = ran.scene.target(state.view.at);
    if let Some(live) = &mut state.live {
        live.seek(Goal { at, fraction });
        live.poll();
    }
}

/// The segment a finding in operation `op` is at, if it is at a move: the
/// segment ending there, since move `k` of an operation ends segment `k - 1` of
/// its span.
pub fn finding_segment(scene: &Scene, op: Option<usize>, f: &Finding) -> Option<usize> {
    op.zip(f.at_move)
        .and_then(|(op, k)| Some(scene.ops.get(op)?.first + k.saturating_sub(1)))
        .filter(|&i| i < scene.segments.len())
}

/// Stop and stand after `shown` segments, playing back.
fn go_to(state: &mut SetupState, shown: usize) {
    transport::stand(state, Playhead::at(shown));
    set_mode(state, ViewMode::Playback);
}

/// Move the playhead to a steep descent, following the stock; where to look.
pub fn jump_to(state: &mut SetupState, steep: Steep) -> DVec3 {
    go_to(state, steep.index + 1);
    steep.at
}

/// What the viewport should draw for setup `name`, with `selection` the job's.
pub fn content<'a>(state: &'a SetupState, name: &str, selection: &Selection) -> Content<'a> {
    let view = &state.view;
    let back = playing_back(state);
    let scene = state.ran.as_ref().filter(|_| paths_fit(state));
    let cutter = scene
        .filter(|_| back && view.show_cutter)
        .and_then(|r| r.scene.cutter_at(view.at));
    let stock = scene.filter(|_| back).and_then(|r| {
        let live = state.live.as_ref().filter(|l| l.scene_key == r.scene_key)?;
        let (key, image, changed) = live.shown()?;
        Some(StockShow {
            key: (key, usize::MAX),
            image: StockRef(image),
            changed,
            mode: view.transport.color,
        })
    });
    let laid = state
        .layout
        .as_ref()
        .and_then(|l| Some((l.key, l.prepared.as_ref().ok()?, &l.outline)));
    let part = laid
        .map(|(key, p, _)| (key, &p.part))
        .or_else(|| state.ran.as_ref().map(|r| (r.scene_key, &r.ran.part)));
    let (show_part, show_paths, shown, ops) = if back {
        (
            view.part_in_playback,
            view.paths_in_playback,
            view.at.shown,
            view.view.ops,
        )
    } else {
        // Setting up: the selection's toolpaths, all of them. An operation of
        // this setup is its alone; anything else, the setup's.
        let ops = match selection {
            Selection::Operation(s, i) if s == name => u32::try_from(*i)
                .ok()
                .and_then(|i| 1u32.checked_shl(i))
                .unwrap_or(0),
            _ => u32::MAX,
        };
        let all = scene.map_or(0, |r| r.scene.segments.len());
        (view.view.part, view.show_paths, all, ops)
    };
    Content {
        part,
        framing: view.framing,
        scene: scene.map(|r| (r.scene_key, &r.scene)),
        outline: laid.map(|(key, _, o)| (key, o.as_slice())),
        stock_box: laid.map(|(_, p, _)| p.stock_bounds()).or_else(|| {
            let stocks = &state.ran.as_ref()?.ran.result.stocks;
            stocks.iter().copied().reduce(|a, b| a.union(&b))
        }),
        stock,
        view: View {
            part: show_part,
            roles: if show_paths { view.view.roles } else { 0 },
            ops,
            shown,
            cutter,
            ..view.view
        },
        selection: None,
        hover: None,
        ghost: None,
        hint: None,
        planes: &[],
    }
}
