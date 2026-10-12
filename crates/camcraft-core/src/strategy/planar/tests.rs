use super::*;
use crate::path::toolpath::MoveKind;
use glam::dvec2;

fn square(lo: f64, hi: f64) -> Contour {
    Contour::new(vec![
        dvec2(lo, lo),
        dvec2(hi, lo),
        dvec2(hi, hi),
        dvec2(lo, hi),
    ])
    .unwrap()
}

/// Levels as a 2D operation's are without a top or bottom asked for: from
/// zero down to the lowest pick, or to `fallback` with none.
fn floor(fallback: f64) -> impl Fn(Option<Picks>) -> (f64, f64) {
    move |p| (0.0, p.map_or(fallback, |p| p.lowest))
}

fn params() -> PlanarParams {
    PlanarParams {
        stepdown: Some(2.0),
        stepover: 2.0,
        feed: 1000.0,
        arc: 0.01,
        radius: 1.5,
        kept_on_right: true,
        leads: None,
        spiral: None,
    }
}

/// The cutting moves of a path, grouped into passes.
fn passes(path: &Toolpath) -> Vec<Vec<DVec3>> {
    let mut out: Vec<Vec<DVec3>> = Vec::new();
    for m in path.moves() {
        if m.role == MoveRole::Link {
            out.push(vec![m.to]);
        } else if let Some(p) = out.last_mut() {
            p.push(m.to);
        }
    }
    out
}

fn signed_area(pass: &[DVec3]) -> f64 {
    pass.iter()
        .zip(pass.iter().skip(1))
        .map(|(a, b)| a.x * b.y - b.x * a.y)
        .sum::<f64>()
        * 0.5
}

/// Whether `q` is within `d` of one of `region`'s rings.
fn rings_near(region: &Region, q: DVec2, d: f64) -> bool {
    region
        .rings()
        .iter()
        .any(|c| Ring::from_points(c.points()).is_some_and(|r| r.distance_to(q) <= d))
}

/// A profile with leads arrives on each ring along an arc in the waste,
/// tangent to the ring where it joins it, and leaves along another: outside
/// a part cut out, inside a hole. Where the waste has no room even for a
/// quarter of the arc, the ring goes without, and says so; a groove has none.
#[test]
fn leads_arc_on_and_off_in_the_waste() {
    let part = Region::new(vec![square(0.0, 40.0)]);
    let lead = LeadArc {
        radius: 4.0,
        sweep: 90f64.to_radians(),
        straight: 1.0,
    };
    let p = PlanarParams {
        leads: Some(lead),
        ..params()
    };
    for (side, offset) in [(ProfileSide::Outside, 1.5), (ProfileSide::Inside, -1.5)] {
        let areas = areas(&part, &[], offset, 0.01, &[], &floor(-1.0)).unwrap();
        let region = &areas[0].region;
        let (path, report) = profile(&areas, side, &p).unwrap();
        assert_eq!(report.no_room_to_lead, 0, "{side:?}");
        let moves = path.moves();
        let first_cut = moves.iter().position(|m| m.role == MoveRole::Cut).unwrap();
        // Link, then the lead in, a straight and a level arc, then the ring.
        assert_eq!(moves[0].role, MoveRole::Link);
        assert!(moves[1..first_cut].iter().all(|m| m.role == MoveRole::Lead));
        assert_eq!(first_cut, 3, "{side:?}");
        assert_eq!(moves[1].kind, MoveKind::Linear);
        assert!(
            matches!(moves[2].kind, MoveKind::Arc(a) if (a.radius(moves[1].to) - 4.0).abs() < 1e-9)
        );
        let mut at = moves[0].to;
        for m in moves.iter() {
            if m.role == MoveRole::Lead {
                for p in m.chords(at, 0.01) {
                    let q = p.truncate();
                    let ring = region.rings()[0].points();
                    let on_ring = ring.iter().zip(ring.iter().cycle().skip(1)).any(|(a, b)| {
                        let t = ((q - *a).dot(*b - *a) / b.distance_squared(*a)).clamp(0.0, 1.0);
                        q.distance(*a + (*b - *a) * t) < 1e-9
                    });
                    // The region is the clipping's, a hair outside the ring.
                    let on_ring = on_ring || rings_near(region, q, 0.03);
                    let inside = region.contains(q);
                    assert!(
                        on_ring || inside == (side == ProfileSide::Inside),
                        "{side:?}: a lead point at {q} is not in the waste"
                    );
                }
            }
            at = m.to;
        }
        // Joining the ring along it: the lead's arc ends heading the way the
        // ring's first cut starts.
        let join = moves[first_cut - 1];
        let along = moves[first_cut]
            .tangent_at(join.to, 0.0)
            .unwrap()
            .truncate()
            .normalize();
        let arriving = join
            .tangent_at(moves[first_cut - 2].to, 1.0)
            .unwrap()
            .truncate()
            .normalize();
        assert!(
            along.dot(arriving) > 1.0 - 1e-9,
            "{side:?}: {along} vs {arriving}"
        );
        // Left along a lead too, after the ring closes.
        assert_eq!(moves.last().unwrap().role, MoveRole::Lead);
    }

    // A 4 mm hole cut from inside: no room for even a 1 mm arc.
    let hole = Region::new(vec![square(0.0, 4.0)]);
    let small = areas(&hole, &[], -1.5, 0.01, &[], &floor(-1.0)).unwrap();
    let (path, report) = profile(&small, ProfileSide::Inside, &p).unwrap();
    assert_eq!(report.no_room_to_lead, 1);
    assert!(path.moves().iter().all(|m| m.role != MoveRole::Lead));

    let on = areas(&part, &[], 0.0, 0.01, &[], &floor(-1.0)).unwrap();
    let (path, report) = profile(&on, ProfileSide::On, &p).unwrap();
    assert_eq!(report.no_room_to_lead, 0);
    assert!(path.moves().iter().all(|m| m.role != MoveRole::Lead));
}

