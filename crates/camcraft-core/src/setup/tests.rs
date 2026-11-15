use super::*;
use crate::job::{Along, FixedBox, ModelDef, Place, StockDef};
use crate::setup::frame::{Axis, Orientation};
use glam::dvec3;

fn job(stock: StockSpec) -> Job {
    let mut j = Job::default();
    j.models.push(ModelDef {
        name: "m".into(),
        path: "m.stl".into(),
        units: None,
        transform: Default::default(),
    });
    j.stocks.push(StockDef {
        name: "s".into(),
        spec: stock,
    });
    let mut s = Setup::new("top");
    s.models = vec!["m".into()];
    s.stocks = vec!["s".into()];
    j.setups.push(s);
    j
}

/// A box from (0, 0, 0) to (10, 20, 5).
fn brick() -> TriMesh {
    TriMesh::new(
        vec![
            dvec3(0.0, 0.0, 0.0),
            dvec3(10.0, 20.0, 5.0),
            dvec3(10.0, 0.0, 0.0),
        ],
        vec![[0, 1, 2]],
    )
    .unwrap()
}

/// As modelled, with the model's own zero, nothing moves: the path the old
/// single-setup job took.
#[test]
fn the_default_setup_is_the_model_as_drawn() {
    let mesh = brick();
    let body = Body::derived(mesh.clone());
    let meshes: Bodies<'_> = [("m", &body)].into_iter().collect();
    let j = job(StockSpec::AroundModels {
        models: vec![],
        margin: [2.0, 2.0, 3.0],
        below: 0.0,
    });
    let p = prepare(&j, 0, &meshes).unwrap();
    assert_eq!(p.part, mesh);
    assert_eq!(p.stocks[0].min, dvec3(-2.0, -2.0, 0.0));
    assert_eq!(p.stocks[0].max, dvec3(12.0, 22.0, 8.0));
}

/// A fixed box sits against the models' box on each axis, as Fusion's does:
/// centred, beyond a low or high side, or at a coordinate.
#[test]
fn a_fixed_box_is_placed_against_the_models() {
    let mesh = brick();
    let body = Body::derived(mesh.clone());
    let meshes: Bodies<'_> = [("m", &body)].into_iter().collect();
    let block = |place: [Place; 3]| {
        let spec = StockSpec::FixedBox(FixedBox {
            size: [14.0, 30.0, 8.0],
            models: vec![],
            place,
        });
        prepare(&job(spec), 0, &meshes).unwrap().stocks[0]
    };
    // The brick is (0, 0, 0) to (10, 20, 5).
    let b = block([
        Place::Center {},
        Place::Center {},
        Place::Max { offset: 0.0 },
    ]);
    assert_eq!(b.min, dvec3(-2.0, -5.0, -3.0));
    assert_eq!(b.max, dvec3(12.0, 25.0, 5.0));
    let b = block([
        Place::Min { offset: 1.0 },
        Place::Max { offset: 2.0 },
        Place::Min { offset: 0.5 },
    ]);
    assert_eq!(b.min, dvec3(-1.0, -8.0, -0.5));
    assert_eq!(b.max, dvec3(13.0, 22.0, 7.5));
    let b = block([
        Place::At { coordinate: 7.0 },
        Place::Min { offset: -1.0 },
        Place::Center {},
    ]);
    assert_eq!(b.min, dvec3(7.0, 1.0, -1.5));
    assert_eq!(b.max, dvec3(21.0, 31.0, 6.5));
}

/// One placed against named models is placed against them, not the setup's.
#[test]
fn a_fixed_box_can_be_placed_against_named_models() {
    let body = Body::derived(brick());
    let other = Body::derived(brick().mapped(|p| p + dvec3(100.0, 0.0, 0.0)).unwrap());
    let meshes: Bodies<'_> = [("m", &body), ("n", &other)].into_iter().collect();
    let mut j = job(StockSpec::FixedBox(FixedBox {
        models: vec!["n".into()],
        ..FixedBox::centred([10.0, 20.0, 6.0])
    }));
    j.models.push(ModelDef {
        name: "n".into(),
        path: "n.stl".into(),
        units: None,
        transform: Default::default(),
    });
    let b = prepare(&j, 0, &meshes).unwrap().stocks[0];
    assert_eq!(b.min, dvec3(100.0, 0.0, -1.0));
    assert_eq!(b.max, dvec3(110.0, 20.0, 5.0));
    assert_eq!(
        stock_block(&j.stocks[0].spec, &["m".into()], &meshes).unwrap(),
        b
    );
}

