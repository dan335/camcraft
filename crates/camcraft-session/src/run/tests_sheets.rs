//! A job on an arranged sheet, loaded from its files and run: where its parts
//! stand is on record, and the checks hold the sheet to its parts.

use std::path::Path;

use camcraft_core::arrange::Outcome;
use camcraft_core::job::{
    Arranged, Boundary, FeatureRef, Job, Operation, Outline, SheetPart, StockDef, StockSpec,
    StrategySpec,
};
use camcraft_core::run::StrategyDetail;

use crate::checks::Level;
use crate::test_support::small_job;
use crate::{Ran, load_models, models_of, run_setup};

/// Two hinge jigs, 25.4 by 31.8 and 33.3 mm tall, on a sheet `size`, both
/// machined by the one setup. Its finish keeps to the parts' silhouette, as
/// a new one starts: over the sheet round them, with no roughing before it,
/// it would come straight down into the whole sheet, and bury the tool's
/// shank (CC108).
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
    setup.operations[0].boundary.kind = Boundary::Silhouette;
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
        .flat_map(|p| p.copies.iter().map(|c| (p.model.clone(), c.outcome)))
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

/// Copies a sheet has no room for are counted: CC109 says how many of how
/// many, the log lists every copy, where it stands or that it is left out,
/// and the copies that fit are each cut.
#[test]
fn copies_with_no_room_are_counted() {
    let mut job = on_a_sheet([120.0, 80.0, 40.0]);
    if let StockSpec::Arranged(a) = &mut job.stocks[0].spec {
        a.parts[0].copies = 6;
    }
    job.validate().unwrap();
    let loaded = load_models(&job, Path::new("."), &[0]).unwrap();
    let sheet = loaded.arrangement.sheet("sheet").unwrap();
    let a = &sheet.parts()[0];
    let (placed, left) = (a.placed(), a.no_room());
    assert!(placed >= 1 && left >= 1, "{placed} placed, {left} left out");
    assert_eq!(placed + left, 6);
    let ran = run_setup(&job, 0, &models_of(&loaded)).unwrap();
    let said = (
        Level::Fail,
        format!("sheet 'sheet' has no room for {left} of 6 'a'"),
    );
    assert!(
        found(&ran, "CC109").contains(&said),
        "{:?}",
        found(&ran, "CC109")
    );
    assert_eq!(found(&ran, "CC110")[0].0, Level::Pass);
    let (all, of) = sheet.placed();
    assert_eq!(of, 7);
    assert_eq!(ran.prepared.pieces.len(), all);
    assert!(
        ran.notes
            .iter()
            .any(|n| n.starts_with(&format!("sheet 'sheet': {all} of 7 part(s) placed"))),
        "{:?}",
        ran.notes
    );
    for k in 1..=placed {
        let at = format!("sheet 'sheet': 'a' copy {k} of 6 at (");
        assert!(
            ran.notes.iter().any(|n| n.starts_with(&at)),
            "{at}: {:?}",
            ran.notes
        );
    }
    let last = "sheet 'sheet': 'a' copy 6 of 6 left out: there is no room left for it";
    assert!(ran.notes.iter().any(|n| n == last), "{:?}", ran.notes);
    // 'b', one copy, is named as a part always was.
    assert!(
        ran.notes
            .iter()
            .any(|n| n.starts_with("sheet 'sheet': 'b' at (")
                || n.starts_with("sheet 'sheet': 'b' left out")),
        "{:?}",
        ran.notes
    );

    // All placed, the pass counts the copies.
    let mut roomy = on_a_sheet([400.0, 300.0, 40.0]);
    if let StockSpec::Arranged(a) = &mut roomy.stocks[0].spec {
        a.parts[0].copies = 3;
    }
    let ran = run(&roomy);
    assert_eq!(
        found(&ran, "CC109"),
        [(
            Level::Pass,
            "sheet 'sheet' holds all 4 copies of its 2 parts".to_string()
        )]
    );
    assert_eq!(ran.prepared.pieces.len(), 4);
}

