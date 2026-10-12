//! Every post read back as a controller would, against the path it was posted
//! from: each built-in profile in every way it can be set to write, in both
//! units, over paths with arcs of every size, way and plane. And programs
//! broken on purpose, one fault each, so the check is shown to catch what it
//! is there to catch.

#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod common;

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use camcraft_core::path::toolpath::{Arc, MoveRole, Toolpath};
use camcraft_post::op::{ProgramSpec, lower};
use camcraft_post::profile::{ArcStyle, Profile, ProgramUnits, ToolChangeStyle};
use camcraft_post::readback::check;
use camcraft_post::render::render;
use common::{drilling, fixture, spec};
use glam::{DVec3, dvec3};

fn post(path: &Toolpath, spec: &ProgramSpec, profile: &Profile) -> String {
    render(&lower(path, spec).unwrap(), profile, "readback").0
}

fn reads_back(path: &Toolpath, spec: &ProgramSpec, profile: &Profile, what: &str) {
    let text = post(path, spec, profile);
    if let Err(p) = check(&text, path, spec, profile) {
        panic!("{what} does not read back: {p}\n{text}");
    }
}

/// Each built-in profile, and each way a profile can be set to write.
fn profiles() -> Vec<(String, Profile)> {
    let mut out = Vec::new();
    for name in Profile::builtin_names() {
        let p = Profile::builtin(name).unwrap().unwrap();
        let mut variant = |what: &str, change: &dyn Fn(&mut Profile)| {
            let mut v = p.clone();
            change(&mut v);
            out.push((format!("{name} {what}"), v));
        };
        variant("as built", &|_| {});
        variant("with radius arcs", &|v| v.arc_style = ArcStyle::R);
        variant("with centre arcs", &|v| v.arc_style = ArcStyle::Ij);
        variant("numbered", &|v| v.numbering.enabled = true);
        variant("with trailing zeros", &|v| {
            v.format.strip_trailing_zeros = false
        });
        variant("with nothing suppressed", &|v| {
            v.modal.suppress_motion = false;
            v.modal.suppress_axes = false;
            v.modal.suppress_feed = false;
        });
        variant("without going up or home", &|v| {
            v.safe_retract.clear();
            v.end_home.clear();
        });
        variant("changing by hand", &|v| {
            v.tool_change = ToolChangeStyle::Pause
        });
        variant("changing by M6", &|v| v.tool_change = ToolChangeStyle::M6);
        variant("with a length offset", &|v| v.length_offset = true);
        variant("without a length offset", &|v| v.length_offset = false);
    }
    out
}

/// Arcs a path can hold, chained: level, upright about X and about Y, each
/// way, from a hair of a turn to a whole one, small and large, level and as
/// helices.
fn arcs() -> Toolpath {
    let mut t = Toolpath::new();
    t.rapid_to(dvec3(0.0, 0.0, 30.0)).unwrap();
    t.feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Plunge, 300.0)
        .unwrap();
    let mut from = DVec3::ZERO;
    let sweeps = [0.05, 0.5, FRAC_PI_2, 2.0, PI, 4.0, 3.0 * FRAC_PI_2, TAU];
    let radii = [0.8, 6.0, 250.0];
    let mut i = 0u32;
    for axis in [DVec3::Z, DVec3::X, DVec3::Y, -DVec3::Z] {
        for &r in &radii {
            for &s in &sweeps {
                for sign in [1.0, -1.0] {
                    i += 1;
                    // The centre square to the axis from where the arc starts.
                    let side = axis.any_orthogonal_vector().normalize();
                    let centre = from + side * r;
                    // Every third a helix, moving along its axis as it turns.
                    let drop = if i.is_multiple_of(3) { -0.7 } else { 0.0 };
                    let arc = Arc {
                        centre,
                        axis,
                        sweep: sign * s,
                    };
                    let to = arc.point_at(from, from + axis * drop, 1.0);
                    t.arc_to(to, arc, MoveRole::Cut, 900.0 + 10.0 * f64::from(i % 4))
                        .unwrap();
                    from = to;
                    // A straight between some, so arcs follow lines too.
                    if i.is_multiple_of(5) {
                        from += dvec3(1.25, -0.5, 0.0);
                        t.feed_to(from, MoveRole::Cut, 1200.0).unwrap();
                    }
                }
            }
        }
    }
    t.feed_to(from + DVec3::Z * 30.0, MoveRole::Retract, 900.0)
        .unwrap();
    t
}

