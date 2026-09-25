//! Looking at a setup's result: what it did, the playhead through its program in
//! machine order, the material at any stage, and the steep descents.
//!
//! All of it is per setup, kept in its [`SetupState`], so each setup of each open
//! job keeps its own place.
//!
//! Here is what is kept up each frame and what the viewport is given; the panel on
//! the left is [`show_panel`], and the checks window [`checks_window`].

mod checks_window;
mod show_panel;

use camcraft_session::checks::Finding;
use eframe::egui;
use glam::DVec3;

use crate::document::{SetupState, StockMode, next_key};
use crate::stock::live::{Goal, LiveStock};
use crate::stock::stock_view::StockImage;
use crate::transport;
use crate::view::scene::{Playhead, Scene, Steep};
use crate::view::viewport::{Content, StockChange, StockRef, StockShow, View};

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

/// The fixed stage the stock view shows, if it shows one.
pub fn stage_shown(state: &SetupState) -> Option<usize> {
    match state.view.stock_mode {
        StockMode::Stage(i) => Some(i),
        StockMode::Off | StockMode::Follow => None,
    }
}

/// Classify the shown stage's material, the first time it is shown.
pub fn ensure_stock_image(state: &mut SetupState) {
    let Some(stage) = stage_shown(state) else {
        return;
    };
    let Some(ran) = &state.ran else {
        return;
    };
    let result = &ran.ran.result;
    if let (Some(slot @ None), Some(Ok(stock))) = (
        state.view.stock_images.get_mut(stage),
        result.keyframes.get(stage).map(|s| s.unpack()),
    ) {
        let image = StockImage::build(&stock, &result.part, result.verify_budget);
        *slot = Some((next_key(), image));
    }
}

/// Keep the playback material following the playhead, while it is shown.
pub fn follow_stock(ctx: &egui::Context, state: &mut SetupState) {
    if state.view.stock_mode != StockMode::Follow || !paths_fit(state) {
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

/// Stop and stand after `shown` segments, cutting the stock as it plays.
fn go_to(state: &mut SetupState, shown: usize) {
    transport::stand(state, Playhead::at(shown));
    state.view.stock_mode = StockMode::Follow;
}

/// Move the playhead to a steep descent, following the stock; where to look.
pub fn jump_to(state: &mut SetupState, steep: Steep) -> DVec3 {
    go_to(state, steep.index + 1);
    steep.at
}

/// What the viewport should draw for this setup.
pub fn content(state: &SetupState) -> Content<'_> {
    let view = &state.view;
    let fits = paths_fit(state);
    let scene = state.ran.as_ref().filter(|_| fits);
    let cutter = scene
        .filter(|_| view.show_cutter)
        .and_then(|r| r.scene.cutter_at(view.at));
    let stock = scene.and_then(|r| match view.stock_mode {
        StockMode::Off => None,
        StockMode::Stage(i) => {
            let (key, image) = view.stock_images.get(i)?.as_ref()?;
            Some(StockShow {
                key: (*key, i),
                image: StockRef::Plain(image),
                changed: StockChange::Nothing,
                mode: view.transport.color,
            })
        }
        StockMode::Follow => {
            let live = state.live.as_ref().filter(|l| l.scene_key == r.scene_key)?;
            let (key, image, changed) = live.shown()?;
            Some(StockShow {
                key: (key, usize::MAX),
                image: StockRef::Shared(image),
                changed,
                mode: view.transport.color,
            })
        }
    });
    let laid = state
        .layout
        .as_ref()
        .and_then(|l| Some((l.key, l.prepared.as_ref().ok()?, &l.outline)));
    let part = laid
        .map(|(key, p, _)| (key, &p.part))
        .or_else(|| state.ran.as_ref().map(|r| (r.scene_key, &r.ran.part)));
    let show_part = view.view.part && !(stock.is_some() && view.hide_part_with_stock);
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
            roles: if view.show_paths { view.view.roles } else { 0 },
            shown: view.at.shown,
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
