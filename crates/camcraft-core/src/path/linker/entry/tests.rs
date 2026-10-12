use std::f64::consts::FRAC_PI_2;

use glam::dvec2;

use super::*;
use crate::path::linker::tests::{FlatPart, level_material, params, roles, zigzag};
use crate::path::linker::{Air, ClearancePlane, LinkParams, NoPart, link};
use crate::path::toolpath::Toolpath;

/// Fusion's vertical lead: a quarter turn of the lead radius from straight
/// down to the pass, in the pass's vertical plane, ending on it level.
#[test]
fn a_lead_in_arcs_down_onto_the_pass() {
    let r = 0.6;
    let to = dvec3(1.0, 2.0, 3.0);
    let (top, arc) = lead_in(to, DVec3::X, r).unwrap();
    assert!(
        (top - dvec3(1.0 - r, 2.0, 3.0 + r)).length() < 1e-12,
        "{top:?}"
    );
    assert!(arc.ends_at(top, to, 1e-12));
    assert!((arc.centre - dvec3(1.0, 2.0, 3.0 + r)).length() < 1e-12);
    assert!((arc.sweep.abs() - FRAC_PI_2).abs() < 1e-12);
    // In the pass's plane: about Y, so every point keeps its Y.
    assert_eq!(arc.axis, DVec3::Y);
    // Straight down at the top, level as the pass runs at the bottom.
    assert!((arc.tangent_at(top, to, 0.0) + DVec3::Z).length() < 1e-9);
    assert!((arc.tangent_at(top, to, 1.0) - DVec3::X).length() < 1e-9);

    let (end, out) = lead_out(to, DVec3::X, r).unwrap();
    assert!((end - dvec3(1.0 + r, 2.0, 3.0 + r)).length() < 1e-12);
    assert!((out.tangent_at(to, end, 0.0) - DVec3::X).length() < 1e-9);
    assert!((out.tangent_at(to, end, 1.0) - DVec3::Z).length() < 1e-9);
}

/// Entered along a lead where one fits; straight down where the part rises
/// behind the pass's start, as Fusion leaves the lead out there.
#[test]
fn a_lead_is_used_only_where_it_fits() {
    let plane = ClearancePlane::new(10.0).unwrap();
    let leading = LinkParams {
        lead_radius: 0.5,
        ..params()
    };
    let (out, _) = link(&zigzag(60.0), &plane, &mut Air, &FlatPart(0.0), &leading).unwrap();
    let r = roles(&out);
    assert!(r.contains(&MoveRole::Lead));
    // In, then out of the first pass and into the second.
    assert_eq!(r.iter().filter(|x| **x == MoveRole::Plunge).count(), 2);

    // A part a millimetre above the start of the pass, behind it: no lead in.
    struct Behind;
    impl Surface for Behind {
        fn at(&self, p: DVec2) -> Option<f64> {
            Some(if p.x < -0.01 { 1.0 } else { 0.0 })
        }
        fn along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
            vec![from.extend(0.0), to.extend(0.0)]
        }
    }
    let mut t = Toolpath::new();
    t.feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Link, 1000.0)
        .unwrap();
    t.feed_to(dvec3(5.0, 0.0, 0.0), MoveRole::Cut, 1000.0)
        .unwrap();
    let (out, _) = link(&t, &plane, &mut Air, &Behind, &leading).unwrap();
    assert_eq!(
        roles(&out)[..3],
        [MoveRole::Rapid, MoveRole::Rapid, MoveRole::Plunge]
    );
    assert_eq!(
        out.moves()[2].to,
        dvec3(0.0, 0.0, 0.0),
        "plunged to the pass"
    );
}

/// A ring at -2 in material up to 0.
fn ring() -> Toolpath {
    let mut t = Toolpath::new();
    t.feed_to(dvec3(0.0, 0.0, -2.0), MoveRole::Link, 1000.0)
        .unwrap();
    for p in [(10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)] {
        t.feed_to(dvec3(p.0, p.1, -2.0), MoveRole::Cut, 1000.0)
            .unwrap();
    }
    t
}

