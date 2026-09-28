use super::part::column_height;
use super::*;
use camcraft_geom::heightmap::narrow;
use camcraft_geom::mesh::distance_to_segment;
use camcraft_geom::synthetic;
use glam::{dvec2, dvec3};

/// A ball of radius r, standing in for the real cutter.
struct Ball(f64);
impl SweptTool for Ball {
    fn max_radius(&self) -> f64 {
        self.0
    }
    fn height_at(&self, d: f64) -> Option<f64> {
        (d <= self.0).then(|| self.0 - (self.0 * self.0 - d * d).max(0.0).sqrt())
    }
}

/// A flat mill of radius r.
struct Flat(f64);
impl SweptTool for Flat {
    fn max_radius(&self) -> f64 {
        self.0
    }
    fn height_at(&self, d: f64) -> Option<f64> {
        (d <= self.0).then_some(0.0)
    }
}

fn block() -> Stock {
    Stock::block(dvec2(-10.0, -10.0), dvec2(10.0, 10.0), 5.0, -5.0, 0.5).unwrap()
}

/// One block through `from_boxes` is the same stock as `block`, cell for cell.
#[test]
fn one_box_is_a_block() {
    for (min, max, pitch) in [
        (dvec2(-10.0, -10.0), dvec2(10.0, 10.0), 0.5),
        (dvec2(-3.3, 1.7), dvec2(12.1, 9.05), 0.37),
    ] {
        let a = Stock::block(min, max, 5.0, -5.0, pitch).unwrap();
        let b = Stock::from_boxes(&[(min, max, 5.0)], -5.0, pitch).unwrap();
        assert_eq!(a, b);
    }
}

/// Two blocks apart: material over each at its own top, none between, and the
/// volumes add up.
#[test]
fn two_boxes_are_their_union() {
    let a = (dvec2(0.0, 0.0), dvec2(10.0, 10.0), 5.0);
    let b = (dvec2(20.0, 0.0), dvec2(30.0, 10.0), 8.0);
    let s = Stock::from_boxes(&[a, b], 0.0, 0.5).unwrap();
    assert_eq!(s.height_at(5.0, 5.0), Some(5.0));
    assert_eq!(s.height_at(25.0, 5.0), Some(8.0));
    assert_eq!(s.height_at(15.0, 5.0), Some(0.0), "the gap is empty");
    let alone = |(lo, hi, top)| Stock::block(lo, hi, top, 0.0, 0.5).unwrap().volume();
    assert!((s.volume() - (alone(a) + alone(b))).abs() < 1e-6);
    // Overlapping, the taller one wins where they meet.
    let c = (dvec2(5.0, 0.0), dvec2(15.0, 10.0), 9.0);
    let s = Stock::from_boxes(&[a, c], 0.0, 0.5).unwrap();
    assert_eq!(s.height_at(7.0, 5.0), Some(9.0));
    assert_eq!(s.height_at(2.0, 5.0), Some(5.0));
    assert!(matches!(
        Stock::from_boxes(&[], 0.0, 0.5),
        Err(StockError::NoBlocks)
    ));
}

#[test]
fn rejects_an_inverted_block() {
    let e = Stock::block(dvec2(0.0, 0.0), dvec2(1.0, 1.0), 0.0, 5.0, 0.5);
    assert!(matches!(e, Err(StockError::Inverted { .. })));
}

#[test]
fn a_fresh_block_is_full() {
    let s = block();
    assert_eq!(s.height_at(0.0, 0.0), Some(5.0));
    // 20 x 20 x 10, within a cell of rounding on the grown grid.
    assert!((s.volume() - 4000.0).abs() < 400.0, "got {}", s.volume());
}

/// The defining behaviour: cutting removes material down to the tool underside.
#[test]
fn a_flat_cut_leaves_a_flat_floor() {
    let mut s = block();
    let moves = vec![
        SimMove {
            to: dvec3(-8.0, 0.0, 1.0),
            class: MoveClass::MayCut,
            arc: None,
        },
        SimMove {
            to: dvec3(8.0, 0.0, 1.0),
            class: MoveClass::MayCut,
            arc: None,
        },
    ];
    let r = s.apply(&moves, &Flat(2.0));
    assert!(r.is_clean());
    assert!(r.removed_mm3 > 0.0);
    assert_eq!(
        s.height_at(0.0, 0.0),
        Some(1.0),
        "floor should be at the tip"
    );
    assert_eq!(
        s.height_at(0.0, 9.0),
        Some(5.0),
        "away from the cut, untouched"
    );
}

