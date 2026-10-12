use super::*;
use crate::path::toolpath::MoveKind;
use camcraft_geom::heightmap::HeightMap;
use glam::dvec2;

pub(super) fn params() -> LinkParams {
    LinkParams::plain(800.0, 300.0, 3.0)
}

/// Link over no material and no part.
fn plain<C: Clearance>(
    path: &Toolpath,
    clearance: &C,
    params: &LinkParams,
) -> Result<(Toolpath, LinkReport), LinkError> {
    link(path, clearance, &mut Air, &NoPart, params)
}

/// Two short passes with a link in front of each, as a strategy would emit.
fn strategy_output() -> Toolpath {
    let mut t = Toolpath::new();
    t.feed_to(dvec3(0.0, 0.0, -2.0), MoveRole::Link, 1000.0)
        .unwrap();
    t.feed_to(dvec3(5.0, 0.0, -2.0), MoveRole::Cut, 1000.0)
        .unwrap();
    t.feed_to(dvec3(5.0, 1.0, -3.0), MoveRole::Link, 1000.0)
        .unwrap();
    t.feed_to(dvec3(0.0, 1.0, -3.0), MoveRole::Cut, 1000.0)
        .unwrap();
    t
}

pub(super) fn roles(path: &Toolpath) -> Vec<MoveRole> {
    path.moves().iter().map(|m| m.role).collect()
}

#[test]
fn rejects_unusable_parameters() {
    let p = ClearancePlane::new(10.0).unwrap();
    for bad in [
        LinkParams {
            retract_feed: 0.0,
            ..params()
        },
        LinkParams {
            plunge_feed: f64::NAN,
            ..params()
        },
        LinkParams {
            tool_radius: -1.0,
            ..params()
        },
        LinkParams {
            safe_distance: -1.0,
            ..params()
        },
        LinkParams {
            lead_radius: f64::NAN,
            ..params()
        },
        LinkParams {
            ramp: Some(Ramp {
                angle: 0.0,
                feed: 100.0,
            }),
            ..params()
        },
    ] {
        assert!(plain(&strategy_output(), &p, &bad).is_err());
    }
    assert!(ClearancePlane::new(f64::INFINITY).is_err());
}

/// Each refusal says what the parameter must be: a feed more than nothing, a
/// distance at least nothing, a ramp's angle less than straight down.
#[test]
fn a_refusal_says_what_was_wanted() {
    let p = ClearancePlane::new(10.0).unwrap();
    let refusal = |bad: LinkParams| plain(&strategy_output(), &p, &bad).unwrap_err().to_string();
    assert_eq!(
        refusal(LinkParams {
            plunge_feed: 0.0,
            ..params()
        }),
        "plunge_feed must be finite and greater than zero, got 0"
    );
    assert_eq!(
        refusal(LinkParams {
            safe_distance: -1.0,
            ..params()
        }),
        "safe_distance must be finite and not negative, got -1"
    );
    assert_eq!(
        refusal(LinkParams {
            ramp: Some(Ramp {
                angle: 2.0,
                feed: 100.0
            }),
            ..params()
        }),
        "ramp angle must be greater than zero and less than a right angle, in radians, got 2"
    );
    assert_eq!(
        ClearancePlane::new(f64::NAN).unwrap_err().to_string(),
        "clearance must be finite, got NaN"
    );
    let field = HeightMap::filled(dvec2(0.0, 0.0), 1.0, 2, 2, 0.0).unwrap();
    assert_eq!(
        ClearanceField::new(field, 0.0, 5.0).map(|_| ()),
        Ok(()),
        "a margin of nothing is a margin"
    );
}

/// The whole point: a linked path is machinable, and can prove it.
#[test]
fn resolves_every_link() {
    let input = strategy_output();
    assert!(input.has_unplanned_links());

    let plane = ClearancePlane::new(10.0).unwrap();
    let (out, report) = plain(&input, &plane, &params()).unwrap();

    assert!(!out.has_unplanned_links(), "output must be machinable");
    assert_eq!(report.links_resolved, 2);
}

