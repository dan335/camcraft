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
    let meshes: Bodies<'_> = BTreeMap::new();
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
    assert!(p.piece("m").is_some());
    assert_eq!(p.piece("o"), None);
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
