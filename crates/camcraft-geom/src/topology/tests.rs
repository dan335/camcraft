#![allow(clippy::unwrap_used)]

use super::*;
use crate::synthetic;
use glam::dvec3;
use std::collections::{BTreeMap, BTreeSet};

/// The mesh with vertices within a nanometre joined and triangles that lose a
/// corner dropped, as the importers leave it. A UV sphere's seam and poles are
/// only nearly coincident, by a rounding error in `sin`.
fn weld(mesh: &TriMesh) -> TriMesh {
    let mut index: BTreeMap<[i64; 3], u32> = BTreeMap::new();
    let mut vertices = Vec::new();
    #[allow(clippy::cast_possible_truncation)]
    let mut id = |p: DVec3| {
        let key = [p.x, p.y, p.z].map(|c| (c * 1e9).round() as i64);
        *index.entry(key).or_insert_with(|| {
            vertices.push(p);
            u32::try_from(vertices.len() - 1).unwrap()
        })
    };
    let mut triangles = Vec::new();
    for i in 0..mesh.len() {
        let t = mesh.triangle(i).unwrap().map(&mut id);
        if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
            triangles.push(t);
        }
    }
    TriMesh::new(vertices, triangles).unwrap()
}

fn planes(t: &Topology) -> Vec<(DVec3, f64)> {
    t.faces()
        .iter()
        .filter_map(|f| match f.kind {
            FaceKind::Plane { normal, offset } => Some((normal, offset)),
            FaceKind::Curved => None,
        })
        .collect()
}

fn has_plane(t: &Topology, normal: DVec3, offset: f64) -> bool {
    planes(t)
        .iter()
        .any(|(n, o)| (*n - normal).length() < 1e-9 && (o - offset).abs() < 1e-9)
}

/// Each face as the set of its triangles' corners, so faces compare across
/// renumbering and reordering.
fn face_sets(body: &Body) -> BTreeSet<Vec<[u32; 3]>> {
    body.topology()
        .faces()
        .iter()
        .map(|f| {
            let mut tris: Vec<[u32; 3]> = f
                .triangles
                .iter()
                .map(|&t| {
                    let mut c = *body.mesh().triangles().get(t as usize).unwrap();
                    c.sort_unstable();
                    c
                })
                .collect();
            tris.sort_unstable();
            tris
        })
        .collect()
}

#[test]
fn a_box_has_six_faces_twelve_edges_and_eight_corners() {
    let body = Body::derived(synthetic::cuboid(
        dvec3(-1.0, -2.0, -3.0),
        dvec3(4.0, 5.0, 6.0),
    ));
    let t = body.topology();
    assert_eq!(t.source(), FaceSource::Derived);
    assert_eq!(t.faces().len(), 6);
    assert_eq!(planes(t).len(), 6);
    assert!(has_plane(t, dvec3(0.0, 0.0, 1.0), 6.0));
    assert!(has_plane(t, dvec3(0.0, 0.0, -1.0), 3.0));
    assert!(has_plane(t, dvec3(-1.0, 0.0, 0.0), 1.0));
    assert_eq!(t.edges().len(), 12);
    for e in t.edges() {
        assert_eq!(e.kind, EdgeKind::Line);
        assert_eq!(e.faces.len(), 2);
        assert!(!e.closed);
        assert_eq!(e.points.len(), 2);
    }
    assert_eq!(t.vertices().len(), 8);
    for v in t.vertices() {
        assert_eq!(v.edges.len(), 3);
    }
    for f in t.faces() {
        assert_eq!(f.edges.len(), 4);
        assert_eq!(f.triangles.len(), 2);
    }
    // Vertices come in order of position.
    assert_eq!(
        t.vertices().first().unwrap().position,
        dvec3(-1.0, -2.0, -3.0)
    );
    assert_eq!(t.vertices().last().unwrap().position, dvec3(4.0, 5.0, 6.0));
}

