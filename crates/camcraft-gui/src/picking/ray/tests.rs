use super::*;
use crate::picking::describe::{describe_box_point, describe_circle};
use crate::ui::measure::Measure;
use crate::view::camera::Camera;
use camcraft_core::job::{Job, ModelDef, Setup, StockDef, StockSpec};
use camcraft_core::setup::{Bodies, prepare};
use camcraft_geom::synthetic;
use camcraft_geom::topology::FaceKind;
use eframe::egui::{Rect, pos2, vec2};
use glam::{Vec3, dvec3};

/// The plate with a hole, 40 square and 5 thick, laid out with the stock 2 mm
/// round it and the work zero at the model's own.
fn scene() -> (PickScene, Arc<Model>) {
    let (scene, model, _) = laid_out();
    (scene, model)
}

/// The same, with the layout it was made ready from.
fn laid_out() -> (PickScene, Arc<Model>, Prepared) {
    let body =
        camcraft_geom::topology::Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 32));
    let mesh = body.mesh();
    let model = Arc::new(Model {
        bounds: mesh.bounds().unwrap(),
        body: body.clone(),
        file: Arc::new(body.clone()),
        path: "p.stl".into(),
        units: "mm".into(),
        notes: Vec::new(),
        warnings: Vec::new(),
    });
    let bvh = Arc::new(MeshBvh::new(model.mesh()));
    let mut j = Job::default();
    j.models.push(ModelDef {
        name: "p".into(),
        path: "p.stl".into(),
        units: None,
        transform: Default::default(),
    });
    j.stocks.push(StockDef {
        name: "s".into(),
        spec: StockSpec::default(),
    });
    let mut s = Setup::new("top");
    s.models = vec!["p".into()];
    s.stocks = vec!["s".into()];
    j.setups.push(s);
    let bodies: Bodies<'_> = [("p", &model.body)].into_iter().collect();
    let prepared = prepare(&j, 0, &bodies).unwrap();
    let scene = PickScene::new(
        &prepared,
        [("p".to_string(), model.clone(), bvh)],
        Vec::new(),
    );
    (scene, model, prepared)
}

/// Looking straight down on the plate from above, 800 by 600 points.
fn looking_down() -> (Camera, Projector, f32) {
    let mut c = Camera::framing(Vec3::new(0.0, 0.0, 2.5), 40.0);
    c.pitch = 1.5;
    c.yaw = -std::f32::consts::FRAC_PI_2;
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
    let aspect = 800.0 / 600.0;
    (c, Projector::new(c.view_projection(aspect), rect), aspect)
}

/// Looking straight down on the plate, orthographically, as the view cube's
/// Top does.
fn straight_down() -> (Camera, Projector, f32) {
    let c = Camera::framing(Vec3::new(0.0, 0.0, 2.5), 40.0).turned_to(Vec3::Z);
    assert!(c.orthographic());
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
    let aspect = 800.0 / 600.0;
    (c, Projector::new(c.view_projection(aspect), rect), aspect)
}

fn pick_at(scene: &PickScene, screen: Pos2, accept: Accept) -> Option<Target> {
    pick_from(looking_down(), scene, screen, accept)
}

fn pick_from(
    (c, proj, aspect): (Camera, Projector, f32),
    scene: &PickScene,
    screen: Pos2,
    accept: Accept,
) -> Option<Target> {
    let ray = c.ray(aspect, proj.ndc(screen));
    scene.pick(ray, c.sight(), &proj, screen, accept)
}

/// Looking straight down orthographically, a corner on top is picked, and
/// the one straight under it is hidden by the part.
#[test]
fn picking_works_straight_down() {
    let (scene, _) = scene();
    let (c, proj, _) = straight_down();
    let top = proj.screen(dvec3(20.0, 20.0, 5.0)).unwrap();
    let Some(Target::Point { point, .. }) = pick_from(straight_down(), &scene, top, Accept::Points)
    else {
        unreachable!("no point");
    };
    let at = DVec3::from(point.at);
    assert!(at.distance(dvec3(20.0, 20.0, 5.0)) < 1e-9, "{at}");
    // Under the top face, hidden by it.
    let under = dvec3(10.0, 12.0, 0.0);
    assert!(!scene.visible(c.sight(), under));
    assert!(scene.visible(c.sight(), dvec3(20.0, 20.0, 5.0)));
}

