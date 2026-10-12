use super::*;
use crate::heightmap::HeightMap;

/// A span's inside is `contains`' inside: scanning a square with a square hole
/// in it, at many angles, every point strictly inside a span is inside the
/// region, and every point strictly between spans is not.
#[test]
fn spans_agree_with_contains() {
    let outer = Contour::new(vec![
        dvec2(0.0, 0.0),
        dvec2(10.0, 0.0),
        dvec2(10.0, 10.0),
        dvec2(0.0, 10.0),
    ])
    .unwrap();
    let hole = Contour::new(vec![
        dvec2(3.0, 3.0),
        dvec2(3.0, 7.0),
        dvec2(7.0, 7.0),
        dvec2(7.0, 3.0),
    ])
    .unwrap();
    let region = Region::new(vec![outer, hole]);
    for deg in [0.0_f64, 17.0, 45.0, 90.0, 133.0] {
        let along = dvec2(deg.to_radians().cos(), deg.to_radians().sin());
        let across = along.perp();
        for k in -20..=160 {
            let v = f64::from(k) * 0.1;
            let spans = region.spans(along, v);
            for (u0, u1) in &spans {
                assert!(u1 > u0);
                let mid = along * ((u0 + u1) / 2.0) + across * v;
                assert!(region.contains(mid), "{deg} {v}: {mid} should be inside");
            }
            for w in spans.windows(2) {
                if let [(_, a), (b, _)] = w {
                    let mid = along * ((a + b) / 2.0) + across * v;
                    assert!(!region.contains(mid), "{deg} {v}: {mid} should be outside");
                }
            }
        }
    }
    // Through the hole along X: two spans; below the square: none.
    let spans = region.spans(dvec2(1.0, 0.0), 5.0);
    assert_eq!(spans, vec![(0.0, 3.0), (7.0, 10.0)]);
    assert!(region.spans(dvec2(1.0, 0.0), -1.0).is_empty());
    assert_eq!(region.bounds(), Some((dvec2(0.0, 0.0), dvec2(10.0, 10.0))));
    assert_eq!(Region::default().bounds(), None);
}

fn square(half: f64) -> Region {
    Region::rectangle(dvec2(-half, -half), dvec2(half, half))
}

#[test]
fn a_ring_needs_three_points() {
    assert!(Contour::new(vec![dvec2(0.0, 0.0), dvec2(1.0, 0.0)]).is_none());
    assert!(Contour::new(vec![dvec2(0.0, 0.0), dvec2(1.0, 0.0), dvec2(0.0, 1.0)]).is_some());
    assert!(Contour::new(vec![dvec2(f64::NAN, 0.0), dvec2(1.0, 0.0), dvec2(0.0, 1.0)]).is_none());
}

#[test]
fn area_and_edges_are_right() {
    let r = square(5.0);
    assert!((r.area() - 100.0).abs() < 1e-9);
    let ring = &r.rings()[0];
    let edges: Vec<(DVec2, DVec2)> = ring.edges().collect();
    assert_eq!(edges.len(), 4);
    assert_eq!(
        edges[3],
        (ring.points()[3], ring.points()[0]),
        "closing last"
    );
    let perimeter: f64 = edges.iter().map(|(a, b)| a.distance(*b)).sum();
    assert!((perimeter - 40.0).abs() < 1e-9);
}

/// The sign is how an outer boundary is told from a hole.
#[test]
fn winding_shows_in_the_sign() {
    let ccw = Contour::new(vec![
        dvec2(0.0, 0.0),
        dvec2(2.0, 0.0),
        dvec2(2.0, 2.0),
        dvec2(0.0, 2.0),
    ])
    .unwrap();
    let mut cw_pts = ccw.points().to_vec();
    cw_pts.reverse();
    let cw = Contour::new(cw_pts).unwrap();
    assert!(ccw.signed_area() > 0.0);
    assert!(cw.signed_area() < 0.0);
}

#[test]
fn offsetting_grows_and_shrinks() {
    let r = square(5.0);
    let bigger = r.offset(1.0, 0.001);
    let smaller = r.offset(-1.0, 0.001);
    assert!(bigger.area() > r.area(), "outward offset must grow");
    assert!(smaller.area() < r.area(), "inward offset must shrink");
    // 10x10 grown by 1 with round corners: 144 minus the corner shortfall.
    assert!((bigger.area() - 143.1).abs() < 1.0, "got {}", bigger.area());
    assert!(
        (smaller.area() - 64.0).abs() < 0.5,
        "got {}",
        smaller.area()
    );
}

/// Shrinking past the middle must vanish, not invert.
#[test]
fn over_shrinking_produces_nothing() {
    let gone = square(2.0).offset(-5.0, 0.001);
    assert!(
        gone.is_empty() || gone.area() < 1e-6,
        "area {}",
        gone.area()
    );
}

