use std::collections::BTreeMap;

use camcraft_geom::synthetic::cuboid;
use camcraft_geom::topology::Body;
use glam::{DVec3, dvec3};

use super::*;
use crate::job::{MOST_COPIES, ModelDef, Rotation, SheetPart, StockDef};

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

/// A part's one copy's outcome.
fn outcome(p: &PartLaid) -> Outcome {
    assert_eq!(p.copies.len(), 1, "{}", p.model);
    p.copies[0].outcome
}

fn stand(p: &PartLaid) -> Stand {
    match outcome(p) {
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
        let outline = p.copies[0].outline.as_ref().unwrap();
        let (olo, ohi) = outline.bounds().unwrap();
        let (plo, phi) = points
            .iter()
            .fold((points[0], points[0]), |(l, h), v| (l.min(*v), h.max(*v)));
        assert!(olo.distance(plo.truncate()) < 1e-3 && ohi.distance(phi.truncate()) < 1e-3);
        assert_eq!(a.stands(&p.model), Some(&[s][..]));
    }
    let far = parts
        .iter()
        .filter_map(|p| p.copies[0].outline.as_ref()?.bounds())
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
    assert_eq!(outcome(&parts[0]), Outcome::TooLarge);
    let placed = parts[1..]
        .iter()
        .filter(|p| matches!(outcome(p), Outcome::Placed(_)))
        .count();
    assert_eq!(placed, 1);
    assert!(parts[1..].iter().any(|p| outcome(p) == Outcome::NoRoom));
    let left: Vec<&str> = a.sheets()[0].left_out().map(|p| p.model.as_str()).collect();
    assert_eq!(left.len(), 2);
    assert_eq!(a.stands("big"), Some(&[][..]));
    for p in parts.iter().filter(|p| outcome(p) == Outcome::NoRoom) {
        assert_eq!(a.stands(&p.model), Some(&[][..]));
        assert!(p.copies[0].outline.is_none());
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
    assert_eq!(outcome(&done(&a).0[0]), Outcome::TooLarge);
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
    let stands = |a: &Arrangement| -> Vec<Outcome> { done(a).0.iter().map(outcome).collect() };
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

/// Model `name`'s part on the sheet of `j`.
fn part_mut<'a>(j: &'a mut Job, name: &str) -> &'a mut SheetPart {
    sheet_mut(j)
        .parts
        .iter_mut()
        .find(|p| p.model == name)
        .unwrap()
}

/// Every copy laid, each a part to the packer: each placed, at least the
/// spacing from the others, as the layout's check holds it; the arrangement
/// stands the model once for each, in copy order, and one outline is cast
/// for them all.
#[test]
fn every_copy_of_a_part_is_laid() {
    let mut j = job([400.0, 200.0, 18.0], [0.0; 3], &["a"]);
    part_mut(&mut j, "a").copies = 5;
    let named = [("a", block(DVec3::ZERO, dvec3(60.0, 40.0, 5.0)))];
    let mut outlines = Outlines::default();
    let a = arrange(&j, &bodies(&named), &mut outlines, &Watch::default()).unwrap();
    assert_eq!(outlines.len(), 1, "one outline for every copy");
    let (parts, ..) = done(&a);
    assert_eq!(parts.len(), 1);
    let p = &parts[0];
    assert_eq!((p.copies.len(), p.placed(), p.no_room()), (5, 5, 0));
    let stands: Vec<Stand> = p.stands().collect();
    assert_eq!(a.stands("a"), Some(stands.as_slice()));
    assert_eq!(a.sheets()[0].placed(), (5, 5));
    assert!(a.sheets()[0].left_out().next().is_none());
    // Apart by the spacing, every two of them.
    let outlines: Vec<&Region> = p
        .copies
        .iter()
        .map(|c| c.outline.as_ref().unwrap())
        .collect();
    for (i, x) in outlines.iter().enumerate() {
        for y in &outlines[i + 1..] {
            let grown = x.offset(10.0 - 1e-3, 1e-3);
            assert!(grown.intersection(y).unwrap().is_empty(), "too near");
        }
    }
}

/// A part some of whose copies have no room keeps those that fit: the last
/// are left out, each said to be, and the model stands once for each placed.
/// The sheet places as many of a part with copies as of as many parts.
#[test]
fn copies_with_no_room_are_left_out_and_the_rest_placed() {
    let mut j = job([200.0, 100.0, 18.0], [0.0; 3], &["a"]);
    part_mut(&mut j, "a").copies = 4;
    let named = [("a", block(DVec3::ZERO, dvec3(80.0, 60.0, 5.0)))];
    let a = arrange(
        &j,
        &bodies(&named),
        &mut Outlines::default(),
        &Watch::default(),
    )
    .unwrap();
    let p = &done(&a).0[0];
    let placed = p.placed();
    assert_eq!(placed, 2, "{:?}", p.copies);
    assert_eq!(p.no_room(), 2);
    assert_eq!(p.too_large(), 0);
    for (k, c) in p.copies.iter().enumerate() {
        assert_eq!(matches!(c.outcome, Outcome::Placed(_)), k < placed, "{k}");
        assert_eq!(c.outline.is_some(), k < placed, "{k}");
    }
    assert_eq!(a.stands("a").map(<[Stand]>::len), Some(placed));
    assert_eq!(a.sheets()[0].placed(), (2, 4));
    let left: Vec<&str> = a.sheets()[0].left_out().map(|p| p.model.as_str()).collect();
    assert_eq!(left, ["a"]);

    // As many parts of one copy each lay the same.
    let each = job([200.0, 100.0, 18.0], [0.0; 3], &["a", "b", "c", "d"]);
    let named4 = [
        ("a", block(DVec3::ZERO, dvec3(80.0, 60.0, 5.0))),
        ("b", block(DVec3::ZERO, dvec3(80.0, 60.0, 5.0))),
        ("c", block(DVec3::ZERO, dvec3(80.0, 60.0, 5.0))),
        ("d", block(DVec3::ZERO, dvec3(80.0, 60.0, 5.0))),
    ];
    let four = arrange(
        &each,
        &bodies(&named4),
        &mut Outlines::default(),
        &Watch::default(),
    )
    .unwrap();
    let stood: Vec<Option<Stand>> = done(&four).0.iter().map(|p| p.stands().next()).collect();
    let copies: Vec<Option<Stand>> = p
        .copies
        .iter()
        .map(|c| match c.outcome {
            Outcome::Placed(s) => Some(s),
            _ => None,
        })
        .collect();
    assert_eq!(stood, copies);
}

