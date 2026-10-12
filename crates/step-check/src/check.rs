//! Holding a kernel's mesh up against what the file says.

use glam::DVec3;

use crate::part21::{self, StepFacts};

/// What a kernel makes of a STEP file: triangles in millimetres, grouped by the
/// B-rep face they came from.
#[derive(Debug, Clone, Default)]
pub struct KernelMesh {
    /// Corner positions, in millimetres.
    pub vertices: Vec<DVec3>,
    /// Three per triangle, the triangles in face order.
    pub indices: Vec<usize>,
    /// How many triangles each face gave, in the kernel's order of faces. The order
    /// should depend only on the file, so a face keeps its number when the
    /// tolerance changes.
    pub face_triangles: Vec<usize>,
}

/// Whether the mesh was checked against the file's own geometry.
#[derive(Debug, Clone, PartialEq)]
pub enum FileCheck {
    /// Both geometric checks ran and passed.
    Passed,
    /// They could not run, and why. Worth telling a person about.
    Skipped(&'static str),
}

/// A mesh that does not match the file it came from.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Problem {
    /// Fewer faces came out of the kernel than the file defines.
    #[error(
        "the file defines {in_file} face(s) but only {meshed} arrived; \
         a missing face is a hole in the part"
    )]
    DroppedFaces {
        /// Faces the file defines.
        in_file: u64,
        /// Faces the kernel gave.
        meshed: u64,
    },
    /// A face came back without a single triangle. A kernel that gives up on a face
    /// may hand it back empty rather than refuse, and the face count still passes.
    #[error("face {face} came back with no triangles; a missing face is a hole in the part")]
    EmptyFace {
        /// The face, in the kernel's order.
        face: usize,
    },
    /// The faces' triangle counts do not add up to the triangles given.
    #[error("the faces account for {counted} triangles but the mesh has {triangles}")]
    Miscounted {
        /// The sum of the faces' counts.
        counted: usize,
        /// Triangles in the mesh.
        triangles: usize,
    },
    /// The mesh extends beyond anything the file describes.
    #[error(
        "the mesh reaches {by:.3} mm outside the file's own geometry on {axis}; \
         the surfaces were evaluated wrongly or the units are not what the file says"
    )]
    OutsideFile {
        /// Which axis and side.
        axis: &'static str,
        /// How far, in millimetres.
        by: f64,
    },
    /// The mesh falls short of the file's own vertices.
    #[error(
        "the mesh stops {by:.3} mm short of the file's vertices on {axis}; \
         the part is smaller than the file says, which is what a unit error looks like"
    )]
    ShortOfVertices {
        /// Which axis and side.
        axis: &'static str,
        /// How far, in millimetres.
        by: f64,
    },
}

/// Hold `mesh`, tessellated to within `tolerance` millimetres, up against what the
/// file says. See the crate docs for the checks.
pub fn check(facts: &StepFacts, mesh: &KernelMesh, tolerance: f64) -> Result<FileCheck, Problem> {
    let meshed = mesh.face_triangles.len() as u64;
    if meshed < facts.faces {
        return Err(Problem::DroppedFaces {
            in_file: facts.faces,
            meshed,
        });
    }
    if let Some(face) = mesh.face_triangles.iter().position(|&n| n == 0) {
        return Err(Problem::EmptyFace { face });
    }
    let counted: usize = mesh.face_triangles.iter().sum();
    if counted * 3 != mesh.indices.len() {
        return Err(Problem::Miscounted {
            counted,
            triangles: mesh.indices.len() / 3,
        });
    }

    if facts.unplaced {
        return Ok(FileCheck::Skipped(
            "geometry is placed by transforms this cannot follow, so the file's raw coordinates bound nothing",
        ));
    }
    let used = mesh.indices.iter().filter_map(|&i| mesh.vertices.get(i));
    let (Some(extent), Some(on_part), Some(got)) = (
        facts.extent,
        part21::bounds(&facts.vertices),
        part21::bounds(used),
    ) else {
        return Ok(FileCheck::Skipped(
            "the file has no vertices to check against",
        ));
    };

    // Room for the tessellation itself and for float noise on a large part.
    let size = (extent.1 - extent.0).max_element().max(1.0);
    let slack = tolerance + size * 1e-9;

    let (lo, hi) = (
        extent.0 - DVec3::splat(slack),
        extent.1 + DVec3::splat(slack),
    );
    if let Some((axis, by)) = worst_excess(got, (lo, hi)) {
        return Err(Problem::OutsideFile { axis, by });
    }

    let (lo, hi) = (got.0 - DVec3::splat(slack), got.1 + DVec3::splat(slack));
    if let Some((axis, by)) = worst_excess(on_part, (lo, hi)) {
        return Err(Problem::ShortOfVertices { axis, by });
    }
    Ok(FileCheck::Passed)
}

