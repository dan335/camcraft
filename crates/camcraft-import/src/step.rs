//! STEP import, through ogeom.
//!
//! The kernel reads the B-rep, converts it to millimetres and tessellates it. None
//! of that is taken on trust, because a STEP importer fails in ways that look fine:
//! a face that did not transfer is a hole the cutter drops into, and a unit read
//! wrongly is a part 25.4 times the wrong size. So the mesh is checked against the
//! file it came from, using facts read from the file's text independently of the
//! kernel (see [`step_check::part21`]):
//!
//! The checks are [`step_check`]'s, kept free of camcraft so any importer can use them:
//!
//! - **Every face arrives, with triangles.** The file's face count against the
//!   kernel's, and then every face the kernel gave must have at least one triangle.
//!   [`ogeom`] refuses a face it cannot triangulate, but a kernel that hands one
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
//! - **Every face lies on its surface.** A face on a plane, cylinder, cone, sphere or
//!   torus must lie within the job's tolerance of it (four times the tessellation
//!   tolerance, which is a quarter of the job's), and every such face of the file
//!   must have some of the mesh on it. This sees surface wrong inside the part,
//!   which the boxes cannot. A face on a B-spline is held to its box alone.
//!
//! The two geometric checks are made where the file's assembly puts each component
//! ([`step_check::part21`] follows its placements). Only geometry placed in a way that
//! cannot be followed, such as a mapped item, leaves them reported as skipped rather
//! than passed.
//!
//! Another kernel's mesh can be put through the same checks with
//! [`load_step_with`]. With the `opencascade` feature, [`opencascade()`] is the
//! kernel camcraft used before ogeom, kept to cross-check it against.
//!
//! # Faces
//!
//! Every triangle keeps the B-rep face it was tessellated from, numbered in the
//! order the kernel explores the shape's faces, so the result is a [`Body`] whose
//! faces are the file's own. That order depends only on the file, not on the
//! tessellation, so a face keeps its number when the tolerance changes.

use std::path::Path;
#[cfg(feature = "opencascade")]
use std::sync::{Mutex, PoisonError};

use camcraft_geom::mesh::MeshError;
use camcraft_geom::topology::{Body, TopologyError};
use glam::{DVec3, dvec3};
use ogeom_core::Tolerances;
use ogeom_geom::{Surface, SurfaceKind, project_on_surface, project_on_surface_from};
use ogeom_math::Point;
use ogeom_mesh::{Deflection, face_meshes_for};
use ogeom_topo::{Filter, Model, NodeData, Shape, ShapeType, Triangulation, explore};
use step_check::Problem;
use step_check::part21::{self, LengthUnit, Part21Error};
pub use step_check::{FileCheck, KernelMesh};

use crate::weld::Welder;

/// The kernel's tolerances for a model in millimetres, which is what ogeom builds
/// whatever the file's unit.
const MILLIMETRES: Tolerances = Tolerances::millimetres();

/// ogeom's chord, as a part of the tolerance asked for. The chord bounds how far
/// an edge's polyline strays from its curve, but a face's triangles can stand
/// further off a doubly curved surface: step-conformance measured ogeom 0.10.0 up
/// to 3.1 times its chord off the freeform panel's exact shape. Meshed at a third,
/// every part there and the private sculpted panel landed within 1.04 times the
/// tolerance, where OpenCascade reaches 1.3 times (3.4 on the bowl). See
/// `docs/decisions/0001-step-backend.md`.
const CHORD_PER_TOLERANCE: f64 = 1.0 / 3.0;

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
    load_step_with(path, tolerance, "ogeom", |path, _facts| {
        ogeom(path, tolerance)
    })
}

