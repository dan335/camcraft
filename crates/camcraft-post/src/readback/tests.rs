use super::*;
use std::f64::consts::{FRAC_PI_2, PI};

fn moves(text: &str) -> Vec<Motion> {
    read(text)
        .unwrap()
        .into_iter()
        .filter_map(|s| match s.action {
            Action::Move(m) => Some(m),
            _ => None,
        })
        .collect()
}

fn at(x: f64, y: f64, z: f64) -> Position {
    [Some(x), Some(y), Some(z)]
}

fn near(a: DVec3, b: DVec3) -> bool {
    a.distance(b) < 1.0e-9
}

/// Lengths and feeds come out in millimetres, whatever units the program
/// was written in.
#[test]
fn inches_read_back_in_millimetres() {
    let m = moves("G21 G90\nG0 X1 Y2 Z3\nG20\nG1 X1 F10");
    assert_eq!(m[0].to, at(1.0, 2.0, 3.0));
    assert_eq!(m[1].to, at(25.4, 2.0, 3.0));
    assert_eq!(m[1].feed, Some(254.0));
}

/// Comments, line numbers and tape marks are not words, and what a comment
/// says is not done.
#[test]
fn comments_are_not_words() {
    let m = moves("%\nN10 G21 (G20 here) G90 ; G91\nN20 G0 X1 (Y5) Y2 Z0\n%");
    assert_eq!(
        m,
        [Motion {
            from: [None; 3],
            to: at(1.0, 2.0, 0.0),
            feed: None,
            arc: None,
        }]
    );
}

/// A move before the program says its units or its distance mode could go
/// anywhere, so it is refused, as is anything not understood.
#[test]
fn what_cannot_be_read_is_refused() {
    let refused = |text: &str, line: usize, what: &str| {
        let p = read(text).unwrap_err();
        assert_eq!(p.line, Some(line), "{p}");
        assert!(p.what.contains(what), "{p}");
    };
    refused("G90\nG0 X1", 2, "units");
    refused("G21\nG0 X1", 2, "distance mode");
    refused("G21 G90\nG41 X1", 2, "G41");
    refused("G21 G90\nG0 A10", 2, "A words");
    refused("G21 G90\nG0 X1 X2", 2, "twice");
    refused("G21 G90\nG0 G1 X1", 2, "two motions");
    refused("G21 G91\nG0 X1", 2, "not known");
    refused("G21 G90 (open", 1, "never closed");
    refused("G21 G90\nG0 X0 Y0 Z0\nG17 G2 X1 Y1 K1 F100", 3, "K is not");
    refused("G21 G90\nM6", 2, "no tool");
}

/// Centre offsets are from the start, and the direction is the word's: `G3`
/// counter-clockwise seen from the plane's normal, the short way or the long.
#[test]
fn an_arc_turns_the_way_its_word_says() {
    let start = "G21 G90 G17\nG0 X1 Y0 Z0\nG1 F100\n";
    let ccw = moves(&format!("{start}G3 X0 Y1 I-1 J0"));
    let t = ccw[1].arc.unwrap();
    assert!((t.sweep - FRAC_PI_2).abs() < 1e-12, "{t:?}");
    let mid = ccw[1].point_at(0.5).unwrap();
    assert!(near(mid, dvec3(0.5f64.sqrt(), 0.5f64.sqrt(), 0.0)), "{mid}");
    let cw = moves(&format!("{start}G2 X0 Y1 I-1 J0"));
    assert!((cw[1].arc.unwrap().sweep + 3.0 * FRAC_PI_2).abs() < 1e-12);
}

/// By radius, a positive one takes the short way round and a negative one the
/// long way.
#[test]
fn a_radius_says_which_way_round() {
    let start = "G21 G90 G17\nG0 X1 Y0 Z0\nG1 F100\n";
    let short = moves(&format!("{start}G3 X0 Y1 R1"))[1].arc.unwrap();
    assert!(near(short.centre, DVec3::ZERO), "{short:?}");
    assert!((short.sweep - FRAC_PI_2).abs() < 1e-12);
    let long = moves(&format!("{start}G3 X0 Y1 R-1"))[1].arc.unwrap();
    assert!(near(long.centre, dvec3(1.0, 1.0, 0.0)), "{long:?}");
    assert!((long.sweep - 3.0 * FRAC_PI_2).abs() < 1e-12);
    let p = read(&format!("{start}G3 X-3 Y0 R1")).unwrap_err();
    assert!(p.what.contains("too small"), "{p}");
}

