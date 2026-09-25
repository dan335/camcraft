//! The document's tests: editing, renaming, undo, what runs, and when a result
//! is out of date.

use super::run::SetupInputs;
use super::*;
use camcraft_core::job::{Preset, StrategySpec, ToolSpec};
use camcraft_core::run::{Plan, Stage};
use glam::DVec3;

use crate::view::viewport::OutlineKind;

fn example() -> Document {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/rail-jig.camcraft");
    let job = camcraft_session::read_job(&path).unwrap();
    Document::opened(1, Some(path), job)
}

/// A new job whose setup machines a plate with a hole, loaded as if from disk.
fn with_plate() -> (Document, String) {
    let mut doc = Document::untitled(1);
    let def = ModelDef {
        name: "p".into(),
        path: "p.stl".into(),
        units: Some(LengthUnit::Mm),
    };
    let body = camcraft_geom::topology::Body::derived(camcraft_geom::synthetic::plate_with_hole(
        20.0, 5.0, 4.0, 32,
    ));
    let bvh = Arc::new(MeshBvh::new(body.mesh()));
    let model = Arc::new(Model {
        bounds: body.mesh().bounds().unwrap(),
        body,
        path: "p.stl".into(),
        units: "mm".into(),
        notes: Vec::new(),
        warnings: Vec::new(),
    });
    let key = ModelKey::of(Path::new("."), &def, doc.job.tolerance);
    doc.models
        .insert("p".into(), ModelSlot::Ready(key, model, bvh));
    doc.job.models.push(def);
    doc.job.setups[0].models = vec!["p".into()];
    let setup = doc.job.setups[0].name.clone();
    (doc, setup)
}

/// Moving the work zero moves everything drawn, by as much the other way:
/// the layout says so, so the view can stay on the part.
#[test]
fn a_moved_work_zero_says_how_far_the_drawing_moved() {
    let (mut doc, setup) = with_plate();
    doc.job.setups[0].origin = Origin::ModelOrigin { offset: [0.0; 3] };
    assert_eq!(
        doc.refresh_layout(&setup),
        None,
        "the first layout moves nothing"
    );
    doc.job.setups[0].origin = Origin::ModelOrigin {
        offset: [1.0, 2.0, 5.0],
    };
    assert_eq!(
        doc.refresh_layout(&setup),
        Some(DVec3::new(-1.0, -2.0, -5.0))
    );
    assert_eq!(doc.refresh_layout(&setup), None, "nothing changed since");
    // Turned over, the picture changes altogether: nothing to keep.
    doc.job.setups[0].orientation = doc.job.setups[0].orientation.flipped();
    assert_eq!(doc.refresh_layout(&setup), None);
}

/// A work zero picked on something no longer there still draws the part, from
/// the model's own zero and without axes, so it can be picked again.
#[test]
fn a_lost_work_zero_still_shows_the_part() {
    use camcraft_core::job::{PointRef, Snap};
    let (mut doc, setup) = with_plate();
    doc.job.setups[0].origin = Origin::Point {
        point: PointRef {
            model: "p".into(),
            snap: Snap::Vertex,
            index: 0,
            at: [99.0, 99.0, 99.0],
        },
        offset: [0.0; 3],
    };
    doc.refresh_layout(&setup);
    let layout = doc.setups[&setup].layout.as_ref().unwrap();
    let p = layout.prepared.as_ref().unwrap();
    assert!(p.frame.origin.length() < 1e-12);
    assert!(!layout.outline.is_empty());
    assert!(layout.outline.iter().all(|l| l.kind == OutlineKind::Stock));
}

#[test]
fn a_new_job_is_one_setup_from_one_stock() {
    let doc = Document::untitled(1);
    assert_eq!(doc.title(), "Untitled");
    assert!(!doc.dirty());
    assert_eq!(doc.job.setups.len(), 1);
    assert_eq!(doc.job.setups[0].stocks, ["Stock 1"]);
    assert!(doc.job.check_common().is_ok());
}

#[test]
fn an_edit_is_unsaved_until_saved() {
    let mut doc = example();
    assert!(!doc.dirty());
    doc.job.tolerance = 0.1;
    assert!(doc.dirty());
    doc.job.tolerance = 0.05;
    assert!(!doc.dirty(), "back as it was is not a change");
}

