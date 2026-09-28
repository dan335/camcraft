//! Playing a setup's program: the bar under the view, its keys, and the clock.
//! The bar also switches the view between setting the setup up and playing it
//! back; only playing back does the rest of it show, and the keys work.
//!
//! Everything moves the one [`Playhead`]. Playing advances it by machining time at
//! the chosen speed, a frame at a time; the buttons, the timeline and the keys set
//! it. The paths, the cutter and the material all follow from it, the material on
//! its own thread (`stock/live.rs`), so nothing here waits for a sweep.

use camcraft_geom::heightmap::narrow;
use camcraft_session::checks::Level;
use eframe::egui;

use crate::document::{SetupState, Transport, ViewMode};
use crate::help::Help;
use crate::help::topics::app;
use crate::playback::{self, finding_segment, playing_back, thousands};
use crate::ui::measure::Measure;
use crate::ui::theme;
use crate::ui::widgets::{self, Icon, Size, Variant};
use crate::view::scene::{Playhead, Scene, clock};

/// The speeds on offer, in machining seconds per second.
pub const SPEEDS: [f64; 15] = [
    0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0,
    10000.0,
];

/// The longest frame playing takes as one step, in seconds: after a stall, the
/// program picks up where it was rather than jumping.
const LONGEST_FRAME: f32 = 1.0 / 15.0;

/// A speed, as the bar shows it.
pub fn speed_label(speed: f64) -> String {
    if speed < 1.0 {
        format!("{speed}×")
    } else {
        // One of `SPEEDS`: whole, positive and small.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let whole = speed.round() as usize;
        format!("{}×", thousands(whole))
    }
}

/// The next speed up or down from `speed`, held to [`SPEEDS`].
pub fn faster(speed: f64, up: bool) -> f64 {
    let i = SPEEDS
        .iter()
        .position(|s| *s >= speed)
        .unwrap_or(SPEEDS.len() - 1);
    let j = if up {
        (i + 1).min(SPEEDS.len() - 1)
    } else {
        i.saturating_sub(1)
    };
    SPEEDS.get(j).copied().unwrap_or(speed)
}

/// A kind of place worth marking on the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// A failed check.
    Fail,
    /// A warning, or a steep descent.
    Warn,
}

/// Where the checks found something at a move, and the steep descents: each as
/// the playhead position just after it, sorted.
pub fn marks(state: &SetupState) -> Vec<(usize, Mark)> {
    let Some(ran) = &state.ran else {
        return Vec::new();
    };
    let scene = &ran.scene;
    let mut out: Vec<(usize, Mark)> = ran
        .ran
        .checks
        .groups
        .iter()
        .flat_map(|g| g.findings.iter().map(move |f| (g, f)))
        .filter_map(|(g, f)| {
            let mark = match f.level {
                Level::Fail => Mark::Fail,
                Level::Warn => Mark::Warn,
                Level::Note | Level::Pass => return None,
            };
            Some((finding_segment(scene, g.op, f)? + 1, mark))
        })
        .chain(scene.steep.iter().map(|s| (s.index + 1, Mark::Warn)))
        .collect();
    out.sort_unstable_by_key(|(at, mark)| (*at, *mark != Mark::Fail));
    out.dedup_by_key(|(at, _)| *at);
    out
}

/// Where playing from `from` to `to` should stop instead, at one of `stops`
/// (whole-segment positions, sorted): the first passed, going either way. A stop
/// the playhead starts on is not passed, so playing on from one leaves it.
pub fn stop_between(from: Playhead, to: Playhead, stops: &[usize]) -> Option<Playhead> {
    let key = |p: Playhead| (p.shown, p.fraction);
    let (a, b) = (key(from), key(to));
    if b > a {
        stops
            .iter()
            .find(|&&s| (s, 0.0) > a && (s, 0.0) <= b)
            .map(|&s| Playhead::at(s))
    } else {
        stops
            .iter()
            .rev()
            .find(|&&s| (s, 0.0) < a && (s, 0.0) >= b)
            .map(|&s| Playhead::at(s))
    }
}

/// Where each operation starts, and the end of the program.
fn op_bounds(scene: &Scene) -> Vec<usize> {
    let mut b: Vec<usize> = scene.ops.iter().map(|o| o.first).collect();
    b.push(scene.segments.len());
    b
}