/// Linking decides how to travel between cuts. It must not touch the cuts.
#[test]
fn cutting_moves_pass_through_unchanged() {
    let input = strategy_output();
    let plane = ClearancePlane::new(10.0).unwrap();
    let (out, _) = plain(&input, &plane, &params()).unwrap();

    let cuts = |t: &Toolpath| -> Vec<Move> {
        t.moves()
            .iter()
            .filter(|m| m.role == MoveRole::Cut)
            .copied()
            .collect()
    };
    assert_eq!(cuts(&input), cuts(&out), "cuts were altered by linking");
}

/// Every traverse happens at the safe height, not somewhere convenient.
#[test]
fn traverses_happen_at_clearance() {
    let plane = ClearancePlane::new(12.5).unwrap();
    let (out, _) = plain(&strategy_output(), &plane, &params()).unwrap();

    let mut at = DVec3::ZERO;
    let mut across = 0;
    for (i, m) in out.moves().iter().enumerate() {
        let moved = (m.to.truncate() - at.truncate()).length() > 1e-9;
        if i > 0 && m.role == MoveRole::Rapid && moved {
            assert_eq!(
                m.to.z, 12.5,
                "rapid across at {:?} is not at clearance",
                m.to
            );
            assert_eq!(at.z, 12.5, "rapid across from {at:?} is not at clearance");
            across += 1;
        }
        at = m.to;
    }
    assert_eq!(across, 1);
}

/// Retracts go straight up and plunges straight down. A diagonal exit drags the
/// cutter sideways through the wall it just cut.
#[test]
fn retracts_and_plunges_are_vertical() {
    let plane = ClearancePlane::new(10.0).unwrap();
    let (out, _) = plain(&strategy_output(), &plane, &params()).unwrap();

    let mut at = DVec3::ZERO;
    let (mut retracts, mut plunges) = (0, 0);
    for (i, m) in out.moves().iter().enumerate() {
        if i > 0 {
            match m.role {
                MoveRole::Retract => {
                    assert_eq!(m.to.truncate(), at.truncate(), "retract moved sideways");
                    assert!(m.to.z > at.z, "retract must go up");
                    retracts += 1;
                }
                MoveRole::Plunge => {
                    assert_eq!(m.to.truncate(), at.truncate(), "plunge moved sideways");
                    assert!(m.to.z < at.z, "plunge must go down");
                    plunges += 1;
                }
                _ => {}
            }
        }
        at = m.to;
    }
    assert_eq!(plunges, 2);
    assert_eq!(retracts, 1, "only the second link follows a cut");
}

/// Nothing has been cut yet at the first link, so there is nothing to retract
/// from. Emitting one would be a move from an unknown position.
#[test]
fn the_first_link_does_not_retract() {
    let plane = ClearancePlane::new(10.0).unwrap();
    let (out, _) = plain(&strategy_output(), &plane, &params()).unwrap();
    let first = out.moves().first().unwrap();
    assert_eq!(
        first.role,
        MoveRole::Rapid,
        "a program must open by moving to clearance, not by retracting from nowhere"
    );
}

/// Out of a cut at the retract feed, into one at the plunge feed.
///
/// Counted, not just filtered: an earlier version of this test only iterated
/// over retract moves and asserted their feed, so replacing the retract with a
/// rapid left zero of them and the test passed vacuously.
#[test]
fn retracts_and_plunges_use_their_feeds() {
    let plane = ClearancePlane::new(10.0).unwrap();
    let (out, _) = plain(&strategy_output(), &plane, &params()).unwrap();
    let feeds = |role| -> Vec<Option<f64>> {
        out.moves()
            .iter()
            .filter(|m| m.role == role)
            .map(|m| m.feed)
            .collect()
    };
    assert_eq!(feeds(MoveRole::Retract), [Some(800.0)]);
    assert_eq!(feeds(MoveRole::Plunge), [Some(300.0), Some(300.0)]);
}

/// Flat at 0 over the whole field: the material every entry here goes into.
pub(super) fn level_material() -> Unchanging {
    Unchanging(HeightMap::filled(dvec2(-20.0, -20.0), 0.5, 81, 81, 0.0).unwrap())
}

