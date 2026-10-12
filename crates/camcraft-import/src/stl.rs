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
use glam::dvec3;
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
    let io = |source| ImportError::Io {
        path: name.clone(),
        source,
    };
    let mut file = std::io::Cursor::new(unambiguous(bytes));
    // Triangle by triangle, straight into the welder: the reader's own indexed
    // mesh would weld every corner once more for nothing.
    let stl = stl_io::create_stl_reader(&mut file).map_err(io)?;

    let scale = units.to_mm();
    let mut report = ImportReport::default();
    let mut weld = Welder::default();

    for face in stl {
        let face = face.map_err(io)?;
        report.triangles += 1;
        let corners = face.vertices.map(|v| {
            let [x, y, z] = v.0;
            dvec3(
                f64::from(x) * scale,
                f64::from(y) * scale,
                f64::from(z) * scale,
            )
        });
        // A non-finite corner is dropped and counted by the welder.
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
mod tests;
