//! Every committed STEP fixture, through the real kernel.
//!
//! Each must arrive whole, in the right unit, at the right size, and pass the
//! checks against its own file. Sizes are the extent of the file's own B-rep
//! vertices, measured independently of any kernel, so they are ground truth rather
//! than a recording of whatever OpenCascade happened to say.
//!
//! Parts that cannot be committed are swept from `CAMCRAFT_FIXTURES` when it is
//! set. See `fixtures/step/README.md`.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use camcraft_geom::topology::{Body, EdgeKind, FaceKind, FaceSource};
use camcraft_import::step::{FileCheck, KernelMesh, StepError, load_step, load_step_with};
use glam::{DVec2, dvec3};
use step_check::Problem;

/// Tessellation tolerance, in millimetres. The spike's figure.
const TOLERANCE: f64 = 0.01;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/step")
        .join(name)
}

fn check(name: &str, unit: &str, faces: u64, size: [f64; 3]) {
    let (body, report) =
        load_step(fixture(name), TOLERANCE).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mesh = body.mesh();
    assert_eq!(report.unit.name, unit, "{name}: unit");
    assert_eq!(report.faces, faces, "{name}: faces");
    assert_eq!(
        report.check,
        FileCheck::Passed,
        "{name}: checks against the file"
    );
    assert_eq!(report.degenerate, 0, "{name}: degenerate triangles");
    let got = mesh.bounds().unwrap().extent();
    for (axis, (g, want)) in ["X", "Y", "Z"].iter().zip(got.to_array().iter().zip(size)) {
        assert!(
            (g - want).abs() < 0.1,
            "{name}: {axis} extent {g:.2} mm, expected {want} mm"
        );
    }
    check_faces(name, &body, faces);
}

/// The faces are the file's: one per B-rep face, every one with triangles, every
/// triangle on one. And the part is a closed solid, so every edge runs between
/// exactly two faces. That last holds only if the kernel gives the two faces either
/// side of a B-rep edge the very same points along it, which the weld relies on.
fn check_faces(name: &str, body: &Body, faces: u64) {
    let t = body.topology();
    assert_eq!(t.source(), FaceSource::File, "{name}");
    assert_eq!(t.faces().len() as u64, faces, "{name}: topology faces");
    assert_eq!(t.triangle_faces().len(), body.mesh().len(), "{name}");
    for (i, f) in t.faces().iter().enumerate() {
        assert!(!f.triangles.is_empty(), "{name}: face {i} has no triangles");
    }
    for e in t.edges() {
        assert_eq!(
            e.faces.len(),
            2,
            "{name}: an edge runs along {:?}, not between two faces; the solid is open there",
            e.faces
        );
    }
}

#[test]
fn hinge_jig_planar_inch() {
    check("hinge_jig.step", "inch", 39, [25.4, 31.8, 33.3]);
}

#[test]
fn lego_cones_and_cylinders() {
    check("lego_4x2_01.step", "inch", 76, [100.6, 49.8, 38.1]);
}

#[test]
fn door_handle_guide() {
    check("door_handle_guide.step", "inch", 89, [57.1, 215.9, 33.0]);
}

#[test]
fn rail_guide_many_faces() {
    check("rail_guide.step", "inch", 134, [237.0, 40.6, 12.7]);
}

#[test]
fn sled_base_mixed_analytic_and_nurbs() {
    check("sled_base.step", "inch", 387, [237.0, 227.0, 76.2]);
}

/// In millimetres, so a hard-coded inch conversion fails as loudly as a missing one,
/// and revolved, so the extent check has to bound the far side of an axis.
#[test]
fn bowl_revolved_millimetre() {
    check("bowl_01.step", "millimetre", 5, [380.0, 380.0, 20.0]);
}

