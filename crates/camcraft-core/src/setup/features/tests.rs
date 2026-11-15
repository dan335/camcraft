use super::*;
use crate::arrange::Stand;
use camcraft_geom::synthetic;
use camcraft_geom::topology::FaceKind;
use glam::{dvec2, dvec3};

fn plate() -> Body {
    Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 32))
}

fn top_face(body: &Body) -> u32 {
    let i = body
        .topology()
        .faces()
        .iter()
        .position(|f| matches!(f.kind, FaceKind::Plane { normal, .. } if normal.z > 0.5))
        .unwrap();
    u32::try_from(i).unwrap()
}

fn rim(body: &Body, z: f64) -> u32 {
    let i = body
        .topology()
        .edges()
        .iter()
        .position(|e| matches!(e.kind, EdgeKind::Circle(c) if (c.center.z - z).abs() < 1e-9))
        .unwrap();
    u32::try_from(i).unwrap()
}

#[test]
fn an_unchanged_model_finds_picks_by_number() {
    let body = plate();
    let face = FeatureRef {
        model: "p".into(),
        index: top_face(&body),
        at: [10.0, 10.0, 5.0],
    };
    let found = find_face(&body, &face, 0.01).unwrap();
    assert_eq!(found.by, FoundBy::Index);
    assert_eq!(found.index, face.index);

    let e = rim(&body, 5.0);
    let edge = FeatureRef {
        model: "p".into(),
        index: e,
        at: [4.0, 0.0, 5.0],
    };
    assert_eq!(find_edge(&body, &edge, 0.01).unwrap().index, e);

    let centre = PointRef {
        model: "p".into(),
        snap: Snap::CircleCenter,
        index: e,
        at: [0.0, 0.0, 5.0],
    };
    let (p, found) = find_point(&body, &centre, 0.01).unwrap();
    assert!(p.distance(dvec3(0.0, 0.0, 5.0)) < 1e-9);
    assert_eq!(found.by, FoundBy::Index);
}

/// A pick is saved where the model's file puts it, so it moves with the
/// model: placed elsewhere, every pick is still found by its number, and where
/// it now is.
#[test]
fn picks_move_with_their_model() {
    use crate::job::Transform;
    let body = plate();
    let place = Transform {
        position: [100.0, 50.0, -5.0],
        rotation: [0.0, 0.0, 90.0],
        scale: [2.0; 3],
    };
    let placed = body.placed(place.matrix()).unwrap();
    let face = FeatureRef {
        model: "p".into(),
        index: top_face(&body),
        at: [10.0, 10.0, 5.0],
    };
    let found = find_face(&placed, &face, 0.01).unwrap();
    assert_eq!((found.index, found.by), (face.index, FoundBy::Index));
    let e = rim(&body, 5.0);
    let centre = PointRef {
        model: "p".into(),
        snap: Snap::CircleCenter,
        index: e,
        at: [0.0, 0.0, 5.0],
    };
    let (p, found) = find_point(&placed, &centre, 0.01).unwrap();
    assert!(p.distance(dvec3(100.0, 50.0, 5.0)) < 1e-9, "{p}");
    assert_eq!(found.by, FoundBy::Index);
    // What a click on the placed model saves is where its file has it.
    let at = unplaced(&placed, dvec3(80.0, 30.0, 5.0));
    assert!(DVec3::from_array(at).distance(dvec3(-10.0, 10.0, 5.0)) < 1e-9);
}