/// Start playing, forward or back: from the start again if it is at the end.
fn play(state: &mut SetupState, reverse: bool) {
    let total = state.ran.as_ref().map_or(0, |r| r.scene.segments.len());
    let view = &mut state.view;
    if !reverse && view.at.shown >= total {
        view.at = Playhead::default();
    }
    if reverse && view.at == Playhead::default() {
        view.at = Playhead::at(total);
    }
    view.transport.playing = true;
    view.transport.reverse = reverse;
}

/// Stop, and stand at `at`.
pub fn stand(state: &mut SetupState, at: Playhead) {
    state.view.transport.playing = false;
    state.view.at = at;
}

/// What a button or key asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Command {
    Toggle,
    Play {
        reverse: bool,
    },
    /// Play that way, faster if already playing that way.
    Shuttle {
        reverse: bool,
    },
    Pause,
    Step(isize),
    PrevOp,
    NextOp,
    Start,
    End,
    Speed {
        up: bool,
    },
}

fn apply(state: &mut SetupState, command: Command) {
    let Some(scene) = state.ran.as_ref().map(|r| &r.scene) else {
        return;
    };
    let total = scene.segments.len();
    let at = state.view.at;
    let bounds = op_bounds(scene);
    let t = state.view.transport;
    match command {
        Command::Toggle if t.playing => state.view.transport.playing = false,
        Command::Toggle => play(state, t.reverse),
        Command::Play { reverse } => play(state, reverse),
        Command::Shuttle { reverse } => {
            if t.playing && t.reverse == reverse {
                state.view.transport.speed = faster(t.speed, true);
            } else {
                play(state, reverse);
            }
        }
        Command::Pause => state.view.transport.playing = false,
        Command::Step(n) => {
            let shown = if n < 0 {
                // Part-way along a move, a step back goes to its start.
                let back = n.unsigned_abs() - usize::from(at.fraction > 0.0);
                at.shown.saturating_sub(back)
            } else {
                (at.shown + n.unsigned_abs()).min(total)
            };
            stand(state, Playhead::at(shown));
        }
        Command::PrevOp => {
            let here = (at.shown, at.fraction);
            let to = bounds
                .iter()
                .rev()
                .find(|&&b| (b, 0.0) < here)
                .copied()
                .unwrap_or(0);
            stand(state, Playhead::at(to));
        }
        Command::NextOp => {
            let to = bounds
                .iter()
                .find(|&&b| b > at.shown)
                .copied()
                .unwrap_or(total);
            stand(state, Playhead::at(to));
        }
        Command::Start => stand(state, Playhead::default()),
        Command::End => stand(state, Playhead::at(total)),
        Command::Speed { up } => state.view.transport.speed = faster(t.speed, up),
    }
}

/// Keys, unless a widget has the keyboard: Space plays and pauses; J, K and L
/// shuttle back, pause and forward, J or L again going faster; Left and Right
/// step a move, a hundred with Shift; Up and Down go to the previous and next
/// operation; Home and End; `[` and `]` change the speed. Only while playing
/// back, but for Space, which starts it if it can.
pub fn keys(ctx: &egui::Context, state: &mut SetupState) {
    if state.ran.is_none() || ctx.memory(|m| m.focused().is_some()) {
        return;
    }
    if !playing_back(state) {
        let space = ctx.input(|i| i.key_pressed(egui::Key::Space) && !i.modifiers.command);
        if space && playback::cannot_play_back(state).is_none() {
            playback::set_mode(state, ViewMode::Playback);
            apply(state, Command::Play { reverse: false });
        }
        return;
    }
    let commands = ctx.input(|i| {
        let shift = i.modifiers.shift;
        let step = if shift { 100 } else { 1 };
        [
            (egui::Key::Space, Command::Toggle),
            (egui::Key::J, Command::Shuttle { reverse: true }),
            (egui::Key::K, Command::Pause),
            (egui::Key::L, Command::Shuttle { reverse: false }),
            (egui::Key::ArrowLeft, Command::Step(-step)),
            (egui::Key::ArrowRight, Command::Step(step)),
            (egui::Key::ArrowUp, Command::PrevOp),
            (egui::Key::ArrowDown, Command::NextOp),
            (egui::Key::Home, Command::Start),
            (egui::Key::End, Command::End),
            (egui::Key::OpenBracket, Command::Speed { up: false }),
            (egui::Key::CloseBracket, Command::Speed { up: true }),
        ]
        .into_iter()
        .filter(|(k, _)| i.key_pressed(*k) && !i.modifiers.command)
        .map(|(_, c)| c)
        .collect::<Vec<_>>()
    });
    for c in commands {
        apply(state, c);
    }
}