/// A part's grain is used instead of the sheet's: on a sheet too narrow for
/// a long part as it stands, the copies of one free to turn are turned, and
/// those of one kept as it stands are too large, though the sheet allows
/// quarter turns. A half turn allows only none or half round.
#[test]
fn a_parts_grain_limits_its_copies_turns() {
    let mut j = job([100.0, 400.0, 18.0], [0.0; 3], &["free", "grain"]);
    part_mut(&mut j, "free").copies = 2;
    part_mut(&mut j, "grain").copies = 2;
    part_mut(&mut j, "grain").rotation = Some(Rotation::None);
    let named = [
        ("free", block(DVec3::ZERO, dvec3(150.0, 20.0, 5.0))),
        ("grain", block(DVec3::ZERO, dvec3(150.0, 20.0, 5.0))),
    ];
    let a = arrange(
        &j,
        &bodies(&named),
        &mut Outlines::default(),
        &Watch::default(),
    )
    .unwrap();
    let (parts, ..) = done(&a);
    assert_eq!(parts[0].placed(), 2);
    for s in parts[0].stands() {
        assert!([90.0, 270.0].contains(&s.turn.degrees()), "{s:?}");
    }
    assert_eq!(parts[1].too_large(), 2);
    assert_eq!(a.stands("grain"), Some(&[][..]));

    // Half round at most: on a sheet long enough, turned by none or a half.
    let mut j = job([400.0, 400.0, 18.0], [0.0; 3], &["grain"]);
    part_mut(&mut j, "grain").copies = 6;
    part_mut(&mut j, "grain").rotation = Some(Rotation::HalfTurn);
    let named = [("grain", block(DVec3::ZERO, dvec3(150.0, 20.0, 5.0)))];
    let a = arrange(
        &j,
        &bodies(&named),
        &mut Outlines::default(),
        &Watch::default(),
    )
    .unwrap();
    let p = &done(&a).0[0];
    assert_eq!(p.placed(), 6);
    assert!(p.stands().all(|s| [0.0, 180.0].contains(&s.turn.degrees())));

    // A grain by a step that is not one says whose it is.
    part_mut(&mut j, "grain").rotation = Some(Rotation::Step(0.5));
    let a = arrange(
        &j,
        &bodies(&named),
        &mut Outlines::default(),
        &Watch::default(),
    )
    .unwrap();
    match &a.sheets()[0].laid {
        Laid::Failed(e) => assert_eq!(*e, ArrangeError::Grain("grain".into())),
        other => panic!("{other:?}"),
    }
    // So does a part asking for more copies than a sheet lays.
    part_mut(&mut j, "grain").rotation = None;
    part_mut(&mut j, "grain").copies = MOST_COPIES + 1;
    let a = arrange(
        &j,
        &bodies(&named),
        &mut Outlines::default(),
        &Watch::default(),
    )
    .unwrap();
    assert!(matches!(
        &a.sheets()[0].laid,
        Laid::Failed(ArrangeError::Copies { copies, .. }) if *copies == MOST_COPIES + 1
    ));
}

/// Copies of a part with a hole take another part inside each hole: two
/// plates fill the sheet, and the small blocks go in their holes.
#[test]
fn copies_of_a_part_with_a_hole_take_others_inside_it() {
    let mut j = job([180.0, 95.0, 18.0], [0.0; 3], &["plate", "peg"]);
    {
        let a = sheet_mut(&mut j);
        a.spacing = 5.0;
        a.border = 5.0;
    }
    part_mut(&mut j, "plate").copies = 2;
    part_mut(&mut j, "peg").copies = 2;
    let named = [
        (
            "plate",
            Body::derived(camcraft_geom::synthetic::plate_with_hole(
                40.0, 5.0, 25.0, 64,
            )),
        ),
        ("peg", block(DVec3::ZERO, dvec3(10.0, 10.0, 5.0))),
    ];
    let a = arrange(
        &j,
        &bodies(&named),
        &mut Outlines::default(),
        &Watch::default(),
    )
    .unwrap();
    let (parts, ..) = done(&a);
    assert_eq!(parts[0].placed(), 2, "{:?}", parts[0].copies);
    assert_eq!(parts[1].placed(), 2, "{:?}", parts[1].copies);
    // Each peg in a hole: its middle within the hole's radius of a plate's.
    let plates: Vec<DVec2> = parts[0]
        .stands()
        .map(|s| s.point(DVec3::ZERO).truncate())
        .collect();
    for s in parts[1].stands() {
        let middle = s.point(dvec3(5.0, 5.0, 0.0)).truncate();
        assert!(
            plates.iter().any(|p| p.distance(middle) < 25.0 - 7.0),
            "a peg at {middle} in no hole: {plates:?}"
        );
    }
}