/// Private parts, when this machine has them. Each must load, with its checks
/// passed or their skipping explained; none may fail.
/// A cone that comes to a point with no vertex there, as Fusion writes one. The
/// size check once refused it, its box stopping short of the apex. Generated, so
/// exact: 20 mm across and 30 tall (step-conformance's `cone_point`).
///
/// Unlike the shop parts, a face pinching to a point may leave OpenCascade a
/// triangle with two corners at the apex, which the weld drops.
#[test]
fn a_cone_to_a_point() {
    let (body, report) = load_step(fixture("cone_point.step"), TOLERANCE).unwrap();
    assert_eq!(report.check, FileCheck::Passed);
    assert_eq!(report.faces, 2);
    let got = body.mesh().bounds().unwrap().extent();
    assert!(
        (got - glam::dvec3(20.0, 20.0, 30.0)).abs().max_element() < 0.1,
        "{got:?}"
    );
    check_faces("cone_point.step", &body, 2);
}

/// A cylinder face of the lego, sunk a millimetre into the part: still inside every
/// box the file gives, so only measuring it against the file's own surface sees it.
#[test]
fn a_face_wrong_inside_the_part_is_refused() {
    let result = load_step_with(fixture("lego_4x2_01.step"), TOLERANCE, "test", |path, _| {
        let shape = opencascade::primitives::Shape::read_step(path).map_err(|e| e.to_string())?;
        let mesh = shape
            .mesh_with_tolerance(TOLERANCE)
            .map_err(|e| e.to_string())?;
        let mut k = KernelMesh {
            vertices: mesh.vertices.iter().map(|v| dvec3(v.x, v.y, v.z)).collect(),
            indices: mesh.indices,
            face_triangles: mesh.face_triangles,
        };
        // OpenCascade gives every face its own vertices, so a face moves alone.
        let (lo, hi) = step_check::part21::bounds(&k.vertices).unwrap();
        let mut first = 0;
        for &n in &k.face_triangles.clone() {
            let corners: Vec<usize> = k.indices[first * 3..(first + n) * 3].to_vec();
            first += n;
            let points: Vec<_> = corners.iter().map(|&i| k.vertices[i]).collect();
            let (a, b) = step_check::part21::bounds(&points).unwrap();
            let axis = DVec2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
            let radii: Vec<f64> = points.iter().map(|p| p.truncate().distance(axis)).collect();
            let upright_cylinder = b.z - a.z > 1.0
                && radii
                    .iter()
                    .all(|r| (r - radii[0]).abs() < 1e-6 && *r > 2.0);
            let inside =
                a.x > lo.x + 2.0 && b.x < hi.x - 2.0 && a.y > lo.y + 2.0 && b.y < hi.y - 2.0;
            if upright_cylinder && inside {
                // Each vertex once, though triangles share it.
                let moved: std::collections::BTreeSet<usize> = corners.into_iter().collect();
                for i in moved {
                    let p = k.vertices[i];
                    let out = (p.truncate() - axis).normalize();
                    k.vertices[i] = p - (out * 1.0).extend(0.0);
                }
                return Ok(k);
            }
        }
        Err("no cylinder face inside the part".into())
    });
    assert!(
        matches!(
            result,
            Err(StepError::Check {
                source: Problem::OffSurface { by, .. },
                ..
            }) if (by - 1.0).abs() < 0.05
        ),
        "{result:?}"
    );
}

#[test]
fn private_fixtures_when_available() {
    let Some(dir) = std::env::var_os("CAMCRAFT_FIXTURES") else {
        return;
    };
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let is_step = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("step") || e.eq_ignore_ascii_case("stp"));
        if !is_step {
            continue;
        }
        seen += 1;
        if let Err(e) = load_step(&path, TOLERANCE) {
            panic!("{}: {e}", path.display());
        }
    }
    assert!(seen > 0, "CAMCRAFT_FIXTURES is set but holds no STEP files");
}