/// Fusion's safe distance: the tool rapids down until it is that far above the
/// material, and only feeds from there; and feeds out of a cut only until it is
/// that far clear, then rapids. Not metres of air at the plunge feed.
#[test]
fn feeds_only_within_the_safe_distance_of_the_material() {
    let plane = ClearancePlane::new(30.0).unwrap();
    let (out, _) = link(
        &strategy_output(),
        &plane,
        &mut level_material(),
        &NoPart,
        &params(),
    )
    .unwrap();
    let moves = out.moves();
    for (i, m) in moves.iter().enumerate() {
        let before = if i > 0 { moves[i - 1].to } else { DVec3::ZERO };
        match m.role {
            MoveRole::Plunge => assert_eq!(before.z, 2.0, "plunge starts at {before:?}"),
            MoveRole::Retract => assert_eq!(m.to.z, 2.0, "retract ends at {:?}", m.to),
            _ => {}
        }
    }
    assert_eq!(
        roles(&out),
        [
            MoveRole::Rapid,
            MoveRole::Rapid,
            MoveRole::Plunge,
            MoveRole::Cut,
            MoveRole::Retract,
            MoveRole::Rapid,
            MoveRole::Rapid,
            MoveRole::Rapid,
            MoveRole::Plunge,
            MoveRole::Cut,
        ]
    );
}

/// A flat part at `z`.
pub(super) struct FlatPart(pub(super) f64);

impl Surface for FlatPart {
    fn at(&self, _p: DVec2) -> Option<f64> {
        Some(self.0)
    }

    fn along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
        vec![from.extend(self.0), to.extend(self.0)]
    }
}

/// A part flat at 0 with a gentle ridge 1 mm high across x = 1 to 4, at its
/// highest at x = 2.5.
struct Ridge;

impl Surface for Ridge {
    fn at(&self, p: DVec2) -> Option<f64> {
        let u = (p.x - 2.5) / 1.5;
        Some((1.0 - u * u).max(0.0))
    }

    fn along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
        (0..=100)
            .map(|k| {
                let p = from.lerp(to, f64::from(k) / 100.0);
                p.extend(self.at(p).unwrap_or(0.0))
            })
            .collect()
    }
}

/// Two passes on the part at 0, the second `gap` along from the first's end.
pub(super) fn zigzag(gap: f64) -> Toolpath {
    let mut t = Toolpath::new();
    t.feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Link, 1000.0)
        .unwrap();
    t.feed_to(dvec3(5.0, 0.0, 0.0), MoveRole::Cut, 1000.0)
        .unwrap();
    t.feed_to(dvec3(5.0, gap, 0.0), MoveRole::Link, 1000.0)
        .unwrap();
    t.feed_to(dvec3(0.0, gap, 0.0), MoveRole::Cut, 1000.0)
        .unwrap();
    t
}

fn staying(max: f64) -> LinkParams {
    LinkParams {
        stay_down: StayDown::OverPart { max },
        ..params()
    }
}

/// A wall 10 mm tall at y = 1 and beyond, and nothing short of it.
struct Wall;

impl Surface for Wall {
    fn at(&self, p: DVec2) -> Option<f64> {
        Some(if p.y >= 1.0 { 10.0 } else { 0.0 })
    }

    fn along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
        vec![
            from.extend(self.at(from).unwrap_or(0.0)),
            to.extend(self.at(to).unwrap_or(0.0)),
        ]
    }
}

/// Passes beside a wall, as a waterline's run, are below where the tool would
/// come to rest from above: the way from one up to the next is through the wall,
/// so it lifts, however near they are.
#[test]
fn passes_beside_a_wall_are_not_joined_over_the_part() {
    let mut t = Toolpath::new();
    t.feed_to(dvec3(0.0, 1.5, 2.0), MoveRole::Link, 1000.0)
        .unwrap();
    t.feed_to(dvec3(5.0, 1.5, 2.0), MoveRole::Cut, 1000.0)
        .unwrap();
    t.feed_to(dvec3(5.0, 1.8, 8.0), MoveRole::Link, 1000.0)
        .unwrap();
    t.feed_to(dvec3(0.0, 1.8, 8.0), MoveRole::Cut, 1000.0)
        .unwrap();
    let plane = ClearancePlane::new(20.0).unwrap();
    let (out, report) = link(&t, &plane, &mut Air, &Wall, &staying(50.0)).unwrap();
    assert_eq!(report.stayed_down, 0);
    assert!(!out.moves().iter().any(|m| m.role == MoveRole::Transition));
}

