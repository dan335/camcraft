//! The per-user tool library.
//!
//! One file of tools, in the same `[tools.<name>]` schema as a job, so copying a
//! tool between the library and a job is literal. Jobs keep copies rather than
//! links, as in Fusion: a job still cuts exactly what was verified after the
//! library changes.
//!
//! It lives in the user's app-data folder (`~/Library/Application Support/camcraft/
//! tools.toml` on macOS), or wherever `CAMCRAFT_TOOL_LIBRARY` points. It is never
//! written except by an explicit save.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use camcraft_core::job::{ToolFieldError, ToolSpec};
use serde::{Deserialize, Serialize};

use crate::fs::write_atomically;

/// Overrides where the library lives.
pub const LIBRARY_ENV: &str = "CAMCRAFT_TOOL_LIBRARY";

/// A library that could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    /// There is nowhere to keep it: no home or app-data folder.
    #[error("there is no app-data folder to keep the tool library in; set {LIBRARY_ENV}")]
    NoHome,
    /// The file could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// Which file.
        path: String,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not a tool library.
    #[error("{path} is not a tool library: {source}")]
    Parse {
        /// Which file.
        path: String,
        /// Why.
        source: toml::de::Error,
    },
    /// A tool could not be written.
    #[error("cannot write the tool library: {0}")]
    Write(#[from] toml::ser::Error),
    /// A tool is not physically meaningful; the library does not keep it.
    #[error("tool '{name}', {source}")]
    BadTool {
        /// Which tool.
        name: String,
        /// Why.
        source: ToolFieldError,
    },
}

/// The tools, by name.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct Library {
    /// Every tool in the library.
    #[serde(default)]
    pub tools: BTreeMap<String, ToolSpec>,
}

/// Where the library lives.
pub fn library_path() -> Result<PathBuf, LibraryError> {
    if let Some(p) = std::env::var_os(LIBRARY_ENV) {
        return Ok(PathBuf::from(p));
    }
    directories::ProjectDirs::from("", "", "camcraft")
        .map(|d| d.data_dir().join("tools.toml"))
        .ok_or(LibraryError::NoHome)
}

impl Library {
    /// Read the library at `path`. A library that does not exist yet is empty.
    pub fn load(path: &Path) -> Result<Self, LibraryError> {
        let name = path.display().to_string();
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => return Err(LibraryError::Io { path: name, source }),
        };
        toml::from_str(&text).map_err(|source| LibraryError::Parse { path: name, source })
    }

    /// Write the library to `path`, making its folder if need be. Refuses a tool
    /// that does not check clean, so the library never holds one a job would refuse.
    pub fn save(&self, path: &Path) -> Result<(), LibraryError> {
        for (name, tool) in &self.tools {
            if let Some(source) = tool.check().into_iter().next() {
                return Err(LibraryError::BadTool {
                    name: name.clone(),
                    source,
                });
            }
        }
        let body = toml::to_string(self)?;
        let text = format!(
            "# camcraft tool library. Written by the tool manager; the same [tools.<name>]\n\
             # schema as a job, so tools copy between the two as they are.\n\n{body}"
        );
        let io = |source| LibraryError::Io {
            path: path.display().to_string(),
            source,
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        write_atomically(path, &text).map_err(io)
    }
}

/// A name not yet in `taken`: `base`, or `base-2`, `base-3` and so on.
pub fn free_name(base: &str, taken: &BTreeMap<String, ToolSpec>) -> String {
    let base = if base.trim().is_empty() {
        "tool"
    } else {
        base.trim()
    };
    if !taken.contains_key(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|n| !taken.contains_key(n))
        .unwrap_or_else(|| base.to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use camcraft_core::job::{Job, ToolKind};

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("camcraft-library-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("nested").join("tools.toml")
    }

    #[test]
    fn a_missing_library_is_empty_not_an_error() {
        let lib = Library::load(&temp("missing")).unwrap();
        assert!(lib.tools.is_empty());
    }

    /// Saved and read back, the library is the same, and its folder is made for it.
    #[test]
    fn the_library_round_trips() {
        let path = temp("round-trip");
        let mut lib = Library::default();
        lib.tools.insert(
            "6-ball".into(),
            ToolSpec {
                kind: ToolKind::Ball,
                diameter: 6.0,
                flutes: Some(2),
                stickout: Some(25.0),
                vendor: Some("Amana".into()),
                ..ToolSpec::default()
            },
        );
        lib.tools.insert("default".into(), ToolSpec::default());
        lib.save(&path).unwrap();
        assert_eq!(Library::load(&path).unwrap(), lib);
    }

    /// A library tool drops into a job's tool table unchanged: one schema.
    #[test]
    fn a_library_tool_is_a_job_tool() {
        let path = temp("schema");
        let mut lib = Library::default();
        lib.tools.insert(
            "fl".into(),
            ToolSpec {
                flutes: Some(3),
                ..ToolSpec::default()
            },
        );
        lib.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let job = format!(
            "{text}\n[setup]\nstock = {{ kind = \"box\" }}\n[[operation]]\nname = \"f\"\n\
             strategy = \"parallel\"\ntool = \"fl\"\n"
        );
        let job: Job = toml::from_str(&job).unwrap();
        job.validate().unwrap();
        assert_eq!(job.tools["fl"], lib.tools["fl"]);
    }

    #[test]
    fn a_bad_tool_is_not_saved() {
        let path = temp("bad");
        let mut lib = Library::default();
        lib.tools.insert(
            "bad".into(),
            ToolSpec {
                flutes: Some(0),
                ..ToolSpec::default()
            },
        );
        assert!(matches!(lib.save(&path), Err(LibraryError::BadTool { .. })));
        assert!(!path.exists());
    }

    #[test]
    fn free_names_count_up() {
        let mut taken = BTreeMap::new();
        assert_eq!(free_name("ball", &taken), "ball");
        taken.insert("ball".into(), ToolSpec::default());
        taken.insert("ball-2".into(), ToolSpec::default());
        assert_eq!(free_name("ball", &taken), "ball-3");
        assert_eq!(free_name("  ", &taken), "tool");
    }
}
