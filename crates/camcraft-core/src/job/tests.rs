use super::*;
use crate::frame::{Axis, Orientation};
use crate::tool::{Cutter, ToolProfile};

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
strategy = "zlevel-rough"
tool = "rough-6-flat"
stepdown = 2.0
geometry = { margin = 1.0, bottom = { ref = "model-bottom" } }
heights = { clearance = { ref = "stock-top", offset = 8.0 } }
passes = { stepover = 3.0, feed = 1800.0, stock_to_leave = { radial = 0.5, axial = 0.2 } }
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
                Place::Center,
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
}

/// A fixed box from before placing, at a corner, reads as placed at that corner
/// on each axis and is written back as it was, so an older build still reads it.
#[test]
fn a_fixed_box_at_a_corner_keeps_its_old_form() {
    let text = "kind = \"fixed-box\"\nname = \"plate\"\ncorner = [1.0, 2.0, 3.0]\nsize = [4.0, 5.0, 6.0]\n";
    let old: StockDef = toml::from_str(text).unwrap();
    assert_eq!(
        old.spec,
        StockSpec::FixedBox(FixedBox::at([1.0, 2.0, 3.0], [4.0, 5.0, 6.0]))
    );
    let again = toml::to_string(&old).unwrap();
    assert!(again.contains("corner"), "{again}");
    assert!(!again.contains("from"), "{again}");
    let mut j = sample();
    j.stocks.push(old);
    assert_eq!(j.required_version(), sample().required_version());
}

/// One placed against the models is written with a place on each axis, as
/// version 5, which an older build refuses by name.
#[test]
fn a_placed_fixed_box_is_version_5() {
    let mut j = sample();
    j.stocks.push(StockDef {
        name: "board".into(),
        spec: StockSpec::FixedBox(FixedBox::centred([10.0, 10.0, 10.0])),
    });
    assert_eq!(j.required_version(), 5);
    let text = toml::to_string(&j.stocks[1]).unwrap();
    assert!(text.contains("from = \"center\""), "{text}");
    assert!(!text.contains("corner"), "{text}");
}