/// Ramped into, a ring is one pass, as Fusion cuts it: on along the lead at
/// the top, down along the ring the way it cuts, once round at each level
/// from where the ramp reached it, straight on down to the next, and off
/// along the lead at the end. Never up between.
#[test]
fn a_ramped_ring_is_one_spiral() {
    let part = Region::new(vec![square(0.0, 40.0)]);
    let areas = areas(&part, &[], 1.5, 0.01, &[], &floor(-6.0)).unwrap();
    let p = PlanarParams {
        stepdown: Some(2.0),
        leads: Some(LeadArc {
            radius: 1.0,
            sweep: 90f64.to_radians(),
            straight: 0.5,
        }),
        spiral: Some(Spiral {
            angle: 2f64.to_radians(),
            feed: 300.0,
            most: Some(2.0),
            above: 1.0,
        }),
        ..params()
    };
    let (path, report) = profile(&areas, ProfileSide::Outside, &p).unwrap();
    assert_eq!(report.passes, 3);
    let moves = path.moves();
    assert_eq!(
        moves.iter().filter(|m| m.role == MoveRole::Link).count(),
        1,
        "one pass"
    );
    assert!(
        (moves[0].to.z - 1.0).abs() < 1e-9,
        "on at the top, above it"
    );
    // Leads only at the very start and the very end.
    let first_ramp = moves.iter().position(|m| m.role == MoveRole::Ramp).unwrap();
    let last_cut = moves.iter().rposition(|m| m.role == MoveRole::Cut).unwrap();
    assert!(
        moves[1..first_ramp]
            .iter()
            .all(|m| m.role == MoveRole::Lead)
    );
    assert!(
        moves[first_ramp..=last_cut]
            .iter()
            .all(|m| m.role != MoveRole::Lead)
    );
    assert!(
        moves[last_cut + 1..]
            .iter()
            .all(|m| m.role == MoveRole::Lead)
    );
    // Never up, and every level's lap at its level, round and back.
    let mut z = f64::INFINITY;
    for m in moves.iter().skip(1) {
        assert!(m.to.z <= z + 1e-12, "up at {m:?}");
        z = m.to.z;
    }
    for level in [-2.0, -4.0, -6.0] {
        let laps = moves
            .iter()
            .filter(|m| m.role == MoveRole::Cut && (m.to.z - level).abs() < 1e-9)
            .count();
        assert!(laps > 0, "a lap at {level}");
    }
    // No steeper than two degrees, nor two millimetres a lap.
    let lap = 4.0 * 40.0 + std::f64::consts::TAU * 1.5;
    let mut at = moves[0].to;
    for m in moves {
        if m.role == MoveRole::Ramp {
            let drop = at.z - m.to.z;
            let run = m.length(at).max(1e-12);
            assert!(drop / run <= (2.0 / lap).min(2f64.to_radians().tan()) + 1e-9);
        }
        at = m.to;
    }
}

