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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use camcraft_core::job::{ToolFieldError, ToolSpec};
use serde::{Deserialize, Serialize};

use crate::fs::write_atomically;

pub mod fusion_csv;

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
    pub fn save(&self, path: &Path) -> Result<(), LibraryError> {
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
        assert_eq!(lib.root.count(), 0);
    }

    /// Saved and read back, the library is the same, and its folder is made for it.
    #[test]
    fn the_library_round_trips() {
        let path = temp("round-trip");
        let mut lib = Library::default();
        lib.root.tools.insert(
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
        lib.root.tools.insert("default".into(), ToolSpec::default());
        let m1 = lib.root.create(&[], "machine_01").unwrap();
        let shank = lib.root.create(&m1, "1-4 shank").unwrap();
        lib.root
            .at_mut(&shank)
            .unwrap()
            .tools
            .insert("default".into(), ToolSpec::default());
        lib.root.create(&[], "empty").unwrap();
        lib.save(&path).unwrap();
        assert_eq!(Library::load(&path).unwrap(), lib);
    }

    /// A library tool drops into a job's tools unchanged: one schema.
    #[test]
    fn a_library_tool_is_a_job_tool() {
        let path = temp("schema");
        let mut lib = Library::default();
        lib.root.tools.insert(
            "fl".into(),
            ToolSpec {
                flutes: Some(3),
                ..ToolSpec::default()
            },
        );
        lib.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let job: Job = toml::from_str(&format!("version = 1\n{text}")).unwrap();
        job.check_common().unwrap();
        assert_eq!(job.tools["fl"], lib.root.tools["fl"]);
    }

    #[test]
    fn a_bad_tool_is_not_saved() {
        let path = temp("bad");
        let mut lib = Library::default();
        let deep = lib.root.create(&[], "m").unwrap();
        lib.root.at_mut(&deep).unwrap().tools.insert(
            "bad".into(),
            ToolSpec {
                flutes: Some(0),
                ..ToolSpec::default()
            },
        );
        assert!(
            matches!(lib.save(&path), Err(LibraryError::BadTool { ref name, .. }) if name == "m/bad")
        );
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

    /// A library written before folders reads as its top folder.
    #[test]
    fn a_library_from_before_folders_reads() {
        let old = "# camcraft tool library.\n\n[tools.flat-6]\nkind = \"flat\"\ndiameter = 6.0\n";
        let lib: Library = toml::from_str(old).unwrap();
        assert_eq!(lib.root.tools.len(), 1);
        assert!(lib.root.folders.is_empty());
    }

    /// A tool from before presets, with its feeds in one `cutting` table, reads
    /// with them as a preset, and is saved that way, in a folder as at the top.
    #[test]
    fn feeds_from_before_presets_read_as_a_preset() {
        let old = "[folders.onefinity.tools.quarter-upcut]\nkind = \"flat\"\n\
                   diameter = 6.35\nrpm = 16000\n\n\
                   [folders.onefinity.tools.quarter-upcut.cutting]\nfeed = 3810.0\n";
        let lib: Library = toml::from_str(old).unwrap();
        let tool = &lib.root.at(&["onefinity".into()]).unwrap().tools["quarter-upcut"];
        assert_eq!(tool.presets.len(), 1);
        assert_eq!(tool.presets[0].name, "Default");
        assert_eq!(tool.presets[0].rpm, Some(16000));
        assert_eq!(tool.presets[0].feed, Some(3810.0));
        let path = temp("presets");
        lib.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("[[folders.onefinity.tools.quarter-upcut.presets]]"),
            "{text}"
        );
        assert!(!text.contains("cutting"), "{text}");
        assert_eq!(Library::load(&path).unwrap(), lib);
    }

    #[test]
    fn folders_are_made_renamed_and_removed() {
        let mut root = Folder::default();
        let a = root.create(&[], "machine_01").unwrap();
        assert_eq!(
            root.create(&[], " machine_01 "),
            Err(FolderError::Taken("machine_01".into()))
        );
        assert_eq!(root.create(&[], "a/b"), Err(FolderError::Slash));
        assert_eq!(root.create(&[], "  "), Err(FolderError::NoName));
        let b = root.create(&a, "ball").unwrap();
        assert_eq!(b, ["machine_01", "ball"]);
        root.at_mut(&b)
            .unwrap()
            .tools
            .insert("t".into(), ToolSpec::default());
        let renamed = root.rename(&a, "machine_02").unwrap();
        assert!(root.at(&["machine_02".into(), "ball".into()]).is_some());
        assert_eq!(root.count(), 1);
        assert_eq!(root.rename(&[], "x"), Err(FolderError::Root));
        let gone = root.remove(&renamed).unwrap();
        assert_eq!(gone.count(), 1);
        assert_eq!(root.count(), 0);
        assert_eq!(root.folder_paths(), vec![Vec::<String>::new()]);
    }

    /// A tool moved onto a name that is taken gets a free one.
    #[test]
    fn tools_move_between_folders() {
        let mut root = Folder::default();
        let a = root.create(&[], "a").unwrap();
        root.tools.insert("t".into(), ToolSpec::default());
        root.at_mut(&a)
            .unwrap()
            .tools
            .insert("t".into(), ToolSpec::default());
        assert_eq!(root.move_tool(&[], "t", &a).unwrap(), "t-2");
        assert!(root.tools.is_empty());
        assert_eq!(root.at(&a).unwrap().tools.len(), 2);
        assert!(root.move_tool(&[], "nope", &a).is_err());
        assert!(root.move_tool(&a, "t", &["nowhere".into()]).is_err());
        assert_eq!(
            root.at(&a).unwrap().tools.len(),
            2,
            "a failed move keeps the tool"
        );
        let walked: Vec<String> = root
            .walk()
            .iter()
            .map(|(p, n, _)| format!("{}/{n}", folder_label(p)))
            .collect();
        assert_eq!(walked, ["a/t", "a/t-2"]);
    }
}