/// Zigzag passes a stepover apart are joined without lifting, at the cutting
/// feed, as Fusion's are, by its default smooth transition: a half turn out
/// past their ends, tangent to both. Further apart than the stay-down
/// distance, they lift.
#[test]
fn near_passes_are_joined_without_lifting() {
    let plane = ClearancePlane::new(10.0).unwrap();
    let (out, report) = link(
        &zigzag(1.0),
        &plane,
        &mut Air,
        &FlatPart(0.0),
        &staying(50.0),
    )
    .unwrap();
    assert_eq!(report.stayed_down, 1);
    assert_eq!(
        roles(&out),
        [
            MoveRole::Rapid,
            MoveRole::Rapid,
            MoveRole::Plunge,
            MoveRole::Cut,
            MoveRole::Transition,
            MoveRole::Cut,
        ]
    );
    let transition = out.moves()[4];
    assert_eq!(transition.feed, Some(1000.0));
    assert_eq!(transition.to, dvec3(5.0, 1.0, 0.0));
    let MoveKind::Arc(arc) = transition.kind else {
        panic!("a straight join: {transition:?}");
    };
    assert_eq!(arc.sweep.abs(), std::f64::consts::PI);
    let from = dvec3(5.0, 0.0, 0.0);
    // On along +x where the pass left off, out past the end, back along -x.
    assert!(arc.tangent_at(from, transition.to, 0.0).distance(DVec3::X) < 1e-9);
    assert!(arc.tangent_at(from, transition.to, 1.0).distance(-DVec3::X) < 1e-9);
    assert!((arc.point_at(from, transition.to, 0.5).x - 5.5).abs() < 1e-9);

    let (far, report) = link(
        &zigzag(60.0),
        &plane,
        &mut Air,
        &FlatPart(0.0),
        &staying(50.0),
    )
    .unwrap();
    assert_eq!(report.stayed_down, 0);
    assert!(!roles(&far).contains(&MoveRole::Transition));
}

/// A half turn is made only where it fits: where the part rises into it, it
/// is the straight line lifted over the part; between passes that run the
/// same way, or further apart than the tool is wide, it is the straight line.
#[test]
fn a_half_turn_only_where_it_fits() {
    fn joins<S: Surface>(t: &Toolpath, part: &S) -> Vec<MoveKind> {
        let plane = ClearancePlane::new(10.0).unwrap();
        let (out, report) = link(t, &plane, &mut Air, part, &staying(50.0)).unwrap();
        assert_eq!(report.stayed_down, 1);
        out.moves()
            .iter()
            .filter(|m| m.role == MoveRole::Transition)
            .map(|m| m.kind)
            .collect()
    }
    let arcs = |kinds: &[MoveKind]| kinds.iter().any(|k| matches!(k, MoveKind::Arc(_)));
    assert!(arcs(&joins(&zigzag(1.0), &FlatPart(0.0))));
    // A step up just past the passes' ends, where the turn would go.
    struct Step;
    impl Surface for Step {
        fn at(&self, p: DVec2) -> Option<f64> {
            Some(if p.x > 5.2 { 3.0 } else { 0.0 })
        }
        fn along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
            vec![from.extend(0.0), to.extend(0.0)]
        }
    }
    assert!(!arcs(&joins(&zigzag(1.0), &Step)));
    // Wider than the tool (radius 3).
    assert!(!arcs(&joins(&zigzag(7.0), &FlatPart(0.0))));
    // One way: the second pass runs as the first did.
    let mut one_way = Toolpath::new();
    one_way
        .feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Link, 1000.0)
        .unwrap();
    one_way
        .feed_to(dvec3(5.0, 0.0, 0.0), MoveRole::Cut, 1000.0)
        .unwrap();
    one_way
        .feed_to(dvec3(5.0, 1.0, 0.0), MoveRole::Link, 1000.0)
        .unwrap();
    one_way
        .feed_to(dvec3(10.0, 1.0, 0.0), MoveRole::Cut, 1000.0)
        .unwrap();
    assert!(!arcs(&joins(&one_way, &FlatPart(0.0))));
}