/// Numbers that now belong to something else: each pick is found again by
/// where it is.
#[test]
fn a_renumbered_model_finds_picks_by_position() {
    let body = plate();
    let top = top_face(&body);
    let wrong = (top + 1) % u32::try_from(body.topology().faces().len()).unwrap();
    let face = FeatureRef {
        model: "p".into(),
        index: wrong,
        at: [10.0, 10.0, 5.0],
    };
    let found = find_face(&body, &face, 0.01).unwrap();
    assert_eq!(
        found,
        Found {
            index: top,
            by: FoundBy::Position
        }
    );

    let bottom_rim = rim(&body, 0.0);
    let top_rim = rim(&body, 5.0);
    let edge = FeatureRef {
        model: "p".into(),
        index: bottom_rim,
        at: [0.0, 4.0, 5.0],
    };
    assert_eq!(find_edge(&body, &edge, 0.01).unwrap().index, top_rim);

    let centre = PointRef {
        model: "p".into(),
        snap: Snap::CircleCenter,
        index: bottom_rim,
        at: [0.0, 0.0, 5.0],
    };
    let (p, found) = find_point(&body, &centre, 0.01).unwrap();
    assert_eq!(found.index, top_rim);
    assert!(p.distance(dvec3(0.0, 0.0, 5.0)) < 1e-9);
}

#[test]
fn a_pick_on_nothing_is_gone() {
    let body = plate();
    let face = FeatureRef {
        model: "p".into(),
        index: top_face(&body),
        at: [10.0, 10.0, 10.0],
    };
    let err = find_face(&body, &face, 0.01).unwrap_err();
    assert!(matches!(err, FeatureError::Gone { what: "face", .. }));
    assert!(err.to_string().contains("pick it again"), "{err}");
    let point = PointRef {
        model: "p".into(),
        snap: Snap::Vertex,
        index: 0,
        at: [3.0, 3.0, 3.0],
    };
    assert!(find_point(&body, &point, 0.01).is_err());
}

/// A face the file named that no longer passes through its pick is refused by
/// name: it may have moved, or another face may have its number now, and the
/// two cannot be told apart.
#[test]
fn a_moved_face_from_a_file_is_refused_by_name() {
    let mesh = synthetic::cuboid(DVec3::ZERO, dvec3(10.0, 10.0, 10.0));
    let labels: Vec<u32> = (0..12).map(|t| t / 2).collect();
    let body = Body::with_faces(mesh.clone(), labels.clone(), 6).unwrap();
    // Face 1 is the top; the part has since grown 2 mm taller.
    let taller = TriMesh::new(
        mesh.vertices()
            .iter()
            .map(|v| {
                if v.z > 5.0 {
                    *v + dvec3(0.0, 0.0, 2.0)
                } else {
                    *v
                }
            })
            .collect(),
        mesh.triangles().to_vec(),
    )
    .unwrap();
    let moved = Body::with_faces(taller.clone(), labels, 6).unwrap();
    let pick = FeatureRef {
        model: "p".into(),
        index: 1,
        at: [5.0, 5.0, 10.0],
    };
    assert_eq!(find_face(&body, &pick, 0.01).unwrap().by, FoundBy::Index);
    let err = find_face(&moved, &pick, 0.01).unwrap_err();
    assert!(
        matches!(err, FeatureError::Moved { index: 1, off_by, .. } if (off_by - 2.0).abs() < 1e-9),
        "{err:?}"
    );
    assert!(err.to_string().contains("2.00 mm away"), "{err}");
    // Faces found from triangles have no numbers of their own to go by.
    let derived = Body::derived(taller);
    assert!(matches!(
        find_face(&derived, &pick, 0.01),
        Err(FeatureError::Gone { .. })
    ));
}

/// A point on a box's corner is on three edges: an edge picked there is
/// ambiguous once its number is lost.
#[test]
fn an_edge_that_could_be_several_is_refused() {
    let body = Body::derived(synthetic::cuboid(DVec3::ZERO, DVec3::ONE));
    let pick = FeatureRef {
        model: "p".into(),
        index: 999,
        at: [0.0, 0.0, 0.0],
    };
    assert!(matches!(
        find_edge(&body, &pick, 0.01),
        Err(FeatureError::Ambiguous { .. })
    ));
}

fn laid_out(body: &Body) -> Prepared {
    use crate::job::{Job, ModelDef, Setup, StockDef, StockSpec};
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
    let mut setup = Setup::new("top");
    setup.models = vec!["p".into()];
    setup.stocks = vec!["s".into()];
    j.setups.push(setup);
    let bodies: Bodies<'_> = [("p", body)].into_iter().collect();
    crate::setup::prepare(&j, 0, &bodies).unwrap()
}

