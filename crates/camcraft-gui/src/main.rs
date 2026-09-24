//! camcraft desktop app.
//!
//! Open a job, run its setups, and see what will be cut. It runs on the same
//! engine path as the `camcraft` command line, through `camcraft-session`, so the
//! two cannot disagree about what a file means, what runs, or what may be posted.

mod app;
mod browser;
mod camera;
mod document;
mod file_dialog;
mod live;
mod measure;
mod pick;
mod picking;
mod playback;
mod posting;
mod properties;
mod pyramid;
mod scene;
mod settings;
mod slots;
mod stock_tiles;
mod stock_view;
#[cfg(test)]
mod test_jobs;
mod theme;
mod tiles;
mod tool_drawing;
mod tool_editor;
mod tool_manager;
mod tool_picker;
mod transport;
#[cfg(all(test, feature = "ui-snapshots"))]
mod ui_snapshots;
mod viewport;
mod widgets;
mod worker;

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
