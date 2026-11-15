//! A job's sheets arranged live: what an arrangement is kept by, nothing
//! generated or laid out while one is being worked out, and a model put on a
//! sheet losing its position with the edit.

use std::collections::BTreeMap;

use camcraft_core::arrange::{Outcome, arrange};
use camcraft_core::job::{Arranged, SheetPart, StockSpec, StrategySpec, ToolSpec, Transform};
use camcraft_geom::topology::Body;

use super::run::SetupInputs;
use super::*;

/// The plate, 40 square, on a sheet 200 by 100, its setup runnable.
fn on_a_sheet() -> (Document, String) {
    let (mut doc, setup) = with_plate();
    let mut sheet = Arranged::new([200.0, 100.0, 18.0]);
    sheet.corner = [500.0, 0.0, 0.0];
    sheet.parts = vec![SheetPart::of("p")];
    doc.job.stocks[0].spec = StockSpec::Arranged(sheet);
    doc.job.tools.insert("t".into(), ToolSpec::default());
    let op = doc
        .add_operation(&setup, StrategySpec::Parallel { angle: 0.0 })
        .unwrap();
    doc.job.setups[0].operations[op].tool = Some("t".into());
    doc.checkpoint();
    (doc, setup)
}

fn sheet(doc: &mut Document) -> &mut Arranged {
    let StockSpec::Arranged(a) = &mut doc.job.stocks[0].spec else {
        panic!("not a sheet");
    };
    a
}

/// Work out the arrangement the job wants, as the worker does, and hand it
/// back.
fn arrange_now(doc: &mut Document) {
    let a = doc.arrange_wanted().unwrap();
    let bodies: BTreeMap<&str, &Body> = a
        .models
        .iter()
        .map(|(n, m)| (n.as_str(), &m.body))
        .collect();
    let mut outlines = a.outlines;
    let arranged = arrange(&a.job, &bodies, &mut outlines, &a.control.watch(0));
    doc.arranged(a.seq, a.key, arranged, outlines);
}

/// An arrangement is kept by each sheet's settings and each part as loaded:
/// a sheet's setting or a part placed again by its own rotation makes
/// another; what no sheet depends on, such as an operation, does not.
#[test]
fn an_arrangement_is_kept_by_what_it_is_laid_from() {
    let (mut doc, _) = on_a_sheet();
    let first = doc.arrange_key().unwrap();
    doc.job.setups[0].operations[0].passes.stepover = 7.0;
    assert_eq!(doc.arrange_key().unwrap(), first);
    sheet(&mut doc).spacing = 3.0;
    let spaced = doc.arrange_key().unwrap();
    assert_ne!(spaced, first);
    sheet(&mut doc).spacing = 10.0;
    assert_eq!(doc.arrange_key().unwrap(), first);

    // Turned, it is laid again once placed again, as the body it is laid by
    // is the one loaded.
    doc.job.models[0].transform.rotation = [0.0, 0.0, 30.0];
    assert_eq!(doc.arrange_key().unwrap(), first, "not placed yet");
    let key = ModelKey::of(&doc.dir(), &doc.job.models[0], doc.job.tolerance);
    let model = doc.models["p"]
        .model()
        .unwrap()
        .placed(&doc.job.models[0].transform)
        .unwrap();
    let bvh = Arc::new(MeshBvh::new(model.mesh()));
    doc.placing("p", key.clone());
    doc.loaded("p", &key, Ok((Arc::new(model), bvh)));
    assert_ne!(doc.arrange_key().unwrap(), first);

    let (plain, _) = with_plate();
    assert_eq!(plain.arrange_key(), None, "no sheets");
}