#[test]
fn faces_named_by_the_file_are_kept_as_named() {
    let mesh = synthetic::cuboid(DVec3::ZERO, DVec3::ONE);
    // Two triangles a side, labelled backwards to show the numbering is the file's.
    let labels: Vec<u32> = (0..12).map(|t| 5 - t / 2).collect();
    let body = Body::with_faces(mesh.clone(), labels, 6).unwrap();
    let t = body.topology();
    assert_eq!(t.source(), FaceSource::File);
    assert_eq!(t.face_of(0), Some(5));
    assert_eq!(t.face(5).unwrap().triangles, vec![0, 1]);
    assert_eq!(t.edges().len(), 12);
    // A face the file names but no triangle carries is kept, empty.
    let labels: Vec<u32> = (0..12).map(|t| t / 2).collect();
    let body = Body::with_faces(mesh.clone(), labels, 7).unwrap();
    assert_eq!(body.topology().faces().len(), 7);
    assert!(body.topology().face(6).unwrap().triangles.is_empty());
    assert_eq!(body.topology().face(6).unwrap().bounds, None);

    assert_eq!(
        Body::with_faces(mesh.clone(), vec![0; 11], 1),
        Err(TopologyError::LabelCount {
            labels: 11,
            triangles: 12
        })
    );
    assert_eq!(
        Body::with_faces(mesh, vec![0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0], 3),
        Err(TopologyError::LabelRange {
            triangle: 2,
            face: 3,
            count: 3
        })
    );
}

#[test]
fn an_open_step_keeps_the_corners_of_its_outline() {
    let body = Body::derived(synthetic::step(5.0, 10.0));
    let t = body.topology();
    assert_eq!(t.faces().len(), 3);
    assert!(has_plane(t, dvec3(0.0, 0.0, 1.0), 5.0));
    assert!(has_plane(t, dvec3(0.0, 0.0, 1.0), 0.0));
    assert_eq!(planes(t).len(), 3);
    // Two edges where the wall meets the levels, three along each level's open
    // outline, and one along each open end of the wall.
    assert_eq!(t.edges().len(), 10);
    assert_eq!(t.edges().iter().filter(|e| e.faces.len() == 2).count(), 2);
    assert!(t.edges().iter().all(|e| e.kind == EdgeKind::Line));
    assert_eq!(t.vertices().len(), 8);
}

#[test]
fn a_hole_has_a_curved_wall_and_round_rims() {
    let body = Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 32));
    let t = body.topology();
    assert_eq!(t.faces().len(), 7);
    assert_eq!(planes(t).len(), 6);
    assert!(has_plane(t, dvec3(0.0, 0.0, 1.0), 5.0));
    assert!(has_plane(t, dvec3(0.0, 0.0, -1.0), 0.0));
    assert!(has_plane(t, dvec3(1.0, 0.0, 0.0), 20.0));
    let wall = t
        .faces()
        .iter()
        .find(|f| f.kind == FaceKind::Curved)
        .unwrap();
    assert_eq!(wall.triangles.len(), 64);
    assert_eq!(wall.edges.len(), 2);

    assert_eq!(t.edges().len(), 14);
    let mut rims: Vec<Circle> = t
        .edges()
        .iter()
        .filter_map(|e| match e.kind {
            EdgeKind::Circle(c) => Some(c),
            _ => None,
        })
        .collect();
    rims.sort_by(|a, b| a.center.z.total_cmp(&b.center.z));
    assert_eq!(rims.len(), 2);
    for (c, z) in rims.iter().zip([0.0, 5.0]) {
        assert!(c.full);
        assert!((c.radius - 4.0).abs() < 1e-9, "{}", c.radius);
        assert!(c.center.distance(dvec3(0.0, 0.0, z)) < 1e-9, "{}", c.center);
        assert!((c.normal - dvec3(0.0, 0.0, 1.0)).length() < 1e-12);
    }
    let rim = t.edges().iter().find(|e| e.closed).unwrap();
    assert_eq!(rim.points.len(), 32);
    assert_eq!(rim.ends, None);
    assert_eq!(
        t.edges()
            .iter()
            .filter(|e| e.kind == EdgeKind::Line)
            .count(),
        12
    );
    assert_eq!(t.vertices().len(), 8);
}

#[test]
fn a_sphere_is_one_curved_face_with_no_edges() {
    let body = Body::derived(weld(&synthetic::uv_sphere(5.0, 16, 24)));
    let t = body.topology();
    assert_eq!(t.faces().len(), 1);
    assert_eq!(t.faces().first().unwrap().kind, FaceKind::Curved);
    assert!(t.edges().is_empty());
    assert!(t.vertices().is_empty());
}