/// Where one pass ends short of the other, the turn is at the further end:
/// straight on along the shorter first, or after, tangent all the way.
#[test]
fn a_turn_between_passes_of_different_lengths() {
    let plane = ClearancePlane::new(10.0).unwrap();
    let turned = |next_start: f64| {
        let mut t = Toolpath::new();
        t.feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Link, 1000.0)
            .unwrap();
        t.feed_to(dvec3(5.0, 0.0, 0.0), MoveRole::Cut, 1000.0)
            .unwrap();
        t.feed_to(dvec3(next_start, 1.0, 0.0), MoveRole::Link, 1000.0)
            .unwrap();
        t.feed_to(dvec3(-1.0, 1.0, 0.0), MoveRole::Cut, 1000.0)
            .unwrap();
        let (out, _) = link(&t, &plane, &mut Air, &FlatPart(0.0), &staying(50.0)).unwrap();
        out.moves()
            .iter()
            .filter(|m| m.role == MoveRole::Transition)
            .map(|m| (m.to, matches!(m.kind, MoveKind::Arc(_))))
            .collect::<Vec<_>>()
    };
    // The next pass starts further on: on to there, then round.
    assert_eq!(
        turned(7.0),
        [(dvec3(7.0, 0.0, 0.0), false), (dvec3(7.0, 1.0, 0.0), true)]
    );
    // It starts short: round at this pass's end, then back along to it.
    assert_eq!(
        turned(3.0),
        [(dvec3(5.0, 1.0, 0.0), true), (dvec3(3.0, 1.0, 0.0), false)]
    );
}

/// A link that stays down rides over the part where it rises above the line
/// between the cuts, and never reaches into it.
#[test]
fn staying_down_rides_over_the_part() {
    let mut t = Toolpath::new();
    t.feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Link, 1000.0)
        .unwrap();
    t.feed_to(dvec3(0.0, 1.0, 0.0), MoveRole::Cut, 1000.0)
        .unwrap();
    t.feed_to(dvec3(5.0, 1.0, 0.0), MoveRole::Link, 1000.0)
        .unwrap();
    t.feed_to(dvec3(5.0, 2.0, 0.0), MoveRole::Cut, 1000.0)
        .unwrap();
    let plane = ClearancePlane::new(10.0).unwrap();
    let (out, report) = link(&t, &plane, &mut Air, &Ridge, &staying(50.0)).unwrap();
    assert_eq!(report.stayed_down, 1);
    let over: Vec<DVec3> = out
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Transition)
        .map(|m| m.to)
        .collect();
    assert!(over.iter().any(|p| p.z == 1.0), "it rises over the ridge");
    for p in &over {
        assert!(
            p.z >= Ridge.at(p.truncate()).unwrap(),
            "{p:?} is in the part"
        );
    }
}

/// Descending further than `max_cut_step` in one steep fall at the cutting feed
/// is the plunge the gate refuses, so such a link lifts clear instead.
#[test]
fn a_link_down_a_wall_lifts_clear() {
    let mut t = Toolpath::new();
    t.feed_to(dvec3(0.0, 0.0, 5.0), MoveRole::Link, 1000.0)
        .unwrap();
    t.feed_to(dvec3(1.0, 0.0, 5.0), MoveRole::Cut, 1000.0)
        .unwrap();
    t.feed_to(dvec3(1.1, 0.0, 0.0), MoveRole::Link, 1000.0)
        .unwrap();
    t.feed_to(dvec3(3.0, 0.0, 0.0), MoveRole::Cut, 1000.0)
        .unwrap();
    let plane = ClearancePlane::new(10.0).unwrap();
    let (out, report) = link(&t, &plane, &mut Air, &FlatPart(-10.0), &staying(50.0)).unwrap();
    assert_eq!(report.stayed_down, 0);
    assert!(!roles(&out).contains(&MoveRole::Transition));
}

