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

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::expect_used,
    clippy::indexing_slicing
)]

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

/// Step frames until the worker has replied, and anything the reply started (a
/// job's models, once it has opened) has finished too.
fn settle(harness: &mut Harness<'_, App>) {
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        harness.step();
        while harness.state().busy().is_some() {
            assert!(Instant::now() < deadline, "the worker never finished");
            std::thread::sleep(Duration::from_millis(50));
            harness.step();
        }
        // Let the reply's effects land and the layout settle; stop once that
        // starts nothing more.
        harness.run_steps(4);
        if harness.state().busy().is_none() {
            break;
        }
    }
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
    shoot(&mut h, "start_screen");

    let ctx = h.ctx.clone();
    h.state_mut()
        .open_job(&ctx, repo("examples/rail-jig.camcraft"));
    settle(&mut h);
    shoot(&mut h, "job_opened");

    h.state_mut().start_run(&ctx);
    settle(&mut h);

    let outcome = h.state().outcome().expect("the run produced a result");
    let passes: Vec<u64> = outcome.result.operations.iter().map(|o| o.passes).collect();
    println!("passes {passes:?}, verified {}", outcome.result.is_clean());
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

    // The same, in inches, with the settings window that chose them open.
    h.state_mut()
        .settings_for_snapshot(crate::measure::Measure::Imperial);
    h.run_steps(3);
    shoot(&mut h, "steep_deepest_imperial");

    // A check fails: the HALT banner, and posting asks before writing anyway.
    h.state_mut()
        .settings_for_snapshot(crate::measure::Measure::Metric);
    let out = output_dir().join("posted");
    let _ = std::fs::remove_dir_all(&out);
    h.state_mut().fail_a_check_and_post_for_snapshot(&out);
    h.run_steps(3);
    shoot(&mut h, "checks_failed_post_prompt");
}

/// A job with a top and a bottom setup sharing one block, open beside the example,
/// with the tree, an operation's properties, the tool picker and the prompt before
/// unsaved work is lost.
#[test]
#[ignore = "renders to PNG for looking at; run with --features ui-snapshots"]
fn two_setups_in_tabs() {
    use crate::document::Selection;
    use camcraft_core::frame::Orientation;
    use camcraft_core::job::{Along, BoxOf, Origin};

    // The example, with a second setup turned over and zeroed on the stock's top.
    let dir = output_dir().join("two-sided");
    std::fs::create_dir_all(&dir).unwrap();
    let mut job = camcraft_session::read_job(&repo("examples/rail-jig.camcraft")).unwrap();
    job.models[0].path = repo("fixtures/step/sled_base.step").display().to_string();
    let mut bottom = job.setups[0].clone();
    bottom.name = "bottom".into();
    bottom.wcs = 55;
    bottom.orientation = Orientation::default().flipped();
    bottom.origin = Origin::BoxPoint {
        of: BoxOf::Stock,
        x: Along::Center,
        y: Along::Center,
        z: Along::Max,
        offset: [0.0; 3],
    };
    bottom.operations.truncate(1);
    job.setups.push(bottom);
    let path = dir.join("two-sided.camcraft");
    camcraft_session::write_job(&path, &job).unwrap();

    let mut h = harness();
    h.run_steps(2);
    h.state_mut().library_for_snapshot(dir.join("library.toml"));
    let ctx = h.ctx.clone();
    h.state_mut()
        .open_job(&ctx, repo("examples/rail-jig.camcraft"));
    settle(&mut h);
    h.state_mut().open_job(&ctx, path);
    settle(&mut h);

    // The bottom setup: the part turned over, its zero on the stock's top.
    if let Some(doc) = h.state_mut().doc_for_snapshot() {
        doc.show("bottom");
    }
    h.run_steps(3);
    shoot(&mut h, "two_sided_bottom");

    h.state_mut().start_run(&ctx);
    settle(&mut h);
    h.run_steps(2);
    shoot(&mut h, "two_sided_bottom_generated");

    // An operation's properties.
    if let Some(doc) = h.state_mut().doc_for_snapshot() {
        doc.selection = Selection::Operation("top".into(), 1);
        doc.active = Some("top".into());
    }
    h.run_steps(3);
    shoot(&mut h, "operation_properties");

    // Choosing its tool, from a folder of the library.
    h.state_mut().pick_tool_for_snapshot(Some(("top", 1)));
    // Many frames: a window that grows a little each frame only shows after a while.
    h.run_steps(60);
    shoot(&mut h, "tool_picker");
    h.state_mut().pick_tool_for_snapshot(None);

    // An edit makes the job unsaved, and its result out of date; closing asks.
    if let Some(doc) = h.state_mut().doc_for_snapshot() {
        doc.job.setups[1].operations[0].passes.stepover = 2.0;
        doc.selection = Selection::Setup("bottom".into());
        doc.active = Some("bottom".into());
    }
    h.run_steps(3);
    shoot(&mut h, "stale_after_edit");
    h.state_mut().close_for_snapshot();
    h.run_steps(3);
    shoot(&mut h, "close_unsaved_prompt");
}

/// The file dialog, open on the job filter: drawn inside the app, themed with it.
#[test]
#[ignore = "renders to PNG for looking at; run with --features ui-snapshots"]
fn open_job_dialog() {
    let mut h = harness();
    h.run_steps(2);
    h.state_mut().open_dialog_for_snapshot();
    h.run_steps(4);
    shoot(&mut h, "open_job_dialog");
}

/// The tool manager, rendered from its real UI: the library with a fully
/// described tool, the same in inches, and a validation error.
#[test]
#[ignore = "renders to PNG for looking at; run with --features ui-snapshots"]
fn tool_manager() {
    use crate::tool_manager::ToolManager;
    use camcraft_core::job::{
        CuttingData, Holder, HolderSegment, ToolKind, ToolMaterial, ToolSpec,
    };

    let library = output_dir().join("snapshot-library.toml");
    let _ = std::fs::remove_file(&library);
    let mut manager = ToolManager::with_library(library);

    let shoot_manager = |manager: ToolManager, name: &str| {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1180.0, 900.0))
            .wgpu()
            .build_ui_state(|ui, m: &mut ToolManager| m.ui(ui), manager);
        // Before the first frame, as the app does at startup.
        crate::theme::apply(&h.ctx);
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
    manager.show_for_snapshot("bullnose-8", Some(described.clone()));
    let mut manager = shoot_manager(manager, "tools_library");

    // The same tool shown in inches: the fields and labels are wider.
    manager.measure = crate::measure::Measure::Imperial;
    let mut manager = shoot_manager(manager, "tools_library_imperial");
    manager.measure = crate::measure::Measure::Metric;

    // Flutes longer than the stickout: the error sits beside the field.
    let wrong = ToolSpec {
        flute_length: Some(40.0),
        ..described
    };
    manager.show_for_snapshot("bullnose-8", Some(wrong));
    let mut manager = shoot_manager(manager, "tools_error");

    // Folders for two machines, the first open with the tools inside it.
    manager.folders_for_snapshot();
    shoot_manager(manager, "tools_library_folders");
}