/// Move the playhead on by the time since the last frame, while playing.
pub fn tick(ctx: &egui::Context, state: &mut SetupState) {
    let Transport {
        playing,
        reverse,
        speed,
        stop_at_problems,
        stop_at_op_end,
        ..
    } = state.view.transport;
    if !playing {
        return;
    }
    let Some(scene) = state
        .ran
        .as_ref()
        .filter(|_| playing_back(state))
        .map(|r| &r.scene)
    else {
        state.view.transport.playing = false;
        return;
    };
    let dt = f64::from(ctx.input(|i| i.stable_dt).min(LONGEST_FRAME));
    let from = state.view.at;
    let step = if reverse { -dt * speed } else { dt * speed };
    let mut to = scene.advance(from, step);
    let mut stops: Vec<usize> = Vec::new();
    if stop_at_problems {
        stops.extend(marks(state).into_iter().map(|(at, _)| at));
    }
    if stop_at_op_end {
        stops.extend(op_bounds(scene));
    }
    stops.sort_unstable();
    stops.dedup();
    let stopped = stop_between(from, to, &stops);
    if let Some(stop) = stopped {
        to = stop;
    }
    let total = scene.segments.len();
    let at_end = if reverse {
        to == Playhead::default()
    } else {
        to.shown >= total
    };
    state.view.at = to;
    if stopped.is_some() || at_end {
        state.view.transport.playing = false;
    }
    ctx.request_repaint();
}

/// The bar: whether the setup is being set up or played back, and playing back,
/// the timeline, then the buttons, where the program stands, and how it plays.
pub fn bar(ui: &mut egui::Ui, state: &mut SetupState, m: Measure) {
    let cannot = playback::cannot_play_back(state);
    ui.horizontal(|ui| {
        let mut page = usize::from(playing_back(state));
        let changed = widgets::tabs(
            ui,
            &mut page,
            &[widgets::Tab::new("Setup"), widgets::Tab::new("Playback")],
        );
        if changed {
            match (page, cannot) {
                (0, _) => playback::set_mode(state, ViewMode::Setup),
                (_, None) => playback::set_mode(state, ViewMode::Playback),
                (_, Some(_)) => {}
            }
        }
        if let Some(why) = cannot {
            ui.add_space(theme::SPACE_3);
            ui.label(widgets::hint_text(why));
        }
    });
    if !playing_back(state) {
        return;
    }
    ui.add_space(theme::SPACE_2);
    let marks = marks(state);
    timeline(ui, state, &marks);
    ui.add_space(theme::SPACE_2);
    let mut commands = Vec::new();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SPACE_1;
        let t = state.view.transport;
        let mut on = |r: egui::Response, c: Command| {
            if r.clicked() {
                commands.push(c);
            }
        };
        let icon = widgets::icon_button;
        on(icon(ui, Icon::ChevronFirst, "Start (Home)"), Command::Start);
        on(
            icon(ui, Icon::SkipBack, "Previous operation (Up)"),
            Command::PrevOp,
        );
        on(
            icon(ui, Icon::StepBack, "Back a move (Left; Shift for 100)"),
            Command::Step(-1),
        );
        if t.playing && t.reverse {
            on(icon(ui, Icon::Pause, "Pause (K)"), Command::Pause);
        } else {
            on(
                icon(ui, Icon::Rewind, "Play backwards (J)"),
                Command::Play { reverse: true },
            );
        }
        let forward = t.playing && !t.reverse;
        let (glyph, tip, c, variant) = if forward {
            (
                Icon::Pause,
                "Pause (Space)",
                Command::Pause,
                Variant::Secondary,
            )
        } else {
            (
                Icon::Play,
                "Play (Space)",
                Command::Play { reverse: false },
                Variant::Primary,
            )
        };
        on(
            widgets::button(ui, variant, Size::Sm, Some(glyph), "").on_hover_text(tip),
            c,
        );
        on(
            icon(ui, Icon::StepForward, "On a move (Right; Shift for 100)"),
            Command::Step(1),
        );
        on(
            icon(ui, Icon::SkipForward, "Next operation (Down)"),
            Command::NextOp,
        );
        on(icon(ui, Icon::ChevronLast, "End (End)"), Command::End);
        ui.add_space(theme::SPACE_3);
        clock_readout(ui, state);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = theme::SPACE_3;
            let paths = &mut state.view.paths_in_playback;
            let variant = if *paths {
                Variant::Secondary
            } else {
                Variant::Ghost
            };
            if widgets::button(ui, variant, Size::Sm, None, "toolpaths")
                .on_hover_text("the toolpaths, as far as the playhead")
                .clicked()
            {
                *paths = !*paths;
            }
            let t = &mut state.view.transport;
            let stops = usize::from(t.stop_at_problems) + usize::from(t.stop_at_op_end);
            ui.menu_button(format!("stops ({stops})"), |ui| {
                ui.checkbox(&mut t.stop_at_problems, "at problems")
                    .help(&app::STOP_AT_PROBLEMS);
                ui.checkbox(&mut t.stop_at_op_end, "at the end of each operation")
                    .help(&app::STOP_AT_OPERATION_END);
            })
            .response
            .on_hover_text("where playing pauses by itself");
            egui::ComboBox::from_id_salt("speed")
                .selected_text(widgets::mono(speed_label(t.speed), theme::TEXT_SM))
                .width(72.0)
                .show_ui(ui, |ui| {
                    for s in SPEEDS {
                        ui.selectable_value(&mut t.speed, s, speed_label(s));
                    }
                })
                .response
                .help(&app::SPEED);
        });
    });
    move_readout(ui, state, m);
    for c in commands {
        apply(state, c);
    }
}

