//! ogeom against OpenCascade, the kernel camcraft used before it, on every
//! committed STEP fixture and every part in `CAMCRAFT_FIXTURES`. Only with the
//! `opencascade` feature, which builds OpenCascade:
//!
//!   cargo test -p camcraft-import --features opencascade --test opencascade
//!
//! Both kernels' meshes face the same checks against the file. Beyond those, they
//! must agree on the faces and on the part's extent, to within the tolerance each
//! was asked for and the tolerance OpenCascade's own meshes are known to miss it by
//! (3.4 times, on the bowl).

#![cfg(feature = "opencascade")]
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};

use camcraft_import::step::{FileCheck, load_step_with, ogeom, opencascade};

/// Tessellation tolerance, in millimetres: the one ogeom meshes every fixture at.
const TOLERANCE: f64 = 0.0125;

fn parts() -> Vec<PathBuf> {
    let mut dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/step")];
    dirs.extend(std::env::var_os("CAMCRAFT_FIXTURES").map(PathBuf::from));
    let mut parts: Vec<PathBuf> = dirs
        .iter()
        .flat_map(|d| std::fs::read_dir(d).unwrap())
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("step") || e.eq_ignore_ascii_case("stp"))
        })
        .collect();
    parts.sort();
    parts
}

#[test]
fn ogeom_agrees_with_opencascade() {
    let parts = parts();
    assert!(!parts.is_empty());
    for part in parts {
        let name = part.display();
        let (o, o_report) = load_step_with(&part, TOLERANCE, "ogeom", |p, _| ogeom(p, TOLERANCE))
            .unwrap_or_else(|e| panic!("{e}"));
        let (c, c_report) = load_step_with(&part, TOLERANCE, "OpenCascade", |p, _| {
            opencascade(p, TOLERANCE)
        })
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(o_report.check, FileCheck::Passed, "{name}: ogeom");
        assert_eq!(c_report.check, FileCheck::Passed, "{name}: OpenCascade");
        assert_eq!(
            o.topology().faces().len(),
            c.topology().faces().len(),
            "{name}: faces"
        );
        let (a, b) = (o.mesh().bounds().unwrap(), c.mesh().bounds().unwrap());
        let apart = (a.min - b.min)
            .abs()
            .max((a.max - b.max).abs())
            .max_element();
        assert!(
            apart <= (1.0 + 3.4) * TOLERANCE,
            "{name}: the kernels' extents differ by {apart} mm"
        );
    }
}