/// Read the STEP file at `path` with ogeom and tessellate it to within `tolerance`
/// millimetres, face by face in ogeom's order of faces, for [`load_step_with`].
///
/// ogeom converts the file to millimetres as it reads it, and places every part of
/// an assembly where the file's product tree puts it. The faces either side of a
/// B-rep edge are drawn to the same points along it, which the weld relies on.
/// A face ogeom will not mesh is refused with ogeom's reason, since a face missing
/// is a hole. A curved face is measured against its surface (see [`stand_off`])
/// and refused if any of it lies further than `tolerance` off, since that is error
/// the simulator cannot see.
pub fn ogeom(path: &Path, tolerance: f64) -> Result<KernelMesh, String> {
    let text = std::fs::read(path).map_err(|e| e.to_string())?;
    let import = ogeom_io::read_step(&String::from_utf8_lossy(&text), MILLIMETRES)
        .map_err(|e| e.to_string())?;
    let document = &import.document;
    let model = document.model();

    // Every part where the product tree places it, then any body the tree does not
    // reach, as the file states it.
    let mut bodies: Vec<Shape> = Vec::new();
    for root in document.roots() {
        for occurrence in document.occurrences_of(root).map_err(|e| e.to_string())? {
            bodies.push(occurrence.shape);
        }
    }
    let mut reached = std::collections::BTreeSet::new();
    for body in &bodies {
        reached.extend(faces_of(model, body)?.iter().map(Shape::node));
    }
    for body in import.solids.iter().chain(&import.shells) {
        if faces_of(model, body)?
            .iter()
            .any(|f| !reached.contains(&f.node()))
        {
            bodies.push(body.clone());
        }
    }

    let deflection = Deflection {
        chord: tolerance * CHORD_PER_TOLERANCE,
        ..Deflection::default()
    };
    let mut out = KernelMesh::default();
    let mut faces_from = Vec::new();
    for body in &bodies {
        let (faces, _) = face_meshes_for(model, body, deflection, None, MILLIMETRES)
            .map_err(|e| e.to_string())?;
        for (shape, piece) in faces {
            let face = out.face_triangles.len();
            let mut mesh = piece.map_err(|e| format!("face {face} would not mesh: {e}"))?;
            let measured = |mesh: &Triangulation| {
                stand_off(model, &shape, mesh, tolerance)
                    .map_err(|e| format!("face {face} could not be measured: {e}"))
            };
            // A ruled face in fewer triangles, where they hold to the surface.
            let fewer = is_ruled(model, &shape)
                .then(|| crate::ruled::from_boundary(&mesh))
                .flatten()
                .map(|triangles| Triangulation {
                    triangles,
                    ..mesh.clone()
                });
            let off = match fewer {
                Some(fewer) => match measured(&fewer)? {
                    off if off <= tolerance => {
                        mesh = fewer;
                        off
                    }
                    _ => measured(&mesh)?,
                },
                None => measured(&mesh)?,
            };
            if off > tolerance {
                return Err(format!(
                    "face {face} lies {off:.4} mm off its surface, more than {tolerance} mm"
                ));
            }
            let base = out.vertices.len();
            out.vertices
                .extend(mesh.positions.iter().map(|p| dvec3(p.x, p.y, p.z)));
            out.indices
                .extend(mesh.triangles.iter().flatten().map(|&i| base + i as usize));
            out.face_triangles.push(mesh.triangles.len());
            faces_from.push(base);
        }
    }
    snap_shared_points(&mut out.vertices, &faces_from);
    Ok(out)
}

/// How far apart two faces' copies of one point can be, in millimetres, and still
/// be taken for the same point. See [`snap_shared_points`].
const SAME_POINT: f64 = 1e-9;