#[test]
fn levels_are_even_and_end_at_the_bottom() {
    assert_eq!(
        levels(0.0, -5.0, Some(2.0)),
        vec![-5.0 / 3.0, -10.0 / 3.0, -5.0]
    );
    assert_eq!(levels(0.0, -4.0, Some(2.0)), vec![-2.0, -4.0]);
    assert_eq!(levels(0.0, -1.0, Some(2.0)), vec![-1.0]);
    assert_eq!(levels(0.0, -5.0, None), vec![-5.0], "the bottom alone");
    assert!(levels(0.0, 0.0, Some(2.0)).is_empty());
    assert!(levels(0.0, 1.0, None).is_empty());
}

/// Cutting a part out: its hole first, then its outline, each all the way
/// down, climbing: clockwise round the outside, anticlockwise round the hole.
#[test]
fn a_profile_cuts_the_holes_before_the_outline() {
    let part = Region::new(vec![square(0.0, 40.0), square(15.0, 25.0)]);
    let areas = areas(&part, &[], 1.5, 0.01, &[], &floor(-3.0)).unwrap();
    assert_eq!(areas.len(), 1);
    let (path, report) = profile(&areas, ProfileSide::Outside, &params()).unwrap();
    assert_eq!(report.rings, 2);
    assert_eq!(report.levels, 2);
    assert_eq!(report.passes, 4);
    let passes = passes(&path);
    assert_eq!(passes.len(), 4);
    // The hole, at both levels, then the outline.
    for p in &passes[..2] {
        assert!(p.iter().all(|q| q.x > 10.0 && q.x < 30.0));
        assert!(signed_area(p) > 0.0, "the hole is climbed anticlockwise");
    }
    for p in &passes[2..] {
        assert!(p.iter().any(|q| q.x < 0.0));
        assert!(signed_area(p) < 0.0, "the outline is climbed clockwise");
    }
    assert_eq!(passes[0][0].z, -1.5);
    assert_eq!(passes[1][0].z, -3.0);
    // The tool's side on the part's edge, to the arc tolerance.
    let left = passes[2].iter().map(|q| q.x).fold(f64::MAX, f64::min);
    assert!((left + 1.5).abs() < 0.02, "{left}");
}

/// Conventional cutting goes the other way round.
#[test]
fn conventional_reverses_a_profile() {
    let part = Region::new(vec![square(0.0, 40.0)]);
    let areas = areas(&part, &[], 1.5, 0.01, &[], &floor(-1.0)).unwrap();
    let p = PlanarParams {
        kept_on_right: false,
        ..params()
    };
    let (path, _) = profile(&areas, ProfileSide::Outside, &p).unwrap();
    assert!(signed_area(&passes(&path)[0]) > 0.0);
    let (path, _) = profile(&areas, ProfileSide::Inside, &params()).unwrap();
    assert!(signed_area(&passes(&path)[0]) > 0.0);
}

/// An area whose rings are its region's, straight lines all.
fn area(region: Region, top: f64, bottom: f64) -> Area {
    let rings = region
        .rings()
        .iter()
        .filter_map(|c| Ring::from_points(c.points()))
        .collect();
    Area {
        region,
        rings,
        top,
        bottom,
    }
}

