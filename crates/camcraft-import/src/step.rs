//! STEP import, through OpenCascade.
//!
//! The kernel reads the B-rep, converts it to millimetres and tessellates it. None
//! of that is taken on trust, because a STEP importer fails in ways that look fine:
//! a face that did not transfer is a hole the cutter drops into, and a unit read
//! wrongly is a part 25.4 times the wrong size. So the mesh is checked against the
//! file it came from, using facts read from the file's text independently of the
//! kernel (see [`crate::part21`]):
//!
//! The checks are [`step_check`]'s, kept free of camcraft so any importer can use them:
//!
//! - **Every face arrives, with triangles.** The file's face count against the
//!   kernel's, and then every face the kernel gave must have at least one triangle.
//!   OpenCascade refuses a face it cannot triangulate, but a kernel that hands one
//!   back empty would pass the count.
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
//! The two geometric checks are made where the file's assembly puts each component
//! ([`step_check::part21`] follows its placements). Only geometry placed in a way that
//! cannot be followed, such as a mapped item, leaves them reported as skipped rather
//! than passed.
//!
//! Another kernel's mesh can be put through the same checks with
//! [`load_step_with`], which is where a replacement for OpenCascade plugs in.
//!
//! # Faces
//!
//! Every triangle keeps the B-rep face it was tessellated from, numbered in the
//! order the kernel explores the shape's faces, so the result is a [`Body`] whose
//! faces are the file's own. That order depends only on the file, not on the
//! tessellation, so a face keeps its number when the tolerance changes.

use std::path::Path;
use std::sync::{Mutex, PoisonError};

use camcraft_geom::mesh::MeshError;
use camcraft_geom::topology::{Body, TopologyError};
use glam::{DVec3, dvec3};
use step_check::Problem;
use step_check::part21::{self, LengthUnit, Part21Error};
pub use step_check::{FileCheck, KernelMesh};

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
    #[error("{path}: {kernel} could not read it ({detail})")]
    Kernel {
        /// Which file.
        path: String,
        /// Which kernel.
        kernel: &'static str,
        /// What the kernel said.
        detail: String,
    },
    /// The mesh does not match the file it came from.
    #[error("{path}: {source}")]
    Check {
        /// Which file.
        path: String,
        /// How it does not match.
        source: Problem,
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
    load_step_with(path, tolerance, "OpenCascade", |path, _facts| {
        // One kernel call at a time, across every thread. OpenCascade's data
        // exchange keeps its settings in process-wide statics, so this does not
        // rely on every caller happening to read on one thread. A read once failed
        // during a full gate run with "failed to read STEP file" on a fixture that
        // reads fine alone; that was never reproduced (0 of 80 runs, with and
        // without this lock), so this is a precaution, not a proven fix. The shape
        // is declared after the guard, so it is dropped while the lock is held.
        let _kernel = KERNEL.lock().unwrap_or_else(PoisonError::into_inner);
        let shape = opencascade::primitives::Shape::read_step(path).map_err(|e| e.to_string())?;
        // Refuses outright if any face could not be triangulated.
        let mesh = shape
            .mesh_with_tolerance(tolerance)
            .map_err(|e| e.to_string())?;
        Ok(KernelMesh {
            vertices: mesh.vertices.iter().map(|v| dvec3(v.x, v.y, v.z)).collect(),
            indices: mesh.indices,
            face_triangles: mesh.face_triangles,
        })
    })
}

/// [`load_step`] with the kernel's half supplied: `kernel` reads the file at
/// `path` and tessellates it to within `tolerance` millimetres, given what the
/// file says independently (its unit, for a kernel that ignores units). The result
/// faces the same checks OpenCascade's does, and comes back as camcraft uses it.
pub fn load_step_with(
    path: impl AsRef<Path>,
    tolerance: f64,
    kernel_name: &'static str,
    kernel: impl FnOnce(&Path, &part21::StepFacts) -> Result<KernelMesh, String>,
) -> Result<(Body, StepReport), StepError> {
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

    let soup = kernel(path, &facts).map_err(|detail| StepError::Kernel {
        path: name.clone(),
        kernel: kernel_name,
        detail,
    })?;
    let check = step_check::check(&facts, &soup, tolerance).map_err(|source| StepError::Check {
        path: name.clone(),
        source,
    })?;

    let faces = |detail: String| StepError::Faces {
        path: name.clone(),
        detail,
    };
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
                *slot = *v;
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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    const HINGE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/step/hinge_jig.step"
    );

    /// OpenCascade's mesh of the hinge jig, handed back as another kernel would.
    fn hinge_from_another_kernel() -> KernelMesh {
        let (body, _) = load_step(HINGE, 0.01).unwrap();
        let mesh = body.mesh();
        let mut out = KernelMesh {
            vertices: mesh.vertices().to_vec(),
            ..KernelMesh::default()
        };
        for face in body.topology().faces() {
            out.face_triangles.push(face.triangles.len());
            for &t in &face.triangles {
                out.indices
                    .extend(mesh.triangles()[t as usize].map(|i| i as usize));
            }
        }
        out
    }

    #[test]
    fn another_kernel_faces_the_same_checks() {
        let soup = hinge_from_another_kernel();
        let (_, report) = load_step_with(HINGE, 0.01, "test", |_, _| Ok(soup)).unwrap();
        assert_eq!(report.check, FileCheck::Passed);
    }

    /// Both unit-error directions, against a real part: a mesh 25.4 times too small
    /// falls short of the file's vertices, and one 25.4 times too large leaves the
    /// file's geometry. Neither may pass.
    #[test]
    fn a_mesh_at_the_wrong_scale_is_refused_either_way() {
        let scaled = |k: f64| {
            let mut soup = hinge_from_another_kernel();
            soup.vertices.iter_mut().for_each(|v| *v *= k);
            load_step_with(HINGE, 0.01, "test", |_, _| Ok(soup))
        };
        assert!(matches!(
            scaled(1.0 / 25.4),
            Err(StepError::Check {
                source: Problem::ShortOfVertices { .. },
                ..
            })
        ));
        assert!(matches!(
            scaled(25.4),
            Err(StepError::Check {
                source: Problem::OutsideFile { .. },
                ..
            })
        ));
    }

    /// A kernel that gives up on a face but still counts it is refused, which
    /// OpenCascade's own refusal made unnecessary to check until now.
    #[test]
    fn a_face_handed_back_empty_is_refused() {
        let mut soup = hinge_from_another_kernel();
        let dropped = soup.face_triangles[3];
        let start: usize = soup.face_triangles[..3].iter().sum();
        soup.indices.drain(start * 3..(start + dropped) * 3);
        soup.face_triangles[3] = 0;
        assert!(matches!(
            load_step_with(HINGE, 0.01, "test", |_, _| Ok(soup)),
            Err(StepError::Check {
                source: Problem::EmptyFace { face: 3 },
                ..
            })
        ));
    }

    #[test]
    fn a_face_left_out_is_refused() {
        let mut soup = hinge_from_another_kernel();
        let last = soup.face_triangles.pop().unwrap();
        soup.indices.truncate(soup.indices.len() - last * 3);
        assert!(matches!(
            load_step_with(HINGE, 0.01, "test", |_, _| Ok(soup)),
            Err(StepError::Check {
                source: Problem::DroppedFaces { .. },
                ..
            })
        ));
    }

    #[test]
    fn a_kernel_that_fails_is_named() {
        let err = load_step_with(HINGE, 0.01, "truck", |_, _| Err("no".into())).unwrap_err();
        assert!(
            err.to_string().contains("truck could not read it (no)"),
            "{err}"
        );
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
