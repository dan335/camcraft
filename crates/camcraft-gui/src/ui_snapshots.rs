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
use egui_kittest::kittest::Queryable as _;

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

    // Part-way through roughing, with the cutter where the playhead stands.
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

    // Playing: the material cut as far as the playhead, part-way through
    // roughing and then finishing, in the material's colour, with the transport.
    for (op, fraction, name) in [
        (0, 0.3, "playback_mid_rough"),
        (1, 0.5, "playback_mid_finish"),
    ] {
        h.state_mut().set_stock_for_snapshot(None);
        h.state_mut().set_view_for_snapshot(op, fraction, u32::MAX);
        let deadline = Instant::now() + Duration::from_secs(60);
        h.run_steps(2);
        while !h.state_mut().live_caught_up_for_snapshot() {
            assert!(Instant::now() < deadline, "the material never caught up");
            std::thread::sleep(Duration::from_millis(20));
            h.step();
        }
        h.run_steps(3);
        shoot(&mut h, name);
    }

    // Space plays: the playhead moves on in machining time, and Space again stops
    // it where it is.
    let (before, _) = h.state_mut().playhead_for_snapshot().unwrap();
    h.key_press(egui::Key::Space);
    h.run_steps(20);
    let (playing, is_playing) = h.state_mut().playhead_for_snapshot().unwrap();
    assert!(is_playing, "Space should start playback");
    assert!(playing.shown > before.shown, "{before:?} -> {playing:?}");
    h.key_press(egui::Key::Space);
    h.run_steps(2);
    let (stopped, is_playing) = h.state_mut().playhead_for_snapshot().unwrap();
    assert!(!is_playing, "Space again should pause");
    h.run_steps(5);
    assert_eq!(h.state_mut().playhead_for_snapshot().unwrap().0, stopped);
    println!("played from {before:?} to {stopped:?}");

    // Clicking the deepest steep descent: playhead, stock and camera all go there.
    let deepest = h.state_mut().jump_to_deepest_for_snapshot();
    println!("deepest steep descent {deepest:?}");
    h.run_steps(3);
    shoot(&mut h, "steep_deepest");

    // Faces picked for finishing: lit in the view, listed in the panel, the
    // boundary they make drawn at the stock's top.
    let faces = h.state_mut().pick_faces_for_snapshot(1);
    println!("{faces} faces on the model");
    h.run_steps(4);
    shoot(&mut h, "faces_picked");
    h.state_mut().escape_for_snapshot();

    // The work zero, picked on a hole: the pointer over its rim offers the
    // centre, and a click puts the work zero there.
    let (rim, centre) = h
        .state_mut()
        .arm_work_zero_for_snapshot()
        .expect("the model has a round rim in view");
    h.run_steps(2);
    h.hover_at(rim);
    h.run_steps(3);
    shoot(&mut h, "work_zero_hover");
    h.drag_at(rim);
    h.step();
    h.drop_at(rim);
    h.run_steps(4);
    shoot(&mut h, "work_zero_picked");
    let origin = h.state_mut().doc_for_snapshot().unwrap().job.setups[0]
        .origin
        .clone();
    let camcraft_core::job::Origin::Point { point, .. } = &origin else {
        panic!("the click did not set the work zero: {origin:?}");
    };
    assert_eq!(point.snap, camcraft_core::job::Snap::CircleCenter);
    println!("work zero on {point:?}, rim centre was {centre}");
    // Back as it was, for the shots after.
    h.state_mut().doc_for_snapshot().unwrap().undo();
    h.run_steps(2);

    // The same, in inches, with the settings window that chose them open.
    h.state_mut()
        .settings_for_snapshot(crate::ui::measure::Measure::Imperial);
    h.run_steps(3);
    shoot(&mut h, "steep_deepest_imperial");

    // A check fails: the HALT banner, and posting asks before writing anyway.
    h.state_mut()
        .settings_for_snapshot(crate::ui::measure::Measure::Metric);
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

