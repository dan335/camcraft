//! Renders the real app headlessly to PNG files, so it can be looked at without a
//! window and a person at the screen.
//!
//! The approach is nodemangler's (`mangler_gui/src/ui_snapshots.rs`). These are
//! not assertions about pixels. They exist because a visual defect (the part
//! upside down, a panel cramped, a label clipped) is invisible to ordinary tests.
//! The viewport renders offscreen through wgpu, so it appears in the snapshot too.
//!
//! Behind the off-by-default `ui-snapshots` feature and `#[ignore]`d. Run with
//! `--release`, because the job runs in the engine:
//!
//! ```text
//! cargo test --release -p camcraft-gui --features ui-snapshots -- --ignored --nocapture
//! ```
//!
//! and open the paths it prints (under `target/ui-snapshots/`).

#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use egui_kittest::Harness;

use crate::app::App;

fn output_dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

/// Step frames until the worker has replied.
fn settle(harness: &mut Harness<'_, App>) {
    let deadline = Instant::now() + Duration::from_secs(600);
    harness.step();
    while harness.state().busy().is_some() {
        assert!(Instant::now() < deadline, "the worker never finished");
        std::thread::sleep(Duration::from_millis(50));
        harness.step();
    }
    // Let the reply's effects land and the layout settle.
    harness.run_steps(4);
}

fn shoot(harness: &mut Harness<'_, App>, name: &str) {
    match harness.render() {
        Ok(image) => {
            let path = output_dir().join(format!("{name}.png"));
            image.save(&path).unwrap();
            println!("wrote {}", path.display());
        }
        // No GPU adapter (a headless box): say so rather than fail.
        Err(err) => println!("skipped {name}: could not render ({err})"),
    }
}

fn harness() -> Harness<'static, App> {
    Harness::builder()
        .with_size(egui::vec2(1400.0, 860.0))
        .wgpu()
        .build_eframe(|cc| App::new(cc))
}

/// The router jig from STEP with the example job, run: the view a person gets
/// after pressing Run.
#[test]
#[ignore = "renders to PNG for looking at; run with --features ui-snapshots"]
fn main_window_after_a_run() {
    let mut h = harness();
    h.run_steps(2);
    shoot(&mut h, "empty");

    let ctx = h.ctx.clone();
    h.state_mut()
        .open_model(&ctx, repo("fixtures/step/sled_base.step"));
    settle(&mut h);
    shoot(&mut h, "model_loaded");

    h.state_mut().open_job(&ctx, repo("examples/rail-jig.toml"));
    settle(&mut h);
    h.state_mut().start_run(&ctx);
    settle(&mut h);

    let outcome = h.state().outcome().expect("the run produced a result");
    let passes: Vec<u64> = outcome
        .ran
        .result
        .operations
        .iter()
        .map(|o| o.passes)
        .collect();
    println!(
        "passes {passes:?}, verified {}",
        outcome.ran.result.is_clean()
    );
    shoot(&mut h, "after_run");
}

/// The file dialog, open on the model filter: drawn inside the app, themed with it.
#[test]
#[ignore = "renders to PNG for looking at; run with --features ui-snapshots"]
fn open_model_dialog() {
    let mut h = harness();
    h.run_steps(2);
    h.state_mut().open_dialog_for_snapshot();
    h.run_steps(4);
    shoot(&mut h, "open_model_dialog");
}
