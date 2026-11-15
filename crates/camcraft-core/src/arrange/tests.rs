use std::collections::HashSet;
use std::time::{Duration, Instant};

use camcraft_geom::contour::{Contour, Region};
use camcraft_geom::turn::Turn;
use glam::{DVec2, dvec2, dvec3};

use super::grid::{GAP_PER_CELL, Grid};
use super::pack::{ORDERS, Order, pack_in};
use super::*;
use crate::control::{Control, Watch};

fn polygon(points: &[DVec2]) -> Region {
    Region::new(vec![Contour::new(points.to_vec()).unwrap()])
}

/// A `w` by `h` rectangle with its front-left corner at the origin.
fn rect(w: f64, h: f64) -> Region {
    Region::rectangle(DVec2::ZERO, dvec2(w, h))
}

/// A regular polygon of `n` sides round the origin, its corners `r` from it.
fn regular(r: f64, n: u32) -> Region {
    let at = |k: u32| {
        let a = std::f64::consts::TAU * f64::from(k) / f64::from(n);
        dvec2(r * a.cos(), r * a.sin())
    };
    polygon(&(0..n).map(at).collect::<Vec<_>>())
}

fn disc(r: f64) -> Region {
    regular(r, 96)
}

fn ring(outer: f64, inner: f64) -> Region {
    disc(outer).difference(&disc(inner)).unwrap()
}

/// An L, `w` along X and `h` along Y, its arms `arm` wide.
fn ell(w: f64, h: f64, arm: f64) -> Region {
    rect(w, arm).union(&rect(arm, h)).unwrap()
}

/// A T, its bar `w` long across the top, its stem `h` down from the top.
fn tee(w: f64, h: f64, arm: f64) -> Region {
    let stem = Region::rectangle(dvec2((w - arm) / 2.0, 0.0), dvec2((w + arm) / 2.0, h));
    let bar = Region::rectangle(dvec2(0.0, h - arm), dvec2(w, h));
    stem.union(&bar).unwrap()
}

/// A star of `points` points, `outer` and `inner` from its middle.
fn star(outer: f64, inner: f64, points: u32) -> Region {
    let n = 2 * points;
    let at = |k: u32| {
        let a = std::f64::consts::TAU * f64::from(k) / f64::from(n);
        let r = if k.is_multiple_of(2) { outer } else { inner };
        dvec2(r * a.cos(), r * a.sin())
    };
    polygon(&(0..n).map(at).collect::<Vec<_>>())
}

fn turns(degrees: &[f64]) -> Vec<Turn> {
    degrees
        .iter()
        .map(|&d| Turn::from_degrees(d).unwrap())
        .collect()
}

const QUARTERS: [f64; 4] = [0.0, 90.0, 180.0, 270.0];

/// Every turn `step` degrees apart, from none.
fn stepped(step: f64) -> Vec<Turn> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = (360.0 / step).round() as u32;
    (0..n)
        .map(|k| Turn::from_degrees(f64::from(k) * step).unwrap())
        .collect()
}

fn part(shape: usize, degrees: &[f64]) -> Part {
    Part {
        shape,
        turns: turns(degrees),
    }
}

/// A sheet `w` by `h` from the origin, 10 apart and 5 in, packed from the
/// front left along X.
fn sheet(w: f64, h: f64) -> Sheet {
    Sheet {
        corner: DVec2::ZERO,
        size: dvec2(w, h),
        spacing: 10.0,
        border: 5.0,
        from: Corner::FrontLeft,
        along: Direction::X,
    }
}

/// The parts packed, and the layout checked again here.
fn packed(sheet: &Sheet, shapes: &[Region], parts: &[Part]) -> Vec<Outcome> {
    let outcomes = pack(sheet, shapes, parts, &Watch::default()).unwrap();
    assert_eq!(outcomes.len(), parts.len());
    verify(sheet, shapes, parts, &outcomes).unwrap();
    outcomes
}

/// Part `k`'s outline where it stands, if it was placed.
fn outline(shapes: &[Region], parts: &[Part], outcomes: &[Outcome], k: usize) -> Option<Region> {
    match outcomes[k] {
        Outcome::Placed(stand) => Some(stand.region(&shapes[parts[k].shape])),
        _ => None,
    }
}

