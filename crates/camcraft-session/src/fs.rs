//! Files written so that a crash or a full disk never leaves half of one.

use std::path::Path;

/// Write `text` beside `path` and move it into place.
pub fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".camcraft-saving");
    let temp = std::path::PathBuf::from(temp);
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, path)
}
