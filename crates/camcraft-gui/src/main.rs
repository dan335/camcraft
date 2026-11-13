//! camcraft desktop app.
//!
//! Open a job, run its setups, and see what will be cut. It runs on the same
//! engine path as the `camcraft` command line, through `camcraft-session`, so the
//! two cannot disagree about what a file means, what runs, or what may be posted.
//!
//! Position in the dependency DAG: a **root**. Depends on `camcraft-session` and,
//! through it and directly, on the library crates whose types it shows
//! (`camcraft-core`, `camcraft-geom`, `camcraft-sim`); nothing depends on it.

// The window and its panels.
mod app;
mod browser;
mod file_dialog;
mod playback;
mod properties;
mod settings;
mod tools;

// A job as the app holds it, and the work done off the UI thread.
mod document;
mod worker;

// The 3D view, picking in it, and the material it draws.
mod picking;
mod stock;
mod view;

// The design system, and lengths in millimetres or inches.
mod ui;
// What every input does, shown on hovering it.
mod help;

#[cfg(test)]
mod test_jobs;
#[cfg(all(test, feature = "ui-snapshots"))]
mod ui_snapshots;

use eframe::egui;

/// The jobs named on the command line: `camcraft-gui [JOB]...`, each in a tab.
fn jobs_from_args() -> Vec<std::path::PathBuf> {
    std::env::args_os()
        .skip(1)
        .map(std::path::PathBuf::from)
        .collect()
}

/// The app's icon: a pocket cleared from the middle out, drawn in
/// `assets/icon.svg` and rendered to 512 px.
fn icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default()
}

fn main() -> eframe::Result {
    leave_a_core_for_the_window();
    let jobs = jobs_from_args();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("camcraft")
            .with_icon(icon())
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([820.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native(
        "camcraft",
        options,
        Box::new(|cc| {
            if let Some(render) = &cc.wgpu_render_state {
                say_why_the_device_is_lost(render);
            }
            Ok(Box::new(app::App::with_jobs(cc, jobs)))
        }),
    )
}

/// When the graphics device is lost, say why.
///
/// A device is lost when its driver gives up: reset, out of memory, or an
/// error it does not name. wgpu tells no one unless asked, and the next frame
/// then panics in egui's renderer, about a staging buffer for index data,
/// which says nothing of the cause. So the driver's own words are written to
/// the terminal first, with the adapter they came from.
fn say_why_the_device_is_lost(render: &eframe::egui_wgpu::RenderState) {
    let info = render.adapter.get_info();
    let adapter = format!(
        "{} through {:?}, driver {} {}",
        info.name, info.backend, info.driver, info.driver_info
    );
    render
        .device
        .set_device_lost_callback(move |reason, message| {
            // Closing the window destroys the device, which is no loss.
            if matches!(reason, eframe::wgpu::DeviceLostReason::Destroyed) {
                return;
            }
            eprintln!(
                "camcraft: the graphics device was lost: {message} ({adapter}). The window \n                 cannot draw again and will close; jobs not saved are lost. If this was the \n                 computer running out of memory, close other programs and open the job again."
            );
        });
}

/// Generating works every core it is given, for as long as it takes; the
/// window must still draw each frame meanwhile. So the work gets every core
/// but one, which is the window's.
pub(crate) fn leave_a_core_for_the_window() {
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    // Already built only if something ran before main, which nothing does;
    // then the pool it built stands.
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(cores.saturating_sub(1).max(1))
        .build_global();
}

#[cfg(test)]
mod tests {
    /// The icon is the rendered PNG, not egui's own for want of one.
    #[test]
    fn the_icon_loads() {
        let icon = super::icon();
        assert_eq!((icon.width, icon.height), (512, 512));
    }
}