/// The setup's panel on each of its tabs, the list of models to add from, and
/// models picked in the view with the job's others shown faintly beside the part.
#[test]
#[ignore = "renders to PNG for looking at; run with --features ui-snapshots"]
fn setup_panel() {
    use crate::document::Selection;
    use crate::picking::slots::Slot;
    use camcraft_core::job::{ModelDef, StockDef, StockSpec};

    // The example, with two more models and a second stock the setup does not use.
    let dir = output_dir().join("setup-panel");
    std::fs::create_dir_all(&dir).unwrap();
    let mut job = camcraft_session::read_job(&repo("examples/rail-jig.camcraft")).unwrap();
    job.models[0].path = repo("fixtures/step/sled_base.step").display().to_string();
    for (name, file) in [
        ("rail_guide", "rail_guide.step"),
        ("hinge_jig", "hinge_jig.step"),
    ] {
        job.models.push(ModelDef {
            name: name.into(),
            path: repo(&format!("fixtures/step/{file}")).display().to_string(),
            units: None,
        });
    }
    job.stocks.push(StockDef {
        name: "offcut".into(),
        spec: StockSpec::FixedBox {
            corner: [120.0, 0.0, 0.0],
            size: [80.0, 60.0, 20.0],
        },
    });
    let path = dir.join("setup-panel.camcraft");
    camcraft_session::write_job(&path, &job).unwrap();

    let mut h = harness();
    h.run_steps(2);
    let ctx = h.ctx.clone();
    h.state_mut().open_job(&ctx, path);
    settle(&mut h);
    if let Some(doc) = h.state_mut().doc_for_snapshot() {
        doc.selection = Selection::Setup("top".into());
        doc.active = Some("top".into());
    }
    for (tab, name) in [
        (0_usize, "setup_parts"),
        (1, "setup_position"),
        (2, "setup_post"),
    ] {
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("setup-tab"), tab));
        h.run_steps(3);
        shoot(&mut h, name);
    }

    // The work zero on the stock's top back-left corner, chosen from the grid.
    if let Some(doc) = h.state_mut().doc_for_snapshot() {
        doc.job.setups[0].origin = camcraft_core::job::Origin::BoxPoint {
            of: camcraft_core::job::BoxOf::Stock,
            x: camcraft_core::job::Along::Min,
            y: camcraft_core::job::Along::Max,
            z: camcraft_core::job::Along::Max,
            offset: [0.0, 0.0, 1.5],
        };
    }
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("setup-tab"), 1_usize));
    h.run_steps(4);
    shoot(&mut h, "setup_work_zero_grid");

    // The list of models to add from.
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("setup-tab"), 0_usize));
    h.run_steps(2);
    // The models card's button, not the Add menu in the menu bar above.
    h.get_all_by_label("Add")
        .find(|n| n.rect().top() > 40.0)
        .unwrap()
        .click();
    h.run_steps(3);
    shoot(&mut h, "setup_add_models");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);

    // Picking models in the view: the job's others drawn faintly where the setup
    // would put them.
    h.state_mut().arm_for_snapshot(Slot::Models {
        setup: "top".into(),
    });
    h.run_steps(4);
    shoot(&mut h, "setup_pick_models");
    h.state_mut().arm_for_snapshot(Slot::Stocks {
        setup: "top".into(),
    });
    h.run_steps(4);
    shoot(&mut h, "setup_pick_stocks");
    h.state_mut().escape_for_snapshot();

    // An operation's panel, on its tabs.
    if let Some(doc) = h.state_mut().doc_for_snapshot() {
        doc.selection = Selection::Operation("top".into(), 1);
    }
    for (tab, name) in [
        (0_usize, "operation_tool"),
        (1, "operation_geometry"),
        (2, "operation_heights"),
        (3, "operation_passes"),
    ] {
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("operation-tab"), tab));
        h.run_steps(3);
        shoot(&mut h, name);
    }

    // A roughing operation's heights: clearance, top and bottom, as planes.
    if let Some(doc) = h.state_mut().doc_for_snapshot() {
        doc.selection = Selection::Operation("top".into(), 0);
    }
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("operation-tab"), 2_usize));
    h.run_steps(3);
    shoot(&mut h, "operation_heights_rough");
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
    use crate::tools::manager::ToolManager;
    use camcraft_core::job::{
        Coolant, Holder, HolderSegment, Preset, SpindleDirection, StockToLeave, ToolKind,
        ToolMaterial, ToolSpec,
    };

    let library = output_dir().join("snapshot-library.toml");
    let _ = std::fs::remove_file(&library);
    let mut manager = ToolManager::with_library(library);

    // Each shot names the editor's tab to show, or none for the one it opens on.
    let shoot_tabs = |manager: ToolManager, shots: &[(&str, Option<&str>)]| {
        use egui_kittest::kittest::Queryable;
        let mut h = Harness::builder()
            .with_size(egui::vec2(1280.0, 860.0))
            .wgpu()
            .build_ui_state(
                |ui, m: &mut ToolManager| {
                    // The harness draws a frame before the theme can be applied,
                    // and the app never does: skip it, since the fonts the
                    // manager names are not loaded yet.
                    if ui
                        .ctx()
                        .fonts(|f| f.definitions().font_data.contains_key("plex"))
                    {
                        m.ui(ui);
                    }
                },
                manager,
            );
        crate::ui::theme::apply(&h.ctx);
        h.run_steps(3);
        for (name, tab) in shots {
            if let Some(tab) = tab {
                h.get_by_label(tab).click();
                h.run_steps(3);
            }
            match h.render() {
                Ok(image) => {
                    let path = output_dir().join(format!("{name}.png"));
                    image.save(&path).unwrap();
                    println!("wrote {}", path.display());
                }
                Err(err) => println!("skipped {name}: could not render ({err})"),
            }
        }
        h.into_state()
    };
    let shoot_manager = |manager: ToolManager, name: &str| shoot_tabs(manager, &[(name, None)]);
    let described = ToolSpec {
        kind: ToolKind::Bullnose,
        diameter: 8.0,
        corner: 1.0,
        angle: None,
        tip_diameter: 0.0,
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
        spindle: SpindleDirection::Clockwise,
        coolant: Coolant::Mist,
        presets: vec![
            Preset {
                name: "Roughing".into(),
                rpm: Some(16000),
                feed: Some(2400.0),
                plunge_feed: Some(500.0),
                stepdown: Some(3.0),
                stepover: Some(3.0),
                stock_to_leave: Some(StockToLeave {
                    radial: 0.3,
                    axial: 0.2,
                }),
                ..Preset::default()
            },
            Preset {
                name: "Finishing".into(),
                comment: "from the maker's chart".into(),
                stepover: Some(0.4),
                stock_to_leave: Some(StockToLeave::default()),
                ..Preset::default()
            },
        ],
    };
    manager.show_for_snapshot("bullnose-8", Some(described.clone()));
    let mut manager = shoot_tabs(
        manager,
        &[
            ("tools_library", None),
            ("tools_library_general", Some("General")),
            ("tools_library_holder", Some("Holder")),
            ("tools_library_presets", Some("Presets")),
            ("tools_library_post", Some("Post")),
        ],
    );

    // The same tool shown in inches: the fields and labels are wider.
    manager.measure = crate::ui::measure::Measure::Imperial;
    let mut manager = shoot_manager(manager, "tools_library_imperial");
    manager.measure = crate::ui::measure::Measure::Metric;

    // Flutes longer than the stickout: the error sits beside the field.
    let wrong = ToolSpec {
        flute_length: Some(40.0),
        ..described
    };
    manager.show_for_snapshot("bullnose-8", Some(wrong));
    let mut manager = shoot_manager(manager, "tools_error");

    // A 60° V-bit with a small flat tip: the flank drawn to its angle.
    let vbit = ToolSpec {
        kind: ToolKind::Cone,
        diameter: 12.7,
        angle: Some(60.0),
        tip_diameter: 0.5,
        number: 7,
        comment: "1/2in 60° V-bit".into(),
        flutes: Some(2),
        flute_length: Some(14.0),
        shank_diameter: Some(12.7),
        stickout: Some(25.0),
        overall_length: Some(50.0),
        ..ToolSpec::default()
    };
    manager.show_for_snapshot("vbit-60", Some(vbit));
    let mut manager = shoot_manager(manager, "tools_vbit");

    // Folders for two machines, the first open with the tools inside it.
    manager.folders_for_snapshot();
    let mut manager = shoot_manager(manager, "tools_library_folders");

    // A folder with nothing in it: what to do instead of a blank table.
    manager.empty_folder_for_snapshot();
    shoot_manager(manager, "tools_empty_folder");
}