/// How far along X the parts placed reach from the sheet's left edge.
fn used_along_x(sheet: &Sheet, shapes: &[Region], parts: &[Part], outcomes: &[Outcome]) -> f64 {
    (0..parts.len())
        .filter_map(|k| outline(shapes, parts, outcomes, k))
        .map(|r| r.bounds().unwrap().1.x - sheet.corner.x)
        .fold(0.0, f64::max)
}

fn stand(degrees: f64, x: f64, y: f64) -> Stand {
    Stand {
        turn: Turn::from_degrees(degrees).unwrap(),
        offset: dvec3(x, y, 0.0),
    }
}

/// A stand turns a point about Z, then moves it, up or down too; a direction
/// is only turned. A quarter turn is exact, and no stand leaves a point as it
/// was, bits and all.
#[test]
fn a_stand_turns_then_moves() {
    let s = stand(90.0, 10.0, -0.5);
    assert_eq!(s.point(dvec3(1.0, 2.0, 3.0)), dvec3(8.0, 0.5, 3.0));
    assert_eq!(s.direction(dvec3(1.0, 2.0, 3.0)), dvec3(-2.0, 1.0, 3.0));
    let seated = Stand {
        offset: dvec3(10.0, -0.5, -18.0),
        ..s
    };
    assert_eq!(seated.point(dvec3(1.0, 2.0, 3.0)), dvec3(8.0, 0.5, -15.0));
    assert_eq!(
        seated.direction(dvec3(1.0, 2.0, 3.0)),
        dvec3(-2.0, 1.0, 3.0)
    );
    let only_raised = Stand {
        offset: dvec3(0.0, 0.0, 2.0),
        ..Stand::NONE
    };
    assert!(!only_raised.is_none());
    assert_eq!(
        only_raised.point(dvec3(1.0, 2.0, 3.0)),
        dvec3(1.0, 2.0, 5.0)
    );
    // Seen from above, a move up or down is no move.
    assert_eq!(seated.region(&rect(2.0, 1.0)), s.region(&rect(2.0, 1.0)));
    let odd = dvec3(-0.0, f64::INFINITY, -0.0);
    let back = Stand::NONE.point(odd);
    assert_eq!(back.x.to_bits(), odd.x.to_bits());
    assert_eq!(back.y, f64::INFINITY);
    assert_eq!(back.z.to_bits(), odd.z.to_bits());
    let only_moved = stand(0.0, 1.0, 0.0).direction(odd);
    assert_eq!(only_moved.x.to_bits(), odd.x.to_bits());
    assert!(Stand::default().is_none() && !s.is_none());

    // The outline turned and moved as each of its points is.
    let square = rect(2.0, 1.0);
    let moved = s.region(&square);
    for (a, b) in square.rings()[0]
        .points()
        .iter()
        .zip(moved.rings()[0].points())
    {
        assert_eq!(s.point(a.extend(0.0)).truncate(), *b);
    }
}

/// Undoing a stand brings a point back: exactly at a quarter turn, to within
/// rounding at any other, and no stand leaves it as it was, bits and all.
#[test]
fn a_stand_undone_brings_a_point_back() {
    let p = dvec3(1.0, 2.0, 3.0);
    let quarter = Stand {
        offset: dvec3(10.0, -0.5, -18.0),
        ..stand(90.0, 0.0, 0.0)
    };
    assert_eq!(quarter.undo_point(quarter.point(p)), p);
    let free = stand(23.0, 4.0, 5.0);
    assert!(free.undo_point(free.point(p)).distance(p) < 1e-12);
    let odd = dvec3(-0.0, f64::INFINITY, -0.0);
    let back = Stand::NONE.undo_point(odd);
    assert_eq!(back.x.to_bits(), odd.x.to_bits());
    assert_eq!(back.y, f64::INFINITY);
}

/// Stands are equal and hash alike by their turns and their offsets' bits,
/// up and down as well as across.
#[test]
fn stands_are_keys() {
    let raised = |z: f64| Stand {
        offset: dvec3(1.0, 2.0, z),
        ..stand(90.0, 0.0, 0.0)
    };
    let set: HashSet<Stand> = [
        stand(90.0, 1.0, 2.0),
        stand(450.0, 1.0, 2.0),
        stand(90.0, 1.0, 2.000_000_000_001),
        raised(0.0),
        raised(-18.0),
        raised(-18.0),
        Stand::NONE,
    ]
    .into_iter()
    .collect();
    assert_eq!(set.len(), 4);
    assert_ne!(raised(-18.0), raised(-18.000_000_000_001));
}