/// How far `inner` sticks out of `outer`, at worst, and where.
fn worst_excess(inner: (DVec3, DVec3), outer: (DVec3, DVec3)) -> Option<(&'static str, f64)> {
    let candidates = [
        ("-X", outer.0.x - inner.0.x),
        ("-Y", outer.0.y - inner.0.y),
        ("-Z", outer.0.z - inner.0.z),
        ("+X", inner.1.x - outer.1.x),
        ("+Y", inner.1.y - outer.1.y),
        ("+Z", inner.1.z - outer.1.z),
    ];
    candidates
        .into_iter()
        .filter(|(_, by)| *by > 0.0)
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use glam::dvec3;

    use super::*;

    /// One 10 mm square face, its four corners the file's vertices.
    const SQUARE: &str = "ISO-10303-21;
HEADER; FILE_SCHEMA (('AUTOMOTIVE_DESIGN'));
ENDSEC;
DATA;
#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(10.,0.,0.));
#3=CARTESIAN_POINT('',(10.,10.,0.));
#4=CARTESIAN_POINT('',(0.,10.,0.));
#11=VERTEX_POINT('',#1);
#12=VERTEX_POINT('',#2);
#13=VERTEX_POINT('',#3);
#14=VERTEX_POINT('',#4);
#20=ADVANCED_FACE('',(#21),#22,.T.);
#900=( GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#901)) REPRESENTATION_CONTEXT('','3D') );
#901=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );
ENDSEC;
END-ISO-10303-21;
";

    fn square(scale: f64) -> KernelMesh {
        KernelMesh {
            vertices: [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
                .map(|(x, y)| dvec3(x, y, 0.0) * scale)
                .to_vec(),
            indices: vec![0, 1, 2, 0, 2, 3],
            face_triangles: vec![2],
        }
    }

    #[test]
    fn the_right_mesh_passes() {
        let facts = part21::facts(SQUARE).unwrap();
        assert_eq!(check(&facts, &square(1.0), 0.01), Ok(FileCheck::Passed));
    }

    /// Both unit-error directions: 25.4 times too large leaves the file's
    /// geometry, and 25.4 times too small falls short of its vertices.
    #[test]
    fn a_mesh_at_the_wrong_scale_is_refused_either_way() {
        let facts = part21::facts(SQUARE).unwrap();
        assert!(matches!(
            check(&facts, &square(25.4), 0.01),
            Err(Problem::OutsideFile { .. })
        ));
        assert!(matches!(
            check(&facts, &square(1.0 / 25.4), 0.01),
            Err(Problem::ShortOfVertices { .. })
        ));
    }

    #[test]
    fn missing_and_empty_faces_are_refused() {
        let facts = part21::facts(SQUARE).unwrap();
        let none = KernelMesh::default();
        assert_eq!(
            check(&facts, &none, 0.01),
            Err(Problem::DroppedFaces {
                in_file: 1,
                meshed: 0
            })
        );
        let mut empty = square(1.0);
        empty.face_triangles = vec![2, 0];
        assert_eq!(
            check(&facts, &empty, 0.01),
            Err(Problem::EmptyFace { face: 1 })
        );
        let mut miscounted = square(1.0);
        miscounted.face_triangles = vec![1];
        assert!(matches!(
            check(&facts, &miscounted, 0.01),
            Err(Problem::Miscounted { .. })
        ));
    }

    #[test]
    fn excess_names_the_worst_side() {
        let outer = (dvec3(0.0, 0.0, 0.0), dvec3(2.0, 2.0, 2.0));
        // Out by 1.0 on -X and by 1.5 on +Y: the worse one is reported.
        let inner = (dvec3(-1.0, 0.0, 0.0), dvec3(1.0, 3.5, 1.0));
        assert_eq!(worst_excess(inner, outer), Some(("+Y", 1.5)));
        let inside = (dvec3(0.5, 0.5, 0.5), dvec3(1.5, 1.5, 1.5));
        assert_eq!(worst_excess(inside, outer), None);
    }
}