/// The 2D part's job on a sheet laying `copies` copies of it, `size`: its
/// tools, the 6 mm flat (T1) and a 6 mm ball (T2), and its one setup, with no
/// operations yet.
fn ops_2d_sheet(size: [f64; 3], copies: u32, tolerance: f64) -> Job {
    let text = format!(
        r#"
version = 1
tolerance = {tolerance:?}

[tools.flat6]
kind = "flat"
diameter = 6.0
number = 1
flute_length = 20.0
shoulder_length = 20.0
shank_diameter = 6.0
stickout = 30.0

[tools.ball6]
kind = "ball"
diameter = 6.0
number = 2
flute_length = 20.0
shoulder_length = 20.0
shank_diameter = 6.0
stickout = 30.0

[[model]]
name = "part"
path = "{}"

[[stock]]
name = "sheet"
kind = "arranged"
size = {size:?}
parts = [{{ model = "part", copies = {copies} }}]

[[setup]]
name = "top"
models = ["part"]
stocks = ["sheet"]
post = "masso"
"#,
        crate::test_support::fixture("ops_2d.step")
            .display()
            .to_string()
            .replace('\\', "/"),
    );
    crate::parse_job(&text, "sheet").unwrap()
}

/// A new operation of `strategy` with `tool`, as the app makes one, at
/// 1500 mm/min.
fn new_op(job: &Job, name: &str, strategy: &str, tool: &str) -> Operation {
    let mut op = Operation::fresh(
        name,
        StrategySpec::named(strategy).unwrap(),
        (Some(tool.to_string()), job.tools.get(tool), None),
    );
    op.passes.feed = 1500.0;
    op
}

/// A pick on the 2D part at `at`.
fn on_part(at: [f64; 3]) -> FeatureRef {
    FeatureRef {
        model: "part".into(),
        index: 0,
        at,
    }
}

/// The floor of the 2D part's pocket with the island, and a point on the
/// wall of each of its two holes, as `compare::tests::ops_2d` picks them.
const POCKET_FLOOR: [f64; 3] = [-47.0, -22.0, 12.0];
const HOLES: [[f64; 3]; 2] = [[-47.0, 35.0, 10.0], [-23.0, 35.0, 15.0]];

/// Three copies of the 2D part on a sheet: a pocket kept to the floor picked
/// once, and a bore of the two holes picked once, cut every copy where it
/// stands. The setup passes its checks, nothing is gouged, its programs are
/// posted, and the material is cut down in the pocket and through each hole
/// of each copy.
#[test]
fn a_pick_on_a_part_of_copies_cuts_every_copy() {
    let mut job = ops_2d_sheet([480.0, 140.0, 20.0], 3, 0.05);
    let mut pocket = new_op(&job, "pocket", "pocket", "flat6");
    pocket.outline = Outline::Selection;
    pocket.selection.faces = vec![on_part(POCKET_FLOOR)];
    let mut bore = new_op(&job, "bore", "bore", "flat6");
    bore.selection.faces = HOLES.iter().map(|&at| on_part(at)).collect();
    job.setups[0].operations = vec![pocket, bore];
    let job = crate::parse_job(&crate::job_text(&job).unwrap(), "sheet").unwrap();
    let ran = run(&job);
    assert!(ran.passed(), "{:?}", ran.checks.failure_lines());
    assert_eq!(ran.result.verify.gouged, 0);
    assert_eq!(ran.prepared.pieces.len(), 3);
    let StrategyDetail::Bore(bored) = &ran.result.operations[1].detail else {
        panic!("{:?}", ran.result.operations[1].detail);
    };
    assert_eq!(bored.holes, 6, "two holes on each of three copies");

    let dir = std::env::temp_dir().join("camcraft-sheet-copies");
    let posting =
        crate::plan_posting(&[(&job.setups[0], &ran)], Path::new("."), &dir, None).unwrap();
    assert!(posting.refused.is_empty(), "{:?}", posting.refused);
    assert_eq!(posting.programs.len(), 2);
    assert!(!dir.exists(), "planning writes nothing");

    // Each copy, where it stands: its pocket's floor bared, and the bore's
    // ring cut through its through hole, 16 across at (-55, 35).
    let stock = ran.result.keyframes.last().unwrap().unpack().unwrap();
    let frame = &ran.prepared.frame;
    for piece in &ran.prepared.pieces {
        let at = |x: f64, y: f64| {
            let p = frame.to_setup_as(&piece.stand, glam::dvec3(x, y, 0.0));
            stock.height_at(p.x, p.y).unwrap()
        };
        let floor = at(POCKET_FLOOR[0], POCKET_FLOOR[1]);
        let hole = at(-50.0, 35.0);
        let top = at(65.0, 45.0);
        // Down to its floor, at 12, but for the stock it leaves.
        assert!(floor < 13.0, "copy {}: the pocket at {floor}", piece.copy);
        assert!(hole < 0.5, "copy {}: the hole at {hole}", piece.copy);
        assert!(top > 19.5, "copy {}: the top at {top}", piece.copy);
    }
}