/// The clock, and whether the material has caught up with it.
fn clock_readout(ui: &mut egui::Ui, state: &SetupState) {
    let Some(scene) = state.ran.as_ref().map(|r| &r.scene) else {
        return;
    };
    ui.label(widgets::mono(
        format!(
            "{} / {}",
            clock(scene.time_of(state.view.at)),
            clock(scene.total_time())
        ),
        theme::TEXT_SM,
    ))
    .on_hover_text("machining time, estimated");
    let behind = playing_back(state) && state.live.as_ref().is_some_and(|l| !l.caught_up());
    if behind {
        ui.label(
            egui::RichText::new("cutting…")
                .size(theme::TEXT_XS)
                .color(theme::egui_color(theme::TEXT_3)),
        )
        .on_hover_text("the material is catching up with the playhead");
    }
}

/// The operation and the move under way, or the last one made at the end.
fn move_readout(ui: &mut egui::Ui, state: &SetupState, m: Measure) {
    let Some(scene) = state.ran.as_ref().map(|r| &r.scene) else {
        return;
    };
    let at = state.view.at;
    let index = if at.shown < scene.segments.len() {
        at.shown
    } else {
        at.shown.saturating_sub(1)
    };
    let Some(s) = scene.segments.get(index) else {
        return;
    };
    let op = scene.ops.get(s.op as usize).map_or("", |o| o.name.as_str());
    let feed = s
        .feed
        .map_or_else(String::new, |f| format!("  F{}", m.feed_number(f)));
    let text = format!(
        "{}. {op}  ·  move {} of {}  ·  {} to {}, {}, {} {}{feed}",
        s.op + 1,
        thousands(index + 1),
        thousands(scene.segments.len()),
        theme::role_name(s.role),
        m.number(s.b.x, 2),
        m.number(s.b.y, 2),
        m.number(s.b.z, 2),
        m.unit(),
    );
    ui.add(
        egui::Label::new(
            widgets::mono(text, theme::TEXT_XS).color(theme::egui_color(theme::TEXT_3)),
        )
        .truncate(),
    );
}

