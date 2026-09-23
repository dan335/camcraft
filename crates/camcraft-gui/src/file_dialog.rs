//! The app's one file dialog.
//!
//! `egui-file-dialog`, as in nodemangler: drawn inside egui, so it follows the
//! theme and never blocks the frame loop. The cost is that a dialog is not a
//! function that returns a path. A click can only *start* one; the request is
//! parked in the dialog as user data, and a later frame's [`AppFileDialog::update`]
//! hands the path back with the request it answers.

use std::path::{Path, PathBuf};

use eframe::egui;
use egui_file_dialog::{DialogState, FileDialog, FileDialogStorage, Filter};

const JOBS: &str = "Jobs (.camcraft)";

/// What a pick is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// A job file.
    Job,
}

/// The dialog and whatever it is currently answering.
pub struct AppFileDialog {
    dialog: FileDialog,
}

/// Whether `path` has one of `extensions`, ignoring case: Fusion writes `.STEP`,
/// and the dialog's own extension filter is case-sensitive.
fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| extensions.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

impl AppFileDialog {
    /// A dialog, remembering where the last one was.
    pub fn new(storage: Option<FileDialogStorage>) -> Self {
        let dialog = FileDialog::new().add_file_filter(
            JOBS,
            Filter::new(|p: &Path| has_extension(p, &["camcraft"])),
        );
        let dialog = match storage {
            Some(s) => dialog.storage(s),
            None => dialog,
        };
        Self { dialog }
    }

    /// Start picking a file for `pick`.
    pub fn open(&mut self, pick: Pick) {
        let config = self.dialog.config_mut();
        let (title, filter) = match pick {
            Pick::Job => ("Open job", JOBS),
        };
        config.title = Some(title.to_owned());
        config.default_file_filter = Some(filter.to_owned());
        self.dialog.set_user_data(pick);
        self.dialog.pick_file();
    }

    /// Whether the dialog is showing.
    pub fn is_open(&self) -> bool {
        matches!(self.dialog.state(), DialogState::Open)
    }

    /// Draw the dialog, and hand back a finished pick with what it was for.
    pub fn update(&mut self, ctx: &egui::Context) -> Option<(Pick, PathBuf)> {
        self.dialog.update(ctx);
        let path = self.dialog.take_picked()?;
        let pick = *self.dialog.user_data::<Pick>()?;
        Some((pick, path))
    }

    /// What to remember between runs of the app.
    pub fn storage(&mut self) -> &FileDialogStorage {
        self.dialog.storage_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_match_regardless_of_case() {
        assert!(has_extension(
            Path::new("jig.STEP"),
            &["step", "stp", "stl"]
        ));
        assert!(has_extension(Path::new("jig.stp"), &["step", "stp", "stl"]));
        assert!(has_extension(Path::new("rail-jig.camcraft"), &["camcraft"]));
        assert!(!has_extension(Path::new("jig.step.bak"), &["step"]));
        assert!(!has_extension(Path::new("README"), &["step"]));
    }
}