/// Nothing is generated, or laid out again, while the sheet is being
/// arranged: the view keeps what it had. Arranged, the setup runs where the
/// sheet stands its part; arranged again for an edit, it waits again, and
/// the one in flight for what came before is stopped; undone, the one kept
/// is taken at once.
#[test]
fn nothing_is_generated_while_a_sheet_is_arranged() {
    let (mut doc, setup) = on_a_sheet();
    assert_eq!(
        doc.cannot_generate(&setup, None).as_deref(),
        Some("'Stock 1' is being arranged")
    );
    assert_eq!(doc.refresh_layout(&setup), None);
    assert!(
        doc.setups
            .get(&setup)
            .and_then(|s| s.layout.as_ref())
            .is_none()
    );
    assert!(doc.arrangement_for(0).is_none());

    arrange_now(&mut doc);
    assert!(doc.arrange_wanted().is_none(), "kept");
    assert_eq!(doc.cannot_generate(&setup, None), None);
    let stands = doc.stands_now("p").unwrap().to_vec();
    assert_eq!(stands.len(), 1);
    let b = doc.bodies().get("p").unwrap().bounds().unwrap();
    assert!(b.min.x >= 510.0 - 1e-6 && b.max.x <= 690.0 + 1e-6, "{b:?}");
    assert_eq!(b.min.z, 0.0, "on the sheet's bottom");
    doc.refresh_layout(&setup);
    let laid = doc.setups[&setup].layout.as_ref().unwrap().key;
    let inputs = SetupInputs::of(&doc.job, &doc, 0).unwrap();

    // Another spacing: being arranged again, the layout stays, and the run
    // would be out of date.
    sheet(&mut doc).spacing = 4.0;
    doc.checkpoint();
    let wanted = doc.arrange_wanted().unwrap();
    assert!(doc.arrange_wanted().is_none(), "in flight");
    assert!(
        doc.cannot_generate(&setup, None)
            .is_some_and(|why| why.contains("being arranged"))
    );
    assert_eq!(doc.refresh_layout(&setup), None);
    assert_eq!(doc.setups[&setup].layout.as_ref().unwrap().key, laid);
    assert!(!inputs.is_now(&doc, 0));
    assert!(doc.bodies().get("p").is_some(), "shown where it was");

    // Edited again before it is done: the one in flight is stopped.
    sheet(&mut doc).spacing = 6.0;
    doc.checkpoint();
    let newer = doc.arrange_wanted().unwrap();
    assert!(wanted.control.watch(0).stopped());
    assert!(!newer.control.watch(0).stopped());

    // Undone twice, the first arrangement is the one kept: at once.
    doc.undo();
    doc.undo();
    assert!(doc.arrange_wanted().is_none());
    assert!(newer.control.watch(0).stopped(), "no longer wanted");
    assert_eq!(doc.cannot_generate(&setup, None), None);
    assert!(inputs.is_now(&doc, 0));
}

/// A sheet with no room for its part says so where the part is shown; the
/// setup still runs what fits.
#[test]
fn a_part_left_out_is_said_to_be() {
    let (mut doc, _) = on_a_sheet();
    sheet(&mut doc).size = [30.0, 30.0, 18.0];
    sheet(&mut doc).border = 1.0;
    arrange_now(&mut doc);
    let SheetNow::Laid(_, laid) = doc.sheet_now("Stock 1") else {
        panic!("not laid");
    };
    let left: Vec<Outcome> = laid
        .left_out()
        .flat_map(|p| p.copies.iter().map(|c| c.outcome))
        .collect();
    assert_eq!(left, [Outcome::TooLarge]);
    assert_eq!(doc.stands_now("p"), Some(&[][..]));
    assert!(matches!(doc.sheet_now("nothing"), SheetNow::NotASheet));
}

