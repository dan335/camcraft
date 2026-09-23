//! STEP import, through OpenCascade.
//!
//! The kernel reads the B-rep, converts it to millimetres and tessellates it. None
//! of that is taken on trust, because a STEP importer fails in ways that look fine:
//! a face that did not transfer is a hole the cutter drops into, and a unit read
//! wrongly is a part 25.4 times the wrong size. So the mesh is checked against the
//! file it came from, using facts read from the file's text independently of the
//! kernel (see [`crate::part21`]):
//!
//! - **Every face arrives.** The file's face count against the kernel's. The
//!   tessellator separately refuses any face it could not triangulate.
//! - **Nothing lands outside the file's own geometry.** A NURBS surface lies within
//!   the convex hull of its control points, and a circle within its radius of its
//!   centre, so the mesh must fit inside the bounding box of every point in the file,
//!   grown by the largest radius. This is what caught `truck-stepio` putting a
//!   surface 9.5 times outside its hull; see `docs/decisions/0001-step-backend.md`.
//! - **The mesh reaches every vertex.** B-rep vertices lie on the part, so the mesh
//!   must extend at least as far as they do. This is the check that catches a unit
//!   error which makes the part too *small*, which the hull check alone cannot: a
//!   shrunken part still sits inside the hull.
//!
//! The two geometric checks need the file's raw coordinates to be in the frame of
//! the result. When geometry is placed by transforms, as in an assembly, they are
//! not, and the checks are reported as skipped rather than passed.
//!
//! # Faces
//!
//! Every triangle keeps the B-rep face it was tessellated from, numbered in the
//! order the kernel explores the shape's faces, so the result is a [`Body`] whose
//! faces are the file's own. That order depends only on the file, not on the
//! tessellation, so a face keeps its number when the tolerance changes.

use std::path::Path;
use std::sync::{Mutex, PoisonError};

use camcraft_geom::mesh::{MeshError, TriMesh};
use camcraft_geom::topology::{Body, TopologyError};
use glam::{DVec3, dvec3};

use crate::part21::{self, LengthUnit, Part21Error};
use crate::weld::Welder;

/// Serialises every call into OpenCascade. See [`load_step`].
static KERNEL: Mutex<()> = Mutex::new(());

/// A STEP file that could not be turned into a trustworthy mesh.
#[derive(Debug, thiserror::Error)]
pub enum StepError {
    /// The file could not be read.
    #[error("cannot read {path}: {source}")]
    Io {
        /// Which file.
        path: String,
        /// Why.
        source: std::io::Error,
    },
    /// The file's text could not be understood well enough to check the result.
    #[error("{path}: {source}")]
    Text {
        /// Which file.
        path: String,
        /// Why.
        source: Part21Error,
    },
    /// The kernel could not read the file.
    #[error("{path}: OpenCascade could not read it ({detail})")]
    Kernel {
        /// Which file.
        path: String,
        /// What the kernel said.
        detail: String,
    },
    /// Fewer faces came out of the kernel than the file defines.
    #[error(
        "{path}: the file defines {in_file} face(s) but only {meshed} arrived; \
         a missing face is a hole the cutter would drop into"
    )]
    DroppedFaces {
        /// Which file.
        path: String,
        /// Faces the file defines.
        in_file: u64,
        /// Faces the kernel produced.
        meshed: u64,
    },
    /// The mesh extends beyond anything the file describes.
    #[error(
        "{path}: the mesh reaches {by:.3} mm outside the file's own geometry on {axis}; \
         the surfaces were evaluated wrongly or the units are not what the file says"
    )]
    OutsideFile {
        /// Which file.
        path: String,
        /// Which axis and side.
        axis: &'static str,
        /// How far, in millimetres.
        by: f64,
    },
    /// The mesh falls short of the file's own vertices.
    #[error(
        "{path}: the mesh stops {by:.3} mm short of the file's vertices on {axis}; \
         the part is smaller than the file says, which is what a unit error looks like"
    )]
    ShortOfVertices {
        /// Which file.
        path: String,
        /// Which axis and side.
        axis: &'static str,
        /// How far, in millimetres.
        by: f64,
    },
    /// The mesh was not usable.
    #[error("{path} is not a usable mesh: {source}")]
    Mesh {
        /// Which file.
        path: String,
        /// Why.
        source: MeshError,
    },
    /// The triangles could not be matched to the faces they came from.
    #[error("{path}: the triangles do not match the file's faces: {detail}")]
    Faces {
        /// Which file.
        path: String,
        /// What did not match.
        detail: String,
    },
    /// Nothing was left to machine.
    #[error("{path} produced no triangles")]
    Empty {
        /// Which file.
        path: String,
    },
    /// The requested tessellation tolerance was not usable.
    #[error("tessellation tolerance must be finite and greater than zero, got {0}")]
    BadTolerance(f64),
}