/// Every profile, set every way, in both units: the fixture and the arcs.
#[test]
fn every_post_reads_back_to_its_path() {
    let paths = [("the fixture", fixture()), ("the arcs", arcs())];
    for (name, profile) in profiles() {
        for units in [ProgramUnits::Mm, ProgramUnits::Inch] {
            let spec = ProgramSpec { units, ..spec() };
            for (what, path) in &paths {
                reads_back(
                    path,
                    &spec,
                    &profile,
                    &format!("{what} for {name} in {units:?}"),
                );
            }
        }
    }
}

/// Holes as canned cycles, plain, with a pause and pecked, read back as the
/// holes they drill, in every profile that is set to take cycles.
#[test]
fn drilling_cycles_read_back_to_their_holes() {
    for (name, mut profile) in profiles() {
        profile.drill_cycles = true;
        for units in [ProgramUnits::Mm, ProgramUnits::Inch] {
            for (peck, dwell) in [(None, None), (Some(2.0), None), (None, Some(0.5))] {
                let spec = ProgramSpec {
                    units,
                    ..drilling(peck, dwell)
                };
                reads_back(
                    &Toolpath::new(),
                    &spec,
                    &profile,
                    &format!("holes ({peck:?}, {dwell:?}) for {name} in {units:?}"),
                );
            }
        }
    }
}

/// The fixture posted for `profile`, with one fault put in: `from` (which
/// must be there, once, as written) replaced by `to`.
fn broken(
    profile: &str,
    units: ProgramUnits,
    from: &str,
    to: &str,
) -> (String, Toolpath, ProgramSpec, Profile) {
    let profile = Profile::builtin(profile).unwrap().unwrap();
    let path = fixture();
    let spec = ProgramSpec { units, ..spec() };
    let text = post(&path, &spec, &profile);
    assert_eq!(
        text.matches(from).count(),
        1,
        "{from:?} is not in the program once:\n{text}"
    );
    check(&text, &path, &spec, &profile).unwrap();
    (text.replace(from, to), path, spec, profile)
}

/// Each fault is caught, and said: the first thing the program does wrong.
#[test]
fn faults_are_caught() {
    use ProgramUnits::{Inch, Mm};
    let faults: &[(&str, ProgramUnits, &str, &str, &str)] = &[
        // An arc the wrong way round.
        (
            "masso",
            Mm,
            "G17 G2 X12. Y11.5",
            "G17 G3 X12. Y11.5",
            "strays",
        ),
        // Its centre on the other side.
        ("masso", Mm, "I2. J0.", "I-2. J0.", "strays"),
        // A lead in the wrong plane.
        ("masso", Mm, "G18 G2", "G19 G2", "not a centre offset"),
        // Inches read as millimetres.
        (
            "masso-inch",
            Inch,
            "N15 G20",
            "N15 G21",
            "on its way to the path's start",
        ),
        // A cut made a rapid.
        (
            "grbl",
            Mm,
            "G1 Z1 F250",
            "G0 Z1 F250",
            "a rapid where the path cuts",
        ),
        // The wrong feed.
        ("grbl", Mm, "X10 F1200", "X10 F1300", "feeds at 1300.000"),
        // A cut left out.
        (
            "masso-inch",
            Inch,
            "N85 Y0.374\n",
            "",
            "where move 5 of the path goes",
        ),
        ("grbl", Mm, "Y9.5\n", "", "where move 5 of the path goes"),
        // Not raised before the tool change, so across at a height not known.
        (
            "masso",
            Mm,
            "N25 G28 G91 Z0\nN30 G90\n",
            "",
            "goes across at a height not known",
        ),
        // Across and down together from nowhere known.
        (
            "grbl",
            Mm,
            "G0 Z25\nX0 Y0",
            "G0 X0 Y0 Z25",
            "across and down at once",
        ),
        // The wrong tool, the wrong offset, the wrong length.
        ("centroid-avid", Mm, "T3 M6", "T4 M6", "before tool 3 is in"),
        ("centroid-avid", Mm, "\nG54\n", "\nG55\n", "G54 is selected"),
        (
            "centroid-avid",
            Mm,
            "G43 Z25 H3",
            "G43 Z25 H4",
            "tool 4's length",
        ),
        // Z not sent home before the change, so across at a height not known.
        (
            "centroid-avid",
            Mm,
            "M5\nG53 G0 Z0\n(6MM",
            "M5\n(6MM",
            "height not known",
        ),
        // The spindle not stopped for the change, not started, at the wrong
        // speed; the coolant left off.
        (
            "centroid-avid",
            Mm,
            "M5\nG53 G0 Z0\n(6MM",
            "G53 G0 Z0\n(6MM",
            "without stopping the spindle",
        ),
        ("grbl", Mm, "S18000 M3\n", "", "before the spindle turns"),
        (
            "centroid-avid",
            Inch,
            "S18000 M3",
            "S12000 M3",
            "before the spindle turns",
        ),
        ("masso", Mm, "N50 M8\n", "", "coolant not Flood"),
        // Down after the last cut, and an end with the spindle on.
        (
            "grbl",
            Mm,
            "M9\n",
            "M9\nG0 Z0\n",
            "goes down after the path has ended",
        ),
        (
            "centroid-avid",
            Mm,
            "M5\nG53 G0 Z0\nG0 X0 Y0",
            "G53 G0 Z0\nG0 X0 Y0",
            "ends with the spindle on",
        ),
        // No end at all.
        ("masso", Mm, "N155 M30\n", "", "no end"),
        // Something the reader does not know.
        (
            "grbl",
            Mm,
            "G90 G94 G17",
            "G90 G94 G17 G41",
            "G41 is not understood",
        ),
    ];
    for &(profile, units, from, to, said) in faults {
        let (text, path, spec, profile) = broken(profile, units, from, to);
        match check(&text, &path, &spec, &profile) {
            Ok(()) => panic!("{from:?} made {to:?} was not caught:\n{text}"),
            Err(p) => assert!(
                p.to_string().contains(said),
                "{from:?} made {to:?} was caught as {p:?}, not {said:?}\n{text}"
            ),
        }
    }
}