/// Each plane takes its own two offsets and turns about its own normal; a
/// lead down in the XZ plane, as the golden fixture has one.
#[test]
fn an_upright_arc_turns_about_its_normal() {
    let m = moves("G21 G90\nG0 X12 Y11.5 Z-1\nG1 F100\nG18 G2 X13 Z-2 I1 K0");
    let t = m[1].arc.unwrap();
    assert_eq!(t.normal, DVec3::Y);
    assert!(near(t.centre, dvec3(13.0, 11.5, -1.0)), "{t:?}");
    assert!((t.sweep + FRAC_PI_2).abs() < 1e-12, "{t:?}");
    let p = read("G21 G90\nG0 X0 Y0 Z0\nG18 G2 X1 Z1 J1 F100").unwrap_err();
    assert!(p.what.contains("J is not a centre offset in G18"), "{p}");
}

/// Ends that coincide make a full turn, and a helix goes down as it turns.
#[test]
fn a_full_turn_of_helix() {
    let m = moves("G21 G90 G17\nG0 X1 Y0 Z0\nG3 X1 Y0 Z-1 I-1 J0 F100");
    let t = m[1].arc.unwrap();
    assert!((t.sweep - 2.0 * PI).abs() < 1e-12);
    assert!(near(m[1].point_at(0.5).unwrap(), dvec3(-1.0, 0.0, -0.5)));
}

/// Home and machine coordinates leave the axes they move unknown in work
/// coordinates; so do a tool change and a length offset.
#[test]
fn what_is_not_known_is_not_assumed() {
    let steps = read(concat!(
        "G21 G90\nG0 X1 Y2 Z3\nG28 G91 Z0\nG90\nG53 G0 X0\nG0 Y5\n",
        "T2 M6\nG0 X1 Y1\nG43 Z4 H2\nX2\nG49\nX3"
    ))
    .unwrap();
    let actions: Vec<&Action> = steps.iter().map(|s| &s.action).collect();
    assert!(actions.contains(&&Action::Home([false, false, true])));
    assert!(actions.contains(&&Action::Machine([Some(0.0), None, None])));
    assert!(actions.contains(&&Action::Change(2)));
    assert!(actions.contains(&&Action::LengthOffset(Some(2))));
    let m = moves(concat!(
        "G21 G90\nG0 X1 Y2 Z3\nG28 G91 Z0\nG90\nG53 G0 X0\nG0 Y5\n",
        "T2 M6\nG0 X1 Y1\nG43 Z4 H2\nX2\nG49\nX3"
    ));
    assert_eq!(m[1].from, [None, Some(2.0), None]);
    assert_eq!(m[2].from, [None; 3], "a tool change moves the machine");
    assert_eq!(m[3].from, [Some(1.0), Some(1.0), None]);
    assert_eq!(m[3].to, at(1.0, 1.0, 4.0));
    assert_eq!(m[5].from, [Some(2.0), Some(1.0), None]);
}

/// A peck drilling cycle, as the standard has it: across at the height it
/// starts at, down to R, pecks back to R between, and up to where it started
/// with G98, or to R with G99.
#[test]
fn a_peck_cycle_drills_as_the_standard_says() {
    let text = "G21 G90\nG0 X0 Y0 Z20\nG98\nG83 X10 Y5 Z-1.5 R7 Q2 F250\nG80";
    let m = moves(text);
    let feeds: Vec<f64> = m
        .iter()
        .filter(|m| m.feed.is_some())
        .map(|m| m.to[2].unwrap())
        .collect();
    assert_eq!(feeds, [5.0, 3.0, 1.0, -1.0, -1.5]);
    assert!(
        m.iter()
            .all(|m| m.feed.is_none_or(|f| (f - 250.0).abs() < 1e-9))
    );
    assert_eq!(m[1].to, at(10.0, 5.0, 20.0), "across at the start's height");
    assert_eq!(m.last().unwrap().to, at(10.0, 5.0, 20.0));
    // Never a rapid below what has been drilled.
    let mut floor = 7.0f64;
    for m in &m[2..] {
        let z = m.to[2].unwrap();
        if m.feed.is_some() {
            floor = floor.min(z);
        } else {
            assert!(z >= floor, "{m:?}");
        }
    }
    let r = moves(&text.replace("G98", "G99"));
    assert_eq!(r.last().unwrap().to, at(10.0, 5.0, 7.0));
}

/// A cycle with a dwell pauses at the bottom.
#[test]
fn a_dwell_cycle_pauses_at_the_bottom() {
    let steps = read("G21 G90\nG0 X0 Y0 Z20\nG82 X1 Y1 Z-1 R2 P0.5 F100\nG80").unwrap();
    assert!(steps.iter().any(|s| s.action == Action::Dwell(0.5)));
}