/// A link that carries on a fall the cut before it was making, or that the cut
/// after it carries on, is judged with it: each alone is within `max_cut_step`,
/// together they are the plunge the gate refuses, so the link lifts clear.
#[test]
fn a_link_joining_a_steep_fall_lifts_clear() {
    // A cut down a wall, 0.3 mm, then a link 0.3 mm further down.
    let mut before = Toolpath::new();
    before
        .feed_to(dvec3(0.0, 0.0, 5.0), MoveRole::Link, 1000.0)
        .unwrap();
    before
        .feed_to(dvec3(1.0, 0.0, 5.0), MoveRole::Cut, 1000.0)
        .unwrap();
    before
        .feed_to(dvec3(1.0, 0.0, 4.7), MoveRole::Cut, 1000.0)
        .unwrap();
    before
        .feed_to(dvec3(1.01, 0.0, 4.4), MoveRole::Link, 1000.0)
        .unwrap();
    before
        .feed_to(dvec3(3.0, 0.0, 4.4), MoveRole::Cut, 1000.0)
        .unwrap();
    // A link 0.3 mm down onto a cut that goes on down 0.3 mm.
    let mut after = Toolpath::new();
    after
        .feed_to(dvec3(0.0, 0.0, 5.0), MoveRole::Link, 1000.0)
        .unwrap();
    after
        .feed_to(dvec3(1.0, 0.0, 5.0), MoveRole::Cut, 1000.0)
        .unwrap();
    after
        .feed_to(dvec3(1.01, 0.0, 4.7), MoveRole::Link, 1000.0)
        .unwrap();
    after
        .feed_to(dvec3(1.02, 0.0, 4.4), MoveRole::Cut, 1000.0)
        .unwrap();
    after
        .feed_to(dvec3(3.0, 0.0, 4.4), MoveRole::Cut, 1000.0)
        .unwrap();
    let t_max = || staying(50.0).max_cut_step;
    let plane = ClearancePlane::new(10.0).unwrap();
    for t in [before, after] {
        let (out, report) = link(&t, &plane, &mut Air, &FlatPart(-10.0), &staying(50.0)).unwrap();
        assert_eq!(report.stayed_down, 0, "{:?}", out.moves());
        let (_, deepest) = out.steep_cut_descents(DVec3::ZERO, PLUNGE_RATIO);
        assert!(deepest <= t_max(), "{deepest}");
    }
    // A fall the link does not join is its own: a shallow link after one.
    let mut apart = Toolpath::new();
    apart
        .feed_to(dvec3(0.0, 0.0, 5.0), MoveRole::Link, 1000.0)
        .unwrap();
    apart
        .feed_to(dvec3(1.0, 0.0, 4.7), MoveRole::Cut, 1000.0)
        .unwrap();
    apart
        .feed_to(dvec3(1.0, 0.0, 4.4), MoveRole::Cut, 1000.0)
        .unwrap();
    apart
        .feed_to(dvec3(3.0, 0.0, 4.4), MoveRole::Link, 1000.0)
        .unwrap();
    apart
        .feed_to(dvec3(5.0, 0.0, 4.4), MoveRole::Cut, 1000.0)
        .unwrap();
    let (_, report) = link(&apart, &plane, &mut Air, &FlatPart(-10.0), &staying(50.0)).unwrap();
    assert_eq!(report.stayed_down, 1);
}

/// A level link stays down only where it does not plough through material
/// standing between the cuts it joins: across uncut stock it lifts, as
/// Fusion's does, however short it is.
#[test]
fn a_level_link_lifts_over_uncut_stock() {
    let plane = ClearancePlane::new(10.0).unwrap();
    let level = LinkParams {
        stay_down: StayDown::Level { max: 50.0 },
        ..params()
    };
    let path = zigzag(12.0);
    let (_, cleared) = link(
        &path,
        &plane,
        &mut level_material(),
        &FlatPart(-1.0),
        &level,
    )
    .unwrap();
    assert_eq!(cleared.stayed_down, 1, "the way is cleared");
    // A wall of stock 5 mm up across the middle of the way.
    let mut field = HeightMap::filled(dvec2(-20.0, -20.0), 0.5, 81, 81, 0.0).unwrap();
    for ix in 0..81 {
        for iy in 50..54 {
            field.set(ix, iy, 5.0);
        }
    }
    let (_, blocked) = link(
        &path,
        &plane,
        &mut Unchanging(field),
        &FlatPart(-1.0),
        &level,
    )
    .unwrap();
    assert_eq!(blocked.stayed_down, 0, "the way is through stock");
}