/// A corner and a place on an axis say two things; neither is taken.
#[test]
fn a_fixed_box_with_a_corner_and_a_place_is_refused() {
    let text = "kind = \"fixed-box\"\nname = \"p\"\ncorner = [0.0, 0.0, 0.0]\nsize = [1.0, 1.0, 1.0]\nx = { from = \"center\" }\n";
    assert!(toml::from_str::<StockDef>(text).is_err());
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
                Place::Center,
                Place::Center,
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

/// A job from before boundaries has none written: picks are its boundary, and
/// without them the stock's. One chosen otherwise, or an offset that now moves a
/// boundary the tool is kept inside inward, is version 6.
#[test]
fn a_machining_boundary_reads_as_before_or_is_version_6() {
    let j = sample();
    let g = &j.setups[0].operations[0].geometry;
    assert_eq!(g.boundary(), Boundary::Stock);
    let mut picked = Geometry::default();
    picked.faces.push(FeatureRef {
        model: "part".into(),
        index: 0,
        at: [0.0; 3],
    });
    assert_eq!(picked.boundary(), Boundary::Selection);
    let base = j.required_version();
    assert!(base < 6);

    let mut chosen = j.clone();
    chosen.setups[0].operations[0].geometry.boundary_kind = Some(Boundary::Stock);
    assert_eq!(chosen.required_version(), base, "the same as before");
    chosen.setups[0].operations[0].geometry.boundary_kind = Some(Boundary::Silhouette);
    assert_eq!(chosen.required_version(), 6);
    let text = toml::to_string(&chosen).unwrap();
    assert!(text.contains("boundary = \"silhouette\""), "{text}");
    let again: Job = toml::from_str(&text).unwrap();
    assert_eq!(again, chosen);

    let mut inward = j.clone();
    let g = &mut inward.setups[0].operations[1].geometry;
    *g = picked;
    g.containment = Containment::Inside;
    assert_eq!(inward.required_version(), 2, "picks, and no offset");
    inward.setups[0].operations[1].geometry.margin = 0.5;
    assert_eq!(inward.required_version(), 6);
}

/// A file without a version is refused by name, not with a parse error about
/// some field.
#[test]
fn another_format_is_refused() {
    let mut j = sample();
    j.version = 9;
    assert_eq!(j.validate(), Err(JobError::UnsupportedVersion(9)));
}

/// The reason heights are symbolic: the same job survives a different block.
#[test]
fn heights_resolve_against_the_setup() {
    let ctx = HeightContext {
        stock_top: 20.0,
        stock_bottom: -5.0,
        model_top: 18.0,
        model_bottom: 0.0,
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
        StrategySpec::ZlevelRough { stepdown, .. } => assert_eq!(stepdown, 2.0),
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
    assert_eq!(with(|t| t.presets[0].feed = Some(0.0)), ["presets.feed"]);
    assert_eq!(with(|t| t.presets[1].rpm = Some(0)), ["presets.rpm"]);
    assert_eq!(with(|t| t.presets[1].name = " ".into()), ["presets.name"]);
    assert_eq!(
        with(|t| t.presets[1].name = "Roughing".into()),
        ["presets.name"]
    );
    assert_eq!(
        with(|t| t.presets[0].stock_to_leave.as_mut().unwrap().axial = -1.0),
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
    let g = &mut s.operations[1].geometry;
    g.faces.push(FeatureRef {
        model: "part".into(),
        index: 12,
        at: [30.2, 14.0, 12.7],
    });
    g.edges.push(FeatureRef {
        model: "part".into(),
        index: 3,
        at: [1.0, 2.0, 3.0],
    });
    g.containment = Containment::Inside;
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
        text.contains("[[setup.operation.geometry.faces]]"),
        "{text}"
    );
    // Nothing picked, nothing written.
    let plain = toml::to_string(&sample()).unwrap();
    assert!(!plain.contains("faces"), "{plain}");
    assert!(!plain.contains("containment"), "{plain}");
}

/// A version 1 reader would skip picks and cut the whole part, so a job with
/// any is version 2; every other job stays version 1, readable as before.
#[test]
fn only_a_job_with_picks_needs_version_2() {
    assert_eq!(sample().required_version(), 1);
    assert_eq!(picked().required_version(), 2);
    let mut origin_only = sample();
    origin_only.setups[0].origin = picked().setups[0].origin.clone();
    assert_eq!(origin_only.required_version(), 2);
    let mut faces_only = picked();
    faces_only.setups[0].origin = Origin::default();
    faces_only.setups[0].operations[1].geometry.edges.clear();
    assert_eq!(faces_only.required_version(), 2);
    // Both versions are read.
    let mut one = sample();
    one.version = 1;
    one.validate().unwrap();
    let mut two = sample();
    two.version = 2;
    two.validate().unwrap();
    let mut five = sample();
    five.version = 5;
    five.validate().unwrap();
    let mut six = sample();
    six.version = 6;
    six.validate().unwrap();
    let mut seven = sample();
    seven.version = 7;
    assert_eq!(seven.validate(), Err(JobError::UnsupportedVersion(7)));
}

/// An older reader would skip an operation's own spindle speed and cut at the
/// tool's, so a job with one is version 3, whatever else it has.
#[test]
fn only_a_job_with_an_operation_speed_needs_version_3() {
    let mut j = sample();
    j.setups[0].operations[1].passes.rpm = Some(12000);
    assert_eq!(j.required_version(), 3);
    j.validate().unwrap();
    let mut both = picked();
    both.setups[0].operations[0].passes.rpm = Some(12000);
    assert_eq!(both.required_version(), 3);
    let mut mist = sample();
    mist.tools.get_mut("rough-6-flat").unwrap().coolant = Coolant::Mist;
    assert_eq!(mist.required_version(), 3);
    let mut left = sample();
    left.tools.get_mut("rough-6-flat").unwrap().spindle = SpindleDirection::CounterClockwise;
    assert_eq!(left.required_version(), 3);
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
    op.strategy = StrategySpec::ZlevelRough {
        stepdown: 3.0,
        min_ring_length: 3.0,
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

/// An older reader would cut an operation turned off, so a job with one is
/// version 4, whatever else it has.
#[test]
fn only_a_job_with_an_operation_off_needs_version_4() {
    let mut j = picked();
    j.setups[0].operations[0].passes.rpm = Some(12000);
    assert_eq!(j.required_version(), 3);
    j.setups[0].operations[1].enabled = false;
    assert_eq!(j.required_version(), 4);
    j.validate().unwrap();
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
    assert_eq!(op.passes.stepover, 2.5);
    assert!(matches!(op.strategy, StrategySpec::ZlevelRough { stepdown, .. } if stepdown == 3.0));
    assert_eq!(op.passes.stock_to_leave.radial, 0.3);
    assert!(op.differs_from(tool.preset("Roughing").unwrap()).is_empty());

    op.apply_preset(tool.preset("Finishing").unwrap());
    assert_eq!(op.preset.as_deref(), Some("Finishing"));
    assert_eq!(op.passes.stepover, 0.5);
    assert_eq!(op.passes.stock_to_leave, StockToLeave::default());
    // Finishing has no feeds or speed, so Roughing's stay.
    assert_eq!(op.passes.feed, 2000.0);
    assert_eq!(op.passes.rpm, Some(16000));
}

/// What has been changed since a preset was applied is named, and only that.
#[test]
fn changes_from_a_preset_are_named() {
    let tool = preset_tool();
    let roughing = tool.preset("Roughing").unwrap();
    let mut op = sample().setups[0].operations[0].clone();
    op.apply_preset(roughing);
    op.passes.feed = 1500.0;
    op.passes.stepover = 2.0;
    assert_eq!(op.differs_from(roughing), ["feed", "stepover"]);
    // A parallel finish has no stepdown, so a preset's is not a change.
    let mut finish = sample().setups[0].operations[1].clone();
    finish.apply_preset(roughing);
    assert!(finish.differs_from(roughing).is_empty());
}

/// An operation's values saved as a preset set them back exactly.
#[test]
fn an_operation_saved_as_a_preset_sets_itself_back() {
    let op = sample().setups[0].operations[0].clone();
    let saved = op.as_preset("Mine");
    let mut other = op.clone();
    other.passes.feed = 1.0;
    other.passes.stepover = 1.0;
    other.apply_preset(&saved);
    assert!(other.differs_from(&saved).is_empty());
    assert_eq!(other.passes, op.passes);
    assert_eq!(other.strategy, op.strategy);
}

/// A tool from before presets kept one `cutting` table; it reads as a preset
/// called "Default" at the tool's speed, and is written back as one.
#[test]
fn cutting_data_from_before_presets_reads_as_a_preset() {
    let old = "kind = \"flat\"\ndiameter = 6.35\nrpm = 18000\n\
               cutting = { feed = 3810.0, plunge_feed = 1354.667 }\n";
    let tool: ToolSpec = toml::from_str(old).unwrap();
    assert_eq!(tool.presets.len(), 1);
    let p = &tool.presets[0];
    assert_eq!(p.name, "Default");
    assert_eq!(p.rpm, Some(18000));
    assert_eq!(p.feed, Some(3810.0));
    assert_eq!(p.plunge_feed, Some(1354.667));
    let text = toml::to_string(&tool).unwrap();
    assert!(!text.contains("cutting"), "{text}");
    let again: ToolSpec = toml::from_str(&text).unwrap();
    assert_eq!(again, tool);
}

#[test]
fn a_pick_must_be_on_the_setup_and_a_point() {
    let mut j = picked();
    j.setups[0].operations[1].geometry.faces[0].model = "elsewhere".into();
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
    let g = &s.operations[1].geometry;
    assert!(g.faces.iter().chain(&g.edges).all(|r| r.model == "bracket"));
    j.rename_model_refs("nothing", "else");
    assert_eq!(j.setups[0].operations[1].geometry.faces[0].model, "bracket");
}

#[test]
fn an_origin_keeps_its_kind_when_its_offset_changes() {
    let o = picked().setups[0].origin.clone();
    assert_eq!(o.offset(), [0.0, 0.0, 1.0]);
    let moved = o.clone().with_offset([1.0, 2.0, 3.0]);
    assert_eq!(moved.offset(), [1.0, 2.0, 3.0]);
    assert!(matches!(moved, Origin::Point { .. }));
}
