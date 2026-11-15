//! Arranged sheets in a job file: their shape, their defaults, and what is
//! refused.

use super::*;
use crate::arrange::{Corner, Direction};

/// A job of a sheet holding `bracket` and `side`, and a setup machining both
/// on it.
const SHEET: &str = r#"
version = 1
tolerance = 0.05

[tools.flat6]
kind = "flat"
diameter = 6.0
number = 1

[[model]]
name = "bracket"
path = "bracket.step"

[[model]]
name = "side"
path = "side.step"
transform = { rotation = [90.0, 0.0, 0.0] }

[[model]]
name = "jig"
path = "jig.step"

[[stock]]
name = "sheet"
kind = "arranged"
size = [2440.0, 1220.0, 18.0]
parts = [{ model = "bracket" }, { model = "side" }]

[[setup]]
name = "top"
models = ["bracket", "side"]
stocks = ["sheet"]

[[setup.operation]]
name = "cut"
strategy = "face"
tool = "flat6"
passes = { stepover = 3.0, feed = 1000.0 }
linking = { retract_feed = 1000.0, plunge_feed = 300.0 }
"#;

fn sheet_job() -> Job {
    toml::from_str(SHEET).unwrap()
}

fn sheet(j: &mut Job) -> &mut Arranged {
    let StockSpec::Arranged(a) = &mut j.stocks[0].spec else {
        panic!("not a sheet");
    };
    a
}

/// Left out, a sheet's settings are its defaults: at the origin, 10 apart and
/// 10 in, by quarter turns, from the front left along X, on its bottom.
#[test]
fn a_sheet_left_out_has_its_defaults() {
    let mut j = sheet_job();
    j.validate().unwrap();
    let a = sheet(&mut j).clone();
    assert_eq!(
        a,
        Arranged {
            parts: vec![SheetPart::of("bracket"), SheetPart::of("side")],
            ..Arranged::new([2440.0, 1220.0, 18.0])
        }
    );
    assert_eq!(a.corner, [0.0; 3]);
    assert_eq!((a.spacing, a.border), (10.0, 10.0));
    assert_eq!(a.rotation, Rotation::QuarterTurns);
    assert_eq!(
        (a.from, a.along, a.seat),
        (Corner::FrontLeft, Direction::X, Seat::Bottom)
    );
    // Its size is not: a sheet is the size it is.
    let no_size = SHEET.replace("size = [2440.0, 1220.0, 18.0]\n", "");
    assert!(toml::from_str::<Job>(&no_size).is_err());
}

/// Every setting is written and read back the same, the corner only where it
/// is not the origin, and each way of saying the rotation.
#[test]
fn a_sheet_reads_back_as_written() {
    let mut j = sheet_job();
    let text = toml::to_string(&j).unwrap();
    assert!(!text.contains("corner"), "{text}");
    assert!(text.contains("kind = \"arranged\""), "{text}");
    assert_eq!(toml::from_str::<Job>(&text).unwrap(), j);

    for rotation in [
        Rotation::None,
        Rotation::HalfTurn,
        Rotation::QuarterTurns,
        Rotation::Step(15.0),
    ] {
        *sheet(&mut j) = Arranged {
            size: [1000.0, 500.0, 12.0],
            corner: [-5.0, 20.0, -12.0],
            spacing: 6.5,
            border: 4.0,
            rotation,
            from: Corner::BackRight,
            along: Direction::Y,
            seat: Seat::Top,
            parts: vec![SheetPart::of("side"), SheetPart::of("bracket")],
        };
        let text = toml::to_string(&j.stocks[0]).unwrap();
        let again: StockDef = toml::from_str(&text).unwrap();
        assert_eq!(again, j.stocks[0], "{text}");
        assert!(text.contains("from = \"back-right\""), "{text}");
        assert!(text.contains("along = \"y\""), "{text}");
        assert!(text.contains("seat = \"top\""), "{text}");
        assert!(text.contains("corner = [-5.0, 20.0, -12.0]"), "{text}");
    }
    // As a person writes them.
    for (written, rotation) in [
        ("\"none\"", Rotation::None),
        ("\"half-turn\"", Rotation::HalfTurn),
        ("\"quarter-turns\"", Rotation::QuarterTurns),
        ("{ step = 15.0 }", Rotation::Step(15.0)),
        ("{ step = 30 }", Rotation::Step(30.0)),
    ] {
        let text = SHEET.replace(
            "size = [2440.0",
            &format!("rotation = {written}\nsize = [2440.0"),
        );
        let mut read: Job = toml::from_str(&text).unwrap();
        assert_eq!(sheet(&mut read).rotation, rotation, "{written}");
    }
    for wrong in ["\"quarter\"", "{ steps = 15.0 }", "{}", "15.0"] {
        let text = SHEET.replace(
            "size = [2440.0",
            &format!("rotation = {wrong}\nsize = [2440.0"),
        );
        assert!(toml::from_str::<Job>(&text).is_err(), "{wrong}");
    }
    // A part has its model, its copies and its grain, and nothing else.
    let text = SHEET.replace("{ model = \"side\" }", "{ model = \"side\", count = 2 }");
    assert!(toml::from_str::<Job>(&text).is_err());
}