/// Put on a sheet, a model's position goes, with the same edit; a copy of a
/// model on a sheet is on it too, where the sheet puts it, with no position
/// of its own.
#[test]
fn a_model_put_on_a_sheet_has_no_position() {
    let (mut doc, _) = with_plate();
    doc.job.models[0].transform = Transform {
        position: [30.0, 5.0, 0.0],
        rotation: [0.0, 0.0, 90.0],
        ..Transform::default()
    };
    doc.checkpoint();
    let mut sheet = Arranged::new([200.0, 100.0, 18.0]);
    sheet.parts = vec![SheetPart::of("p")];
    doc.job.stocks[0].spec = StockSpec::Arranged(sheet);
    doc.placed_by_sheets(&["p".to_string()]);
    assert_eq!(doc.job.models[0].transform.position, [0.0; 3]);
    assert_eq!(
        doc.job.models[0].transform.rotation,
        [0.0, 0.0, 90.0],
        "its own"
    );
    doc.checkpoint();
    doc.undo();
    assert_eq!(
        doc.job.models[0].transform.position,
        [30.0, 5.0, 0.0],
        "one step"
    );
    assert!(matches!(
        doc.job.stocks[0].spec,
        StockSpec::AroundModels { .. }
    ));
    doc.redo();

    let copy = doc.duplicate_model("p").unwrap();
    assert_eq!(doc.job.stocks[0].spec.models(), ["p", copy.as_str()]);
    assert_eq!(doc.job.model(&copy).unwrap().transform.position, [0.0; 3]);
    doc.job.check_common().unwrap();
}

/// Three copies of the plate on the sheet, laid live: the setup is laid out
/// with each, where the sheet stands it. A ray down onto any copy's top meets
/// that copy and the one face of the body, which is what a click saves; the
/// face picked once is drawn on every copy.
#[test]
fn a_pick_on_any_real_copy_is_one_pick_drawn_on_every_copy() {
    use camcraft_core::job::FeatureRef;
    use camcraft_core::setup::features::find_face;

    use crate::picking::ray::PickScene;
    use crate::picking::shown::shown;
    use crate::ui::measure::Measure;

    let (mut doc, setup) = on_a_sheet();
    sheet(&mut doc).size = [400.0, 100.0, 18.0];
    sheet(&mut doc).parts[0].copies = 3;
    arrange_now(&mut doc);
    let stands = doc.stands_now("p").unwrap().to_vec();
    assert_eq!(stands.len(), 3);
    doc.refresh_layout(&setup);
    let prepared = doc.setups[&setup]
        .layout
        .as_ref()
        .unwrap()
        .prepared
        .clone()
        .unwrap();
    assert_eq!(prepared.pieces.len(), 3);
    let bodies = doc.bodies();
    let (model, bvh) = doc.models["p"].loaded().unwrap();
    let scene = PickScene::new(
        &prepared,
        [(
            "p".to_string(),
            model.clone(),
            bvh.clone(),
            bodies.get("p").unwrap().stands.to_vec(),
        )],
        Vec::new(),
    );
    assert_eq!(scene.models[0].copies.len(), 3);

    // Straight down onto each copy's top, beside its hole: that copy, and
    // the body's top face, whichever copy.
    let mut faces = Vec::new();
    for (k, stand) in stands.iter().enumerate() {
        let on_top = prepared
            .frame
            .to_setup_as(stand, glam::dvec3(12.0, 12.0, 5.0));
        let hit = scene
            .cast(
                on_top + glam::DVec3::Z * 50.0,
                -glam::DVec3::Z,
                f64::INFINITY,
            )
            .unwrap();
        assert_eq!(hit.copy, k);
        faces.push(model.body.topology().face_of(hit.triangle).unwrap());
    }
    assert!(faces.iter().all(|f| *f == faces[0]), "{faces:?}");

    // Saved once, on the body, and drawn on every copy.
    let top = FeatureRef {
        model: "p".into(),
        index: faces[0],
        at: [12.0, 12.0, 5.0],
    };
    assert_eq!(find_face(&model.body, &top, 0.01).unwrap().index, faces[0]);
    let op = 0;
    doc.job.setups[0].operations[op].selection.faces = vec![top];
    let one = model
        .body
        .topology()
        .face(faces[0])
        .unwrap()
        .triangles
        .len();
    let drawn = shown(
        &doc,
        &setup,
        Some(op),
        (&prepared, Ok(None)),
        &scene,
        Measure::Metric,
    );
    assert_eq!(drawn.faces.len(), 1, "one pick");
    assert!(drawn.faces[0].is_ok(), "{:?}", drawn.faces);
    assert_eq!(drawn.highlight.faces.len(), 3 * one, "on every copy");
}