/// A part inside another part's hole is cut out before that hole frees it.
#[test]
fn a_part_inside_a_hole_is_cut_first() {
    let rings = [square(0.0, 100.0), square(20.0, 80.0), square(40.0, 60.0)];
    let areas: Vec<Area> = rings
        .iter()
        .map(|r| area(Region::new(vec![r.clone()]), 0.0, -1.0))
        .collect();
    let (path, _) = profile(&areas, ProfileSide::On, &params()).unwrap();
    let widths: Vec<f64> = passes(&path)
        .iter()
        .map(|p| {
            let xs = p.iter().map(|q| q.x);
            xs.clone().fold(f64::MIN, f64::max) - xs.fold(f64::MAX, f64::min)
        })
        .collect();
    assert_eq!(widths, vec![20.0, 60.0, 100.0]);
}

/// A pocket goes from its middle out, its last ring along the wall, and
/// leaves nothing in the middle the tool does not reach.
#[test]
fn a_pocket_clears_from_the_middle_out() {
    let shape = Region::new(vec![square(0.0, 21.0)]);
    let areas = areas(&shape, &[], -1.5, 0.01, &[], &floor(-2.0)).unwrap();
    let (path, report) = pocket(&areas, &params()).unwrap();
    assert_eq!(report.areas, 1);
    let passes = passes(&path);
    let widths: Vec<f64> = passes
        .iter()
        .map(|p| {
            let xs = p.iter().map(|q| q.x);
            xs.clone().fold(f64::MIN, f64::max) - xs.fold(f64::MAX, f64::min)
        })
        .collect();
    assert!(widths.windows(2).all(|w| w[0] < w[1]), "{widths:?}");
    // The wall ring, erring inward by the arc tolerance on each side.
    let last = widths.last().copied().unwrap();
    assert!(last <= 18.0 && last > 18.0 - 0.03, "{last}");
    // The innermost ring is within the tool's radius of the middle.
    let first = passes[0].iter().map(|q| q.x).fold(f64::MAX, f64::min);
    assert!(10.5 - first <= 1.5, "{first}");
    // Climbing: the wall on the right, anticlockwise.
    assert!(passes.iter().all(|p| signed_area(p) > 0.0));
    // Each ring starts where the last one came nearest: a stepover away, and
    // the arc tolerance the inner rings are shrunk by.
    for w in passes.windows(2) {
        let end = w[0].last().unwrap();
        let start = w[1][0];
        assert!(end.truncate().distance(start.truncate()) <= 2.0 + 0.02);
    }
}

/// Pockets picked at different depths are each cut to their own floor.
#[test]
fn each_area_goes_to_its_own_floor() {
    let shape = Region::new(vec![square(0.0, 10.0), square(20.0, 30.0)]);
    let floors = [dvec3(0.0, 0.0, -3.0), dvec3(25.0, 25.0, -6.0)];
    let areas = areas(&shape, &[], -1.5, 0.01, &floors, &floor(-1.0)).unwrap();
    let mut bottoms: Vec<f64> = areas.iter().map(|a| a.bottom).collect();
    bottoms.sort_by(f64::total_cmp);
    assert_eq!(bottoms, vec![-6.0, -3.0]);
}

/// Two outlines closer together than the tool is wide make one ring round
/// both, cut to the lower of their floors.
#[test]
fn outlines_the_tool_cannot_pass_between_merge() {
    let shape = Region::new(vec![square(0.0, 10.0), square(11.0, 21.0)]);
    let shape = Region::union_of(shape.rings()).unwrap();
    let floors = [dvec3(0.0, 0.0, -2.0), dvec3(21.0, 21.0, -4.0)];
    let areas = areas(&shape, &[], 1.5, 0.01, &floors, &floor(0.0)).unwrap();
    assert_eq!(areas.len(), 1);
    assert_eq!(areas[0].bottom, -4.0);
}

#[test]
fn a_floor_above_the_top_cuts_nothing() {
    let shape = Region::new(vec![square(0.0, 10.0)]);
    let areas = areas(&shape, &[], 1.5, 0.01, &[], &floor(1.0)).unwrap();
    let (path, report) = profile(&areas, ProfileSide::Outside, &params()).unwrap();
    assert!(path.is_empty());
    assert_eq!(report.too_shallow, 1);
}