/// Whether the mesh was checked against the file's own geometry.
#[derive(Debug, Clone, PartialEq)]
pub enum FileCheck {
    /// Both geometric checks ran and passed.
    Passed,
    /// They could not run, and why. Worth telling a person about.
    Skipped(&'static str),
}

/// What loading found, beyond the mesh itself.
#[derive(Debug, Clone)]
pub struct StepReport {
    /// The length unit the file declared.
    pub unit: LengthUnit,
    /// Faces the file defines.
    pub faces: u64,
    /// Triangles the kernel produced.
    pub triangles: u64,
    /// Triangles dropped because corners coincided.
    pub degenerate: u64,
    /// Vertices after welding.
    pub vertices: u64,
    /// The tessellation tolerance used, in millimetres.
    pub tolerance: f64,
    /// Whether the geometric checks ran.
    pub check: FileCheck,
}

/// Read a STEP file and tessellate it to within `tolerance` millimetres, keeping
/// the face each triangle came from.
///
/// The tolerance is the largest distance between a triangle and the true surface.
/// It is error the simulator cannot see, because the simulator checks against this
/// mesh, so it should be a small part of the machining tolerance.
pub fn load_step(path: impl AsRef<Path>, tolerance: f64) -> Result<(Body, StepReport), StepError> {
    if !(tolerance.is_finite() && tolerance > 0.0) {
        return Err(StepError::BadTolerance(tolerance));
    }
    let path = path.as_ref();
    let name = path.display().to_string();

    let text = std::fs::read(path).map_err(|source| StepError::Io {
        path: name.clone(),
        source,
    })?;
    // STEP is ASCII by the standard; anything else is in strings, which are only
    // ever skipped here.
    let text = String::from_utf8_lossy(&text);
    let facts = part21::facts(&text).map_err(|source| StepError::Text {
        path: name.clone(),
        source,
    })?;

    let kernel = |detail: String| StepError::Kernel {
        path: name.clone(),
        detail,
    };
    let soup = {
        // One kernel call at a time, across every thread. OpenCascade's data
        // exchange keeps its settings in process-wide statics, so this does not
        // rely on every caller happening to read on one thread. A read once failed
        // during a full gate run with "failed to read STEP file" on a fixture that
        // reads fine alone; that was never reproduced (0 of 80 runs, with and
        // without this lock), so this is a precaution, not a proven fix. The shape
        // is declared after the guard, so it is dropped while the lock is held.
        let _kernel = KERNEL.lock().unwrap_or_else(PoisonError::into_inner);
        let shape =
            opencascade::primitives::Shape::read_step(path).map_err(|e| kernel(e.to_string()))?;
        let meshed = shape.faces().count() as u64;
        if meshed < facts.faces {
            return Err(StepError::DroppedFaces {
                path: name,
                in_file: facts.faces,
                meshed,
            });
        }
        // Refuses outright if any face could not be triangulated.
        shape
            .mesh_with_tolerance(tolerance)
            .map_err(|e| kernel(e.to_string()))?
    };

    let faces = |detail: String| StepError::Faces {
        path: name.clone(),
        detail,
    };
    let counted: usize = soup.face_triangles.iter().sum();
    if counted * 3 != soup.indices.len() {
        return Err(faces(format!(
            "the faces account for {counted} triangles but the kernel gave {}",
            soup.indices.len() / 3
        )));
    }
    let face_count = u32::try_from(soup.face_triangles.len())
        .map_err(|_| faces(format!("{} faces is too many", soup.face_triangles.len())))?;
    // The face of each triangle, in the kernel's order.
    let face_of = soup
        .face_triangles
        .iter()
        .zip(0u32..)
        .flat_map(|(&n, face)| std::iter::repeat_n(face, n));

    let mut weld = Welder::default();
    let mut labels = Vec::new();
    let mut triangles = 0;
    for (tri, face) in soup.indices.as_chunks::<3>().0.iter().zip(face_of) {
        triangles += 1;
        let mut corners = [DVec3::NAN; 3];
        for (slot, &i) in corners.iter_mut().zip(tri) {
            if let Some(v) = soup.vertices.get(i) {
                *slot = dvec3(v.x, v.y, v.z);
            }
        }
        // A triangle the weld drops takes its label with it.
        if weld.push(corners) {
            labels.push(face);
        }
    }
    let degenerate = weld.degenerate;
    let vertices = weld.vertex_count() as u64;
    if weld.triangle_count() == 0 {
        return Err(StepError::Empty { path: name });
    }
    let mesh = weld.finish().map_err(|source| StepError::Mesh {
        path: name.clone(),
        source,
    })?;

    let check = check_against_file(&mesh, &facts, tolerance, &name)?;
    let body = Body::with_faces(mesh, labels, face_count)
        .map_err(|e: TopologyError| faces(e.to_string()))?;
    Ok((
        body,
        StepReport {
            unit: facts.unit,
            faces: facts.faces,
            triangles,
            degenerate,
            vertices,
            tolerance,
            check,
        },
    ))
}

/// The two geometric checks described in the module documentation.
fn check_against_file(
    mesh: &TriMesh,
    facts: &part21::StepFacts,
    tolerance: f64,
    name: &str,
) -> Result<FileCheck, StepError> {
    if facts.transformed {
        return Ok(FileCheck::Skipped(
            "geometry is placed by transforms, so the file's raw coordinates bound nothing",
        ));
    }
    let (Some(extent), Some(on_part), Some(got)) = (
        facts.extent,
        part21::bounds(&facts.vertices),
        mesh.bounds().map(|b| (b.min, b.max)),
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
        return Err(StepError::OutsideFile {
            path: name.to_string(),
            axis,
            by,
        });
    }

    let (lo, hi) = (got.0 - DVec3::splat(slack), got.1 + DVec3::splat(slack));
    if let Some((axis, by)) = worst_excess(on_part, (lo, hi)) {
        return Err(StepError::ShortOfVertices {
            path: name.to_string(),
            axis,
            by,
        });
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
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn excess_names_the_worst_side() {
        let outer = (dvec3(0.0, 0.0, 0.0), dvec3(2.0, 2.0, 2.0));
        // Out by 1.0 on -X and by 1.5 on +Y: the worse one is reported.
        let inner = (dvec3(-1.0, 0.0, 0.0), dvec3(1.0, 3.5, 1.0));
        assert_eq!(worst_excess(inner, outer), Some(("+Y", 1.5)));
        let inside = (dvec3(0.5, 0.5, 0.5), dvec3(1.5, 1.5, 1.5));
        assert_eq!(worst_excess(inside, outer), None);
    }

    /// Both unit-error directions, against a real part: a mesh 25.4 times too small
    /// falls short of the file's vertices, and one 25.4 times too large leaves the
    /// file's geometry. Neither may pass.
    #[test]
    fn a_mesh_at_the_wrong_scale_is_refused_either_way() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/step/hinge_jig.step"
        );
        let text = std::fs::read_to_string(path).unwrap();
        let facts = part21::facts(&text).unwrap();
        let (body, _) = load_step(path, 0.01).unwrap();
        let mesh = body.mesh();
        assert_eq!(
            check_against_file(mesh, &facts, 0.01, "hinge").unwrap(),
            FileCheck::Passed
        );

        let scaled = |k: f64| {
            let v = mesh.vertices().iter().map(|p| *p * k).collect();
            TriMesh::new(v, mesh.triangles().to_vec()).unwrap()
        };
        assert!(matches!(
            check_against_file(&scaled(1.0 / 25.4), &facts, 0.01, "hinge"),
            Err(StepError::ShortOfVertices { .. })
        ));
        assert!(matches!(
            check_against_file(&scaled(25.4), &facts, 0.01, "hinge"),
            Err(StepError::OutsideFile { .. })
        ));
    }

    #[test]
    fn an_unusable_tolerance_is_refused_before_reading() {
        assert!(matches!(
            load_step("does-not-matter.step", 0.0),
            Err(StepError::BadTolerance(_))
        ));
        assert!(matches!(
            load_step("does-not-matter.step", f64::NAN),
            Err(StepError::BadTolerance(_))
        ));
    }
}