/// A part's copies and grain read as a person writes them, and are written
/// back the same: each left out where it is as a part starts, one copy turned
/// as its sheet allows.
#[test]
fn a_parts_copies_and_grain_read_back_as_written() {
    let text = SHEET.replace(
        "parts = [{ model = \"bracket\" }, { model = \"side\" }]",
        "parts = [{ model = \"bracket\", copies = 6 }, { model = \"side\", rotation = \"half-turn\" }]",
    );
    let mut j: Job = toml::from_str(&text).unwrap();
    j.validate().unwrap();
    let parts = sheet(&mut j).parts.clone();
    assert_eq!(
        parts,
        [
            SheetPart {
                copies: 6,
                ..SheetPart::of("bracket")
            },
            SheetPart {
                rotation: Some(Rotation::HalfTurn),
                ..SheetPart::of("side")
            },
        ]
    );
    assert_eq!(j.copies_asked("bracket").0, 6);
    assert_eq!(j.copies_asked("side").0, 1);
    assert_eq!(j.copies_asked("jig"), (1, None));

    let written = toml::to_string(&j.stocks[0]).unwrap();
    assert!(written.contains("copies = 6"), "{written}");
    assert!(written.contains("rotation = \"half-turn\""), "{written}");
    assert!(!written.contains("copies = 1"), "{written}");
    assert_eq!(toml::from_str::<StockDef>(&written).unwrap(), j.stocks[0]);
    // A step of its own, as the sheet's is written.
    sheet(&mut j).parts[1].rotation = Some(Rotation::Step(30.0));
    let written = toml::to_string(&j.stocks[0]).unwrap();
    assert!(written.contains("step = 30.0"), "{written}");
    assert_eq!(toml::from_str::<StockDef>(&written).unwrap(), j.stocks[0]);
    // One copy, turned as the sheet allows, is written as a part always was.
    let plain = sheet_job();
    let written = toml::to_string(&plain.stocks[0]).unwrap();
    assert!(!written.contains("copies"), "{written}");
    assert_eq!(
        written.matches("rotation").count(),
        1,
        "the sheet's: {written}"
    );

    // A copy of a model on the sheet is a copy of its part: as many copies,
    // with the same grain.
    j.stocks[0].spec.copy_model("bracket", "bracket copy");
    assert_eq!(
        sheet(&mut j).parts[2],
        SheetPart {
            copies: 6,
            ..SheetPart::of("bracket copy")
        }
    );
}

