//! Looking at a setup, in one of two modes. Setting up: the part, the stock's
//! outline and the selection's toolpaths in full, or in place of the part, the
//! material as the selection leaves it. Playing back: the playhead through its
//! program in machine order, the material cut as far as it and the cutter
//! there. And in both, what its last run did and its steep descents.
//!
//! All of it is per setup, kept in its [`SetupState`], so each setup of each open
//! job keeps its own mode and place.
//!
//! Here is what is kept up each frame and what the viewport is given; the panel on
//! the left is [`show_panel`], and the checks window [`checks_window`].

mod checks_window;
mod show_panel;

use camcraft_core::replay::Position;
use camcraft_geom::mesh::Aabb;
use camcraft_session::checks::Finding;
use eframe::egui;
use glam::DVec3;

use crate::document::{Look, Selection, SetupState, Shown, ViewMode};
use crate::stock::live::{Goal, LiveStock};
use crate::transport;
use crate::view::scene::{Playhead, Scene, Steep};
use crate::view::viewport::{Content, StockRef, StockShow, View};

pub use checks_window::checks;
pub use show_panel::{controls, result};

/// `12345` as `12,345`, as counts are printed ([`camcraft_session::checks::thousands`]).
pub fn thousands(n: usize) -> String {
    camcraft_session::checks::thousands(u64::try_from(n).unwrap_or(u64::MAX))
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
        Some("It plays back once it is generated.")
    } else if !paths_fit(state) {
        Some("Out of date: it plays back once it is generated again.")
    } else {
        None
    }
}

/// Whether the material is shown: playing back, or setting up with it asked
/// for and a run laid out as the setup is now to show it from. Otherwise the
/// part is.
pub fn showing_material(state: &SetupState) -> bool {
    playing_back(state) || (state.view.material && paths_fit(state))
}

/// How the setup is being looked at.
pub fn look(state: &SetupState) -> Look {
    if playing_back(state) {
        Look::Playback
    } else if showing_material(state) {
        Look::CutStock
    } else {
        Look::Part
    }
}

/// The operation of setup `name` that `selection` is, if it is one of its.
pub fn selected_operation(selection: &Selection, name: &str) -> Option<usize> {
    match selection {
        Selection::Operation(s, i) if s == name => Some(*i),
        _ => None,
    }
}