#[test]
fn booleans_behave() {
    let a = Region::rectangle(dvec2(0.0, 0.0), dvec2(10.0, 10.0));
    let b = Region::rectangle(dvec2(5.0, 0.0), dvec2(15.0, 10.0));
    assert!((a.intersection(&b).unwrap().area() - 50.0).abs() < 0.1);
    assert!((a.union(&b).unwrap().area() - 150.0).abs() < 0.1);
    assert!((a.difference(&b).unwrap().area() - 50.0).abs() < 0.1);
    assert!(a.difference(&a).unwrap().area() < 0.1);
}

#[test]
fn empty_operands_are_handled() {
    let a = square(3.0);
    let e = Region::default();
    assert_eq!(a.difference(&e).unwrap().area(), a.area());
    assert!(a.intersection(&e).unwrap().is_empty());
    assert!((a.union(&e).unwrap().area() - a.area()).abs() < 1e-9);
    assert!(e.offset(1.0, 0.001).is_empty());
}

/// Offsets err to the side a cutter needs: a grown region contains the exact
/// offset, a shrunk one lies within it. A square's round-cornered offset has a
/// known area, and the error each way is bounded by the arc tolerance.
#[test]
fn offsets_err_to_the_safe_side() {
    let r = square(5.0);
    let tol = 0.01;
    let exact_grown = 100.0 + 4.0 * 10.0 * 2.0 + std::f64::consts::PI * 4.0;
    let grown = r.offset(2.0, tol).area();
    assert!(grown >= exact_grown, "{grown} < {exact_grown}");
    assert!(grown <= exact_grown + 2.0 * tol * 60.0, "{grown}");

    // A frame shrunk inward: the outer square loses 2 each side and keeps its
    // corners; the hole grows round-cornered, exactly as `square` did above.
    // The room left must not exceed the exact room.
    let frame = Region::rectangle(dvec2(-10.0, -10.0), dvec2(10.0, 10.0))
        .difference(&r)
        .unwrap();
    let room = frame.offset(-2.0, tol).area();
    let exact = 16.0 * 16.0 - exact_grown;
    assert!(room <= exact + 1e-6, "{room} > {exact}");
    assert!(room >= exact - 2.0 * tol * 60.0, "{room}");
}

/// Growing for coverage contains the round offset, however the corners turn.
#[test]
fn growing_for_coverage_contains_the_round_offset() {
    // An L: convex corners and one reflex one.
    let l = Region::rectangle(dvec2(0.0, 0.0), dvec2(10.0, 2.0))
        .union(&Region::rectangle(dvec2(0.0, 0.0), dvec2(2.0, 10.0)))
        .unwrap();
    // Round, and inside the exact offset: offsets are padded by their arc
    // tolerance, so asking for that much less stays within the exact one.
    let round = l.offset(3.0 - 0.002, 0.001);
    let covering = l.grow_covering(3.0);
    assert!(
        round.difference(&covering).unwrap().area() < 1e-6,
        "the covering growth misses part of the round one"
    );
}

/// Adding polygons to a region keeps its holes, unless a polygon covers them,
/// and polygons count once whichever way they wind.
#[test]
fn adding_polygons_keeps_holes() {
    let frame = Region::rectangle(dvec2(0.0, 0.0), dvec2(10.0, 10.0))
        .difference(&square_at(5.0, 5.0, 2.0))
        .unwrap();
    assert!((frame.area() - 84.0).abs() < 1e-6);

    let far = Contour::new(vec![dvec2(20.0, 0.0), dvec2(20.0, 1.0), dvec2(21.0, 0.0)]).unwrap();
    let kept = frame.with_polygons(&[far]).unwrap();
    assert!((kept.area() - 84.5).abs() < 1e-6, "{}", kept.area());

    // Clockwise, over the hole: fills it.
    let plug = Contour::new(vec![
        dvec2(2.0, 2.0),
        dvec2(2.0, 8.0),
        dvec2(8.0, 8.0),
        dvec2(8.0, 2.0),
    ])
    .unwrap();
    let filled = frame.with_polygons(&[plug]).unwrap();
    assert!((filled.area() - 100.0).abs() < 1e-6, "{}", filled.area());
}

fn square_at(cx: f64, cy: f64, half: f64) -> Region {
    Region::rectangle(dvec2(cx - half, cy - half), dvec2(cx + half, cy + half))
}

#[test]
fn containment_respects_holes() {
    let frame = Region::rectangle(dvec2(0.0, 0.0), dvec2(10.0, 10.0))
        .difference(&square_at(5.0, 5.0, 2.0))
        .unwrap();
    assert!(frame.contains(dvec2(1.0, 1.0)));
    assert!(!frame.contains(dvec2(5.0, 5.0)), "inside the hole");
    assert!(!frame.contains(dvec2(11.0, 5.0)));
}