/// Models added to a new job land in its setup, and save relative to the job.
#[test]
fn models_are_added_and_saved_relative() {
    let dir = std::env::temp_dir().join(format!("camcraft-doc-save-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("jobs")).unwrap();
    std::fs::create_dir_all(dir.join("models")).unwrap();
    let model = dir.join("models/part.step");
    std::fs::write(&model, "").unwrap();

    let mut doc = Document::untitled(1);
    let names = doc.add_models(std::slice::from_ref(&model));
    assert_eq!(names, ["part"]);
    assert_eq!(doc.job.setups[0].models, ["part"]);
    assert!(Path::new(&doc.job.models[0].path).is_absolute());

    doc.save_to(&dir.join("jobs/first")).unwrap();
    assert_eq!(
        doc.path.as_deref(),
        Some(dir.join("jobs/first.camcraft").as_path())
    );
    assert_eq!(doc.job.models[0].path, "../models/part.step");
    assert!(!doc.dirty());

    // Saved somewhere else, it still finds the model.
    std::fs::create_dir_all(dir.join("jobs/deeper")).unwrap();
    doc.save_to(&dir.join("jobs/deeper/second.camcraft"))
        .unwrap();
    assert_eq!(doc.job.models[0].path, "../../models/part.step");
    let again = camcraft_session::read_job(&dir.join("jobs/deeper/second.camcraft")).unwrap();
    assert_eq!(again, doc.job);
}

/// Saving drops tools no operation uses.
#[test]
fn unused_tools_are_not_saved() {
    let dir = std::env::temp_dir().join(format!("camcraft-doc-prune-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut doc = example();
    doc.job.tools.insert("spare".into(), ToolSpec::default());
    doc.save_to(&dir.join("pruned.camcraft")).unwrap();
    assert!(!doc.job.tools.contains_key("spare"));
    assert_eq!(doc.job.tools.len(), 2);
}

#[test]
fn renames_carry_every_reference() {
    let mut doc = example();
    doc.rename_model("sled_base", "sled").unwrap();
    assert_eq!(doc.job.setups[0].models, ["sled"]);
    doc.rename_stock("block", "blank").unwrap();
    assert_eq!(doc.job.setups[0].stocks, ["blank"]);
    doc.setups.insert("top".into(), SetupState::default());
    doc.selection = Selection::Operation("top".into(), 1);
    doc.rename_setup("top", "front").unwrap();
    assert!(doc.setups.contains_key("front"));
    assert_eq!(doc.active.as_deref(), Some("front"));
    assert_eq!(doc.selection, Selection::Operation("front".into(), 1));
    assert!(doc.rename_setup("front", " ").is_err());
    doc.job.check_common().unwrap();
    doc.job.check_setup(0).unwrap();
}

/// A stock's copy is the same block under a free name, after it, shown, and
/// used by nothing, so it can be changed or removed on its own.
#[test]
fn a_stock_is_duplicated() {
    let mut doc = example();
    let copy = doc.duplicate_stock("block").unwrap();
    assert_eq!(copy, "block copy");
    assert_eq!(doc.job.stocks[1].name, "block copy");
    assert_eq!(doc.job.stocks[1].spec, doc.job.stocks[0].spec);
    assert_eq!(doc.selection, Selection::Stock(copy.clone()));
    assert_eq!(doc.duplicate_stock("block").unwrap(), "block copy 2");
    doc.remove_stock(&copy).unwrap();
    assert!(doc.duplicate_stock("nothing").is_none());
}

/// While a setup runs, its operations wait, run and are done in turn as
/// word comes in; word from a run since superseded is dropped.
#[test]
fn operations_follow_their_run() {
    let mut doc = example();
    assert_eq!(doc.op_status("top", 0), OpStatus::NotRun);
    let seq = doc.started("top");
    assert_eq!(doc.op_status("top", 0), OpStatus::Waiting);
    doc.progressed("top", seq, Stage::Operation(0));
    assert!(matches!(doc.op_status("top", 0), OpStatus::Running(_)));
    assert_eq!(doc.op_status("top", 1), OpStatus::Waiting);
    doc.progressed("top", seq + 1, Stage::Operation(1));
    assert!(matches!(doc.op_status("top", 0), OpStatus::Running(_)));
    doc.progressed("top", seq, Stage::Operation(1));
    assert_eq!(doc.op_status("top", 0), OpStatus::Done);
    assert!(matches!(doc.op_status("top", 1), OpStatus::Running(_)));
    doc.progressed("top", seq, Stage::Checking);
    assert_eq!(doc.op_status("top", 1), OpStatus::Done);
    doc.finished("top", seq, Err(SessionError::Refused));
    assert_eq!(doc.op_status("top", 0), OpStatus::NotRun);
    assert!(doc.setups["top"].progress.is_none());
}

/// An operation dragged to a new place takes it, the others close up, and
/// the selection stays on the operation it was on.
#[test]
fn operations_are_reordered() {
    let mut doc = example();
    let names = |d: &Document| -> Vec<String> {
        d.job.setups[0]
            .operations
            .iter()
            .map(|o| o.name.clone())
            .collect()
    };
    doc.duplicate_operation("top", 1);
    assert_eq!(names(&doc), ["rough", "finish", "finish copy"]);
    doc.selection = Selection::Operation("top".into(), 1);
    doc.reorder_operation("top", 2, 0);
    assert_eq!(names(&doc), ["finish copy", "rough", "finish"]);
    assert_eq!(doc.selection, Selection::Operation("top".into(), 2));
    doc.reorder_operation("top", 0, 2);
    assert_eq!(names(&doc), ["rough", "finish", "finish copy"]);
    assert_eq!(doc.selection, Selection::Operation("top".into(), 1));
    doc.reorder_operation("top", 1, 1);
    doc.reorder_operation("top", 0, 9);
    assert_eq!(names(&doc), ["rough", "finish", "finish copy"]);
}

/// Selecting a setup shows all its toolpaths; selecting an operation, its
/// alone; and the choice is made once, not held against what Show says after.
#[test]
fn the_view_follows_the_selection() {
    let mut doc = example();
    let ops = |d: &Document| {
        let v = &d.setups["top"].view;
        (v.show_paths, v.view.ops)
    };
    doc.selection = Selection::Operation("top".into(), 1);
    doc.follow_selection();
    assert_eq!(ops(&doc), (true, 0b10));
    doc.setups.get_mut("top").unwrap().view.view.ops = 0b11;
    doc.follow_selection();
    assert_eq!(
        ops(&doc),
        (true, 0b11),
        "left alone until the selection changes"
    );
    doc.selection = Selection::Setup("top".into());
    doc.follow_selection();
    assert_eq!(ops(&doc), (true, u32::MAX));
    doc.selection = Selection::Operation("top".into(), 0);
    doc.follow_selection();
    assert_eq!(ops(&doc), (true, 0b01));
    doc.selection = Selection::Models;
    doc.follow_selection();
    assert_eq!(ops(&doc), (true, 0b01), "anything else leaves the view be");
}

/// An operation dragged into another setup leaves its own, takes its place
/// in the other under a name free there, and is shown there.
#[test]
fn an_operation_moves_to_another_setup() {
    let mut doc = example();
    let other = doc.duplicate_setup("top").unwrap();
    let names = |d: &Document, s: usize| -> Vec<String> {
        d.job.setups[s]
            .operations
            .iter()
            .map(|o| o.name.clone())
            .collect()
    };
    doc.move_operation_to("top", 0, &other, 1);
    assert_eq!(names(&doc, 0), ["finish"]);
    assert_eq!(names(&doc, 1), ["rough", "rough 2", "finish"]);
    assert_eq!(doc.selection, Selection::Operation(other.clone(), 1));
    assert_eq!(doc.active.as_deref(), Some(other.as_str()));
    // Past the end is the end; within a setup it is a reorder.
    doc.move_operation_to("top", 0, &other, 99);
    assert_eq!(names(&doc, 1), ["rough", "rough 2", "finish", "finish 2"]);
    assert!(names(&doc, 0).is_empty());
    doc.move_operation_to(&other, 0, &other, 99);
    assert_eq!(names(&doc, 1), ["rough 2", "finish", "finish 2", "rough"]);
}

/// After a run, generating one operation keeps the others that have not
/// changed and leaves out the ones that have; an operation turned off is
/// shown so and cannot be generated.
#[test]
fn one_operation_is_generated_alone() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let text = std::fs::read_to_string(dir.join("rail-jig.camcraft")).unwrap();
    // The small fixture, so the test runs quickly.
    let text = text.replace("sled_base.step", "hinge_jig.step");
    let job = camcraft_session::parse_job(&text, "rail-jig").unwrap();
    let mut doc = Document::opened(1, Some(dir.join("rail-jig.camcraft")), job);
    let seq = doc.started("top");
    let ran = crate::test_jobs::run_setup_zero(&doc.job, &dir);
    doc.finished("top", seq, Ok(ran));
    let kinds = |plans: &[Plan]| -> Vec<&'static str> {
        plans
            .iter()
            .map(|p| match p {
                Plan::Generate => "generate",
                Plan::Keep(_) => "keep",
                Plan::Skip => "skip",
            })
            .collect()
    };
    assert!(matches!(
        doc.op_status("top", 0),
        OpStatus::Done | OpStatus::Warnings
    ));
    assert_eq!(kinds(&doc.plans(0, Some(1))), ["keep", "generate"]);
    assert!(
        doc.plans(0, None).is_empty(),
        "the setup generates everything"
    );

    doc.job.setups[0].operations[0].passes.feed += 100.0;
    assert_eq!(doc.op_status("top", 0), OpStatus::Stale);
    assert!(matches!(
        doc.op_status("top", 1),
        OpStatus::Done | OpStatus::Warnings
    ));
    assert_eq!(kinds(&doc.plans(0, Some(1))), ["skip", "generate"]);
    assert_eq!(doc.status("top"), Status::Stale);

    doc.set_enabled("top", 0, false);
    assert_eq!(doc.op_status("top", 0), OpStatus::Disabled);
    assert!(doc.cannot_generate("top", Some(0)).is_some());
}

#[test]
fn what_is_in_use_cannot_be_removed() {
    let mut doc = example();
    assert!(doc.remove_model("sled_base").is_err());
    assert!(doc.remove_stock("block").is_err());
    doc.remove_setup("top");
    doc.remove_stock("block").unwrap();
    doc.remove_model("sled_base").unwrap();
    assert!(doc.job.models.is_empty());
}

#[test]
fn operations_are_added_moved_and_removed() {
    let mut doc = example();
    let i = doc
        .add_operation("top", StrategySpec::Parallel { angle: 90.0 })
        .unwrap();
    assert_eq!(i, 2);
    let op = &doc.job.setups[0].operations[2];
    assert_eq!(op.name, "finish 2");
    assert_eq!(
        op.tool.as_deref(),
        Some("finish-6-ball"),
        "the last tool, again"
    );
    doc.move_operation("top", 2, -1);
    assert_eq!(doc.job.setups[0].operations[1].name, "finish 2");
    assert_eq!(doc.selection, Selection::Operation("top".into(), 1));
    doc.move_operation("top", 0, -1);
    assert_eq!(
        doc.job.setups[0].operations[0].name, "rough",
        "nowhere to go"
    );
    doc.remove_operation("top", 1);
    assert_eq!(doc.job.setups[0].operations.len(), 2);
}

/// A library tool is copied in once: picking the same one again reuses it, and
/// a different one under a taken name gets a free name.
#[test]
fn tools_are_adopted_as_copies() {
    let mut doc = example();
    let same = doc.job.tools["finish-6-ball"].clone();
    assert_eq!(doc.adopt_tool("anything", &same), "finish-6-ball");
    let other = ToolSpec {
        diameter: 3.0,
        ..same
    };
    assert_eq!(doc.adopt_tool("finish-6-ball", &other), "finish-6-ball-2");
    assert_eq!(doc.job.tools.len(), 3);
    // Used by nothing yet, so no clash.
    doc.job.tools.get_mut("finish-6-ball-2").unwrap().number = 1;
    assert_eq!(doc.number_clash("finish-6-ball-2"), None);
    doc.job.setups[0].operations[1].tool = Some("finish-6-ball-2".into());
    assert_eq!(
        doc.number_clash("finish-6-ball-2").as_deref(),
        Some("rough-6-flat")
    );
}

/// Choosing a tool with a preset fills in the operation from it; choosing one
/// without keeps the operation's values and forgets the preset.
#[test]
fn a_tool_chosen_with_a_preset_fills_in_the_operation() {
    let mut doc = example();
    let mut spec = doc.job.tools["finish-6-ball"].clone();
    spec.diameter = 3.0;
    spec.presets = vec![Preset {
        name: "Finishing".into(),
        rpm: Some(20000),
        feed: Some(900.0),
        stepover: Some(0.2),
        ..Preset::default()
    }];
    let name = doc.adopt_tool("small-ball", &spec);
    let before = doc.job.setups[0].operations[1].clone();
    doc.use_tool("top", 1, name.clone(), Some("Finishing"));
    let op = &doc.job.setups[0].operations[1];
    assert_eq!(op.tool.as_deref(), Some(name.as_str()));
    assert_eq!(op.preset.as_deref(), Some("Finishing"));
    assert_eq!(op.passes.rpm, Some(20000));
    assert_eq!(op.passes.feed, 900.0);
    assert_eq!(op.passes.stepover, 0.2);
    assert_eq!(op.linking, before.linking, "the preset has no plunge feed");

    doc.use_tool("top", 1, "finish-6-ball".into(), None);
    let op = &doc.job.setups[0].operations[1];
    assert_eq!(op.preset, None);
    assert_eq!(op.passes.feed, 900.0, "values stay without a preset");
}

#[test]
fn undo_and_redo_step_through_finished_edits() {
    let mut doc = example();
    doc.job.tolerance = 0.1;
    doc.checkpoint();
    doc.add_setup();
    doc.checkpoint();
    assert_eq!(doc.job.setups.len(), 2);
    doc.undo();
    assert_eq!(doc.job.setups.len(), 1);
    assert_eq!(doc.job.tolerance, 0.1);
    doc.undo();
    assert_eq!(doc.job.tolerance, 0.05);
    assert!(!doc.dirty());
    doc.redo();
    doc.redo();
    assert_eq!(doc.job.setups.len(), 2);
    assert!(!doc.can_redo());
}

/// A run's reply lands only if it is the latest asked for, and its result goes
/// out of date when the setup changes.
#[test]
fn results_know_when_they_are_out_of_date() {
    let mut doc = example();
    let first = doc.started("top");
    let second = doc.started("top");
    assert_eq!(doc.status("top"), Status::Running);
    let failed = || Err(SessionError::Refused);
    doc.finished("top", first, failed());
    assert_eq!(
        doc.status("top"),
        Status::Running,
        "a superseded reply is dropped"
    );
    doc.finished("top", second, failed());
    assert_eq!(doc.status("top"), Status::NotRun);

    // A stand-in result, as a run would leave it.
    let inputs = SetupInputs::of(&doc.job, &doc.models, 0).unwrap();
    // Whether they are as they were, asked both ways: by making them again,
    // and in place, as the tree asks every frame. The two must agree.
    let same = |doc: &Document| {
        let made = SetupInputs::of(&doc.job, &doc.models, 0).as_ref() == Some(&inputs);
        assert_eq!(made, inputs.is_now(&doc.job, &doc.models, 0));
        made
    };
    assert!(same(&doc));
    doc.job.setups[0].operations[0].passes.stepover = 2.5;
    assert!(!same(&doc));
    doc.job.setups[0].operations[0].passes.stepover = 3.0;
    doc.job.tools.get_mut("rough-6-flat").unwrap().rpm = 12000;
    assert!(!same(&doc));
    doc.job.tools.get_mut("rough-6-flat").unwrap().rpm = 16000;
    // A tool no operation of this setup uses does not matter to it.
    doc.job.tools.insert("spare".into(), ToolSpec::default());
    assert!(same(&doc));
    // What decides where the part and stock sit matters, and so does a tool
    // an operation names going missing.
    doc.job.setups[0].wcs = 55;
    assert!(!same(&doc));
    doc.job.setups[0].wcs = 54;
    doc.job.tolerance = 0.1;
    assert!(!same(&doc));
    doc.job.tolerance = 0.05;
    doc.job.stocks[0].name = "other".into();
    assert!(!same(&doc));
    doc.job.stocks[0].name = "block".into();
    assert!(same(&doc));
    doc.job.tools.remove("rough-6-flat");
    assert!(!same(&doc));
}

#[test]
fn free_names_count_up() {
    assert_eq!(free("Setup", ["Setup 1"].into_iter()), "Setup 2");
    assert_eq!(free("Stock", std::iter::empty()), "Stock 1");
    assert_eq!(free("rough", ["rough"].into_iter()), "rough 2");
    assert_eq!(free("part", std::iter::empty()), "part");
}