/// The check fails two parts nearer each other than the spacing, by a
/// hundredth, and passes them a thousandth further than it; the same for
/// the border; and a part at a turn it may not take, or outcomes that are
/// not one a part.
#[test]
fn the_check_fails_a_layout_that_breaks_the_rules() {
    let sheet = sheet(200.0, 100.0);
    let shapes = [rect(20.0, 20.0)];
    let parts = [part(0, &[0.0]), part(0, &[0.0])];
    let at = |a: Stand, b: Stand| {
        verify(
            &sheet,
            &shapes,
            &parts,
            &[Outcome::Placed(a), Outcome::Placed(b)],
        )
    };
    let a = stand(0.0, 10.0, 10.0);
    assert_eq!(at(a, stand(0.0, 40.001, 10.0)), Ok(()));
    assert_eq!(
        at(a, stand(0.0, 39.99, 10.0)),
        Err(ArrangeError::Unverified(Violation::Spacing { a: 0, b: 1 }))
    );
    assert_eq!(
        at(a, stand(0.0, 15.0, 15.0)),
        Err(ArrangeError::Unverified(Violation::Spacing { a: 0, b: 1 })),
        "overlapping"
    );
    // Diagonally, the corners are nearer than the boxes' sides say.
    assert_eq!(at(a, stand(0.0, 37.08, 37.08)), Ok(()));
    assert_eq!(
        at(a, stand(0.0, 37.0, 37.0)),
        Err(ArrangeError::Unverified(Violation::Spacing { a: 0, b: 1 }))
    );
    let far = stand(0.0, 100.0, 10.0);
    assert_eq!(at(stand(0.0, 5.001, 10.0), far), Ok(()));
    assert_eq!(
        at(stand(0.0, 4.99, 10.0), far),
        Err(ArrangeError::Unverified(Violation::Border { part: 0 }))
    );
    assert_eq!(
        at(far, stand(0.0, 100.0, 75.01)),
        Err(ArrangeError::Unverified(Violation::Border { part: 1 }))
    );
    assert_eq!(
        at(a, stand(90.0, 100.0, 10.0)),
        Err(ArrangeError::Unverified(Violation::Turn { part: 1 }))
    );
    assert_eq!(
        verify(&sheet, &shapes, &parts, &[Outcome::NoRoom]),
        Err(ArrangeError::Unverified(Violation::Outcomes {
            parts: 2,
            outcomes: 1
        }))
    );
    // A part not placed is not checked.
    assert_eq!(
        verify(
            &sheet,
            &shapes,
            &parts,
            &[Outcome::Placed(a), Outcome::NoRoom]
        ),
        Ok(())
    );
}

/// Parts packed hard against the border from every corner and against one
/// another in rows, at spacings, borders and cells from small to large, and
/// on the largest sheet with no spacing at all, where the most cells set the
/// cell: every layout passes the exact check, which `pack` runs, so the
/// margin the grid keeps for it holds wherever a layout is as tight as the
/// grid allows. No stand moves its part up or down.
#[test]
fn layouts_as_tight_as_the_grid_allows_pass_the_check() {
    let shapes = [rect(37.0, 23.0), regular(15.0, 7)];
    let parts: Vec<Part> = (0..16).map(|k| part(k % 2, &[0.0, 30.0, 90.0])).collect();
    let mut sheets = Vec::new();
    for spacing in [0.0, 0.5, 3.0, 6.35, 25.0] {
        for border in [0.0, 2.0, 10.0] {
            for from in [
                Corner::FrontLeft,
                Corner::FrontRight,
                Corner::BackLeft,
                Corner::BackRight,
            ] {
                sheets.push(Sheet {
                    spacing,
                    border,
                    from,
                    ..sheet(260.0, 150.0)
                });
            }
        }
    }
    for s in &sheets {
        let outcomes = packed(s, &shapes, &parts);
        let stands: Vec<Stand> = outcomes
            .iter()
            .filter_map(|o| match o {
                Outcome::Placed(stand) => Some(*stand),
                _ => None,
            })
            .collect();
        assert!(stands.len() >= 4, "{s:?}: {outcomes:?}");
        assert!(stands.iter().all(|st| st.offset.z == 0.0));
    }
    let largest = Sheet {
        spacing: 0.0,
        border: 0.0,
        ..sheet(2440.0, 1220.0)
    };
    let many: Vec<Part> = (0..40).map(|k| part(k % 2, &QUARTERS)).collect();
    let outcomes = packed(&largest, &shapes, &many);
    assert!(outcomes.iter().all(|o| matches!(o, Outcome::Placed(_))));
}