/// A floor that runs into its wall through a fillet: the fillet's facets are no
/// larger than each other, so they are one curved face, and the floor and wall,
/// which dwarf them, stay planes.
#[test]
fn a_fillet_between_planes_is_its_own_curved_face() {
    let mut profile = vec![dvec3(-20.0, 0.0, 0.0), dvec3(0.0, 0.0, 0.0)];
    for i in 1..=8 {
        let a = std::f64::consts::FRAC_PI_2 * f64::from(i) / 8.0;
        profile.push(dvec3(5.0 * a.sin(), 0.0, 5.0 - 5.0 * a.cos()));
    }
    profile.push(dvec3(5.0, 0.0, 25.0));
    let mut vertices = profile.clone();
    vertices.extend(profile.iter().map(|p| *p + dvec3(0.0, 10.0, 0.0)));
    let n = u32::try_from(profile.len()).unwrap();
    let triangles = (0..n - 1)
        .flat_map(|i| [[i, i + n + 1, i + 1], [i, i + n, i + n + 1]])
        .collect();
    let body = Body::derived(TriMesh::new(vertices, triangles).unwrap());
    let t = body.topology();
    assert_eq!(t.faces().len(), 3);
    assert!(has_plane(t, dvec3(0.0, 0.0, 1.0), 0.0) || has_plane(t, dvec3(0.0, 0.0, -1.0), 0.0));
    assert_eq!(planes(t).len(), 2);
    let fillet = t
        .faces()
        .iter()
        .find(|f| f.kind == FaceKind::Curved)
        .unwrap();
    assert_eq!(fillet.triangles.len(), 16);
    // The edges on either side of the fillet are straight, and the arcs at its
    // open ends are arcs of its radius.
    let arcs: Vec<Circle> = t
        .edges()
        .iter()
        .filter_map(|e| match e.kind {
            EdgeKind::Circle(c) => Some(c),
            _ => None,
        })
        .collect();
    assert_eq!(arcs.len(), 2);
    assert!(
        arcs.iter()
            .all(|c| (c.radius - 5.0).abs() < 1e-9 && !c.full)
    );
}

#[test]
fn winding_and_triangle_order_do_not_change_the_faces() {
    let mesh = synthetic::plate_with_hole(15.0, 3.0, 2.5, 24);
    let body = Body::derived(mesh.clone());

    let flipped: Vec<[u32; 3]> = mesh
        .triangles()
        .iter()
        .enumerate()
        .map(|(i, &[a, b, c])| if i % 2 == 0 { [a, c, b] } else { [a, b, c] })
        .collect();
    let flipped = Body::derived(TriMesh::new(mesh.vertices().to_vec(), flipped).unwrap());
    assert_eq!(face_sets(&flipped), face_sets(&body));
    assert_eq!(
        flipped.topology().edges().len(),
        body.topology().edges().len()
    );

    let mut reordered = mesh.triangles().to_vec();
    reordered.reverse();
    let reordered = Body::derived(TriMesh::new(mesh.vertices().to_vec(), reordered).unwrap());
    assert_eq!(face_sets(&reordered), face_sets(&body));
    // Edges run through the same points, and vertices sit in the same places;
    // only the numbers faces and edges get may differ.
    let points = |b: &Body| -> Vec<Vec<u32>> {
        b.topology()
            .edges()
            .iter()
            .map(|e| e.points.clone())
            .collect()
    };
    let mut a = points(&reordered);
    let mut b = points(&body);
    a.sort();
    b.sort();
    assert_eq!(a, b);
    let corners =
        |b: &Body| -> Vec<DVec3> { b.topology().vertices().iter().map(|v| v.position).collect() };
    assert_eq!(corners(&reordered), corners(&body));
}

#[test]
fn the_same_mesh_gives_the_same_topology() {
    let mesh = synthetic::plate_with_hole(10.0, 2.0, 3.0, 40);
    assert_eq!(Body::derived(mesh.clone()), Body::derived(mesh));
}