fn entering(entry: EntryKind) -> LinkParams {
    LinkParams {
        ramp: Some(Ramp {
            angle: 2f64.to_radians(),
            feed: 250.0,
        }),
        entry,
        helix_radius: 2.0,
        ..params()
    }
}

/// Into material down a helix beside where the pass starts, off to the side
/// it climbs from and touching its line there: turns about Z, each at most a
/// whole one, ramp role and feed, from the safe distance above the material,
/// ending on the pass heading the way it runs.
#[test]
fn a_helix_goes_down_beside_the_pass() {
    let plane = ClearancePlane::new(10.0).unwrap();
    let p = entering(EntryKind::Helix);
    let (out, report) = link(&ring(), &plane, &mut level_material(), &NoPart, &p).unwrap();
    assert_eq!(report.helixes, 1);
    let moves = out.moves();
    let first = moves.iter().position(|m| m.role == MoveRole::Ramp).unwrap();
    let start = moves[first - 1].to;
    assert!(
        (start.z - 2.0).abs() < 1e-9,
        "from the safe distance: {start}"
    );
    let helix: Vec<&Move> = moves[first..]
        .iter()
        .take_while(|m| m.role == MoveRole::Ramp)
        .collect();
    let (bottom, down) = helix.split_last().unwrap();
    let mut at = start;
    let mut turned = 0.0;
    let mut before = start;
    for m in down {
        let MoveKind::Arc(a) = m.kind else {
            panic!("a helix is arcs: {m:?}");
        };
        assert!((a.centre.truncate() - dvec2(0.0, 2.0)).length() < 1e-9);
        assert!(a.sweep.abs() <= TAU + 1e-9);
        assert!(m.to.z < at.z);
        assert_eq!(m.feed, Some(250.0));
        turned += a.sweep.abs();
        before = at;
        at = m.to;
    }
    assert_eq!(at, dvec3(0.0, 0.0, -2.0), "it ends on the pass");
    let last = down.last().unwrap();
    let heading = last.tangent_at(before, 1.0).unwrap().truncate().normalize();
    assert!((heading - dvec2(1.0, 0.0)).length() < 1e-6, "{heading}");
    // Then once round level, clearing its circle to the depth.
    let MoveKind::Arc(a) = bottom.kind else {
        panic!("{bottom:?}");
    };
    assert_eq!(bottom.to, at);
    assert!((a.sweep.abs() - TAU).abs() < 1e-12);
    // Four millimetres down at two degrees round a circle of radius two.
    let expected = 4.0 / (2.0 * 2f64.to_radians().tan());
    assert!((turned - expected).abs() < 1e-6, "{turned} vs {expected}");

    // No steeper than the most a lap may go down.
    let shallow = LinkParams {
        ramp_stepdown: Some(0.1),
        ..p
    };
    let (out, _) = link(&ring(), &plane, &mut level_material(), &NoPart, &shallow).unwrap();
    let mut at = out.moves()[0].to;
    for m in out.moves() {
        if let (MoveKind::Arc(a), MoveRole::Ramp) = (m.kind, m.role) {
            let per_turn = (at.z - m.to.z) / (a.sweep.abs() / TAU);
            assert!(per_turn <= 0.1 + 1e-9, "{per_turn}");
        }
        at = m.to;
    }
}

/// The helix goes into the waste, the side the cut climbs from, never over
/// the part it is cutting beside, however low the part is there.
#[test]
fn a_helix_is_in_the_waste() {
    // The part is beside the ring, on its right as it runs along X, below
    // the pass: a helix over it would not touch it at this depth, but
    // would at the next.
    struct Right;
    impl Surface for Right {
        fn at(&self, p: DVec2) -> Option<f64> {
            (p.y < -0.01).then_some(-3.0)
        }
        fn along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
            vec![from.extend(-3.0), to.extend(-3.0)]
        }
    }
    let plane = ClearancePlane::new(10.0).unwrap();
    let p = entering(EntryKind::Helix);
    let (out, report) = link(&ring(), &plane, &mut level_material(), &Right, &p).unwrap();
    assert_eq!(report.helixes, 1);
    for m in out.moves() {
        if let (MoveKind::Arc(a), MoveRole::Ramp) = (m.kind, m.role) {
            assert!(a.centre.y > 0.0, "the helix is off the waste side: {a:?}");
        }
    }
}

