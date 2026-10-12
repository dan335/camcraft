use super::*;
use camcraft_core::job::{Along, PointRef, Snap};

fn doc_with_op() -> Document {
    let mut doc = Document::untitled(1);
    doc.job.models.push(camcraft_core::job::ModelDef {
        name: "p".into(),
        path: "p.stl".into(),
        units: None,
        transform: Default::default(),
    });
    let setup = doc.job.setups[0].name.clone();
    doc.job.setups[0].models = vec!["p".into()];
    doc.add_operation(
        &setup,
        camcraft_core::job::StrategySpec::Parallel { angle: 0.0 },
    );
    doc
}

fn face(face: u32) -> Target {
    Target::Face {
        model: "p".into(),
        face,
        at: DVec3::new(1.0, 2.0, 3.0),
    }
}

/// A face clicked once is added; clicked again, taken out; a face the slot
/// does not take changes nothing.
#[test]
fn faces_toggle() {
    let mut doc = doc_with_op();
    let setup = doc.job.setups[0].name.clone();
    let slot = Slot::Faces { setup, op: 0 };
    let same = |r: &FeatureRef, _| Some(r.index);
    assert!(apply(&mut doc, &slot, &face(3), same));
    assert!(apply(&mut doc, &slot, &face(5), same));
    let faces = &doc.job.setups[0].operations[0].selection.faces;
    assert_eq!(faces.iter().map(|r| r.index).collect::<Vec<_>>(), [3, 5]);
    assert_eq!(faces[0].at, [1.0, 2.0, 3.0]);
    assert!(apply(&mut doc, &slot, &face(3), same));
    let faces = &doc.job.setups[0].operations[0].selection.faces;
    assert_eq!(faces.iter().map(|r| r.index).collect::<Vec<_>>(), [5]);
    let edge = Target::Edge {
        model: "p".into(),
        edge: 1,
        at: DVec3::ZERO,
    };
    assert!(!apply(&mut doc, &slot, &edge, same));
}

/// A loop clicked adds the edges of it not yet picked; clicked again with all
/// of it picked, takes them all out.
#[test]
fn edges_toggle_a_loop_at_a_time() {
    let mut doc = doc_with_op();
    let setup = doc.job.setups[0].name.clone();
    let slot = Slot::Edges { setup, op: 0 };
    let same = |r: &FeatureRef, _| Some(r.index);
    let round = Target::Loop {
        model: "p".into(),
        edges: vec![(4, DVec3::X), (5, DVec3::Y), (6, DVec3::Z)],
    };
    let one = Target::Edge {
        model: "p".into(),
        edge: 5,
        at: DVec3::Y,
    };
    let picked = |doc: &Document| {
        doc.job.setups[0].operations[0]
            .selection
            .edges
            .iter()
            .map(|r| r.index)
            .collect::<Vec<_>>()
    };
    assert!(apply(&mut doc, &slot, &one, same));
    assert!(apply(&mut doc, &slot, &round, same));
    assert_eq!(picked(&doc), [5, 4, 6]);
    assert_eq!(
        doc.job.setups[0].operations[0].selection.edges[2].at,
        [0.0, 0.0, 1.0]
    );
    assert!(apply(&mut doc, &slot, &round, same));
    assert!(picked(&doc).is_empty());
}

/// A saved pick found now under another number is still the same face: the
/// click takes it out rather than adding it twice.
#[test]
fn a_renumbered_pick_is_still_recognised() {
    let mut doc = doc_with_op();
    let setup = doc.job.setups[0].name.clone();
    let slot = Slot::Faces { setup, op: 0 };
    doc.job.setups[0].operations[0]
        .selection
        .faces
        .push(FeatureRef {
            model: "p".into(),
            index: 9,
            at: [0.0; 3],
        });
    let now_seven = |_: &FeatureRef, _| Some(7);
    assert!(apply(&mut doc, &slot, &face(7), now_seven));
    assert!(doc.job.setups[0].operations[0].selection.faces.is_empty());
}