/// A line crossing a region comes back as the stretches inside it.
#[test]
fn polylines_are_clipped_to_the_stretches_inside() {
    let r = square_at(0.0, 0.0, 5.0);
    let line = vec![dvec2(-10.0, 0.0), dvec2(10.0, 0.0)];
    let pieces = r.clip_polylines(&[line]).unwrap();
    assert_eq!(pieces.len(), 1);
    let len: f64 = pieces[0].windows(2).map(|w| w[0].distance(w[1])).sum();
    assert!((len - 10.0).abs() < 1e-6, "{len}");

    // In and out twice: two pieces.
    let frame = r.difference(&square_at(0.0, 0.0, 2.0)).unwrap();
    let line = vec![dvec2(-10.0, 0.0), dvec2(10.0, 0.0)];
    assert_eq!(frame.clip_polylines(&[line]).unwrap().len(), 2);
    assert!(
        Region::default()
            .clip_polylines(&[vec![dvec2(0.0, 0.0), dvec2(1.0, 0.0)]])
            .unwrap()
            .is_empty()
    );
}

/// The question roughing actually asks: where is the surface above this level?
#[test]
fn contours_a_plateau_out_of_a_field() {
    // A 20x20 field, everything at 0 except a 6x6 block at 10.
    let mut f = HeightMap::filled(dvec2(0.0, 0.0), 1.0, 20, 20, 0.0).unwrap();
    for iy in 7..13 {
        for ix in 7..13 {
            f.set(ix, iy, 10.0);
        }
    }
    let r = contour_above(&f, 5.0);
    assert!(!r.is_empty(), "the plateau should produce a ring");
    // The boundary runs through the midpoints between inside and outside cell
    // centres, so six cells at pitch 1 enclose a 6 x 6 region: area 36. The four
    // corners are cut diagonally, each removing half of a 0.5 x 0.5 square, so
    // the exact answer is 36 - 4 * 0.125 = 35.5.
    assert!(
        (r.area() - 35.5).abs() < 0.01,
        "expected 35.5 from the marching-squares geometry, got {}",
        r.area()
    );
}

/// A rib one cell wide running diagonally either way is one connected
/// region of the same area: the saddles where its cells meet corner to
/// corner are joined, wound as every other case is, so the rings close on
/// themselves rather than across a chord.
#[test]
fn a_diagonal_rib_contours_the_same_either_way() {
    let rib = |up: bool| {
        let mut f = HeightMap::filled(dvec2(0.0, 0.0), 1.0, 12, 12, 0.0).unwrap();
        for k in 0..7 {
            let iy = if up { 2 + k } else { 9 - k };
            f.set(2 + k, iy, 10.0);
        }
        contour_above(&f, 5.0)
    };
    let (up, down) = (rib(true), rib(false));
    // Seven cells, each a diamond of half a cell, and six saddles joining
    // them, each half a cell less its two cut corners' eighths: 7 × 0.5 +
    // 6 × 0.5.
    for (r, which) in [(&up, "up"), (&down, "down")] {
        assert_eq!(r.rings().len(), 1, "{which}: one ring");
        assert!(
            (r.area() - 6.5).abs() < 0.01,
            "{which}: expected 6.5, got {}",
            r.area()
        );
    }
}

#[test]
fn a_level_above_everything_contours_nothing() {
    let f = HeightMap::filled(dvec2(0.0, 0.0), 1.0, 10, 10, 1.0).unwrap();
    assert!(contour_above(&f, 5.0).is_empty());
}

/// An unset cell means the height is unknown. Claiming material there would let
/// roughing cut where nothing has been established.
#[test]
fn unset_cells_count_as_below() {
    let mut f = HeightMap::filled(dvec2(0.0, 0.0), 1.0, 10, 10, f64::NAN).unwrap();
    for iy in 3..7 {
        for ix in 3..7 {
            f.set(ix, iy, 10.0);
        }
    }
    let r = contour_above(&f, 1.0);
    assert!(!r.is_empty());
    assert!(r.area() < 30.0, "only the known block, got {}", r.area());
}

/// A field entirely above the level has no boundary *inside* the grid. Without
/// a border of "outside", marching squares emits nothing and the caller reads
/// that as "nothing to cut" -- the difference between clearing the whole block
/// and clearing none of it.
#[test]
fn a_uniformly_high_field_contours_its_whole_extent() {
    let f = HeightMap::filled(dvec2(0.0, 0.0), 1.0, 10, 10, 10.0).unwrap();
    let r = contour_above(&f, 5.0);
    assert!(!r.is_empty(), "a full field must contour to its own extent");
    // Cell centres span 0..9, and the boundary sits half a cell beyond each
    // edge, so the ring encloses 10 x 10 less the four cut corners.
    assert!(
        (r.area() - 99.5).abs() < 0.01,
        "expected the full extent, got {}",
        r.area()
    );
}