/// Turned over, the underside is on top, and a work zero at the stock's top
/// centre puts the top of the block at Z = 0.
#[test]
fn a_flipped_setup_sees_the_underside() {
    let mesh = brick();
    let body = Body::derived(mesh.clone());
    let meshes: Bodies<'_> = [("m", &body)].into_iter().collect();
    let mut j = job(StockSpec::FixedBox(FixedBox::at(
        [-1.0, -1.0, -1.0],
        [12.0, 22.0, 7.0],
    )));
    j.setups[0].orientation = Orientation::default().flipped();
    j.setups[0].origin = Origin::BoxPoint {
        of: BoxOf::Stock,
        x: Along::Center,
        y: Along::Center,
        z: Along::Max,
        offset: [0.0; 3],
    };
    let p = prepare(&j, 0, &meshes).unwrap();
    let s = p.stocks[0];
    assert_eq!(s.max.z, 0.0);
    assert_eq!(s.min.z, -7.0);
    assert_eq!((s.min.x + s.max.x) / 2.0, 0.0);
    assert_eq!((s.min.y + s.max.y) / 2.0, 0.0);
    // The model's bottom (z = 0) is now 1 mm under the stock top.
    assert_eq!(p.model_bounds.max.z, -1.0);
    // And the frame takes a model point to where it now is.
    assert_eq!(p.frame.to_setup(dvec3(0.0, 0.0, 0.0)).z, -1.0);
}

/// One block serves a top and a bottom setup: the same material, seen from
/// each side.
#[test]
fn two_setups_share_one_block() {
    let mesh = brick();
    let body = Body::derived(mesh.clone());
    let meshes: Bodies<'_> = [("m", &body)].into_iter().collect();
    let mut j = job(StockSpec::AroundModels {
        models: vec![],
        margin: [1.0, 1.0, 1.0],
        below: 2.0,
    });
    let mut bottom = j.setups[0].clone();
    bottom.name = "bottom".into();
    bottom.orientation = Orientation {
        z: Axis::NegZ,
        x: Axis::PosX,
    };
    j.setups.push(bottom);
    let top = prepare(&j, 0, &meshes).unwrap();
    let under = prepare(&j, 1, &meshes).unwrap();
    let (a, b) = (top.stocks[0], under.stocks[0]);
    assert_eq!(a.extent(), b.extent());
    // Model zero stays zero, so the block's top is the top's 6 and the
    // bottom's 2.
    assert_eq!(a.max.z, 6.0);
    assert_eq!(b.max.z, 2.0);
}

/// A stock placed in a setup's frame from outside it lands where the setup
/// would put it, were it the setup's own.
#[test]
fn a_stock_in_frame_is_where_prepare_puts_it() {
    let mesh = brick();
    let body = Body::derived(mesh.clone());
    let meshes: Bodies<'_> = [("m", &body)].into_iter().collect();
    for spec in [
        StockSpec::AroundModels {
            models: vec![],
            margin: [1.0, 2.0, 3.0],
            below: 4.0,
        },
        StockSpec::FixedBox(FixedBox::at([-5.0, -5.0, -1.0], [30.0, 40.0, 9.0])),
    ] {
        let mut j = job(spec.clone());
        j.setups[0].orientation = Orientation {
            z: Axis::NegZ,
            x: Axis::PosX,
        };
        j.setups[0].origin = Origin::BoxPoint {
            of: BoxOf::Stock,
            x: Along::Max,
            y: Along::Center,
            z: Along::Min,
            offset: [1.0, 0.0, 0.5],
        };
        let p = prepare(&j, 0, &meshes).unwrap();
        let placed = stock_in_frame(&spec, &j.setups[0].models, &p.frame, &meshes).unwrap();
        assert_eq!(placed, p.stocks[0]);
    }
}

#[test]
fn a_missing_mesh_is_named() {
    let meshes = Bodies::default();
    let j = job(StockSpec::default());
    assert!(matches!(
        prepare(&j, 0, &meshes),
        Err(PrepareError::NotLoaded(n)) if n == "m"
    ));
}

