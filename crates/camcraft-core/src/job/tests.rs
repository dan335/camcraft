use super::*;
use crate::cutter::tool::{Cutter, ToolProfile};
use crate::setup::frame::{Axis, Orientation};

const SAMPLE: &str = r#"
version = 1
tolerance = 0.02

[tools.rough-6-flat]
kind = "flat"
diameter = 6.0
number = 1
rpm = 16000
comment = "6mm flat, carbide"

[tools.finish-4-ball]
kind = "ball"
diameter = 4.0
number = 2

[[model]]
name = "part"
path = "part.step"

[[stock]]
name = "block"
kind = "around-models"
margin = [2.0, 2.0, 1.0]

[[setup]]
name = "top"
models = ["part"]
stocks = ["block"]
wcs = 54

[[setup.operation]]
name = "rough"
strategy = "pocket-clearing"
tool = "rough-6-flat"
boundary = { margin = 1.0 }
heights = { clearance = { ref = "stock-top", offset = 8.0 }, bottom = { ref = "model-bottom" } }
passes = { stepdown = 2.0, stepover = 3.0, feed = 1800.0, stock_to_leave = { radial = 0.5, axial = 0.2 } }
linking = { retract_feed = 1800.0, plunge_feed = 400.0 }

[[setup.operation]]
name = "finish"
strategy = "parallel"
tool = "finish-4-ball"
angle = 90.0
passes = { stepover = 0.4, feed = 2400.0 }
"#;

fn sample() -> Job {
    toml::from_str(SAMPLE).unwrap()
}

#[test]
fn parses_a_complete_job() {
    let j = sample();
    assert_eq!(j.tolerance, 0.02);
    assert_eq!(j.tools.len(), 2);
    assert_eq!(j.setups.len(), 1);
    assert_eq!(j.setups[0].operations.len(), 2);
    j.validate().unwrap();
}

#[test]
fn defaults_fill_in_what_is_omitted() {
    let minimal = r#"
version = 1
[tools.t]
kind = "ball"
diameter = 6.0
[[model]]
name = "m"
path = "m.step"
[[stock]]
name = "s"
kind = "around-models"
[[setup]]
name = "top"
models = ["m"]
stocks = ["s"]
[[setup.operation]]
name = "finish"
strategy = "parallel"
tool = "t"
"#;
    let j: Job = toml::from_str(minimal).unwrap();
    j.validate().unwrap();
    assert_eq!(j.tolerance, 0.01);
    let setup = &j.setups[0];
    assert_eq!(setup.wcs, 54);
    assert_eq!(setup.post, "masso");
    assert_eq!(setup.orientation, Orientation::default());
    assert_eq!(setup.origin, Origin::default());
    let op = setup.operations.first().unwrap();
    assert_eq!(op.passes.stepover, 1.0);
    assert!(op.linking.reorder);
    assert!(
        !op.linking.allow_reverse,
        "reversing flips climb for conventional and must be asked for"
    );
}

/// Everything a job can say survives being written and read back, and the
/// written file leaves out what is only default.
#[test]
fn a_job_round_trips() {
    let mut j = sample();
    j.stocks.push(StockDef {
        name: "plate".into(),
        spec: StockSpec::FixedBox(FixedBox::at([-5.0, -5.0, -12.5], [110.0, 60.0, 25.0])),
    });
    j.models.push(ModelDef {
        name: "stl".into(),
        path: "../meshes/other.stl".into(),
        units: Some(LengthUnit::Inch),
        transform: Transform {
            position: [10.0, -20.0, 0.5],
            rotation: [90.0, 0.0, -45.0],
            scale: [1.0, 1.0, 2.0],
        },
    });
    let mut bottom = Setup::new("bottom");
    bottom.models = vec!["part".into(), "stl".into()];
    bottom.stocks = vec!["block".into(), "plate".into()];
    bottom.wcs = 55;
    bottom.post = "profiles/mine.toml".into();
    bottom.orientation = Orientation {
        z: Axis::NegZ,
        x: Axis::PosY,
    };
    bottom.origin = Origin::BoxPoint {
        of: BoxOf::Stock,
        x: Along::Min,
        y: Along::Center,
        z: Along::Max,
        offset: [0.0, 0.0, -1.5],
    };
    j.stocks.push(StockDef {
        name: "board".into(),
        spec: StockSpec::FixedBox(FixedBox {
            size: [300.0, 200.0, 19.0],
            models: vec!["part".into()],
            place: [
                Place::Center {},
                Place::Min { offset: 5.0 },
                Place::At { coordinate: -1.0 },
            ],
        }),
    });
    let mut unfinished = j.setups[0].operations[1].clone();
    unfinished.tool = None;
    bottom.operations = vec![unfinished];
    j.setups.push(bottom);
    j.setups[0].origin = Origin::ModelOrigin {
        offset: [1.0, 2.0, 3.0],
    };

    let text = toml::to_string(&j).unwrap();
    let again: Job = toml::from_str(&text).unwrap();
    assert_eq!(again, j, "{text}");
    // Defaults are not written out.
    let plain = toml::to_string(&sample()).unwrap();
    assert!(!plain.contains("orientation"), "{plain}");
    assert!(!plain.contains("below"), "{plain}");
    assert!(!plain.contains("transform"), "{plain}");
}

/// A fixed box is written with a place on each axis, and read back the same.
#[test]
fn a_fixed_box_is_written_with_a_place_on_each_axis() {
    let mut j = sample();
    j.stocks.push(StockDef {
        name: "board".into(),
        spec: StockSpec::FixedBox(FixedBox::centred([10.0, 10.0, 10.0])),
    });
    let text = toml::to_string(&j.stocks[1]).unwrap();
    assert!(text.contains("from = \"center\""), "{text}");
    let again: StockDef = toml::from_str(&text).unwrap();
    assert_eq!(again, j.stocks[1]);
}

/// A setting the format does not have is refused, wherever it is written: an
/// operation's own settings and its strategy's are one table in the file, and a
/// stock's kind and its settings another, so those are checked too.
#[test]
fn unknown_settings_are_refused() {
    let refused = |from: &str, to: &str| {
        assert!(SAMPLE.contains(from), "{from}");
        let text = SAMPLE.replacen(from, to, 1);
        match toml::from_str::<Job>(&text) {
            Err(e) => assert!(e.to_string().contains("unknown field"), "{to}: {e}"),
            Ok(_) => panic!("read: {to}"),
        }
    };
    // An operation's stepdown belongs under its passes.
    refused("angle = 90.0\n", "angle = 90.0\nstepdown = 2.0\n");
    refused(
        "strategy = \"pocket-clearing\"\n",
        "strategy = \"pocket-clearing\"\nstepdwon = 2.0\n",
    );
    refused("plunge_feed = 400.0 }", "plunge_feed = 400.0, leeds = {} }");
    refused(
        "margin = [2.0, 2.0, 1.0]\n",
        "margin = [2.0, 2.0, 1.0]\nsize = 3.0\n",
    );
    refused("version = 1\n", "version = 1\nunits = \"mm\"\n");
    refused("number = 2\n", "number = 2\nflute = 2\n");
    refused(
        "wcs = 54\n",
        "wcs = 54\norigin = { kind = \"model-origin\", x = 1.0 }\n",
    );
    refused(
        "margin = [2.0, 2.0, 1.0]\n",
        "margin = [2.0, 2.0, 1.0]\n[[stock]]\nname = \"b\"\nkind = \"fixed-box\"\nsize = [1.0, 1.0, 1.0]\n\
         x = { from = \"center\", offset = 3.0 }\ny = { from = \"center\" }\nz = { from = \"center\" }\n",
    );
    // What is written where it belongs still reads.
    let fine = SAMPLE.replacen("angle = 90.0\n", "angle = 90.0\nenabled = true\n", 1);
    toml::from_str::<Job>(&fine).unwrap();
}

/// A profile's leads are Fusion's: a quarter turn, and every size a tenth of
/// the tool, unless given; and nothing past the default is written.
#[test]
fn leads_are_fusions_unless_given() {
    let l = Leads::default().sizes(6.35);
    assert_eq!((l.radius, l.straight, l.vertical), (0.635, 0.635, 0.635));
    assert_eq!(l.sweep, 90.0);
    assert_eq!(toml::to_string(&Leads::default()).unwrap(), "");
    let given = Leads {
        vertical: Some(0.0),
        ..Leads::default()
    };
    assert_eq!(given.sizes(6.35).vertical, 0.0, "zero for none");
    let text = SAMPLE.replacen(
        "strategy = \"parallel\"\n",
        "strategy = \"profile\"\nlinking = { entry = \"plunge\", leads = { straight = 1.0 } }\n",
        1,
    )
    .replace("angle = 90.0\n", "");
    let j: Job = toml::from_str(&text).unwrap();
    j.validate().unwrap();
    let linking = &j.setups[0].operations[1].linking;
    assert_eq!(linking.entry, Some(Entry::Plunge));
    assert_eq!(linking.leads.unwrap().straight, Some(1.0));
}

/// An entry the strategy does not offer is refused: adaptive clearing goes
/// down a helix or straight, and has no ring to ramp along.
#[test]
fn an_entry_not_offered_is_refused() {
    let adaptive = |entry: &str| {
        let text = SAMPLE
            .replacen(
                "strategy = \"parallel\"\n",
                &format!("strategy = \"adaptive\"\nlinking = {{ entry = \"{entry}\" }}\n"),
                1,
            )
            .replace("angle = 90.0\n", "")
            .replace(
                "passes = { stepover = 0.4, feed = 2400.0 }",
                "passes = { stepover = 0.4, feed = 2400.0, stepdown = 1.0 }",
            );
        toml::from_str::<Job>(&text).unwrap().validate()
    };
    adaptive("plunge").unwrap();
    assert!(matches!(
        adaptive("ramp"),
        Err(JobError::NotOffered {
            value: "ramp",
            strategy: "adaptive",
            ..
        })
    ));
}

