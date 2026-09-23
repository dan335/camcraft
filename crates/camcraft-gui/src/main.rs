//! camcraft desktop app.
//!
//! Open a part and a job, run it, and see what will be cut. It runs on the same
//! engine path as the `camcraft` command line, through `camcraft-session`, so the
//! two cannot disagree about what a file means, what runs, or what may be posted.

mod app;
mod camera;
mod file_dialog;
#[cfg(all(test, feature = "ui-snapshots"))]
mod ui_snapshots;
mod viewport;
mod worker;

use eframe::egui;

fn main() -> eframe::Result {
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
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