fn boundary(
    body: &Body,
    b: &MachiningBoundary,
    s: &Selection,
    radius: f64,
) -> Result<Option<Region>, FeatureError> {
    let prepared = laid_out(body);
    let bodies: Bodies<'_> = [("p", body)].into_iter().collect();
    machining_boundary(b, s, &prepared, &bodies, radius, 0.01)
}

/// A boundary kept to what is picked, and nothing picked yet.
fn selection() -> (MachiningBoundary, Selection) {
    let b = MachiningBoundary {
        kind: Boundary::Selection,
        ..MachiningBoundary::default()
    };
    (b, Selection::default())
}

fn face_at(body: &Body, at: [f64; 3]) -> FeatureRef {
    let pick = FeatureRef {
        model: "p".into(),
        index: u32::MAX,
        at,
    };
    let found = find_face(body, &pick, 0.01).unwrap();
    FeatureRef {
        index: found.index,
        ..pick
    }
}

fn edge_at(body: &Body, at: [f64; 3]) -> FeatureRef {
    let pick = FeatureRef {
        model: "p".into(),
        index: u32::MAX,
        at,
    };
    let found = find_edge(body, &pick, 0.01).unwrap();
    FeatureRef {
        index: found.index,
        ..pick
    }
}

#[test]
fn nothing_picked_is_no_boundary() {
    let body = plate();
    assert_eq!(
        boundary(
            &body,
            &MachiningBoundary::default(),
            &Selection::default(),
            1.0
        ),
        Ok(None)
    );
}

/// The models' box and outline from above: the plate is 40 square with a
/// hole through it, which the box covers and the silhouette leaves open.
#[test]
fn the_bounding_box_and_silhouette_are_the_part_from_above() {
    let body = plate();
    let mut b = MachiningBoundary {
        kind: Boundary::BoundingBox,
        ..MachiningBoundary::default()
    };
    let s = Selection::default();
    let rect = boundary(&body, &b, &s, 1.0).unwrap().unwrap();
    assert!((rect.area() - 1600.0).abs() < 1e-6, "{}", rect.area());
    assert!(rect.contains(dvec2(0.0, 0.0)));

    b.kind = Boundary::Silhouette;
    let shadow = boundary(&body, &b, &s, 1.0).unwrap().unwrap();
    let hole = 0.5 * 32.0 * 16.0 * (std::f64::consts::TAU / 32.0).sin();
    assert!(
        (shadow.area() - (1600.0 - hole)).abs() < 1e-3,
        "{}",
        shadow.area()
    );
    assert!(!shadow.contains(dvec2(0.0, 0.0)));
    assert!(shadow.contains(dvec2(10.0, 10.0)));

    // Kept wholly inside, a positive offset moves it in, as Fusion's does.
    b.containment = Containment::Inside;
    b.margin = 1.0;
    let inside = boundary(&body, &b, &s, 1.0).unwrap().unwrap();
    assert!(!inside.contains(dvec2(18.5, 0.0)));
    assert!(inside.contains(dvec2(17.5, 0.0)));
    // Reaching in from outside, it moves out.
    b.containment = Containment::Outside;
    let outside = boundary(&body, &b, &s, 1.0).unwrap().unwrap();
    assert!(outside.contains(dvec2(21.9, 0.0)));
    assert!(!outside.contains(dvec2(22.1, 0.0)));
}

/// The stock's boundary is none; a selection with nothing picked is refused,
/// not taken for the stock's.
#[test]
fn a_selection_needs_picks() {
    let body = plate();
    let mut b = MachiningBoundary::default();
    let mut s = Selection::default();
    assert_eq!(boundary(&body, &b, &s, 1.0), Ok(None));
    b.kind = Boundary::Selection;
    assert_eq!(
        boundary(&body, &b, &s, 1.0),
        Err(FeatureError::NothingPicked)
    );
    // Picks with the stock's boundary chosen are kept, and not used.
    s.faces.push(face_at(&body, [15.0, 15.0, 5.0]));
    b.kind = Boundary::Stock;
    assert_eq!(boundary(&body, &b, &s, 1.0), Ok(None));
}

