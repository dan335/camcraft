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
        .with_size(egui::vec2(1400.0, 1250.0))
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
    h.run_steps(2);
    shoot(&mut h, "after_run");

    // Part-way through roughing, with the cutter where the scrub stands.
    h.state_mut().set_view_for_snapshot(0, 0.4, u32::MAX);
    h.run_steps(2);
    shoot(&mut h, "scrub_mid_rough");

    // Part-way through finishing: the ball riding the top of the part.
    h.state_mut().set_view_for_snapshot(1, 0.5, u32::MAX);
    h.run_steps(2);
    shoot(&mut h, "scrub_mid_finish");

    // Rapids alone, all the way through: every traverse in the program.
    h.state_mut().set_view_for_snapshot(1, 1.0, 1 << 1);
    h.run_steps(2);
    shoot(&mut h, "rapids_only");

    // The finished material, coloured against the part, with its legend.
    h.state_mut().set_stock_for_snapshot(Some(passes.len()));
    h.run_steps(3);
    shoot(&mut h, "stock_final");

    // The material after roughing: what finishing starts from.
    h.state_mut().set_stock_for_snapshot(Some(1));
    h.run_steps(3);
    shoot(&mut h, "stock_after_rough");

    // Clicking the deepest steep descent: scrub, stock and camera all go there.
    let deepest = h.state_mut().jump_to_deepest_for_snapshot();
    println!("deepest steep descent {deepest:?}");
    h.run_steps(3);
    shoot(&mut h, "steep_deepest");
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

/// The tool manager, rendered from its real UI: the job's tools with one selected
/// and drawn, the library with a fully described tool, and a validation error.
#[test]
#[ignore = "renders to PNG for looking at; run with --features ui-snapshots"]
fn tool_manager() {
    use crate::tool_manager::{Source, ToolManager};
    use camcraft_core::job::{
        CuttingData, Holder, HolderSegment, Job, ToolKind, ToolMaterial, ToolSpec,
    };

    let library = output_dir().join("snapshot-library.toml");
    let _ = std::fs::remove_file(&library);
    let mut manager = ToolManager::with_library(library);
    let job: Job = toml::from_str(include_str!("../../../examples/rail-jig.toml")).unwrap();
    manager.set_job(&repo("examples/rail-jig.toml"), &job);
    manager.show_for_snapshot(Source::Job, "finish-6-ball", None);

    let shoot_manager = |manager: ToolManager, name: &str| {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1180.0, 900.0))
            .wgpu()
            .build_ui_state(
                |ui, m: &mut ToolManager| {
                    m.ui(ui);
                },
                manager,
            );
        h.run_steps(3);
        match h.render() {
            Ok(image) => {
                let path = output_dir().join(format!("{name}.png"));
                image.save(&path).unwrap();
                println!("wrote {}", path.display());
            }
            Err(err) => println!("skipped {name}: could not render ({err})"),
        }
        h.into_state()
    };
    let mut manager = shoot_manager(manager, "tools_job");

    let described = ToolSpec {
        kind: ToolKind::Bullnose,
        diameter: 8.0,
        corner: 1.0,
        number: 4,
        rpm: 16000,
        comment: "8mm bullnose r1".into(),
        flutes: Some(3),
        flute_length: Some(20.0),
        shoulder_length: Some(24.0),
        shank_diameter: Some(8.0),
        overall_length: Some(63.0),
        stickout: Some(32.0),
        material: Some(ToolMaterial::Carbide),
        vendor: Some("Amana".into()),
        product: Some("46260".into()),
        holder: Some(Holder {
            name: "ER20 collet chuck".into(),
            segments: vec![
                HolderSegment {
                    diameter: 34.0,
                    length: 22.0,
                },
                HolderSegment {
                    diameter: 44.0,
                    length: 30.0,
                },
            ],
        }),
        cutting: Some(CuttingData {
            feed: Some(2400.0),
            plunge_feed: Some(500.0),
            ramp_feed: None,
        }),
    };
    manager.show_for_snapshot(Source::Library, "bullnose-8", Some(described.clone()));
    let mut manager = shoot_manager(manager, "tools_library");

    // Flutes longer than the stickout: the error sits beside the field.
    let wrong = ToolSpec {
        flute_length: Some(40.0),
        ..described
    };
    manager.show_for_snapshot(Source::Library, "bullnose-8", Some(wrong));
    shoot_manager(manager, "tools_error");
}