/// A ball leaves its own radius behind, not a flat floor.
#[test]
fn a_ball_leaves_a_round_bottom() {
    let mut s = Stock::block(dvec2(-6.0, -6.0), dvec2(6.0, 6.0), 5.0, -5.0, 0.1).unwrap();
    let moves = vec![
        SimMove {
            to: dvec3(0.0, 0.0, 0.0),
            class: MoveClass::MayCut,
            arc: None,
        },
        SimMove {
            to: dvec3(0.0, 0.0, 0.0),
            class: MoveClass::MayCut,
            arc: None,
        },
    ];
    s.apply(&moves, &Ball(3.0));
    assert!(
        (s.height_at(0.0, 0.0).unwrap() - 0.0).abs() < 0.05,
        "tip touches bottom"
    );
    // One radius out, the ball surface is a full radius up.
    let side = s.height_at(3.0, 0.0).unwrap();
    assert!(
        (side - 3.0).abs() < 0.2,
        "expected ~3.0 at the equator, got {side}"
    );
}

/// The check the plan singles out, and the defect Pngcam documents.
#[test]
fn a_rapid_through_material_is_caught() {
    let mut s = block();
    let moves = vec![
        SimMove {
            to: dvec3(-8.0, 0.0, 1.0),
            class: MoveClass::Rapid,
            arc: None,
        },
        SimMove {
            to: dvec3(8.0, 0.0, 1.0),
            class: MoveClass::Rapid,
            arc: None,
        },
    ];
    let r = s.apply(&moves, &Flat(2.0));
    assert!(
        !r.is_clean(),
        "a rapid ploughing through stock must not pass"
    );
    assert_eq!(r.rapid_contacts, 1);
    assert!(
        r.deepest_rapid_contact_mm > 3.0,
        "got {}",
        r.deepest_rapid_contact_mm
    );
}

/// And the same move above the block is fine, or the check would be useless.
#[test]
fn a_rapid_above_the_stock_is_fine() {
    let mut s = block();
    let moves = vec![
        SimMove {
            to: dvec3(-8.0, 0.0, 20.0),
            class: MoveClass::Rapid,
            arc: None,
        },
        SimMove {
            to: dvec3(8.0, 0.0, 20.0),
            class: MoveClass::Rapid,
            arc: None,
        },
    ];
    let r = s.apply(&moves, &Flat(2.0));
    assert!(r.is_clean());
    assert_eq!(r.removed_mm3, 0.0);
}

#[test]
fn part_surface_reads_a_plane() {
    let mesh = synthetic::plane(2.0, 10.0);
    let p = PartSurface::from_mesh(&mesh, dvec2(-5.0, -5.0), dvec2(5.0, 5.0), 0.5).unwrap();
    assert!((p.height_at(0.0, 0.0).unwrap() - 2.0).abs() < 1e-9);
    assert!((p.height_at(3.3, -2.1).unwrap() - 2.0).abs() < 1e-9);
}

/// Rasterising triangle by triangle gives every column exactly the height
/// that looking at every triangle does, NaN where there is none.
#[test]
fn part_surface_is_the_highest_triangle_over_each_column() {
    for (mesh, pitch) in [
        (synthetic::uv_sphere(8.0, 40, 60), 0.37),
        (synthetic::step(4.0, 10.0), 0.25),
    ] {
        let (lo, hi) = (dvec2(-9.0, -9.0), dvec2(9.0, 9.0));
        let p = PartSurface::from_mesh(&mesh, lo, hi, pitch).unwrap();
        let grid = HeightMap::covering(lo, hi, pitch, 0.0).unwrap();
        let (mut hit, mut miss) = (0, 0);
        for iy in 0..grid.ny() {
            for ix in 0..grid.nx() {
                let at = grid.position(ix, iy).unwrap();
                assert_eq!(p.position(ix, iy), at);
                let want = column_height(&mesh, at.x, at.y).map(narrow);
                let got = p
                    .at_cell(i64::from(ix), i64::from(iy))
                    .map_or(f32::NAN, narrow);
                match want {
                    Some(w) => {
                        assert_eq!(got.to_bits(), w.to_bits(), "({ix}, {iy})");
                        hit += 1;
                    }
                    None => {
                        assert!(got.is_nan(), "({ix}, {iy}) has {got}");
                        miss += 1;
                    }
                }
            }
        }
        assert!(hit > 100, "{hit} {miss}");
    }
}

