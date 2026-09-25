//! What the command-line tests share: fixtures, scratch folders, and the binary.

// Each test file uses its own part of this, and the rest would be dead to it.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A STEP file from the repository's fixtures, as an argument.
pub fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/step")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

/// A fresh directory per test, under the target directory Cargo provides.
pub fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run the binary with `args`, whatever its exit status.
pub fn camcraft(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_camcraft"))
        .args(args)
        .output()
        .unwrap()
}
