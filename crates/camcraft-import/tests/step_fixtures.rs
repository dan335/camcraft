//! Every committed STEP fixture, through the real kernel.
//!
//! Each must arrive whole, in the right unit, at the right size, and pass the
//! checks against its own file. Sizes are the extent of the file's own B-rep
//! vertices, measured independently of any kernel, so they are ground truth rather
//! than a recording of whatever OpenCascade happened to say.
//!
//! Parts that cannot be committed are swept from `CAMCRAFT_FIXTURES` when it is
//! set. See `fixtures/step/README.md`.

#![allow(clippy::unwrap_used, clippy::panic, clippy::float_cmp)]

use std::path::{Path, PathBuf};

use camcraft_import::step::{FileCheck, load_step};

/// Tessellation tolerance, in millimetres. The spike's figure.
const TOLERANCE: f64 = 0.01;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/step")
        .join(name)
}

fn check(name: &str, unit: &str, faces: u64, size: [f64; 3]) {
    let (mesh, report) =
        load_step(fixture(name), TOLERANCE).unwrap_or_else(|e| panic!("{name}: {e}"));
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
                scope.spawn(move || load_step(fixture(name), 0.05).map(|(m, _)| (name, m.len())))
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