#[test]
fn a_face_under_the_pointer_is_picked_where_it_was_hit() {
    let (scene, model) = scene();
    let (_, proj, _) = looking_down();
    let at = proj.screen(dvec3(12.0, 12.0, 5.0)).unwrap();
    let Some(Target::Face { face, at, .. }) = pick_at(&scene, at, Accept::Faces) else {
        unreachable!("no face");
    };
    let kind = model.body.topology().face(face).unwrap().kind;
    assert!(matches!(kind, FaceKind::Plane { normal, .. } if normal.z > 0.5));
    assert!((at - dvec3(12.0, 12.0, 5.0)).length() < 0.05, "{at}");
    // Over the hole there is nothing: the ray goes straight through.
    let hole = proj.screen(dvec3(0.0, 0.0, 5.0)).unwrap();
    assert_eq!(pick_at(&scene, hole, Accept::Faces), None);
}

/// Pointing at a hole's rim offers its centre, for a work zero.
#[test]
fn a_round_rim_offers_its_centre() {
    let (scene, model) = scene();
    let (_, proj, _) = looking_down();
    // Between two of the rim's points, away from any of them.
    let a = std::f64::consts::TAU / 64.0;
    let rim = proj
        .screen(dvec3(4.0 * a.cos(), 4.0 * a.sin(), 5.0))
        .unwrap();
    let Some(Target::Point { point, setup }) = pick_at(&scene, rim, Accept::Points) else {
        unreachable!("no point");
    };
    assert_eq!(point.snap, Snap::CircleCenter);
    assert!(setup.distance(dvec3(0.0, 0.0, 5.0)) < 1e-9, "{setup}");
    assert!(
        describe_circle(&model.body, point.index, Measure::Metric)
            .unwrap()
            .contains("Ø8.00")
    );
    // The same place, picking edges, is the rim.
    let Some(Target::Edge { edge, .. }) = pick_at(&scene, rim, Accept::Edges) else {
        unreachable!("no edge");
    };
    assert_eq!(edge, point.index);
}

/// A corner under the pointer is a vertex; one on the far side, hidden by the
/// plate, is not picked.
#[test]
fn hidden_points_are_not_picked() {
    let (scene, _) = scene();
    let (_, proj, _) = looking_down();
    let top = proj.screen(dvec3(20.0, 20.0, 5.0)).unwrap();
    let Some(Target::Point { point, .. }) = pick_at(&scene, top, Accept::Points) else {
        unreachable!("no point");
    };
    assert_eq!(point.snap, Snap::Vertex);
    let at = DVec3::from(point.at);
    assert!(at.distance(dvec3(20.0, 20.0, 5.0)) < 1e-9, "{at}");
    // A bottom corner lies straight under the top one, and is hidden by it.
    let v = scene.models[0]
        .vertices
        .iter()
        .position(|p| (*p - dvec3(-20.0, -20.0, 0.0)).length() < 1e-9)
        .unwrap();
    let (c, _, _) = looking_down();
    assert!(!scene.visible(c.sight(), scene.models[0].vertices[v]));
}

#[test]
fn a_stock_corner_is_named_by_where_it_is_on_the_box() {
    let (scene, _) = scene();
    let (_, proj, _) = looking_down();
    let b = scene.stock.unwrap();
    let corner = proj.screen(DVec3::new(b.min.x, b.min.y, b.max.z)).unwrap();
    let Some(Target::StockPoint { x, y, z, .. }) = pick_at(&scene, corner, Accept::Points) else {
        unreachable!("no stock point");
    };
    assert_eq!((x, y, z), (Along::Min, Along::Min, Along::Max));
    assert_eq!(describe_box_point(x, y, z), "top, front-left corner");
    assert_eq!(box_points().count(), 26);
}