/// Give the faces' copies of a point they share the very same coordinates.
///
/// The weld joins faces only where their points are bit-for-bit equal, and ogeom
/// sometimes works out a B-rep vertex once for each face it bounds: on `sled_base`
/// one face's corners come out 1e-14 mm from its neighbours', which leaves the
/// solid open along that face. So a point within [`SAME_POINT`] of a point of an
/// earlier face takes that point's coordinates. The finest tolerance camcraft
/// meshes to is millions of times that, so no two points of the part that are
/// really distinct are joined; points within one face are left as ogeom gave them.
///
/// `faces_from` is the index of each face's first vertex, in order.
fn snap_shared_points(vertices: &mut [DVec3], faces_from: &[usize]) {
    let cell = |p: DVec3| (p / SAME_POINT).floor().as_i64vec3().to_array();
    let mut seen: std::collections::BTreeMap<[i64; 3], Vec<(DVec3, usize)>> =
        std::collections::BTreeMap::new();
    let mut face = 0;
    for (i, v) in vertices.iter_mut().enumerate() {
        while faces_from.get(face + 1).is_some_and(|&next| next <= i) {
            face += 1;
        }
        let [x, y, z] = cell(*v);
        let earlier = (-1..=1)
            .flat_map(|dx| {
                (-1..=1).flat_map(move |dy| (-1..=1).map(move |dz| [x + dx, y + dy, z + dz]))
            })
            .filter_map(|key| seen.get(&key))
            .flatten()
            .find(|&&(p, f)| f != face && p.distance(*v) <= SAME_POINT);
        match earlier {
            Some(&(p, _)) => *v = p,
            None => seen.entry([x, y, z]).or_default().push((*v, face)),
        }
    }
}

/// How far, at most, `mesh` lies off the surface of `face`, in millimetres: the
/// distance to the surface from every triangle's middle and every side's
/// midpoint, each projected onto the surface from the parameters ogeom gives the
/// triangle's corners. A plane's triangles lie on it exactly. Distances up to
/// `tolerance` are taken from the nearest foot to that guess; one beyond it is
/// looked for over the whole surface before it counts.
///
/// ogeom says whether a face met the chord it was asked for, but measures only the
/// middles, and holds them to that chord rather than to camcraft's tolerance, which
/// is three times it (see [`CHORD_PER_TOLERANCE`]). The sides are measured too, so
/// that a stretch ogeom drew coarser than asked, at a cap on its steps, is seen.
fn stand_off(
    model: &Model,
    face: &Shape,
    mesh: &Triangulation,
    tolerance: f64,
) -> Result<f64, String> {
    let Some(NodeData::Face(data)) = model.node(face).map(|n| n.data()) else {
        return Err("it is not a face of the model".into());
    };
    let Some(surface) = model.geometry().surface(data.surface) else {
        return Err("its surface is not in the model".into());
    };
    if matches!(surface.kind(), SurfaceKind::Plane) {
        return Ok(0.0);
    }
    if mesh.parameters.len() != mesh.positions.len() {
        return Err("its vertices carry no surface parameters".into());
    }
    let placement = face.transform(model.datums()).map_err(|e| e.to_string())?;
    let to_surface = placement.inverse().map_err(|e| e.to_string())?;
    let at = |i: u32| -> Result<(DVec3, (f64, f64)), String> {
        let i = i as usize;
        match (mesh.positions.get(i), mesh.parameters.get(i)) {
            (Some(p), Some(&uv)) => Ok((dvec3(p.x, p.y, p.z), uv)),
            _ => Err(format!(
                "a triangle names vertex {i}, which it does not have"
            )),
        }
    };
    // How far `p` is from the surface, starting from parameters near its foot.
    // From a poor guess, such as beside a cone's apex, where a vertex's `u` is
    // any at all, Newton can stop short of the nearest foot; so a distance over
    // the tolerance is looked for again over the whole surface before it counts.
    let off = |p: DVec3, guess: (f64, f64)| -> Result<f64, String> {
        let target = to_surface.apply(Point::new(p.x, p.y, p.z));
        let distance = |foot: Point| {
            let foot = placement.apply(foot);
            dvec3(foot.x, foot.y, foot.z).distance(p)
        };
        let near = project_on_surface_from(surface, target, guess, MILLIMETRES)
            .map_err(|e| e.to_string())?;
        let d = distance(near.point);
        if d <= tolerance {
            return Ok(d);
        }
        let anywhere =
            project_on_surface(surface, target, 16, MILLIMETRES).map_err(|e| e.to_string())?;
        Ok(d.min(distance(anywhere.point)))
    };
    let mid = |a: (f64, f64), b: (f64, f64)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);

    let mut worst: f64 = 0.0;
    for t in &mesh.triangles {
        let [(a, ua), (b, ub), (c, uc)] = [at(t[0])?, at(t[1])?, at(t[2])?];
        for ((p, up), (q, uq)) in [((a, ua), (b, ub)), ((b, ub), (c, uc)), ((c, uc), (a, ua))] {
            worst = worst.max(off((p + q) / 2.0, mid(up, uq))?);
        }
        let middle = ((ua.0 + ub.0 + uc.0) / 3.0, (ua.1 + ub.1 + uc.1) / 3.0);
        worst = worst.max(off((a + b + c) / 3.0, middle)?);
    }
    Ok(worst)
}

