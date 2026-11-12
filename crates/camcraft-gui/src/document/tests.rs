//! The document's tests: editing, renaming, undo, what runs, and when a result
//! is out of date.

use super::run::SetupInputs;
use super::*;
use camcraft_core::control::Control;
use camcraft_core::job::{Preset, StrategySpec, ToolSpec};
use camcraft_core::run::{Plan, RunError, Stage};
use camcraft_session::Ran;
use glam::DVec3;

use crate::view::scene::{Playhead, Scene};
use crate::view::viewport::OutlineKind;

/// What the worker hands back for a run of `job`'s first setup, its models found
/// from `dir`: what the run made, with its toolpaths laid out for drawing.
/// Start a run of setup `name`, as the app does, generating everything.
fn start(doc: &mut Document, name: &str) -> u64 {
    let operations = doc.setup(name).map_or(0, |s| s.operations.len());
    doc.started(name, Control::new(operations), &[])
}

fn ran_zero(job: &Job, dir: &Path) -> Result<(Ran, Scene), SessionError> {
    let ran = crate::test_jobs::run_setup_zero(job, dir);
    let scene = Scene::build(job, &job.setups[0], &ran.result);
    Ok((ran, scene))
}

/// The example, cutting the small fixture so a test runs quickly, with its
/// models found from the folder returned.
fn quick_example() -> (Document, PathBuf) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let text = std::fs::read_to_string(dir.join("rail-jig.camcraft")).unwrap();
    let text = text.replace("sled_base.step", "hinge_jig.step");
    let job = camcraft_session::parse_job(&text, "rail-jig").unwrap();
    (
        Document::opened(1, Some(dir.join("rail-jig.camcraft")), job),
        dir,
    )
}

fn example() -> Document {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/rail-jig.camcraft");
    let job = camcraft_session::read_job(&path).unwrap();
    Document::opened(1, Some(path), job)
}

