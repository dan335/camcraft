use super::*;
use crate::path::linker::tests::{FlatPart, level_material, params, roles, zigzag};
use crate::path::linker::{Air, ClearancePlane, LinkParams, Unchanging, link};
use crate::path::toolpath::{MoveKind, Toolpath};
use camcraft_geom::heightmap::HeightMap;
use glam::dvec2;

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

fn staying(max: f64) -> LinkParams {
    LinkParams {
        stay_down: StayDown::OverPart { max },
        ..params()
    }
}

/// A part flat at 0, the tool resting on faces it avoids alone between
/// x = 2 and 3.
struct AvoidedStrip;

impl Surface for AvoidedStrip {
    fn at(&self, _p: DVec2) -> Option<f64> {
        Some(0.0)
    }

    fn along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
        (0..=100)
            .map(|k| from.lerp(to, f64::from(k) / 100.0).extend(0.0))
            .collect()
    }

    fn rests_avoided(&self, p: DVec3) -> bool {
        (2.0..=3.0).contains(&p.x)
    }
}

/// Two passes along a line, either side of where the tool would rest on
/// faces it avoids: the link between does not stay down there, rubbing them,
/// but lifts; with nothing avoided it stays down.
#[test]
fn a_link_over_faces_avoided_lifts() {
    let mut t = Toolpath::new();
    t.feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Link, 1000.0)
        .unwrap();
    t.feed_to(dvec3(1.5, 0.0, 0.0), MoveRole::Cut, 1000.0)
        .unwrap();
    t.feed_to(dvec3(3.5, 0.0, 0.0), MoveRole::Link, 1000.0)
        .unwrap();
    t.feed_to(dvec3(5.0, 0.0, 0.0), MoveRole::Cut, 1000.0)
        .unwrap();
    let plane = ClearancePlane::new(10.0).unwrap();
    let (out, report) = link(&t, &plane, &mut Air, &AvoidedStrip, &staying(50.0)).unwrap();
    assert_eq!(report.stayed_down, 0);
    assert!(!roles(&out).contains(&MoveRole::Transition));
    let (_, report) = link(&t, &plane, &mut Air, &FlatPart(0.0), &staying(50.0)).unwrap();
    assert_eq!(report.stayed_down, 1);
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

/// A block 10 tall at y = 1 and beyond, and nothing short of it: off it, the
/// tool falls past its side.
struct Block;

impl Surface for Block {
    fn at(&self, p: DVec2) -> Option<f64> {
        (p.y >= 1.0).then_some(10.0)
    }

    fn along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
        (0..=100)
            .map(|k| from.lerp(to, f64::from(k) / 100.0))
            .filter_map(|p| Some(p.extend(self.at(p)?)))
            .collect()
    }
}

/// Rings down the side of a part, below where the tool would rest from
/// above, with nothing under it: the way from one to the next, a little
/// lower, beside the side and never into it, stays down, as a link at a level
/// does; one that would go into the part lifts.
#[test]
fn rings_down_a_side_are_joined_beside_it() {
    let side = |to: DVec3| {
        let mut t = Toolpath::new();
        t.feed_to(dvec3(0.0, 0.5, 4.0), MoveRole::Link, 1000.0)
            .unwrap();
        t.feed_to(dvec3(5.0, 0.5, 4.0), MoveRole::Cut, 1000.0)
            .unwrap();
        t.feed_to(to, MoveRole::Link, 1000.0).unwrap();
        t.feed_to(to + DVec3::X, MoveRole::Cut, 1000.0).unwrap();
        t
    };
    let plane = ClearancePlane::new(20.0).unwrap();
    let beside = side(dvec3(6.0, 0.5, 3.8));
    let (out, report) = link(&beside, &plane, &mut Air, &Block, &staying(50.0)).unwrap();
    assert_eq!(report.stayed_down, 1);
    let way: Vec<DVec3> = out
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Transition)
        .map(|m| m.to)
        .collect();
    assert_eq!(way, vec![dvec3(6.0, 0.5, 3.8)]);
    // Across into the block, it lifts.
    let into = side(dvec3(6.0, 1.5, 3.8));
    let (_, report) = link(&into, &plane, &mut Air, &Block, &staying(50.0)).unwrap();
    assert_eq!(report.stayed_down, 0);
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