/// A work zero on a hole's centre puts the hole's centre at the setup's
/// origin, however the part sits.
#[test]
fn a_work_zero_on_a_picked_point() {
    use crate::job::{PointRef, Snap};
    use camcraft_geom::synthetic;
    use camcraft_geom::topology::EdgeKind;
    let body = Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 32));
    let rim = body
        .topology()
        .edges()
        .iter()
        .position(|e| matches!(e.kind, EdgeKind::Circle(c) if c.center.z > 2.5))
        .unwrap();
    let bodies: Bodies<'_> = [("m", &body)].into_iter().collect();
    let mut j = job(StockSpec::default());
    j.setups[0].origin = Origin::Point {
        point: PointRef {
            model: "m".into(),
            snap: Snap::CircleCenter,
            index: u32::try_from(rim).unwrap(),
            at: [0.0, 0.0, 5.0],
        },
        offset: [0.0; 3],
    };
    for flipped in [false, true] {
        if flipped {
            j.setups[0].orientation = Orientation::default().flipped();
        }
        let p = prepare(&j, 0, &bodies).unwrap();
        let centre = p.frame.to_setup(dvec3(0.0, 0.0, 5.0));
        assert!(centre.length() < 1e-9, "flipped {flipped}: {centre}");
    }
    // Picked on something that is not there: the setup is not laid out.
    if let Origin::Point { point, .. } = &mut j.setups[0].origin {
        point.at = [7.0, 7.0, 7.0];
    }
    assert!(matches!(
        prepare(&j, 0, &bodies),
        Err(PrepareError::Feature(_))
    ));
}

/// Two models merged: the part is both, and names them in the setup's order.
#[test]
fn pieces_name_the_models_merged_in_order() {
    let a = Body::derived(brick());
    let two = TriMesh::new(
        vec![
            dvec3(0.0, 0.0, 0.0),
            dvec3(1.0, 0.0, 0.0),
            dvec3(0.0, 1.0, 0.0),
            dvec3(1.0, 1.0, 0.0),
        ],
        vec![[0, 1, 2], [1, 3, 2]],
    )
    .unwrap();
    let b = Body::derived(two);
    let bodies: Bodies<'_> = [("m", &a), ("n", &b)].into_iter().collect();
    let mut j = job(StockSpec::default());
    j.models.push(ModelDef {
        name: "n".into(),
        path: "n.stl".into(),
        units: None,
        transform: Default::default(),
    });
    j.setups[0].models = vec!["n".into(), "m".into()];
    let p = prepare(&j, 0, &bodies).unwrap();
    assert_eq!(p.part.len(), 3);
    let names: Vec<&str> = p.pieces.iter().map(|p| p.model.as_str()).collect();
    assert_eq!(names, ["n", "m"]);
    assert!(p.machines("m"));
    assert!(!p.machines("o"));
    assert!(p.pieces.iter().all(|p| p.copy == 0 && p.stand.is_none()));
}

/// The part's shadow is cast once and kept, and is the part seen from above.
#[test]
fn the_shadow_is_cast_once() {
    let body = Body::derived(camcraft_geom::synthetic::step(5.0, 10.0));
    let bodies: Bodies<'_> = [("m", &body)].into_iter().collect();
    let p = prepare(&job(StockSpec::default()), 0, &bodies).unwrap();
    let first = p.shadow().unwrap();
    assert!(std::ptr::eq(first, p.shadow().unwrap()));
    assert!(
        (first.area().area() - 400.0).abs() < 1e-6,
        "{}",
        first.area().area()
    );
    let heights = p.heights();
    assert_eq!((heights.model_bottom, heights.model_top), (0.0, 5.0));
    assert_eq!(heights.picked, None);
}