/// A new job whose setup machines a plate with a hole, loaded as if from disk.
pub(crate) fn with_plate() -> (Document, String) {
    let mut doc = Document::untitled(1);
    let def = ModelDef {
        name: "p".into(),
        path: "p.stl".into(),
        units: Some(LengthUnit::Mm),
        transform: Default::default(),
    };
    let body = camcraft_geom::topology::Body::derived(camcraft_geom::synthetic::plate_with_hole(
        20.0, 5.0, 4.0, 32,
    ));
    let bvh = Arc::new(MeshBvh::new(body.mesh()));
    let model = Arc::new(Model {
        bounds: body.mesh().bounds().unwrap(),
        file: Arc::new(body.clone()),
        body,
        path: "p.stl".into(),
        units: "mm".into(),
        notes: Vec::new(),
        warnings: Vec::new(),
    });
    let key = ModelKey::of(&doc.dir(), &def, doc.job.tolerance);
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

/// A layout's part's shadow is asked of a worker, once, and nothing here casts
/// it: the silhouette waits for it, and is then the one the engine works out.
/// Kept in the setup's turned frame, it serves a layout made again with the
/// work zero moved, or the stock changed, and a run, without casting it again;
/// one turned another way is cast afresh, and one that cannot be cast says
/// why, and is not asked for again.
#[test]
fn a_layouts_shadow_is_cast_off_the_window_once() {
    let (mut doc, setup) = with_plate();
    doc.refresh_layout(&setup);
    let wanted = doc.shadows_wanted();
    assert_eq!(wanted.len(), 1);
    let (key, prepared) = wanted.into_iter().next().unwrap();
    let layout = |doc: &Document| doc.setups[&setup].layout.clone().unwrap();
    assert_eq!(
        layout(&doc).silhouette(),
        Ok(None),
        "waits, and casts nothing"
    );
    assert!(prepared.cast_shadow().is_none());
    assert!(doc.turned_shadow(0).is_none());

    doc.casting(key);
    assert!(doc.shadows_wanted().is_empty(), "one at a time");
    // Laid out again meanwhile, from another work zero.
    doc.job.setups[0].origin = Origin::ModelOrigin {
        offset: [1.0, 2.5, 0.0],
    };
    doc.refresh_layout(&setup);
    assert!(doc.shadows_wanted().is_empty());

    // The worker casts it into the layout it was sent, the old one, and hands
    // it back turned: the new layout takes it, moved, and nothing more is cast.
    prepared.shadow().unwrap();
    doc.cast(key, Ok(Arc::new(prepared.turned_shadow().unwrap())));
    assert!(doc.shadows_wanted().is_empty(), "taken, not cast again");
    let laid = layout(&doc);
    let cast = laid.silhouette().unwrap().unwrap();
    let moved = laid.prepared.clone().unwrap();
    let fresh = camcraft_core::setup::features::silhouette(&moved.part).unwrap();
    assert!(cast.difference(&fresh).unwrap().area() < 1e-9);
    assert!(fresh.difference(cast).unwrap().area() < 1e-9);
    // The plate, less its hole.
    let hole = std::f64::consts::PI * 16.0;
    assert!(
        (cast.area() - (1600.0 - hole)).abs() < 5.0,
        "{}",
        cast.area()
    );
    // A run of it takes it too.
    assert!(doc.turned_shadow(0).is_some());

    // Another stock: laid out again, with the shadow at once.
    doc.job.stocks[0].spec = StockSpec::AroundModels {
        models: Vec::new(),
        margin: [5.0, 5.0, 1.0],
        below: 0.0,
    };
    doc.refresh_layout(&setup);
    assert!(doc.shadows_wanted().is_empty());
    assert!(layout(&doc).silhouette().unwrap().is_some());

    // Turned over, the part casts another shadow, asked for afresh; one that
    // cannot be cast says why, and is not asked for again.
    doc.job.setups[0].orientation = doc.job.setups[0].orientation.flipped();
    doc.refresh_layout(&setup);
    let (failing, _) = doc.shadows_wanted().into_iter().next().unwrap();
    doc.casting(failing);
    doc.cast(failing, Err("too tangled".into()));
    assert!(doc.shadows_wanted().is_empty());
    assert_eq!(
        layout(&doc).silhouette(),
        Err("the part's silhouette: too tangled".into())
    );
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

    // A machine profile that is a file moves with it, a built-in one does not,
    // and so do the edits there are to undo: undone, they still find their
    // files.
    std::fs::create_dir_all(dir.join("jobs/machines")).unwrap();
    std::fs::write(dir.join("jobs/machines/mine.toml"), "").unwrap();
    doc.job.setups[0].post = "../machines/mine.toml".into();
    doc.checkpoint();
    doc.job.setups[0].name = "renamed".into();
    doc.checkpoint();
    doc.save_to(&dir.join("jobs/third.camcraft")).unwrap();
    assert_eq!(doc.job.setups[0].post, "machines/mine.toml");
    doc.undo();
    assert_eq!(doc.job.setups[0].post, "machines/mine.toml");
    doc.undo();
    assert_eq!(doc.job.models[0].path, "../models/part.step");
    doc.job.setups[0].post = "masso".into();
    doc.save_to(&dir.join("jobs/deeper/fourth.camcraft"))
        .unwrap();
    assert_eq!(doc.job.setups[0].post, "masso");
}

/// A new job with its model loaded from a folder beside where it is then saved:
/// saving names the model from there ("../models/part.stl"), which is still the
/// file it was loaded from, so nothing is loaded or laid out again. It was, and
/// casting the part's shadow for the layout made again froze the window.
#[test]
fn saving_a_new_job_beside_its_model_loads_nothing_again() {
    let dir = std::env::temp_dir().join(format!("camcraft-doc-beside-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("jobs")).unwrap();
    std::fs::create_dir_all(dir.join("models")).unwrap();
    let file = dir.join("models/part.stl");
    std::fs::write(&file, "").unwrap();

    // The plate, as loaded from the file the dialog gave.
    let (mut doc, setup) = with_plate();
    doc.job.models[0].path = file.display().to_string();
    let key = ModelKey::of(&doc.dir(), &doc.job.models[0], doc.job.tolerance);
    let Some(ModelSlot::Ready(k, ..)) = doc.models.get_mut("p") else {
        panic!("the plate is loaded");
    };
    *k = key;
    assert!(doc.wanted_loads().is_empty());
    doc.refresh_layout(&setup);
    let laid = doc.setups[&setup].layout.as_ref().unwrap().key;

    doc.save_to(&dir.join("jobs/tray")).unwrap();
    assert_eq!(doc.job.models[0].path, "../models/part.stl");
    assert!(doc.wanted_loads().is_empty(), "{:?}", doc.wanted_loads());
    doc.refresh_layout(&setup);
    assert_eq!(
        doc.setups[&setup].layout.as_ref().unwrap().key,
        laid,
        "laid out as it was"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// However a model's file is named, from wherever, it is one key: with `..`
/// in it, and on Windows as `\\?\` names it and with the drive letter in the
/// other case.
#[test]
fn a_file_named_another_way_is_the_same_key() {
    let dir = std::env::temp_dir().join(format!("camcraft-doc-names-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("jobs")).unwrap();
    std::fs::create_dir_all(dir.join("models")).unwrap();
    let file = dir.join("models/part.step");
    std::fs::write(&file, "").unwrap();
    let key = |dir: &Path, path: String| {
        let def = ModelDef {
            name: "part".into(),
            path,
            units: None,
            transform: Default::default(),
        };
        ModelKey::of(dir, &def, 0.05)
    };
    let plain = key(&dir, file.display().to_string());
    let mut names = vec![
        key(&dir.join("jobs"), "../models/part.step".into()),
        key(
            &dir.join("jobs"),
            dir.join("jobs/../models/part.step").display().to_string(),
        ),
    ];
    if cfg!(windows) {
        let verbatim = file.canonicalize().unwrap().display().to_string();
        assert!(verbatim.starts_with(r"\\?\"), "{verbatim}");
        names.push(key(&dir, verbatim));
        let shown = file.display().to_string();
        let mut other_case = shown.clone();
        if let Some(d) = shown.get(..1) {
            let swapped = if d == d.to_lowercase() {
                d.to_uppercase()
            } else {
                d.to_lowercase()
            };
            other_case.replace_range(..1, &swapped);
        }
        names.push(key(&dir, other_case));
    }
    for k in &names {
        assert_eq!(*k, plain);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A setup's rename undone, or redone, keeps what the setup ran, and the
/// selection on it, as the rename itself does.
#[test]
fn a_rename_undone_keeps_the_setup_run() {
    let mut doc = example();
    let seq = start(&mut doc, "top");
    doc.checkpoint();
    doc.rename_setup("top", "side").unwrap();
    doc.checkpoint();
    doc.selection = Selection::Setup("side".into());
    doc.undo();
    assert_eq!(doc.job.setups[0].name, "top");
    assert!(
        doc.setups["top"]
            .running
            .as_ref()
            .is_some_and(|r| r.seq == seq)
    );
    assert_eq!(doc.selection, Selection::Setup("top".into()));
    doc.redo();
    assert!(doc.setups["side"].running.is_some());
    assert_eq!(doc.active.as_deref(), Some("side"));
}

/// Another tool keeps an offset chosen as zero: the channel round the part is
/// made for the first tool only.
#[test]
fn another_tool_keeps_a_chosen_offset() {
    use camcraft_core::job::{Boundary, Containment};
    let mut doc = example();
    let b = &mut doc.job.setups[0].operations[0].boundary;
    b.kind = Boundary::Silhouette;
    b.containment = Containment::Outside;
    b.margin = 0.0;
    let other = doc.job.setups[0].operations[1].tool.clone().unwrap();
    doc.use_tool("top", 0, other, None);
    assert_eq!(doc.job.setups[0].operations[0].boundary.margin, 0.0);
}

/// Room is made round the part by one rule, wherever it is asked: a boundary
/// round the part, reached into from outside, with no offset at all. An offset
/// in, or any other boundary, is one chosen.
#[test]
fn room_is_made_only_where_there_is_none() {
    use camcraft_core::job::{Boundary, Containment, MachiningBoundary};
    let mut b = MachiningBoundary {
        kind: Boundary::Silhouette,
        containment: Containment::Outside,
        margin: 0.0,
    };
    assert!(b.needs_room());
    b.kind = Boundary::BoundingBox;
    assert!(b.needs_room());
    b.margin = -0.5;
    assert!(!b.needs_room(), "an offset in is chosen");
    b.margin = 0.0;
    b.containment = Containment::Center;
    assert!(!b.needs_room());
    b.containment = Containment::Outside;
    b.kind = Boundary::Stock;
    assert!(!b.needs_room());
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

/// An operation is renamed as models, stocks and setups are: trimmed, and never
/// to nothing or to the name of another of its setup's operations.
#[test]
fn an_operation_is_renamed_as_anything_else_is() {
    let mut doc = example();
    doc.rename_operation("top", 1, "  finish 2 ").unwrap();
    assert_eq!(doc.job.setups[0].operations[1].name, "finish 2");
    assert_eq!(
        doc.rename_operation("top", 1, "rough"),
        Err("there is already an operation called 'rough'".into())
    );
    assert_eq!(
        doc.rename_operation("top", 1, " "),
        Err("an operation needs a name".into())
    );
    doc.rename_operation("top", 1, "finish 2").unwrap();
    assert!(doc.rename_operation("top", 9, "other").is_err());
    assert_eq!(doc.job.setups[0].operations[1].name, "finish 2");
    assert_eq!(
        doc.rename_model("sled_base", ""),
        Err("a model needs a name".into())
    );
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

/// A model duplicated is another of the same part beside it: the same file,
/// turned and scaled the same, moved along X by its width and a gap, machined
/// wherever the original is, and placed from the one loaded, not read again.
#[test]
fn a_model_is_duplicated_beside_itself() {
    use camcraft_core::job::Transform;
    let (mut doc, setup) = with_plate();
    doc.job.stocks[0].spec = StockSpec::AroundModels {
        models: vec!["p".into()],
        margin: [2.0; 3],
        below: 0.0,
    };
    doc.job.models[0].transform = Transform {
        rotation: [0.0, 0.0, 30.0],
        ..Transform::default()
    };
    let copy = doc.duplicate_model("p").unwrap();
    assert_eq!(copy, "p copy");
    assert_eq!(doc.selection, Selection::Model(copy.clone()));
    let (p, c) = (&doc.job.models[0], &doc.job.models[1]);
    assert_eq!(
        (c.name.as_str(), &c.path, c.units),
        ("p copy", &p.path, p.units)
    );
    assert_eq!(c.transform.rotation, p.transform.rotation);
    assert_eq!(
        c.transform.position,
        [50.0, 0.0, 0.0],
        "40 wide and 10 apart"
    );
    assert_eq!(doc.setup(&setup).unwrap().models, ["p", "p copy"]);
    assert!(matches!(&doc.job.stocks[0].spec,
        StockSpec::AroundModels { models, .. } if models == &["p", "p copy"]));
    // The original waits to be turned; the copy is placed from it.
    let wanted = doc.wanted_loads();
    assert!(
        wanted
            .iter()
            .any(|(def, _, how)| def.name == "p copy" && matches!(how, Wanted::Place(_))),
        "{wanted:?}"
    );
    assert_eq!(doc.duplicate_model("p").unwrap(), "p copy 2");
    assert!(doc.duplicate_model("nothing").is_none());
}

/// While a setup runs, each operation waits until its work starts, then says
/// what it is doing, working out its passes at once with the others or
/// cutting them in its turn; word from a run since superseded is dropped. A
/// run that fails leaves the operation it failed at marked, and the setup as
/// it is.
#[test]
fn operations_follow_their_run() {
    let mut doc = example();
    assert_eq!(doc.op_status("top", 0), OpStatus::NotRun);
    let seq = start(&mut doc, "top");
    assert_eq!(doc.op_status("top", 0), OpStatus::Waiting);
    doc.progressed(seq, Stage::Operation(1));
    assert!(matches!(
        doc.op_status("top", 1),
        OpStatus::Running {
            work: OpWork::Planning,
            ..
        }
    ));
    assert_eq!(doc.op_status("top", 0), OpStatus::Waiting);
    doc.progressed(seq + 1, Stage::Operation(0));
    assert_eq!(doc.op_status("top", 0), OpStatus::Waiting, "superseded");
    doc.progressed(seq, Stage::Operation(0));
    doc.progressed(seq, Stage::Cutting(0));
    assert!(matches!(
        doc.op_status("top", 0),
        OpStatus::Running {
            work: OpWork::Cutting,
            ..
        }
    ));
    assert_eq!(doc.status("top"), Status::Running);
    doc.finished(seq, Err(SessionError::Refused));
    assert!(doc.setups["top"].running.is_none());
    assert_eq!(doc.op_status("top", 0), OpStatus::Failed);
    assert_eq!(doc.status("top"), Status::Failed);
}

/// A setup renamed while it runs keeps its run: word from it, and its end, go
/// to the setup by its new name, and it is not left running for ever.
#[test]
fn a_setup_renamed_while_it_runs_keeps_its_run() {
    let mut doc = example();
    let seq = start(&mut doc, "top");
    doc.rename_setup("top", "side").unwrap();
    doc.progressed(seq, Stage::Operation(0));
    assert!(matches!(doc.op_status("side", 0), OpStatus::Running { .. }));
    doc.finished(seq, Err(SessionError::Refused));
    assert_ne!(doc.status("side"), Status::Running);
    assert!(doc.setups["side"].running.is_none());
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

/// A changed operation takes its toolpath with it, and every one after it, since
/// each cuts what the ones before left: the run is cut back to the operations
/// before it, still drawn, and the playhead comes back to their end. Changing
/// the first, or deleting it, forgets the run. What only goes to the post, or
/// a setup's name, keeps every path.
#[test]
fn a_changed_operation_takes_its_toolpath_with_it() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let text = std::fs::read_to_string(dir.join("rail-jig.camcraft")).unwrap();
    let text = text.replace("sled_base.step", "hinge_jig.step");
    let job = camcraft_session::parse_job(&text, "rail-jig").unwrap();
    let mut doc = Document::opened(1, Some(dir.join("rail-jig.camcraft")), job);
    let run = |doc: &mut Document| {
        let seq = start(doc, "top");
        let ran = ran_zero(&doc.job, &dir);
        doc.finished(seq, ran);
    };
    let ops = |doc: &Document| {
        doc.setups["top"]
            .ran
            .as_ref()
            .map_or(0, |r| r.ran.result.operations.len())
    };
    run(&mut doc);
    let whole = doc.setups["top"].ran.as_ref().unwrap().scene.segments.len();
    let state = doc.setups.get_mut("top").unwrap();
    state.view.at = Playhead::at(whole);
    state.view.transport.playing = true;

    // A new name, or posting only: every path stands.
    doc.rename_setup("top", "upper").unwrap();
    doc.invalidate_changed_operations();
    assert_eq!(ops_of(&doc, "upper"), 2, "a renamed setup cuts the same");
    doc.rename_setup("upper", "top").unwrap();
    doc.job.setups[0].wcs = 55;
    doc.invalidate_changed_operations();
    assert_eq!(ops(&doc), 2, "a work offset changes no path");
    doc.job.setups[0].wcs = 54;

    // The finish changed: the roughing stands, the finish is gone, and what
    // the timeline marks goes with it.
    doc.job.setups[0].operations[1].passes.feed += 100.0;
    doc.invalidate_changed_operations();
    assert_eq!(ops(&doc), 1);
    let state = &doc.setups["top"];
    let ran = state.ran.as_ref().unwrap();
    assert_eq!(
        ran.marks,
        crate::playback::transport::marks(&ran.ran, &ran.scene)
    );
    let left = ran.scene.segments.len();
    assert!(0 < left && left < whole, "{left} of {whole}");
    assert_eq!(
        state.view.at,
        Playhead::at(left),
        "back to the roughing's end"
    );
    assert!(!state.view.transport.playing);
    assert!(matches!(
        doc.op_status("top", 0),
        OpStatus::Done | OpStatus::Warnings
    ));
    assert_eq!(doc.op_status("top", 1), OpStatus::NotRun);
    assert_eq!(doc.status("top"), Status::Stale);
    // Asked again, nothing more goes.
    doc.invalidate_changed_operations();
    assert_eq!(ops(&doc), 1);

    // Deleting the finish keeps the roughing.
    run(&mut doc);
    doc.remove_operation("top", 1);
    doc.invalidate_changed_operations();
    assert_eq!(ops(&doc), 1, "the operation before a deleted one stands");

    // Changing the first leaves nothing.
    doc.job.setups[0].operations[0].passes.feed += 100.0;
    doc.invalidate_changed_operations();
    let state = &doc.setups["top"];
    assert!(state.ran.is_none() && state.live.is_none());
    assert_eq!(state.view.at, Playhead::default());
    assert_eq!(doc.status("top"), Status::NotRun);
    assert_eq!(doc.op_status("top", 0), OpStatus::NotRun);

    // An undo that takes away an operation keeps what was before it.
    doc.checkpoint();
    doc.duplicate_operation("top", 0);
    doc.checkpoint();
    run(&mut doc);
    doc.undo();
    doc.invalidate_changed_operations();
    assert_eq!(ops(&doc), 1, "undoing the copy leaves what was before it");
}

/// Setting up draws the selection's toolpaths in full over the part and the
/// stock's outline; playing back, the material and the cutter as far as the
/// playhead, the toolpaths only when asked for; and with nothing to play back,
/// the view goes back to setting up.
#[test]
fn the_view_sets_up_or_plays_back() {
    use crate::playback::{
        cannot_play_back, content, playing_back, set_mode, settle_mode, showing_material,
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let text = std::fs::read_to_string(dir.join("rail-jig.camcraft")).unwrap();
    let text = text.replace("sled_base.step", "hinge_jig.step");
    let job = camcraft_session::parse_job(&text, "rail-jig").unwrap();
    let mut doc = Document::opened(1, Some(dir.join("rail-jig.camcraft")), job);
    assert!(cannot_play_back(doc.setups.entry("top".into()).or_default()).is_some());
    let seq = start(&mut doc, "top");
    let ran = ran_zero(&doc.job, &dir);
    doc.finished(seq, ran);
    let whole = doc.setups["top"].ran.as_ref().unwrap().scene.segments.len();

    let state = doc.setups.get_mut("top").unwrap();
    assert_eq!(
        state.view.at,
        Playhead::default(),
        "a new run plays back from its first move, the stock uncut"
    );
    state.view.at = Playhead::at(3);
    let setup = Selection::Setup("top".into());
    let c = content(state, "top", &setup);
    assert!(c.stock.is_none() && c.view.cutter.is_none());
    assert!(c.view.part && c.view.stock && c.view.roles != 0);
    assert_eq!(
        (c.view.shown, c.view.ops),
        (whole, u32::MAX),
        "all, in full"
    );
    let finish = Selection::Operation("top".into(), 1);
    assert_eq!(
        content(state, "top", &finish).view.ops,
        0b10,
        "the one selected"
    );
    let other = Selection::Operation("bottom".into(), 0);
    assert_eq!(content(state, "top", &other).view.ops, u32::MAX);

    // The material asked for while setting up: in place of the part, its
    // stock's outline and work zero.
    state.view.material = true;
    assert!(showing_material(state) && !playing_back(state));
    let c = content(state, "top", &finish);
    assert!(!c.view.part && !c.view.stock && !c.view.origin);
    assert!(c.view.cutter.is_none());
    assert_eq!(c.view.roles, 0, "no toolpaths over it until asked for");
    state.view.looks.of_mut(Look::CutStock).toolpaths = true;
    let c = content(state, "top", &finish);
    assert!(c.view.roles != 0 && c.view.ops == 0b10, "the selection's");
    state.view.material = false;

    assert!(cannot_play_back(state).is_none());
    set_mode(state, ViewMode::Playback);
    assert!(playing_back(state));
    let c = content(state, "top", &finish);
    assert_eq!(c.view.shown, 3, "as far as the playhead");
    assert_eq!(
        c.view.ops,
        u32::MAX,
        "every operation, whatever is selected"
    );
    assert!(c.view.cutter.is_some());
    assert!(!c.view.part && c.view.roles == 0, "the material alone");
    state.view.looks.of_mut(Look::Playback).toolpaths = true;
    assert_ne!(content(state, "top", &finish).view.roles, 0);

    state.view.transport.playing = true;
    set_mode(state, ViewMode::Setup);
    assert!(!state.view.transport.playing, "setting up pauses");
    set_mode(state, ViewMode::Playback);

    // The roughing changed: nothing is left to play back.
    doc.job.setups[0].operations[0].passes.feed += 100.0;
    doc.invalidate_changed_operations();
    let state = doc.setups.get_mut("top").unwrap();
    assert!(!playing_back(state) && cannot_play_back(state).is_some());
    settle_mode(state);
    assert_eq!(state.view.mode, ViewMode::Setup);
}

fn ops_of(doc: &Document, setup: &str) -> usize {
    doc.setups[setup]
        .ran
        .as_ref()
        .map_or(0, |r| r.ran.result.operations.len())
}

/// After a run, generating one operation keeps the others that have not
/// changed and leaves out the ones that have; one after a changed one, itself
/// unchanged, links its passes again; an operation turned off is shown so and
/// cannot be generated.
#[test]
fn one_operation_is_generated_alone() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let text = std::fs::read_to_string(dir.join("rail-jig.camcraft")).unwrap();
    // The small fixture, so the test runs quickly.
    let text = text.replace("sled_base.step", "hinge_jig.step");
    let job = camcraft_session::parse_job(&text, "rail-jig").unwrap();
    let mut doc = Document::opened(1, Some(dir.join("rail-jig.camcraft")), job);
    let seq = start(&mut doc, "top");
    let ran = ran_zero(&doc.job, &dir);
    doc.finished(seq, ran);
    let kinds = |plans: &[Plan]| -> Vec<&'static str> {
        plans
            .iter()
            .map(|p| match p {
                Plan::Generate => "generate",
                Plan::Relink(_) => "relink",
                Plan::Keep(_) => "keep",
                Plan::Skip => "skip",
            })
            .collect()
    };
    assert!(matches!(
        doc.op_status("top", 0),
        OpStatus::Done | OpStatus::Warnings
    ));
    // Generated alone, unchanged, from the passes it found.
    assert_eq!(kinds(&doc.plans(0, Some(1))), ["keep", "relink"]);
    assert_eq!(
        kinds(&doc.plans(0, None)),
        ["keep", "keep"],
        "the setup keeps what stands"
    );
    // With the material each left, to put back rather than cut again.
    assert!(
        doc.plans(0, None)
            .iter()
            .all(|p| matches!(p, Plan::Keep(k) if k.after.is_some()))
    );

    doc.job.setups[0].operations[0].passes.feed += 100.0;
    assert_eq!(doc.op_status("top", 0), OpStatus::Stale);
    assert!(matches!(
        doc.op_status("top", 1),
        OpStatus::Done | OpStatus::Warnings
    ));
    assert_eq!(kinds(&doc.plans(0, Some(1))), ["skip", "relink"]);
    assert_eq!(
        kinds(&doc.plans(0, None)),
        ["generate", "relink"],
        "what comes after a changed operation cuts what it leaves, from the          passes it found from the part"
    );
    assert_eq!(doc.status("top"), Status::Stale);

    // The run cut back to what stands keeps the passes all the same: with the
    // change undone, the roughing's are found again too.
    doc.invalidate_changed_operations();
    assert_eq!(kinds(&doc.plans(0, None)), ["generate", "relink"]);
    doc.job.setups[0].operations[0].passes.feed -= 100.0;
    assert_eq!(kinds(&doc.plans(0, None)), ["relink", "relink"]);
    // The finish changed: its passes are not the ones kept.
    doc.job.setups[0].operations[1].passes.stepover += 0.5;
    assert_eq!(kinds(&doc.plans(0, None)), ["relink", "generate"]);
    doc.job.setups[0].operations[1].passes.stepover -= 0.5;
    // Nor are they where the part is placed otherwise.
    doc.job.tolerance *= 0.5;
    assert_eq!(kinds(&doc.plans(0, None)), ["generate", "generate"]);
    doc.job.tolerance *= 2.0;

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

/// A new operation starts on the part's silhouette: finishing with the tool's
/// centre inside it, roughing reaching in from outside with a channel a tool wide,
/// set once the tool is known. A 2D one cuts round the silhouette.
#[test]
fn new_operations_start_on_the_silhouette() {
    use camcraft_core::job::{Boundary, Containment, Outline};
    let mut doc = example();
    let finish = doc
        .add_operation("top", StrategySpec::Parallel { angle: 0.0 })
        .unwrap();
    let b = &doc.job.setups[0].operations[finish].boundary;
    assert_eq!(b.kind, Boundary::Silhouette);
    assert_eq!(b.containment, Containment::Center);
    assert_eq!(b.margin, 0.0);

    let rough = doc
        .add_operation(
            "top",
            StrategySpec::PocketClearing {
                min_ring_length: 3.0,
                conventional: false,
                flat_areas: true,
            },
        )
        .unwrap();
    let tool = doc.job.setups[0].operations[rough].tool.clone().unwrap();
    let diameter = doc.job.tools[&tool].diameter;
    let b = &doc.job.setups[0].operations[rough].boundary;
    assert_eq!(b.kind, Boundary::Silhouette);
    assert_eq!(b.containment, Containment::Outside);
    assert_eq!(b.margin, diameter);

    let profile = doc
        .add_operation(
            "top",
            StrategySpec::Profile {
                side: Default::default(),
                conventional: false,
            },
        )
        .unwrap();
    let o = &doc.job.setups[0].operations[profile];
    assert_eq!(o.outline, Outline::Silhouette);
    assert_eq!(o.boundary, Default::default());
    // And arcs on and off each ring.
    assert!(
        doc.job.setups[0].operations[profile]
            .linking
            .leads
            .is_some()
    );

    // A 2D operation has no additional offset, so a tool never gives it one,
    // even with a boundary round the part that reaches in from outside.
    let o = &mut doc.job.setups[0].operations[profile];
    o.boundary.kind = Boundary::Silhouette;
    o.boundary.containment = Containment::Outside;
    o.tool = None;
    doc.use_tool("top", profile, tool.clone(), None);
    assert_eq!(doc.job.setups[0].operations[profile].boundary.margin, 0.0);

    // Facing starts both ways at the bottom alone, and with a flat end mill
    // steps over 0.7 of its width, its passes running half a tool and the
    // tolerance past the stock, as Fusion's since 2022.
    let [(_, two_d), ..] = crate::browser::operation_groups();
    let (_, face) = two_d[0][0];
    let face = doc.add_operation("top", face).unwrap();
    doc.use_tool("top", face, "rough-6-flat".into(), None);
    let o = &doc.job.setups[0].operations[face];
    assert_eq!(o.strategy.name(), "face");
    assert_eq!(o.stepdown(), None);
    let d = doc.job.tools["rough-6-flat"].diameter;
    assert!(
        (o.passes.stepover - 0.7 * d).abs() < 1e-9,
        "{}",
        o.passes.stepover
    );
    let StrategySpec::Face { pass_extension, .. } = o.strategy else {
        unreachable!("a face is a face")
    };
    let pass_extension = pass_extension.unwrap();
    assert!((pass_extension - (d / 2.0 + doc.job.tolerance)).abs() < 1e-9);
}

/// A new roughing operation, a 2D pocket among them, leaves half a millimetre
/// for the finish, as Fusion's does; a new finish or profile cuts to size.
#[test]
fn new_roughing_leaves_stock_for_the_finish() {
    use camcraft_core::job::StockToLeave;
    let mut doc = example();
    let rough = doc
        .add_operation(
            "top",
            StrategySpec::PocketClearing {
                min_ring_length: 3.0,
                conventional: false,
                flat_areas: true,
            },
        )
        .unwrap();
    assert_eq!(
        doc.job.setups[0].operations[rough].passes.stock_to_leave,
        StockToLeave {
            radial: 0.5,
            axial: 0.5
        }
    );
    let pocket = doc
        .add_operation(
            "top",
            StrategySpec::Pocket {
                conventional: false,
            },
        )
        .unwrap();
    assert_eq!(
        doc.job.setups[0].operations[pocket].passes.stock_to_leave,
        StockToLeave {
            radial: 0.5,
            axial: 0.5
        }
    );
    let profile = doc
        .add_operation(
            "top",
            StrategySpec::Profile {
                side: Default::default(),
                conventional: false,
            },
        )
        .unwrap();
    assert_eq!(
        doc.job.setups[0].operations[profile].passes.stock_to_leave,
        StockToLeave::default()
    );
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
    assert!(doc.redo_name().is_none());
}

/// A tool's edit and what its operations' expressions make of it are one
/// step, named for the tool: undone, both are back and stay back; redone,
/// both come again.
#[test]
fn what_an_edit_works_out_is_undone_with_it() {
    let (mut doc, setup) = with_plate();
    doc.job.tools.insert(
        "t".into(),
        ToolSpec {
            diameter: 6.0,
            ..ToolSpec::default()
        },
    );
    let i = doc
        .add_operation(&setup, StrategySpec::named("adaptive").unwrap())
        .unwrap();
    doc.use_tool(&setup, i, "t".into(), None);
    doc.checkpoint();
    let stepover = |doc: &Document| doc.operation(&setup, i).unwrap().passes.stepover;
    assert!((stepover(&doc) - 2.4).abs() < 1e-9);

    doc.job.tools.get_mut("t").unwrap().diameter = 10.0;
    doc.checkpoint();
    assert!((stepover(&doc) - 4.0).abs() < 1e-9, "worked out with it");
    assert_eq!(doc.undo_name().as_deref(), Some("diameter of tool t"));
    doc.undo();
    // The next frame works the expressions out again, and changes nothing.
    doc.checkpoint();
    assert_eq!(doc.job.tools["t"].diameter, 6.0);
    assert!((stepover(&doc) - 2.4).abs() < 1e-9);
    assert_eq!(doc.redo_name().as_deref(), Some("diameter of tool t"));
    doc.redo();
    assert!((stepover(&doc) - 4.0).abs() < 1e-9);
}

/// Undoing a setting shows where it was: its operation selected, and the
/// setting given for its tab.
#[test]
fn undoing_shows_where_the_edit_was() {
    let (mut doc, setup) = with_plate();
    let i = doc
        .add_operation(&setup, StrategySpec::named("adaptive").unwrap())
        .unwrap();
    doc.checkpoint();
    doc.operation_mut(&setup, i).unwrap().linking.ramp_angle = 4.0;
    doc.checkpoint();
    doc.selection = Selection::Job;
    assert_eq!(doc.undo().as_deref(), Some("linking.ramp_angle"));
    assert_eq!(doc.selection, Selection::Operation(setup.clone(), i));
    assert_eq!(doc.redo().as_deref(), Some("linking.ramp_angle"));
}

/// A run's reply lands only if it is the latest asked for, and its result goes
/// out of date when the setup changes.
#[test]
fn results_know_when_they_are_out_of_date() {
    let mut doc = example();
    let first = start(&mut doc, "top");
    let second = start(&mut doc, "top");
    assert_eq!(doc.status("top"), Status::Running);
    let failed = || Err(SessionError::Refused);
    doc.finished(first, failed());
    assert_eq!(
        doc.status("top"),
        Status::Running,
        "a superseded reply is dropped"
    );
    doc.finished(second, failed());
    assert_eq!(
        doc.status("top"),
        Status::Failed,
        "a failed run leaves the setup as it is until it changes"
    );

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

/// A model moved is placed again from the one loaded, not read again, and shown
/// where it was until it is; one placement at a time, the latest winning, and
/// nothing generated until the model is where the job says.
#[test]
fn a_moved_model_is_placed_again_not_read() {
    use camcraft_core::job::Transform;
    let (mut doc, setup) = with_plate();
    doc.job.tools.insert("t".into(), ToolSpec::default());
    let op = doc
        .add_operation(&setup, StrategySpec::Parallel { angle: 0.0 })
        .unwrap();
    doc.job.setups[0].operations[op].tool = Some("t".into());
    assert_eq!(doc.cannot_generate(&setup, None), None);
    assert!(doc.wanted_loads().is_empty());
    doc.job.models[0].transform = Transform {
        position: [50.0, 0.0, 0.0],
        ..Transform::default()
    };
    let wanted = doc.wanted_loads();
    let [(def, key, Wanted::Place(model))] = wanted.as_slice() else {
        panic!("expected a placement, got {wanted:?}");
    };
    doc.placing(&def.name, key.clone());
    assert!(
        doc.models["p"].model().is_some(),
        "still shown where it was"
    );
    assert!(
        doc.cannot_generate(&setup, None)
            .is_some_and(|why| why.contains("being placed")),
        "{:?}",
        doc.cannot_generate(&setup, None)
    );
    doc.job.models[0].transform.position[1] = 10.0;
    assert!(doc.wanted_loads().is_empty(), "one placement at a time");

    let placed = model.placed(&def.transform).unwrap();
    let bvh = Arc::new(MeshBvh::new(placed.mesh()));
    let before = doc.log.len();
    doc.loaded("p", key, Ok((Arc::new(placed), bvh)));
    let bounds = doc.models["p"].model().unwrap().bounds;
    assert!((bounds.min.x - 30.0).abs() < 1e-9, "{bounds:?}");
    assert_eq!(doc.log.len(), before, "nothing new to say of its file");
    // The later move is placed next.
    assert!(matches!(
        doc.wanted_loads().as_slice(),
        [(_, _, Wanted::Place(_))]
    ));
    // Another file is read.
    doc.job.models[0].path = "q.stl".into();
    assert!(matches!(
        doc.wanted_loads().as_slice(),
        [(_, _, Wanted::Read)]
    ));
}

/// A run cut back to its first operations keeps the scene a run of them alone
/// would have drawn: the same segments at the same times, the same steep
/// descents in the same order, and the same counts.
#[test]
fn a_scene_cut_back_is_the_one_built_for_what_is_left() {
    let (doc, dir) = quick_example();
    let job = &doc.job;
    let (ran, scene) = ran_zero(job, &dir).unwrap();
    let n = ran.result.operations.len();
    assert!(n >= 2 && scene.ops.iter().all(|o| o.count > 0));
    for kept in 0..=n {
        let mut cut = scene.clone();
        super::setup_state::keep_first(&mut cut, kept);
        let mut result = ran.result.clone();
        result.operations.truncate(kept);
        let built = Scene::build(job, &job.setups[0], &result);
        assert_eq!(cut.segments, built.segments, "{kept} kept");
        assert_eq!(cut.steep, built.steep, "{kept} kept");
        assert_eq!(cut.role_counts, built.role_counts, "{kept} kept");
        assert_eq!(cut.ops.len(), built.ops.len(), "{kept} kept");
        for (a, b) in cut.ops.iter().zip(&built.ops) {
            assert_eq!((&a.name, a.first, a.count), (&b.name, b.first, b.count));
            assert_eq!(a.cutter, b.cutter);
        }
    }
}

/// An operation's defaults are what it would be new with its tool and the
/// preset it names: one just made is all defaults, an edit shows against
/// them, and they stay put while it is edited; another preset is another
/// default.
#[test]
fn an_operations_defaults_are_a_new_one_with_its_tool() {
    let (mut doc, setup) = with_plate();
    let preset = |name: &str, feed: f64| Preset {
        name: name.into(),
        feed: Some(feed),
        ..Preset::default()
    };
    doc.job.tools.insert(
        "quarter".into(),
        ToolSpec {
            diameter: 6.35,
            presets: vec![preset("wood", 2500.0), preset("acrylic", 900.0)],
            ..ToolSpec::default()
        },
    );
    let i = doc
        .add_operation(&setup, StrategySpec::named("adaptive").unwrap())
        .unwrap();
    doc.use_tool(&setup, i, "quarter".into(), Some("wood"));
    let op = |doc: &Document| doc.operation(&setup, i).unwrap().clone();
    let def = doc.operation_defaults(&setup, i).unwrap();
    assert_eq!(op(&doc).passes, def.passes, "new, it is all defaults");
    assert_eq!(op(&doc).linking, def.linking);
    assert_eq!(def.passes.feed, 2500.0, "the preset's feed");

    doc.operation_mut(&setup, i).unwrap().passes.stepover = 9.0;
    let again = doc.operation_defaults(&setup, i).unwrap();
    assert_eq!(again.passes, def.passes, "an edit is not a default");
    assert_ne!(op(&doc).passes.stepover, again.passes.stepover);

    doc.use_tool(&setup, i, "quarter".into(), Some("acrylic"));
    assert_eq!(
        doc.operation_defaults(&setup, i).unwrap().passes.feed,
        900.0
    );
}

/// A setup that can be generated and has not been wants a run, until it is
/// running; stopped, it waits as it is until it changes; and one whose run
/// failed is not tried again until it changes either.
#[test]
fn a_setup_wants_generating_until_it_runs_or_is_left_as_it_is() {
    let (mut doc, setup) = with_plate();
    doc.job.tools.insert("t".into(), ToolSpec::default());
    let op = doc
        .add_operation(&setup, StrategySpec::Parallel { angle: 0.0 })
        .unwrap();
    doc.job.setups[0].operations[op].tool = Some("t".into());
    assert_eq!(doc.wanting_runs(), vec![setup.clone()]);

    let seq = start(&mut doc, &setup);
    assert!(doc.wanting_runs().is_empty(), "running");
    doc.stop_generating(&setup);
    let running = doc.setups[&setup].running.as_ref().unwrap();
    assert_eq!(running.control.stopped_from(), Some(0));
    doc.finished(seq, Err(SessionError::Run(RunError::Stopped)));
    assert_eq!(doc.status(&setup), Status::Stopped);
    assert!(doc.wanting_runs().is_empty(), "stopped where it stands");
    doc.job.setups[0].operations[op].passes.feed += 1.0;
    assert_eq!(doc.wanting_runs(), vec![setup.clone()], "changed since");

    let seq = start(&mut doc, &setup);
    doc.finished(seq, Err(SessionError::Refused));
    assert_eq!(doc.status(&setup), Status::Failed);
    assert!(doc.wanting_runs().is_empty(), "failed where it stands");
    doc.resume_generating(&setup);
    assert_eq!(doc.wanting_runs(), vec![setup], "asked for again");
}

/// A run in flight is stopped from the first of its operations changed since
/// it was sent, the ones before it going on; from the first of all when what
/// shapes every cut changes; and not at all for an operation added after them.
#[test]
fn a_run_is_stopped_from_what_changed() {
    let mut doc = example();
    let _ = start(&mut doc, "top");
    let stopped = |doc: &Document| {
        doc.setups["top"]
            .running
            .as_ref()
            .unwrap()
            .control
            .stopped_from()
    };
    doc.stop_changed_runs();
    assert_eq!(stopped(&doc), None);
    doc.duplicate_operation("top", 1);
    doc.stop_changed_runs();
    assert_eq!(stopped(&doc), None, "added after them all");
    doc.job.setups[0].operations[1].passes.feed += 1.0;
    doc.stop_changed_runs();
    assert_eq!(stopped(&doc), Some(1));
    doc.job.tolerance *= 2.0;
    doc.stop_changed_runs();
    assert_eq!(stopped(&doc), Some(0));
}

/// Editing settles once the job has stayed as it is a moment, or longer while
/// a field is still being typed in; until then it says how long is left, for
/// a frame to be drawn then, though nothing else moves.
#[test]
fn editing_settles_after_a_moment_as_it_is() {
    use std::time::{Duration, Instant};
    let mut doc = example();
    let t = Instant::now() + Duration::from_secs(10);
    let ms = Duration::from_millis;
    assert!(
        doc.until_settled(t, false).is_zero(),
        "nothing changed since it opened"
    );
    doc.job.tolerance *= 2.0;
    assert_eq!(doc.until_settled(t, false), ms(300), "just changed");
    assert_eq!(doc.until_settled(t + ms(100), false), ms(200));
    assert!(doc.until_settled(t + ms(400), false).is_zero());
    assert!(
        !doc.until_settled(t + ms(400), true).is_zero(),
        "still typing"
    );
    assert!(doc.until_settled(t + ms(1600), true).is_zero());
}

/// What a run has made so far shows as it is made: the operations made are
/// done, the rest still under way, and the setup still running; the
/// playhead of a setup shown before stays where it was.
#[test]
fn what_a_run_has_made_so_far_shows_at_once() {
    let (mut doc, dir) = quick_example();
    let seq = start(&mut doc, "top");
    let (ran, scene) = ran_zero(&doc.job, &dir).unwrap();
    doc.finished(seq, Ok((ran.clone(), scene.clone())));
    doc.setups.get_mut("top").unwrap().view.at = Playhead::at(3);

    let seq = start(&mut doc, "top");
    let mut first = ran.clone();
    first.result.operations.truncate(1);
    first.result.keyframes.truncate(2);
    first.finished = false;
    let scene = Scene::build(&doc.job, &doc.job.setups[0], &first.result);
    doc.so_far(seq, first, scene);
    assert_eq!(doc.status("top"), Status::Running);
    assert!(matches!(
        doc.op_status("top", 0),
        OpStatus::Done | OpStatus::Warnings
    ));
    assert_eq!(doc.op_status("top", 1), OpStatus::Waiting);
    let state = &doc.setups["top"];
    assert_eq!(state.ran.as_ref().unwrap().ran.result.operations.len(), 1);
    assert_eq!(state.view.at, Playhead::at(3), "kept where it was");
}

/// What an operation's work shows of its passes as it finds them is drawn,
/// no more often than a preview is made again; and once the operation is
/// made, its preview goes, since what it made is drawn instead.
#[test]
fn passes_show_as_they_are_found_until_their_operation_is_made() {
    use std::time::{Duration, Instant};
    let (mut doc, dir) = quick_example();
    let operations = doc.setup("top").map_or(0, |s| s.operations.len());
    let control = Control::new(operations);
    let seq = doc.started("top", control.clone(), &[]);
    let line = |y: f64| vec![DVec3::new(0.0, y, 0.0), DVec3::new(10.0, y, 0.0)];
    control.watch(1).show([line(0.0), line(1.0)]);
    let preview = |doc: &Document| {
        let r = doc.setups["top"].running.as_ref().unwrap();
        (r.preview.lines.len(), r.preview_key)
    };
    let (none, key) = preview(&doc);
    assert_eq!(none, 0);
    let later = Instant::now() + Duration::from_secs(1);
    doc.take_previews(later);
    let (lines, shown) = preview(&doc);
    assert!(lines >= 2, "{lines}");
    assert_ne!(shown, key, "drawn again");
    // Not made again before its time, whatever has come since.
    control.watch(1).show([line(2.0)]);
    doc.take_previews(later + Duration::from_millis(10));
    assert_eq!(preview(&doc).0, lines);
    doc.take_previews(later + Duration::from_secs(1));
    assert!(preview(&doc).0 > lines);

    // Both operations made: nothing is left to preview.
    let (ran, scene) = ran_zero(&doc.job, &dir).unwrap();
    let mut both = ran;
    both.finished = false;
    doc.so_far(seq, both, scene);
    assert_eq!(preview(&doc).0, 0);
}
