use std::collections::BTreeMap;

use camcraft_geom::synthetic::cuboid;
use camcraft_geom::topology::Body;
use glam::{DVec3, dvec3};

use super::*;
use crate::job::{ModelDef, Rotation, SheetPart, StockDef};

/// A job with a sheet `size` from `corner` holding `parts`, and a model for
/// each part.
fn job(size: [f64; 3], corner: [f64; 3], parts: &[&str]) -> Job {
    let mut j = Job::default();
    for p in parts {
        j.models.push(ModelDef {
            name: (*p).to_string(),
            path: format!("{p}.stl"),
            units: None,
            transform: Default::default(),
        });
    }
    let mut sheet = Arranged::new(size);
    sheet.corner = corner;
    sheet.parts = parts.iter().map(|p| SheetPart::of(p)).collect();
    j.stocks.push(StockDef {
        name: "sheet".into(),
        spec: StockSpec::Arranged(sheet),
    });
    j
}

fn sheet_mut(j: &mut Job) -> &mut Arranged {
    let StockSpec::Arranged(a) = &mut j.stocks[0].spec else {
        panic!("not a sheet");
    };
    a
}

/// A block from `min` to `max`, as its file would place it.
fn block(min: DVec3, max: DVec3) -> Body {
    Body::derived(cuboid(min, max))
}