/// A profile has Fusion's leads unless they are turned off, as Fusion's does;
/// off is written, and on at Fusion's sizes is not.
#[test]
fn leads_are_on_unless_turned_off() {
    let profile = |linking: &str| {
        let text = SAMPLE
            .replacen(
                "strategy = \"parallel\"\n",
                &format!("strategy = \"profile\"\n{linking}"),
                1,
            )
            .replace("angle = 90.0\n", "");
        let j: Job = toml::from_str(&text).unwrap();
        j.validate().unwrap();
        j.setups[0].operations[1].linking.clone()
    };
    assert_eq!(profile("").leads, Some(Leads::default()));
    assert_eq!(profile("linking = { leads = false }\n").leads, None);
    assert_eq!(
        profile("linking = { leads = true }\n").leads,
        Some(Leads::default())
    );
    let off = Linking {
        leads: None,
        ..Linking::default()
    };
    let text = toml::to_string(&off).unwrap();
    assert!(text.contains("leads = false"), "{text}");
    assert!(
        !toml::to_string(&Linking::default())
            .unwrap()
            .contains("leads")
    );
    let back: Linking = toml::from_str(&text).unwrap();
    assert_eq!(back, off);
    // A misspelt size is still refused.
    assert!(toml::from_str::<Linking>("leads = { radious = 1.0 }").is_err());
}

/// A fixed box needs a place on every axis.
#[test]
fn a_fixed_box_without_a_place_on_each_axis_is_refused() {
    let text =
        "kind = \"fixed-box\"\nname = \"p\"\nsize = [1.0, 1.0, 1.0]\nx = { from = \"center\" }\n";
    assert!(
        toml::from_str::<StockDef>(text).is_err(),
        "y and z are missing"
    );
}

/// A fixed box's place must be a number, and the models it is placed against
/// must be the job's.
#[test]
fn a_fixed_box_is_checked() {
    let mut j = sample();
    j.stocks.push(StockDef {
        name: "board".into(),
        spec: StockSpec::FixedBox(FixedBox {
            place: [
                Place::Min { offset: f64::NAN },
                Place::Center {},
                Place::Center {},
            ],
            ..FixedBox::centred([10.0, 10.0, 10.0])
        }),
    });
    assert!(matches!(j.validate(), Err(JobError::BadValue { .. })));
    j.stocks[1].spec = StockSpec::FixedBox(FixedBox {
        models: vec!["nothing".into()],
        ..FixedBox::centred([10.0, 10.0, 10.0])
    });
    assert!(matches!(j.validate(), Err(JobError::UnknownModel { .. })));
}

/// A boundary starts as the stock's, and is written only once chosen, and read
/// back the same.
#[test]
fn a_chosen_boundary_is_written() {
    let mut j = sample();
    assert_eq!(j.setups[0].operations[0].boundary.kind, Boundary::Stock);
    let text = toml::to_string(&j).unwrap();
    assert!(!text.contains("kind = \"stock\""), "{text}");
    j.setups[0].operations[0].boundary.kind = Boundary::Silhouette;
    let text = toml::to_string(&j).unwrap();
    assert!(text.contains("kind = \"silhouette\""), "{text}");
    let again: Job = toml::from_str(&text).unwrap();
    assert_eq!(again, j);
}

/// A file of another format is refused by name, not with a parse error about
/// some field.
#[test]
fn another_format_is_refused() {
    let mut j = sample();
    j.version = 2;
    assert_eq!(j.validate(), Err(JobError::UnsupportedVersion(2)));
}

/// The reason heights are symbolic: the same job survives a different block.
#[test]
fn heights_resolve_against_the_setup() {
    let ctx = HeightContext {
        stock_top: 20.0,
        stock_bottom: -5.0,
        model_top: 18.0,
        model_bottom: 0.0,
        picked: None,
    };
    let cases = [
        (HeightRef::StockTop, 8.0, 28.0),
        (HeightRef::StockBottom, 0.0, -5.0),
        (HeightRef::ModelTop, 2.0, 20.0),
        (HeightRef::ModelBottom, -1.0, -1.0),
        (HeightRef::Absolute, 12.5, 12.5),
    ];
    for (reference, offset, want) in cases {
        let h = Height { reference, offset };
        assert_eq!(h.resolve(&ctx), want, "{reference:?} + {offset}");
    }
}

/// A top measured from the picks is their highest point, a bottom their
/// lowest, each plus the offset; with none found, the model's.
#[test]
fn a_height_from_the_selection_reads_the_picks() {
    let h = Height {
        reference: HeightRef::Selection,
        offset: -0.5,
    };
    let mut ctx = HeightContext {
        stock_top: 20.0,
        stock_bottom: -5.0,
        model_top: 18.0,
        model_bottom: 0.0,
        picked: Picks::of([6.0, 4.0, 9.0]),
    };
    assert_eq!(h.resolve_top(&ctx), 8.5);
    assert_eq!(h.resolve_bottom(&ctx), 3.5);
    ctx.picked = None;
    assert_eq!(h.resolve_top(&ctx), 17.5);
    assert_eq!(h.resolve_bottom(&ctx), -0.5);
    assert_eq!(Picks::of([]), None);
}

/// A 2D operation's levels over one piece of its outline: a height from the
/// selection reads the piece's picks, or all of them where it has none; an
/// empty bottom is the piece's lowest pick only when the picks are the outline.
#[test]
fn a_2d_area_is_measured_from_its_own_picks() {
    let ctx = HeightContext {
        stock_top: 20.0,
        stock_bottom: -5.0,
        model_top: 18.0,
        model_bottom: 0.0,
        picked: Picks::of([2.0, 12.0]),
    };
    let own = Picks::of([5.0, 7.0]);
    let from_picks = |offset| {
        Some(Height {
            reference: HeightRef::Selection,
            offset,
        })
    };
    let mut op = sample().setups[0].operations[0].clone();
    op.outline = Outline::Selection;
    op.heights.bottom = None;
    assert_eq!(op.levels_2d(&ctx, own), (20.0, 5.0));
    assert_eq!(op.levels_2d(&ctx, None), (20.0, 0.0));
    op.heights.bottom = from_picks(-0.5);
    op.heights.top = from_picks(1.0);
    assert_eq!(op.levels_2d(&ctx, own), (8.0, 4.5));
    assert_eq!(op.levels_2d(&ctx, None), (13.0, 1.5));
    op.heights.bottom = None;
    op.outline = Outline::Silhouette;
    assert_eq!(op.levels_2d(&ctx, own).1, 0.0);
}