#[test]
fn part_surface_reads_a_step() {
    let mesh = synthetic::step(4.0, 10.0);
    let p = PartSurface::from_mesh(&mesh, dvec2(-5.0, -5.0), dvec2(5.0, 5.0), 0.25).unwrap();
    assert!(
        (p.height_at(-3.0, 0.0).unwrap() - 4.0).abs() < 1e-9,
        "plateau"
    );
    assert!(p.height_at(3.0, 0.0).unwrap().abs() < 1e-9, "shelf");
}

/// Cutting too deep must be detected, and cutting to the right depth must not.
#[test]
fn verify_distinguishes_a_gouge_from_a_good_cut() {
    let mesh = synthetic::plane(0.0, 12.0);
    let part = PartSurface::from_mesh(&mesh, dvec2(-6.0, -6.0), dvec2(6.0, 6.0), 0.5).unwrap();

    let sweep = |z: f64| {
        let mut s = Stock::block(dvec2(-6.0, -6.0), dvec2(6.0, 6.0), 5.0, -5.0, 0.5).unwrap();
        let mut moves = Vec::new();
        let mut y = -5.0;
        while y <= 5.0 {
            moves.push(SimMove {
                to: dvec3(-5.0, y, z),
                class: MoveClass::MayCut,
                arc: None,
            });
            moves.push(SimMove {
                to: dvec3(5.0, y, z),
                class: MoveClass::MayCut,
                arc: None,
            });
            y += 1.0;
        }
        s.apply(&moves, &Flat(1.5));
        s
    };

    let good = sweep(0.0).verify(&part, 0.01, 0.5);
    assert!(
        good.is_clean(),
        "cutting to the surface is not a gouge: {good:?}"
    );
    assert!(good.compared > 100);

    let bad = sweep(-1.0).verify(&part, 0.01, 0.5);
    assert!(!bad.is_clean(), "cutting 1 mm under the part is a gouge");
    assert!(
        (bad.deepest_gouge_mm - 1.0).abs() < 0.05,
        "got {}",
        bad.deepest_gouge_mm
    );
}

/// Beside a vertical wall, material a hair past the wall is not a gouge the
/// wall's height deep once the part is given a reach across; a cut well into
/// the wall still is.
#[test]
fn a_wall_is_judged_across_as_well_as_down() {
    // Plateau at 4 for x < 0, shelf at 0 beyond: a 4 mm wall at x = 0.
    let mesh = synthetic::step(4.0, 10.0);
    let (lo, hi, pitch) = (dvec2(-5.0, -5.0), dvec2(5.0, 5.0), 0.1);
    let column = PartSurface::from_mesh(&mesh, lo, hi, pitch).unwrap();
    let across = column.clone().reaching(0.25);
    // Clear the shelf with a flat whose edge runs `into` past the wall.
    let cut = |into: f64| {
        let mut s = Stock::block(lo, hi, 5.0, -5.0, pitch).unwrap();
        let mut moves = Vec::new();
        for y in [-4.0, -2.0, 0.0, 2.0, 4.0] {
            for x in [1.0 - into, 4.0] {
                moves.push(SimMove {
                    to: dvec3(x, y, 0.0),
                    class: MoveClass::MayCut,
                    arc: None,
                });
            }
        }
        s.sweep(&moves, &Flat(1.0));
        s
    };
    let shaved = cut(0.15);
    assert!(
        shaved.verify(&column, 0.1, 50.0).gouged > 0,
        "by columns alone"
    );
    let v = shaved.verify(&across, 0.1, 50.0);
    assert_eq!(v.gouged, 0, "{v:?}");
    let v = cut(1.0).verify(&across, 0.1, 50.0);
    assert!(v.gouged > 0 && v.deepest_gouge_mm > 3.9, "{v:?}");
    // Where nothing is near a wall, the floor is the column's own height.
    assert_eq!(across.floor_near(-3.0, 0.0), column.height_at(-3.0, 0.0));
    // Past the grid's edge there is no part: a column at the edge, where a
    // part's edge meets a stock with no margin, may be cut down past it.
    assert_eq!(across.floor_near(-5.0, 0.0), Some(f64::NEG_INFINITY));
}