/// How long a sheet of twenty copies of the 2D part takes, step by step, in
/// a release build on four threads: arranging it, laying the copies out as
/// one part, what is worked out from that part alone, and one 2D and one 3D
/// operation run on it, each run alone.
/// `cargo test --release -p camcraft-session --lib run::tests_sheets::twenty_copies_timed -- --ignored --nocapture`
#[test]
#[ignore = "times a sheet of twenty copies: a minute or more, in a release build"]
fn twenty_copies_timed() {
    use std::collections::BTreeMap;
    use std::time::Instant;

    use camcraft_core::arrange::{Outlines, arrange};
    use camcraft_core::control::{Control, Watch};
    use camcraft_core::run::{Plan, run_prepared_planned};
    use camcraft_core::setup::{Bodies, prepare};

    let mut job = ops_2d_sheet([2440.0, 1220.0, 20.0], 20, 0.05);
    let mut pocket = new_op(&job, "pocket", "pocket", "flat6");
    pocket.outline = Outline::Selection;
    pocket.selection.faces = vec![on_part(POCKET_FLOOR)];
    let mut parallel = new_op(&job, "parallel", "parallel", "ball6");
    parallel.passes.stepover = 2.0;
    job.setups[0].operations = vec![pocket, parallel];
    let job = crate::parse_job(&crate::job_text(&job).unwrap(), "sheet").unwrap();
    let loaded = load_models(&job, Path::new("."), &[0]).unwrap();
    let body = &loaded.models["part"].body;
    let named: BTreeMap<&str, &camcraft_geom::topology::Body> = [("part", body)].into();

    let time = |what: &str, start: Instant| {
        println!("{what}: {:.3} s", start.elapsed().as_secs_f64());
    };
    let start = Instant::now();
    let arrangement = arrange(&job, &named, &mut Outlines::default(), &Watch::default()).unwrap();
    time("(a) arrange, its outline cast", start);
    let mut outlines = Outlines::default();
    arrange(&job, &named, &mut outlines, &Watch::default()).unwrap();
    let start = Instant::now();
    arrange(&job, &named, &mut outlines, &Watch::default()).unwrap();
    time("(a) arrange again, its outline kept", start);
    let sheet = arrangement.sheet("sheet").unwrap();
    println!("    {:?} of the copies placed", sheet.placed());

    let bodies = Bodies::arranged([("part", body)], &arrangement);
    let start = Instant::now();
    let prepared = prepare(&job, 0, &bodies).unwrap();
    time("(b) prepare, the copies merged", start);
    println!(
        "    the model {} triangles, the part {} triangles",
        body.mesh().len(),
        prepared.part.len()
    );
    let start = Instant::now();
    prepared.shadow().unwrap();
    time("(c) the part's shadow", start);
    let start = Instant::now();
    let cutter = prepared.drop_cutter();
    time("(c) the drop-cutter's index", start);
    drop(cutter);
    // The pitch the run simulates at: 8 tolerances, the stock being too large
    // for a finer one, and coarser than a 64 million cell grid would need.
    let pitch = 0.4;
    let start = Instant::now();
    prepared.part_surface(pitch).unwrap();
    time("(c) the part's surface", start);

    for (k, name) in ["(d) the 2D pocket's run", "(d) the 3D parallel's run"]
        .into_iter()
        .enumerate()
    {
        let plans: Vec<Plan> = (0..2)
            .map(|i| if i == k { Plan::Generate } else { Plan::Skip })
            .collect();
        let start = Instant::now();
        let result = run_prepared_planned(
            &job,
            0,
            &prepared,
            &bodies,
            &plans,
            &Control::new(2),
            &mut |_| {},
        )
        .unwrap();
        time(name, start);
        let field = result.keyframes[0].unpack().unwrap();
        assert!(
            (field.field().pitch() - pitch).abs() < 1e-12,
            "{}",
            field.field().pitch()
        );
        println!(
            "    {} moves, {} column(s) gouged",
            result.operations[k].path.moves().len(),
            result.verify.gouged
        );
    }
}