/// Where the part leaves no room for a helix beside the pass, even a quarter
/// the size, the tool goes straight down instead.
#[test]
fn no_room_for_a_helix_is_a_plunge() {
    struct Behind;
    impl Surface for Behind {
        fn at(&self, p: DVec2) -> Option<f64> {
            Some(if p.x < -0.01 { 1.0 } else { -5.0 })
        }
        fn along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
            vec![from.extend(-5.0), to.extend(-5.0)]
        }
    }
    let plane = ClearancePlane::new(10.0).unwrap();
    let p = entering(EntryKind::Helix);
    let (out, report) = link(&ring(), &plane, &mut level_material(), &Behind, &p).unwrap();
    assert_eq!(report.helixes, 0);
    let plunge = out
        .moves()
        .iter()
        .find(|m| m.role == MoveRole::Plunge)
        .unwrap();
    assert_eq!(plunge.to, dvec3(0.0, 0.0, -2.0));
}

/// A ring with leads ramped into goes down along the ring itself, in the
/// direction it cuts, onto its start, and is entered there without its
/// lead-in: never back along it, against the cut.
#[test]
fn a_ring_with_leads_is_ramped_along_the_way_it_cuts() {
    let mut t = Toolpath::new();
    t.feed_to(dvec3(-1.0, -1.0, -2.0), MoveRole::Link, 1000.0)
        .unwrap();
    t.feed_to(dvec3(0.0, 0.0, -2.0), MoveRole::Lead, 1000.0)
        .unwrap();
    for p in [(10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)] {
        t.feed_to(dvec3(p.0, p.1, -2.0), MoveRole::Cut, 1000.0)
            .unwrap();
    }
    t.feed_to(dvec3(-1.0, 1.0, -2.0), MoveRole::Lead, 1000.0)
        .unwrap();
    let plane = ClearancePlane::new(10.0).unwrap();
    let p = entering(EntryKind::Ramp);
    let (out, report) = link(&t, &plane, &mut level_material(), &NoPart, &p).unwrap();
    assert_eq!(report.ramps, 1);
    let moves = out.moves();
    let first_cut = moves.iter().position(|m| m.role == MoveRole::Cut).unwrap();
    assert!(
        moves[..first_cut].iter().all(|m| m.role != MoveRole::Lead),
        "the lead-in is left out: {moves:?}"
    );
    // Down onto the ring's start along its last side, as the ring runs.
    let last = moves[first_cut - 1];
    let before = moves[first_cut - 2].to;
    assert_eq!(last.to, dvec3(0.0, 0.0, -2.0));
    assert!(before.x.abs() < 1e-9 && before.y > 0.0, "{before}");
    let mut z = f64::INFINITY;
    for m in &moves[..first_cut] {
        if m.role == MoveRole::Ramp {
            assert!(m.to.z <= z + 1e-12);
            z = m.to.z;
        }
    }
    // Round a lap of 40 mm, no more than the most a lap may go down.
    let lapped = LinkParams {
        ramp_stepdown: Some(0.5),
        ..p
    };
    let (out, _) = link(&t, &plane, &mut level_material(), &NoPart, &lapped).unwrap();
    let mut at = out.moves()[0].to;
    for m in out.moves() {
        if m.role == MoveRole::Ramp {
            let flat = (m.to - at).truncate().length();
            assert!(at.z - m.to.z <= flat * 0.5 / 40.0 + 1e-9);
        }
        at = m.to;
    }
}