/// The lego brick roughed from a block, as it plays: the material at the
/// simulator's fine pitch, with the walls the cutter leaves standing vertical.
#[test]
#[ignore = "renders to PNG for looking at; run with --features ui-snapshots"]
fn lego_playback() {
    let dir = output_dir().join("lego");
    std::fs::create_dir_all(&dir).unwrap();
    let model = repo("fixtures/step/lego_4x2_01.step");
    let job = format!(
        r#"version = 1
tolerance = 0.05

[tools.rough-6-flat]
kind = "flat"
diameter = 6.35
number = 1
rpm = 16000

[[model]]
name = "lego"
path = "{}"

[[stock]]
name = "block"
kind = "around-models"
margin = [2.0, 2.0, 2.0]

[[setup]]
name = "top"
models = ["lego"]
stocks = ["block"]

[[setup.operation]]
name = "rough"
strategy = "zlevel-rough"
stepdown = 2.0
min_ring_length = 3.0
tool = "rough-6-flat"
"#,
        model.display()
    );
    let path = dir.join("lego.camcraft");
    std::fs::write(&path, job).unwrap();

    let mut h = harness();
    h.run_steps(2);
    let ctx = h.ctx.clone();
    h.state_mut().open_job(&ctx, path);
    settle(&mut h);
    h.state_mut().start_run(&ctx);
    settle(&mut h);

    for (fraction, name) in [(0.5, "lego_half_roughed"), (1.0, "lego_roughed")] {
        h.state_mut().set_stock_for_snapshot(None);
        h.state_mut().set_view_for_snapshot(0, fraction, u32::MAX);
        let deadline = Instant::now() + Duration::from_secs(120);
        h.run_steps(2);
        while !h.state_mut().live_caught_up_for_snapshot() {
            assert!(Instant::now() < deadline, "the material never caught up");
            std::thread::sleep(Duration::from_millis(20));
            h.step();
        }
        h.run_steps(3);
        shoot(&mut h, name);
    }
    // How long a frame of the whole brick takes to draw and read back: an upper
    // bound on what drawing the material costs.
    let frame_ms = |h: &mut Harness<'_, App>| {
        let started = Instant::now();
        for _ in 0..20 {
            h.step();
            let _ = h.render();
        }
        started.elapsed().as_secs_f64() * 1e3 / 20.0
    };
    let with = frame_ms(&mut h);
    h.state_mut().stock_off_for_snapshot();
    let without = frame_ms(&mut h);
    h.state_mut().set_stock_for_snapshot(None);
    h.run_steps(3);
    println!(
        "lego frame, drawn and read back: {with:.1} ms with the material, {without:.1} ms without"
    );

    // Close in on a stud, to see the walls and the cells.
    h.state_mut()
        .focus_for_snapshot(glam::DVec3::new(20.0, 12.0, 30.0), 18.0);
    h.run_steps(3);
    shoot(&mut h, "lego_close");
}