/// Only a top or a bottom is measured from the picks, only with something
/// picked, and never a drill's.
#[test]
fn heights_from_the_selection_need_picks() {
    let mut j = sample();
    let selection = Height {
        reference: HeightRef::Selection,
        offset: 0.0,
    };
    let op = &mut j.setups[0].operations[0];
    op.heights.bottom = Some(selection);
    assert!(matches!(j.validate(), Err(JobError::BadHeight { .. })));
    let op = &mut j.setups[0].operations[0];
    op.selection.edges.push(FeatureRef {
        model: "part".into(),
        index: 0,
        at: [0.0; 3],
    });
    j.validate().unwrap();
    let op = &mut j.setups[0].operations[0];
    op.heights.clearance = selection;
    assert!(matches!(j.validate(), Err(JobError::BadHeight { .. })));
    let op = &mut j.setups[0].operations[0];
    op.heights.clearance = Heights::default().clearance;
    op.strategy = StrategySpec::Drill {
        peck: None,
        dwell: None,
    };
    assert!(op.cannot_measure_from_picks().is_some());

    let text = toml::to_string(&j).unwrap();
    assert!(text.contains(r#"ref = "selection""#), "{text}");
}

#[test]
fn a_taller_block_moves_the_clearance_with_it() {
    let h = Height {
        reference: HeightRef::StockTop,
        offset: 10.0,
    };
    let short = HeightContext {
        stock_top: 10.0,
        stock_bottom: 0.0,
        model_top: 8.0,
        model_bottom: 0.0,
        picked: None,
    };
    let tall = HeightContext {
        stock_top: 50.0,
        ..short
    };
    assert_eq!(h.resolve(&short), 20.0);
    assert_eq!(h.resolve(&tall), 60.0);
}

#[test]
fn tools_build_into_the_bullnose_family() {
    let j = sample();
    let flat = j.tools.get("rough-6-flat").unwrap().build().unwrap();
    assert_eq!(flat.max_radius(), 3.0);
    let flat = flat.bullnose().unwrap();
    assert_eq!(flat.corner(), 0.0, "a flat mill has no corner");

    let ball = j.tools.get("finish-4-ball").unwrap().build().unwrap();
    let ball = ball.bullnose().unwrap();
    assert_eq!(ball.corner(), 2.0, "a ball mill's corner fills its radius");
}

fn vbit() -> ToolSpec {
    toml::from_str("kind = \"cone\"\ndiameter = 6.35\nangle = 60.0\nnumber = 3\n").unwrap()
}

/// A V-bit reads with its angle, builds into a cone, and names what is wrong
/// with it where it is wrong.
#[test]
fn a_vbit_is_a_cone() {
    let v = vbit();
    assert!(v.check().is_empty(), "{:?}", v.check());
    let Cutter::Cone(cone) = v.build().unwrap() else {
        panic!("not a cone");
    };
    assert!((cone.rise() - 3f64.sqrt()).abs() < 1e-12);
    let text = toml::to_string(&v).unwrap();
    assert!(
        text.contains("angle = 60.0") && !text.contains("tip_diameter"),
        "{text}"
    );

    let field = |edit: fn(&mut ToolSpec)| {
        let mut t = vbit();
        edit(&mut t);
        t.check().into_iter().map(|e| e.field).collect::<Vec<_>>()
    };
    assert_eq!(field(|t| t.angle = None), ["angle"]);
    assert_eq!(field(|t| t.angle = Some(180.0)), ["angle"]);
    assert_eq!(field(|t| t.tip_diameter = 7.0), ["tip_diameter"]);
}

/// A drill drills and nothing else, and a drilling operation takes a drill or a
/// flat end mill.
#[test]
fn drills_drill_and_only_drill() {
    let drill: ToolSpec = toml::from_str("kind = \"drill\"\ndiameter = 6.0\nnumber = 4\n").unwrap();
    assert!(drill.check().is_empty(), "{:?}", drill.check());
    let Cutter::Cone(point) = drill.build().unwrap() else {
        panic!("a drill's point is a cone");
    };
    assert!((point.rise() - 1.0 / 59f64.to_radians().tan()).abs() < 1e-12);
    assert_eq!(point.tip(), 0.0);

    let mut j = sample();
    j.tools.insert("d".into(), drill);
    j.setups[0].operations[1].tool = Some("d".into());
    assert!(matches!(j.validate(), Err(JobError::WrongTool { .. })));
    j.setups[0].operations[1].strategy = StrategySpec::Drill {
        peck: Some(2.0),
        dwell: None,
    };
    j.validate().unwrap();
    j.setups[0].operations[1].tool = Some("finish-4-ball".into());
    assert!(matches!(j.validate(), Err(JobError::WrongTool { .. })));
    j.setups[0].operations[1].tool = Some("d".into());
    j.setups[0].operations[1].strategy = StrategySpec::Drill {
        peck: Some(0.0),
        dwell: None,
    };
    assert!(matches!(j.validate(), Err(JobError::BadValue { .. })));

    let text = toml::to_string(&j).unwrap();
    assert!(text.contains(r#"strategy = "drill""#), "{text}");
}

/// Every operation has stock to leave, and cuts with as much of it as its
/// strategy uses: a drill none, a profile on the line only its floor's. What
/// it does not use is not checked, so a drill with some left over still runs.
#[test]
fn stock_to_leave_is_what_the_strategy_uses() {
    let mut j = sample();
    j.tools.insert(
        "d".into(),
        toml::from_str("kind = \"drill\"\ndiameter = 6.0\nnumber = 4\n").unwrap(),
    );
    let op = &mut j.setups[0].operations[0];
    let leave = op.passes.stock_to_leave;
    assert_eq!(op.stock_to_leave(), leave);
    op.strategy = StrategySpec::Profile {
        side: ProfileSide::On,
        conventional: false,
    };
    assert_eq!(op.strategy.uses().stock_to_leave, Leave::Axial);
    assert_eq!(op.stock_to_leave().radial, 0.0);
    assert_eq!(op.stock_to_leave().axial, leave.axial);
    op.strategy = StrategySpec::Drill {
        peck: None,
        dwell: None,
    };
    op.tool = Some("d".into());
    assert_eq!(op.stock_to_leave(), StockToLeave::default());
    assert_eq!(
        op.passes.stock_to_leave, leave,
        "kept, for a strategy that uses it"
    );
    j.validate().unwrap();
}

/// Negative stock to leave cuts past the part, as Fusion's does: by less than
/// the tool's radius beside, and in 3D by no more than a round end's corner
/// radius down. Anything but a number is refused.
#[test]
fn stock_to_leave_may_be_negative_within_the_tool() {
    let mut j = sample();
    // The finish is a 4 mm ball in 3D: radius and corner are both 2 mm.
    let leave = |j: &mut Job, radial: f64, axial: f64| {
        j.setups[0].operations[1].passes.stock_to_leave = StockToLeave { radial, axial };
        j.validate()
    };
    leave(&mut j, -0.1, -0.1).unwrap();
    leave(&mut j, -1.9, -2.0).unwrap();
    assert!(matches!(
        leave(&mut j, -2.0, 0.0),
        Err(JobError::WrongTool { .. })
    ));
    assert!(matches!(
        leave(&mut j, 0.0, -2.1),
        Err(JobError::WrongTool { .. })
    ));
    assert!(matches!(
        leave(&mut j, f64::NAN, 0.0),
        Err(JobError::BadValue { .. })
    ));
    leave(&mut j, 0.0, 0.0).unwrap();
    // A flat mill's end is flat, and a 2D floor any depth: down is not limited.
    let op = &mut j.setups[0].operations[0];
    op.passes.stock_to_leave = StockToLeave {
        radial: -0.5,
        axial: -5.0,
    };
    j.validate().unwrap();
    j.setups[0].operations[1].strategy = StrategySpec::Pocket {
        conventional: false,
    };
    leave(&mut j, 0.0, -5.0).unwrap();
}

/// A stepover wider than the tool leaves ridges between passes, and is refused;
/// a profile, which has none, is not held to its.
#[test]
fn a_stepover_wider_than_the_tool_is_refused() {
    let mut j = sample();
    // The finish is a 4 mm ball.
    j.setups[0].operations[1].passes.stepover = 4.0;
    j.validate().unwrap();
    j.setups[0].operations[1].passes.stepover = 4.5;
    assert!(matches!(j.validate(), Err(JobError::WrongTool { .. })));
    j.setups[0].operations[1].strategy = StrategySpec::Profile {
        side: ProfileSide::Outside,
        conventional: false,
    };
    j.validate().unwrap();
}

/// Roughing starts leaving half a millimetre for the finish, as Fusion's does;
/// the rest cut to size. None written is none left.
#[test]
fn roughing_leaves_stock_to_start_and_none_is_not_written() {
    let rough = StrategySpec::PocketClearing {
        min_ring_length: 3.0,
        conventional: false,
        flat_areas: true,
    };
    assert_eq!(
        rough.default_stock_to_leave(),
        StockToLeave {
            radial: 0.5,
            axial: 0.5
        }
    );
    assert_eq!(
        StrategySpec::Parallel { angle: 0.0 }.default_stock_to_leave(),
        StockToLeave::default()
    );
    let mut j = sample();
    j.setups[0].operations[0].passes.stock_to_leave = StockToLeave::default();
    let text = toml::to_string(&j).unwrap();
    assert!(!text.contains("stock_to_leave"), "{text}");
    let again: Job = toml::from_str(&text).unwrap();
    assert_eq!(again, j);
}

/// What each strategy uses of the settings every operation has.
#[test]
fn each_strategy_says_what_it_uses() {
    let parallel = StrategySpec::Parallel { angle: 0.0 }.uses();
    assert!(parallel.lead_radius && !parallel.tabs && !parallel.ramp && !parallel.leads);
    let pocket = StrategySpec::Pocket {
        conventional: false,
    }
    .uses();
    assert!(pocket.stepover && !pocket.tabs && pocket.ramp && !pocket.leads);
    let profile = StrategySpec::Profile {
        side: ProfileSide::Outside,
        conventional: false,
    }
    .uses();
    assert!(!profile.stepover && profile.tabs && profile.leads);
    assert_eq!(profile.entries, [Entry::Helix, Entry::Plunge, Entry::Ramp]);
    assert!(
        pocket.entries.is_empty() && parallel.entries.is_empty(),
        "a pocket and a finishing pass have no choice of entry"
    );
    // Fusion's 3D Adaptive has no ring to ramp along.
    let adaptive = StrategySpec::named("adaptive").unwrap().uses();
    assert_eq!(adaptive.entries, [Entry::Helix, Entry::Plunge]);
    assert_eq!(profile.stock_to_leave, Leave::Both);
    let drill = StrategySpec::Drill {
        peck: None,
        dwell: None,
    }
    .uses();
    assert_eq!(drill.stock_to_leave, Leave::None);
    assert!(!drill.stepover && !drill.tabs && !drill.stay_down && !drill.ramp);
}

/// A V-bit finishes, but neither roughs in Z levels nor leaves stock.
#[test]
fn a_vbit_is_refused_where_it_cannot_cut() {
    let mut j = sample();
    j.tools.insert("v".into(), vbit());
    j.tools.get_mut("v").unwrap().number = 5;
    j.setups[0].operations[1].tool = Some("v".into());
    j.validate().unwrap();
    j.setups[0].operations[1].passes.stock_to_leave.radial = 0.2;
    assert!(matches!(j.validate(), Err(JobError::WrongTool { .. })));
    j.setups[0].operations[1].passes.stock_to_leave.radial = 0.0;
    j.setups[0].operations[0].tool = Some("v".into());
    j.setups[0].operations[0].passes.stock_to_leave = StockToLeave::default();
    assert!(matches!(j.validate(), Err(JobError::WrongTool { .. })));
}

#[test]
fn an_unknown_tool_is_caught_before_anything_runs() {
    let bad = SAMPLE.replace(r#"tool = "finish-4-ball""#, r#"tool = "nonesuch""#);
    let j: Job = toml::from_str(&bad).unwrap();
    assert_eq!(
        j.validate(),
        Err(JobError::UnknownTool {
            op: "top/finish".into(),
            tool: "nonesuch".into()
        })
    );
}

/// A new operation has no tool; it saves, but it does not run.
#[test]
fn an_operation_without_a_tool_does_not_run() {
    let mut j = sample();
    j.setups[0].operations[1].tool = None;
    assert!(j.check_common().is_ok());
    assert_eq!(
        j.validate(),
        Err(JobError::NoTool {
            op: "top/finish".into()
        })
    );
}

/// Catchable mistakes are caught up front, not part-way through an operation
/// with a half-built path in hand.
#[test]
fn unusable_values_are_rejected_up_front() {
    for (from, to) in [
        ("stepover = 0.4", "stepover = 0.0"),
        ("feed = 2400.0", "feed = -1.0"),
        ("tolerance = 0.02", "tolerance = 0.0"),
        ("wcs = 54", "wcs = 99"),
        ("stepdown = 2.0", "stepdown = 0.0"),
        ("margin = [2.0, 2.0, 1.0]", "margin = [2.0, -2.0, 1.0]"),
    ] {
        let j: Job = toml::from_str(&SAMPLE.replace(from, to)).unwrap();
        assert!(j.validate().is_err(), "{to} should be rejected");
    }
}

/// Setups and stocks name what they use, and a name that is not there, or is
/// there twice, stops the job.
#[test]
fn references_must_resolve() {
    let cases = [
        (r#"models = ["part"]"#, r#"models = ["nope"]"#),
        (r#"stocks = ["block"]"#, r#"stocks = ["nope"]"#),
        (r#"models = ["part"]"#, "models = []"),
        (r#"stocks = ["block"]"#, "stocks = []"),
    ];
    for (from, to) in cases {
        let j: Job = toml::from_str(&SAMPLE.replace(from, to)).unwrap();
        assert!(j.validate().is_err(), "{to} should be rejected");
    }
    let mut j = sample();
    j.stocks[0].spec = StockSpec::AroundModels {
        models: vec!["ghost".into()],
        margin: [0.0; 3],
        below: 0.0,
    };
    assert!(matches!(
        j.check_common(),
        Err(JobError::UnknownModel { .. })
    ));
    let mut j = sample();
    j.setups.push(j.setups[0].clone());
    assert!(matches!(
        j.check_common(),
        Err(JobError::DuplicateName { what: "setup", .. })
    ));
}

/// A setup still being written does not stop another from running.
#[test]
fn one_setup_is_checked_on_its_own() {
    let mut j = sample();
    j.setups.push(Setup::new("bottom"));
    j.check_common().unwrap();
    j.check_setup(0).unwrap();
    assert_eq!(j.check_setup(1), Err(JobError::NoModels("bottom".into())));
    assert!(j.validate().is_err());
}

#[test]
fn a_setup_with_no_operations_is_refused() {
    let mut j = sample();
    j.setups[0].operations.clear();
    assert_eq!(j.validate(), Err(JobError::NoOperations("top".into())));
    j.setups.clear();
    assert_eq!(j.validate(), Err(JobError::NoSetups));
}

/// A model is placed anywhere, turned any way and scaled by anything more than
/// nothing; a flattened or mirrored one is refused, by name.
#[test]
fn a_model_transform_must_be_a_real_one() {
    let mut j = sample();
    j.models[0].transform = Transform {
        position: [1.0e4, -3.0, 0.0],
        rotation: [0.0, 180.0, 30.0],
        scale: [0.5, 0.5, 0.5],
    };
    assert_eq!(j.validate(), Ok(()));
    for scale in [0.0, -1.0, f64::NAN] {
        j.models[0].transform.scale = [1.0, scale, 1.0];
        assert!(
            matches!(&j.validate(), Err(JobError::BadValue { field, .. }) if field == "model 'part'.transform.scale"),
            "{scale}: {:?}",
            j.validate()
        );
    }
}

#[test]
fn a_bad_orientation_is_refused() {
    let mut j = sample();
    j.setups[0].orientation = Orientation {
        z: Axis::PosX,
        x: Axis::NegX,
    };
    assert!(matches!(j.validate(), Err(JobError::BadOrientation { .. })));
}

/// A setup needs its own models and those its stocks are around.
#[test]
fn a_setup_needs_the_models_its_stock_is_around() {
    let mut j = sample();
    j.models.push(ModelDef {
        name: "fixture".into(),
        path: "fixture.step".into(),
        units: None,
        transform: Default::default(),
    });
    j.stocks[0].spec = StockSpec::AroundModels {
        models: vec!["fixture".into()],
        margin: [0.0; 3],
        below: 0.0,
    };
    let names: Vec<&str> = j.models_for(0).iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["part", "fixture"]);
}

#[test]
fn strategy_parameters_belong_to_their_strategy() {
    let j = sample();
    let ops = &j.setups[0].operations;
    match ops[0].strategy {
        StrategySpec::PocketClearing { .. } => assert_eq!(ops[0].stepdown(), Some(2.0)),
        _ => panic!("first operation should be roughing"),
    }
    match ops[1].strategy {
        StrategySpec::Parallel { angle } => assert_eq!(angle, 90.0),
        _ => panic!("second operation should be finishing"),
    }
}

/// Every shipped example parses and validates.
#[test]
fn the_examples_validate() {
    let job: Job = toml::from_str(include_str!("../../../../examples/rail-jig.camcraft")).unwrap();
    job.validate().unwrap();
    sample().validate().unwrap();
}

const FULL_TOOL: &str = r#"
kind = "bullnose"
diameter = 6.0
corner = 0.5
number = 3
rpm = 16000
comment = "6mm bullnose"
flutes = 2
flute_length = 15.0
shoulder_length = 20.0
shank_diameter = 6.0
overall_length = 50.0
stickout = 25.0
material = "carbide"
vendor = "Amana"
product = "46170-K"
holder = { name = "ER20", segments = [{ diameter = 32.0, length = 20.0 }, { diameter = 40.0, length = 30.0 }] }
spindle = "counter-clockwise"
coolant = "flood"

[[presets]]
name = "Roughing"
rpm = 16000
feed = 2000.0
plunge_feed = 400.0
ramp_feed = 700.0
stepdown = 3.0
stepover = 2.5
stock_to_leave = { radial = 0.3, axial = 0.2 }

[[presets]]
name = "Finishing"
comment = "light and slow"
stepover = 0.5
stock_to_leave = { radial = 0.0, axial = 0.0 }
"#;

/// A tool described the way Fusion describes one reads, checks clean and writes
/// back unchanged, which is what copying between a library and a job relies on.
#[test]
fn a_fully_described_tool_round_trips() {
    let tool: ToolSpec = toml::from_str(FULL_TOOL).unwrap();
    assert!(tool.check().is_empty(), "{:?}", tool.check());
    assert_eq!(tool.material, Some(ToolMaterial::Carbide));
    assert_eq!(tool.holder.as_ref().unwrap().segments.len(), 2);
    let again: ToolSpec = toml::from_str(&toml::to_string(&tool).unwrap()).unwrap();
    assert_eq!(again, tool);
    // Absent stays absent: a minimal tool writes only what it has.
    let minimal = toml::to_string(&ToolSpec::default()).unwrap();
    assert!(
        !minimal.contains("flutes")
            && !minimal.contains("corner")
            && !minimal.contains("spindle")
            && !minimal.contains("coolant"),
        "{minimal}"
    );
    assert_eq!(tool.spindle, SpindleDirection::CounterClockwise);
    assert_eq!(tool.coolant, Coolant::Flood);
}

fn with(edit: impl FnOnce(&mut ToolSpec)) -> Vec<&'static str> {
    let mut tool: ToolSpec = toml::from_str(FULL_TOOL).unwrap();
    edit(&mut tool);
    tool.check().into_iter().map(|e| e.field).collect()
}

/// Each rule names the field it is about, so an editor can show it there.
#[test]
fn tool_checks_name_the_field_at_fault() {
    assert_eq!(with(|t| t.flutes = Some(0)), ["flutes"]);
    assert_eq!(with(|t| t.flute_length = Some(30.0)), ["flute_length"]);
    assert_eq!(with(|t| t.stickout = Some(60.0)), ["stickout"]);
    assert_eq!(with(|t| t.shank_diameter = Some(-1.0)), ["shank_diameter"]);
    assert_eq!(with(|t| t.corner = 4.0), ["corner"]);
    assert_eq!(with(|t| t.diameter = 0.0), ["diameter"]);
    assert_eq!(with(|t| t.rpm = 0), ["rpm"]);
    assert_eq!(with(|t| t.number = 0), ["number"]);
    assert_eq!(with(|t| t.presets[0].feed = Some(0.0)), ["presets.feed"]);
    assert_eq!(with(|t| t.presets[1].rpm = Some(0)), ["presets.rpm"]);
    assert_eq!(with(|t| t.presets[1].name = " ".into()), ["presets.name"]);
    assert_eq!(
        with(|t| t.presets[1].name = "Roughing".into()),
        ["presets.name"]
    );
    assert_eq!(
        with(|t| t.presets[0].stock_to_leave.as_mut().unwrap().axial = f64::NAN),
        ["presets.stock_to_leave"]
    );
    assert_eq!(
        with(|t| t.holder.as_mut().unwrap().segments[0].length = 0.0),
        ["holder"]
    );
    // Missing lengths are not wrong, only unknown.
    assert!(with(|t| t.shoulder_length = None).is_empty());
}

/// A bad field stops the job, as a bad diameter always has.
#[test]
fn a_bad_tool_field_stops_the_job() {
    let bad = SAMPLE.replace(
        "[tools.rough-6-flat]\n",
        "[tools.rough-6-flat]\nflutes = 0\n",
    );
    assert_ne!(bad, SAMPLE, "the edit must land");
    let job: Job = toml::from_str(&bad).unwrap();
    assert!(matches!(
        job.validate(),
        Err(JobError::BadToolField { ref source, .. }) if source.field == "flutes"
    ));
}

/// Two tools the job uses may not share a number, in any setup; a spare in the
/// table may, and one tool used by two operations is one tool.
#[test]
fn two_used_tools_may_not_share_a_number() {
    let clash = SAMPLE.replace("number = 2", "number = 1");
    assert_ne!(clash, SAMPLE, "the edit must land");
    let job: Job = toml::from_str(&clash).unwrap();
    assert!(matches!(
        job.validate(),
        Err(JobError::DuplicateToolNumber { number: 1, .. })
    ));

    let spare = format!("{SAMPLE}\n[tools.spare]\nkind = \"flat\"\ndiameter = 3.0\nnumber = 1\n");
    let job: Job = toml::from_str(&spare).unwrap();
    job.validate().unwrap();

    let mut shared = sample();
    let tool = shared.setups[0].operations[0].tool.clone();
    shared.setups[0].operations[1].tool = tool;
    shared.validate().unwrap();

    // Across setups: the machine has one tool table.
    let mut two = sample();
    let mut other = two.setups[0].clone();
    other.name = "bottom".into();
    two.setups.push(other);
    two.tools.get_mut("finish-4-ball").unwrap().number = 1;
    two.setups[0].operations.truncate(1);
    two.setups[1].operations.remove(0);
    assert!(matches!(
        two.validate(),
        Err(JobError::DuplicateToolNumber { number: 1, .. })
    ));
}

fn picked() -> Job {
    let mut j = sample();
    let s = &mut j.setups[0];
    s.origin = Origin::Point {
        point: PointRef {
            model: "part".into(),
            snap: Snap::CircleCenter,
            index: 41,
            at: [12.7, 6.35, 25.4],
        },
        offset: [0.0, 0.0, 1.0],
    };
    let op = &mut s.operations[1];
    op.selection.faces.push(FeatureRef {
        model: "part".into(),
        index: 12,
        at: [30.2, 14.0, 12.7],
    });
    op.selection.edges.push(FeatureRef {
        model: "part".into(),
        index: 3,
        at: [1.0, 2.0, 3.0],
    });
    op.boundary.containment = Containment::Inside;
    j
}

/// Picks survive being written and read back, and are written as the plan's
/// TOML shows them.
#[test]
fn picks_round_trip() {
    let j = picked();
    j.validate().unwrap();
    let text = toml::to_string(&j).unwrap();
    let again: Job = toml::from_str(&text).unwrap();
    assert_eq!(again, j, "{text}");
    assert!(text.contains(r#"kind = "point""#), "{text}");
    assert!(text.contains(r#"snap = "circle-center""#), "{text}");
    assert!(text.contains(r#"containment = "inside""#), "{text}");
    assert!(
        text.contains("[[setup.operation.selection.faces]]"),
        "{text}"
    );
    // Nothing picked, nothing written.
    let plain = toml::to_string(&sample()).unwrap();
    assert!(!plain.contains("faces"), "{plain}");
    assert!(!plain.contains("containment"), "{plain}");
}

/// An operation's own spindle speed is taken, but not a stopped one.
#[test]
fn an_operation_speed_must_turn() {
    let mut j = sample();
    j.setups[0].operations[1].passes.rpm = Some(12000);
    j.validate().unwrap();
    j.setups[0].operations[1].passes.rpm = Some(0);
    assert!(matches!(j.validate(), Err(JobError::BadValue { .. })));
}

/// Fusion's stepovers: a quarter of a square end's width for parallel, the
/// tolerance's cusp for a round one, and 0.6 of the tool for clearing.
#[test]
fn stepovers_default_as_fusions() {
    let j = sample();
    let mut op = j.setups[0].operations[1].clone();
    let tool = |kind, diameter, corner| ToolSpec {
        kind,
        diameter,
        corner,
        ..j.tools["rough-6-flat"].clone()
    };
    op.strategy = StrategySpec::Parallel { angle: 0.0 };
    assert_eq!(
        op.default_stepover(&tool(ToolKind::Flat, 6.35, 0.0), 0.05),
        Some(1.588)
    );
    // r = 3.175: 2·√(3.175² − 3.125²) / √2.
    assert_eq!(
        op.default_stepover(&tool(ToolKind::Ball, 6.35, 0.0), 0.05),
        Some(0.794)
    );
    op.strategy = StrategySpec::PocketClearing {
        min_ring_length: 3.0,
        conventional: false,
        flat_areas: true,
    };
    let clearing = op
        .default_stepover(&tool(ToolKind::Flat, 6.35, 0.0), 0.05)
        .unwrap();
    assert!((clearing - 3.81).abs() < 1e-12);
    op.strategy = StrategySpec::Drill {
        peck: None,
        dwell: None,
    };
    assert_eq!(
        op.default_stepover(&tool(ToolKind::Drill, 6.0, 0.0), 0.05),
        None
    );
}

fn preset_tool() -> ToolSpec {
    toml::from_str(FULL_TOOL).unwrap()
}

/// Choosing a preset copies what it has into the operation and records it; a
/// preset with only some values leaves the others as they were.
#[test]
fn a_preset_sets_only_what_it_has() {
    let tool = preset_tool();
    let mut op = sample().setups[0].operations[0].clone();
    op.apply_preset(tool.preset("Roughing").unwrap());
    assert_eq!(op.preset.as_deref(), Some("Roughing"));
    assert_eq!(op.passes.rpm, Some(16000));
    assert_eq!(op.passes.feed, 2000.0);
    assert_eq!(op.linking.plunge_feed, 400.0);
    assert_eq!(op.linking.ramp_feed, Some(700.0));
    assert_eq!(op.passes.stepover, 2.5);
    assert_eq!(op.stepdown(), Some(3.0));
    assert_eq!(op.passes.stock_to_leave.radial, 0.3);

    op.apply_preset(tool.preset("Finishing").unwrap());
    assert_eq!(op.preset.as_deref(), Some("Finishing"));
    assert_eq!(op.passes.stepover, 0.5);
    assert_eq!(op.passes.stock_to_leave, StockToLeave::default());
    // Finishing has no feeds or speed, so Roughing's stay.
    assert_eq!(op.passes.feed, 2000.0);
    assert_eq!(op.passes.rpm, Some(16000));
}

#[test]
fn a_pick_must_be_on_the_setup_and_a_point() {
    let mut j = picked();
    j.setups[0].operations[1].boundary.kind = Boundary::Selection;
    j.setups[0].operations[1].selection.faces[0].model = "elsewhere".into();
    assert!(matches!(
        j.validate(),
        Err(JobError::PickedFromOtherModel { model, .. }) if model == "elsewhere"
    ));
    let mut j = picked();
    if let Origin::Point { point, .. } = &mut j.setups[0].origin {
        point.at[1] = f64::NAN;
    }
    assert!(matches!(j.validate(), Err(JobError::BadPick { .. })));
}

#[test]
fn renaming_a_model_carries_its_picks() {
    let mut j = picked();
    j.rename_model_refs("part", "bracket");
    let s = &j.setups[0];
    assert!(matches!(&s.origin, Origin::Point { point, .. } if point.model == "bracket"));
    assert!(
        s.operations[1]
            .selection
            .iter()
            .all(|r| r.model == "bracket")
    );
    j.rename_model_refs("nothing", "else");
    assert_eq!(
        j.setups[0].operations[1].selection.faces[0].model,
        "bracket"
    );
}

#[test]
fn an_origin_keeps_its_kind_when_its_offset_changes() {
    let o = picked().setups[0].origin.clone();
    assert_eq!(o.offset(), [0.0, 0.0, 1.0]);
    let moved = o.clone().with_offset([1.0, 2.0, 3.0]);
    assert_eq!(moved.offset(), [1.0, 2.0, 3.0]);
    assert!(matches!(moved, Origin::Point { .. }));
}

/// A profile and a pocket read from a job as Fusion's 2D operations are set,
/// take a preset's stepdown, and are written back the same. A V-bit profiles
/// only on the line, and pockets nothing.
#[test]
fn profiles_and_pockets_read_as_fusions() {
    let text = SAMPLE.replace(
        "strategy = \"parallel\"",
        "strategy = \"profile\"\nside = \"inside\"\nconventional = true",
    );
    // A profile has no angle.
    let text = text.replace("angle = 90.0\n", "");
    let mut j: Job = toml::from_str(&text).unwrap();
    j.validate().unwrap();
    assert_eq!(
        j.setups[0].operations[1].strategy,
        StrategySpec::Profile {
            side: ProfileSide::Inside,
            conventional: true,
        }
    );
    let again: Job = toml::from_str(&toml::to_string(&j).unwrap()).unwrap();
    assert_eq!(again, j);

    let op = &mut j.setups[0].operations[1];
    op.strategy = StrategySpec::Profile {
        side: ProfileSide::default(),
        conventional: false,
    };
    let text = toml::to_string(&j).unwrap();
    assert!(!text.contains("side ="), "outside is the default: {text}");
    assert!(!text.contains("conventional"), "{text}");

    let op = &mut j.setups[0].operations[1];
    op.strategy = StrategySpec::Pocket {
        conventional: false,
    };
    let deep = Preset {
        name: "Deep".into(),
        comment: String::new(),
        rpm: None,
        feed: None,
        plunge_feed: None,
        ramp_feed: None,
        stepdown: Some(3.0),
        stepover: None,
        stock_to_leave: None,
    };
    // Straight to the bottom stays so; multiple depths take the preset's.
    op.apply_preset(&deep);
    assert_eq!(op.stepdown(), None);
    op.passes.stepdown = Some(1.0);
    op.apply_preset(&deep);
    assert_eq!(op.stepdown(), Some(3.0));
    op.passes.stepdown = Some(0.0);
    assert!(matches!(j.validate(), Err(JobError::BadValue { .. })));
    j.setups[0].operations[1].passes.stepdown = None;

    j.tools.insert("v".into(), vbit());
    let op = &mut j.setups[0].operations[1];
    op.tool = Some("v".into());
    op.passes.stock_to_leave = Default::default();
    op.strategy = StrategySpec::Pocket {
        conventional: false,
    };
    assert!(matches!(j.validate(), Err(JobError::WrongTool { .. })));
    j.setups[0].operations[1].strategy = StrategySpec::Profile {
        side: ProfileSide::Outside,
        conventional: false,
    };
    assert!(matches!(j.validate(), Err(JobError::WrongTool { .. })));
    j.setups[0].operations[1].strategy = StrategySpec::Profile {
        side: ProfileSide::On,
        conventional: false,
    };
    j.validate().unwrap();
}

/// A 2D operation cuts round an outline, the silhouette unless the picks are
/// chosen, as Fusion's 2D Contour and Pocket select only what they cut; a 3D one
/// is kept to a boundary. Each strategy uses one or neither, never both.
#[test]
fn a_2d_operation_has_an_outline_and_a_3d_one_a_boundary() {
    assert_eq!(Outline::default(), Outline::Silhouette);
    let text = toml::to_string(&sample()).unwrap();
    assert!(!text.contains("outline"), "{text}");
    let strategies = [
        StrategySpec::Parallel { angle: 0.0 },
        StrategySpec::PocketClearing {
            min_ring_length: 3.0,
            conventional: false,
            flat_areas: true,
        },
        StrategySpec::Profile {
            side: ProfileSide::Outside,
            conventional: false,
        },
        StrategySpec::Pocket {
            conventional: false,
        },
        StrategySpec::Face {
            angle: 0.0,
            direction: FaceDirection::BothWays,
            pass_extension: 0.0,
            stock_offset: 0.0,
        },
        StrategySpec::Drill {
            peck: None,
            dwell: None,
        },
    ];
    for s in strategies {
        let u = s.uses();
        assert!(!(u.boundary && u.outline), "{}", s.name());
        let flat = matches!(
            s,
            StrategySpec::Profile { .. } | StrategySpec::Pocket { .. }
        );
        assert_eq!(u.outline, flat, "{}", s.name());
    }
}

/// Multiple depths are one setting every operation has, used as its strategy
/// says: roughing always cuts in levels, and without a stepdown is refused; 2D
/// cutting and facing go straight to the bottom unless asked; finishing and
/// drilling have none, whatever is written.
#[test]
fn multiple_depths_are_shared_and_used_as_the_strategy_says() {
    let mut j = sample();
    let rough = &mut j.setups[0].operations[0];
    assert_eq!(rough.strategy.uses().depths, Depths::Required);
    rough.passes.stepdown = None;
    assert!(matches!(j.validate(), Err(JobError::NoStepdown { .. })));

    let mut j = sample();
    let finish = &mut j.setups[0].operations[1];
    assert_eq!(finish.strategy.uses().depths, Depths::None);
    finish.passes.stepdown = Some(0.0);
    assert_eq!(finish.stepdown(), None, "not read");
    j.validate().unwrap();

    for (s, starts) in [
        (StrategySpec::Parallel { angle: 0.0 }, None),
        (
            StrategySpec::PocketClearing {
                min_ring_length: 3.0,
                conventional: false,
                flat_areas: true,
            },
            Some(2.0),
        ),
        (
            StrategySpec::Pocket {
                conventional: false,
            },
            Some(2.0),
        ),
        (
            StrategySpec::Face {
                angle: 0.0,
                direction: FaceDirection::BothWays,
                pass_extension: 0.0,
                stock_offset: 0.0,
            },
            None,
        ),
        (
            StrategySpec::Drill {
                peck: None,
                dwell: None,
            },
            None,
        ),
    ] {
        assert_eq!(s.default_stepdown(), starts, "{}", s.name());
    }
}

/// Facing reads with Fusion's defaults, both ways at the bottom alone, writes
/// back only what differs from them. A ball end mill or a
/// V-bit cannot face flat; a pass extension below zero is refused.
#[test]
fn facing_reads_with_fusions_defaults() {
    let text = SAMPLE
        .replace(
            "strategy = \"pocket-clearing\"\ntool = \"rough-6-flat\"",
            "strategy = \"face\"\ntool = \"rough-6-flat\"",
        )
        .replace("passes = { stepdown = 2.0, ", "passes = { ");
    let mut j: Job = toml::from_str(&text).unwrap();
    j.validate().unwrap();
    let face = StrategySpec::Face {
        angle: 0.0,
        direction: FaceDirection::BothWays,
        pass_extension: 0.0,
        stock_offset: 0.0,
    };
    assert_eq!(j.setups[0].operations[0].strategy, face);
    let written = toml::to_string(&j).unwrap();
    assert!(!written.contains("direction"), "{written}");
    assert!(!written.contains("pass_extension"), "{written}");
    let again: Job = toml::from_str(&written).unwrap();
    assert_eq!(again, j);

    let op = &mut j.setups[0].operations[0];
    assert_eq!(op.stepdown(), None);
    op.strategy = StrategySpec::Face {
        angle: 0.0,
        direction: FaceDirection::Climb,
        pass_extension: 0.0,
        stock_offset: 0.0,
    };
    op.passes.stepdown = Some(1.0);
    assert_eq!(op.stepdown(), Some(1.0));
    let written = toml::to_string(&j).unwrap();
    assert!(written.contains("direction = \"climb\""), "{written}");
    assert!(written.contains("stepdown = 1.0"), "{written}");
    assert_eq!(toml::from_str::<Job>(&written).unwrap(), j);

    let rough = j.tools["rough-6-flat"].clone();
    let op = &mut j.setups[0].operations[0];
    assert_eq!(
        op.default_stepover(&rough, 0.01),
        Some(0.95 * rough.diameter)
    );
    let ball = j.tools["finish-4-ball"].clone();
    assert!(op.cannot_use(&ball).is_some());
    op.strategy = StrategySpec::Face {
        angle: 0.0,
        direction: FaceDirection::BothWays,
        pass_extension: -1.0,
        stock_offset: 0.0,
    };
    assert!(j.validate().is_err());
}

/// Pocket clearing reads with its defaults, and is written back with them left
/// out; a new one starts from them.
#[test]
fn pocket_clearing_reads_with_its_defaults() {
    let j: Job = toml::from_str(SAMPLE).unwrap();
    let clearing = StrategySpec::PocketClearing {
        min_ring_length: 3.0,
        conventional: false,
        flat_areas: true,
    };
    assert_eq!(j.setups[0].operations[0].strategy, clearing);
    assert_eq!(StrategySpec::named("pocket-clearing"), Some(clearing));
    let written = toml::to_string(&j).unwrap();
    assert!(!written.contains("conventional"), "{written}");
    assert!(!written.contains("flat_areas"), "{written}");
    assert_eq!(toml::from_str::<Job>(&written).unwrap(), j);

    let set = SAMPLE.replace(
        "strategy = \"pocket-clearing\"\n",
        "strategy = \"pocket-clearing\"\nconventional = true\nflat_areas = false\n",
    );
    let j: Job = toml::from_str(&set).unwrap();
    assert_eq!(
        j.setups[0].operations[0].strategy,
        StrategySpec::PocketClearing {
            min_ring_length: 3.0,
            conventional: true,
            flat_areas: false,
        }
    );
    assert_eq!(
        toml::from_str::<Job>(&toml::to_string(&j).unwrap()).unwrap(),
        j
    );
}

/// Every strategy is found by the name a job file gives it, as a file that gives
/// none of its parameters has it; nothing else is.
#[test]
fn every_strategy_is_named() {
    for name in [
        "parallel",
        "scallop",
        "contour",
        "flat",
        "spiral",
        "radial",
        "morphed-spiral",
        "pocket-clearing",
        "adaptive",
        "profile",
        "pocket",
        "face",
        "drill",
    ] {
        let spec = StrategySpec::named(name).unwrap();
        assert_eq!(spec.name(), name);
    }
    assert_eq!(
        StrategySpec::named("radial"),
        Some(StrategySpec::Radial {
            centre: None,
            inner_radius: 0.0,
            outer_radius: 0.0,
            angle_from: 0.0,
            angle_to: 360.0,
            direction: RadialDirection::BothWays,
            slope_from: 0.0,
            slope_to: 90.0,
        })
    );
    assert_eq!(StrategySpec::named("zlevel-rough"), None);
}

/// Scallop reads with Fusion's defaults, both ways from the boundary in on
/// every slope, and writes back only what differs; its stepover is parallel's,
/// its order its own, and a slope range that is no range is refused.
#[test]
fn scallop_reads_with_fusions_defaults() {
    let text = SAMPLE.replace(
        "strategy = \"parallel\"\ntool = \"finish-4-ball\"\nangle = 90.0\n",
        "strategy = \"scallop\"\ntool = \"finish-4-ball\"\n",
    );
    let mut j: Job = toml::from_str(&text).unwrap();
    j.validate().unwrap();
    let scallop = StrategySpec::Scallop {
        direction: CutDirection::BothWays,
        order: PassOrder::OutsideIn,
        slope_from: 0.0,
        slope_to: 90.0,
    };
    assert_eq!(j.setups[0].operations[1].strategy, scallop);
    let written = toml::to_string(&j).unwrap();
    assert!(written.contains("strategy = \"scallop\""), "{written}");
    for default in ["direction", "order", "slope_from", "slope_to"] {
        let key = format!(
            "
{default} ="
        );
        assert!(!written.contains(&key), "{default} written: {written}");
    }
    assert_eq!(toml::from_str::<Job>(&written).unwrap(), j);

    let op = &mut j.setups[0].operations[1];
    assert!(!op.strategy.uses().reorder);
    let ball = j.tools["finish-4-ball"].clone();
    let parallel = Operation {
        strategy: StrategySpec::Parallel { angle: 0.0 },
        ..op.clone()
    };
    assert_eq!(
        op.default_stepover(&ball, 0.01),
        parallel.default_stepover(&ball, 0.01)
    );

    op.strategy = StrategySpec::Scallop {
        direction: CutDirection::Climb,
        order: PassOrder::InsideOut,
        slope_from: 10.0,
        slope_to: 60.0,
    };
    let written = toml::to_string(&j).unwrap();
    assert!(written.contains("direction = \"climb\""), "{written}");
    assert!(written.contains("order = \"inside-out\""), "{written}");
    assert_eq!(toml::from_str::<Job>(&written).unwrap(), j);

    for (from, to) in [(60.0, 10.0), (0.0, 95.0), (-5.0, 30.0)] {
        j.setups[0].operations[1].strategy = StrategySpec::Scallop {
            direction: CutDirection::BothWays,
            order: PassOrder::OutsideIn,
            slope_from: from,
            slope_to: to,
        };
        assert!(j.validate().is_err(), "{from}..{to} accepted");
    }
    let unknown = text.replace(
        "strategy = \"scallop\"\n",
        "strategy = \"scallop\"\nangle = 45.0\n",
    );
    assert!(toml::from_str::<Job>(&unknown).is_err());
}

/// The finishing strategies after Fusion's Contour, Flat, Spiral, Radial and
/// Morphed Spiral each read with Fusion's defaults and write back only what
/// differs from them; each refuses another's settings.
#[test]
fn more_finishing_strategies_read_with_fusions_defaults() {
    let with = |strategy: &str, extra: &str, passes: &str| {
        SAMPLE.replace(
            "strategy = \"parallel\"\ntool = \"finish-4-ball\"\nangle = 90.0\npasses = { stepover = 0.4, feed = 2400.0 }\n",
            &format!("strategy = \"{strategy}\"\ntool = \"finish-4-ball\"\n{extra}passes = {{ {passes} }}\n"),
        )
    };
    let cases = [
        (
            with("contour", "", "stepdown = 0.5, feed = 2400.0"),
            StrategySpec::Contour {
                direction: CutDirection::Climb,
                order: ContourOrder::ByDepth,
                slope_from: 0.0,
                slope_to: 90.0,
            },
        ),
        (
            with("flat", "", "stepover = 1.5, feed = 2400.0"),
            StrategySpec::Flat {
                direction: CutDirection::Climb,
                order: PassOrder::OutsideIn,
            },
        ),
        (
            with("spiral", "", "stepover = 0.4, feed = 2400.0"),
            StrategySpec::Spiral {
                centre: None,
                inner_radius: 0.0,
                outer_radius: 0.0,
                direction: CutDirection::Climb,
                order: PassOrder::OutsideIn,
                slope_from: 0.0,
                slope_to: 90.0,
            },
        ),
        (
            with("radial", "", "stepover = 0.4, feed = 2400.0"),
            StrategySpec::Radial {
                centre: None,
                inner_radius: 0.0,
                outer_radius: 0.0,
                angle_from: 0.0,
                angle_to: 360.0,
                direction: RadialDirection::BothWays,
                slope_from: 0.0,
                slope_to: 90.0,
            },
        ),
        (
            with("morphed-spiral", "", "stepover = 0.4, feed = 2400.0"),
            StrategySpec::MorphedSpiral {
                centre: None,
                direction: CutDirection::Climb,
                order: PassOrder::OutsideIn,
                slope_from: 0.0,
                slope_to: 90.0,
            },
        ),
    ];
    for (text, spec) in cases {
        let j: Job = toml::from_str(&text).unwrap();
        j.validate().unwrap();
        assert_eq!(j.setups[0].operations[1].strategy, spec);
        let written = toml::to_string(&j).unwrap();
        assert!(
            written.contains(&format!("strategy = \"{}\"", spec.name())),
            "{written}"
        );
        for default in [
            "direction",
            "order",
            "slope_from",
            "slope_to",
            "centre",
            "inner_radius",
            "outer_radius",
            "angle_from",
            "angle_to",
        ] {
            assert!(
                !written.contains(&format!("\n{default} =")),
                "{default} written: {written}"
            );
        }
        assert_eq!(toml::from_str::<Job>(&written).unwrap(), j);
        // Scallop's angle is not theirs.
        let unknown = text.replace(
            &format!("strategy = \"{}\"\n", spec.name()),
            &format!("strategy = \"{}\"\nangle = 45.0\n", spec.name()),
        );
        assert!(toml::from_str::<Job>(&unknown).is_err(), "{}", spec.name());
    }
}

/// What each new finishing strategy uses of the settings every operation has,
/// and the stepover and stepdown it starts with.
#[test]
fn more_finishing_strategies_use_what_fusions_do() {
    let j = sample();
    let ball = j.tools["finish-4-ball"].clone();
    let op = |strategy| Operation {
        strategy,
        ..j.setups[0].operations[1].clone()
    };
    let contour = op(StrategySpec::Contour {
        direction: CutDirection::Climb,
        order: ContourOrder::ByArea,
        slope_from: 30.0,
        slope_to: 90.0,
    });
    assert_eq!(contour.strategy.uses().depths, Depths::Required);
    assert!(!contour.strategy.uses().stepover);
    assert_eq!(contour.strategy.default_stepdown(), Some(1.0));
    assert_eq!(contour.default_stepover(&ball, 0.01), None);
    let flat = op(StrategySpec::Flat {
        direction: CutDirection::Conventional,
        order: PassOrder::InsideOut,
    });
    assert!(flat.strategy.conventional());
    assert_eq!(flat.strategy.uses().depths, Depths::None);
    assert_eq!(
        flat.default_stepover(&ball, 0.01),
        Some(0.6 * ball.diameter)
    );
    let parallel = op(StrategySpec::Parallel { angle: 0.0 });
    for strategy in [
        StrategySpec::Spiral {
            centre: Some([1.0, 2.0]),
            inner_radius: 1.0,
            outer_radius: 10.0,
            direction: CutDirection::Climb,
            order: PassOrder::InsideOut,
            slope_from: 0.0,
            slope_to: 90.0,
        },
        StrategySpec::Radial {
            centre: None,
            inner_radius: 0.0,
            outer_radius: 0.0,
            angle_from: 0.0,
            angle_to: 180.0,
            direction: RadialDirection::Outward,
            slope_from: 0.0,
            slope_to: 90.0,
        },
        StrategySpec::MorphedSpiral {
            centre: None,
            direction: CutDirection::Climb,
            order: PassOrder::OutsideIn,
            slope_from: 0.0,
            slope_to: 45.0,
        },
    ] {
        let o = op(strategy);
        assert_eq!(
            o.default_stepover(&ball, 0.01),
            parallel.default_stepover(&ball, 0.01)
        );
        assert!(!o.strategy.uses().reorder);
        assert!(o.strategy.slopes().is_some());
    }
}

/// A pattern about a centre is refused a radius or an angle it cannot have.
#[test]
fn patterns_about_a_centre_refuse_what_they_cannot_have() {
    let mut j = sample();
    let spiral = |inner: f64, outer: f64| StrategySpec::Spiral {
        centre: None,
        inner_radius: inner,
        outer_radius: outer,
        direction: CutDirection::Climb,
        order: PassOrder::OutsideIn,
        slope_from: 0.0,
        slope_to: 90.0,
    };
    for (inner, outer) in [(-1.0, 0.0), (5.0, 5.0), (6.0, 5.0), (0.0, f64::NAN)] {
        j.setups[0].operations[1].strategy = spiral(inner, outer);
        assert!(j.validate().is_err(), "{inner}..{outer} accepted");
    }
    j.setups[0].operations[1].strategy = spiral(2.0, 0.0);
    j.validate().unwrap();
    for (from, to) in [(90.0, 90.0), (0.0, 400.0), (10.0, 5.0)] {
        j.setups[0].operations[1].strategy = StrategySpec::Radial {
            centre: None,
            inner_radius: 0.0,
            outer_radius: 0.0,
            angle_from: from,
            angle_to: to,
            direction: RadialDirection::BothWays,
            slope_from: 0.0,
            slope_to: 90.0,
        };
        assert!(j.validate().is_err(), "{from}..{to} accepted");
    }
    j.setups[0].operations[1].strategy = StrategySpec::MorphedSpiral {
        centre: Some([f64::INFINITY, 0.0]),
        direction: CutDirection::Climb,
        order: PassOrder::OutsideIn,
        slope_from: 0.0,
        slope_to: 90.0,
    };
    assert!(j.validate().is_err());
}

/// Picks nothing reads are kept, and not checked: a boundary switched back to
/// the stock after picking, its model since taken out of the setup, still runs.
/// Read again, as a boundary or by a height, they are checked.
#[test]
fn picks_nothing_reads_do_not_stop_the_job() {
    let mut j = picked();
    let op = &mut j.setups[0].operations[1];
    op.selection.faces[0].model = "gone".into();
    op.selection.edges[0].at[0] = f64::NAN;
    j.validate().unwrap();
    j.setups[0].operations[1].heights.top = Some(Height {
        reference: HeightRef::Selection,
        offset: 0.0,
    });
    assert!(matches!(
        j.validate(),
        Err(JobError::PickedFromOtherModel { .. })
    ));
    j.setups[0].operations[1].heights.top = None;
    j.setups[0].operations[1].boundary.kind = Boundary::Selection;
    assert!(j.validate().is_err());
}

/// A value is refused naming the field at fault and what it must be.
#[test]
fn a_bad_value_names_its_field_and_range() {
    let mut j = sample();
    j.setups[0].operations[1].strategy = StrategySpec::Scallop {
        direction: CutDirection::BothWays,
        order: PassOrder::OutsideIn,
        slope_from: 30.0,
        slope_to: 20.0,
    };
    let err = j.validate().unwrap_err();
    assert!(
        matches!(&err, JobError::BadValue { field, value, .. } if field == "top/finish.slope_to" && *value == 20.0),
        "{err:?}"
    );
    assert!(err.to_string().contains("greater than slope_from"), "{err}");
    j.setups[0].operations[1].strategy = StrategySpec::Scallop {
        direction: CutDirection::BothWays,
        order: PassOrder::OutsideIn,
        slope_from: -1.0,
        slope_to: 20.0,
    };
    assert!(matches!(
        j.validate(),
        Err(JobError::BadValue { field, .. }) if field == "top/finish.slope_from"
    ));
    // Zero is a margin, not a mistake; below it is.
    let mut j = sample();
    let StockSpec::AroundModels { margin, .. } = &mut j.stocks[0].spec else {
        panic!("the sample's stock is around its models");
    };
    margin[0] = -1.0;
    let err = j.validate().unwrap_err();
    assert!(err.to_string().contains("at least zero"), "{err}");
    assert!(!err.to_string().contains("greater than zero"), "{err}");
}

/// Every value an operation's strategy reads is checked before anything runs,
/// and only those: a NaN margin on a 3D operation, a safe distance below the
/// material, a ramp that does not descend, a height that is not a number.
#[test]
fn every_value_a_strategy_reads_is_checked_up_front() {
    let field_of = |j: &Job| match j.validate() {
        Err(JobError::BadValue { field, .. }) => field,
        other => panic!("expected a bad value, got {other:?}"),
    };
    type Edit = fn(&mut Operation);
    let edits: [(&str, Edit); 6] = [
        ("top/rough.boundary.margin", |op| {
            op.boundary.margin = f64::NAN;
        }),
        ("top/rough.linking.safe_distance", |op| {
            op.linking.safe_distance = -1.0;
        }),
        ("top/rough.linking.ramp_angle", |op| {
            op.linking.ramp_angle = 0.0;
        }),
        ("top/rough.linking.ramp_feed", |op| {
            op.linking.ramp_feed = Some(0.0);
        }),
        ("top/rough.heights.clearance.offset", |op| {
            op.heights.clearance.offset = f64::INFINITY;
        }),
        ("top/rough.heights.bottom.offset", |op| {
            op.heights.bottom = Some(Height {
                reference: HeightRef::ModelBottom,
                offset: f64::NAN,
            });
        }),
    ];
    for (field, edit) in edits {
        let mut j = sample();
        edit(&mut j.setups[0].operations[0]);
        assert_eq!(field_of(&j), field);
    }
    let mut j = sample();
    j.setups[0].operations[1].linking.lead_radius = Some(-0.5);
    assert_eq!(field_of(&j), "top/finish.linking.lead_radius");

    // What a strategy does not read is not checked: a profile has no boundary,
    // and parallel finishing neither ramps nor has a top.
    let mut j = sample();
    let finish = &mut j.setups[0].operations[1];
    finish.linking.ramp_angle = f64::NAN;
    finish.heights.top = Some(Height {
        reference: HeightRef::StockTop,
        offset: f64::NAN,
    });
    j.validate().unwrap();
    j.setups[0].operations[1].strategy = StrategySpec::Profile {
        side: ProfileSide::Outside,
        conventional: false,
    };
    j.setups[0].operations[1].boundary.margin = f64::NAN;
    j.setups[0].operations[1].linking.ramp_angle = 2.0;
    j.setups[0].operations[1].heights.top = None;
    j.setups[0].operations[1].tool = Some("rough-6-flat".into());
    j.validate().unwrap();
}

/// Settings a strategy has no use for are not required: a contour, a profile
/// and a drill have no stepover, and a drill no cutting feed.
#[test]
fn settings_a_strategy_does_not_use_are_not_required() {
    let mut j = sample();
    let finish = &mut j.setups[0].operations[1];
    finish.strategy = StrategySpec::named("contour").unwrap();
    finish.passes.stepdown = Some(1.0);
    finish.passes.stepover = 0.0;
    j.validate().unwrap();
    let finish = &mut j.setups[0].operations[1];
    finish.strategy = StrategySpec::Drill {
        peck: None,
        dwell: None,
    };
    finish.tool = Some("rough-6-flat".into());
    finish.passes.feed = 0.0;
    j.validate().unwrap();
    j.setups[0].operations[1].strategy = StrategySpec::named("parallel").unwrap();
    assert!(matches!(
        j.validate(),
        Err(JobError::BadValue { field, .. }) if field == "top/finish.passes.stepover"
    ));
}

/// How each strategy links, and what a drill's picks and adaptive's stepover
/// are, is its `uses()`'s to say.
#[test]
fn each_strategy_says_how_it_links() {
    for name in ["parallel", "scallop", "spiral", "radial", "morphed-spiral"] {
        let s = StrategySpec::named(name).unwrap();
        assert_eq!(s.uses().links, Links::OverPart, "{name}");
    }
    // A contour's passes run round the part beside its walls, not over it: it
    // links as a level's do.
    for name in [
        "contour",
        "flat",
        "pocket-clearing",
        "pocket",
        "profile",
        "face",
    ] {
        assert_eq!(
            StrategySpec::named(name).unwrap().uses().links,
            Links::Level,
            "{name}"
        );
    }
    for name in ["adaptive", "drill"] {
        assert_eq!(
            StrategySpec::named(name).unwrap().uses().links,
            Links::Own,
            "{name}"
        );
    }
    let drill = StrategySpec::named("drill").unwrap().uses();
    assert!(drill.holes && !drill.feed && !drill.load);
    let adaptive = StrategySpec::named("adaptive").unwrap().uses();
    assert!(adaptive.load && adaptive.feed && !adaptive.holes);
}

/// Each linking value left out is its strategy's own for the tool, as the run
/// cuts with it; given, it is as given.
#[test]
fn linking_values_default_as_the_strategy_says() {
    let op = |name: &str| Operation::new("o", StrategySpec::named(name).unwrap());
    assert_eq!(op("parallel").stay_down(6.0), 50.0);
    assert_eq!(op("pocket-clearing").stay_down(6.0), 30.0);
    assert_eq!(op("adaptive").stay_down(6.0), 60.0);
    assert_eq!(op("drill").stay_down(6.0), 0.0);
    assert!((op("contour").lead_radius(6.0) - 0.6).abs() < 1e-12);
    assert_eq!(op("pocket").lead_radius(6.0), 0.0);
    assert!((op("profile").helix_diameter(10.0) - 9.0).abs() < 1e-12);
    assert_eq!(op("profile").entry(), Some(Entry::Helix));
    assert_eq!(op("pocket").entry(), None);
    assert!(op("profile").leads(6.0).is_some());
    assert_eq!(op("pocket").leads(6.0), None);
    // The stepdown, or the tool without one.
    assert_eq!(op("pocket").ramp_stepdown(6.0), 2.0);
    assert_eq!(op("parallel").ramp_stepdown(6.0), 6.0);
    let mut given = op("adaptive");
    given.linking.stay_down = Some(12.0);
    given.linking.entry = Some(Entry::Plunge);
    given.linking.ramp_stepdown = Some(0.5);
    assert_eq!(given.stay_down(6.0), 12.0);
    assert_eq!(given.entry(), Some(Entry::Plunge));
    assert_eq!(given.ramp_stepdown(6.0), 0.5);
}

/// A new operation starts with its strategy's own depths and stock to leave,
/// and no tool.
#[test]
fn a_new_operation_starts_as_fusions() {
    let rough = Operation::new("rough", StrategySpec::named("adaptive").unwrap());
    assert_eq!(rough.name, "rough");
    assert_eq!(rough.tool, None);
    assert_eq!(rough.passes.stepdown, Some(2.0));
    assert_eq!(rough.passes.stock_to_leave.radial, 0.5);
    assert!(rough.enabled);
    let finish = Operation::new("finish", StrategySpec::named("parallel").unwrap());
    assert_eq!(finish.passes.stepdown, None);
    assert_eq!(finish.passes.stock_to_leave, StockToLeave::default());
    // Only a height from its picks, and not a drill's.
    assert!(!finish.can_measure_from_picks());
    let mut picked = picked();
    assert!(picked.setups[0].operations[1].can_measure_from_picks());
    picked.setups[0].operations[1].strategy = StrategySpec::named("drill").unwrap();
    assert!(!picked.setups[0].operations[1].can_measure_from_picks());
}