/// The step's plateau, seen from above, is a 10 by 20 rectangle; the tool's
/// centre may cover it, keep a radius inside it, or reach a radius past it.
#[test]
fn a_picked_face_bounds_by_its_outline_and_the_containment() {
    let body = Body::derived(synthetic::step(5.0, 10.0));
    let (mut b, mut s) = selection();
    s.faces.push(face_at(&body, [-5.0, 0.0, 5.0]));
    let centre = boundary(&body, &b, &s, 1.0).unwrap().unwrap();
    assert!((centre.area() - 200.0).abs() < 0.01, "{}", centre.area());
    assert!(centre.contains(dvec2(-0.1, 9.9)));
    assert!(!centre.contains(dvec2(0.1, 0.0)));

    b.containment = Containment::Inside;
    let inside = boundary(&body, &b, &s, 1.0).unwrap().unwrap();
    // Shrinking errs inward by the arc tolerance, half the job's 0.01, a side.
    let err = 0.005 * 2.0 * (8.0 + 18.0) * 2.0;
    assert!(
        (inside.area() - 8.0 * 18.0).abs() < err,
        "{}",
        inside.area()
    );
    assert!(!inside.contains(dvec2(-0.5, 0.0)));

    b.containment = Containment::Outside;
    let outside = boundary(&body, &b, &s, 1.0).unwrap().unwrap();
    // Growing errs outward by the arc tolerance, so the radius is 1.005.
    let r: f64 = 1.005;
    let rounded = (10.0 + 2.0 * r) * (20.0 + 2.0 * r) - (4.0 - std::f64::consts::PI) * r * r;
    assert!(
        (outside.area() - rounded).abs() < 0.05,
        "{}",
        outside.area()
    );
    assert!(outside.contains(dvec2(0.9, 0.0)));

    // The margin moves the boundary out on top of the containment.
    b.containment = Containment::Center;
    b.margin = 1.0;
    let margin = boundary(&body, &b, &s, 1.0).unwrap().unwrap();
    assert!((margin.area() - outside.area()).abs() < 0.1);

    // A tool too big to fit inside.
    b.margin = 0.0;
    b.containment = Containment::Inside;
    assert_eq!(
        boundary(&body, &b, &s, 6.0),
        Err(FeatureError::NoRoom { diameter: 12.0 })
    );
}

/// The step's wall is seen edge-on from above: there is nothing to machine.
#[test]
fn a_wall_alone_has_no_area() {
    let body = Body::derived(synthetic::step(5.0, 10.0));
    let (b, mut s) = selection();
    s.faces.push(face_at(&body, [0.0, 0.0, 2.5]));
    assert_eq!(boundary(&body, &b, &s, 1.0), Err(FeatureError::NoArea));
}

/// The four edges round the plate's top and the rim of its hole: a square with
/// a round hole. Three of the four sides alone do not close.
#[test]
fn picked_edges_close_into_loops_with_holes() {
    let body = plate();
    let (b, mut s) = selection();
    for at in [
        [0.0, -20.0, 5.0],
        [20.0, 0.0, 5.0],
        [0.0, 20.0, 5.0],
        [-20.0, 0.0, 5.0],
        [4.0, 0.0, 5.0],
    ] {
        s.edges.push(edge_at(&body, at));
    }
    let region = boundary(&body, &b, &s, 1.0).unwrap().unwrap();
    let hole = 32.0 / 2.0 * 16.0 * (std::f64::consts::TAU / 32.0).sin();
    assert!(
        (region.area() - (1600.0 - hole)).abs() < 0.01,
        "{}",
        region.area()
    );
    assert!(!region.contains(dvec2(0.0, 0.0)));
    assert!(region.contains(dvec2(10.0, 10.0)));

    s.edges.remove(3);
    let err = boundary(&body, &b, &s, 1.0).unwrap_err();
    assert!(matches!(err, FeatureError::OpenLoop { .. }), "{err:?}");
    assert!(err.to_string().contains("do not close"), "{err}");
}