/// A part asking for no copy, or for more than a sheet lays, is refused, and
/// says how many it may have; so is a grain by a step that is not one.
#[test]
fn a_parts_copies_and_grain_are_held_to_what_a_sheet_lays() {
    let mut j = sheet_job();
    for copies in [0, MOST_COPIES + 1] {
        sheet(&mut j).parts[0].copies = copies;
        let e = j.validate().unwrap_err();
        assert_eq!(
            e,
            JobError::Copies {
                stock: "sheet".into(),
                model: "bracket".into(),
                copies
            }
        );
        assert!(
            e.to_string()
                .contains(&format!("from 1 to {MOST_COPIES} copies")),
            "{e}"
        );
    }
    for copies in [1, MOST_COPIES] {
        sheet(&mut j).parts[0].copies = copies;
        j.validate().unwrap();
    }
    for step in [0.5, 181.0, f64::NAN] {
        sheet(&mut j).parts[1].rotation = Some(Rotation::Step(step));
        match j.validate().unwrap_err() {
            JobError::BadValue { field, .. } => {
                assert_eq!(field, "stock 'sheet'.parts.1.rotation.step");
            }
            other => panic!("{other}"),
        }
    }
    sheet(&mut j).parts[1].rotation = Some(Rotation::Step(180.0));
    j.validate().unwrap();
}

/// The work zero picked on a part of several copies is no one place, and a
/// Morph's curves picked on one are two on each copy: both are refused from
/// the job alone, before anything is run, and say what to do instead. On a
/// part of one copy, each is as it ever was.
#[test]
fn what_means_one_place_is_refused_on_a_part_of_several_copies() {
    let mut j = sheet_job();
    j.setups[0].origin = Origin::Point {
        point: PointRef {
            model: "bracket".into(),
            snap: Snap::Vertex,
            index: 0,
            at: [0.0; 3],
        },
        offset: [0.0; 3],
    };
    j.validate().unwrap();
    sheet(&mut j).parts[0].copies = 3;
    let e = j.validate().unwrap_err();
    assert_eq!(
        e,
        JobError::ZeroOnCopies {
            setup: "top".into(),
            model: "bracket".into(),
            copies: 3,
            stock: "sheet".into()
        }
    );
    assert!(e.to_string().contains("pick it on the stock"), "{e}");

    let mut j = sheet_job();
    let mut morph = Operation::fresh(
        "morph",
        StrategySpec::named("morph").unwrap(),
        (Some("flat6".into()), j.tools.get("flat6"), None),
    );
    morph.selection.edges = vec![FeatureRef {
        model: "side".into(),
        index: 0,
        at: [0.0; 3],
    }];
    j.setups[0].operations.push(morph);
    j.validate().unwrap();
    sheet(&mut j).parts[1].copies = 2;
    let e = j.validate().unwrap_err();
    assert_eq!(
        e,
        JobError::MorphOnCopies {
            op: "top/morph".into(),
            model: "side".into(),
            copies: 2,
            stock: "sheet".into()
        }
    );
    assert!(
        e.to_string()
            .contains("Morph cannot yet morph across copies"),
        "{e}"
    );
    assert!(e.to_string().contains("a Morph of its own"), "{e}");
    // Turned off, it is not run, so not refused.
    j.setups[0].operations[1].enabled = false;
    j.validate().unwrap();
}

/// What the sheet names follows its models: a rename, a copy made with one,
/// what has to let go of one, and what a setup needs loaded.
#[test]
fn a_sheet_names_its_models() {
    let mut j = sheet_job();
    assert_eq!(j.stocks[0].spec.models(), ["bracket", "side"]);
    assert_eq!(j.model_users("side"), ["setup 'top'", "stock 'sheet'"]);
    j.rename_model_refs("side", "panel");
    assert_eq!(j.stocks[0].spec.models(), ["bracket", "panel"]);
    j.stocks[0].spec.copy_model("bracket", "bracket copy");
    j.stocks[0].spec.copy_model("jig", "jig copy");
    assert_eq!(
        j.stocks[0].spec.models(),
        ["bracket", "panel", "bracket copy"]
    );
    assert_eq!(
        j.sheet_of("panel").map(|(s, _)| s.name.as_str()),
        Some("sheet")
    );
    assert_eq!(j.sheet_of("jig").map(|(s, _)| s.name.as_str()), None);
    // A setup needs every part of its sheet, as every one decides where the
    // others stand.
    let mut j = sheet_job();
    j.setups[0].models = vec!["bracket".into()];
    let wanted: Vec<&str> = j.models_for(0).iter().map(|m| m.name.as_str()).collect();
    assert_eq!(wanted, ["bracket", "side"]);
}