/// Into material along the pass itself, at the ramp angle and feed, from the
/// safe distance above the material: no plunge, and never off the pass's path.
#[test]
fn a_pass_in_material_is_ramped_into_along_itself() {
    let plane = ClearancePlane::new(10.0).unwrap();
    let angle = 2f64.to_radians();
    let ramping = LinkParams {
        ramp: Some(Ramp { angle, feed: 250.0 }),
        ..params()
    };
    let (out, report) = link(&ring(), &plane, &mut level_material(), &NoPart, &ramping).unwrap();
    assert_eq!(report.ramps, 1);
    assert!(!roles(&out).contains(&MoveRole::Plunge));
    let ramp: Vec<&Move> = out
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Ramp)
        .collect();
    assert!(ramp.iter().all(|m| m.feed == Some(250.0)));
    assert_eq!(ramp.last().unwrap().to, dvec3(0.0, 0.0, -2.0));
    // It starts the safe distance above the material, so runs 4 mm down.
    let start = out
        .moves()
        .iter()
        .take_while(|m| m.role != MoveRole::Ramp)
        .last()
        .unwrap()
        .to;
    assert!((start.z - 2.0).abs() < 1e-9, "{start:?}");
    let mut at = start;
    let mut run = 0.0;
    for m in &ramp {
        let flat = (m.to.truncate() - at.truncate()).length();
        assert!(m.to.z <= at.z + 1e-12, "a ramp only descends");
        run += flat;
        let on_ring = [m.to.x, m.to.y]
            .iter()
            .any(|v| v.abs() < 1e-9 || (v - 10.0).abs() < 1e-9);
        assert!(on_ring, "{:?} is off the ring", m.to);
        at = m.to;
    }
    assert!((run - 4.0 / angle.tan()).abs() < 1e-6, "ran {run}");

    // In air, nothing to ramp into.
    let (out, report) = link(&ring(), &plane, &mut Air, &NoPart, &ramping).unwrap();
    assert_eq!(report.ramps, 0);
    assert!(roles(&out).contains(&MoveRole::Plunge));
}

/// A ramp rides over a tab on the pass, as the pass does, and comes straight
/// down off it as the pass does: at the plunge feed, not the ramp's.
#[test]
fn a_ramp_comes_down_off_a_tab_at_the_plunge_feed() {
    let mut t = Toolpath::new();
    t.feed_to(dvec3(0.0, 0.0, -2.0), MoveRole::Link, 1000.0)
        .unwrap();
    for p in [
        (10.0, 0.0, -2.0),
        (10.0, 4.0, -2.0),
        (10.0, 4.0, -0.5),
        (10.0, 6.0, -0.5),
        (10.0, 6.0, -2.0),
        (10.0, 10.0, -2.0),
        (0.0, 10.0, -2.0),
        (0.0, 0.0, -2.0),
    ] {
        t.feed_to(dvec3(p.0, p.1, p.2), MoveRole::Cut, 1000.0)
            .unwrap();
    }
    let plane = ClearancePlane::new(10.0).unwrap();
    let ramping = LinkParams {
        ramp: Some(Ramp {
            angle: 2f64.to_radians(),
            feed: 250.0,
        }),
        ..params()
    };
    let (out, report) = link(&t, &plane, &mut level_material(), &NoPart, &ramping).unwrap();
    assert_eq!(report.ramps, 1);
    let entry: Vec<&Move> = out
        .moves()
        .iter()
        .take_while(|m| m.role != MoveRole::Cut)
        .collect();
    let mut downs = 0;
    for w in entry.windows(2) {
        let (a, b) = (w[0].to, w[1].to);
        if b.z < a.z - 1e-9
            && a.truncate().distance(b.truncate()) < 1e-9
            && w[1].role != MoveRole::Rapid
        {
            downs += 1;
            assert_eq!(
                w[1].role,
                MoveRole::Plunge,
                "straight down in a ramp at {b}"
            );
            assert_eq!(w[1].feed, Some(ramping.plunge_feed));
        }
    }
    assert!(downs > 0, "the ramp never came down off the tab");
}