/// Uncut material is reported as leftover, not as an error.
#[test]
fn untouched_stock_is_leftover_not_a_gouge() {
    let mesh = synthetic::plane(0.0, 12.0);
    let part = PartSurface::from_mesh(&mesh, dvec2(-6.0, -6.0), dvec2(6.0, 6.0), 0.5).unwrap();
    let s = Stock::block(dvec2(-6.0, -6.0), dvec2(6.0, 6.0), 5.0, -5.0, 0.5).unwrap();
    let r = s.verify(&part, 0.01, 0.5);
    assert!(r.is_clean(), "leftover is unfinished, not ruined");
    assert!(r.leftover > 100);
    assert!((r.tallest_leftover_mm - 5.0).abs() < 1e-6);
}

#[test]
fn writes_a_readable_pgm() {
    let s = block();
    let p = s.to_pgm();
    assert!(p.starts_with("P2\n"));
    assert!(p.lines().count() > 3);
}

/// A zigzag with a rapid and a ramp in it, over the whole block.
fn zigzag() -> Vec<SimMove> {
    let mut moves = vec![SimMove {
        to: dvec3(-9.0, -9.0, 8.0),
        class: MoveClass::Rapid,
        arc: None,
    }];
    let (mut y, mut z, mut sign) = (-7.0, 3.0, 1.0);
    while y <= 7.0 {
        for x in [-7.0, 6.5] {
            moves.push(SimMove {
                to: dvec3(x * sign, y, z),
                class: MoveClass::MayCut,
                arc: None,
            });
        }
        y += 2.3;
        z -= 0.4;
        sign = -sign;
    }
    moves
}

/// Every cell a sweep lowers is inside the rectangle it reports, and it lowers
/// exactly what `apply` does.
#[test]
fn a_sweep_says_where_it_cut() {
    let moves = zigzag();
    for cut in [1..4, 3..9, 0..moves.len(), 7..8] {
        let slice = moves.get(cut).unwrap();
        let mut applied = block();
        applied.apply(slice, &Ball(1.7));
        let mut swept = block();
        let before = swept.clone();
        let touched = swept.sweep(slice, &Ball(1.7)).touched;
        assert_eq!(swept, applied, "sweep and apply must leave the same stock");
        let f = swept.field();
        let mut changed = 0;
        for iy in 0..f.ny() {
            for ix in 0..f.nx() {
                if f.get(ix, iy) != before.field().get(ix, iy) {
                    changed += 1;
                    assert!(
                        touched.is_some_and(|r| r.contains(ix, iy)),
                        "cell ({ix}, {iy}) changed outside {touched:?}"
                    );
                }
            }
        }
        assert_eq!(touched.is_some(), changed > 0);
    }
}