/// A full 5 by 12 foot sheet, 8 inches thick, with a sixteenth-inch cutter in the
/// setup: the largest stock a router here takes, at the finest pitch memory allows
/// for it, drawn as tiles. Surfaced down to the lego brick with a half-inch end
/// mill; the sixteenth has nothing to clear and only sets the pitch.
#[test]
#[ignore = "renders to PNG for looking at; run with --features ui-snapshots"]
fn sheet_playback() {
    let dir = output_dir().join("sheet");
    std::fs::create_dir_all(&dir).unwrap();
    let model = repo("fixtures/step/lego_4x2_01.step");
    let job = format!(
        r#"version = 1
tolerance = 0.05

[tools.half-inch]
kind = "flat"
diameter = 12.7
number = 1

[tools.sixteenth]
kind = "flat"
diameter = 1.5875
number = 2

[[model]]
name = "lego"
path = "{}"

[[stock]]
name = "sheet"
kind = "fixed-box"
corner = [-700.0, -1800.0, -163.0]
size = [1524.0, 3658.0, 203.2]

[[setup]]
name = "top"
models = ["lego"]
stocks = ["sheet"]

[[setup.operation]]
name = "surface"
strategy = "zlevel-rough"
stepdown = 20.0
tool = "half-inch"
passes = {{ stepover = 6.0, feed = 5000.0 }}

[[setup.operation]]
name = "detail"
strategy = "zlevel-rough"
stepdown = 1.0
tool = "sixteenth"
geometry = {{ top = {{ ref = "stock-top", offset = 3.0 }}, bottom = {{ ref = "stock-top", offset = 1.0 }} }}
passes = {{ stepover = 0.8, feed = 1500.0 }}
"#,
        model.display()
    );
    let path = dir.join("sheet.camcraft");
    std::fs::write(&path, job).unwrap();

    let mut h = harness();
    h.run_steps(2);
    let ctx = h.ctx.clone();
    h.state_mut().open_job(&ctx, path);
    settle(&mut h);
    let started = Instant::now();
    h.state_mut().start_run(&ctx);
    settle(&mut h);
    println!("sheet run: {:.1} s", started.elapsed().as_secs_f64());

    for (fraction, name) in [(0.4, "sheet_part_surfaced"), (1.0, "sheet_surfaced")] {
        h.state_mut().set_stock_for_snapshot(None);
        h.state_mut().set_view_for_snapshot(0, fraction, u32::MAX);
        let started = Instant::now();
        let deadline = started + Duration::from_secs(300);
        h.run_steps(2);
        while !h.state_mut().live_caught_up_for_snapshot() {
            assert!(Instant::now() < deadline, "the material never caught up");
            std::thread::sleep(Duration::from_millis(20));
            h.step();
        }
        println!(
            "{name}: caught up in {:.1} s",
            started.elapsed().as_secs_f64()
        );
        h.run_steps(3);
        shoot(&mut h, name);
    }
    let frame = |h: &mut Harness<'_, App>| {
        let started = Instant::now();
        for _ in 0..10 {
            h.step();
            let _ = h.render();
        }
        started.elapsed().as_secs_f64() * 1e3 / 10.0
    };
    println!("sheet frame, drawn and read back: {:.1} ms", frame(&mut h));
    // In close on the brick, where the tiles meet nothing but material.
    h.state_mut()
        .focus_for_snapshot(glam::DVec3::new(0.0, 0.0, 40.0), 120.0);
    h.run_steps(3);
    shoot(&mut h, "sheet_close");
    // The whole sheet: eighteen tiles, and no seams between them.
    h.state_mut()
        .focus_for_snapshot(glam::DVec3::new(62.0, 29.0, 0.0), 2600.0);
    h.run_steps(3);
    shoot(&mut h, "sheet_whole");
}