/// A work zero takes one point, and keeps its offset whichever kind it becomes.
#[test]
fn a_work_zero_keeps_its_offset() {
    let mut doc = doc_with_op();
    let setup = doc.job.setups[0].name.clone();
    doc.job.setups[0].origin = Origin::ModelOrigin {
        offset: [0.0, 0.0, 2.0],
    };
    let slot = Slot::WorkZero {
        setup: setup.clone(),
    };
    assert!(slot.single());
    let point = PointRef {
        model: "p".into(),
        snap: Snap::CircleCenter,
        index: 4,
        at: [1.0, 1.0, 5.0],
    };
    let target = Target::Point {
        point: point.clone(),
        setup: DVec3::ZERO,
    };
    let never = |_: &FeatureRef, _| None;
    assert!(apply(&mut doc, &slot, &target, never));
    assert_eq!(
        doc.job.setups[0].origin,
        Origin::Point {
            point,
            offset: [0.0, 0.0, 2.0]
        }
    );
    let corner = Target::StockPoint {
        x: Along::Min,
        y: Along::Min,
        z: Along::Max,
        setup: DVec3::ZERO,
    };
    assert!(apply(&mut doc, &slot, &corner, never));
    assert!(matches!(
        doc.job.setups[0].origin,
        Origin::BoxPoint {
            of: BoxOf::Stock,
            z: Along::Max,
            offset: [0.0, 0.0, 2.0],
            ..
        }
    ));
    // The same again changes nothing.
    assert!(!apply(&mut doc, &slot, &corner, never));
    assert!(!apply(&mut doc, &slot, &face(1), never));
}

/// A model or stock clicked is added to the setup; clicked again, taken out.
/// A face means nothing to them.
#[test]
fn models_and_stocks_toggle() {
    let mut doc = doc_with_op();
    let setup = doc.job.setups[0].name.clone();
    let never = |_: &FeatureRef, _| None;
    let models = Slot::Models {
        setup: setup.clone(),
    };
    assert!(!models.single());
    let q = Target::Model { model: "q".into() };
    assert!(apply(&mut doc, &models, &q, never));
    assert_eq!(doc.job.setups[0].models, ["p", "q"]);
    assert!(apply(&mut doc, &models, &q, never));
    assert_eq!(doc.job.setups[0].models, ["p"]);
    assert!(!apply(&mut doc, &models, &face(1), never));

    let stocks = Slot::Stocks {
        setup: setup.clone(),
    };
    let before = doc.job.setups[0].stocks.clone();
    let other = Target::Stock {
        stock: "Other".into(),
    };
    assert!(apply(&mut doc, &stocks, &other, never));
    assert_eq!(
        doc.job.setups[0].stocks.last().map(String::as_str),
        Some("Other")
    );
    assert!(apply(&mut doc, &stocks, &other, never));
    assert_eq!(doc.job.setups[0].stocks, before);
    assert!(!apply(&mut doc, &stocks, &q, never));

    doc.selection = Selection::Setup(setup);
    assert!(models.still_valid(&doc));
    doc.selection = Selection::Job;
    assert!(!models.still_valid(&doc));
}

/// Picking stops making sense when what it fills is no longer shown.
#[test]
fn a_slot_lasts_while_its_owner_is_selected() {
    let mut doc = doc_with_op();
    let setup = doc.job.setups[0].name.clone();
    let slot = Slot::Faces {
        setup: setup.clone(),
        op: 0,
    };
    doc.selection = Selection::Operation(setup.clone(), 0);
    assert!(slot.still_valid(&doc));
    doc.selection = Selection::Setup(setup.clone());
    assert!(!slot.still_valid(&doc));
    doc.selection = Selection::Operation(setup.clone(), 3);
    assert!(!Slot::Faces { setup, op: 3 }.still_valid(&doc));
}