/// Clearing at a level stays down only level, and only where the part does not
/// reach up into the way.
#[test]
fn a_level_link_stays_down_only_clear_of_the_part() {
    let plane = ClearancePlane::new(10.0).unwrap();
    let level = LinkParams {
        stay_down: StayDown::Level { max: 50.0 },
        ..params()
    };
    let (_, clear) = link(&zigzag(1.0), &plane, &mut Air, &FlatPart(-1.0), &level).unwrap();
    assert_eq!(clear.stayed_down, 1);
    let (_, reached) = link(&zigzag(1.0), &plane, &mut Air, &FlatPart(1.0), &level).unwrap();
    assert_eq!(reached.stayed_down, 0);
    let mut deeper = Toolpath::new();
    deeper
        .feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Link, 1000.0)
        .unwrap();
    deeper
        .feed_to(dvec3(5.0, 0.0, 0.0), MoveRole::Cut, 1000.0)
        .unwrap();
    deeper
        .feed_to(dvec3(5.0, 1.0, -1.0), MoveRole::Link, 1000.0)
        .unwrap();
    deeper
        .feed_to(dvec3(0.0, 1.0, -1.0), MoveRole::Cut, 1000.0)
        .unwrap();
    // Down to a lower level it goes across at the height it is, then down:
    // as a profile goes to its ring's next depth.
    let (out, stepped) = link(&deeper, &plane, &mut Air, &FlatPart(-5.0), &level).unwrap();
    assert_eq!(stepped.stayed_down, 1);
    assert_eq!(
        roles(&out)[4..6],
        [MoveRole::Transition, MoveRole::Plunge],
        "{:?}",
        out.moves()
    );
    assert_eq!(out.moves()[4].to, dvec3(5.0, 1.0, 0.0), "across, level");
    // Up to a higher one it lifts clear.
    let higher = {
        let mut t = Toolpath::new();
        for m in deeper.moves() {
            t.push(Move {
                to: dvec3(m.to.x, m.to.y, -m.to.z),
                ..*m
            })
            .unwrap();
        }
        t
    };
    let (_, up) = link(&higher, &plane, &mut Air, &FlatPart(-5.0), &level).unwrap();
    assert_eq!(up.stayed_down, 0, "never across and up");
}

/// The tool arrives at the clearance height above the first pass, and only
/// then comes down to where it goes across between passes; a full
/// retraction never goes across lower than the retract height.
#[test]
fn arriving_from_the_clearance_and_retracting_fully() {
    let field = ClearanceField::new(
        HeightMap::filled(dvec2(-20.0, -20.0), 0.5, 81, 81, 0.0).unwrap(),
        2.0,
        30.0,
    )
    .unwrap();
    let arriving = LinkParams {
        start_z: Some(30.0),
        ..params()
    };
    let (out, _) = link(&zigzag(15.0), &field, &mut Air, &NoPart, &arriving).unwrap();
    let first: Vec<DVec3> = out.moves()[..2].iter().map(|m| m.to).collect();
    assert_eq!(first, [dvec3(0.0, 0.0, 30.0), dvec3(0.0, 0.0, 2.0)]);
    // Across between passes at the material's safe distance, 2 mm up.
    assert!(
        out.moves()
            .iter()
            .filter(|m| m.role == MoveRole::Rapid)
            .skip(1)
            .all(|m| m.to.z <= 2.0 + 1e-9),
        "{:?}",
        out.moves()
    );
    let full = field.at_least(5.0);
    let (out, _) = link(&zigzag(15.0), &full, &mut Air, &NoPart, &params()).unwrap();
    let across: Vec<f64> = out
        .moves()
        .windows(2)
        .filter(|w| w[1].role == MoveRole::Rapid && w[0].to.z == w[1].to.z)
        .map(|w| w[1].to.z)
        .collect();
    assert!(
        !across.is_empty() && across.iter().all(|z| *z == 5.0),
        "{across:?}"
    );
}

/// The program does not end with the cutter on the part: after the last cut it
/// goes up to the height asked for.
#[test]
fn the_last_cut_is_left_and_the_tool_raised() {
    let plane = ClearancePlane::new(10.0).unwrap();
    let ending = LinkParams {
        end_z: Some(25.0),
        ..params()
    };
    let (out, _) = plain(&strategy_output(), &plane, &ending).unwrap();
    let n = out.len();
    let (up, last) = (out.moves()[n - 2], out.moves()[n - 1]);
    assert_eq!(up.role, MoveRole::Retract);
    assert_eq!(last.role, MoveRole::Rapid);
    assert_eq!(last.to, dvec3(0.0, 1.0, 25.0));
}