#[test]
fn edges_answer_where_their_points_are() {
    let body = Body::derived(synthetic::cuboid(DVec3::ZERO, dvec3(10.0, 4.0, 2.0)));
    let (mesh, t) = (body.mesh(), body.topology());
    // The edge along X at the front bottom.
    let e = u32::try_from(
        t.edges()
            .iter()
            .position(|e| {
                let p: Vec<DVec3> = e
                    .points
                    .iter()
                    .map(|&v| *mesh.vertices().get(v as usize).unwrap())
                    .collect();
                p == [DVec3::ZERO, dvec3(10.0, 0.0, 0.0)]
            })
            .unwrap(),
    )
    .unwrap();
    assert!((t.edge(e).unwrap().length - 10.0).abs() < 1e-12);
    assert_eq!(t.edge_middle(mesh, e), Some(dvec3(5.0, 0.0, 0.0)));
    let (q, d) = t.closest_on_edge(mesh, e, dvec3(3.0, -1.0, 0.0)).unwrap();
    assert_eq!(q, dvec3(3.0, 0.0, 0.0));
    assert!((d - 1.0).abs() < 1e-12);
    assert_eq!(t.edge_points(mesh, e).count(), 2);
    assert_eq!(t.edge_points(mesh, 999).count(), 0);

    let top = t
        .faces()
        .iter()
        .position(|f| matches!(f.kind, FaceKind::Plane { normal, .. } if normal.z > 0.5))
        .unwrap();
    let top = u32::try_from(top).unwrap();
    let d = t.distance_to_face(mesh, top, dvec3(5.0, 2.0, 7.0)).unwrap();
    assert!((d - 5.0).abs() < 1e-12);
    assert_eq!(t.distance_to_face(mesh, 99, DVec3::ZERO), None);

    // A hole's rim: its middle is on the far side of the circle from its start.
    let body = Body::derived(synthetic::plate_with_hole(10.0, 2.0, 3.0, 16));
    let (mesh, t) = (body.mesh(), body.topology());
    let rim = u32::try_from(t.edges().iter().position(|e| e.closed).unwrap()).unwrap();
    let start = t.edge_points(mesh, rim).next().unwrap();
    let middle = t.edge_middle(mesh, rim).unwrap();
    assert!(
        (start.distance(middle) - 6.0).abs() < 1e-9,
        "{start} {middle}"
    );
}

#[test]
fn nothing_at_all_has_no_topology() {
    let body = Body::derived(TriMesh::default());
    assert!(body.topology().faces().is_empty());
    assert!(body.topology().edges().is_empty());
    assert_eq!(body.topology().face_of(0), None);
}

/// A level edge goes round with the level edges joined to it: the bottom of a
/// box, its four bottom edges and not the walls' upright ones; the plate's hole,
/// its rim alone. An upright edge is in no level loop.
#[test]
fn a_level_edge_goes_round_at_its_height() {
    let body = Body::derived(synthetic::cuboid(
        dvec3(0.0, 0.0, 0.0),
        dvec3(4.0, 5.0, 6.0),
    ));
    let t = body.topology();
    let points = |e: u32| t.edge_points(body.mesh(), e).collect::<Vec<_>>();
    let level_at = |e: u32, z: f64| points(e).iter().all(|p| (p.z - z).abs() < 1e-9);
    let bottom: Vec<u32> = (0..12).filter(|&e| level_at(e, 0.0)).collect();
    assert_eq!(bottom.len(), 4);
    for &e in &bottom {
        let mut round = t.level_loop(e, points, 1e-6).unwrap();
        assert_eq!(round.first(), Some(&e));
        round.sort_unstable();
        assert_eq!(round, bottom);
    }
    let upright = (0..12)
        .find(|&e| !level_at(e, 0.0) && !level_at(e, 6.0))
        .unwrap();
    assert_eq!(t.level_loop(upright, points, 1e-6), None);

    let plate = Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 32));
    let t = plate.topology();
    let (rim, _) = t
        .edges()
        .iter()
        .enumerate()
        .find(|(_, e)| e.closed)
        .unwrap();
    let rim = u32::try_from(rim).unwrap();
    let points = |e: u32| t.edge_points(plate.mesh(), e).collect::<Vec<_>>();
    assert_eq!(t.level_loop(rim, points, 1e-6), Some(vec![rim]));
}