/// A pick that is gone stops the boundary, rather than dropping out of it.
#[test]
fn a_gone_pick_is_an_error_not_a_smaller_boundary() {
    let body = plate();
    let (b, mut s) = selection();
    s.faces.push(face_at(&body, [10.0, 10.0, 5.0]));
    s.faces.push(FeatureRef {
        model: "p".into(),
        index: 0,
        at: [50.0, 50.0, 50.0],
    });
    assert!(matches!(
        boundary(&body, &b, &s, 1.0),
        Err(FeatureError::Gone { .. })
    ));
    s.faces[1].model = "elsewhere".into();
    assert!(matches!(
        boundary(&body, &b, &s, 1.0),
        Err(FeatureError::NotLoaded(_))
    ));
}

/// Where the plate's three copies stand: where it is, a quarter turn round and
/// moved, and turned by a free angle and moved, each well clear of the others.
fn three_stands() -> [Stand; 3] {
    let stand = |degrees: f64, by: DVec3| Stand {
        turn: camcraft_geom::turn::Turn::from_degrees(degrees).unwrap(),
        offset: by,
    };
    [
        Stand::NONE,
        stand(90.0, dvec3(100.0, 0.0, 0.0)),
        stand(30.0, dvec3(0.0, 100.0, 0.0)),
    ]
}

/// The plate laid out standing at each of `stands`.
fn laid_out_standing(body: &Body, stands: &[Stand]) -> Prepared {
    use crate::job::{Job, ModelDef, Setup, StockDef, StockSpec};
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
    let mut setup = Setup::new("top");
    setup.models = vec!["p".into()];
    setup.stocks = vec!["s".into()];
    j.setups.push(setup);
    crate::setup::prepare(&j, 0, &Bodies::standing([("p", body, stands)])).unwrap()
}

/// A face and an edge picked once are found on every copy, each with its
/// copy, and each where its copy stands: the body's own taken through the
/// copy's stand.
#[test]
fn a_pick_is_found_on_every_copy_where_it_stands() {
    let body = plate();
    let stands = three_stands();
    let prepared = laid_out_standing(&body, &stands);
    let bodies = Bodies::standing([("p", &body, &stands[..])]);
    let s = Selection {
        faces: vec![face_at(&body, [10.0, 10.0, 5.0])],
        edges: vec![edge_at(&body, [4.0, 0.0, 5.0])],
    };
    let found = Picked::find(&s, &prepared, &bodies, 0.01).unwrap();
    let frame = &prepared.frame;
    let face = s.faces[0].index;
    let triangles: Vec<[DVec3; 3]> = body
        .topology()
        .face(face)
        .unwrap()
        .triangles
        .iter()
        .map(|&t| body.mesh().triangle(t as usize).unwrap())
        .collect();
    assert_eq!(found.faces.len(), 3);
    for (copy, (f, stand)) in found.faces.iter().zip(&stands).enumerate() {
        assert_eq!((f.model.as_str(), f.copy, f.face), ("p", copy, face));
        let want: Vec<[DVec3; 3]> = triangles
            .iter()
            .map(|t| t.map(|p| frame.to_setup_as(stand, p)))
            .collect();
        assert_eq!(f.triangles, want, "copy {copy}");
    }
    let edge = s.edges[0].index;
    let points: Vec<DVec3> = body.topology().edge_points(body.mesh(), edge).collect();
    assert_eq!(found.chains.len(), 3);
    for (copy, (c, stand)) in found.chains.iter().zip(&stands).enumerate() {
        assert_eq!((c.model.as_str(), c.copy, c.edge), ("p", copy, edge));
        let want: Vec<DVec3> = points
            .iter()
            .map(|&p| frame.to_setup_as(stand, p))
            .collect();
        assert_eq!(c.points, want, "copy {copy}");
    }
    // The quarter-turned copy's rim, about (100, 0) now, exactly as far out.
    for p in &found.chains[1].points {
        let r = (p.truncate() - dvec2(100.0, 0.0)).length();
        assert!((r - 4.0).abs() < 1e-9, "{p}");
    }
    // Picked twice, they are still found once on each copy.
    let twice = Selection {
        faces: vec![s.faces[0].clone(), s.faces[0].clone()],
        edges: vec![s.edges[0].clone(), s.edges[0].clone()],
    };
    let again = Picked::find(&twice, &prepared, &bodies, 0.01).unwrap();
    assert_eq!((again.faces.len(), again.chains.len()), (3, 3));
    // A selection of the top face is that face on every copy, seen from above.
    let area = boundary_shape(Boundary::Selection, &s, &prepared, &bodies, 0.01)
        .unwrap()
        .unwrap();
    for (stand, at) in stands.iter().zip([dvec2(10.0, 10.0); 3]) {
        let there = frame.to_setup_as(stand, at.extend(5.0)).truncate();
        assert!(area.contains(there), "{there}");
    }
}

