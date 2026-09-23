//! STL import.
//!
//! STL is a bag of loose triangles with no shared vertices and no units. Both are
//! handled explicitly here rather than assumed:
//!
//! - **Vertices are welded** on an exact bit-pattern key. A drop-cutter does not
//!   need a manifold mesh, but an unwelded one triples the BVH leaf count for no
//!   benefit, and welding is nearly free during load.
//! - **Units are not in the file.** STL carries no unit context at all, so the
//!   caller must say what the numbers mean. Defaulting to millimetres would be the
//!   failure that destroys a part: the same file exported from a Fusion document in
//!   inches is 25.4 times too small, and nothing in the data would say so.
//! - **ASCII or binary is decided here, not by the reader.** `stl_io` calls a file
//!   ASCII exactly when it begins `solid ` with a space. Many ASCII files begin
//!   `solid` and a newline, with no name — most of opencamlib's samples do — and
//!   were read as binary and failed. And some exporters write binary files whose
//!   free-text header begins `solid `, which it would read as ASCII. A binary STL's
//!   length is fixed by the triangle count in its header, so that decides first;
//!   the reader is then handed a copy it cannot misread.

use camcraft_geom::mesh::{MeshError, TriMesh};
use glam::{DVec3, dvec3};
use std::path::Path;

use crate::weld::Welder;

/// What the numbers in a unit-less file mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Units {
    /// Values are millimetres.
    Millimetres,
    /// Values are inches, and will be scaled by 25.4.
    Inches,
}

impl Units {
    /// Scale factor to millimetres.
    pub fn to_mm(self) -> f64 {
        match self {
            Self::Millimetres => 1.0,
            Self::Inches => 25.4,
        }
    }
}

/// A file that could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// The file could not be opened or read.
    #[error("cannot read {path}: {source}")]
    Io {
        /// Which file.
        path: String,
        /// Why.
        source: std::io::Error,
    },
    /// The contents were not usable as a mesh.
    #[error("{path} is not a usable mesh: {source}")]
    Mesh {
        /// Which file.
        path: String,
        /// Why.
        source: MeshError,
    },
    /// The file parsed but contained no triangles.
    #[error("{path} contains no triangles")]
    Empty {
        /// Which file.
        path: String,
    },
}

/// What loading found, beyond the mesh itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Triangles in the file.
    pub triangles: u64,
    /// Triangles dropped because two or more corners coincided exactly.
    ///
    /// Reported rather than hidden: a file that is mostly degenerate is a sign the
    /// export went wrong, even though the drop-cutter tolerates them.
    pub degenerate: u64,
    /// Vertices after welding.
    pub vertices: u64,
    /// Vertices before welding.
    pub raw_vertices: u64,
}

/// Read an STL file, scaling by `units`.
///
/// Both ASCII and binary STL are accepted; the format is detected by content.
pub fn load_stl(
    path: impl AsRef<Path>,
    units: Units,
) -> Result<(TriMesh, ImportReport), ImportError> {
    let path = path.as_ref();
    let name = path.display().to_string();
    let bytes = std::fs::read(path).map_err(|source| ImportError::Io {
        path: name.clone(),
        source,
    })?;
    let stl =
        stl_io::read_stl(&mut std::io::Cursor::new(unambiguous(bytes))).map_err(|source| {
            ImportError::Io {
                path: name.clone(),
                source,
            }
        })?;

    let scale = units.to_mm();
    let mut report = ImportReport::default();
    let mut weld = Welder::default();

    for face in &stl.faces {
        report.triangles += 1;
        report.raw_vertices += 3;
        let mut corners = [DVec3::NAN; 3];
        for (slot, index) in corners.iter_mut().zip(face.vertices) {
            if let Some(v) = stl.vertices.get(index) {
                *slot = dvec3(
                    f64::from(v[0]) * scale,
                    f64::from(v[1]) * scale,
                    f64::from(v[2]) * scale,
                );
            }
        }
        // A missing or non-finite corner is dropped and counted by the welder.
        weld.push(corners);
    }

    report.degenerate = weld.degenerate;
    report.vertices = weld.vertex_count() as u64;
    if weld.triangle_count() == 0 {
        return Err(ImportError::Empty { path: name });
    }

    let mesh = weld
        .finish()
        .map_err(|source| ImportError::Mesh { path: name, source })?;
    Ok((mesh, report))
}

/// A binary STL: an 80-byte header, a triangle count, then 50 bytes a triangle.
const BINARY_HEADER: usize = 80;

/// Whether `bytes` are a binary STL: exactly as long as the count they give says.
fn is_binary(bytes: &[u8]) -> bool {
    let Some(count) = bytes
        .get(BINARY_HEADER..BINARY_HEADER + 4)
        .and_then(|c| <[u8; 4]>::try_from(c).ok())
        .map(u32::from_le_bytes)
    else {
        return false;
    };
    u64::from(count) * 50 + 84 == bytes.len() as u64
}

/// `bytes`, rewritten so the reader cannot mistake ASCII for binary or back.
///
/// Binary: a header beginning `solid` is blanked, since the header is free text
/// that nothing reads. ASCII: a first line of `solid` with no name, or with a
/// byte-order mark, space or capitals before it, becomes `solid ` and the name.
/// Anything else is passed on as it is, for the reader to judge.
fn unambiguous(mut bytes: Vec<u8>) -> Vec<u8> {
    if is_binary(&bytes) {
        if let Some(header) = bytes.get_mut(..5)
            && header.eq_ignore_ascii_case(b"solid")
        {
            header.copy_from_slice(b"     ");
        }
        return bytes;
    }
    let end = bytes
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(bytes.len());
    let Some(first) = bytes.get(..end) else {
        return bytes;
    };
    let first = String::from_utf8_lossy(first);
    let line = first.trim_start_matches('\u{feff}').trim();
    let solid = line
        .get(..5)
        .is_some_and(|w| w.eq_ignore_ascii_case("solid"));
    let named = line
        .get(5..)
        .is_none_or(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace));
    if !(solid && named) {
        return bytes;
    }
    let rest = line.get(5..).unwrap_or("").trim();
    let mut out = format!("solid {rest}\n").into_bytes();
    out.extend(bytes.get(end + 1..).unwrap_or(&[]));
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests {
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
        assert_eq!(report.raw_vertices, 36);
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
}