/// What cannot be a sheet or a part is refused, not packed.
#[test]
fn a_sheet_or_part_there_cannot_be_is_refused() {
    let shapes = [rect(20.0, 20.0)];
    let parts = [part(0, &[0.0])];
    let refused = |s: &Sheet, p: &[Part]| pack(s, &shapes, p, &Watch::default()).unwrap_err();
    let mut s = sheet(100.0, 100.0);
    s.spacing = -1.0;
    assert!(matches!(
        refused(&s, &parts),
        ArrangeError::Sheet {
            what: "spacing",
            ..
        }
    ));
    let mut s = sheet(100.0, 100.0);
    s.border = 50.0;
    assert_eq!(refused(&s, &parts), ArrangeError::NoRoom);
    let s = sheet(100.0, 100.0);
    assert_eq!(
        refused(&s, &[part(1, &[0.0])]),
        ArrangeError::NoSuchShape {
            part: 0,
            shape: 1,
            shapes: 1
        }
    );
    assert_eq!(
        refused(&s, &[part(0, &[])]),
        ArrangeError::NoTurns { part: 0 }
    );
    assert_eq!(
        pack(&s, &[Region::default()], &parts, &Watch::default()).unwrap_err(),
        ArrangeError::EmptyShape { shape: 0 }
    );
}

/// Mixed outlines at quarter turns, each laid out in every order: the
/// layout keeps the spacing and the border, and says how far it has got.
#[test]
fn every_layout_keeps_the_spacing_and_the_border() {
    let shapes = [
        rect(120.0, 60.0),
        ell(150.0, 100.0, 30.0),
        disc(40.0),
        star(70.0, 30.0, 5),
        ring(60.0, 35.0),
        tee(110.0, 90.0, 25.0),
    ];
    let parts: Vec<Part> = (0..18).map(|k| part(k % shapes.len(), &QUARTERS)).collect();
    let control = Control::new(1);
    let sheet = sheet(700.0, 400.0);
    let outcomes = pack(&sheet, &shapes, &parts, &control.watch(0)).unwrap();
    verify(&sheet, &shapes, &parts, &outcomes).unwrap();
    assert!(
        outcomes.iter().all(|o| matches!(o, Outcome::Placed(_))),
        "{outcomes:?}"
    );
    assert_eq!(control.done(0), 1.0);
    for &order in &ORDERS {
        let one = pack_in(&sheet, &shapes, &parts, &[order], &Watch::default()).unwrap();
        verify(&sheet, &shapes, &parts, &one).unwrap();
    }
}

/// A part stands only at the turns it may take: one too long for the sheet
/// as it is fits turned a quarter, and is too large where it may not turn;
/// parts that may only turn half round stand either way, never across.
#[test]
fn turn_limits_are_honoured() {
    let shapes = [rect(150.0, 400.0), rect(60.0, 30.0)];
    let s = sheet(500.0, 200.0);
    let outcomes = packed(&s, &shapes, &[part(0, &[0.0]), part(0, &[0.0, 90.0])]);
    assert_eq!(outcomes[0], Outcome::TooLarge);
    let Outcome::Placed(turned) = outcomes[1] else {
        panic!("{outcomes:?}");
    };
    assert_eq!(turned.turn.degrees(), 90.0);

    let half = [0.0, 180.0];
    let parts: Vec<Part> = (0..10).map(|_| part(1, &half)).collect();
    for o in packed(&s, &shapes, &parts) {
        let Outcome::Placed(st) = o else {
            panic!("{o:?}");
        };
        assert!(half.contains(&st.turn.degrees()), "{st:?}");
    }
}

/// A disc too large for what the ring leaves of the sheet stands in the
/// ring's hole.
#[test]
fn a_disc_stands_in_a_rings_hole() {
    let shapes = [ring(200.0, 140.0), disc(60.0)];
    let parts = [part(1, &[0.0]), part(0, &[0.0])];
    let s = sheet(425.0, 425.0);
    let outcomes = packed(&s, &shapes, &parts);
    let (Outcome::Placed(disc), Outcome::Placed(ring)) = (outcomes[0], outcomes[1]) else {
        panic!("{outcomes:?}");
    };
    let apart = disc.offset.distance(ring.offset);
    assert!(
        apart <= 140.0 - 60.0 - 10.0,
        "{apart} from the ring's middle"
    );
}