/// Whether `face` lies on a cylinder or a cone, ruled by straight lines: see
/// [`crate::ruled`].
fn is_ruled(model: &Model, face: &Shape) -> bool {
    let Some(NodeData::Face(data)) = model.node(face).map(|n| n.data()) else {
        return false;
    };
    model
        .geometry()
        .surface(data.surface)
        .is_some_and(|s| matches!(s.kind(), SurfaceKind::Cylinder | SurfaceKind::Cone))
}

fn faces_of(model: &Model, shape: &Shape) -> Result<Vec<Shape>, String> {
    explore(model, shape, Filter::OfType(ShapeType::Face)).map_err(|e| e.to_string())
}

/// Serialises every call into OpenCascade. See [`opencascade()`].
#[cfg(feature = "opencascade")]
static OPENCASCADE: Mutex<()> = Mutex::new(());

/// Read the STEP file at `path` with OpenCascade and tessellate it to within
/// `tolerance` millimetres, for [`load_step_with`]: the kernel camcraft used before
/// ogeom, kept to cross-check ogeom against.
#[cfg(feature = "opencascade")]
pub fn opencascade(path: &Path, tolerance: f64) -> Result<KernelMesh, String> {
    // One kernel call at a time, across every thread. OpenCascade's data
    // exchange keeps its settings in process-wide statics, so this does not
    // rely on every caller happening to read on one thread. A read once failed
    // during a full gate run with "failed to read STEP file" on a fixture that
    // reads fine alone; that was never reproduced (0 of 80 runs, with and
    // without this lock), so this is a precaution, not a proven fix. The shape
    // is declared after the guard, so it is dropped while the lock is held.
    let _kernel = OPENCASCADE.lock().unwrap_or_else(PoisonError::into_inner);
    let shape = ::opencascade::primitives::Shape::read_step(path).map_err(|e| e.to_string())?;
    // Refuses outright if any face could not be triangulated.
    let mesh = shape
        .mesh_with_tolerance(tolerance)
        .map_err(|e| e.to_string())?;
    Ok(KernelMesh {
        vertices: mesh.vertices.iter().map(|v| dvec3(v.x, v.y, v.z)).collect(),
        indices: mesh.indices,
        face_triangles: mesh.face_triangles,
    })
}

/// [`load_step`] with the kernel's half supplied: `kernel` reads the file at
/// `path` and tessellates it to within `tolerance` millimetres, given what the
/// file says independently (its unit, for a kernel that ignores units). The result
/// faces the same checks ogeom's does, and comes back as camcraft uses it.
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

    /// ogeom's mesh of the hinge jig, handed back as another kernel would.
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
    /// the kernel's own refusal makes unnecessary to check otherwise.
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

    /// One face's copy of a point a hair from another face's takes its coordinates;
    /// points of one face, and points of two faces further apart, stay as they are.
    #[test]
    fn faces_copies_of_a_point_are_joined() {
        let p = dvec3(1.016, 220.65, 50.8);
        let hair = p + DVec3::X * 1.3e-14;
        let apart = p + DVec3::X * 1e-6;
        // Face 0 is vertices 0 and 1, face 1 is 2 and 3.
        let mut v = vec![p, p + DVec3::Z, hair, apart];
        snap_shared_points(&mut v, &[0, 2]);
        assert_eq!(v, [p, p + DVec3::Z, p, apart]);
        // One face's own points are its own.
        let mut v = vec![p, hair];
        snap_shared_points(&mut v, &[0]);
        assert_eq!(v, [p, hair]);
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