/// A model the setup does not machine is picked only whole, and only when
/// whole models are wanted; a stock box, likewise.
#[test]
fn others_are_picked_only_whole() {
    let body =
        camcraft_geom::topology::Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 32));
    let mesh = body.mesh();
    let model = |path: &str| {
        Arc::new(Model {
            bounds: mesh.bounds().unwrap(),
            body: body.clone(),
            file: Arc::new(body.clone()),
            path: path.into(),
            units: "mm".into(),
            notes: Vec::new(),
            warnings: Vec::new(),
        })
    };
    let (p, q) = (model("p.stl"), model("q.stl"));
    let bvh = Arc::new(MeshBvh::new(p.mesh()));
    let mut j = Job::default();
    for n in ["p", "q"] {
        j.models.push(ModelDef {
            name: n.into(),
            path: format!("{n}.stl"),
            units: None,
            transform: Default::default(),
        });
    }
    j.stocks.push(StockDef {
        name: "s".into(),
        spec: StockSpec::default(),
    });
    let mut s = Setup::new("top");
    s.models = vec!["p".into()];
    s.stocks = vec!["s".into()];
    j.setups.push(s);
    let bodies: Bodies<'_> = [("p", &p.body)].into_iter().collect();
    let prepared = prepare(&j, 0, &bodies).unwrap();
    // "q" lies exactly on "p", but only "p" is the setup's, so only its faces
    // are picked. The job's other stock is off to one side.
    let far = Aabb {
        min: dvec3(100.0, 100.0, 0.0),
        max: dvec3(110.0, 110.0, 5.0),
    };
    let scene = PickScene::new(
        &prepared,
        [("q".to_string(), q, bvh.clone()), ("p".to_string(), p, bvh)],
        vec![PickStock {
            name: "other".into(),
            bounds: far,
            in_setup: false,
        }],
    );
    assert_eq!(scene.models[0].name(), "p", "the setup's own come first");
    assert!(!scene.models[1].in_setup);
    assert!(scene.models[1].edges.is_empty());
    assert_eq!(scene.setup_models().count(), 1);

    let (_, proj, _) = looking_down();
    let face = proj.screen(dvec3(12.0, 12.0, 5.0)).unwrap();
    assert!(matches!(
        pick_at(&scene, face, Accept::Faces),
        Some(Target::Face { model, .. }) if model == "p"
    ));
    assert!(matches!(
        pick_at(&scene, face, Accept::Models),
        Some(Target::Model { .. })
    ));
    assert!(pick_at(&scene, face, Accept::Stocks).is_none());
    let off = proj.screen(dvec3(105.0, 105.0, 5.0)).unwrap();
    assert_eq!(
        pick_at(&scene, off, Accept::Stocks),
        Some(Target::Stock {
            stock: "other".into()
        })
    );
    assert!(pick_at(&scene, off, Accept::Models).is_none());
}

