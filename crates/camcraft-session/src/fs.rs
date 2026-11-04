//! Files: where a job is and what it is called, where its models are, and
//! writing so that a crash or a full disk never leaves half a file.
//!
//! A job names its models by paths relative to its own folder, with `/` between
//! folders on every system, so a job and its models can move together.

use std::path::{Component, Path, PathBuf};

/// Where a file is written before it is moved to `path`: beside it, so the
/// move is a rename within one folder, named for what is `doing` it.
pub(crate) fn beside(path: &Path, doing: &str) -> PathBuf {
    let mut temp = path.as_os_str().to_owned();
    temp.push(format!(".camcraft-{doing}"));
    PathBuf::from(temp)
}

/// Write `text` beside `path` and move it into place; on failure, nothing is
/// left beside it.
pub fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    let temp = beside(path, "saving");
    let written = std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written
}

/// Where a path a job gives is, from the job's folder. An absolute path is where
/// it says.
pub fn resolve(job_dir: &Path, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        job_dir.join(p)
    }
}

/// The folder a job's relative paths start from: the job file's own, or the
/// current folder for a bare file name.
pub fn job_dir(path: &Path) -> PathBuf {
    path.parent()
        .filter(|d| !d.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// `path` with the job extension, if it has none.
pub fn with_job_extension(path: &Path) -> PathBuf {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("camcraft"))
    {
        path.to_path_buf()
    } else {
        let mut p = path.as_os_str().to_owned();
        p.push(".camcraft");
        PathBuf::from(p)
    }
}

/// A path as the file system has it, links followed, when it exists; otherwise
/// its folder's, with its name on the end; otherwise as given. Two paths to
/// the same file, however written (`..` in them, another case, `\\?\` before
/// them), are the same path here: what [`relative_path`] compares.
pub fn real(path: &Path) -> PathBuf {
    if let Ok(p) = path.canonicalize() {
        return p;
    }
    match (path.parent(), path.file_name()) {
        (Some(dir), Some(name)) => dir
            .canonicalize()
            .map_or_else(|_| path.to_path_buf(), |d| d.join(name)),
        _ => path.to_path_buf(),
    }
}

/// How a job in `from_dir` names `to`: relative, with `/` between folders, or
/// absolute when the two share no root (another drive), as a person writes it
/// ([`plain`]).
pub fn relative_path(from_dir: &Path, to: &Path) -> String {
    let (from, target) = (real(from_dir), real(to));
    let a: Vec<Component<'_>> = from.components().collect();
    let b: Vec<Component<'_>> = target.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    // Nothing in common but the root, or not even that: not worth walking up to.
    let shared = a
        .iter()
        .take(common)
        .any(|c| matches!(c, Component::Normal(_)));
    if !shared {
        return plain(&target).display().to_string();
    }
    let mut parts: Vec<String> = Vec::new();
    for _ in common..a.len() {
        parts.push("..".into());
    }
    for c in b.iter().skip(common) {
        parts.push(c.as_os_str().to_string_lossy().into_owned());
    }
    parts.join("/")
}

/// `path`, which [`real`] gave, as a person writes it. On Windows a path the
/// file system has resolved starts `\\?\` (`\\?\C:\...`, `\\?\UNC\server\...`),
/// which tells Windows to take it exactly as written; without it (`C:\...`,
/// `\\server\...`) it is the path everything else expects. It is written
/// without it only when that names the very same file, as [`real`] finds it:
/// a path too long, or with a name Windows treats specially, needs it, and
/// keeps it.
fn plain(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::path::Prefix;
        let mut parts = path.components();
        if let Some(Component::Prefix(prefix)) = parts.next() {
            let head = match prefix.kind() {
                Prefix::VerbatimDisk(drive) => Some(format!("{}:", char::from(drive))),
                Prefix::VerbatimUNC(server, share) => Some(format!(
                    r"\\{}\{}",
                    server.to_string_lossy(),
                    share.to_string_lossy()
                )),
                _ => None,
            };
            if let Some(head) = head {
                let mut written = std::ffi::OsString::from(head);
                written.push(parts.as_path().as_os_str());
                let written = PathBuf::from(written);
                if real(&written) == real(path) {
                    return written;
                }
            }
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("camcraft-fs-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::write(dir.join("models/m.step"), "").unwrap();
        dir
    }

    #[test]
    fn paths_are_relative_to_the_job() {
        let root = temp("rel");
        assert_eq!(
            relative_path(&root.join("a"), &root.join("models/m.step")),
            "../models/m.step"
        );
        assert_eq!(
            relative_path(&root, &root.join("models/m.step")),
            "models/m.step"
        );
        assert_eq!(
            resolve(&root.join("a"), "../models/m.step")
                .canonicalize()
                .unwrap(),
            root.join("models/m.step").canonicalize().unwrap()
        );
    }

    /// A model on another drive is named whole, as a person writes it: not with
    /// the `\\?\` Windows puts before a path it has resolved, and still the
    /// same file.
    #[cfg(windows)]
    #[test]
    fn a_path_on_another_drive_is_written_plainly() {
        let root = temp("plain");
        let file = root.join("models/m.step");
        let canonical = file.canonicalize().unwrap();
        assert!(canonical.to_string_lossy().starts_with(r"\\?\"));
        let written = plain(&canonical);
        assert!(
            !written.to_string_lossy().starts_with(r"\\?\"),
            "{written:?}"
        );
        assert_eq!(real(&written), canonical);
        // From a folder on a drive it shares nothing with.
        let named = relative_path(Path::new(r"Z:\camcraft-nowhere"), &file);
        assert!(!named.starts_with(r"\\?\"), "{named}");
        assert!(Path::new(&named).is_absolute(), "{named}");
        assert_eq!(real(Path::new(&named)), canonical);
        // A path Windows did not resolve is left as it was.
        assert_eq!(plain(Path::new(r"C:\x\y")), Path::new(r"C:\x\y"));
    }

    #[test]
    fn a_job_names_its_folder_and_extension() {
        assert_eq!(job_dir(Path::new("part.camcraft")), Path::new("."));
        assert_eq!(job_dir(Path::new("jobs/part.camcraft")), Path::new("jobs"));
        assert_eq!(
            with_job_extension(Path::new("part")),
            Path::new("part.camcraft")
        );
        assert_eq!(
            with_job_extension(Path::new("part.CAMCRAFT")),
            Path::new("part.CAMCRAFT")
        );
    }
}