/// Passes in line, the next starting on ahead and coming back along the last,
/// are joined by the straight line between them, not a turn: a radial spoke
/// that stops short of the middle, and the next one out from it, whose lines
/// are a micron or two apart there by rounding. A turn that small is no arc
/// the path takes, and the operation was refused for it.
#[test]
fn passes_in_line_are_joined_straight() {
    let plane = ClearancePlane::new(10.0).unwrap();
    for apart in [0.0, 1.0e-9, 1.6e-6, 0.005, 0.015] {
        let mut t = Toolpath::new();
        t.feed_to(dvec3(15.0, 0.0, 0.0), MoveRole::Link, 1000.0)
            .unwrap();
        t.feed_to(dvec3(10.0, 0.0, 0.0), MoveRole::Cut, 1000.0)
            .unwrap();
        t.feed_to(dvec3(0.0, apart, 0.0), MoveRole::Link, 1000.0)
            .unwrap();
        t.feed_to(dvec3(8.0, apart, 0.0), MoveRole::Cut, 1000.0)
            .unwrap();
        let (out, report) = link(&t, &plane, &mut Air, &FlatPart(0.0), &staying(50.0))
            .unwrap_or_else(|e| panic!("{apart} apart: {e}"));
        assert_eq!(report.stayed_down, 1, "{apart} apart");
        let joins: Vec<&Move> = out
            .moves()
            .iter()
            .filter(|m| m.role == MoveRole::Transition)
            .collect();
        assert!(
            joins.iter().all(|m| m.kind == MoveKind::Linear),
            "{apart} apart: {joins:?}"
        );
        assert_eq!(joins.last().unwrap().to, dvec3(0.0, apart, 0.0));
    }
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

/// A part flat at -5, the tool beside faces it avoids wherever it is between
/// x = 2 and 3, rubbing them.
struct AvoidedBeside;

impl Surface for AvoidedBeside {
    fn at(&self, _p: DVec2) -> Option<f64> {
        Some(-5.0)
    }

    fn along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
        vec![from.extend(-5.0), to.extend(-5.0)]
    }

    fn rubs_avoided(&self, from: DVec3, to: DVec3) -> bool {
        from.x.min(to.x) <= 3.0 && from.x.max(to.x) >= 2.0
    }
}

/// Two passes along a line either side of where the tool would rub faces it
/// avoids, the second at the level of the first or below it: the link at the
/// level between them, across or across and then down, does not stay down
/// there, rubbing them, but lifts; with nothing avoided it stays down.
#[test]
fn a_level_link_beside_faces_avoided_lifts() {
    let plane = ClearancePlane::new(10.0).unwrap();
    let level = LinkParams {
        stay_down: StayDown::Level { max: 50.0 },
        ..params()
    };
    for z in [0.0, -1.0] {
        let mut t = Toolpath::new();
        t.feed_to(dvec3(0.0, 0.0, 0.0), MoveRole::Link, 1000.0)
            .unwrap();
        t.feed_to(dvec3(1.5, 0.0, 0.0), MoveRole::Cut, 1000.0)
            .unwrap();
        t.feed_to(dvec3(3.5, 0.0, z), MoveRole::Link, 1000.0)
            .unwrap();
        t.feed_to(dvec3(5.0, 0.0, z), MoveRole::Cut, 1000.0)
            .unwrap();
        let (out, rubbing) = link(&t, &plane, &mut Air, &AvoidedBeside, &level).unwrap();
        assert_eq!(rubbing.stayed_down, 0, "at {z}");
        assert!(!roles(&out).contains(&MoveRole::Transition), "at {z}");
        let (_, clear) = link(&t, &plane, &mut Air, &FlatPart(-5.0), &level).unwrap();
        assert_eq!(clear.stayed_down, 1, "at {z}");
    }
}

