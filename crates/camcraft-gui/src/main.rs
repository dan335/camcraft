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
mod transport;

// A job as the app holds it, and the work done off the UI thread.
mod document;
mod worker;

// The 3D view, picking in it, and the material it draws.
mod picking;
mod stock;
mod view;

// The design system, and lengths in millimetres or inches.
mod ui;

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

fn main() -> eframe::Result {
    let jobs = jobs_from_args();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("camcraft")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([820.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native(
        "camcraft",
        options,
        Box::new(|cc| Ok(Box::new(app::App::with_jobs(cc, jobs)))),
    )
}