/// Every move is cut into the material once, in order, each piece starting
/// from the move before it, so the pieces sweep what the whole would.
#[test]
fn the_material_is_cut_as_it_is_linked() {
    #[derive(Default)]
    struct Recording(Vec<Vec<Move>>);
    impl Material for Recording {
        fn highest(&self, _: DVec2, _: DVec2, _: f64) -> Option<f64> {
            None
        }
        fn cut(&mut self, moves: &[Move]) {
            self.0.push(moves.to_vec());
        }
    }
    let plane = ClearancePlane::new(10.0).unwrap();
    let mut rec = Recording::default();
    let ending = LinkParams {
        end_z: Some(20.0),
        ..params()
    };
    let (out, _) = link(&strategy_output(), &plane, &mut rec, &NoPart, &ending).unwrap();
    assert!(rec.0.len() >= 2, "cut as it went, not once at the end");
    let mut seen: Vec<Move> = Vec::new();
    for piece in &rec.0 {
        if let Some(last) = seen.last() {
            assert_eq!(
                piece.first(),
                Some(last),
                "each piece starts where the last ended"
            );
            seen.extend(piece.iter().skip(1));
        } else {
            seen.extend(piece.iter());
        }
    }
    assert_eq!(seen.as_slice(), out.moves());
}

/// A clearance below the work cannot produce a safe plunge, and the report has
/// to say so rather than quietly emitting it. The second entry starts from where
/// the tool came up out of the first cut, above the work, so only the first is.
#[test]
fn a_clearance_below_the_work_is_reported() {
    let plane = ClearancePlane::new(-50.0).unwrap();
    let (_, report) = link(
        &strategy_output(),
        &plane,
        &mut level_material(),
        &NoPart,
        &params(),
    )
    .unwrap();
    assert_eq!(report.suspicious_plunges, 1);
}

/// Arriving by rapid at a start already the safe distance clear of the
/// material, as a helix's is, is no plunge, whatever the clearance.
#[test]
fn a_start_clear_of_the_material_is_no_plunge() {
    let plane = ClearancePlane::new(-50.0).unwrap();
    let (_, report) = plain(&strategy_output(), &plane, &params()).unwrap();
    assert_eq!(report.suspicious_plunges, 0);
}

/// The clearance source is asked about the traverse, not about a fixed height.
/// This is what lets a varying field drop in later without touching callers.
#[test]
fn clearance_is_queried_per_traverse() {
    struct Recording(std::cell::RefCell<Vec<(DVec2, DVec2, f64)>>);
    impl Clearance for Recording {
        fn safe_z(&self, from: DVec2, to: DVec2, r: f64) -> f64 {
            self.0.borrow_mut().push((from, to, r));
            20.0
        }
    }
    let rec = Recording(std::cell::RefCell::new(Vec::new()));
    let (_, _) = plain(&strategy_output(), &rec, &params()).unwrap();

    let calls = rec.0.borrow();
    assert_eq!(calls.len(), 2, "one query per link");
    let second = calls.get(1).unwrap();
    assert_eq!(
        second.0,
        dvec2(5.0, 0.0),
        "asked from the end of the last cut"
    );
    assert_eq!(second.1, dvec2(5.0, 1.0), "asked to the start of the next");
    assert_eq!(second.2, 3.0, "tool radius is passed through");
}

pub(super) fn stepped_field() -> HeightMap {
    // Flat at 0, with a 20 mm boss in the middle.
    let mut f = HeightMap::filled(dvec2(-10.0, -10.0), 1.0, 21, 21, 0.0).unwrap();
    for iy in 8..13 {
        for ix in 8..13 {
            f.set(ix, iy, 20.0);
        }
    }
    f
}

/// Fields and planes must be interchangeable at the call site, since that is the
/// entire reason clearance is a trait.
#[test]
fn a_field_substitutes_for_a_plane() {
    let field = ClearanceField::new(stepped_field(), 5.0, 0.0).unwrap();
    let (out, report) = plain(&strategy_output(), &field, &params()).unwrap();
    assert!(!out.has_unplanned_links());
    assert_eq!(report.links_resolved, 2);
}

#[test]
fn an_empty_path_links_to_an_empty_path() {
    let plane = ClearancePlane::new(10.0).unwrap();
    let (out, report) = plain(&Toolpath::new(), &plane, &params()).unwrap();
    assert!(out.is_empty());
    assert_eq!(report, LinkReport::default());
}