/// Faults in drilling: back only to R between holes, pecks too deep, a hole
/// not drilled to its bottom, a rapid into what is not drilled.
#[test]
fn drilling_faults_are_caught() {
    let mut profile = Profile::builtin("masso").unwrap().unwrap();
    profile.drill_cycles = true;
    let faults: &[(Option<f64>, &str, &str, &str)] = &[
        (
            Some(2.0),
            "N60 G98",
            "N60 G99",
            "below the clearance height",
        ),
        (
            Some(2.0),
            "X10. Y10. Z-1.5 R7. Q2.",
            "X10. Y10. Z-1.5 R7. Q5.",
            "pecks deeper",
        ),
        (
            None,
            "X30. Y10. Z-1.5",
            "X30. Y10. Z-1.",
            "before hole 2 is drilled to its bottom",
        ),
        (
            None,
            "X30. Y10. Z-1.5 R7.",
            "X30. Y10. Z-1.5 R-1.",
            "rapids to Z-1.000 in hole 2",
        ),
        (None, "X30. Y10.", "X31. Y10.", "where hole 2 is"),
    ];
    for &(peck, from, to, said) in faults {
        let spec = drilling(peck, None);
        let text = post(&Toolpath::new(), &spec, &profile);
        assert_eq!(text.matches(from).count(), 1, "{from:?}:\n{text}");
        check(&text, &Toolpath::new(), &spec, &profile).unwrap();
        let text = text.replace(from, to);
        match check(&text, &Toolpath::new(), &spec, &profile) {
            Ok(()) => panic!("{from:?} made {to:?} was not caught:\n{text}"),
            Err(p) => assert!(p.to_string().contains(said), "{p}\n{text}"),
        }
    }
}

/// An arc too short across for its ends to stay apart once rounded would
/// read as a full circle; it is written as straight moves instead, as Fusion
/// writes it, and reads back to the arc.
#[test]
fn an_arc_too_short_to_write_reads_back_to_itself() {
    let mut t = Toolpath::new();
    t.rapid_to(dvec3(0.0, 0.0, 10.0)).unwrap();
    t.feed_to(DVec3::ZERO, MoveRole::Plunge, 300.0).unwrap();
    // A hair of a turn, either way; and whole turns of a tiny helix.
    for (r, sweep, drop) in [
        (0.8, 0.0005, 0.0),
        (5.0, 0.0001, 0.0),
        (0.8, -0.0004, 0.0),
        (0.1, TAU, -0.2),
    ] {
        let from = t.end().unwrap();
        let arc = Arc {
            centre: from + DVec3::X * r,
            axis: DVec3::Z,
            sweep,
        };
        let to = arc.point_at(from, from + DVec3::Z * drop, 1.0);
        t.arc_to(to, arc, MoveRole::Cut, 1000.0).unwrap();
    }
    t.feed_to(dvec3(5.0, 0.0, -0.2), MoveRole::Cut, 1000.0)
        .unwrap();
    for (name, profile) in profiles() {
        for units in [ProgramUnits::Mm, ProgramUnits::Inch] {
            let spec = ProgramSpec { units, ..spec() };
            let text = post(&t, &spec, &profile);
            let arc = |w: &str| matches!(w, "G2" | "G3" | "G02" | "G03");
            assert!(
                !text.split_whitespace().any(arc),
                "{name} wrote a short arc as an arc:\n{text}"
            );
            reads_back(
                &t,
                &spec,
                &profile,
                &format!("short arcs for {name} in {units:?}"),
            );
        }
    }
}
