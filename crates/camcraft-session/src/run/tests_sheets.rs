//! A job on an arranged sheet, loaded from its files and run: where its parts
//! stand is on record, and the checks hold the sheet to its parts.

use std::path::Path;

use camcraft_core::arrange::Outcome;
use camcraft_core::job::{Arranged, Boundary, FeatureRef, Job, SheetPart, StockDef, StockSpec};

use crate::checks::Level;
use crate::test_support::small_job;
use crate::{Ran, load_models, models_of, run_setup};

/// Two hinge jigs, 25.4 by 31.8 and 33.3 mm tall, on a sheet `size`, both
/// machined by the one setup.
fn on_a_sheet(size: [f64; 3]) -> Job {
    let mut job = small_job();
    job.tolerance = 0.1;
    let hinge = job.models[0].clone();
    job.models = ["a", "b"]
        .into_iter()
        .map(|name| camcraft_core::job::ModelDef {
            name: name.into(),
            ..hinge.clone()
        })
        .collect();
    let mut sheet = Arranged::new(size);
    sheet.parts = vec![SheetPart::of("a"), SheetPart::of("b")];
    job.stocks = vec![StockDef {
        name: "sheet".into(),
        spec: StockSpec::Arranged(sheet),
    }];
    let setup = &mut job.setups[0];
    setup.models = vec!["a".into(), "b".into()];
    setup.stocks = vec!["sheet".into()];
    setup.operations[0].passes.stepover = 4.0;
    job.validate().unwrap();
    job
}

fn run(job: &Job) -> Ran {
    let loaded = load_models(job, Path::new("."), &[0]).unwrap();
    run_setup(job, 0, &models_of(&loaded)).unwrap()
}

/// The checks of `code` the setup's group holds, by level and message.
fn found(ran: &Ran, code: &str) -> Vec<(Level, String)> {
    ran.checks.groups[0]
        .findings
        .iter()
        .filter(|f| f.code == code)
        .map(|f| (f.level, f.message.clone()))
        .collect()
}

/// Both parts fit: each stands on the sheet, the run is on record with
/// where, and every check of the sheet passes. Shrunk so one has no room,
/// the other is still cut, and the sheet fails; thinner than the parts are
/// tall, they stand above it, and it fails; with a part the setup does not
/// machine, the setup says so.
#[test]
fn a_sheet_is_run_and_held_to_its_parts() {
    let job = on_a_sheet([120.0, 80.0, 40.0]);
    let loaded = load_models(&job, Path::new("."), &[0]).unwrap();
    let sheet = loaded.arrangement.sheet("sheet").unwrap();
    assert!(sheet.left_out().next().is_none());
    let ran = run_setup(&job, 0, &models_of(&loaded)).unwrap();
    assert!(ran.passed(), "{:?}", ran.checks.failure_lines());
    assert!(
        ran.notes
            .iter()
            .any(|n| n.starts_with("sheet 'sheet': 2 of 2 part(s) placed")),
        "{:?}",
        ran.notes
    );
    assert!(
        ran.notes
            .iter()
            .any(|n| n.starts_with("sheet 'sheet': 'b' at (")),
        "{:?}",
        ran.notes
    );
    assert_eq!(found(&ran, "CC109")[0].0, Level::Pass);
    assert_eq!(found(&ran, "CC110")[0].0, Level::Pass);
    assert!(found(&ran, "CC225").is_empty());
    // The part is both, each where it stands, inside the sheet.
    assert_eq!(ran.prepared.pieces.len(), 2);
    let b = ran.prepared.model_bounds;
    let s = ran.prepared.stock_bounds();
    assert!(b.min.x >= s.min.x + 10.0 - 1e-6 && b.max.x <= s.max.x - 10.0 + 1e-6);
    assert!(b.min.z >= s.min.z - 1e-9 && b.max.z <= s.max.z);

    // Room for one only: it is cut, and the other is said to have none.
    let small = on_a_sheet([70.0, 60.0, 40.0]);
    let loaded = load_models(&small, Path::new("."), &[0]).unwrap();
    let left: Vec<(String, Outcome)> = loaded
        .arrangement
        .sheet("sheet")
        .unwrap()
        .left_out()
        .map(|p| (p.model.clone(), p.outcome))
        .collect();
    assert_eq!(left, [("b".to_string(), Outcome::NoRoom)]);
    let ran = run_setup(&small, 0, &models_of(&loaded)).unwrap();
    assert!(!ran.passed());
    assert_eq!(
        found(&ran, "CC109"),
        [(Level::Fail, "sheet 'sheet' has no room for 'b'".to_string())]
    );
    assert_eq!(ran.prepared.pieces.len(), 1);
    assert!(
        ran.notes
            .iter()
            .any(|n| n == "sheet 'sheet': 'b' left out: there is no room left for it"),
        "{:?}",
        ran.notes
    );
    // An operation kept to picks on the part left out has nothing to cut,
    // and says why, rather than that nothing was picked.
    let mut picked = small.clone();
    let op = &mut picked.setups[0].operations[0];
    op.boundary.kind = Boundary::Selection;
    op.selection.faces = vec![FeatureRef {
        model: "b".into(),
        index: 0,
        at: [0.0; 3],
    }];
    let err = run_setup(&picked, 0, &models_of(&loaded)).unwrap_err();
    assert!(err.to_string().contains("'b' has no copy placed"), "{err}");

    // Thinner than the parts are tall: they stand above it.
    let thin = run(&on_a_sheet([120.0, 80.0, 18.0]));
    let above = found(&thin, "CC110");
    assert_eq!(above.len(), 2, "{above:?}");
    assert!(
        above
            .iter()
            .all(|(l, m)| *l == Level::Fail && m.contains("mm above sheet 'sheet'"))
    );
    assert!(!thin.passed());

    // A part on the sheet the setup does not machine.
    let mut one = on_a_sheet([120.0, 80.0, 40.0]);
    one.setups[0].models = vec!["a".into()];
    let ran = run(&one);
    assert_eq!(
        found(&ran, "CC225"),
        [(
            Level::Warn,
            "sheet 'sheet' holds 'b', which this setup neither machines nor checks".to_string()
        )]
    );
    assert!(
        ran.passed(),
        "an advisory: {:?}",
        ran.checks.failure_lines()
    );
    assert_eq!(ran.prepared.pieces.len(), 1);
}
