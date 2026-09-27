//! Files: where a job is and what it is called, where its models are, and
//! writing so that a crash or a full disk never leaves half a file.
//!
//! A job names its models by paths relative to its own folder, with `/` between
//! folders on every system, so a job and its models can move together.

use std::path::{Component, Path, PathBuf};

/// Write `text` beside `path` and move it into place.
pub fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".camcraft-saving");
    let temp = std::path::PathBuf::from(temp);
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, path)
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
/// its folder's, with its name on the end; otherwise as given.
fn real(path: &Path) -> PathBuf {
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
/// absolute when the two share no root (another drive).
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
        return target.display().to_string();
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
