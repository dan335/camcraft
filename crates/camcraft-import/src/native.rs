//! Shapes that are ogeom's already: a solid another program holds in memory, or
//! one written in ogeom's native format.
//!
//! A program built on ogeom has its bodies as ogeom shapes, not as files. They
//! are tessellated here the way a STEP file's solids are ([`crate::step`]): every
//! face meshed to within the tolerance and measured against its surface, and the
//! faces welded into one [`Body`] whose faces are the shape's own, in ogeom's order
//! of faces, so a face keeps its number when the tolerance changes.
//!
//! The shapes are taken to be in millimetres and where they are meant to stand:
//! there is no file here to read a unit or a placement from. Nor is there a file
//! to check the mesh against, as a STEP import is checked against its text
//! ([`step_check`]): the shape is the kernel's own, not something read from a
//! file another kernel wrote.

use camcraft_geom::topology::Body;
use ogeom_topo::{Model, Shape};

use crate::step::{WeldError, Welded, mesh_bodies, weld_faces};

/// A shape that could not be turned into a trustworthy mesh.
#[derive(Debug, thiserror::Error)]
pub enum NativeError {
    /// The text is not ogeom's native format, or not a version this reads.
    #[error("the shape does not read: {0}")]
    Read(String),
    /// A face would not mesh, or lies further off its surface than asked.
    #[error("the shape would not mesh: {0}")]
    Mesh(String),
    /// The triangles could not be made one body.
    #[error("the shape's triangles are not a usable body: {0}")]
    Body(String),
    /// Nothing was left to machine.
    #[error("the shape produced no triangles")]
    Empty,
    /// The requested tessellation tolerance was not usable.
    #[error("tessellation tolerance must be finite and greater than zero, got {0}")]
    BadTolerance(f64),
}

/// What loading found, beyond the body itself.
#[derive(Debug, Clone)]
pub struct NativeReport {
    /// Faces the shapes have.
    pub faces: u64,
    /// Triangles the kernel produced.
    pub triangles: u64,
    /// Triangles dropped because corners coincided.
    pub degenerate: u64,
    /// Vertices after welding.
    pub vertices: u64,
}

/// Tessellate `shapes` of `model` to within `tolerance` millimetres as one body,
/// keeping the face each triangle came from.
///
/// The shapes are solids, shells or faces, in millimetres. Their faces are
/// numbered in the order given, each shape's in ogeom's order of its faces.
pub fn load_shapes(
    model: &Model,
    shapes: &[Shape],
    tolerance: f64,
) -> Result<(Body, NativeReport), NativeError> {
    if !(tolerance.is_finite() && tolerance > 0.0) {
        return Err(NativeError::BadTolerance(tolerance));
    }
    let soup =
        mesh_bodies(model, shapes, tolerance).map_err(|e| NativeError::Mesh(e.to_string()))?;
    let faces = soup.face_triangles.len() as u64;
    let Welded {
        body,
        triangles,
        degenerate,
        vertices,
    } = weld_faces(&soup).map_err(|e| match e {
        WeldError::Faces(detail) => NativeError::Body(detail),
        WeldError::Empty => NativeError::Empty,
        WeldError::Mesh(source) => NativeError::Body(source.to_string()),
    })?;
    Ok((
        body,
        NativeReport {
            faces,
            triangles,
            degenerate,
            vertices,
        },
    ))
}

/// Read shapes written in ogeom's native format (`ogeom_io::native::write`) and
/// tessellate them as [`load_shapes`] does.
pub fn load_native(text: &str, tolerance: f64) -> Result<(Body, NativeReport), NativeError> {
    if !(tolerance.is_finite() && tolerance > 0.0) {
        return Err(NativeError::BadTolerance(tolerance));
    }
    let (model, roots) =
        ogeom_io::native::read(text).map_err(|e| NativeError::Read(e.to_string()))?;
    load_shapes(&model, &roots, tolerance)
}
