//! An operation's faces machined and avoided, as a job file has them.

use super::*;

/// A parallel finish that avoids the model but for one face it machines,
/// with more stock left on it, and keeps a millimetre above another.
const GROUPED: &str = r#"
version = 1
tolerance = 0.02

[tools.ball]
kind = "ball"
diameter = 4.0
number = 1

[tools.vee]
kind = "cone"
diameter = 6.0
angle = 90.0
number = 2

[[model]]
name = "part"
path = "part.step"

[[model]]
name = "fixture"
path = "fixture.step"

[[stock]]
name = "block"
kind = "around-models"

[[setup]]
name = "top"
models = ["part"]
stocks = ["block"]

[[setup.operation]]
name = "finish"
strategy = "parallel"
tool = "ball"
passes = { stepover = 0.4, feed = 2400.0 }

[setup.operation.surfaces]
model = { action = "avoid" }

[[setup.operation.surfaces.group]]
action = "machine"
radial = 0.5
faces = [ { model = "part", index = 3, at = [1.0, 2.0, 3.0] } ]

[[setup.operation.surfaces.group]]
action = "avoid"
axial = 1.0
faces = [ { model = "part", index = 7, at = [4.0, 5.0, 6.0] } ]
"#;

fn grouped() -> Job {
    toml::from_str(GROUPED).unwrap()
}

fn op(j: &mut Job) -> &mut Operation {
    &mut j.setups[0].operations[0]
}

#[test]
fn surface_groups_are_read_and_written_as_the_job_file_has_them() {
    let j = grouped();
    let s = &j.setups[0].operations[0].surfaces;
    assert_eq!(s.model.action, SurfaceAction::Avoid);
    assert_eq!(s.groups.len(), 2);
    assert_eq!(s.groups[0].action, SurfaceAction::Machine);
    assert_eq!((s.groups[0].radial, s.groups[0].axial), (0.5, 0.0));
    assert_eq!(s.groups[1].faces[0].index, 7);
    j.validate().unwrap();
    let written = toml::to_string(&j).unwrap();
    assert_eq!(toml::from_str::<Job>(&written).unwrap(), j);
    // What is as it starts is left out: the second group's radial, the first's axial.
    assert_eq!(written.matches("radial").count(), 1, "{written}");
    assert_eq!(written.matches("axial").count(), 1, "{written}");
}

/// An operation with no groups writes nothing of them, so every job file
/// written before there were groups is written as it was.
#[test]
fn an_operation_with_no_groups_writes_none() {
    let mut j = grouped();
    op(&mut j).surfaces = Surfaces::default();
    let written = toml::to_string(&j).unwrap();
    assert!(!written.contains("surfaces"), "{written}");
    assert!(!written.contains("group"), "{written}");
    let fresh = Operation::new("new", StrategySpec::Parallel { angle: 0.0 });
    assert!(fresh.surfaces.is_whole());
}

#[test]
fn renaming_a_model_carries_its_grouped_faces() {
    let mut j = grouped();
    j.rename_model_refs("part", "bracket");
    let s = &j.setups[0].operations[0].surfaces;
    assert!(
        s.groups
            .iter()
            .flat_map(|g| &g.faces)
            .all(|r| r.model == "bracket")
    );
}

/// Each refusal of the job file's own, with its own message.
#[test]
fn surface_groups_that_cannot_be_cut_are_refused() {
    // A strategy that does not take them: left unread, an avoided face would
    // be cut.
    let mut j = grouped();
    op(&mut j).strategy = StrategySpec::named("adaptive").unwrap();
    let e = j.validate().unwrap_err();
    assert!(
        matches!(&e, JobError::NotOffered { field, value: "surface groups", strategy: "adaptive" }
            if field == "top/finish.surfaces"),
        "{e}"
    );

    // An offset that is no distance.
    for bad in [-0.1, f64::NAN, f64::INFINITY] {
        let mut j = grouped();
        op(&mut j).surfaces.groups[1].axial = bad;
        let e = j.validate().unwrap_err();
        assert!(
            matches!(&e, JobError::BadValue { field, .. } if field == "top/finish.surfaces.group.1.axial"),
            "{e}"
        );
        let mut j = grouped();
        op(&mut j).surfaces.groups[0].radial = bad;
        let e = j.validate().unwrap_err();
        assert!(
            matches!(&e, JobError::BadValue { field, .. } if field == "top/finish.surfaces.group.0.radial"),
            "{e}"
        );
    }

    // A group with no faces.
    let mut j = grouped();
    op(&mut j).surfaces.groups[1].faces.clear();
    let e = j.validate().unwrap_err();
    assert_eq!(
        e,
        JobError::EmptyGroup {
            op: "top/finish".into(),
            group: 2
        }
    );
    assert_eq!(
        e.to_string(),
        "operation 'top/finish': surface group 2 has no faces"
    );

    // A face of a model the setup does not machine.
    let mut j = grouped();
    op(&mut j).surfaces.groups[0].faces[0].model = "fixture".into();
    let e = j.validate().unwrap_err();
    assert!(
        matches!(&e, JobError::PickedFromOtherModel { owner, model, .. }
            if model == "fixture" && owner == "operation 'top/finish' surface group 1"),
        "{e}"
    );

    // The model avoided with no group machined: nothing to cut.
    let mut j = grouped();
    op(&mut j).surfaces.groups[0].action = SurfaceAction::Avoid;
    let e = j.validate().unwrap_err();
    assert_eq!(
        e,
        JobError::NothingToMachine {
            op: "top/finish".into()
        }
    );
    // But with the model machined, avoiding is all a group need do.
    op(&mut j).surfaces.model.action = SurfaceAction::Machine;
    j.validate().unwrap();
}

/// Every finish that drops its tool onto the part takes groups; every other
/// strategy refuses them, the level finishes for now among them: what
/// refuses them is that it does not take them
/// ([`surface_groups_that_cannot_be_cut_are_refused`]).
#[test]
fn only_the_finishes_over_the_part_take_surface_groups() {
    let taking = [
        "parallel",
        "scallop",
        "spiral",
        "radial",
        "morphed-spiral",
        "pencil",
        "project",
        "morph",
    ];
    let refusing = [
        "contour",
        "ramp",
        "flat",
        "pocket-clearing",
        "adaptive",
        "profile",
        "pocket",
        "face",
        "drill",
        "bore",
        "circular",
        "thread",
        "slot",
        "trace",
        "chamfer-2d",
        "engrave",
        "adaptive-2d",
    ];
    for name in taking.into_iter().chain(refusing) {
        let strategy = StrategySpec::named(name).unwrap();
        assert_eq!(strategy.uses().surfaces, taking.contains(&name), "{name}");
    }
}

/// An operation turned off is not checked: its groups may be unfinished.
#[test]
fn an_operation_turned_off_may_have_groups_unfinished() {
    let mut j = grouped();
    op(&mut j).surfaces.groups[0].faces.clear();
    op(&mut j).enabled = false;
    let mut on = op(&mut j).clone();
    on.name = "on".into();
    on.enabled = true;
    on.surfaces = Surfaces::default();
    j.setups[0].operations.push(on);
    j.validate().unwrap();
}