/// A shadow kept in the turned frame serves a setup laid out from another work
/// zero, and another stock: moved there, it is the shadow that setup casts
/// itself; and a setup that has one already keeps it.
#[test]
fn a_turned_shadow_serves_any_work_zero() {
    let body = Body::derived(camcraft_geom::synthetic::step(5.0, 10.0));
    let bodies: Bodies<'_> = [("m", &body)].into_iter().collect();
    let first = prepare(&job(StockSpec::default()), 0, &bodies).unwrap();
    first.shadow().unwrap();
    let turned = first.turned_shadow().unwrap();

    let mut moved = job(StockSpec::default());
    moved.setups[0].origin = Origin::ModelOrigin {
        offset: [3.0, -7.5, 2.0],
    };
    let other = prepare(&moved, 0, &bodies).unwrap();
    assert!(other.adopt_shadow(&turned));
    let own = prepare(&moved, 0, &bodies).unwrap();
    let cast = own.shadow().unwrap();
    let given = other.cast_shadow().unwrap();
    assert!((cast.area().area() - 400.0).abs() < 1e-6);
    for (a, b) in [(given, cast), (cast, given)] {
        assert!(a.area().difference(b.area()).unwrap().area() < 1e-9);
    }
    assert!(!other.adopt_shadow(&turned), "one it has is kept");
}

/// A turn about Z by `degrees`, then a move by `by`.
fn stand(degrees: f64, by: DVec3) -> Stand {
    Stand {
        turn: camcraft_geom::turn::Turn::from_degrees(degrees).unwrap(),
        offset: by,
    }
}

/// The box around `mesh`'s points where the copy at `stand` has them.
fn stood_box(mesh: &TriMesh, stand: &Stand) -> Aabb {
    let moved = mesh.mapped(|p| stand.point(p)).unwrap();
    moved.bounds().unwrap()
}

/// A model collected from its name and body stands once, where it is, and is
/// laid out bit for bit as one given that stand outright.
#[test]
fn a_model_collected_from_its_body_stands_once_where_it_is() {
    let body = Body::derived(brick());
    let collected: Bodies<'_> = [("m", &body)].into_iter().collect();
    let copies = collected.get("m").unwrap();
    assert_eq!(copies.stands, [Stand::NONE]);
    let stood = Bodies::standing([("m", &body, &[Stand::NONE][..])]);
    let j = job(StockSpec::default());
    let (a, b) = (
        prepare(&j, 0, &collected).unwrap(),
        prepare(&j, 0, &stood).unwrap(),
    );
    assert_eq!(a.part, b.part);
    assert_eq!(a.part, brick());
    assert_eq!(a.model_bounds, b.model_bounds);
    assert_eq!(a.stocks, b.stocks);
    assert_eq!(a.pieces, b.pieces);
}

/// Three copies of the brick, where it is, a quarter turn round and moved,
/// and turned by a free angle and moved: the part is all three, each where
/// its stand puts it; the box holds them all, the free angle's by its turned
/// points; and the pieces name each copy in order.
#[test]
fn every_copy_is_merged_into_the_part() {
    let body = Body::derived(brick());
    let stands = [
        Stand::NONE,
        stand(90.0, dvec3(100.0, 0.0, 0.0)),
        stand(30.0, dvec3(0.0, 100.0, 0.0)),
    ];
    let bodies = Bodies::standing([("m", &body, &stands[..])]);
    let p = prepare(&job(StockSpec::default()), 0, &bodies).unwrap();
    assert_eq!(p.part.len(), 3);
    let copies: Vec<(usize, Stand)> = p.pieces.iter().map(|q| (q.copy, q.stand)).collect();
    assert_eq!(copies, [(0, stands[0]), (1, stands[1]), (2, stands[2])]);
    assert!(p.pieces.iter().all(|q| q.model == "m"));
    // Each copy's triangle is where its stand puts the brick's.
    for (k, s) in stands.iter().enumerate() {
        let want = brick()
            .triangle(0)
            .unwrap()
            .map(|v| p.frame.to_setup_as(s, v));
        assert_eq!(p.part.triangle(k).unwrap(), want, "copy {k}");
    }
    // The quarter turn takes (10, 20) to (-20, 10), exactly.
    assert_eq!(p.part.triangle(1).unwrap()[1], dvec3(80.0, 10.0, 5.0));
    let all = stands
        .iter()
        .map(|s| stood_box(&brick(), s))
        .reduce(|a, b| a.union(&b))
        .unwrap();
    assert_eq!(p.model_bounds, all);
    assert_eq!(bodies.get("m").unwrap().bounds(), Some(all));
    assert_eq!(bounds_of(&["m".into()], &bodies).unwrap(), all);
    // The stock, around the models, is around every copy.
    let s = p.stocks[0];
    assert!(
        s.min.cmple(all.min).all() && s.max.cmpge(all.max).all(),
        "{s:?}"
    );
}