fn bodies<'a>(named: &'a [(&'a str, Body)]) -> BTreeMap<&'a str, &'a Body> {
    named.iter().map(|(n, b)| (*n, b)).collect()
}

fn done(a: &Arrangement) -> (&[PartLaid], f64, f64) {
    match &a.sheets()[0].laid {
        Laid::Done {
            parts,
            used,
            length,
        } => (parts, *used, *length),
        other => panic!("not laid: {other:?}"),
    }
}

fn stand(p: &PartLaid) -> Stand {
    match p.outcome {
        Outcome::Placed(s) => s,
        other => panic!("{} not placed: {other:?}", p.model),
    }
}

/// Two blocks on a sheet off the origin: each stands inside the sheet, its
/// border kept, on the sheet's bottom face, and its stand moves its body
/// there; its outline as placed is its body's seen from above where it
/// stands. The sheet says how far along it they reach.
#[test]
fn parts_stand_inside_their_sheet_on_its_bottom() {
    let j = job([300.0, 200.0, 18.0], [50.0, -20.0, -18.0], &["a", "b"]);
    // Drawn anywhere, and taller than the sheet is thick.
    let named = [
        ("a", block(dvec3(-40.0, 5.0, 2.0), dvec3(60.0, 45.0, 12.0))),
        ("b", block(dvec3(0.0, 0.0, 0.0), dvec3(30.0, 30.0, 25.0))),
    ];
    let a = arrange(
        &j,
        &bodies(&named),
        &mut Outlines::default(),
        &Watch::default(),
    )
    .unwrap();
    let (parts, used, length) = done(&a);
    assert_eq!(parts.len(), 2);
    assert_eq!(length, 300.0);
    let (lo, hi) = (dvec3(50.0, -20.0, -18.0), dvec3(350.0, 180.0, 0.0));
    for (p, (_, body)) in parts.iter().zip(&named) {
        let s = stand(p);
        let b = body.mesh().bounds().unwrap();
        let points: Vec<DVec3> = body.mesh().vertices().iter().map(|&v| s.point(v)).collect();
        let low = points.iter().map(|v| v.z).fold(f64::INFINITY, f64::min);
        assert!(
            (low - lo.z).abs() < 1e-9,
            "{}: on the bottom, {low}",
            p.model
        );
        assert_eq!(p.heights, [lo.z, lo.z + b.extent().z]);
        for v in &points {
            assert!(
                v.x >= lo.x + 10.0 - 1e-6 && v.x <= hi.x - 10.0 + 1e-6,
                "{v}"
            );
            assert!(
                v.y >= lo.y + 10.0 - 1e-6 && v.y <= hi.y - 10.0 + 1e-6,
                "{v}"
            );
        }
        // To within the clipper's tick, a tenth of a micrometre.
        let outline = p.outline.as_ref().unwrap();
        let (olo, ohi) = outline.bounds().unwrap();
        let (plo, phi) = points
            .iter()
            .fold((points[0], points[0]), |(l, h), v| (l.min(*v), h.max(*v)));
        assert!(olo.distance(plo.truncate()) < 1e-3 && ohi.distance(phi.truncate()) < 1e-3);
        assert_eq!(a.stands(&p.model), Some(&[s][..]));
    }
    let far = parts
        .iter()
        .filter_map(|p| p.outline.as_ref()?.bounds())
        .map(|(_, h)| h.x)
        .fold(f64::MIN, f64::max);
    assert!((used - (far - 50.0)).abs() < 1e-9, "{used}");
    assert!(used < length);
}

/// Seated on top, a part's highest point is the sheet's top face; the turn
/// and the place across the sheet are the same either way.
#[test]
fn a_part_seated_on_top_reaches_the_sheets_top() {
    let mut j = job([200.0, 200.0, 18.0], [0.0; 3], &["a"]);
    let named = [("a", block(dvec3(0.0, 0.0, 5.0), dvec3(40.0, 20.0, 30.0)))];
    let bottom = arrange(
        &j,
        &bodies(&named),
        &mut Outlines::default(),
        &Watch::default(),
    )
    .unwrap();
    sheet_mut(&mut j).seat = Seat::Top;
    let top = arrange(
        &j,
        &bodies(&named),
        &mut Outlines::default(),
        &Watch::default(),
    )
    .unwrap();
    let (b, t) = (stand(&done(&bottom).0[0]), stand(&done(&top).0[0]));
    assert_eq!(b.offset.z, -5.0);
    assert_eq!(t.offset.z, 18.0 - 30.0);
    assert_eq!(done(&top).0[0].heights, [-7.0, 18.0]);
    assert_eq!((b.turn, b.offset.truncate()), (t.turn, t.offset.truncate()));
}

/// A part too large for the sheet at any turn, and one there is no room left
/// for: each is left out with its reason and stands nowhere, and the rest are
/// laid as though they were not there.
#[test]
fn parts_left_out_say_why_and_stand_nowhere() {
    let j = job([200.0, 100.0, 18.0], [0.0; 3], &["big", "a", "b"]);
    let named = [
        ("big", block(DVec3::ZERO, dvec3(300.0, 20.0, 5.0))),
        ("a", block(DVec3::ZERO, dvec3(120.0, 70.0, 5.0))),
        ("b", block(DVec3::ZERO, dvec3(120.0, 70.0, 5.0))),
    ];
    let a = arrange(
        &j,
        &bodies(&named),
        &mut Outlines::default(),
        &Watch::default(),
    )
    .unwrap();
    let (parts, ..) = done(&a);
    assert_eq!(parts[0].outcome, Outcome::TooLarge);
    let placed = parts[1..]
        .iter()
        .filter(|p| matches!(p.outcome, Outcome::Placed(_)))
        .count();
    assert_eq!(placed, 1);
    assert!(parts[1..].iter().any(|p| p.outcome == Outcome::NoRoom));
    let left: Vec<&str> = a.sheets()[0].left_out().map(|p| p.model.as_str()).collect();
    assert_eq!(left.len(), 2);
    assert_eq!(a.stands("big"), Some(&[][..]));
    for p in parts.iter().filter(|p| p.outcome == Outcome::NoRoom) {
        assert_eq!(a.stands(&p.model), Some(&[][..]));
        assert!(p.outline.is_none());
    }
}

/// A sheet some of whose parts' bodies are not given waits for them, and its
/// models stand nowhere yet; a model no sheet holds stands once, where it is.
/// Another sheet with all its bodies is laid all the same.
#[test]
fn a_sheet_waits_for_its_bodies() {
    let mut j = job([200.0, 100.0, 18.0], [0.0; 3], &["a", "b"]);
    let mut other = Arranged::new([100.0, 100.0, 10.0]);
    other.parts = vec![SheetPart::of("c")];
    j.stocks.push(StockDef {
        name: "other".into(),
        spec: StockSpec::Arranged(other),
    });
    let named = [
        ("a", block(DVec3::ZERO, dvec3(20.0, 20.0, 5.0))),
        ("c", block(DVec3::ZERO, dvec3(20.0, 20.0, 5.0))),
    ];
    let a = arrange(
        &j,
        &bodies(&named),
        &mut Outlines::default(),
        &Watch::default(),
    )
    .unwrap();
    match &a.sheets()[0].laid {
        Laid::Waiting(missing) => assert_eq!(missing, &["b"]),
        other => panic!("{other:?}"),
    }
    assert_eq!(a.stands("a"), None);
    assert_eq!(a.stands("b"), None);
    assert_eq!(a.stands("loose"), Some(WHERE_IT_IS));
    assert!(matches!(a.sheet("other").unwrap().laid, Laid::Done { .. }));
    assert_eq!(a.stands("c").map(<[Stand]>::len), Some(1));
}

/// A sheet that cannot be laid says why, and its models stand nowhere; the
/// job's other sheets are laid.
#[test]
fn a_sheet_that_cannot_be_laid_says_why() {
    let mut j = job([200.0, 100.0, 18.0], [0.0; 3], &["a"]);
    sheet_mut(&mut j).rotation = Rotation::Step(0.5);
    let named = [("a", block(DVec3::ZERO, dvec3(20.0, 20.0, 5.0)))];
    let a = arrange(
        &j,
        &bodies(&named),
        &mut Outlines::default(),
        &Watch::default(),
    )
    .unwrap();
    match &a.sheets()[0].laid {
        Laid::Failed(e) => assert!(e.to_string().contains("step between turns"), "{e}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(a.stands("a"), None);
    sheet_mut(&mut j).rotation = Rotation::None;
    sheet_mut(&mut j).border = 60.0;
    let a = arrange(
        &j,
        &bodies(&named),
        &mut Outlines::default(),
        &Watch::default(),
    )
    .unwrap();
    assert!(matches!(
        a.sheets()[0].laid,
        Laid::Failed(ArrangeError::NoRoom)
    ));
}

/// The sheet's rotation is each part's turns: with none, every part stands
/// as drawn; a long part on a sheet narrower than it is long is turned only
/// when a quarter turn is allowed, and then exactly.
#[test]
fn the_sheets_rotation_is_its_parts_turns() {
    let mut j = job([100.0, 300.0, 18.0], [0.0; 3], &["long"]);
    let named = [("long", block(DVec3::ZERO, dvec3(150.0, 20.0, 5.0)))];
    sheet_mut(&mut j).rotation = Rotation::None;
    let a = arrange(
        &j,
        &bodies(&named),
        &mut Outlines::default(),
        &Watch::default(),
    )
    .unwrap();
    assert_eq!(done(&a).0[0].outcome, Outcome::TooLarge);
    sheet_mut(&mut j).rotation = Rotation::QuarterTurns;
    let a = arrange(
        &j,
        &bodies(&named),
        &mut Outlines::default(),
        &Watch::default(),
    )
    .unwrap();
    let s = stand(&done(&a).0[0]);
    assert!(
        [90.0, 270.0].contains(&s.turn.degrees()),
        "{}",
        s.turn.degrees()
    );
    assert_eq!(s.turn.sin_cos().1, 0.0, "a quarter turn is exact");
    for step in [15.0, 90.0, 180.0, 7.0] {
        let turns = Rotation::Step(step).turns().unwrap();
        assert!(turns.iter().all(|t| t.degrees() < 360.0));
        assert_eq!(turns[0], camcraft_geom::turn::Turn::NONE);
    }
    assert_eq!(Rotation::Step(15.0).turns().unwrap().len(), 24);
    assert_eq!(Rotation::Step(7.0).turns().unwrap().len(), 52);
    assert_eq!(Rotation::HalfTurn.turns().unwrap().len(), 2);
}

/// The same job and bodies lay out the same, stand for stand; and laid
/// again with only the spacing changed, no outline is cast again.
#[test]
fn it_is_the_same_each_time_and_casts_each_outline_once() {
    let mut j = job([400.0, 200.0, 18.0], [0.0; 3], &["a", "b", "c"]);
    let named = [
        ("a", block(DVec3::ZERO, dvec3(80.0, 30.0, 5.0))),
        ("b", block(dvec3(-10.0, -10.0, 0.0), dvec3(40.0, 60.0, 9.0))),
        ("c", block(DVec3::ZERO, dvec3(80.0, 30.0, 5.0))),
    ];
    let stands =
        |a: &Arrangement| -> Vec<Outcome> { done(a).0.iter().map(|p| p.outcome).collect() };
    let mut outlines = Outlines::default();
    let first = arrange(&j, &bodies(&named), &mut outlines, &Watch::default()).unwrap();
    // Two bodies alike share one outline.
    assert_eq!(outlines.len(), 2);
    let kept: Vec<Arc<Region>> = outlines.kept.iter().map(|(_, o)| Arc::clone(o)).collect();
    let again = arrange(
        &j,
        &bodies(&named),
        &mut Outlines::default(),
        &Watch::default(),
    )
    .unwrap();
    assert_eq!(stands(&first), stands(&again));

    sheet_mut(&mut j).spacing = 25.0;
    let spaced = arrange(&j, &bodies(&named), &mut outlines, &Watch::default()).unwrap();
    assert_ne!(stands(&first), stands(&spaced), "spaced further apart");
    assert_eq!(outlines.len(), 2);
    for o in &kept {
        assert!(
            outlines.kept.iter().any(|(_, k)| Arc::ptr_eq(k, o)),
            "an outline cast again"
        );
    }
}

/// Stopped, it stops, and says so rather than give a sheet laid part way.
#[test]
fn stopping_stops_it() {
    let j = job([400.0, 200.0, 18.0], [0.0; 3], &["a"]);
    let named = [("a", block(DVec3::ZERO, dvec3(80.0, 30.0, 5.0)))];
    let control = crate::control::Control::new(1);
    control.stop();
    let r = arrange(
        &j,
        &bodies(&named),
        &mut Outlines::default(),
        &control.watch(0),
    );
    assert_eq!(r.err(), Some(Stopped));
}

/// Outlines taken in from elsewhere are kept as the latest used, and only so
/// many of the least lately used beyond them.
#[test]
fn outlines_merged_are_kept_latest() {
    let mut mine = Outlines::default();
    let mut theirs = Outlines::default();
    for k in 0..u32::try_from(KEPT).unwrap() + 10 {
        let mesh = cuboid(DVec3::ZERO, dvec3(1.0 + f64::from(k), 1.0, 1.0));
        mine.keep(Cast::of(&mesh), Arc::new(Region::default()));
    }
    let newest = cuboid(DVec3::ZERO, dvec3(0.5, 0.5, 0.5));
    theirs.keep(Cast::of(&newest), Arc::new(Region::default()));
    mine.merge(theirs);
    assert_eq!(mine.len(), KEPT);
    assert_eq!(mine.kept.last().map(|(c, _)| *c), Some(Cast::of(&newest)));
}