/// Two copies of the plate in one setup, each moved, turned and scaled its
/// own way: a face and a rim centre are picked where each copy now is, saved
/// where the file has them, and found again on the placed copy where they
/// were clicked.
#[test]
fn picks_on_placed_models_are_saved_where_their_file_has_them() {
    use camcraft_core::job::{FeatureRef, Transform};
    use camcraft_core::setup::features::{find_face, find_point};
    let file = Arc::new(camcraft_geom::topology::Body::derived(
        synthetic::plate_with_hole(20.0, 5.0, 4.0, 32),
    ));
    let at_file = Model {
        bounds: file.mesh().bounds().unwrap(),
        body: (*file).clone(),
        file,
        path: "p.stl".into(),
        units: "mm".into(),
        notes: Vec::new(),
        warnings: Vec::new(),
    };
    // "p" small and turned, to the right; "q" stretched upwards, to the left.
    let places = [
        (
            "p",
            Transform {
                position: [30.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 90.0],
                scale: [0.5; 3],
            },
        ),
        (
            "q",
            Transform {
                position: [-30.0, 0.0, 0.0],
                rotation: [0.0; 3],
                scale: [1.0, 1.0, 2.0],
            },
        ),
    ];
    let mut j = Job::default();
    let mut loaded = Vec::new();
    for (name, transform) in places {
        j.models.push(ModelDef {
            name: name.into(),
            path: format!("{name}.stl"),
            units: None,
            transform,
        });
        let m = Arc::new(at_file.placed(&transform).unwrap());
        let bvh = Arc::new(MeshBvh::new(m.mesh()));
        loaded.push((name.to_string(), m, bvh));
    }
    j.stocks.push(StockDef {
        name: "s".into(),
        spec: StockSpec::default(),
    });
    let mut s = Setup::new("top");
    s.models = vec!["p".into(), "q".into()];
    s.stocks = vec!["s".into()];
    j.setups.push(s);
    let bodies: Bodies<'_> = loaded
        .iter()
        .map(|(n, m, _)| (n.as_str(), &m.body))
        .collect();
    let prepared = prepare(&j, 0, &bodies).unwrap();
    let (p, q) = (loaded[0].1.clone(), loaded[1].1.clone());
    let scene = PickScene::new(&prepared, loaded, Vec::new());
    let to_setup = |at: DVec3| prepared.frame.to_setup(at);

    let mut c = Camera::framing(Vec3::new(0.0, 0.0, 5.0), 90.0);
    c.pitch = 1.5;
    c.yaw = -std::f32::consts::FRAC_PI_2;
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
    let aspect = 800.0 / 600.0;
    let proj = Projector::new(c.view_projection(aspect), rect);
    let view = (c, proj, aspect);

    // q's top, now at 10.
    let clicked = dvec3(-18.0, 12.0, 10.0);
    let screen = proj.screen(to_setup(clicked)).unwrap();
    let Some(Target::Face { model, face, at }) = pick_from(view, &scene, screen, Accept::Faces)
    else {
        unreachable!("no face");
    };
    assert_eq!(model, "q");
    assert!(at.distance(clicked) < 0.05, "{at}");
    let saved = FeatureRef {
        model,
        index: face,
        at: camcraft_core::setup::features::unplaced(&q.body, at),
    };
    assert!(DVec3::from(saved.at).distance(dvec3(12.0, 12.0, 5.0)) < 0.05);
    assert_eq!(find_face(&q.body, &saved, 0.01).unwrap().index, face);

    // p's rim, turned and halved: its centre is at p's position, 2.5 up.
    let a = std::f64::consts::TAU / 64.0;
    let rim = dvec3(30.0 + 2.0 * a.cos(), 2.0 * a.sin(), 2.5);
    let screen = proj.screen(to_setup(rim)).unwrap();
    let Some(Target::Point { point, setup }) = pick_from(view, &scene, screen, Accept::Points)
    else {
        unreachable!("no point");
    };
    assert_eq!(
        (point.model.as_str(), point.snap),
        ("p", Snap::CircleCenter)
    );
    assert!(
        DVec3::from(point.at).distance(dvec3(0.0, 0.0, 5.0)) < 1e-9,
        "{:?}",
        point.at
    );
    let centre = dvec3(30.0, 0.0, 2.5);
    assert!(setup.distance(to_setup(centre)) < 1e-9, "{setup}");
    let (found, _) = find_point(&p.body, &point, 0.01).unwrap();
    assert!(found.distance(centre) < 1e-9, "{found}");
}

/// The part's silhouette is worked out once for the layout, and is the one
/// the engine works out.
#[test]
fn the_silhouette_is_worked_out_once() {
    let (scene, _, prepared) = laid_out();
    let first = scene.silhouette(&prepared).unwrap();
    assert!(std::ptr::eq(first, scene.silhouette(&prepared).unwrap()));
    let fresh = camcraft_core::setup::features::silhouette(&prepared.part).unwrap();
    assert!((first.area() - fresh.area()).abs() < 1e-9);
    // The plate, less its hole.
    let hole = std::f64::consts::PI * 16.0;
    assert!(
        (first.area() - (1600.0 - hole)).abs() < 5.0,
        "{}",
        first.area()
    );
}

#[test]
fn a_ray_meets_a_box_where_it_enters() {
    let b = Aabb {
        min: dvec3(0.0, 0.0, 0.0),
        max: dvec3(1.0, 1.0, 1.0),
    };
    let down = dvec3(0.0, 0.0, -1.0);
    assert_eq!(ray_box(dvec3(0.5, 0.5, 5.0), down, &b), Some(4.0));
    assert_eq!(ray_box(dvec3(2.0, 0.5, 5.0), down, &b), None);
    assert_eq!(ray_box(dvec3(0.5, 0.5, 0.5), down, &b), Some(0.0));
    assert_eq!(ray_box(dvec3(0.5, 0.5, -1.0), down, &b), None);
}

/// What a slot does not take is not picked, even right under the pointer.
#[test]
fn only_what_is_accepted_is_picked() {
    let (scene, _) = scene();
    let (_, proj, _) = looking_down();
    let face = proj.screen(dvec3(12.0, 12.0, 5.0)).unwrap();
    assert!(pick_at(&scene, face, Accept::Edges).is_none());
    assert!(pick_at(&scene, face, Accept::Points).is_none());
    assert!(matches!(
        pick_at(&scene, face, Accept::Faces),
        Some(Target::Face { .. })
    ));
}
