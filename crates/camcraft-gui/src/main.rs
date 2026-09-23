//! camcraft desktop app.
//!
//! Open a part and a job, run it, and see what will be cut. It runs on the same
//! engine path as the `camcraft` command line, through `camcraft-session`, so the
//! two cannot disagree about what a file means, what runs, or what may be posted.

mod app;
mod camera;
mod file_dialog;
mod measure;
mod scene;
mod settings;
mod stock_view;
mod theme;
mod tool_drawing;
mod tool_manager;
#[cfg(all(test, feature = "ui-snapshots"))]
mod ui_snapshots;
mod viewport;
mod worker;

use eframe::egui;

/// Files named on the command line: `camcraft-gui [MODEL] [JOB]`, in either order.
/// A `.toml` is the job; anything else is the model, and the app says so if it
/// cannot read it.
fn files_from_args() -> (Option<std::path::PathBuf>, Option<std::path::PathBuf>) {
    let mut model = None;
    let mut job = None;
    for arg in std::env::args_os().skip(1) {
        let path = std::path::PathBuf::from(arg);
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
        {
            job = Some(path);
        } else {
            model = Some(path);
        }
    }
    (model, job)
}

fn main() -> eframe::Result {
    let (model, job) = files_from_args();
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
        Box::new(|cc| Ok(Box::new(app::App::with_files(cc, model, job)))),
    )
}