/// The program as a strip, one span per operation as long as it takes to run,
/// with what the checks found marked on it and the playhead. Click or drag to go
/// anywhere.
fn timeline(ui: &mut egui::Ui, state: &mut SetupState, marks: &[(usize, Mark)]) {
    let Some(scene) = state.ran.as_ref().map(|r| &r.scene) else {
        return;
    };
    let height = 22.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click_and_drag(),
    );
    let total = scene.total_time();
    if total <= 0.0 {
        return;
    }
    let x_of = |t: f64| rect.left() + rect.width() * narrow((t / total).clamp(0.0, 1.0));
    let time_at = |x: f32| f64::from((x - rect.left()) / rect.width().max(1.0)) * total;
    let painter = ui.painter_at(rect);
    let radius = egui::CornerRadius::same(theme::RADIUS_SM);
    painter.rect_filled(rect, radius, theme::egui_color(theme::SURFACE_2));

    for (i, op) in scene.ops.iter().enumerate() {
        let a = x_of(scene.time_of(Playhead::at(op.first)));
        let b = x_of(scene.time_of(Playhead::at(op.first + op.count)));
        let span = egui::Rect::from_x_y_ranges(a..=b, rect.y_range()).shrink2(egui::vec2(0.5, 0.0));
        let fill = if i % 2 == 0 {
            theme::GRAY_3
        } else {
            theme::GRAY_4
        };
        painter.rect_filled(span, radius, theme::egui_color(fill));
        if span.width() > 40.0 {
            let text = egui::RichText::new(format!("{}. {}", i + 1, op.name)).size(theme::TEXT_XS);
            let galley = egui::WidgetText::from(text).into_galley(
                ui,
                Some(egui::TextWrapMode::Truncate),
                span.width() - 8.0,
                egui::TextStyle::Body,
            );
            let at = egui::pos2(span.left() + 4.0, span.center().y - galley.size().y / 2.0);
            painter.galley(at, galley, theme::egui_color(theme::TEXT_3));
        }
    }

    // Done so far, tinted.
    let now = x_of(scene.time_of(state.view.at));
    let done = egui::Rect::from_x_y_ranges(rect.left()..=now, rect.y_range());
    painter.rect_filled(done, radius, theme::ACCENT_SUBTLE.color());

    for (at, mark) in marks {
        let x = x_of(scene.time_of(Playhead::at(*at)));
        let color = match mark {
            Mark::Fail => theme::ERR,
            Mark::Warn => theme::WARN,
        };
        painter.line_segment(
            [
                egui::pos2(x, rect.top() + 3.0),
                egui::pos2(x, rect.bottom() - 3.0),
            ],
            egui::Stroke::new(1.5, theme::egui_color(color)),
        );
    }

    let accent = theme::egui_color(theme::ACCENT);
    painter.line_segment(
        [egui::pos2(now, rect.top()), egui::pos2(now, rect.bottom())],
        egui::Stroke::new(2.0, accent),
    );
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(now - 5.0, rect.top()),
            egui::pos2(now + 5.0, rect.top()),
            egui::pos2(now, rect.top() + 6.0),
        ],
        accent,
        egui::Stroke::NONE,
    ));

    if let Some(pos) = response.hover_pos() {
        let hovered = scene.locate(time_at(pos.x));
        let op = scene
            .segments
            .get(hovered.shown)
            .and_then(|s| scene.ops.get(s.op as usize))
            .map_or("", |o| o.name.as_str());
        response
            .clone()
            .on_hover_text_at_pointer(format!("{}  {op}", clock(time_at(pos.x))));
    }
    if (response.is_pointer_button_down_on() || response.clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        let to = scene.locate(time_at(pos.x));
        stand(state, to);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn speeds_step_up_and_down_and_stay_on_the_list() {
        assert_eq!(faster(10.0, true), 20.0);
        assert_eq!(faster(10.0, false), 5.0);
        assert_eq!(faster(0.25, false), 0.25);
        assert_eq!(faster(10000.0, true), 10000.0);
        // A speed off the list goes to its neighbours on it.
        assert_eq!(faster(7.0, true), 20.0);
        assert_eq!(speed_label(0.25), "0.25×");
        assert_eq!(speed_label(1000.0), "1,000×");
    }

    #[test]
    fn playing_stops_at_the_first_stop_passed_either_way() {
        let stops = [10, 20, 30];
        let p = |shown, fraction| Playhead { shown, fraction };
        assert_eq!(
            stop_between(p(5, 0.0), p(25, 0.3), &stops),
            Some(Playhead::at(10))
        );
        assert_eq!(
            stop_between(p(10, 0.0), p(15, 0.0), &stops),
            None,
            "starts on it"
        );
        assert_eq!(
            stop_between(p(19, 0.9), p(20, 0.0), &stops),
            Some(Playhead::at(20))
        );
        assert_eq!(stop_between(p(9, 0.5), p(9, 0.9), &stops), None);
        // Backwards, the nearest behind.
        assert_eq!(
            stop_between(p(25, 0.0), p(5, 0.0), &stops),
            Some(Playhead::at(20))
        );
        assert_eq!(
            stop_between(p(20, 0.0), p(15, 0.0), &stops),
            None,
            "starts on it"
        );
        assert_eq!(
            stop_between(p(20, 0.5), p(15, 0.0), &stops),
            Some(Playhead::at(20))
        );
    }
}