/// Two Ls, one turned half round, interlock: they use less of the sheet
/// than their boxes side by side would.
#[test]
fn two_ls_use_less_of_the_sheet_than_their_boxes() {
    let shapes = [ell(300.0, 300.0, 100.0)];
    let parts = [part(0, &[0.0, 180.0]), part(0, &[0.0, 180.0])];
    let s = sheet(1000.0, 450.0);
    let outcomes = packed(&s, &shapes, &parts);
    let used = used_along_x(&s, &shapes, &parts, &outcomes);
    assert!(used < 2.0 * 300.0 + s.spacing, "{used}");
}

/// From each corner and along each direction: the first square stands in
/// that corner, inside the border by no more than the grid's slack, and
/// the next ones stand across the sheet from it, the spacing apart and a
/// little more, before any goes further along.
#[test]
fn each_corner_and_direction_is_packed_from() {
    let shapes = [rect(100.0, 100.0)];
    let parts = [part(0, &[0.0]), part(0, &[0.0]), part(0, &[0.0])];
    for from in [
        Corner::FrontLeft,
        Corner::FrontRight,
        Corner::BackLeft,
        Corner::BackRight,
    ] {
        for along in [Direction::X, Direction::Y] {
            let s = Sheet {
                from,
                along,
                ..sheet(1000.0, 600.0)
            };
            let slack = GAP_PER_CELL * Grid::new(&s).cell();
            let outcomes = packed(&s, &shapes, &parts);
            // Each square's distance from the edges the corner is on, along
            // and across the direction.
            let from_corner: Vec<(f64, f64)> = (0..3)
                .map(|k| {
                    let (lo, hi) = outline(&shapes, &parts, &outcomes, k)
                        .unwrap()
                        .bounds()
                        .unwrap();
                    let x = if from.left() { lo.x } else { s.size.x - hi.x };
                    let y = if from.front() { lo.y } else { s.size.y - hi.y };
                    match along {
                        Direction::X => (x, y),
                        Direction::Y => (y, x),
                    }
                })
                .collect();
            let (along0, across0) = from_corner[0];
            for v in [along0, across0] {
                assert!(
                    v >= s.border && v <= s.border + slack,
                    "{from:?} {along:?}: {from_corner:?}"
                );
            }
            for k in 1..3 {
                let (a, b) = from_corner[k];
                let (_, before) = from_corner[k - 1];
                assert_eq!(a, along0, "{from:?} {along:?}: {from_corner:?}");
                let gap = b - before - 100.0;
                assert!(
                    gap >= s.spacing && gap <= s.spacing + slack,
                    "{from:?} {along:?}: {from_corner:?}"
                );
            }
        }
    }
}

/// A part too large for the sheet and one there is no room left for are
/// told apart, and the rest are placed.
#[test]
fn too_large_and_no_room_are_told_while_the_rest_are_placed() {
    let shapes = [rect(200.0, 200.0), rect(600.0, 50.0)];
    let mut parts: Vec<Part> = (0..5).map(|_| part(0, &QUARTERS)).collect();
    parts.insert(2, part(1, &QUARTERS));
    let outcomes = packed(&sheet(500.0, 500.0), &shapes, &parts);
    assert_eq!(outcomes[2], Outcome::TooLarge);
    let placed = outcomes
        .iter()
        .filter(|o| matches!(o, Outcome::Placed(_)))
        .count();
    let no_room = outcomes.iter().filter(|o| **o == Outcome::NoRoom).count();
    assert_eq!((placed, no_room), (4, 1), "{outcomes:?}");
}

/// Many parts at many turns: what is to arrange them for a long while.
fn a_long_job() -> (Sheet, Vec<Region>, Vec<Part>) {
    let shapes = vec![
        rect(300.0, 200.0),
        ell(350.0, 300.0, 90.0),
        star(160.0, 70.0, 5),
        ring(150.0, 90.0),
    ];
    let parts = (0..200)
        .map(|k| Part {
            shape: k % shapes.len(),
            turns: stepped(5.0),
        })
        .collect();
    (sheet(2440.0, 1220.0), shapes, parts)
}