/// Between passes a stepover apart, running opposite ways, the half turn is
/// one arc about the point between their ends, turning the way the last was
/// heading; where the next starts further on, it goes straight on to there
/// first, and where it starts short, it goes straight back along after.
#[test]
fn a_half_turn_is_tangent_to_both_passes() {
    let (out, into) = (dvec2(1.0, 0.0), dvec2(-1.0, 0.0));
    let turn = |to: DVec3| half_turn(dvec3(5.0, 0.0, 0.0), to, out, into, 0.01, 3.0);
    let round = Arc {
        centre: dvec3(5.0, 0.5, 0.0),
        axis: DVec3::Z,
        sweep: PI,
    };
    assert_eq!(
        turn(dvec3(5.0, 1.0, 0.0)),
        Some(vec![(dvec3(5.0, 1.0, 0.0), Some(round))])
    );
    let on = Arc {
        centre: dvec3(7.0, 0.5, 0.0),
        ..round
    };
    assert_eq!(
        turn(dvec3(7.0, 1.0, 0.0)),
        Some(vec![
            (dvec3(7.0, 0.0, 0.0), None),
            (dvec3(7.0, 1.0, 0.0), Some(on))
        ])
    );
    assert_eq!(
        turn(dvec3(3.0, 1.0, 0.0)),
        Some(vec![
            (dvec3(5.0, 1.0, 0.0), Some(round)),
            (dvec3(3.0, 1.0, 0.0), None)
        ])
    );
    // Turning the other way, the next pass on the right of the last.
    let right = half_turn(
        dvec3(5.0, 0.0, 0.0),
        dvec3(5.0, -1.0, 0.0),
        out,
        into,
        0.01,
        3.0,
    );
    assert_eq!(right.unwrap()[0].1.unwrap().sweep, -PI);
}

/// No half turn between passes that run the same way, that are in line, or
/// that are further apart than the tool is wide.
#[test]
fn a_half_turn_only_between_passes_side_by_side() {
    let from = dvec3(5.0, 0.0, 0.0);
    let out = dvec2(1.0, 0.0);
    assert_eq!(
        half_turn(from, dvec3(5.0, 1.0, 0.0), out, out, 0.01, 3.0),
        None
    );
    assert_eq!(
        half_turn(from, dvec3(0.0, 0.0, 0.0), out, -out, 0.01, 3.0),
        None
    );
    assert_eq!(
        half_turn(from, dvec3(5.0, 0.015, 0.0), out, -out, 0.01, 3.0),
        None
    );
    assert_eq!(
        half_turn(from, dvec3(5.0, 7.0, 0.0), out, -out, 0.01, 3.0),
        None
    );
    assert!(half_turn(from, dvec3(5.0, 6.0, 0.0), out, -out, 0.01, 3.0).is_some());
}

/// A way falls steeply too far where, with the fall it carries on from and
/// the pass's own fall after it, one steep descent goes deeper than allowed;
/// a gentle stretch between starts the count again.
#[test]
fn a_steep_fall_is_counted_across_the_way_and_into_the_pass() {
    let from = dvec3(0.0, 0.0, 1.0);
    let down = [dvec3(0.01, 0.0, 0.7)];
    assert!(!falls_steeply(0.0, from, &down, &[], 0.5));
    assert!(falls_steeply(0.3, from, &down, &[], 0.5));
    let gentle = [dvec3(5.0, 0.0, 1.0), dvec3(5.01, 0.0, 0.7)];
    assert!(!falls_steeply(0.3, from, &gentle, &[], 0.5));
    let mut on = Toolpath::new();
    on.feed_to(dvec3(0.01, 0.0, 0.7), MoveRole::Link, 1000.0)
        .unwrap();
    on.feed_to(dvec3(0.02, 0.0, 0.4), MoveRole::Cut, 1000.0)
        .unwrap();
    assert!(falls_steeply(0.0, from, &down, &on.moves()[1..], 0.5));
}