/// Edges join into loops only on one copy: two copies of a triangle, their
/// corners numbered alike, are two loops, each on its own copy, never one
/// running between them.
#[test]
fn loops_do_not_join_across_copies() {
    let side = |copy: usize, ends: [u32; 2], a: DVec3, b: DVec3| Chain {
        model: "p".into(),
        copy,
        edge: ends[0],
        points: vec![a, b],
        closed: false,
        ends: Some(ends),
    };
    let corners = |by: f64| {
        [
            dvec3(by, 0.0, 0.0),
            dvec3(by + 10.0, 0.0, 0.0),
            dvec3(by, 10.0, 0.0),
        ]
    };
    let mut chains = Vec::new();
    for (copy, [a, b, c]) in [(0, corners(0.0)), (1, corners(50.0))] {
        chains.push(side(copy, [1, 2], a, b));
        chains.push(side(copy, [2, 3], b, c));
        chains.push(side(copy, [3, 1], c, a));
    }
    // Interleaved, so the next chain at a corner is always the other copy's
    // first.
    chains.sort_by_key(|c| c.edge);
    let rings = loops(&chains).unwrap();
    assert_eq!(rings.len(), 2);
    for (ring, by) in rings.iter().zip([0.0, 50.0]) {
        assert_eq!(ring.len(), 3);
        assert!(
            ring.iter().all(|p| p.x >= by && p.x <= by + 10.0),
            "{ring:?}"
        );
    }
    // A copy whose loop is not all picked is open, whatever the other has.
    chains.retain(|c| !(c.copy == 1 && c.edge == 3));
    assert!(matches!(loops(&chains), Err(FeatureError::OpenLoop { .. })));
}

/// A model with no copy standing has nothing picked on it to find, though its
/// picks are still looked for on its body.
#[test]
fn a_model_with_no_copy_has_no_picks() {
    let body = plate();
    let prepared = laid_out_standing(&body, &[Stand::NONE]);
    let bodies = Bodies::standing([("p", &body, &[][..])]);
    let s = Selection {
        faces: vec![face_at(&body, [10.0, 10.0, 5.0])],
        edges: vec![edge_at(&body, [4.0, 0.0, 5.0])],
    };
    let found = Picked::find(&s, &prepared, &bodies, 0.01).unwrap();
    assert!(found.faces.is_empty() && found.chains.is_empty());
    let gone = Selection {
        faces: vec![FeatureRef {
            at: [7.0, 7.0, 7.0],
            ..s.faces[0].clone()
        }],
        ..Selection::default()
    };
    assert!(Picked::find(&gone, &prepared, &bodies, 0.01).is_err());
}