/// Stopped before it starts, it does nothing; stopped while it lays the
/// parts, it ends soon after, stopped.
#[test]
fn stopping_returns_promptly() {
    let (sheet, shapes, parts) = a_long_job();
    let control = Control::new(1);
    control.stop();
    assert_eq!(
        pack(&sheet, &shapes, &parts, &control.watch(0)),
        Err(ArrangeError::Stopped(crate::control::Stopped))
    );

    let control = Control::new(1);
    let watching = control.clone();
    let stopper = std::thread::spawn(move || {
        // Once the outlines are on the grid and the laying has begun.
        while watching.done(0) <= 0.1 {
            std::thread::sleep(Duration::from_millis(1));
        }
        watching.stop();
        Instant::now()
    });
    let result = pack(&sheet, &shapes, &parts, &control.watch(0));
    let returned = Instant::now();
    let stopped = stopper.join().unwrap();
    assert_eq!(result, Err(ArrangeError::Stopped(crate::control::Stopped)));
    let after = returned.saturating_duration_since(stopped);
    assert!(
        after < Duration::from_secs(1),
        "{after:?} after it was stopped"
    );
}

/// Mixed outlines at a 15° step, packed on one thread and on eight, and
/// again: the same layout each time.
#[test]
fn is_deterministic() {
    let shapes = [
        rect(150.0, 90.0),
        ell(160.0, 120.0, 40.0),
        star(80.0, 35.0, 5),
        ring(70.0, 40.0),
        regular(60.0, 3),
        tee(120.0, 100.0, 30.0),
    ];
    let parts: Vec<Part> = (0..16)
        .map(|k| Part {
            shape: k % shapes.len(),
            turns: stepped(15.0),
        })
        .collect();
    let s = sheet(900.0, 500.0);
    let on = |threads: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| pack(&s, &shapes, &parts, &Watch::default()).unwrap())
    };
    let one = on(1);
    assert!(
        one.iter()
            .filter(|o| matches!(o, Outcome::Placed(_)))
            .count()
            > 10
    );
    assert_eq!(one, on(8));
    assert_eq!(one, on(8));
}

/// How long arranging takes: fifty mixed parts on a 2440 by 1220 sheet at a
/// 15° step, and ten at quarter turns, each in one order and in all three,
/// on four threads.
/// `cargo test --release -p camcraft-core --lib arrange::tests::benchmark -- --ignored --nocapture`.
#[test]
#[ignore = "times arranging: seconds, in a release build"]
fn benchmark() {
    let shapes = vec![
        rect(300.0, 200.0),
        rect(500.0, 120.0),
        rect(200.0, 200.0),
        ell(350.0, 300.0, 90.0),
        disc(110.0),
        ring(150.0, 90.0),
        polygon(&[dvec2(0.0, 0.0), dvec2(320.0, 0.0), dvec2(100.0, 240.0)]),
        star(160.0, 70.0, 5),
        tee(300.0, 250.0, 80.0),
        regular(130.0, 6),
    ];
    let sheet = Sheet {
        border: 10.0,
        ..sheet(2440.0, 1220.0)
    };
    let grid = Grid::new(&sheet);
    println!(
        "cell {:.3} mm, {} by {} cells; parts at most {:.2} mm further apart than the spacing of {}",
        grid.cell(),
        grid.long(),
        grid.wide(),
        GAP_PER_CELL * grid.cell(),
        sheet.spacing
    );
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    let mixed: Vec<Part> = (0..50)
        .map(|k| Part {
            shape: k % shapes.len(),
            turns: stepped(15.0),
        })
        .collect();
    let ten: Vec<Part> = (0..10).map(|k| part(k, &QUARTERS)).collect();
    for (name, parts) in [("50 at 15°", &mixed), ("10 at quarter turns", &ten)] {
        for orders in [&[Order::Area][..], &ORDERS[..]] {
            let start = Instant::now();
            let outcomes = pool
                .install(|| pack_in(&sheet, &shapes, parts, orders, &Watch::default()))
                .unwrap();
            let took = start.elapsed();
            let placed = outcomes
                .iter()
                .filter(|o| matches!(o, Outcome::Placed(_)))
                .count();
            println!(
                "{name}, {} order(s): {:.3} s, {placed} placed, {:.0} mm used",
                orders.len(),
                took.as_secs_f64(),
                used_along_x(&sheet, &shapes, parts, &outcomes)
            );
        }
    }
}
