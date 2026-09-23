//! camcraft desktop app.
//!
//! Open a job, run its setups, and see what will be cut. It runs on the same
//! engine path as the `camcraft` command line, through `camcraft-session`, so the
//! two cannot disagree about what a file means, what runs, or what may be posted.

mod app;
mod camera;
mod file_dialog;
mod measure;
mod scene;
mod settings;
mod stock_view;
#[cfg(test)]
mod test_jobs;
mod theme;
mod tool_drawing;
mod tool_editor;
mod tool_manager;
#[cfg(all(test, feature = "ui-snapshots"))]
mod ui_snapshots;
mod viewport;
mod worker;

use eframe::egui;

/// The job named on the command line, if any: `camcraft-gui [JOB]`.
fn job_from_args() -> Option<std::path::PathBuf> {
    std::env::args_os().nth(1).map(std::path::PathBuf::from)
}

fn main() -> eframe::Result {
    let job = job_from_args();
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
        Box::new(|cc| Ok(Box::new(app::App::with_job(cc, job)))),
    )
}