/// Many threads reading STEP at once all succeed and all agree. A guard on the
/// importer's kernel lock. It has not been seen to fail without the lock, so it
/// does not prove the lock necessary; it holds the behaviour the lock is there for.
#[test]
fn concurrent_reads_are_safe() {
    let results: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let name = if i % 2 == 0 {
                    "hinge_jig.step"
                } else {
                    "lego_4x2_01.step"
                };
                scope.spawn(move || {
                    load_step(fixture(name), 0.05).map(|(b, _)| (name, b.mesh().len()))
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut sizes = std::collections::BTreeMap::new();
    for r in results {
        let (name, triangles) = r.unwrap_or_else(|e| panic!("a concurrent read failed: {e}"));
        let first = *sizes.entry(name).or_insert(triangles);
        assert_eq!(
            first, triangles,
            "{name} read differently on another thread"
        );
    }
}

/// The hinge jig is all planes, and says so.
#[test]
fn a_planar_part_has_only_planes() {
    let (body, _) = load_step(fixture("hinge_jig.step"), TOLERANCE).unwrap();
    let t = body.topology();
    for (i, f) in t.faces().iter().enumerate() {
        assert!(
            matches!(f.kind, FaceKind::Plane { .. }),
            "hinge_jig face {i} is not a plane"
        );
    }
}

/// The rail guide's holes have round rims, and a rim's circle is what a work zero
/// snaps to.
#[test]
fn round_edges_are_circles() {
    let (body, _) = load_step(fixture("rail_guide.step"), TOLERANCE).unwrap();
    let t = body.topology();
    assert_eq!(t.faces().len(), 134);
    let full = t
        .edges()
        .iter()
        .filter(|e| matches!(e.kind, EdgeKind::Circle(c) if c.full))
        .count();
    assert!(full > 0, "no full circles among {} edges", t.edges().len());
}

/// A face keeps its number, and its plane, when the part is tessellated again at
/// another tolerance: that is what lets a picked face be found by its number.
#[test]
fn faces_keep_their_numbers_across_tolerances() {
    for name in ["hinge_jig.step", "rail_guide.step"] {
        let (fine, _) = load_step(fixture(name), 0.01).unwrap();
        let (coarse, _) = load_step(fixture(name), 0.05).unwrap();
        let (a, b) = (fine.topology().faces(), coarse.topology().faces());
        assert_eq!(a.len(), b.len(), "{name}");
        for (i, (x, y)) in a.iter().zip(b).enumerate() {
            match (x.kind, y.kind) {
                (
                    FaceKind::Plane { normal, offset },
                    FaceKind::Plane {
                        normal: n2,
                        offset: o2,
                    },
                ) => {
                    assert!((normal - n2).length() < 1e-6, "{name}: face {i} turned");
                    assert!((offset - o2).abs() < 1e-6, "{name}: face {i} moved");
                }
                (FaceKind::Curved, FaceKind::Curved) => {
                    let (p, q) = (x.bounds.unwrap(), y.bounds.unwrap());
                    assert!(
                        (p.center() - q.center()).length() < 0.1,
                        "{name}: curved face {i} moved"
                    );
                }
                other => panic!("{name}: face {i} changed kind: {other:?}"),
            }
        }
    }
}

/// Every level edge round the lego brick's chamfered bottom goes all the way round
/// it, whichever of the chamfer's strips and corners it runs along: the outline
/// is eight edges, four straight and four round the corners. A tube's rim is its
/// own loop.
#[test]
fn the_lego_bottom_edges_go_all_the_way_round() {
    let (body, _) = load_step(fixture("lego_4x2_01.step"), TOLERANCE).unwrap();
    let t = body.topology();
    let points = |e: u32| t.edge_points(body.mesh(), e).collect::<Vec<_>>();
    let mut seen = 0;
    for e in 0..u32::try_from(t.edges().len()).unwrap() {
        let p = points(e);
        let lo = p.iter().map(|p| p.z).fold(f64::MAX, f64::min);
        let hi = p.iter().map(|p| p.z).fold(f64::MIN, f64::max);
        if hi > 3.0 || hi - lo > 1e-6 {
            continue;
        }
        seen += 1;
        let round = t.level_loop(e, points, t.tolerance()).unwrap();
        let closed = t.edge(e).unwrap().closed;
        assert_eq!(round.len(), if closed { 1 } else { 8 }, "edge {e}");
        for &n in &round {
            assert!(
                points(n).iter().all(|q| (q.z - lo).abs() < 1e-6),
                "edge {e}"
            );
        }
    }
    // Two chamfer lines, outside and in, each at two heights, and six tube rims.
    assert_eq!(seen, 4 * 8 + 6);
}