/// A level move is swept exactly: every cell ends at the tool's underside at
/// its distance from the move's line, checked cell by cell against a brute
/// force over the whole grid, at every angle; and sampling finely along the
/// move never cuts deeper.
#[test]
fn a_level_move_is_swept_exactly() {
    let ball = Ball(1.7);
    for (a, b) in [
        ((-8.0, -3.0), (7.5, 2.2)),
        ((0.3, -9.0), (0.3, 9.0)),
        ((-9.0, 4.1), (9.0, 4.1)),
        ((5.0, 5.0), (-6.0, -7.0)),
        ((1.1, 1.3), (1.1, 1.3)),
    ] {
        let (from, to) = (dvec3(a.0, a.1, 2.0), dvec3(b.0, b.1, 2.0));
        let moves = [
            SimMove {
                to: from,
                class: MoveClass::MayCut,
                arc: None,
            },
            SimMove {
                to,
                class: MoveClass::MayCut,
                arc: None,
            },
        ];
        let mut exact = block();
        exact.sweep(&moves, &ball);
        let mut sampled = block();
        sampled.stamp_along(from, to, &ball, 0.01, true);
        let f = exact.field();
        for iy in 0..f.ny() {
            for ix in 0..f.nx() {
                let p = f.position(ix, iy).unwrap();
                let d = distance_to_segment(p, from.truncate(), to.truncate());
                let want = ball.height_at(d).map_or(5.0, |h| (2.0 + h).min(5.0));
                let got = f.get(ix, iy).unwrap();
                assert!((got - want).abs() < 1e-5, "({ix}, {iy}): {got} for {want}");
                let s = sampled.field().get(ix, iy).unwrap();
                assert!(s + 1e-6 >= got, "sampling cut deeper at ({ix}, {iy})");
                assert!(s - got < 0.01, "sampling left {} at ({ix}, {iy})", s - got);
            }
        }
    }
}

/// Straight down is the tool at the bottom, whatever the pitch.
#[test]
fn a_plunge_is_the_tool_at_its_lowest() {
    let mut plunged = block();
    let down = [
        SimMove {
            to: dvec3(1.0, 1.0, 8.0),
            class: MoveClass::MayCut,
            arc: None,
        },
        SimMove {
            to: dvec3(1.0, 1.0, 0.5),
            class: MoveClass::MayCut,
            arc: None,
        },
    ];
    plunged.sweep(&down, &Ball(2.0));
    let mut stamped = block();
    stamped.stamp(dvec3(1.0, 1.0, 0.5), &Ball(2.0));
    assert_eq!(plunged, stamped);
}

/// A packed stock is the stock it came from, bit for bit, small when the
/// stock is flat; and restoring it in place says which cells changed.
#[test]
fn a_packed_stock_is_the_stock_bit_for_bit() {
    let fresh = block();
    let mut cut = block();
    cut.sweep(&zigzag(), &Ball(1.7));
    for s in [&fresh, &cut] {
        let p = PackedStock::pack(s);
        assert_eq!(&p.unpack().unwrap(), s);
    }
    let flat = PackedStock::pack(&fresh);
    assert!(
        flat.bytes() < 64,
        "a flat block is one run: {}",
        flat.bytes()
    );
    // Restoring the fresh block over the cut one changes exactly the cut cells.
    let mut back = cut.clone();
    let touched = block().sweep(&zigzag(), &Ball(1.7)).touched;
    assert_eq!(flat.restore(&mut back), Some(touched));
    assert_eq!(back, fresh);
}

/// Cells read out and written back are the same bits, and only those move.
#[test]
fn cells_read_and_written_back_are_exact() {
    let mut s = block();
    let rect = CellRect {
        x0: 3,
        y0: 5,
        x1: 17,
        y1: 9,
    };
    let saved = s.read_cells(rect);
    assert_eq!(saved.len(), (rect.width() * rect.height()) as usize);
    let before = s.clone();
    s.sweep(&zigzag(), &Flat(2.0));
    let after = s.clone();
    s.write_cells(rect, &saved);
    let f = s.field();
    for iy in 0..f.ny() {
        for ix in 0..f.nx() {
            let want = if rect.contains(ix, iy) {
                &before
            } else {
                &after
            };
            assert_eq!(f.get(ix, iy), want.field().get(ix, iy));
        }
    }
}

#[test]
fn a_footprint_covers_the_rim_and_stays_in_the_grid() {
    let s = block();
    // Pitch 0.5 from -10: a radius-1 tool at the origin reaches cells 18..=22.
    let r = s.footprint(dvec3(0.0, 0.0, 0.0), 1.0).unwrap();
    assert_eq!((r.x0, r.x1, r.y0, r.y1), (18, 22, 18, 22));
    let edge = s.footprint(dvec3(-10.0, 30.0, 0.0), 1.0).unwrap();
    assert_eq!((edge.x0, edge.y1), (0, s.field().ny() - 1));
    assert!(s.footprint(dvec3(f64::NAN, 0.0, 0.0), 1.0).is_none());
}