/// A model with no copy standing, as one its sheet had no room for, adds
/// nothing to the part, its box or its stock; a setup or a stock with no copy
/// of anything is refused, never laid out around nothing.
#[test]
fn a_model_with_no_copy_adds_nothing() {
    let body = Body::derived(brick());
    let far = Body::derived(brick().mapped(|p| p + dvec3(500.0, 0.0, 0.0)).unwrap());
    let one = [Stand::NONE];
    let bodies = Bodies::standing([("m", &body, &one[..]), ("n", &far, &[][..])]);
    let mut j = job(StockSpec::default());
    j.models.push(ModelDef {
        name: "n".into(),
        path: "n.stl".into(),
        units: None,
        transform: Default::default(),
    });
    j.setups[0].models = vec!["m".into(), "n".into()];
    let p = prepare(&j, 0, &bodies).unwrap();
    assert_eq!(p.part, brick());
    assert!(p.machines("m") && !p.machines("n"));
    assert_eq!(p.model_bounds, brick().bounds().unwrap());
    assert!(p.stocks[0].max.x < 100.0, "{:?}", p.stocks[0]);
    assert_eq!(bodies.get("n").unwrap().bounds(), None);

    j.setups[0].models = vec!["n".into()];
    let err = prepare(&j, 0, &bodies).unwrap_err();
    assert!(
        matches!(&err, PrepareError::NoCopies(m) if m == "'n'"),
        "{err}"
    );
    assert_eq!(err.to_string(), "no copy of 'n' stands anywhere");
    let around_n = StockSpec::AroundModels {
        models: vec!["n".into()],
        margin: [1.0; 3],
        below: 0.0,
    };
    assert!(matches!(
        stock_block(&around_n, &["m".into()], &bodies),
        Err(PrepareError::NoCopies(_))
    ));
}

/// A work zero picked on a model is where the model's one stand puts it; on a
/// model with no copy, or several, it is no one place, and is refused.
#[test]
fn a_work_zero_is_picked_on_a_model_that_stands_once() {
    use crate::job::{PointRef, Snap};
    use camcraft_geom::synthetic;
    use camcraft_geom::topology::EdgeKind;
    let body = Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 32));
    let rim = body
        .topology()
        .edges()
        .iter()
        .position(|e| matches!(e.kind, EdgeKind::Circle(c) if c.center.z > 2.5))
        .unwrap();
    let mut j = job(StockSpec::default());
    j.setups[0].origin = Origin::Point {
        point: PointRef {
            model: "m".into(),
            snap: Snap::CircleCenter,
            index: u32::try_from(rim).unwrap(),
            at: [0.0, 0.0, 5.0],
        },
        offset: [0.0; 3],
    };
    let moved = [stand(90.0, dvec3(60.0, -10.0, 0.0))];
    let once = Bodies::standing([("m", &body, &moved[..])]);
    let p = prepare(&j, 0, &once).unwrap();
    let centre = p.frame.to_setup_as(&moved[0], dvec3(0.0, 0.0, 5.0));
    assert!(centre.length() < 1e-9, "{centre}");
    assert_eq!(p.frame.origin, dvec3(60.0, -10.0, 5.0));

    // With no copy of it, another model is what the setup machines.
    let other = Body::derived(brick());
    j.models.push(ModelDef {
        name: "n".into(),
        path: "n.stl".into(),
        units: None,
        transform: Default::default(),
    });
    j.setups[0].models.push("n".into());
    let two = [Stand::NONE, moved[0]];
    let one = [Stand::NONE];
    for (stands, copies) in [(&[][..], 0), (&two[..], 2)] {
        let bodies = Bodies::standing([("m", &body, stands), ("n", &other, &one[..])]);
        let err = prepare(&j, 0, &bodies).unwrap_err();
        assert!(
            matches!(&err, PrepareError::NotOnce { model, copies: c } if model == "m" && *c == copies),
            "{err}"
        );
    }
    for (stands, says) in [
        (&[][..], "has no copy placed"),
        (&two[..], "stands 2 times"),
    ] {
        let bodies = Bodies::standing([("m", &body, stands), ("n", &other, &one[..])]);
        assert_eq!(
            prepare(&j, 0, &bodies).unwrap_err().to_string(),
            format!(
                "the work zero is picked on 'm', which {says}: pick it on the stock, or on a \
                 model that stands once"
            )
        );
    }
}
