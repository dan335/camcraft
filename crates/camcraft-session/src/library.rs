//! The per-user tool library.
//!
//! One file of tools, grouped in folders that nest (one per machine, say, and
//! within it one per shank size). A tool is the same `ToolSpec` a job keeps, so
//! choosing one for an operation copies it into the job literally. Jobs keep
//! copies rather than links, as in Fusion: a job still cuts exactly what was
//! verified after the library changes.
//!
//! The file is `[tools.<name>]` for tools at the top and
//! `[folders.<folder>.tools.<name>]` below, folder within folder. A library from
//! before folders is a file of top-level tools, and reads as one.
//!
//! It lives in the user's app-data folder (`~/Library/Application Support/camcraft/
//! tools.toml` on macOS), or wherever `CAMCRAFT_TOOL_LIBRARY` points.
//!
//! Beside the user's own tools the library shows the [`system`] library, a folder
//! of common bits that is built in, cannot be changed, and is never saved.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use camcraft_core::job::{ToolFieldError, ToolSpec};
use serde::{Deserialize, Serialize};

use crate::fs::write_atomically;

pub mod fusion_csv;
pub mod system;

/// Millimetres in an inch, as tool makers' tables give sizes.
const MM_PER_INCH: f64 = 25.4;

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
        /// Which tool, with its folders: `machine_01/6-ball`.
        name: String,
        /// Why.
        source: ToolFieldError,
    },
}

/// Why a change to the library's folders was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FolderError {
    /// No folder there.
    #[error("there is no folder {0}")]
    NoFolder(String),
    /// No tool there.
    #[error("there is no tool '{0}' there")]
    NoTool(String),
    /// Something of that name is there already.
    #[error("there is already a folder called '{0}' there")]
    Taken(String),
    /// A name that is empty or only spaces.
    #[error("a folder needs a name")]
    NoName,
    /// A name with a `/`, which is how folders are written apart.
    #[error("a folder's name cannot have a '/' in it")]
    Slash,
    /// The top of the library is not a folder that can be renamed or removed.
    #[error("the top of the library cannot be renamed or removed")]
    Root,
}

/// Where a folder is: its name and each above it, outermost first. Empty is the
/// top of the library.
pub type FolderPath = Vec<String>;

/// How a person reads a folder path: `machine_01/1-4 shank`, or `Library` for the
/// top.
pub fn folder_label(path: &[String]) -> String {
    if path.is_empty() {
        "Library".into()
    } else {
        path.join("/")
    }
}

/// Tools, and folders of more.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Folder {
    /// The tools here, by name; names are unique within a folder.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tools: BTreeMap<String, ToolSpec>,
    /// The folders here, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub folders: BTreeMap<String, Folder>,
}

impl Folder {
    /// The folder at `path` below this one.
    pub fn at(&self, path: &[String]) -> Option<&Folder> {
        path.iter().try_fold(self, |f, name| f.folders.get(name))
    }

    /// The folder at `path` below this one, to change.
    pub fn at_mut(&mut self, path: &[String]) -> Option<&mut Folder> {
        path.iter()
            .try_fold(self, |f, name| f.folders.get_mut(name))
    }

    fn at_or_err(&mut self, path: &[String]) -> Result<&mut Folder, FolderError> {
        self.at_mut(path)
            .ok_or_else(|| FolderError::NoFolder(folder_label(path)))
    }

    /// Make a folder called `name` in the one at `parent`, and say where it is.
    pub fn create(&mut self, parent: &[String], name: &str) -> Result<FolderPath, FolderError> {
        let name = check_name(name)?;
        let folder = self.at_or_err(parent)?;
        if folder.folders.contains_key(&name) {
            return Err(FolderError::Taken(name));
        }
        folder.folders.insert(name.clone(), Folder::default());
        let mut path = parent.to_vec();
        path.push(name);
        Ok(path)
    }

    /// Rename the folder at `path`, and say where it is now.
    pub fn rename(&mut self, path: &[String], to: &str) -> Result<FolderPath, FolderError> {
        let to = check_name(to)?;
        let (name, parent) = path.split_last().ok_or(FolderError::Root)?;
        if *name == to {
            return Ok(path.to_vec());
        }
        let folder = self.at_or_err(parent)?;
        if folder.folders.contains_key(&to) {
            return Err(FolderError::Taken(to));
        }
        let moved = folder
            .folders
            .remove(name)
            .ok_or_else(|| FolderError::NoFolder(folder_label(path)))?;
        folder.folders.insert(to.clone(), moved);
        let mut path = parent.to_vec();
        path.push(to);
        Ok(path)
    }