#[test]
fn degenerate_fields_do_not_panic() {
    // One cell above the level is a real, if tiny, region: a single cell
    // surrounded by outside. It has a boundary, so it contours.
    let tiny = HeightMap::filled(dvec2(0.0, 0.0), 1.0, 1, 1, 1.0).unwrap();
    assert!(!contour_above(&tiny, 0.0).is_empty());
    assert!(
        contour_above(&tiny, 5.0).is_empty(),
        "below the level: nothing"
    );

    let f = HeightMap::filled(dvec2(0.0, 0.0), 1.0, 4, 4, 1.0).unwrap();
    assert!(contour_above(&f, f64::NAN).is_empty());
}

/// A closed line inside comes back whole; one crossing the edge comes back as
/// the stretches inside, the one through its start joined again, each running
/// the way the line did.
#[test]
fn clip_lines_keeps_whole_rings_and_rejoins_the_seam() {
    let region = Region::rectangle(dvec2(0.0, 0.0), dvec2(10.0, 10.0));
    let inside = vec![
        dvec2(2.0, 2.0),
        dvec2(4.0, 2.0),
        dvec2(4.0, 4.0),
        dvec2(2.0, 4.0),
    ];
    // Starts inside, runs out past x = 10 and back.
    let across = vec![
        dvec2(8.0, 5.0),
        dvec2(8.0, 3.0),
        dvec2(12.0, 3.0),
        dvec2(12.0, 7.0),
        dvec2(8.0, 7.0),
    ];
    let open = vec![dvec2(-2.0, 1.0), dvec2(12.0, 1.0)];
    let out = region
        .clip_lines(&[(inside.clone(), true), (across, true), (open, false)])
        .unwrap();
    assert_eq!(out.len(), 3, "{out:?}");
    assert!(out.iter().any(|(p, closed)| *closed && *p == inside));
    let seam = out
        .iter()
        .find(|(p, _)| p.iter().any(|q| (q.y - 5.0).abs() < 1e-9))
        .unwrap();
    assert!(!seam.1);
    // From where it comes back in at y = 7, down through its start, out at y = 3.
    assert!((seam.0.first().unwrap().y - 7.0).abs() < 1e-6, "{seam:?}");
    assert!((seam.0.last().unwrap().y - 3.0).abs() < 1e-6, "{seam:?}");
    let cut = out
        .iter()
        .find(|(p, _)| p.iter().all(|q| (q.y - 1.0).abs() < 1e-9))
        .unwrap();
    assert!(cut.0.first().unwrap().x < cut.0.last().unwrap().x);
}

/// A frame with an island in its hole is two pieces: the frame with its hole,
/// and the island.
#[test]
fn pieces_nest_holes_in_the_smallest_outer_round_them() {
    let frame = square_at(0.0, 0.0, 10.0)
        .difference(&square_at(0.0, 0.0, 6.0))
        .unwrap();
    let region = frame.union(&square_at(0.0, 0.0, 2.0)).unwrap();
    let mut areas: Vec<f64> = region.pieces().iter().map(Region::area).collect();
    areas.sort_by(f64::total_cmp);
    assert_eq!(areas.len(), 2, "{areas:?}");
    assert!((areas[0] - 16.0).abs() < 1e-6, "{areas:?}");
    assert!((areas[1] - (400.0 - 144.0)).abs() < 1e-6, "{areas:?}");
    assert!(Region::default().pieces().is_empty());
}

/// A hole touching its outer at a point, as the clipper's rings can, is still
/// its hole, even when the point it touches at comes first.
#[test]
fn a_hole_touching_its_outer_stays_in_its_piece() {
    let outer = Contour::new(vec![
        dvec2(0.0, 0.0),
        dvec2(10.0, 0.0),
        dvec2(10.0, 10.0),
        dvec2(0.0, 10.0),
    ])
    .unwrap();
    // Clockwise, from its apex on the outer's top edge.
    let hole = Contour::new(vec![dvec2(5.0, 10.0), dvec2(7.0, 5.0), dvec2(3.0, 5.0)]).unwrap();
    let region = Region {
        rings: vec![outer, hole.clone()],
    };
    let pieces = region.pieces();
    assert_eq!(pieces.len(), 1);
    assert!(
        (pieces[0].area() - 90.0).abs() < 1e-9,
        "{}",
        pieces[0].area()
    );

    // And as the clipper leaves it.
    let cut = Region::rectangle(dvec2(0.0, 0.0), dvec2(10.0, 10.0))
        .difference(&Region::new(vec![hole]))
        .unwrap();
    let area: f64 = cut.pieces().iter().map(Region::area).sum();
    assert!((area - 90.0).abs() < 1e-9, "{area} from {cut:?}");
}