/// Where the material should stand. Playing back, at the playhead; setting
/// up, at the end of operation `only`, or of the setup. An end is a keyframe of
/// the run, so it is put back rather than cut.
fn goal(state: &SetupState, scene: &Scene, only: Option<usize>) -> Goal {
    let (at, fraction) = if playing_back(state) {
        scene.target(state.view.at)
    } else {
        // Past the run's last operation, as one edited and not yet generated
        // again is, is the end of the last it has.
        let end = Position {
            op: only.unwrap_or(usize::MAX),
            moves: usize::MAX,
        };
        (end, 0.0)
    };
    Goal { at, fraction }
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

/// Keep the material where it should stand ([`goal`]), while it is shown,
/// with `only` the operation selected. Not shown, it is kept for showing again
/// while it is of the last run, and let go once it is not.
pub fn follow_stock(ctx: &egui::Context, state: &mut SetupState, only: Option<usize>) {
    let current = |s: &SetupState| {
        s.live
            .as_ref()
            .zip(s.ran.as_ref())
            .is_some_and(|(l, r)| l.scene_key == r.scene_key)
    };
    if !showing_material(state) {
        if !current(state) {
            state.live = None;
            state.outgoing = None;
        }
        return;
    }
    if !current(state) {
        // The last run's material is drawn until the new one's is filled.
        if let Some(mut last) = state.live.take().filter(LiveStock::filled) {
            last.halt();
            state.outgoing = Some(last);
        }
        state.live = state
            .ran
            .as_ref()
            .and_then(|ran| LiveStock::start(ctx, ran));
    }
    let Some(ran) = &state.ran else {
        return;
    };
    let goal = goal(state, &ran.scene, only);
    if let Some(live) = &mut state.live {
        live.seek(goal);
        live.poll();
        if live.filled() {
            state.outgoing = None;
        }
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
    let material = showing_material(state);
    let scene = state.ran.as_ref().filter(|_| paths_fit(state));
    let cutter = scene
        .filter(|_| back && view.show_cutter)
        .and_then(|r| r.scene.cutter_at(view.at));
    let stock = scene.filter(|_| material).and_then(|_| {
        let (key, image, changed) = state.drawn_stock()?.shown()?;
        Some(StockShow {
            key,
            image: StockRef(image),
            changed,
            mode: view.transport.color,
        })
    });
    let laid = state
        .layout
        .as_ref()
        .and_then(|l| Some((l.key, l.prepared.as_ref().ok()?, &l.outline)));
    // Laid out, the part's bounds are known; otherwise, rarely, measured.
    let (part, part_bounds) = match (laid, &state.ran) {
        (Some((key, p, _)), _) => (Some((key, &p.part)), Some(p.model_bounds)),
        (None, Some(r)) => (
            Some((r.scene_key, &r.ran.prepared.part)),
            r.ran.prepared.part.bounds(),
        ),
        (None, None) => (None, None),
    };
    let Shown {
        part: show_part,
        toolpaths: show_paths,
    } = view.looks.of(look(state));
    let (shown, ops, only) = if back {
        (view.at.shown, view.view.ops, None)
    } else {
        // Setting up: the selection's toolpaths, all of them. An operation of
        // this setup is its alone; anything else, the setup's.
        let only = selected_operation(selection, name);
        let ops = only.map_or(u32::MAX, |i| {
            u32::try_from(i)
                .ok()
                .and_then(|i| 1u32.checked_shl(i))
                .unwrap_or(0)
        });
        let all = scene.map_or(0, |r| r.scene.segments.len());
        (all, ops, only)
    };
    // Setting up, the material stands for the stock: its outline and work zero
    // go with the part. Playing back, they are as ticked.
    let outlined = back || !material;
    Content {
        part,
        part_bounds,
        framing: view.framing,
        scene: scene.map(|r| (r.scene_key, &r.scene)),
        outline: laid.map(|(key, _, o)| (key, o.as_slice())),
        stock_box: laid
            .map(|(_, p, _)| p.stock_bounds())
            .or_else(|| Aabb::union_all(&state.ran.as_ref()?.ran.result.stocks)),
        stock,
        view: View {
            part: show_part,
            stock: view.view.stock && outlined,
            origin: view.view.origin && outlined,
            roles: if show_paths { view.view.roles } else { 0 },
            ops,
            only,
            shown,
            cutter,
        },
        selection: None,
        hover: None,
        ghost: None,
        // Setting up, what is being worked out shows as it is found.
        preview: state
            .running
            .as_ref()
            .filter(|r| !back && show_paths && !r.preview.lines.is_empty())
            .map(|r| (r.preview_key, &r.preview)),
        hint: None,
        planes: &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Setting up with an operation selected shows its toolpaths alone, however
    /// far down the setup it is: past the 32 operations the mask holds too.
    #[test]
    fn a_selected_operation_shows_alone_wherever_it_is() {
        let state = SetupState::default();
        for i in [0, 31, 32, 40] {
            let selected = Selection::Operation("top".into(), i);
            assert_eq!(content(&state, "top", &selected).view.only, Some(i));
        }
        let other = Selection::Operation("bottom".into(), 40);
        assert_eq!(content(&state, "top", &other).view.only, None);
        let setup = Selection::Setup("top".into());
        assert_eq!(content(&state, "top", &setup).view.only, None);
    }

    /// Setting up, the material stands at the end of the operation selected,
    /// or with none, of the setup: past the last there is, which the replay
    /// takes as the end of the last.
    #[test]
    fn the_material_stands_where_the_selection_ends() {
        let state = SetupState::default();
        let scene = Scene::default();
        let end = |op| Goal {
            at: Position {
                op,
                moves: usize::MAX,
            },
            fraction: 0.0,
        };
        assert_eq!(goal(&state, &scene, Some(1)), end(1));
        assert_eq!(goal(&state, &scene, None), end(usize::MAX));
    }
}