/// Each way a sheet can be wrong is refused, naming what is wrong.
#[test]
fn a_sheet_that_cannot_be_arranged_is_refused() {
    let refused = |change: &dyn Fn(&mut Job)| {
        let mut j = sheet_job();
        change(&mut j);
        j.validate().unwrap_err()
    };
    let field = |e: JobError| match e {
        JobError::BadValue { field, .. } => field,
        other => panic!("{other}"),
    };
    assert_eq!(
        field(refused(&|j| sheet(j).size[2] = 0.0)),
        "stock 'sheet'.size"
    );
    assert_eq!(
        field(refused(&|j| sheet(j).corner[0] = f64::NAN)),
        "stock 'sheet'.corner"
    );
    assert_eq!(
        field(refused(&|j| sheet(j).spacing = -1.0)),
        "stock 'sheet'.spacing"
    );
    assert_eq!(
        field(refused(&|j| sheet(j).border = -1.0)),
        "stock 'sheet'.border"
    );
    // Twice the border is the whole width, so nothing fits between.
    assert_eq!(
        field(refused(&|j| sheet(j).border = 610.0)),
        "stock 'sheet'.border"
    );
    sheet_job().validate().unwrap();
    for step in [0.5, 181.0, f64::INFINITY] {
        assert_eq!(
            field(refused(&|j| sheet(j).rotation = Rotation::Step(step))),
            "stock 'sheet'.rotation.step"
        );
    }
    for step in [1.0, 180.0] {
        let mut j = sheet_job();
        sheet(&mut j).rotation = Rotation::Step(step);
        j.validate().unwrap();
    }

    assert_eq!(
        refused(&|j| sheet(j).parts.push(SheetPart::of("ghost"))),
        JobError::UnknownModel {
            owner: "stock 'sheet'".into(),
            model: "ghost".into()
        }
    );
    assert_eq!(
        refused(&|j| sheet(j).parts.push(SheetPart::of("side"))),
        JobError::RepeatedPart {
            stock: "sheet".into(),
            model: "side".into()
        }
    );
    let e = refused(&|j| {
        let mut other = j.stocks[0].clone();
        other.name = "offcut".into();
        j.stocks.push(other);
    });
    assert_eq!(
        e,
        JobError::OnTwoSheets {
            model: "bracket".into(),
            first: "sheet".into(),
            second: "offcut".into()
        }
    );
    // Its sheet places it: a position would do nothing.
    let e = refused(&|j| j.models[1].transform.position = [10.0, 0.0, 0.0]);
    assert_eq!(
        e,
        JobError::PlacedBySheet {
            model: "side".into(),
            position: [10.0, 0.0, 0.0],
            stock: "sheet".into()
        }
    );
    assert!(e.to_string().contains("does nothing"), "{e}");
    // Its own rotation and scale say which face is up, and stay its own.
    let mut j = sheet_job();
    j.models[1].transform.scale = [2.0; 3];
    j.validate().unwrap();
    // A model the sheet does not hold may stand anywhere.
    j.models[2].transform.position = [10.0, 0.0, 0.0];
    j.validate().unwrap();

    let e = refused(&|j| {
        j.stocks.push(StockDef {
            name: "block".into(),
            spec: StockSpec::default(),
        });
        j.setups[0].stocks = vec!["block".into()];
    });
    assert_eq!(
        e,
        JobError::SheetNotUsed {
            setup: "top".into(),
            model: "bracket".into(),
            stock: "sheet".into()
        }
    );
}
