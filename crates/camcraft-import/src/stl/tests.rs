use super::*;
use std::io::Write;

/// A unit cube, written as ASCII STL, with every face split into two triangles.
fn cube_ascii(size: f32) -> String {
    let s = size;
    let quads: [[[f32; 3]; 4]; 6] = [
        [[0.0, 0.0, 0.0], [s, 0.0, 0.0], [s, s, 0.0], [0.0, s, 0.0]],
        [[0.0, 0.0, s], [s, 0.0, s], [s, s, s], [0.0, s, s]],
        [[0.0, 0.0, 0.0], [s, 0.0, 0.0], [s, 0.0, s], [0.0, 0.0, s]],
        [[0.0, s, 0.0], [s, s, 0.0], [s, s, s], [0.0, s, s]],
        [[0.0, 0.0, 0.0], [0.0, s, 0.0], [0.0, s, s], [0.0, 0.0, s]],
        [[s, 0.0, 0.0], [s, s, 0.0], [s, s, s], [s, 0.0, s]],
    ];
    let mut out = String::from("solid test\n");
    for q in quads {
        for tri in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
            out.push_str("facet normal 0 0 0\n  outer loop\n");
            for v in tri {
                out.push_str(&format!("    vertex {} {} {}\n", v[0], v[1], v[2]));
            }
            out.push_str("  endloop\nendfacet\n");
        }
    }
    out.push_str("endsolid test\n");
    out
}

fn write_temp(name: &str, contents: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(name);
    let mut f = std::fs::File::create(&p).unwrap();
    f.write_all(contents.as_bytes()).unwrap();
    p
}

#[test]
fn loads_an_ascii_cube() {
    let p = write_temp("camcraft_cube.stl", &cube_ascii(10.0));
    let (mesh, report) = load_stl(&p, Units::Millimetres).unwrap();
    assert_eq!(report.triangles, 12);
    assert_eq!(report.degenerate, 0);
    assert_eq!(mesh.len(), 12);
    let b = mesh.bounds().unwrap();
    assert_eq!(b.min, dvec3(0.0, 0.0, 0.0));
    assert_eq!(b.max, dvec3(10.0, 10.0, 10.0));
}

/// Welding must collapse the 36 loose corners of a cube to its 8 real ones.
#[test]
fn welds_shared_vertices() {
    let p = write_temp("camcraft_weld.stl", &cube_ascii(4.0));
    let (mesh, report) = load_stl(&p, Units::Millimetres).unwrap();
    assert_eq!(report.triangles * 3, 36);
    assert_eq!(report.vertices, 8, "a cube has eight distinct corners");
    assert_eq!(mesh.vertices().len(), 8);
}

/// The failure this signature exists to prevent: an inch file silently read as
/// millimetres is 25.4 times too small, and nothing in an STL would say so.
#[test]
fn units_are_applied_not_assumed() {
    let p = write_temp("camcraft_units.stl", &cube_ascii(1.0));
    let (mm, _) = load_stl(&p, Units::Millimetres).unwrap();
    let (inch, _) = load_stl(&p, Units::Inches).unwrap();
    assert_eq!(mm.bounds().unwrap().max.x, 1.0);
    assert_eq!(inch.bounds().unwrap().max.x, 25.4);
}

#[test]
fn reports_degenerate_triangles_rather_than_hiding_them() {
    let mut s = String::from("solid t\n");
    // One real triangle and one with two identical corners.
    for tri in [
        [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
    ] {
        s.push_str("facet normal 0 0 0\n  outer loop\n");
        for v in tri {
            s.push_str(&format!("    vertex {} {} {}\n", v[0], v[1], v[2]));
        }
        s.push_str("  endloop\nendfacet\n");
    }
    s.push_str("endsolid t\n");
    let p = write_temp("camcraft_degenerate.stl", &s);
    let (mesh, report) = load_stl(&p, Units::Millimetres).unwrap();
    assert_eq!(report.triangles, 2);
    assert_eq!(report.degenerate, 1);
    assert_eq!(mesh.len(), 1, "the degenerate triangle is dropped");
}

#[test]
fn a_missing_file_is_an_error_not_a_panic() {
    let err = load_stl("this-file-does-not-exist.stl", Units::Millimetres);
    assert!(matches!(err, Err(ImportError::Io { .. })));
}

/// The reported failure: `solid` and a newline, no name, as most of opencamlib's
/// samples begin. With Windows line endings, a byte-order mark and capitals too.
#[test]
fn an_ascii_file_without_a_name_is_read() {
    let body = cube_ascii(2.0).replacen("solid test\n", "", 1);
    for (i, head) in [
        "solid\n",
        "solid\r\n",
        "\u{feff}solid\n",
        "SOLID\n",
        "  solid  \n",
    ]
    .iter()
    .enumerate()
    {
        let p = write_temp(
            &format!("camcraft_nameless_{i}.stl"),
            &format!("{head}{body}"),
        );
        let (mesh, report) =
            load_stl(&p, Units::Millimetres).unwrap_or_else(|e| panic!("{head:?}: {e}"));
        assert_eq!(report.triangles, 12, "{head:?}");
        assert_eq!(mesh.bounds().unwrap().max.x, 2.0, "{head:?}");
    }
}

/// A binary file is binary whatever its header says, even `solid`.
#[test]
fn a_binary_file_whose_header_says_solid_is_read_as_binary() {
    let tri = [[0.0f32, 0.0, 0.0], [3.0, 0.0, 0.0], [0.0, 3.0, 0.0]];
    let mut bytes = b"solid exported by a CAD system".to_vec();
    bytes.resize(80, b' ');
    bytes.extend(1u32.to_le_bytes());
    bytes.extend([0u8; 12]);
    for v in tri {
        for c in v {
            bytes.extend(c.to_le_bytes());
        }
    }
    bytes.extend([0u8; 2]);
    let p = std::env::temp_dir().join("camcraft_binary_solid.stl");
    std::fs::write(&p, &bytes).unwrap();
    let (mesh, report) = load_stl(&p, Units::Millimetres).unwrap();
    assert_eq!(report.triangles, 1);
    assert_eq!(mesh.bounds().unwrap().max.x, 3.0);
}

/// A word that merely begins with `solid` is not the ASCII keyword.
#[test]
fn only_the_keyword_counts_as_ascii() {
    assert_eq!(unambiguous(b"solidity\n".to_vec()), b"solidity\n".to_vec());
    assert_eq!(
        unambiguous(b"solid\nfacet".to_vec()),
        b"solid \nfacet".to_vec()
    );
}

#[test]
fn an_empty_solid_is_rejected() {
    let p = write_temp("camcraft_empty.stl", "solid t\nendsolid t\n");
    assert!(matches!(
        load_stl(&p, Units::Millimetres),
        Err(ImportError::Empty { .. })
    ));
}