#[test]
fn survives_non_finite_positions_without_panicking() {
    let mut s = block();
    let moves = vec![
        SimMove {
            to: dvec3(0.0, 0.0, 0.0),
            class: MoveClass::MayCut,
            arc: None,
        },
        SimMove {
            to: dvec3(f64::NAN, 0.0, 0.0),
            class: MoveClass::MayCut,
            arc: None,
        },
    ];
    let _ = s.apply(&moves, &Flat(1.0));
}

/// Cutting past the part as asked is no gouge where the operation that asked
/// for it cut, and still one everywhere else; and walls cut back by the radial
/// amount leave no part beside them.
#[test]
fn cutting_past_the_part_as_asked_is_no_gouge_only_where_asked() {
    let (lo, hi) = (dvec2(-5.0, -5.0), dvec2(5.0, 5.0));
    let pitch = 0.25;
    let mesh = synthetic::plane(0.0, 5.0);
    // Material everywhere half a millimetre under the part.
    let stock = Stock::block(lo, hi, -0.5, -5.0, pitch).unwrap();
    let part = PartSurface::from_mesh(&mesh, lo, hi, pitch).unwrap();
    let half = stock.field().nx() / 2;
    let all = stock.verify(&part, 0.01, 1.0);
    assert!(all.gouged > 0);
    let past = part.cut_past(0.0, 0.5, |ix, _| ix < half);
    let v = stock.verify(&past, 0.01, 1.0);
    assert!(
        v.gouged > 0 && v.gouged < all.gouged / 2 + 50,
        "{v:?} against {all:?}"
    );
    // Cut back a millimetre sideways, the part goes from within a millimetre
    // of its edge, and stays where it is further in.
    let back = PartSurface::from_mesh(&mesh, lo, hi, pitch)
        .unwrap()
        .cut_past(1.0, 0.0, |_, _| true);
    assert_eq!(back.height_at(4.8, 0.0), None);
    assert_eq!(back.height_at(0.0, 0.0), Some(0.0));
}

/// An arc is swept along itself, not along its chord: a level half turn clears a
/// curved trench, and leaves standing the middle it turns round.
#[test]
fn an_arc_is_swept_along_itself() {
    let mut s = block();
    let (from, to) = (dvec3(-6.0, 0.0, 1.0), dvec3(6.0, 0.0, 1.0));
    // Clockwise from -X, over +Y, to +X.
    let arc = Arc {
        centre: dvec3(0.0, 0.0, 1.0),
        axis: DVec3::Z,
        sweep: -std::f64::consts::PI,
    };
    let moves = [
        SimMove {
            to: from,
            class: MoveClass::MayCut,
            arc: None,
        },
        SimMove {
            to,
            class: MoveClass::MayCut,
            arc: Some(arc),
        },
    ];
    s.sweep(&moves, &Flat(1.0));
    for deg in [30.0f64, 90.0, 150.0] {
        let (y, x) = deg.to_radians().sin_cos();
        let h = s.height_at(6.0 * x, 6.0 * y).unwrap();
        assert!((h - 1.0).abs() < 1e-6, "on the arc at {deg}: {h}");
    }
    assert_eq!(s.height_at(0.0, 0.0), Some(5.0), "the chord is not cut");
    assert_eq!(s.height_at(0.0, -3.0), Some(5.0));
    // Half of it, as playback sweeps a move in pieces, stops at its quarter.
    let mut half = block();
    let part = moves[1].part(from, 0.5);
    assert!(
        part.to.distance(dvec3(0.0, 6.0, 1.0)) < 1e-9,
        "{:?}",
        part.to
    );
    half.sweep(&[moves[0], part], &Flat(1.0));
    assert_eq!(half.height_at(4.2, 4.2), Some(5.0));
    assert!((half.height_at(-4.2, 4.2).unwrap() - 1.0).abs() < 1e-6);
}