    /// Take the folder at `path` out, with everything in it.
    pub fn remove(&mut self, path: &[String]) -> Result<Folder, FolderError> {
        let (name, parent) = path.split_last().ok_or(FolderError::Root)?;
        self.at_or_err(parent)?
            .folders
            .remove(name)
            .ok_or_else(|| FolderError::NoFolder(folder_label(path)))
    }

    /// Move tool `name` from the folder at `from` to the one at `to`, under a free
    /// name there, and say what it is called now.
    pub fn move_tool(
        &mut self,
        from: &[String],
        name: &str,
        to: &[String],
    ) -> Result<String, FolderError> {
        if from == to {
            return Ok(name.to_string());
        }
        self.at_or_err(to)?;
        let tool = self
            .at_or_err(from)?
            .tools
            .remove(name)
            .ok_or_else(|| FolderError::NoTool(name.to_string()))?;
        let target = self.at_or_err(to)?;
        let as_name = free_name(name, &target.tools);
        target.tools.insert(as_name.clone(), tool);
        Ok(as_name)
    }

    /// Every tool here and below, with its folder, folder by folder, outermost
    /// first.
    pub fn walk(&self) -> Vec<(FolderPath, &str, &ToolSpec)> {
        let mut out = Vec::new();
        self.walk_into(&mut Vec::new(), &mut out);
        out
    }

    fn walk_into<'a>(
        &'a self,
        path: &mut FolderPath,
        out: &mut Vec<(FolderPath, &'a str, &'a ToolSpec)>,
    ) {
        for (name, tool) in &self.tools {
            out.push((path.clone(), name.as_str(), tool));
        }
        for (name, folder) in &self.folders {
            path.push(name.clone());
            folder.walk_into(path, out);
            path.pop();
        }
    }

    /// The tools here, and in every folder below when `below`, whose name or
    /// description has `filter` in it (see [`tool_matches`]): with their folders,
    /// in the order of [`Folder::walk`].
    pub fn find(&self, below: bool, filter: &str) -> Vec<(FolderPath, &str, &ToolSpec)> {
        let tools = if below {
            self.walk()
        } else {
            self.tools
                .iter()
                .map(|(n, t)| (Vec::new(), n.as_str(), t))
                .collect()
        };
        tools
            .into_iter()
            .filter(|(_, n, t)| tool_matches(n, t, filter))
            .collect()
    }

    /// Every folder here and below, this one first.
    pub fn folder_paths(&self) -> Vec<FolderPath> {
        let mut out = vec![Vec::new()];
        for (name, folder) in &self.folders {
            for mut p in folder.folder_paths() {
                p.insert(0, name.clone());
                out.push(p);
            }
        }
        out
    }

    /// How many tools here and below.
    pub fn count(&self) -> usize {
        self.tools.len() + self.folders.values().map(Folder::count).sum::<usize>()
    }
}

/// Whether a tool called `name` is one a person filtering by `filter` wants: the
/// filter is in its name or its description, ignoring case. An empty filter wants
/// every tool.
pub fn tool_matches(name: &str, tool: &ToolSpec, filter: &str) -> bool {
    let filter = filter.to_lowercase();
    filter.is_empty()
        || name.to_lowercase().contains(&filter)
        || tool.comment.to_lowercase().contains(&filter)
}

fn check_name(name: &str) -> Result<String, FolderError> {
    let name = name.trim();
    if name.is_empty() {
        Err(FolderError::NoName)
    } else if name.contains('/') {
        Err(FolderError::Slash)
    } else {
        Ok(name.to_string())
    }
}

/// The library: its top folder.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Library {
    /// The top of the library, which is a folder like any other but has no name.
    pub root: Folder,
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
    /// The System library, being built in, is left out.
    pub fn save(&self, path: &Path) -> Result<(), LibraryError> {
        let mut own = self.clone();
        own.root.folders.remove(system::SYSTEM);
        own.write(path)
    }

    fn write(&self, path: &Path) -> Result<(), LibraryError> {
        for (folder, name, tool) in self.root.walk() {
            if let Some(source) = tool.check().into_iter().next() {
                let mut at = folder;
                at.push(name.to_string());
                return Err(LibraryError::BadTool {
                    name: at.join("/"),
                    source,
                });
            }
        }
        let body = toml::to_string(self)?;
        let text = format!(
            "# camcraft tool library. Written by the tool manager. A tool here is the\n\
             # same [tools.<name>] a job keeps; folders are [folders.<name>].\n\n{body}"
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
mod tests;
