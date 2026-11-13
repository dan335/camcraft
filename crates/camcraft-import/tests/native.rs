//! Shapes that are ogeom's already, through `camcraft_import::native`.
//!
//! A solid held in memory, and the same solid written in ogeom's native format
//! and read back, must come out as the body its STEP file does: the same faces,
//! triangles and size.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use camcraft_import::native::{NativeError, load_native, load_shapes};
use camcraft_import::step::load_step;
use ogeom_core::Tolerances;
use ogeom_io::native::WriteOptions;

/// Tessellation tolerance, in millimetres.
const TOLERANCE: f64 = 0.01;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/step")
        .join(name)
}

/// `ops_block.step`'s one solid, held in memory and written out and read back,
/// is the body the file itself loads as: 21 faces, the same triangles and
/// vertices, 100 x 70 x 20 mm.
#[test]
fn a_solid_in_memory_loads_as_its_step_file_does() {
    let path = fixture("ops_block.step");
    let (from_file, file) = load_step(&path, TOLERANCE).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    let import = ogeom_io::read_step(&text, Tolerances::millimetres()).unwrap();
    let model = import.document.model();
    assert_eq!(import.solids.len(), 1);

    let (held, held_report) = load_shapes(model, &import.solids, TOLERANCE).unwrap();
    let written = ogeom_io::native::write(model, &import.solids, WriteOptions::default()).unwrap();
    let (read, read_report) = load_native(&written, TOLERANCE).unwrap();

    for (how, body, report) in [
        ("held", &held, &held_report),
        ("written and read", &read, &read_report),
    ] {
        assert_eq!(report.faces, 21, "{how}");
        assert_eq!(report.faces, file.faces, "{how}");
        assert_eq!(report.triangles, file.triangles, "{how}");
        assert_eq!(report.degenerate, file.degenerate, "{how}");
        assert_eq!(report.vertices, file.vertices, "{how}");
        assert_eq!(body.faces().len(), from_file.faces().len(), "{how}");
        assert_eq!(body.triangle_faces(), from_file.triangle_faces(), "{how}");
        let got = body.mesh().bounds().unwrap().extent();
        for (g, want) in got.to_array().iter().zip([100.0, 70.0, 20.0]) {
            assert!((g - want).abs() < 1e-6, "{how}: {g} mm, expected {want}");
        }
    }
}

/// What cannot be loaded says why: a tolerance that is not one, text that is
/// not a shape, and no shapes at all.
#[test]
fn what_cannot_be_loaded_is_refused() {
    assert!(matches!(
        load_native("", 0.0),
        Err(NativeError::BadTolerance(t)) if t == 0.0
    ));
    assert!(matches!(
        load_native("not a shape", TOLERANCE),
        Err(NativeError::Read(_))
    ));
    let model = ogeom_topo::Model::default();
    assert!(matches!(
        load_shapes(&model, &[], f64::NAN),
        Err(NativeError::BadTolerance(_))
    ));
    assert!(matches!(
        load_shapes(&model, &[], TOLERANCE),
        Err(NativeError::Empty)
    ));
}
