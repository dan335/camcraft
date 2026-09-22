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

use camcraft_geom::mesh::{MeshError, TriMesh};
use glam::{DVec3, dvec3};
use std::collections::HashMap;
use std::path::Path;

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
    let mut file = std::fs::File::open(path).map_err(|source| ImportError::Io {
        path: name.clone(),
        source,
    })?;
    let stl = stl_io::read_stl(&mut file).map_err(|source| ImportError::Io {
        path: name.clone(),
        source,
    })?;

    let scale = units.to_mm();
    let mut report = ImportReport::default();
    let mut welded: HashMap<[u64; 3], u32> = HashMap::new();
    let mut vertices: Vec<DVec3> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();

    let intern = |v: stl_io::Vertex,
                  vertices: &mut Vec<DVec3>,
                  welded: &mut HashMap<[u64; 3], u32>|
     -> Option<u32> {
        let p = dvec3(
            f64::from(v[0]) * scale,
            f64::from(v[1]) * scale,
            f64::from(v[2]) * scale,
        );
        if !p.is_finite() {
            return None;
        }
        // Exact bit-pattern key: welding must never merge two points that are
        // genuinely distinct, and a tolerance-based weld can do exactly that.
        let key = [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
        Some(*welded.entry(key).or_insert_with(|| {
            vertices.push(p);
            // Index fits: the count is bounded by the file's triangle count, and a
            // mesh larger than u32::MAX vertices would have failed to load.
            u32::try_from(vertices.len() - 1).unwrap_or(u32::MAX)
        }))
    };

    for face in &stl.faces {
        report.triangles += 1;
        report.raw_vertices += 3;
        let mut abc = [0u32; 3];
        let mut ok = true;
        for (slot, index) in abc.iter_mut().zip(face.vertices) {
            match stl.vertices.get(index).copied() {
                Some(v) => match intern(v, &mut vertices, &mut welded) {
                    Some(i) => *slot = i,
                    None => ok = false,
                },
                None => ok = false,
            }
        }
        if !ok {
            report.degenerate += 1;
            continue;
        }
        if abc[0] == abc[1] || abc[1] == abc[2] || abc[0] == abc[2] {
            report.degenerate += 1;
            continue;
        }
        triangles.push(abc);
    }

    report.vertices = vertices.len() as u64;
    if triangles.is_empty() {
        return Err(ImportError::Empty { path: name });
    }

    let mesh = TriMesh::new(vertices, triangles)
        .map_err(|source| ImportError::Mesh { path: name, source })?;
    Ok((mesh, report))
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

    #[test]
    fn an_empty_solid_is_rejected() {
        let p = write_temp("camcraft_empty.stl", "solid t\nendsolid t\n");
        assert!(matches!(
            load_stl(&p, Units::Millimetres),
            Err(ImportError::Empty { .. })
        ));
    }
}
